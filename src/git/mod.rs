//! Git-aware change detection for schema generation
//!
//! This module provides utilities to detect which schema files have changed
//! using git, enabling incremental code generation.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

/// Type of change detected by git
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
}

impl ChangeType {
    fn from_git_status(status: &str) -> Option<Self> {
        match status.chars().next()? {
            'A' => Some(ChangeType::Added),
            'M' => Some(ChangeType::Modified),
            'D' => Some(ChangeType::Deleted),
            'R' => Some(ChangeType::Renamed),
            '?' => Some(ChangeType::Untracked),
            _ => Some(ChangeType::Modified), // Default to modified for other statuses
        }
    }
}

/// A schema file that has changed
#[derive(Debug, Clone)]
pub struct ChangedSchema {
    /// Path to the schema file (relative to repo root)
    pub path: PathBuf,
    /// Type of change
    pub change_type: ChangeType,
    /// Module name extracted from path
    pub module: String,
    /// Schema type (model, hook, workflow)
    pub schema_type: SchemaType,
}

/// Type of schema file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaType {
    Model,
    Hook,
    Workflow,
    Index,
}

impl SchemaType {
    fn from_filename(filename: &str) -> Option<Self> {
        if filename == "index.model.yaml" || filename == "index.hook.yaml" {
            Some(SchemaType::Index)
        } else if filename.ends_with(".model.yaml") || filename.ends_with(".model.schema") {
            Some(SchemaType::Model)
        } else if filename.ends_with(".hook.yaml") || filename.ends_with(".hook.schema") {
            Some(SchemaType::Hook)
        } else if filename.ends_with(".workflow.yaml") || filename.ends_with(".workflow.schema") {
            Some(SchemaType::Workflow)
        } else {
            None
        }
    }
}

/// Generated output that would be affected by a schema change
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AffectedOutput {
    /// Relative path to the generated file
    pub path: PathBuf,
    /// Generation target type
    pub target: String,
}

/// Git-aware change detector for schema files
pub struct GitChangeDetector {
    /// Repository root path
    repo_root: PathBuf,
    /// Base reference for comparison (e.g., "HEAD", "main", "origin/main")
    base_ref: String,
}

impl GitChangeDetector {
    /// Create a new change detector
    pub fn new(repo_root: PathBuf) -> Self {
        Self {
            repo_root,
            base_ref: "HEAD".to_string(),
        }
    }

    /// Set the base reference for comparison
    pub fn with_base_ref(mut self, base_ref: &str) -> Self {
        self.base_ref = base_ref.to_string();
        self
    }

    /// Find the git repository root from current directory
    pub fn find_repo_root() -> Result<PathBuf> {
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .context("Failed to run git")?;

        if !output.status.success() {
            anyhow::bail!("Not a git repository");
        }

        let path = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();

        Ok(PathBuf::from(path))
    }

    /// Get all changed schema files for a specific module
    pub fn get_changed_schemas(&self, module: &str) -> Result<Vec<ChangedSchema>> {
        let schema_path = format!("libs/modules/{}/schema/", module);
        self.get_changed_schemas_in_path(&schema_path)
    }

    /// Get all changed schema files across all modules
    pub fn get_all_changed_schemas(&self) -> Result<Vec<ChangedSchema>> {
        self.get_changed_schemas_in_path("libs/modules/")
    }

    /// Get changed schema files in a specific path
    fn get_changed_schemas_in_path(&self, path: &str) -> Result<Vec<ChangedSchema>> {
        let mut changes = Vec::new();

        // Get staged changes (committed vs HEAD or base)
        let staged = self.get_git_diff_files(path, true)?;
        changes.extend(staged);

        // Get unstaged changes (working tree vs index)
        let unstaged = self.get_git_diff_files(path, false)?;
        changes.extend(unstaged);

        // Get untracked files
        let untracked = self.get_untracked_files(path)?;
        changes.extend(untracked);

        // Deduplicate by path (keep first occurrence)
        let mut seen = HashSet::new();
        changes.retain(|c| seen.insert(c.path.clone()));

        Ok(changes)
    }

    /// Get files changed according to git diff
    fn get_git_diff_files(&self, path: &str, staged: bool) -> Result<Vec<ChangedSchema>> {
        let mut args = vec!["diff", "--name-status"];

        if staged {
            // Compare against base ref
            args.push(&self.base_ref);
        }

        args.push("--");
        args.push(path);

        let output = Command::new("git")
            .args(&args)
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to run git diff")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        self.parse_git_diff_output(&stdout)
    }

    /// Get untracked files
    fn get_untracked_files(&self, path: &str) -> Result<Vec<ChangedSchema>> {
        let output = Command::new("git")
            .args(["ls-files", "--others", "--exclude-standard", "--", path])
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to run git ls-files")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut changes = Vec::new();

        for line in stdout.lines() {
            let path = PathBuf::from(line.trim());
            if let Some(schema) = self.parse_schema_path(&path, ChangeType::Untracked) {
                changes.push(schema);
            }
        }

        Ok(changes)
    }

