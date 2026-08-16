//! `metaphor schema validate` — parse and resolve a module's schemas, then
//! report parse and validation errors without writing any output.

use anyhow::Result;
use colored::Colorize;

use crate::resolver::{declaration_warnings, resolve_schema};

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
            // Advisory declarations audit (ADR-0014) — printed, never a gate.
            for warning in declaration_warnings(&module_schema) {
                println!("  {} {}", "Warning:".yellow().bold(), warning);
            }
            // The missing-fence-declaration case is the ONE declarations issue that IS a gate
            // here (and in validate-workspace): ADR-0014's sweep is enforced by failing loud,
            // not by a warning authors learn to scroll past. Generate keeps it warning-only
            // so unswept legacy modules can still regenerate.
            if module_schema.company_fence.is_none() {
                anyhow::bail!(
                    "no 'company_fence:' declaration in index.model.yaml — ADR-0014 requires \
                     an explicit posture per module (strict | shared_blank | shared_tree | none)"
                );
            }
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
