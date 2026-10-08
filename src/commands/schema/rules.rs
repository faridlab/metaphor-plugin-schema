//! `metaphor schema rules` — list every rule and state machine a module's hook
//! files declare, the model each governs, and how it is enforced today.
//!
//! Read-only. The count of unenforced rules is the number the enforcement
//! work drives to zero; this is where it is read.

use anyhow::Result;
use colored::Colorize;

use crate::rules::{RuleBody, RuleRegister};

use super::discovery::{find_module_schema_path, find_schema_files};
use super::module_loader::build_module_schema;
use super::OutputFormat;

pub(super) fn execute_rules(module: &str, format: OutputFormat) -> Result<()> {
    let schema_path = find_module_schema_path(module)?;
    let schema_files = find_schema_files(&schema_path)?;
    let (module_schema, _parse_errors) = build_module_schema(module, &schema_files)?;
    let register = &module_schema.rule_register;

    match format {
        OutputFormat::Json => {
            let report = serde_json::json!({
                "module": module_schema.name,
                "declared": register.rules.len(),
                "unenforced": register.unenforced_rules(),
                "rules": register.rules,
                "machines": register.machines,
                "warnings": register.warnings.iter().map(|w| w.to_string()).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        OutputFormat::Pretty => print_pretty(&module_schema.name, register),
    }
    Ok(())
}

/// One line for `schema validate`: how many rules, how many enforced.
pub(super) fn summary_line(register: &RuleRegister) -> String {
    format!(
        "{} rule(s) and {} state machine(s) declared; {} rule(s) not enforced by generated code \
         (`schema rules` lists them)",
        register.rules.len(),
        register.machines.len(),
        register.unenforced_rules()
    )
}

fn print_pretty(module: &str, register: &RuleRegister) {
    println!(
        "{} {} — {}",
        "Rules:".bold(),
        module.cyan(),
        summary_line(register)
    );
    for rule in &register.rules {
        let label = match &rule.id {
            Some(id) => format!("{id} {}", rule.name),
            None => rule.name.clone(),
        };
        let bound = rule
            .bound_model
            .as_deref()
            .map(|m| m.to_string())
            .unwrap_or_else(|| format!("{} (unbound)", rule.target).red().to_string());
        let enforcement = rule.enforcement.as_deref().unwrap_or("db (undeclared)");
        println!(
            "  {}  {}  → {}  {}  when: {}  code: {}  enforcement: {}  {}",
            rule.source.to_string().dimmed(),
            label.bold(),
            bound,
            rule.body.kind(),
            if rule.when.is_empty() { "always".to_string() } else { rule.when.join(", ") },
            rule.code.as_deref().unwrap_or("-"),
            enforcement,
            "UNENFORCED".yellow()
        );
        if let RuleBody::Condition(text) | RuleBody::Assertion(text) = &rule.body {
            println!("      {}", text.dimmed());
        }
    }
    if !register.machines.is_empty() {
        println!("{}", "State machines:".bold());
        for m in &register.machines {
            let bound = m
                .bound_model
                .as_deref()
                .map(|b| b.to_string())
                .unwrap_or_else(|| format!("{} (unbound)", m.target).red().to_string());
            let guarded = m.transitions.iter().filter(|t| t.guard.is_some()).count();
            println!(
                "  {}  {}.{}  {} state(s), {} transition(s), {} guarded  {}",
                m.source.to_string().dimmed(),
                bound,
                m.field,
                m.states.len(),
                m.transitions.len(),
                guarded,
                "GUARDS UNENFORCED".yellow()
            );
        }
    }
    if !register.warnings.is_empty() {
        println!("{} ({})", "Warnings:".yellow().bold(), register.warnings.len());
        for w in &register.warnings {
            println!("  {w}");
        }
    }
}