    /// Parse git diff --name-status output
    fn parse_git_diff_output(&self, output: &str) -> Result<Vec<ChangedSchema>> {
        let mut changes = Vec::new();

        for line in output.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 2 {
                continue;
            }

            let change_type = ChangeType::from_git_status(parts[0])
                .unwrap_or(ChangeType::Modified);

            // Handle renames (R100 old_path new_path)
            let file_path = if change_type == ChangeType::Renamed && parts.len() >= 3 {
                parts[2] // Use new path for renames
            } else {
                parts[1]
            };

            let path = PathBuf::from(file_path);
            if let Some(schema) = self.parse_schema_path(&path, change_type) {
                changes.push(schema);
            }
        }

        Ok(changes)
    }

    /// Parse a path to extract schema information
    fn parse_schema_path(&self, path: &Path, change_type: ChangeType) -> Option<ChangedSchema> {
        let filename = path.file_name()?.to_str()?;

        // Check if it's a schema file
        let schema_type = SchemaType::from_filename(filename)?;

        // Extract module name from path: libs/modules/{module}/schema/...
        let path_str = path.to_str()?;
        let module = if path_str.contains("libs/modules/") {
            path_str
                .split("libs/modules/")
                .nth(1)?
                .split('/')
                .next()?
                .to_string()
        } else {
            "unknown".to_string()
        };

        Some(ChangedSchema {
            path: path.to_path_buf(),
            change_type,
            module,
            schema_type,
        })
    }

    /// Map a changed schema to its affected output files
    pub fn get_affected_outputs(&self, schema: &ChangedSchema) -> Vec<AffectedOutput> {
        let mut outputs = Vec::new();

        // Extract entity name from filename
        let filename = schema.path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        // Remove .model, .hook, .workflow suffix
        let entity_name = filename
            .trim_end_matches(".model")
            .trim_end_matches(".hook")
            .trim_end_matches(".workflow");

        let snake_case = to_snake_case(entity_name);

        match schema.schema_type {
            SchemaType::Model => {
                // Model changes affect entity, repository, handler, DTO, migration
                outputs.push(AffectedOutput {
                    path: PathBuf::from(format!("src/domain/entity/{}.rs", snake_case)),
                    target: "rust".to_string(),
                });
                outputs.push(AffectedOutput {
                    path: PathBuf::from(format!("src/infrastructure/persistence/{}_repository.rs", snake_case)),
                    target: "repository".to_string(),
                });
                outputs.push(AffectedOutput {
                    path: PathBuf::from(format!("src/presentation/http/{}_handler.rs", snake_case)),
                    target: "handler".to_string(),
                });
                outputs.push(AffectedOutput {
                    path: PathBuf::from(format!("proto/domain/{}.proto", snake_case)),
                    target: "proto".to_string(),
                });
            }
            SchemaType::Hook => {
                // Hook changes affect state machine, events, triggers
                outputs.push(AffectedOutput {
                    path: PathBuf::from(format!("src/domain/state_machine/{}.rs", snake_case)),
                    target: "state-machine".to_string(),
                });
                outputs.push(AffectedOutput {
                    path: PathBuf::from(format!("src/domain/events/{}.rs", snake_case)),
                    target: "events".to_string(),
                });
                outputs.push(AffectedOutput {
                    path: PathBuf::from(format!("src/infrastructure/triggers/{}.rs", snake_case)),
                    target: "trigger".to_string(),
                });
            }
            SchemaType::Workflow => {
                // Workflow changes affect workflow handler
                outputs.push(AffectedOutput {
                    path: PathBuf::from(format!("src/application/workflow/{}.rs", snake_case)),
                    target: "flow".to_string(),
                });
            }
            SchemaType::Index => {
                // Index changes affect all entities in module (shared types)
                // This is a special case - we should regenerate all
                outputs.push(AffectedOutput {
                    path: PathBuf::from("src/domain/entity/mod.rs"),
                    target: "rust".to_string(),
                });
                outputs.push(AffectedOutput {
                    path: PathBuf::from("src/lib.rs"),
                    target: "module".to_string(),
                });
            }
        }

        outputs
    }

    /// Get all affected outputs for a list of changed schemas
    pub fn get_all_affected_outputs(&self, schemas: &[ChangedSchema]) -> Vec<AffectedOutput> {
        let mut all_outputs: HashSet<AffectedOutput> = HashSet::new();

        for schema in schemas {
            let outputs = self.get_affected_outputs(schema);
            all_outputs.extend(outputs);
        }

        all_outputs.into_iter().collect()
    }

    /// Get unique generation targets needed for changed schemas
    pub fn get_affected_targets(&self, schemas: &[ChangedSchema]) -> Vec<String> {
        let mut targets: HashSet<String> = HashSet::new();

        for schema in schemas {
            match schema.schema_type {
                SchemaType::Model => {
                    targets.insert("rust".to_string());
                    targets.insert("proto".to_string());
                    targets.insert("sql".to_string());
                    targets.insert("repository".to_string());
                    targets.insert("repository-trait".to_string());
                    targets.insert("handler".to_string());
                    targets.insert("service".to_string());
                    targets.insert("openapi".to_string());
                }
                SchemaType::Hook => {
                    targets.insert("state-machine".to_string());
                    targets.insert("events".to_string());
                    targets.insert("trigger".to_string());
                    targets.insert("validator".to_string());
                    targets.insert("permission".to_string());
                }
                SchemaType::Workflow => {
                    targets.insert("flow".to_string());
                }
                SchemaType::Index => {
                    // Index affects everything
                    targets.insert("all".to_string());
                }
            }
        }

        // If "all" is present, just return that
        if targets.contains("all") {
            return vec!["all".to_string()];
        }

        targets.into_iter().collect()
    }

    /// Check if any schemas have changed
    pub fn has_changes(&self, module: &str) -> Result<bool> {
        let changes = self.get_changed_schemas(module)?;
        Ok(!changes.is_empty())
    }

    /// Get modules that have schema changes
    pub fn get_changed_modules(&self) -> Result<Vec<String>> {
        let changes = self.get_all_changed_schemas()?;
        let modules: HashSet<String> = changes.iter().map(|c| c.module.clone()).collect();
        Ok(modules.into_iter().collect())
    }
}

