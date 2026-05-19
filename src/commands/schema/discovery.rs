//! Filesystem discovery: find schema files and resolve MODULE → schema dir.
//!
//! These helpers are shared by every subcommand that needs to locate a
//! module's `.model.yaml` / `.hook.yaml` / `.workflow.yaml` sources or to
//! auto-detect the active MODULE from the user's CWD.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Find all schema files reachable from `path` (recursively).
///
/// Accepts either a directory or a single file. Supports both the legacy
/// `.schema` DSL and the modern YAML format.
pub(super) fn find_schema_files(path: &PathBuf) -> Result<Vec<PathBuf>> {
    use walkdir::WalkDir;

    let mut files = Vec::new();

    if path.is_file() {
        if is_schema_file(path) {
            files.push(path.clone());
        }
        return Ok(files);
    }

    if !path.exists() {
        return Ok(files);
    }

    for entry in WalkDir::new(path)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() && is_schema_file(path) {
            files.push(path.to_path_buf());
        }
    }

    files.sort();
    Ok(files)
}

/// Whether a path's filename matches a schema file pattern (legacy DSL or
/// modern YAML).
pub(super) fn is_schema_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    // Legacy DSL
    name.ends_with(".model.schema")
        || name.ends_with(".hook.schema")
        || name.ends_with(".workflow.schema")
    // Modern YAML
        || name.ends_with(".model.yaml")
        || name.ends_with(".hook.yaml")
        || name.ends_with(".workflow.yaml")
}

/// Resolve a `Some(module)` arg or auto-detect from CWD.
///
/// `cmd_label` is used in error messages to show the user which command they
/// were trying to run (e.g. "schema generate" vs "kotlin generate"). Returns
/// the explicit name when provided; otherwise walks up CWD looking for a
/// project in `metaphor.yaml`. Errors with a project list when both fail.
pub(crate) fn resolve_module_arg(module: Option<String>, cmd_label: &str) -> Result<String> {
    if let Some(m) = module.filter(|s| !s.is_empty()) {
        return Ok(m);
    }

    let cwd = std::env::current_dir().context("getting current dir")?;
    let workspace = crate::commands::workspace::Workspace::from_cwd(&cwd);
    if let Some(ws) = workspace.as_ref() {
        if let Some(project) = ws.project_for_cwd(&cwd) {
            return Ok(project.name.clone());
        }
        let names: Vec<String> = ws.projects().iter().map(|p| p.name.clone()).collect();
        let listing = if names.is_empty() {
            "(metaphor.yaml has no projects)".to_string()
        } else {
            names.join(", ")
        };
        anyhow::bail!(
            "no <MODULE> arg given and CWD does not match any workspace project. \
             Run `{}` from inside a project dir, or pass MODULE explicitly. \
             Available projects: {}",
            cmd_label,
            listing
        );
    }

    anyhow::bail!(
        "no <MODULE> arg given and not inside a Metaphor workspace. \
         Run `{}` from a directory with a metaphor.yaml in its tree, \
         or pass MODULE explicitly.",
        cmd_label
    );
}

/// Find the schema path for a module.
///
/// Resolution order:
/// 1. **Workspace lookup** — if `metaphor.yaml` exists upward from CWD,
///    match the arg against project names *and* the `module:` field declared
///    in each project's `schema/models/index.model.yaml`. This lets users
///    run `metaphor schema generate bersihir` (schema module name) or
///    `metaphor schema generate bersihir-service` (project name) and have
///    both resolve to the same `apps/bersihir-service/schema/` directory.
/// 2. **Legacy `libs/modules/...` / `modules/...`** candidates relative to CWD.
/// 3. **Direct path** fallback — this quirk lets `metaphor schema generate schema`
///    work when invoked from a directory that happens to have a `./schema/` subdir.
pub(super) fn find_module_schema_path(module: &str) -> Result<PathBuf> {
    // (1) Workspace lookup
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(ws) = crate::commands::workspace::Workspace::from_cwd(&cwd) {
            if let Some(dir) = ws.schema_dir_for(module) {
                return Ok(dir);
            }
        }
    }

    // (2) Legacy candidates
    let candidates = [
        PathBuf::from(format!("libs/modules/{}/schema", module)),
        PathBuf::from(format!("libs/modules/{}", module)),
        PathBuf::from(format!("modules/{}/schema", module)),
        PathBuf::from(format!("modules/{}", module)),
        PathBuf::from(module),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.clone());
        }
    }

    // (3) Direct path fallback
    let path = PathBuf::from(module);
    if path.exists() {
        return Ok(path);
    }

    // Return first candidate as default (will surface "no files found" later)
    Ok(candidates[0].clone())
}
