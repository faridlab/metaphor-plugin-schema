//! Workspace discovery: parse `metaphor.yaml` and locate schema directories.
//!
//! A "workspace" is the directory containing `metaphor.yaml`, which lists every
//! project (apps + modules) and where each lives on disk. Schema-driven generators
//! historically took a `--module-path` pointing at `libs/modules/`, which doesn't
//! match how Metaphor consumer workspaces are laid out. This helper bridges the
//! gap by letting commands resolve a *schema module name* (e.g. `bersihir`,
//! `sapiens`) to the right schema directory regardless of where it lives — and
//! by exposing the project's declared `depends_on` so generators can fan out
//! across module deps in a single command.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One project entry in `metaphor.yaml`.
#[derive(Debug, Clone, Deserialize)]
pub struct Project {
    pub name: String,
    #[serde(default)]
    pub r#type: String,
    pub path: PathBuf,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct MetaphorYaml {
    #[serde(default)]
    projects: Vec<Project>,
}

/// Parsed workspace: root path + project list.
pub struct Workspace {
    pub root: PathBuf,
    projects: Vec<Project>,
    /// schema `module:` field → project name (built lazily on first lookup).
    schema_module_index: BTreeMap<String, String>,
}

impl Workspace {
    /// Walk up from `start` looking for `metaphor.yaml` / `metaphor.yml`.
    pub fn discover(start: &Path) -> Option<PathBuf> {
        let mut current = start.to_path_buf();
        loop {
            for name in ["metaphor.yaml", "metaphor.yml"] {
                if current.join(name).is_file() {
                    return Some(current);
                }
            }
            if !current.pop() {
                return None;
            }
        }
    }

    /// Load the workspace from a directory containing `metaphor.yaml`.
    pub fn load(root: &Path) -> Result<Self> {
        let path = ["metaphor.yaml", "metaphor.yml"]
            .iter()
            .map(|n| root.join(n))
            .find(|p| p.is_file())
            .with_context(|| format!("metaphor.yaml not found at {}", root.display()))?;

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let parsed: MetaphorYaml = serde_yaml::from_str(&content)
            .with_context(|| format!("parsing {}", path.display()))?;

        let mut ws = Workspace {
            root: root.to_path_buf(),
            projects: parsed.projects,
            schema_module_index: BTreeMap::new(),
        };
        ws.build_schema_module_index();
        Ok(ws)
    }

    /// Convenience: discover and load in one shot. Returns `None` if no workspace.
    pub fn from_cwd(cwd: &Path) -> Option<Self> {
        let root = Self::discover(cwd)?;
        Self::load(&root).ok()
    }

    pub fn projects(&self) -> &[Project] {
        &self.projects
    }

    /// Look up a project by `name:` from `metaphor.yaml`.
    pub fn project_by_name(&self, name: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.name == name)
    }

    /// Find the project whose `path:` is the longest ancestor of `cwd`.
    ///
    /// Used to default the `<MODULE>` arg when the user runs a generator from
    /// inside a project directory: e.g. invoking from `apps/bersihir-service/`
    /// (or any subdir like `.../src/foo/`) returns the `bersihir-service`
    /// project. Returns `None` at the workspace root or in unrelated dirs.
    pub fn project_for_cwd(&self, cwd: &Path) -> Option<&Project> {
        let cwd_canon = std::fs::canonicalize(cwd).ok()?;
        let mut best: Option<(&Project, usize)> = None;
        for project in &self.projects {
            let proj_path = self.project_path(project);
            let proj_canon = match std::fs::canonicalize(&proj_path) {
                Ok(p) => p,
                Err(_) => continue,
            };
            if cwd_canon.starts_with(&proj_canon) {
                let depth = proj_canon.components().count();
                if best.map_or(true, |(_, d)| depth > d) {
                    best = Some((project, depth));
                }
            }
        }
        best.map(|(p, _)| p)
    }

    /// Resolve a project's absolute path.
    pub fn project_path(&self, project: &Project) -> PathBuf {
        if project.path.is_absolute() {
            project.path.clone()
        } else {
            self.root.join(&project.path)
        }
    }

    /// Locate a schema directory for the given identifier. Tries, in order:
    /// 1. Match on metaphor.yaml project name → `<project.path>/schema`.
    /// 2. Match on the `module:` field declared inside any project's
    ///    `schema/models/index.model.yaml`.
    pub fn schema_dir_for(&self, identifier: &str) -> Option<PathBuf> {
        // (1) Project name match.
        if let Some(project) = self.project_by_name(identifier) {
            let candidate = self.project_path(project).join("schema");
            if candidate.is_dir() {
                return Some(candidate);
            }
        }

        // (2) Schema-module name match (built once, cached).
        if let Some(project_name) = self.schema_module_index.get(identifier) {
            if let Some(project) = self.project_by_name(project_name) {
                let candidate = self.project_path(project).join("schema");
                if candidate.is_dir() {
                    return Some(candidate);
                }
            }
        }

        None
    }

    /// Resolve a project name → its expected mobileapp/webapp output dir, if the
    /// project's `type:` indicates one. For `mobileapp`, returns
    /// `<path>/shared/src/commonMain/kotlin`.
    pub fn kotlin_output_for_project(&self, name: &str) -> Option<PathBuf> {
        let project = self.project_by_name(name)?;
        let base = self.project_path(project);
        match project.r#type.as_str() {
            "mobileapp" => Some(base.join("shared/src/commonMain/kotlin")),
            _ => None,
        }
    }

    /// Resolve `--output`. If the user passed something that matches a workspace
    /// project name, expand to its conventional kotlin source root. Otherwise
    /// return the user value as-is (caller will join with cwd if relative).
    ///
    /// Also tries `<workspace>/apps/<name>/shared/src/commonMain/kotlin` as a
    /// filesystem fallback — apps that exist on disk but aren't yet declared in
    /// metaphor.yaml still resolve, matching what users intuitively expect.
    pub fn resolve_output(&self, raw: &Path) -> Option<PathBuf> {
        let s = raw.to_str()?;
        // Single-segment string only — multi-segment paths are real paths.
        if s.contains('/') || s.contains('\\') {
            return None;
        }
        if let Some(p) = self.kotlin_output_for_project(s) {
            return Some(p);
        }
        // Filesystem fallback: workspace_root/apps/<name> with mobile layout.
        let candidate = self.root.join("apps").join(s);
        let kotlin_root = candidate.join("shared/src/commonMain/kotlin");
        if kotlin_root.is_dir() {
            return Some(kotlin_root);
        }
        // Some projects use commonMain without `kotlin/` subdir — accept that too.
        let common_main = candidate.join("shared/src/commonMain");
        if common_main.is_dir() {
            return Some(common_main);
        }
        None
    }

    fn build_schema_module_index(&mut self) {
        #[derive(Deserialize)]
        struct IndexHeader {
            module: Option<String>,
        }

        for project in &self.projects {
            let index_path = self
                .root
                .join(&project.path)
                .join("schema/models/index.model.yaml");
            if !index_path.is_file() {
                continue;
            }
            let content = match std::fs::read_to_string(&index_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let header: IndexHeader = match serde_yaml::from_str(&content) {
                Ok(h) => h,
                Err(_) => continue,
            };
            if let Some(module_name) = header.module {
                self.schema_module_index
                    .insert(module_name, project.name.clone());
            }
        }
    }
}
