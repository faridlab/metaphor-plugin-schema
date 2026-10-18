//! Hook triggers: what the generated write hooks run, and the trait their hand-written half
//! implements.
//!
//! Per model with triggers, two generated files and a change to the model's write hooks:
//!
//! - `src/application/triggers/<model>_triggers.rs`: the trait `<Model>Triggers`, one required
//!   method per hand action (no default body, so the compiler makes the declaration and the code
//!   agree), the default implementation when there are none, and the scheduled entries. The
//!   implementation is hand code in `<model>_triggers.ext.rs` (ADR-0031), a struct named
//!   `<Model>TriggersImpl`.
//! - `src/application/triggers/mod.rs`: the module list.
//! - the model's write hooks (`<model>_write_hooks.rs`, [`super::declared_events`]): `before`
//!   runs the `before_*` triggers, `after` the `after_*` ones and then the declared events, all in
//!   the write's transaction. A failure rolls the whole write back. Writes a hand method makes to
//!   other entities go through those entities' generated services, which join the same
//!   transaction; the nesting is capped ([`backbone_core::write_hooks::in_trigger`]).
//!
//! A trigger keyed on a hand verb (`after_login`) has no write that fires it: the write hooks
//! carry a `fire_after_login` method the hand verb calls inside its transaction.
//!
//! `generators: legacy_triggers: true` keeps a module on the old behaviour: nothing is generated.
//! What the actions compile to is [`crate::rules::trigger_compile`].

use std::collections::HashSet;
use std::fmt::Write;

use crate::ast::{Model, ModuleSchema};
use crate::rules::trigger_compile::{compile_model, fire_name, Compiled, Reads, Step, TextPart};
use crate::rules::triggers::{DeclaredTrigger, Op, Phase, TriggerWhen};
use crate::utils::to_snake_case;

/// Whether the module generates its triggers: it has not opted out with `legacy_triggers: true`.
pub fn active(schema: &ModuleSchema) -> bool {
    schema
        .generators_config
        .as_ref()
        .and_then(|c| c.legacy_triggers)
        != Some(true)
}

fn enum_names(schema: &ModuleSchema) -> HashSet<String> {
    schema.enums.iter().map(|e| e.name.clone()).collect()
}

/// The triggers of `model`, compiled, in file order. Empty for a module on `legacy_triggers`.
pub fn model_triggers<'a>(
    schema: &'a ModuleSchema,
    model: &Model,
) -> Vec<(&'a DeclaredTrigger, Compiled)> {
    if !active(schema) {
        return Vec::new();
    }
    compile_model(
        &schema.rule_register.triggers,
        model,
        &enum_names(schema),
        &schema.rule_register.events,
    )
    .into_iter()
    .filter(|(index, _)| schema.rule_register.triggers[*index].problems.is_empty())
    .map(|(index, c)| (&schema.rule_register.triggers[index], c))
    .collect()
}

/// Whether `model` has any trigger the hooks run.
pub fn model_has_triggers(schema: &ModuleSchema, model: &Model) -> bool {
    !model_triggers(schema, model).is_empty()
}

/// Whether the triggers of `model` may stage an outbox row (an event, a notification, or a hand
/// method that stages one by hand).
pub fn model_triggers_stage(schema: &ModuleSchema, model: &Model) -> bool {
    model_triggers(schema, model).iter().any(|(_, c)| {
        c.steps
            .iter()
            .any(|s| s.stages() || !s.hand_methods().is_empty())
    })
}

/// One required method of a model's trigger trait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandMethod {
    pub name: String,
    /// The action as written.
    pub text: String,
    pub key: String,
    pub when: TriggerWhen,
    pub source: String,
}

/// The hand methods of `model`'s trigger trait, in file order.
pub fn hand_methods(schema: &ModuleSchema, model: &Model) -> Vec<HandMethod> {
    let mut out = Vec::new();
    for (t, c) in model_triggers(schema, model) {
        let when = t.when.clone().expect("a resolved trigger");
        for step in &c.steps {
            collect_hand(step, t, &when, &mut out);
        }
    }
    out
}

fn collect_hand(step: &Step, t: &DeclaredTrigger, when: &TriggerWhen, out: &mut Vec<HandMethod>) {
    match step {
        Step::Hand { method, text, .. } => out.push(HandMethod {
            name: method.clone(),
            text: text.clone(),
            key: t.key.clone(),
            when: when.clone(),
            source: t.source.to_string(),
        }),
        Step::If { then, .. } => collect_hand(then, t, when, out),
        _ => {}
    }
}

