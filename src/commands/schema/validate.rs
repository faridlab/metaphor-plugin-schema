//! `metaphor schema validate` — parse and resolve a module's schemas, then
//! report parse and validation errors without writing any output.

use anyhow::Result;
use colored::Colorize;

use crate::resolver::resolve_schema;

use super::discovery::{find_module_schema_path, find_schema_files};
use super::module_loader::build_module_schema;

pub(super) fn execute_validate(module: &str, warnings: bool) -> Result<()> {
    println!("{} module: {}", "Validating".green().bold(), module.cyan());

    if warnings {
        println!("  (including warnings)");
    }

    let schema_path = find_module_schema_path(module)?;
    let schema_files = find_schema_files(&schema_path)?;

    if schema_files.is_empty() {
        println!("{}", "No schema files found".yellow());
        return Ok(());
    }

    let (module_schema, parse_errors) = build_module_schema(module, &schema_files)?;

    if !parse_errors.is_empty() {
        for error in &parse_errors {
            println!("  {} {}", "Parse error:".red().bold(), error);
        }
        anyhow::bail!("Parsing failed with {} error(s)", parse_errors.len());
    }

    match resolve_schema(&module_schema) {
        Ok(_resolved) => {
            println!("  {} All schemas are valid", "✓".green().bold());
        }
        Err(errors) => {
            for err in &errors {
                println!("  {} {}", "Error:".red().bold(), err);
            }
            println!();
            println!(
                "{} {} error(s)",
                "Validation failed:".red().bold(),
                errors.len()
            );
            anyhow::bail!("Validation failed with {} error(s)", errors.len());
        }
    }

    println!();
    println!("{} No issues found", "Validation passed:".green().bold());

    Ok(())
}
