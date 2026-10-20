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
        let problems = super::doctor::rule_ratchet(&module_schema, &root)?;
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
        let now = crate::rules::baseline::measure_module(&module_schema, &root);
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
            // The field attributes' checks, then the hook rules: everything the guard checks.
            let mut rules = crate::rules::fields::model_field_rules(&module_schema, model);
            rules.extend(crate::rules::compile::model_row_rules(&module_schema, model));
            for rule in rules {
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
    let root = super::doctor::locate_consumer_root(&schema_path).unwrap_or_else(|| schema_path.clone());
    let mode = super::permissions_check::scan(&root).mode.unwrap_or(super::permissions_check::Mode::AuthContext);

    match format {
        OutputFormat::Json => {
            let report = serde_json::json!({
                "module": module_schema.name,
                "declared": register.rules.len(),
                "unenforced": register.unenforced_rules(),
                "readiness": register.readiness(),
                "rules": register.rules,
                "machines": register.machines,
                "events": register.events.iter().map(|e| serde_json::json!({
                    "name": e.name,
                    "source": e.source.to_string(),
                    "aggregate": e.bound_model,
                    "status": event_label(&module_schema, e),
                    "on": e.on.iter().map(|t| t.text()).collect::<Vec<_>>(),
                    "if": e.condition,
                    "problems": e.problems,
                })).collect::<Vec<_>>(),
                "triggers": register.triggers.iter().map(|t| {
                    let (label, hand) = trigger_labels(&module_schema, t);
                    serde_json::json!({
                        "source": t.source.to_string(),
                        "model": t.bound_model,
                        "key": t.key,
                        "label": label,
                        "when": t.when.as_ref().map(|w| w.text()),
                        "hand": hand,
                        "problems": t.problems,
                        "notes": t.notes,
                    })
                }).collect::<Vec<_>>(),
                "workflows": module_schema.workflows.iter().map(|w| {
                    let (label, hand, problems) = workflow_labels(&module_schema, w);
                    serde_json::json!({
                        "name": w.name,
                        "trigger": w.trigger.event,
                        "label": label,
                        "hand": hand,
                        "problems": problems,
                    })
                }).collect::<Vec<_>>(),
                "permissions": permissions_json(&module_schema, mode),
                "warnings": register.warnings.iter().map(|w| w.to_string()).collect::<Vec<_>>(),
                "proposals": proposals(register),
            });
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        OutputFormat::Pretty => print_pretty(&module_schema, register, suggest, mode),
    }
    Ok(())
}

/// One line for `schema validate`: how many rules, how many enforced.
pub(super) fn summary_line(register: &RuleRegister) -> String {
    let r = register.readiness();
    let staged = register.events.iter().filter(|e| e.status() == crate::rules::events::EventStatus::Staged).count();
    format!(
        "{} rule(s), {} state machine(s) and {} event(s) ({} staged by the write hooks, {} by hand) declared; {} rule(s) not enforced by generated code; \
         expressions: {} ready, {} unresolved, {} unparsed (`schema rules` lists them)",
        register.rules.len(),
        register.machines.len(),
        register.events.len(),
        staged,
        register.events.len() - staged,
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

/// The label of a declared event: staged by the generated write hooks (and when), by a hook trigger
/// that emits it, or by hand.
fn event_label(schema: &crate::ast::ModuleSchema, event: &crate::rules::events::DeclaredEvent) -> String {
    match event.status() {
        crate::rules::events::EventStatus::Staged => {
            let on: Vec<String> = event.on_text.clone();
            format!("EVENT STAGED (on {}, in tx)", on.join(" | "))
        }
        crate::rules::events::EventStatus::HandStaged => match crate::generators::triggers::emitted_events(schema).get(&event.name) {
            Some(by) => format!("EVENT STAGED (by trigger {}, in tx)", by.join(" | ")),
            None => "EVENT HAND-STAGED".to_string(),
        },
    }
}

/// The label of a declared trigger, and the labels of the hand methods it needs.
fn trigger_labels(schema: &crate::ast::ModuleSchema, t: &crate::rules::triggers::DeclaredTrigger) -> (String, Vec<String>) {
    use crate::rules::triggers::TriggerWhen;
    if !crate::generators::triggers::active(schema) {
        return ("TRIGGER UNGENERATED (legacy_triggers)".to_string(), Vec::new());
    }
    let Some(when) = &t.when else { return ("TRIGGER UNRESOLVED".to_string(), Vec::new()) };
    let hand: Vec<String> = t
        .bound_model
        .as_ref()
        .and_then(|m| schema.models.iter().find(|x| &x.name == m))
        .map(|model| {
            crate::generators::triggers::hand_methods(schema, model)
                .into_iter()
                .filter(|h| h.key == t.key && h.source == t.source.to_string())
                .map(|h| format!("TRIGGER HAND ({})", h.name))
                .collect()
        })
        .unwrap_or_default();
    let label = match when {
        TriggerWhen::Scheduled => format!("TRIGGER SCHEDULED ({})", t.schedule.as_deref().unwrap_or("")),
        TriggerWhen::HandFired => format!("TRIGGER RUNS (hand-fired by fire_{}, in tx)", crate::rules::trigger_compile::ident(&t.key)),
        other => format!("TRIGGER RUNS ({}, in tx)", other.text()),
    };
    (label, hand)
}

/// The check state of an expression, with what stands in its way.
/// How a workflow is held: run by the engine (`WORKFLOW RUNS`), documenting the hand function that
/// runs it (`WORKFLOW DOCUMENTS`), or only documentation (`WORKFLOW DOCUMENTED`); the hand methods
/// an engine workflow requires, and what stops it from running.
fn workflow_labels(schema: &crate::ast::ModuleSchema, w: &crate::ast::workflow::Workflow) -> (String, Vec<String>, Vec<String>) {
    use crate::ast::workflow::Executor;
    match w.executor_kind() {
        None => (
            "WORKFLOW UNRESOLVED".to_string(),
            Vec::new(),
            vec![format!("`config.executor: {}` is neither `engine` nor `hand`", w.executor.as_deref().unwrap_or_default())],
        ),
        Some(Executor::Engine) => {
            let compiled = crate::rules::workflows::compile(schema, w);
            let event = w.trigger.event.as_deref().unwrap_or("?");
            let persisted = if compiled.inline { "one transaction" } else { "persisted" };
            let hand = compiled
                .hand
                .iter()
                .map(|h| format!("WORKFLOW HAND ({})", h.method))
                .chain(compiled.functions.iter().map(|f| format!("WORKFLOW HAND ({})", crate::rules::workflows::function_method(f))))
                .collect();
            (format!("WORKFLOW RUNS (on {event}, {persisted}, executor=engine)"), hand, compiled.problems)
        }
        Some(Executor::Hand) => match &w.implemented_by {
            Some(claim) => (format!("WORKFLOW DOCUMENTS ({claim})"), Vec::new(), Vec::new()),
            None => ("WORKFLOW DOCUMENTED".to_string(), Vec::new(), Vec::new()),
        },
    }
}

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

fn print_pretty(schema: &crate::ast::ModuleSchema, register: &RuleRegister, suggest: bool, mode: super::permissions_check::Mode) {
    let module = schema.name.as_str();
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
            match rule.status {
                crate::rules::EnforcementStatus::Unenforced => "UNENFORCED".yellow(),
                crate::rules::EnforcementStatus::Guard => "GUARD".green(),
                crate::rules::EnforcementStatus::Shadow => "SHADOW".cyan(),
                crate::rules::EnforcementStatus::Verb => "VERB".green(),
            }
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
            // The transitions are held by the generated trigger when the machine's states are the
            // values its field holds; its guards by the generated verbs, or by hand-written ones
            // the register cannot see into.
            let model = m.bound_model.as_deref().and_then(|b| schema.models.iter().find(|x| x.name == b));
            let moves = match model {
                Some(model) if crate::generators::state_transition_check::StateTransitionCheckGenerator::checkable(model, m) => {
                    "MOVES REFUSED".green()
                }
                _ => "MOVES UNENFORCED".yellow(),
            };
            let unclaimed = m.transitions.iter().filter(|t| !t.guard_held(m)).count();
            let guards = match (guarded, m.verbs.as_deref(), unclaimed) {
                (0, _, _) => "".normal(),
                (_, Some("generated"), _) => " GUARDS IN GENERATED VERBS".green(),
                (_, _, 0) => " GUARDS CLAIMED".green(),
                (_, Some(_), n) => format!(" GUARDS IN HAND VERBS, {n} UNCLAIMED").yellow(),
                (_, None, _) => " GUARDS UNENFORCED".yellow(),
            };
            println!(
                "  {}  {}.{}  {} state(s), {} transition(s), {} guarded  {}{}",
                m.source.to_string().dimmed(),
                bound,
                m.field,
                m.states.len(),
                m.transitions.len(),
                guarded,
                moves,
                guards
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
    if !register.events.is_empty() {
        println!("{}", "Events:".bold());
        for e in &register.events {
            let label = event_label(schema, e);
            let label = if label.starts_with("EVENT STAGED") && e.problems.is_empty() {
                label.green()
            } else if e.problems.is_empty() {
                label.yellow()
            } else {
                label.red()
            };
            println!(
                "  {}  {}  → {}  v{}  {}",
                e.source.to_string().dimmed(),
                e.name.bold(),
                e.bound_model.as_deref().unwrap_or("-"),
                e.version,
                label
            );
            if let Some(condition) = &e.condition {
                println!("      if: {}  {}", condition.dimmed(), describe(&e.condition_check));
            }
            for problem in &e.problems {
                println!("      {} {problem}", "problem:".red());
            }
        }
    }
    if !register.triggers.is_empty() {
        println!("{}", "Triggers:".bold());
        for t in &register.triggers {
            let (label, hand) = trigger_labels(schema, t);
            let label = if !t.problems.is_empty() {
                label.red()
            } else if label.starts_with("TRIGGER UNGENERATED") {
                label.yellow()
            } else {
                label.green()
            };
            println!(
                "  {}  {}  → {}  {}",
                t.source.to_string().dimmed(),
                t.key.bold(),
                t.bound_model.as_deref().unwrap_or("-"),
                label
            );
            for h in hand {
                println!("      {}", h.yellow());
            }
            for note in &t.notes {
                println!("      {} {note}", "note:".dimmed());
            }
            for problem in &t.problems {
                println!("      {} {problem}", "problem:".red());
            }
        }
    }
    if !schema.workflows.is_empty() {
        println!("{}", "Workflows:".bold());
        for w in &schema.workflows {
            let (label, hand, problems) = workflow_labels(schema, w);
            let label = if !problems.is_empty() {
                label.red()
            } else if label.starts_with("WORKFLOW DOCUMENTED") {
                label.yellow()
            } else {
                label.green()
            };
            println!("  {}  {}", w.name.bold(), label);
            for h in hand {
                println!("      {}", h.yellow());
            }
            for problem in &problems {
                println!("      {} {problem}", "problem:".red());
            }
        }
    }
    print_permissions(schema, mode);
    if !register.warnings.is_empty() {
        println!("{} ({})", "Warnings:".yellow().bold(), register.warnings.len());
        for w in &register.warnings {
            println!("  {w}");
        }
    }
}

/// The permissions every mounted route requires, and the mode they are checked in.
fn print_permissions(schema: &crate::ast::ModuleSchema, mode: super::permissions_check::Mode) {
    let rbac = crate::rbac::Rbac::new(schema);
    let by_model = rbac.by_model();
    let public = rbac.public();
    if by_model.is_empty() && public.is_empty() {
        return;
    }
    println!("{} mode {}", "Permissions:".bold(), mode.label());
    for p in &public {
        println!(
            "  {}  {} ({})  served without a permission or a login where mounted outside the auth layer (`public_routes()`){}",
            p.model.bold(),
            "PERMISSION PUBLIC".yellow(),
            p.actions(),
            if p.scoped { "; fenced, so only to a caller the tenant scope admits" } else { "" }
        );
    }
    for (model, required) in &by_model {
        let entity = required.first().map(|r| r.entity.as_str()).unwrap_or("");
        let generic: Vec<&crate::rbac::Required> = required.iter().filter(|r| matches!(r.kind, crate::rbac::RouteKind::Generic(_))).collect();
        let defaults: Vec<&str> = generic
            .iter()
            .filter(|r| !r.renamed)
            .map(|r| r.permission.rsplit(':').next().unwrap_or(""))
            .collect();
        if !defaults.is_empty() {
            println!(
                "  {}  {} {}:{{{}}}  generic routes",
                model.bold(),
                "PERMISSION REQUIRED".green(),
                entity,
                defaults.join("|")
            );
        }
        for r in generic.iter().filter(|r| r.renamed) {
            println!("  {}  {} {}  generic routes, renamed by `requires:` ({})", model.bold(), "PERMISSION REQUIRED".green(), r.permission, r.what);
        }
        for r in required.iter().filter(|r| !matches!(r.kind, crate::rbac::RouteKind::Generic(_))) {
            let kind = match r.kind {
                crate::rbac::RouteKind::Transition => "generated verb",
                _ => "hand verb (guard it with `guarded`)",
            };
            println!("  {}  {} {}  {kind}: {}", model.bold(), "PERMISSION REQUIRED".green(), r.permission, r.what);
        }
    }
    // The fields a permission gates: redacted to null, refused as a column, refused as a write,
    // for a caller who does not hold it.
    for gate in rbac.field_permissions() {
        println!(
            "  {}  {} {}  field {} (`{}`): redacted to null, no filter/sort/search/aggregate, no write, without it",
            gate.model.bold(),
            "FIELD REQUIRES".green(),
            gate.permission,
            gate.key,
            gate.field
        );
    }
    let roles = rbac.system_roles();
    if !roles.is_empty() {
        println!("  system roles seeded: {}", roles.iter().map(|r| format!("{} ({} grant(s))", r.name, r.permissions.len())).collect::<Vec<_>>().join(", "));
    }
}

fn permissions_json(schema: &crate::ast::ModuleSchema, mode: super::permissions_check::Mode) -> serde_json::Value {
    let rbac = crate::rbac::Rbac::new(schema);
    serde_json::json!({
        "mode": match mode {
            super::permissions_check::Mode::Resolver => "resolver",
            super::permissions_check::Mode::AuthContext => "auth_context",
        },
        "label": mode.label(),
        "public": rbac.public().iter().map(|p| serde_json::json!({
            "model": p.model,
            "label": format!("PERMISSION PUBLIC ({})", p.actions()),
            "actions": p.gates.iter().map(|g| g.action()).collect::<Vec<_>>(),
            "scoped": p.scoped,
        })).collect::<Vec<_>>(),
        "required": rbac.required().iter().map(|r| serde_json::json!({
            "model": r.model,
            "permission": r.permission,
            "what": r.what,
            "renamed": r.renamed,
            "kind": match r.kind {
                crate::rbac::RouteKind::Generic(g) => format!("generic:{}", g.action()),
                crate::rbac::RouteKind::Transition => "transition".to_string(),
                crate::rbac::RouteKind::HandVerb => "hand_verb".to_string(),
            },
        })).collect::<Vec<_>>(),
        "field_permissions": rbac.field_permissions().iter().map(|g| serde_json::json!({
            "model": g.model,
            "field": g.field,
            "key": g.key,
            "permission": g.permission,
            "label": format!("FIELD REQUIRES {}", g.permission),
        })).collect::<Vec<_>>(),
        "vocabulary": rbac.vocabulary().into_iter().collect::<Vec<_>>(),
        "system_roles": rbac.system_roles().iter().map(|r| serde_json::json!({
            "name": r.name, "permissions": r.permissions,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod workflow_label_tests {
    use super::*;
    use crate::rules::workflows::test_support::{schema_with, BASE};

    #[test]
    fn the_register_says_whether_a_workflow_runs_documents_a_function_or_only_documents() {
        let s = schema_with(BASE);
        let (label, hand, problems) = workflow_labels(&s, &s.workflows[0]);
        assert_eq!(label, "WORKFLOW RUNS (on TicketRaised, persisted, executor=engine)");
        assert_eq!(hand, vec!["WORKFLOW HAND (notify_on_call)".to_string()]);
        assert!(problems.is_empty(), "{problems:?}");

        let mut s = schema_with(&BASE.replace("executor: engine", "executor: hand"));
        assert_eq!(workflow_labels(&s, &s.workflows[0]).0, "WORKFLOW DOCUMENTED");
        s.workflows[0].implemented_by = Some("src/service.rs::post".into());
        assert_eq!(workflow_labels(&s, &s.workflows[0]).0, "WORKFLOW DOCUMENTS (src/service.rs::post)");

        let s = schema_with(&BASE.replace("executor: engine", "executor: robot"));
        assert_eq!(workflow_labels(&s, &s.workflows[0]).0, "WORKFLOW UNRESOLVED");
    }
}
