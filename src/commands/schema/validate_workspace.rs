//! `metaphor schema validate-workspace` — a cross-module pass over every module in the workspace.
//!
//! Per-module validation (`metaphor schema validate <module>`) cannot see other modules, so a
//! `@foreign_key(other_module.Entity.id)` pointing at a nonexistent entity passes. This command loads
//! every module listed in `metaphor.yaml`, builds a registry of module → entities, and reports every
//! cross-module FK that dangles — the check that would have caught `corpus.Organization`.
//!
//! It is also the tenancy-declaration sweep progress report: under ADR-0029 modules are
//! tenant-agnostic, so an ABSENT `company_fence:` declaration is the intended end state. A
//! surviving declaration (still-fenced, not-yet-stripped module) is counted as a note, not a
//! failure — the strip sweep retires them one module per train. A future train inverts this to
//! fail on any surviving declaration once the sweep completes.

use std::collections::HashSet;

use anyhow::Result;
use colored::Colorize;

use crate::resolver::cross_module_fk::{
    collect_cross_module_fk_refs, validate_cross_module_fks, CrossModuleFkRef, EntityRegistry,
};

use super::discovery::find_schema_files;
use super::module_loader::build_module_schema;

pub(super) fn execute_validate_workspace() -> Result<()> {
    println!(
        "{} cross-module foreign keys + tenancy-declaration sweep progress",
        "Validating".green().bold()
    );

    let cwd = std::env::current_dir()?;
    let Some(ws) = crate::commands::workspace::Workspace::from_cwd(&cwd) else {
        anyhow::bail!("no metaphor.yaml found — run this from inside a workspace");
    };

    let mut registry: EntityRegistry = EntityRegistry::new();
    let mut all_refs: Vec<CrossModuleFkRef> = Vec::new();
    let mut modules_scanned = 0usize;
    let mut parse_failures: Vec<String> = Vec::new();
    let mut still_declaring: Vec<String> = Vec::new();

    for project in ws.projects() {
        // Build the schema dir straight from the project's own path (`<project>/schema`), not via a
        // name resolver — the resolver keys on schema-module names (`accounting`), but here we hold
        // project names (`backbone-accounting`), and mixing the two silently loaded only a subset.
        let schema_dir = ws.project_path(project).join("schema");
        if !schema_dir.is_dir() {
            continue; // crate/service project with no schema — skip.
        }
        let Ok(files) = find_schema_files(&schema_dir) else {
            continue;
        };
        if files.is_empty() {
            continue;
        }

        let (schema, parse_errors) = match build_module_schema(&project.name, &files) {
            Ok(pair) => pair,
            Err(e) => {
                parse_failures.push(format!("{}: {e}", project.name));
                continue;
            }
        };
        // A parse error in one module shouldn't hide dangling FKs in the rest — record and continue.
        if !parse_errors.is_empty() {
            parse_failures.push(format!(
                "{}: {} parse error(s)",
                project.name,
                parse_errors.len()
            ));
        }

        // Tenancy sweep progress (ADR-0029): modules are tenant-agnostic, so a missing
        // declaration is the goal, not a gap. Count the modules still carrying one — those
        // await their strip release; the count is a note so the sweep's remaining work is
        // visible on every workspace validation without gating anything.
        if schema.company_fence.is_some() {
            still_declaring.push(project.name.clone());
        }

        // Key the registry by the schema module name (`corpus`, `sapiens`) — the name FK refs use,
        // set from `index.model.yaml`'s `module:` field, not the project name (`backbone-corpus`).
        let module_name = schema.name.clone();
        let entities: HashSet<String> = schema.models.iter().map(|m| m.name.clone()).collect();
        all_refs.extend(collect_cross_module_fk_refs(&module_name, &schema));
        registry.insert(module_name, entities);
        modules_scanned += 1;
    }

    let errors = validate_cross_module_fks(&registry, &all_refs);
    let dangling = errors.len();

    println!(
        "  scanned {} module(s), {} cross-module reference(s) (direct fields + shared types)",
        modules_scanned,
        all_refs.len()
    );

    if !parse_failures.is_empty() {
        println!(
            "  {} {} module(s) could not be fully parsed (their refs may be incomplete):",
            "note:".yellow().bold(),
            parse_failures.len()
        );
        for f in &parse_failures {
            println!("    - {f}");
        }
    }

    if !still_declaring.is_empty() {
        println!(
            "  {} {} module(s) still declare a company_fence posture — tenant-agnostic under \
             ADR-0029; each retires with its strip release:",
            "sweep:".yellow().bold(),
            still_declaring.len()
        );
        for name in &still_declaring {
            println!("    - {name}");
        }
    }

    if errors.is_empty() {
        println!();
        println!(
            "{} every cross-module foreign key resolves",
            "Validation passed:".green().bold()
        );
        return Ok(());
    }

    for e in &errors {
        println!("  {} {e}", "Error:".red().bold());
    }
    println!();
    anyhow::bail!(
        "validation failed: {} dangling cross-module reference(s)",
        dangling
    );
}
