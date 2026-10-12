//! `metaphor schema rules` — list every rule and state machine a module's hook
//! files declare, the model each governs, and how it is enforced today.
//!
//! Read-only. The count of unenforced rules is the number the enforcement
//! work drives to zero; this is where it is read.

use anyhow::Result;
use colored::Colorize;

use crate::rules::expr::ExpressionCheck;
use crate::rules::rewrite;
use crate::rules::{RuleBody, RuleRegister};

use super::discovery::{find_module_schema_path, find_schema_files};
use super::module_loader::build_module_schema;
use super::OutputFormat;

pub(super) fn execute_rules(
    module: &str,
    format: OutputFormat,
    suggest: bool,
    audit: bool,
    update_baseline: bool,
    check_baseline: bool,
) -> Result<()> {
    let schema_path = find_module_schema_path(module)?;
    let schema_files = find_schema_files(&schema_path)?;
    let (module_schema, _parse_errors) = build_module_schema(module, &schema_files)?;
    if check_baseline {
        let root = super::doctor::locate_consumer_root(&schema_path).unwrap_or_else(|| schema_path.clone());
        let problems = super::doctor::rule_ratchet(&module_schema.rule_register, &root)?;
        for p in &problems {
            println!("  {} {p}", "✗".red().bold());
        }
        if !problems.is_empty() {
            anyhow::bail!("the rule ratchet failed ({} problem(s))", problems.len());
        }
        println!("  {} rule ratchet holds", "✓".green());
        return Ok(());
    }
    if update_baseline {
        let root = super::doctor::locate_consumer_root(&schema_path).unwrap_or_else(|| schema_path.clone());
        let now = crate::rules::baseline::measure(&module_schema.rule_register, &root);
        if let Some(was) = crate::rules::baseline::read(&root)? {
            for r in crate::rules::baseline::regressions(&was, &now) {
                println!("  {} {r}: the baseline now accepts it", "!".yellow().bold());
            }
        }
        crate::rules::baseline::write(&root, &now)?;
        println!(
            "  {} {} written: {} unenforced, {} warnings, {} machines without verbs, {} untested rule ids",
            "✓".green(),
            crate::rules::baseline::FILE,
            now.unenforced,
            now.warnings,
            now.machines_without_verbs,
            now.untested_rule_ids
        );
        return Ok(());
    }
    if audit {
        for model in &module_schema.models {
            for rule in crate::rules::compile::model_row_rules(&module_schema, model) {
                // A transition or context rule cannot be checked against stored rows.
                let Some(query) = crate::rules::compile::audit_query(model, &rule) else { continue };
                println!("-- {} `{}` ({})", model.name, rule.name, rule.source);
                println!("{query}");
            }
            for fk in crate::rules::compile::declared_foreign_keys(&module_schema, model) {
                let Some(query) = crate::rules::compile::foreign_key_audit_query(&module_schema, model, &fk) else { continue };
                let spelling = if fk.names_table { " (declared with the table's name; write the column)" } else { "" };
                println!("-- {}.{} -> {}.{}: declared foreign key, a constraint once enforced{spelling}", model.name, fk.field, fk.target, fk.target_column);
                println!("{query}");
            }
            for rule in crate::rules::compile::model_uniqueness_rules(&module_schema, model) {
                println!("-- {} `{}` ({}): uniqueness, a unique index once enforced", model.name, rule.name, rule.source);
                println!("{}", crate::rules::compile::uniqueness_audit_query(model, &rule));
            }
        }
        return Ok(());
    }
    let register = &module_schema.rule_register;

    match format {
        OutputFormat::Json => {
            let report = serde_json::json!({
                "module": module_schema.name,
                "declared": register.rules.len(),
                "unenforced": register.unenforced_rules(),
                "readiness": register.readiness(),
                "rules": register.rules,
                "machines": register.machines,
                "warnings": register.warnings.iter().map(|w| w.to_string()).collect::<Vec<_>>(),
                "proposals": proposals(register),
            });
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        OutputFormat::Pretty => print_pretty(&module_schema.name, register, suggest),
    }
    Ok(())
}

/// One line for `schema validate`: how many rules, how many enforced.
pub(super) fn summary_line(register: &RuleRegister) -> String {
    let r = register.readiness();
    format!(
        "{} rule(s) and {} state machine(s) declared; {} rule(s) not enforced by generated code; \
         expressions: {} ready, {} unresolved, {} unparsed (`schema rules` lists them)",
        register.rules.len(),
        register.machines.len(),
        register.unenforced_rules(),
        r.ready,
        r.unresolved,
        r.unparsed
    )
}

/// `{source, from, to}` for every unparsed expression a mechanical rewrite
/// brings into the rule grammar.
fn proposals(register: &RuleRegister) -> Vec<serde_json::Value> {
    register
        .rules
        .iter()
        .filter(|r| matches!(r.expression, ExpressionCheck::Unparsed { .. }))
        .filter_map(|r| match &r.body {
            RuleBody::Condition(t) | RuleBody::Assertion(t) => rewrite::propose(t).map(|to| {
                serde_json::json!({ "source": r.source.to_string(), "rule": r.name, "from": t, "to": to })
            }),
            _ => None,
        })
        .collect()
}

/// The check state of an expression, with what stands in its way.
fn describe(check: &ExpressionCheck) -> String {
    match check {
        ExpressionCheck::NotAnExpression => String::new(),
        ExpressionCheck::Ready => "ready".green().to_string(),
        ExpressionCheck::Unresolved { names, functions } => {
            let mut parts = Vec::new();
            if !names.is_empty() {
                parts.push(format!("unknown names {}", names.join(", ")));
            }
            if !functions.is_empty() {
                parts.push(format!("unknown functions {}", functions.join(", ")));
            }
            format!("{}: {}", "unresolved".yellow(), parts.join("; "))
        }
        ExpressionCheck::Unparsed { error } => format!("{}: {error}", "unparsed".red()),
    }
}

fn print_pretty(module: &str, register: &RuleRegister, suggest: bool) {
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
        match &rule.body {
            RuleBody::Condition(text) | RuleBody::Assertion(text) => {
                println!("      {}", text.dimmed());
                println!("      {}", describe(&rule.expression));
                if suggest && matches!(rule.expression, ExpressionCheck::Unparsed { .. }) {
                    if let Some(p) = rewrite::propose(text) {
                        println!("      {} {p}", "proposal:".cyan());
                    }
                }
            }
            RuleBody::Catalog(check) => {
                let params: Vec<String> = check.params.iter().map(|(k, v)| format!("{k}: {v}")).collect();
                let layer = check.layer.map(|l| format!("{l:?}").to_lowercase()).unwrap_or_else(|| "?".into());
                println!("      {} {{ {} }}  layer: {}", check.rule, params.join(", "), layer);
                if check.problems.is_empty() {
                    println!("      {}", "sound".green());
                } else {
                    for p in &check.problems {
                        println!("      {} {p}", "problem:".red());
                    }
                }
            }
            _ => {}
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
            for t in m.transitions.iter().filter(|t| t.guard.is_some()) {
                println!(
                    "      {} guard {}: {}",
                    t.verb.as_deref().unwrap_or("transition"),
                    t.guard.as_deref().unwrap_or(""),
                    describe(&t.guard_check)
                );
            }
        }
    }
    if !register.warnings.is_empty() {
        println!("{} ({})", "Warnings:".yellow().bold(), register.warnings.len());
        for w in &register.warnings {
            println!("  {w}");
        }
    }
}