/// The Rust signature (after `&self, ctx: &WriteCtx`) of the hand method of a trigger that fires `when`.
fn hand_params(when: &TriggerWhen, entity: &str) -> String {
    match when {
        TriggerWhen::Write {
            phase: Phase::Before,
            op: Op::Create,
        } => format!(", e: &mut {entity}"),
        TriggerWhen::Write {
            phase: Phase::Before,
            op: Op::Delete,
        } => format!(", e: &{entity}"),
        TriggerWhen::Write {
            phase: Phase::Before,
            op: Op::Update,
        }
        | TriggerWhen::FieldChange {
            phase: Phase::Before,
            ..
        } => {
            format!(", old: &{entity}, e: &mut {entity}")
        }
        TriggerWhen::Write {
            op: Op::Create | Op::Delete,
            ..
        }
        | TriggerWhen::HandFired => format!(", e: &{entity}"),
        TriggerWhen::Write { .. }
        | TriggerWhen::FieldChange { .. }
        | TriggerWhen::Transition { .. }
        | TriggerWhen::Enter(_)
        | TriggerWhen::Exit(_) => format!(", old: &{entity}, e: &{entity}"),
        TriggerWhen::Scheduled => String::new(),
    }
}

/// `src/application/triggers/<model>_triggers.rs`, or `None` when the model has no trigger.
pub fn trait_file(schema: &ModuleSchema, model: &Model) -> Option<String> {
    let triggers = model_triggers(schema, model);
    if triggers.is_empty() {
        return None;
    }
    let name = &model.name;
    let snake = to_snake_case(name);
    let methods = hand_methods(schema, model);
    let mut out = String::new();
    writeln!(out, "//! The hand-written half of the triggers of {name}.").unwrap();
    writeln!(out, "//!").unwrap();
    writeln!(
        out,
        "//! Generated by metaphor-schema. Do not edit manually."
    )
    .unwrap();
    writeln!(out, "//!").unwrap();
    writeln!(out, "//! The hooks of the model run what the schema's `triggers:` can say (`emit:`, `log`, `set:` before a").unwrap();
    writeln!(out, "//! write, `send_email`, `notify`, conditions) and call the methods below for the rest, in file order,").unwrap();
    writeln!(out, "//! inside the write's transaction. A method that returns an error rolls the write back and everything it").unwrap();
    writeln!(out, "//! staged; return `ServiceError::Violations` for a refusal that carries a code. A write a method makes to").unwrap();
    writeln!(out, "//! another entity goes through that entity's generated service, which joins the transaction.").unwrap();
    writeln!(out, "//! An effect outside the database (mail, HTTP) is staged as an event for the relay, never performed here.").unwrap();
    if methods.is_empty() {
        writeln!(out, "//!").unwrap();
        writeln!(out, "//! Every action of {name}'s triggers compiles, so nothing is left for hand code: `{name}TriggersImpl` is empty.").unwrap();
    } else {
        writeln!(out, "//!").unwrap();
        writeln!(out, "//! Implement it in `{snake}_triggers.ext.rs`, beside this file; the module declares it when it exists. The").unwrap();
        writeln!(out, "//! generated service builder makes it with `Default`; state it needs from the module (another service) is bound").unwrap();
        writeln!(out, "//! in the module's build step (`lib.ext.rs`):").unwrap();
        writeln!(out, "//!").unwrap();
        writeln!(out, "//! ```ignore").unwrap();
        writeln!(out, "//! use super::*;").unwrap();
        writeln!(out, "//!").unwrap();
        writeln!(out, "//! #[derive(Default)]").unwrap();
        writeln!(out, "//! pub struct {name}TriggersImpl;").unwrap();
        writeln!(out, "//!").unwrap();
        writeln!(out, "//! #[async_trait]").unwrap();
        writeln!(out, "//! impl {name}Triggers for {name}TriggersImpl {{").unwrap();
        for m in &methods {
            writeln!(
                out,
                "//!     async fn {}(&self, ctx: &WriteCtx{}) -> ServiceResult<()> {{ todo!() }}",
                m.name,
                hand_params(&m.when, name)
            )
            .unwrap();
        }
        writeln!(out, "//! }}").unwrap();
        writeln!(out, "//! ```").unwrap();
    }
    writeln!(out).unwrap();
    writeln!(out, "#[allow(unused_imports)]").unwrap();
    writeln!(out, "use async_trait::async_trait;").unwrap();
    writeln!(out, "#[allow(unused_imports)]").unwrap();
    writeln!(out, "use backbone_core::write_hooks::WriteCtx;").unwrap();
    writeln!(out, "#[allow(unused_imports)]").unwrap();
    writeln!(out, "use backbone_core::{{ServiceError, ServiceResult}};").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "use crate::domain::entity::{name};").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "/// The actions of {name}'s triggers that only Rust can perform, one required method each.").unwrap();
    // Method names are `<key>__<action>`: the double underscore keeps the trigger and the action readable.
    writeln!(out, "#[allow(non_snake_case)]").unwrap();
    writeln!(out, "#[async_trait]").unwrap();
    writeln!(out, "pub trait {name}Triggers: Send + Sync {{").unwrap();
    for (i, m) in methods.iter().enumerate() {
        if i > 0 {
            writeln!(out).unwrap();
        }
        writeln!(
            out,
            "    /// `{}` ({}): `{}`",
            m.key,
            m.source,
            m.text.replace('\n', " ")
        )
        .unwrap();
        writeln!(
            out,
            "    async fn {}(&self, ctx: &WriteCtx{}) -> ServiceResult<()>;",
            m.name,
            hand_params(&m.when, name)
        )
        .unwrap();
    }
    writeln!(out, "}}").unwrap();
    if methods.is_empty() {
        writeln!(out).unwrap();
        writeln!(out, "/// The triggers of {name} need no hand code.").unwrap();
        writeln!(out, "#[derive(Default)]").unwrap();
        writeln!(out, "pub struct {name}TriggersImpl;").unwrap();
        writeln!(out).unwrap();
        writeln!(out, "#[async_trait]").unwrap();
        writeln!(out, "impl {name}Triggers for {name}TriggersImpl {{}}").unwrap();
    }
    let scheduled: Vec<(&DeclaredTrigger, &Compiled)> = triggers
        .iter()
        .filter(|(t, _)| t.when == Some(TriggerWhen::Scheduled))
        .map(|(t, c)| (*t, c))
        .collect();
    if !scheduled.is_empty() {
        let upper = snake.to_uppercase();
        writeln!(out).unwrap();
        writeln!(out, "/// The scheduled entries of {name}'s triggers. The composing service's scheduler runs each with").unwrap();
        writeln!(out, "/// [`run_{snake}_scheduled`] at its schedule.").unwrap();
        writeln!(
            out,
            "pub const {upper}_SCHEDULED: &[backbone_core::write_hooks::ScheduledTrigger] = &["
        )
        .unwrap();
        for (t, _) in &scheduled {
            writeln!(
                out,
                "    backbone_core::write_hooks::ScheduledTrigger {{ name: {:?}, schedule: {:?}, description: {:?} }},",
                crate::rules::trigger_compile::ident(&t.key),
                t.schedule.as_deref().unwrap_or(""),
                t.description.as_deref().unwrap_or("")
            )
            .unwrap();
        }
        writeln!(out, "];").unwrap();
        writeln!(out).unwrap();
        writeln!(out, "/// Run the scheduled entry `name` of {name}: its required method, with a context for a job (no actor).").unwrap();
        writeln!(out, "/// Returns `false` when `name` is no entry.").unwrap();
        writeln!(out, "pub async fn run_{snake}_scheduled(hand: &dyn {name}Triggers, name: &str) -> ServiceResult<bool> {{").unwrap();
        writeln!(
            out,
            "    let ctx = WriteCtx::current(Some({:?}));",
            super::declared_events::outbox_schema_of(model)
        )
        .unwrap();
        writeln!(out, "    match name {{").unwrap();
        for (t, c) in &scheduled {
            if let Some(method) = c.hand_methods().first() {
                writeln!(
                    out,
                    "        {:?} => hand.{method}(&ctx).await?,",
                    crate::rules::trigger_compile::ident(&t.key)
                )
                .unwrap();
            }
        }
        writeln!(out, "        _ => return Ok(false),").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out, "    Ok(true)").unwrap();
        writeln!(out, "}}").unwrap();
    }
    Some(out)
}

