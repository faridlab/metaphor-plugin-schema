//! `metaphor schema status` — read-only drift check.
//!
//! Same load/diff pipeline as the migration command, but no SQL is emitted
//! and no snapshot is updated. When drift is detected the command exits
//! with an error so CI gates that want to fail on drift can simply check
//! the exit status.

use anyhow::Result;
use colored::Colorize;

use crate::resolver::resolve_schema;

use super::super::discovery::{find_module_schema_path, find_schema_files};
use super::super::module_loader::build_module_schema;
use super::snapshot::{build_schema_snapshot, get_old_schema};

/// Show schema drift between YAML definitions and database/snapshot (read-only).
pub(in crate::commands::schema) fn execute_status(
    module: &str,
    database_url: Option<String>,
) -> Result<()> {
    use crate::migration::{diff_schemas, SafetyAnalysis};

    println!(
        "{} for module: {}",
        "Checking schema status".green().bold(),
        module.cyan()
    );

    let schema_path = find_module_schema_path(module)?;
    let schema_files = find_schema_files(&schema_path)?;

    if schema_files.is_empty() {
        println!("{}", "No schema files found".yellow());
        return Ok(());
    }

    let (module_schema, parse_errors) = build_module_schema(module, &schema_files)?;

    if !parse_errors.is_empty() {
        for error in &parse_errors {
            println!("  {}", error.red());
        }
        anyhow::bail!("Parsing failed");
    }

    let resolved = resolve_schema(&module_schema)
        .map_err(|e| anyhow::anyhow!("Schema validation failed: {:?}", e))?;

    let new_schema = build_schema_snapshot(&resolved);
    let old_schema = get_old_schema(&schema_path, database_url.as_deref())?;

    let diff = diff_schemas(&old_schema, &new_schema);

    if !diff.has_changes() {
        println!();
        println!("  {} Schema is up to date — no drift detected", "✓".green());
        return Ok(());
    }

    let safety = SafetyAnalysis::from_diff(&diff);

    println!();
    println!("{}", "Schema drift detected:".yellow().bold());
    println!("{}", diff.summary());
    println!();
    println!("{}", "Safety analysis:".blue().bold());
    println!("{}", safety.summary());

    for change in diff.table_changes.values() {
        for rename in &change.rename_candidates {
            println!(
                "  {} Possible rename in {}: {} -> {} (type: {})",
                "?".cyan(),
                change.table_name,
                rename.old_name,
                rename.new_name,
                rename.data_type
            );
        }
    }

    if diff.has_destructive_changes() {
        println!();
        println!("{}", "WARNING: Destructive changes detected!".red().bold());
    }

    println!();
    println!(
        "Run {} to generate migration files.",
        format!("metaphor-schema schema migration {}", module).cyan()
    );

    // Signal drift via error so callers (CLI, CI) can handle appropriately.
    anyhow::bail!("Schema drift detected — {} change(s) pending", {
        let mut count = diff.tables_added.len() + diff.tables_removed.len();
        for change in diff.table_changes.values() {
            count += change.columns_added.len()
                + change.columns_removed.len()
                + change.columns_modified.len();
        }
        count += diff.enums_added.len() + diff.enums_removed.len();
        count
    });
}
