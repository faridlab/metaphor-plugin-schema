//! Phase 2: discover, load, filter, and resolve a module's schema.
//!
//! Encapsulates everything between "have a module name" and "have a
//! [`ResolvedSchema`] ready to feed into generators". Fans out into the
//! discovery and module-loader submodules; this layer just orchestrates
//! the calls, runs the user-requested model/hook/workflow filters, and
//! switches between strict and lenient resolver behaviour.

use anyhow::Result;
use colored::Colorize;
use std::path::PathBuf;

use crate::ast::ModuleSchema;
use crate::resolver::{resolve_schema, ResolvedSchema};

use super::super::discovery::{find_module_schema_path, find_schema_files};
use super::super::module_loader::build_module_schema;

/// Result of the discover+load+filter+resolve pipeline. `None` means no
/// schema files were found and the caller should bail out cleanly.
pub(super) struct LoadedSchema {
    pub schema_path: PathBuf,
    pub resolved: ResolvedSchema,
}

pub(super) fn load_and_resolve(
    module: &str,
    models_filter: Option<&str>,
    hooks_filter: Option<&str>,
    workflows_filter: Option<&str>,
    lenient: bool,
) -> Result<Option<LoadedSchema>> {
    let schema_path = find_module_schema_path(module)?;
    let schema_files = find_schema_files(&schema_path)?;

    if schema_files.is_empty() {
        println!("{}", "No schema files found".yellow());
        return Ok(None);
    }

    let (mut module_schema, parse_errors) = build_module_schema(module, &schema_files)?;

    if !parse_errors.is_empty() {
        for error in &parse_errors {
            println!("  {} {}", "Parse error:".red().bold(), error);
        }
        anyhow::bail!("Parsing failed with {} error(s)", parse_errors.len());
    }

    if let Some(ref config) = module_schema.generators_config {
        if let Some(ref enabled) = config.enabled {
            println!("  Enabled generators: {}", enabled.join(", ").yellow());
        }
        if let Some(ref disabled) = config.disabled {
            println!("  Disabled generators: {}", disabled.join(", ").yellow());
        }
    }

    apply_filters(
        &mut module_schema,
        models_filter,
        hooks_filter,
        workflows_filter,
    );

    let is_filtered =
        models_filter.is_some() || hooks_filter.is_some() || workflows_filter.is_some();

    let resolved = match resolve_schema(&module_schema) {
        Ok(resolved) => resolved,
        Err(errors) => {
            if lenient || is_filtered {
                println!(
                    "  {} {} validation warning(s) (lenient mode)",
                    "⚠".yellow(),
                    errors.len()
                );
                if !errors.is_empty() {
                    println!(
                        "    {} Use --lenient to suppress these warnings",
                        "Tip:".blue()
                    );
                }
                ResolvedSchema {
                    schema: module_schema.clone(),
                }
            } else {
                for err in &errors {
                    println!("  {} {}", "Error:".red().bold(), err);
                }
                anyhow::bail!("Schema validation failed with {} error(s)", errors.len());
            }
        }
    };

    Ok(Some(LoadedSchema {
        schema_path,
        resolved,
    }))
}

/// Apply user-supplied `--models` / `--hooks` / `--workflows` filters in
/// place. Each filter is a comma-separated list of names; matches are
/// case-insensitive substring (and exact case-insensitive name).
fn apply_filters(
    module_schema: &mut ModuleSchema,
    models_filter: Option<&str>,
    hooks_filter: Option<&str>,
    workflows_filter: Option<&str>,
) {
    if let Some(filter) = models_filter {
        let filter_names: Vec<&str> = filter.split(',').map(|s| s.trim()).collect();
        let original_count = module_schema.models.len();
        module_schema.models.retain(|model| {
            filter_names.iter().any(|f| {
                model.name.eq_ignore_ascii_case(f)
                    || model.name.to_lowercase().contains(&f.to_lowercase())
            })
        });
        let filtered_count = module_schema.models.len();
        println!(
            "  {} Filtered models: {} -> {} (filter: {})",
            "🔍".cyan(),
            original_count,
            filtered_count,
            filter.yellow()
        );

        module_schema.entities.retain(|entity| {
            filter_names.iter().any(|f| {
                entity.name.eq_ignore_ascii_case(f)
                    || entity.name.to_lowercase().contains(&f.to_lowercase())
            })
        });
    }

    if let Some(filter) = hooks_filter {
        let filter_names: Vec<&str> = filter.split(',').map(|s| s.trim()).collect();
        let original_count = module_schema.hooks.len();
        module_schema.hooks.retain(|hook| {
            filter_names.iter().any(|f| {
                hook.name.eq_ignore_ascii_case(f)
                    || hook.name.to_lowercase().contains(&f.to_lowercase())
            })
        });
        let filtered_count = module_schema.hooks.len();
        println!(
            "  {} Filtered hooks: {} -> {} (filter: {})",
            "🔍".cyan(),
            original_count,
            filtered_count,
            filter.yellow()
        );
    }

    if let Some(filter) = workflows_filter {
        let filter_names: Vec<&str> = filter.split(',').map(|s| s.trim()).collect();
        let original_count = module_schema.workflows.len();
        module_schema.workflows.retain(|workflow| {
            filter_names.iter().any(|f| {
                workflow.name.eq_ignore_ascii_case(f)
                    || workflow.name.to_lowercase().contains(&f.to_lowercase())
            })
        });
        let filtered_count = module_schema.workflows.len();
        println!(
            "  {} Filtered workflows: {} -> {} (filter: {})",
            "🔍".cyan(),
            original_count,
            filtered_count,
            filter.yellow()
        );
    }
}