/// `src/application/triggers/mod.rs`, or `None` when no model has a trigger.
pub fn mod_file(schema: &ModuleSchema) -> Option<String> {
    let models: Vec<&Model> = schema
        .models
        .iter()
        .filter(|m| model_has_triggers(schema, m))
        .collect();
    if models.is_empty() {
        return None;
    }
    let mut out = String::new();
    writeln!(
        out,
        "//! Hook triggers: the hand-written half of the schema's `triggers:`."
    )
    .unwrap();
    writeln!(out, "//!").unwrap();
    writeln!(
        out,
        "//! Generated by metaphor-schema. Do not edit manually."
    )
    .unwrap();
    writeln!(out).unwrap();
    for m in &models {
        let snake = to_snake_case(&m.name);
        writeln!(out, "pub mod {snake}_triggers;").unwrap();
    }
    writeln!(out).unwrap();
    for m in &models {
        let snake = to_snake_case(&m.name);
        writeln!(out, "pub use {snake}_triggers::*;").unwrap();
    }
    Some(out)
}

/// The Rust for a value of `serde_json::Value` form of a literal key value: `true`, `7`, or text.
fn literal_val(value: &str) -> String {
    match value {
        "true" => "r::Val::Bool(true)".to_string(),
        "false" => "r::Val::Bool(false)".to_string(),
        v if v.parse::<f64>().is_ok() => format!("r::Val::Num({v}f64)"),
        v => format!("r::text({v:?})"),
    }
}