/// Convert a name to snake_case
fn to_snake_case(name: &str) -> String {
    let mut result = String::new();
    let mut prev_was_upper = false;

    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 && !prev_was_upper {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap());
            prev_was_upper = true;
        } else if c == '-' || c == ' ' {
            result.push('_');
            prev_was_upper = false;
        } else {
            result.push(c);
            prev_was_upper = false;
        }
    }

    result
}

/// Summary of changes for display
pub struct ChangeSummary {
    pub total_schemas: usize,
    pub models_changed: usize,
    pub hooks_changed: usize,
    pub workflows_changed: usize,
    pub index_changed: bool,
    pub affected_modules: Vec<String>,
}

impl ChangeSummary {
    pub fn from_changes(changes: &[ChangedSchema]) -> Self {
        let mut models = 0;
        let mut hooks = 0;
        let mut workflows = 0;
        let mut index = false;
        let mut modules: HashSet<String> = HashSet::new();

        for change in changes {
            modules.insert(change.module.clone());
            match change.schema_type {
                SchemaType::Model => models += 1,
                SchemaType::Hook => hooks += 1,
                SchemaType::Workflow => workflows += 1,
                SchemaType::Index => index = true,
            }
        }

        Self {
            total_schemas: changes.len(),
            models_changed: models,
            hooks_changed: hooks,
            workflows_changed: workflows,
            index_changed: index,
            affected_modules: modules.into_iter().collect(),
        }
    }

    pub fn display(&self) -> String {
        let mut lines = Vec::new();

        if self.total_schemas == 0 {
            return "No schema changes detected".to_string();
        }

        lines.push(format!("📦 {} schema(s) changed:", self.total_schemas));

        if self.models_changed > 0 {
            lines.push(format!("  • {} model(s)", self.models_changed));
        }
        if self.hooks_changed > 0 {
            lines.push(format!("  • {} hook(s)", self.hooks_changed));
        }
        if self.workflows_changed > 0 {
            lines.push(format!("  • {} workflow(s)", self.workflows_changed));
        }
        if self.index_changed {
            lines.push("  • index (shared types) - full regeneration needed".to_string());
        }

        if !self.affected_modules.is_empty() {
            lines.push(format!("  Modules: {}", self.affected_modules.join(", ")));
        }

        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_type_from_filename() {
        assert_eq!(
            SchemaType::from_filename("user.model.yaml"),
            Some(SchemaType::Model)
        );
        assert_eq!(
            SchemaType::from_filename("user.hook.yaml"),
            Some(SchemaType::Hook)
        );
        assert_eq!(
            SchemaType::from_filename("registration.workflow.yaml"),
            Some(SchemaType::Workflow)
        );
        assert_eq!(
            SchemaType::from_filename("index.model.yaml"),
            Some(SchemaType::Index)
        );
        assert_eq!(SchemaType::from_filename("README.md"), None);
    }

    #[test]
    fn test_to_snake_case() {
        assert_eq!(to_snake_case("UserProfile"), "user_profile");
        assert_eq!(to_snake_case("user"), "user");
        // OAuth is treated as an acronym - consecutive uppercase letters don't add underscores
        assert_eq!(to_snake_case("OAuth2Provider"), "oauth2_provider");
        assert_eq!(to_snake_case("user-role"), "user_role");
    }

    #[test]
    fn test_change_type_from_git_status() {
        assert_eq!(ChangeType::from_git_status("M"), Some(ChangeType::Modified));
        assert_eq!(ChangeType::from_git_status("A"), Some(ChangeType::Added));
        assert_eq!(ChangeType::from_git_status("D"), Some(ChangeType::Deleted));
        assert_eq!(ChangeType::from_git_status("R100"), Some(ChangeType::Renamed));
        assert_eq!(ChangeType::from_git_status("?"), Some(ChangeType::Untracked));
    }
}
