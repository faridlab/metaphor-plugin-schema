//! `metaphor schema diff` — preview which generated files would change if
//! `metaphor schema generate` were run now.
//!
//! Runs the full generator and compares each output against the on-disk
//! file, printing a per-file summary with line-count deltas. Does not
//! consult the merge engine, so the preview reflects raw generator output
//! rather than merged output — the user can spot stale generators before
//! committing.

use anyhow::Result;
use colored::Colorize;
use std::fs;

use crate::generators::{generate_all_with_options, GenerationOptions, GenerationTarget};
use crate::resolver::resolve_schema;

use super::discovery::{find_module_schema_path, find_schema_files};
use super::module_loader::build_module_schema;

pub(super) fn execute_diff(module: &str, base: &str) -> Result<()> {
    println!(
        "{} for module: {} (comparing against {})",
        "Showing diff".green().bold(),
        module.cyan(),
        base.yellow()
    );

    let schema_path = find_module_schema_path(module)?;
    let schema_files = find_schema_files(&schema_path)?;

    if schema_files.is_empty() {
        println!("{}", "No schema files found".yellow());
        return Ok(());
    }

    let (module_schema, _) = build_module_schema(module, &schema_files)?;

    let resolved = resolve_schema(&module_schema)
        .map_err(|_| anyhow::anyhow!("Schema validation failed"))?;

    let targets = GenerationTarget::all();
    let generated = generate_all_with_options(&resolved, &targets, &GenerationOptions::default())?;

    let output_dir = schema_path.parent().unwrap_or(&schema_path).to_path_buf();

    println!();
    let mut changes = 0;

    for (path, new_content) in &generated.files {
        let full_path = output_dir.join(path);

        if !full_path.exists() {
            println!("  {} {}", "New file:".green(), full_path.display());
            changes += 1;
            continue;
        }

        let existing_content = fs::read_to_string(&full_path).unwrap_or_default();

        if existing_content != *new_content {
            println!("  {} {}", "Modified:".yellow(), full_path.display());

            let old_lines = existing_content.lines().count();
            let new_lines = new_content.lines().count();
            let diff = new_lines as i64 - old_lines as i64;

            if diff > 0 {
                println!(
                    "    {} lines, {} lines",
                    format!("+{}", diff).green(),
                    "-0".to_string().red()
                );
            } else if diff < 0 {
                println!(
                    "    {} lines, {} lines",
                    "+0".green(),
                    format!("{}", diff).red()
                );
            } else {
                println!("    Content changed (same line count)");
            }

            changes += 1;
        }
    }

    if changes == 0 {
        println!("  {} Generated code is up to date", "✓".green());
    } else {
        println!();
        println!(
            "{} {} file(s) would change",
            "Summary:".cyan().bold(),
            changes
        );
        println!("  Run {} to update", "backbone schema generate".yellow());
    }

    Ok(())
}