/// The condition under which `when` fires, over the `change` the hooks were handed.
fn fires(when: &TriggerWhen) -> String {
    match when {
        TriggerWhen::Write { op: Op::Create, .. } => "matches!(change, Change::Created { .. })".into(),
        TriggerWhen::Write { op: Op::Update, .. } => "matches!(change, Change::Updated { .. } | Change::Transitioned { .. })".into(),
        TriggerWhen::Write { op: Op::Delete, .. } => "matches!(change, Change::Deleted { hard: false, .. })".into(),
        TriggerWhen::FieldChange { field, value, .. } => {
            let base = format!("matches!(change, Change::Updated {{ .. }} | Change::Transitioned {{ .. }}) && change.field_changed({field:?})");
            match value {
                Some(v) => format!("{base} && r::holds(&r::eq(r::field(row, {field:?}), {}))", literal_val(v)),
                None => base,
            }
        }
        TriggerWhen::Transition { verb } => format!("matches!(change, Change::Transitioned {{ transition, .. }} if transition == {verb:?})"),
        TriggerWhen::Enter(state) => format!("matches!(change, Change::Transitioned {{ to: Some(to), .. }} if to == {state:?})"),
        TriggerWhen::Exit(state) => format!("matches!(change, Change::Transitioned {{ from: Some(from), .. }} if from == {state:?})"),
        TriggerWhen::HandFired | TriggerWhen::Scheduled => "true".into(),
    }
}

fn indent(code: &str, by: usize) -> String {
    let pad = " ".repeat(by);
    code.lines()
        .map(|l| {
            if l.is_empty() {
                String::new()
            } else {
                format!("{pad}{l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A Rust identifier for a log field: keywords and the fields the log line carries itself get a
/// trailing underscore.
fn log_field(name: &str) -> String {
    if matches!(
        name,
        "trigger"
            | "correlation_id"
            | "log_event"
            | "message"
            | "type"
            | "match"
            | "as"
            | "fn"
            | "ref"
            | "self"
            | "in"
            | "if"
            | "for"
    ) {
        format!("{name}_")
    } else {
        name.to_string()
    }
}

/// Re-read the row after a step changed it, so what follows sees the change.
const REFRESH_ROW: &str =
    "let row = &serde_json::to_value(change.subject()).unwrap_or(serde_json::Value::Null);";

/// The Rust for one step. `phase` is `Some(Phase::Before)` where the row about to be stored may be
/// changed; `args` is how a hand method is called (`write, e`, `write, old, e`).
fn step_code(step: &Step, trigger: &str, call_args: &HandCall, model: &str) -> String {
    let mut out = String::new();
    match step {
        Step::Log { name, fields } => {
            let mut extra = String::new();
            for (k, v) in fields {
                write!(extra, ", {} = %r::to_json(&{v})", log_field(k)).unwrap();
            }
            writeln!(
                out,
                "tracing::info!(target: \"backbone::trigger\", trigger = {trigger:?}, correlation_id = %write.correlation_id, log_event = {name:?}{extra});"
            )
            .unwrap();
        }
        Step::Emit { event, fields } => {
            writeln!(out, "{{").unwrap();
            writeln!(out, "    let mut payload = serde_json::Map::new();").unwrap();
            for (k, v) in fields {
                writeln!(
                    out,
                    "    payload.insert({k:?}.to_string(), r::to_json(&{v}));"
                )
                .unwrap();
            }
            writeln!(out, "    let event: declared::{event} = serde_json::from_value(serde_json::Value::Object(payload))").unwrap();
            writeln!(out, "        .map_err(|e| ServiceError::Internal(format!(\"trigger `{trigger}` could not build {event}: {{e}}\")))?;").unwrap();
            writeln!(
                out,
                "    write.stage(event.record_for(&aggregate_id, company, write)).await?;"
            )
            .unwrap();
            writeln!(out, "}}").unwrap();
        }
        Step::Notify {
            channel,
            template,
            text,
            to,
            data,
        } => {
            writeln!(out, "{{").unwrap();
            writeln!(out, "    let mut data = serde_json::Map::new();").unwrap();
            for (k, v) in data {
                writeln!(out, "    data.insert({k:?}.to_string(), r::to_json(&{v}));").unwrap();
            }
            if text.is_empty() {
                writeln!(out, "    let body: Option<String> = None;").unwrap();
            } else {
                writeln!(out, "    let mut body = String::new();").unwrap();
                for part in text {
                    match part {
                        TextPart::Lit(s) => writeln!(out, "    body.push_str({s:?});").unwrap(),
                        TextPart::Expr(e) => {
                            writeln!(out, "    body.push_str(&r::to_text(&{e}));").unwrap()
                        }
                    }
                }
                writeln!(out, "    let body = Some(body);").unwrap();
            }
            writeln!(out, "    let request = NotificationRequested {{").unwrap();
            writeln!(out, "        channel: {channel:?}.to_string(),").unwrap();
            match template {
                Some(t) => writeln!(out, "        template: Some({t:?}.to_string()),").unwrap(),
                None => writeln!(out, "        template: None,").unwrap(),
            }
            writeln!(out, "        body,").unwrap();
            writeln!(out, "        to: r::to_json(&{to}),").unwrap();
            writeln!(out, "        data: serde_json::Value::Object(data),").unwrap();
            writeln!(out, "        trigger: {trigger:?}.to_string(),").unwrap();
            writeln!(out, "    }};").unwrap();
            writeln!(
                out,
                "    write.stage(request.record({model:?}, &aggregate_id, company, write)).await?;"
            )
            .unwrap();
            writeln!(out, "}}").unwrap();
        }
        Step::Set { field, rust } => {
            let f = super::declared_events::ident(field);
            writeln!(out, "if let Some(entity) = change.after_mut() {{").unwrap();
            writeln!(
                out,
                "    entity.{f} = serde_json::from_value(r::to_json(&{rust}))"
            )
            .unwrap();
            writeln!(out, "        .map_err(|e| ServiceError::Validation(format!(\"trigger `{trigger}` could not set `{field}`: {{e}}\")))?;").unwrap();
            writeln!(out, "}}").unwrap();
            writeln!(out, "{REFRESH_ROW}").unwrap();
        }
        Step::If { condition, then } => {
            writeln!(out, "if r::holds(&{condition}) {{").unwrap();
            writeln!(
                out,
                "{}",
                indent(&step_code(then, trigger, call_args, model), 4)
            )
            .unwrap();
            writeln!(out, "}}").unwrap();
        }
        Step::Hand { method, .. } => {
            writeln!(out, "{}", call_args.call(method)).unwrap();
        }
    }
    out.trim_end().to_string()
}

/// How a hand method is called from the hooks, by the phase and kind of the trigger.
struct HandCall {
    when: TriggerWhen,
}

impl HandCall {
    fn call(&self, method: &str) -> String {
        match &self.when {
            // The row about to be stored, mutable.
            TriggerWhen::Write { phase: Phase::Before, op: Op::Create } => {
                format!("if let Some(entity) = change.after_mut() {{\n    self.triggers.{method}(write, entity).await?;\n}}\n{REFRESH_ROW}")
            }
            TriggerWhen::Write { phase: Phase::Before, op: Op::Update } | TriggerWhen::FieldChange { phase: Phase::Before, .. } => {
                format!("if let Some((old, entity)) = change.before_and_after_mut() {{\n    self.triggers.{method}(write, old, entity).await?;\n}}\n{REFRESH_ROW}")
            }
            TriggerWhen::Write { phase: Phase::Before, op: Op::Delete } => {
                format!("self.triggers.{method}(write, change.subject()).await?;")
            }
            TriggerWhen::Write { op: Op::Create | Op::Delete, .. } | TriggerWhen::HandFired => {
                format!("self.triggers.{method}(write, subject).await?;")
            }
            _ => format!("self.triggers.{method}(write, change.before().unwrap_or(subject), subject).await?;"),
        }
    }
}

fn trigger_block(
    t: &DeclaredTrigger,
    c: &Compiled,
    model: &str,
    in_change: bool,
    refresh: bool,
) -> String {
    let when = t.when.clone().expect("a resolved trigger");
    let label = format!("{model}.{}", t.key);
    let call = HandCall { when: when.clone() };
    let mut inner = String::new();
    writeln!(inner, "// `{}`: {} ({})", t.key, when.text(), t.source).unwrap();
    let mut guard = if in_change {
        fires(&when)
    } else {
        "true".to_string()
    };
    if let Some(cond) = &c.condition {
        guard = if guard == "true" {
            format!("r::holds(&{cond})")
        } else {
            format!("{guard} && r::holds(&{cond})")
        };
    }
    writeln!(inner, "if {guard} {{").unwrap();
    for step in &c.steps {
        writeln!(
            inner,
            "{}",
            indent(&step_code(step, &label, &call, model), 4)
        )
        .unwrap();
    }
    writeln!(inner, "}}").unwrap();
    if !refresh {
        return inner;
    }
    // A block of its own, so the row each trigger reads is read afresh after the ones before it.
    let mut out = String::new();
    writeln!(out, "{{").unwrap();
    writeln!(out, "    {REFRESH_ROW}").unwrap();
    writeln!(out, "{}", indent(inner.trim_end(), 4)).unwrap();
    writeln!(out, "}}").unwrap();
    out
}

/// What the generated hooks need from the triggers of a model.
pub struct HookCode {
    /// Statements of `before` (inside `in_trigger`), empty when no trigger runs before a write.
    pub before: String,
    /// Statements of `after`, run before the declared events are staged.
    pub after: String,
    /// The `fire_<key>` methods, as complete items.
    pub fires: String,
    /// What the expressions read beyond the row as stored.
    pub reads: Reads,
    /// Whether `before` or `after` has anything to do.
    pub any: bool,
}

/// The code the model's write hooks carry for its triggers.
pub fn hook_code(schema: &ModuleSchema, model: &Model) -> HookCode {
    let name = &model.name;
    let triggers = model_triggers(schema, model);
    let mut reads = Reads::default();
    let mut before = String::new();
    let mut after = String::new();
    let mut fires_code = String::new();
    for (t, c) in &triggers {
        reads.merge(c.reads);
        let when = t.when.clone().expect("a resolved trigger");
        match when.phase() {
            Some(Phase::Before) => writeln!(
                before,
                "{}",
                indent(&trigger_block(t, c, name, true, true), 12)
            )
            .unwrap(),
            Some(Phase::After) => writeln!(
                after,
                "{}",
                indent(&trigger_block(t, c, name, true, false), 12)
            )
            .unwrap(),
            None if when == TriggerWhen::HandFired => {
                let f = fire_name(t);
                let needs_old = c.reads.old;
                writeln!(fires_code).unwrap();
                writeln!(fires_code, "    /// Run `{}` ({}): called by the hand verb that is its moment, inside the verb's transaction,", t.key, t.source).unwrap();
                writeln!(fires_code, "    /// with the row as it now stands.").unwrap();
                writeln!(
                    fires_code,
                    "    #[allow(unused_variables, unused_assignments)]"
                )
                .unwrap();
                writeln!(fires_code, "    pub async fn {f}(&self, write: &WriteCtx, subject: &{name}) -> ServiceResult<()> {{").unwrap();
                writeln!(fires_code, "        use backbone_core::row_rule as r;").unwrap();
                writeln!(
                    fires_code,
                    "        backbone_core::write_hooks::in_trigger(async {{"
                )
                .unwrap();
                writeln!(
                    fires_code,
                    "            let row = serde_json::to_value(subject)"
                )
                .unwrap();
                writeln!(fires_code, "                .map_err(|e| ServiceError::Internal(format!(\"a row could not be read for its triggers: {{e}}\")))?;").unwrap();
                writeln!(fires_code, "            let row = &row;").unwrap();
                if needs_old {
                    writeln!(
                        fires_code,
                        "            let old_row = &serde_json::Value::Null;"
                    )
                    .unwrap();
                }
                writeln!(fires_code, "            let ctx = EvalCtx {{ actor: write.actor.clone(), at: chrono::Utc::now() }};").unwrap();
                writeln!(fires_code, "            let _ = &ctx;").unwrap();
                writeln!(
                    fires_code,
                    "            let aggregate_id = subject.entity_id();"
                )
                .unwrap();
                writeln!(
                    fires_code,
                    "            let company = write.company_of(row);"
                )
                .unwrap();
                writeln!(fires_code, "            let _ = (&aggregate_id, company);").unwrap();
                // A hand-fired trigger has no change, so its steps use the subject directly.
                let block = trigger_block(t, c, name, false, false);
                writeln!(fires_code, "{}", indent(&block, 12)).unwrap();
                writeln!(fires_code, "            Ok(())").unwrap();
                writeln!(fires_code, "        }})").unwrap();
                writeln!(fires_code, "        .await").unwrap();
                writeln!(fires_code, "    }}").unwrap();
            }
            None => {}
        }
    }
    let any = !before.is_empty() || !after.is_empty();
    HookCode {
        before,
        after,
        fires: fires_code,
        reads,
        any,
    }
}

/// The declared events some compiled trigger emits, with the triggers that do (`Model.key`).
pub fn emitted_events(schema: &ModuleSchema) -> std::collections::BTreeMap<String, Vec<String>> {
    fn emits(step: &Step, out: &mut Vec<String>) {
        match step {
            Step::Emit { event, .. } => out.push(event.clone()),
            Step::If { then, .. } => emits(then, out),
            _ => {}
        }
    }
    let mut out: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for model in &schema.models {
        for (t, c) in model_triggers(schema, model) {
            let mut names = Vec::new();
            for step in &c.steps {
                emits(step, &mut names);
            }
            for name in names {
                let by = out.entry(name).or_default();
                let label = format!("{}.{}", model.name, t.key);
                if !by.contains(&label) {
                    by.push(label);
                }
            }
        }
    }
    out
}

/// The `ScheduledJob` declarations of `schema`'s scheduled trigger entries, for the module's job list.
pub fn scheduled_jobs(schema: &mut ModuleSchema) {
    if !active(schema) {
        return;
    }
    let jobs: Vec<crate::ast::ScheduledJob> = schema
        .rule_register
        .triggers
        .iter()
        .filter(|t| t.when == Some(TriggerWhen::Scheduled) && t.problems.is_empty())
        .filter_map(|t| {
            let model = t.bound_model.as_ref()?;
            let snake = to_snake_case(model);
            Some(crate::ast::ScheduledJob {
                name: format!("{snake}_{}", crate::rules::trigger_compile::ident(&t.key)),
                schedule: t.schedule.clone().unwrap_or_default(),
                handler: format!(
                    "application::triggers::{snake}_triggers::run_{snake}_scheduled({:?})",
                    crate::rules::trigger_compile::ident(&t.key)
                ),
                posture: t
                    .posture
                    .as_deref()
                    .and_then(|p| serde_yaml::from_str(p).ok()),
                triggers: Vec::new(),
                commit_policy: t
                    .commit_policy
                    .as_deref()
                    .and_then(|p| serde_yaml::from_str(p).ok()),
                pickup_lock: t.pickup_lock,
            })
        })
        .collect();
    for job in jobs {
        if !schema.scheduled_jobs.iter().any(|j| j.name == job.name) {
            schema.scheduled_jobs.push(job);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Field, PrimitiveType, TypeRef};

    fn schema(hook: &str) -> ModuleSchema {
        let mut schema = ModuleSchema::new("shop");
        let mut ticket = Model::new("Ticket");
        ticket.schema = Some("shop".into());
        for (name, ty) in [
            ("id", PrimitiveType::Uuid),
            ("title", PrimitiveType::String),
            ("status", PrimitiveType::String),
            ("attempts", PrimitiveType::Int),
            ("email", PrimitiveType::String),
            ("is_primary", PrimitiveType::Bool),
        ] {
            ticket.fields.push(Field::new(name, TypeRef::Primitive(ty)));
        }
        schema.models.push(ticket);
        schema.rule_register.read_file("ticket.hook.yaml", hook);
        let models = schema.models.clone();
        schema.rule_register.bind(&models);
        schema.rule_register.check(&models);
        schema
            .rule_register
            .mark_compiled(&models, &Default::default());
        schema
    }

    const HOOK: &str = "name: Ticket\nmodel: Ticket\nstates:\n  field: status\n  values:\n    open: { initial: true }\n    closed:\n      on_enter:\n        - \"log(ticket.closed, { id: id })\"\n        - \"trigger: archive_later\"\n  transitions:\n    close: { from: open, to: closed }\nevents:\n  TicketOpened:\n    aggregate: Ticket\n    fields: { ticket_id: uuid, title: string }\ntriggers:\n  before_create:\n    actions:\n      - \"set: status = open\"\n      - \"trigger: apply_defaults\"\n  after_create:\n    actions:\n      - \"emit: TicketOpened(ticket_id: id, title: title)\"\n      - \"send_email(welcome, email)\"\n  after_update_is_primary_true:\n    if: \"status == 'open'\"\n    actions: [\"log(primary)\"]\n  after_close:\n    actions: [\"trigger: notify_owner\"]\n  after_login:\n    actions: [\"log(login, { id: id })\", \"trigger: bump\"]\n  sweep:\n    schedule: \"every 5.minutes\"\n    description: sweeps stale tickets\n    actions: [\"update: Ticket(status: 'x') WHERE id == id\"]\n";

    #[test]
    fn the_trait_requires_one_method_per_hand_action_with_no_default_body() {
        let s = schema(HOOK);
        assert!(
            s.rule_register
                .triggers
                .iter()
                .all(|t| t.problems.is_empty()),
            "{:?}",
            s.rule_register
                .triggers
                .iter()
                .map(|t| &t.problems)
                .collect::<Vec<_>>()
        );
        let file = trait_file(&s, &s.models[0]).expect("a model with triggers");
        for expected in [
            "pub trait TicketTriggers: Send + Sync {",
            "async fn before_create__apply_defaults(&self, ctx: &WriteCtx, e: &mut Ticket) -> ServiceResult<()>;",
            "async fn after_close__notify_owner(&self, ctx: &WriteCtx, old: &Ticket, e: &Ticket) -> ServiceResult<()>;",
            "async fn on_enter_closed__archive_later(&self, ctx: &WriteCtx, old: &Ticket, e: &Ticket) -> ServiceResult<()>;",
            "async fn after_login__bump(&self, ctx: &WriteCtx, e: &Ticket) -> ServiceResult<()>;",
            "async fn scheduled__sweep(&self, ctx: &WriteCtx) -> ServiceResult<()>;",
            "pub const TICKET_SCHEDULED: &[backbone_core::write_hooks::ScheduledTrigger]",
            "name: \"sweep\", schedule: \"every 5.minutes\", description: \"sweeps stale tickets\"",
            "pub async fn run_ticket_scheduled(hand: &dyn TicketTriggers, name: &str)",
            "\"sweep\" => hand.scheduled__sweep(&ctx).await?,",
        ] {
            assert!(file.contains(expected), "{expected}\n---\n{file}");
        }
        assert!(
            !file.contains("\npub struct TicketTriggersImpl;"),
            "hand methods are required, so the implementation is hand code"
        );
        assert!(
            !file.contains("-> ServiceResult<()> {\n"),
            "no method has a default body"
        );
        assert!(
            file.contains("#[allow(non_snake_case)]\n#[async_trait]\npub trait TicketTriggers"),
            "the `<key>__<action>` names must not warn"
        );
    }

    #[test]
    fn a_model_whose_actions_all_compile_gets_an_empty_default_implementation() {
        let s = schema("name: Ticket\nmodel: Ticket\ntriggers:\n  after_create:\n    actions: [\"log(made)\"]\n");
        let file = trait_file(&s, &s.models[0]).unwrap();
        assert!(
            file.contains("pub struct TicketTriggersImpl;")
                && file.contains("impl TicketTriggers for TicketTriggersImpl {}"),
            "{file}"
        );
        assert!(hand_methods(&s, &s.models[0]).is_empty());
        assert!(mod_file(&s).unwrap().contains("pub mod ticket_triggers;"));
    }

    #[test]
    fn the_hooks_run_triggers_by_what_changed_in_file_order() {
        let s = schema(HOOK);
        let code = hook_code(&s, &s.models[0]);
        for expected in [
            "if matches!(change, Change::Created { .. }) {",
            "entity.status = serde_json::from_value(r::to_json(&r::text(\"open\")))",
            "self.triggers.before_create__apply_defaults(write, entity).await?;",
        ] {
            assert!(
                code.before.contains(expected),
                "{expected}\n---\n{}",
                code.before
            );
        }
        // Order: the set, then the hand method.
        assert!(
            code.before.find("entity.status =").unwrap()
                < code.before.find("before_create__apply_defaults").unwrap()
        );
        for expected in [
            "let event: declared::TicketOpened = serde_json::from_value",
            "write.stage(event.record_for(&aggregate_id, company, write)).await?;",
            "let request = NotificationRequested {",
            "channel: \"email\".to_string(),",
            "matches!(change, Change::Updated { .. } | Change::Transitioned { .. }) && change.field_changed(\"is_primary\") && r::holds(&r::eq(r::field(row, \"is_primary\"), r::Val::Bool(true))) && r::holds(&",
            "matches!(change, Change::Transitioned { transition, .. } if transition == \"close\")",
            "self.triggers.after_close__notify_owner(write, change.before().unwrap_or(subject), subject).await?;",
            "matches!(change, Change::Transitioned { to: Some(to), .. } if to == \"closed\")",
        ] {
            assert!(code.after.contains(expected), "{expected}\n---\n{}", code.after);
        }
        assert!(
            code.fires.contains(
                "pub async fn fire_after_login(&self, write: &WriteCtx, subject: &Ticket)"
            ),
            "{}",
            code.fires
        );
        assert!(
            code.fires
                .contains("self.triggers.after_login__bump(write, subject).await?;"),
            "{}",
            code.fires
        );
        assert!(code.reads.old == false);
    }

    #[test]
    fn a_module_on_legacy_triggers_generates_nothing() {
        let mut s = schema(HOOK);
        s.generators_config = Some(crate::parser::yaml_parser::GeneratorsConfig {
            legacy_triggers: Some(true),
            ..Default::default()
        });
        assert!(model_triggers(&s, &s.models[0]).is_empty());
        assert!(trait_file(&s, &s.models[0]).is_none());
        assert!(mod_file(&s).is_none());
    }

    #[test]
    fn scheduled_entries_become_module_jobs() {
        let mut s = schema(HOOK);
        scheduled_jobs(&mut s);
        assert_eq!(s.scheduled_jobs.len(), 1);
        assert_eq!(s.scheduled_jobs[0].name, "ticket_sweep");
        assert_eq!(s.scheduled_jobs[0].schedule, "every 5.minutes");
    }
}
