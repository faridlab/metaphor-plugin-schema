//! Declared events: the typed payload of every event a module declares, and the write hooks that
//! stage the ones declared with `on:`.
//!
//! Two kinds of generated file, both written whole (nothing in them is hand-edited):
//!
//! - `src/domain/event/declared.rs`, for a module that declares any event: one payload struct per
//!   declaration, namespaced under `declared::` so it can never collide with a hand-written event
//!   type, each implementing `backbone_core::write_hooks::DeclaredEvent`; plus the Postgres
//!   schemas the module's events are staged in, and `migrate_outbox` for the composing service
//!   to create their tables with at startup. A declaration with no `on:` is only this: the
//!   struct and the helper hand code stages it with.
//! - `src/application/service/<model>_write_hooks.rs`, for a model some event is declared on with
//!   `on:`: a `WriteHooks` implementation that stages each such event in the write's own
//!   transaction, in declaration order, after the row is written. The generated service
//!   constructor attaches it.
//!
//! The declarations themselves are read and checked by [`crate::rules::events`].

use std::collections::HashSet;
use std::fmt::Write;

use crate::ast::{Model, ModuleSchema, PrimitiveType, TypeRef};
use crate::rules::events::{DeclaredEvent, EventTrigger};

/// The events to generate: the first declaration of each name whose name is a Rust identifier.
pub fn events_of(schema: &ModuleSchema) -> Vec<&DeclaredEvent> {
    let mut seen = HashSet::new();
    schema
        .rule_register
        .events
        .iter()
        .filter(|e| is_ident(&e.name))
        .filter(|e| seen.insert(e.name.as_str()))
        .collect()
}

/// The staged events of `model`, in declaration order.
pub fn staged_events<'a>(schema: &'a ModuleSchema, model: &Model) -> Vec<&'a DeclaredEvent> {
    events_of(schema)
        .into_iter()
        .filter(|e| {
            !e.on.is_empty()
                && e.problems.is_empty()
                && e.bound_model.as_deref() == Some(model.name.as_str())
        })
        .collect()
}

/// Whether the generated service of `model` carries write hooks: it has a declared event the hooks
/// stage, or a hook trigger they run.
pub fn model_has_write_hooks(schema: &ModuleSchema, model: &Model) -> bool {
    !staged_events(schema, model).is_empty() || super::triggers::model_has_triggers(schema, model)
}

/// Whether the hooks of `model` may stage an outbox row: a declared event, or a trigger that emits,
/// notifies or has a hand method (which may stage by hand).
pub fn model_stages(schema: &ModuleSchema, model: &Model) -> bool {
    !staged_events(schema, model).is_empty() || super::triggers::model_triggers_stage(schema, model)
}

/// The Postgres schema `model`'s events are staged in: the schema of its table.
pub fn outbox_schema_of(model: &Model) -> String {
    model
        .schema
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "public".to_string())
}

/// The distinct outbox schemas the module's staged events use, sorted.
pub fn outbox_schemas(schema: &ModuleSchema) -> Vec<String> {
    let mut out: Vec<String> = schema
        .models
        .iter()
        .filter(|m| model_stages(schema, m))
        .map(outbox_schema_of)
        .collect();
    out.sort();
    out.dedup();
    out
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "try"
            | "gen"
    )
}

const VALUE: &str = "serde_json::Value";

/// The Rust type a declared type name stands for, or `None` when it is not one the generator
/// knows (an enum, a type of another module): that field is then carried as a JSON value.
pub fn rust_type_of_declared(ty: &str) -> Option<String> {
    let t = ty.trim();
    if let Some(inner) = t.strip_suffix('?') {
        return rust_type_of_declared(inner).map(|i| format!("Option<{i}>"));
    }
    if let Some(inner) = t.strip_suffix("[]") {
        return rust_type_of_declared(inner).map(|i| format!("Vec<{i}>"));
    }
    for (wrapper, rust) in [("Option<", "Option"), ("Vec<", "Vec")] {
        if let Some(inner) = t.strip_prefix(wrapper).and_then(|r| r.strip_suffix('>')) {
            return rust_type_of_declared(inner).map(|i| format!("{rust}<{i}>"));
        }
    }
    let lower = t.to_ascii_lowercase();
    Some(
        match lower.as_str() {
            "uuid" => "uuid::Uuid",
            "string" | "text" | "email" | "url" | "phone" | "slug" | "str" => "String",
            "int" | "int32" | "i32" => "i32",
            "int64" | "i64" | "bigint" => "i64",
            "u32" => "u32",
            "u64" => "u64",
            "float" | "float64" | "f64" | "double" => "f64",
            "float32" | "f32" => "f32",
            "bool" | "boolean" => "bool",
            "datetime" | "timestamp" | "datetime<utc>" | "chrono::datetime<chrono::utc>" => {
                "chrono::DateTime<chrono::Utc>"
            }
            "date" | "naivedate" => "chrono::NaiveDate",
            "time" | "naivetime" => "chrono::NaiveTime",
            "decimal" | "money" | "percentage" => "rust_decimal::Decimal",
            "json" | "value" | "serde_json::value" => VALUE,
            _ => return None,
        }
        .to_string(),
    )
}

/// The Rust type of a model field, as an event payload carries it. Types an event cannot spell
/// without importing the module's own (enums, relations) are carried as a JSON value.
fn rust_type_of_field(type_ref: &TypeRef) -> String {
    match type_ref {
        TypeRef::Optional(inner) => format!("Option<{}>", rust_type_of_field(inner)),
        TypeRef::Array(inner) => format!("Vec<{}>", rust_type_of_field(inner)),
        TypeRef::Primitive(p) => match p {
            PrimitiveType::Uuid => "uuid::Uuid".into(),
            PrimitiveType::DateTime | PrimitiveType::Timestamp => {
                "chrono::DateTime<chrono::Utc>".into()
            }
            PrimitiveType::Date => "chrono::NaiveDate".into(),
            PrimitiveType::Time => "chrono::NaiveTime".into(),
            PrimitiveType::Decimal | PrimitiveType::Money | PrimitiveType::Percentage => {
                "rust_decimal::Decimal".into()
            }
            PrimitiveType::Duration => VALUE.into(),
            other => other.rust_type().to_string(),
        },
        _ => VALUE.into(),
    }
}

/// One payload field of a generated struct.
struct PayloadField {
    name: String,
    ty: String,
}

/// The payload of `event`: its declared fields, typed. A staged event reads its payload out of the
/// aggregate's row, so its fields take the aggregate's own types; a hand-staged one takes the
/// type it declares, else the aggregate's, else a JSON value. A field whose name is not an
/// identifier is left out.
fn payload_of(schema: &ModuleSchema, event: &DeclaredEvent) -> Vec<PayloadField> {
    let model = event
        .bound_model
        .as_ref()
        .and_then(|n| schema.models.iter().find(|m| &m.name == n));
    let staged = !event.on.is_empty();
    event
        .fields
        .iter()
        .filter(|f| is_ident(&f.name))
        .map(|f| {
            let from_model = model
                .and_then(|m| m.find_field(&f.name))
                .map(|field| rust_type_of_field(&field.type_ref));
            let declared = f.ty.as_deref().and_then(rust_type_of_declared);
            let ty = match (staged, from_model, declared) {
                (true, Some(model_type), _) => model_type,
                (false, _, Some(declared)) => declared,
                (false, Some(model_type), None) if f.ty.is_none() => model_type,
                _ => VALUE.to_string(),
            };
            PayloadField {
                name: f.name.clone(),
                ty,
            }
        })
        .collect()
}

/// Fields of a hand-staged event whose type the generator could not spell, as warnings for
/// `schema doctor`: they are carried as JSON values.
pub fn untyped_payload_fields(schema: &ModuleSchema) -> Vec<String> {
    let mut out = Vec::new();
    for event in events_of(schema).into_iter().filter(|e| e.on.is_empty()) {
        for (field, payload) in event
            .fields
            .iter()
            .filter(|f| is_ident(&f.name))
            .zip(payload_of(schema, event))
        {
            if payload.ty == VALUE
                && !field
                    .ty
                    .as_deref()
                    .is_some_and(|t| rust_type_of_declared(t).is_some())
            {
                out.push(format!(
                    "{}: event `{}` field `{}` has no type the generator knows ({}); it is carried as a JSON value",
                    event.source,
                    event.name,
                    field.name,
                    match (&field.ty, &event.bound_model) {
                        (Some(t), _) => format!("`{t}`"),
                        (None, Some(m)) => format!("{m} has no such field"),
                        (None, None) => "the event names no aggregate to take it from".to_string(),
                    }
                ));
            }
        }
    }
    out
}

pub(crate) fn ident(name: &str) -> String {
    if is_keyword(name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

/// `src/domain/event/declared.rs`, or `None` when the module declares no event.
pub fn declared_module(schema: &ModuleSchema) -> Option<String> {
    let events = events_of(schema);
    if events.is_empty() {
        return None;
    }
    let mut out = String::new();
    out.push_str(
        "//! The events this module declares, as typed payloads (`events:` in its schema files).\n\
         //!\n\
         //! Generated by metaphor-schema. Do not edit manually.\n\
         //!\n\
         //! Each declaration is a struct here, namespaced under `declared::` so it cannot collide with a\n\
         //! hand-written event type, and implements `DeclaredEvent`: its name, version and aggregate, and\n\
         //! `record(..)`, which builds the outbox row. An event declared with `on:` is staged by the write\n\
         //! hooks of its aggregate, in the write's transaction. One without is staged by hand code:\n\
         //! `ctx.stage(event.record(&id, &ctx))` inside a write's hooks, or\n\
         //! `backbone_outbox::outbox::stage` in a transaction of its own.\n\
         //! Delivery is at least once; a consumer dedupes with `backbone_outbox::inbox::once`.\n\n\
         use backbone_core::write_hooks::DeclaredEvent;\n\
         use serde::{Deserialize, Serialize};\n",
    );
    for event in &events {
        out.push('\n');
        let mut doc: Vec<String> = Vec::new();
        if let Some(d) = event.description.as_ref().or(event.when_text.as_ref()) {
            doc.push(d.trim().to_string());
        }
        doc.push(match event.on_text.as_slice() {
            [] => format!("Declared in `{}`; staged by hand code.", event.source.file),
            on => format!(
                "Declared in `{}`; staged by the write hooks on {}.",
                event.source.file,
                on.join(", ")
            ),
        });
        for line in doc.iter().flat_map(|d| d.lines()) {
            writeln!(out, "/// {line}").unwrap();
        }
        writeln!(
            out,
            "#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]"
        )
        .unwrap();
        let payload = payload_of(schema, event);
        if payload.is_empty() {
            writeln!(out, "pub struct {} {{}}", event.name).unwrap();
        } else {
            writeln!(out, "pub struct {} {{", event.name).unwrap();
            for f in &payload {
                writeln!(out, "    pub {}: {},", ident(&f.name), f.ty).unwrap();
            }
            writeln!(out, "}}").unwrap();
        }
        writeln!(out).unwrap();
        writeln!(out, "impl DeclaredEvent for {} {{", event.name).unwrap();
        writeln!(out, "    const NAME: &'static str = {:?};", event.name).unwrap();
        writeln!(out, "    const VERSION: i32 = {};", event.version).unwrap();
        let aggregate = event
            .bound_model
            .as_deref()
            .or(event.aggregate.as_deref())
            .unwrap_or("");
        writeln!(out, "    const AGGREGATE: &'static str = {aggregate:?};").unwrap();
        writeln!(out, "}}").unwrap();
    }
    let schemas = outbox_schemas(schema);
    out.push_str(
        "\n/// The Postgres schemas whose `outbox_events` table this module's staged events are written to.\n",
    );
    writeln!(
        out,
        "pub const OUTBOX_SCHEMAS: &[&str] = &[{}];",
        schemas
            .iter()
            .map(|s| format!("{s:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    )
    .unwrap();
    out.push_str(
        "\n/// Create the outbox and inbox tables of [`OUTBOX_SCHEMAS`] (idempotent). The composing service\n\
         /// runs it at startup with a role that may create tables, beside its relay; a write that stages an\n\
         /// event needs the table to exist.\n\
         pub async fn migrate_outbox(pool: &backbone_orm::PgPool) -> anyhow::Result<()> {\n\
         \x20   for schema in OUTBOX_SCHEMAS {\n\
         \x20       backbone_core::write_hooks::migrate_outbox(pool, schema).await?;\n\
         \x20   }\n\
         \x20   Ok(())\n\
         }\n",
    );
    Some(out)
}

/// The Rust condition that decides whether `trigger` fires for `change`.
fn trigger_condition(trigger: &EventTrigger) -> String {
    match trigger {
        EventTrigger::Create => "matches!(change, Change::Created { .. })".into(),
        EventTrigger::Update => "matches!(change, Change::Updated { .. } | Change::Transitioned { .. })".into(),
        EventTrigger::UpdateField(field) => format!(
            "matches!(change, Change::Updated {{ .. }} | Change::Transitioned {{ .. }}) && change.field_changed({field:?})"
        ),
        EventTrigger::Delete => "matches!(change, Change::Deleted { hard: false, .. })".into(),
        EventTrigger::Transition(verb) => {
            format!("matches!(change, Change::Transitioned {{ transition, .. }} if transition == {verb:?})")
        }
        EventTrigger::Enter(state) => {
            format!("matches!(change, Change::Transitioned {{ to: Some(to), .. }} if to == {state:?})")
        }
    }
}

/// `src/application/service/<model>_write_hooks.rs`, or `None` when no event is staged on the model
/// and no trigger runs on it.
pub fn model_write_hooks(
    schema: &ModuleSchema,
    model: &Model,
    enums: &HashSet<String>,
) -> Option<String> {
    let events = staged_events(schema, model);
    let trig = super::triggers::hook_code(schema, model);
    let has_triggers = super::triggers::model_has_triggers(schema, model);
    if events.is_empty() && !has_triggers {
        return None;
    }
    let name = &model.name;
    let conditions: Vec<Option<crate::rules::compile::CompiledEventCondition>> = events
        .iter()
        .map(|e| {
            e.condition
                .as_deref()
                .and_then(|c| crate::rules::compile::compile_event_condition(c, model, enums).ok())
        })
        .collect();
    let reads_old = conditions.iter().flatten().any(|c| c.reads_old) || trig.reads.old;
    let reads_context = conditions
        .iter()
        .flatten()
        .any(|c| c.reads_actor || c.reads_time)
        || trig.reads.actor
        || trig.reads.time
        || has_triggers;
    let stages = model_stages(schema, model);

    let mut out = String::new();
    writeln!(
        out,
        "//! The write hooks of {name}: they run, in the write's own transaction, the hook triggers the schema\n\
         //! declares for it and stage the events it declares with `on:`.\n\
         //!\n\
         //! Generated by metaphor-schema. Do not edit manually.\n\
         //!\n\
         //! They run around the repository write: `before` the triggers keyed `before_*`, after the row is written\n\
         //! and its lifecycle hooks have run, the `after_*` triggers in file order and then the declared events,\n\
         //! all before the commit. A failure of any of them rolls the write back, so a committed write always has\n\
         //! its events and the effects of its triggers. Delivery to subscribers is the outbox relay's, at least once."
    )
    .unwrap();
    writeln!(out, "#[allow(unused_imports)]").unwrap();
    if has_triggers {
        writeln!(out, "use backbone_core::write_hooks::{{payload_from_row, Change, DeclaredEvent, NotificationRequested, WriteCtx, WriteHooks}};").unwrap();
    } else {
        writeln!(out, "use backbone_core::write_hooks::{{payload_from_row, Change, DeclaredEvent, WriteCtx, WriteHooks}};").unwrap();
    }
    writeln!(
        out,
        "#[allow(unused_imports)]\nuse backbone_core::{{PersistentEntity, ServiceError, ServiceResult}};"
    )
    .unwrap();
    writeln!(out).unwrap();
    writeln!(out, "use crate::domain::entity::{name};").unwrap();
    writeln!(out, "#[allow(unused_imports)]").unwrap();
    writeln!(out, "use crate::domain::event::declared;").unwrap();
    if has_triggers {
        writeln!(out, "use crate::application::triggers::{name}Triggers;").unwrap();
    }
    writeln!(out).unwrap();
    if reads_context {
        writeln!(
            out,
            "/// What a condition that reads the writer or the time of the write evaluates against.\n\
             #[allow(dead_code)]\n\
             struct EvalCtx {{\n\
             \x20   actor: Option<String>,\n\
             \x20   at: chrono::DateTime<chrono::Utc>,\n\
             }}\n"
        )
        .unwrap();
    }
    if has_triggers {
        writeln!(out, "/// Runs the triggers of {name} and stages its declared events. `triggers` is the hand-written half (`{name}TriggersImpl`).").unwrap();
        writeln!(out, "pub struct {name}WriteHooks {{").unwrap();
        writeln!(out, "    triggers: std::sync::Arc<dyn {name}Triggers>,").unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
        writeln!(out, "impl {name}WriteHooks {{").unwrap();
        writeln!(
            out,
            "    /// The hooks over the hand-written half of the triggers."
        )
        .unwrap();
        writeln!(
            out,
            "    pub fn new(triggers: std::sync::Arc<dyn {name}Triggers>) -> Self {{"
        )
        .unwrap();
        writeln!(out, "        Self {{ triggers }}").unwrap();
        writeln!(out, "    }}").unwrap();
        write!(out, "{}", trig.fires).unwrap();
        writeln!(out, "}}").unwrap();
    } else {
        writeln!(out, "/// Stages the declared events of {name}.").unwrap();
        writeln!(out, "pub struct {name}WriteHooks;").unwrap();
    }
    writeln!(out).unwrap();
    writeln!(out, "#[async_trait::async_trait]").unwrap();
    writeln!(out, "impl WriteHooks<{name}> for {name}WriteHooks {{").unwrap();
    if stages {
        writeln!(out, "    fn outbox_schema(&self) -> Option<&str> {{").unwrap();
        writeln!(out, "        Some({:?})", outbox_schema_of(model)).unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();
    }
    if !trig.before.is_empty() {
        writeln!(out, "    #[allow(unused_variables, unused_assignments)]").unwrap();
        writeln!(
            out,
            "    async fn before(&self, write: &WriteCtx, change: &mut Change<'_, {name}>) -> ServiceResult<()> {{"
        )
        .unwrap();
        writeln!(out, "        use backbone_core::row_rule as r;").unwrap();
        writeln!(
            out,
            "        backbone_core::write_hooks::in_trigger(async {{"
        )
        .unwrap();
        writeln!(out, "            let old_json = change.before().and_then(|b| serde_json::to_value(b).ok());").unwrap();
        writeln!(
            out,
            "            let old_row = old_json.as_ref().unwrap_or(&serde_json::Value::Null);"
        )
        .unwrap();
        writeln!(out, "            let ctx = EvalCtx {{ actor: write.actor.clone(), at: chrono::Utc::now() }};").unwrap();
        writeln!(
            out,
            "            let aggregate_id = change.subject().entity_id();"
        )
        .unwrap();
        writeln!(out, "            let company = write.company_of(&serde_json::to_value(change.subject()).unwrap_or(serde_json::Value::Null));").unwrap();
        writeln!(
            out,
            "            let _ = (&ctx, &old_row, &aggregate_id, company);"
        )
        .unwrap();
        write!(out, "{}", trig.before).unwrap();
        writeln!(out, "            Ok(())").unwrap();
        writeln!(out, "        }})").unwrap();
        writeln!(out, "        .await").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();
    }
    if has_triggers {
        writeln!(out, "    #[allow(unused_variables, unused_assignments)]").unwrap();
    }
    writeln!(
        out,
        "    async fn after(&self, write: &WriteCtx, change: &Change<'_, {name}>) -> ServiceResult<()> {{"
    )
    .unwrap();
    if conditions.iter().any(Option::is_some) || !trig.after.is_empty() {
        writeln!(out, "        use backbone_core::row_rule as r;").unwrap();
    }
    writeln!(out, "        let subject = change.subject();").unwrap();
    writeln!(out, "        let row = serde_json::to_value(subject)").unwrap();
    writeln!(out, "            .map_err(|e| ServiceError::Internal(format!(\"a row could not be read for its events: {{e}}\")))?;").unwrap();
    writeln!(out, "        let row = &row;").unwrap();
    if reads_old {
        writeln!(
            out,
            "        let old = change.before().and_then(|b| serde_json::to_value(b).ok());"
        )
        .unwrap();
        writeln!(
            out,
            "        let old_row = old.as_ref().unwrap_or(&serde_json::Value::Null);"
        )
        .unwrap();
    }
    if reads_context {
        writeln!(
            out,
            "        let ctx = EvalCtx {{ actor: write.actor.clone(), at: chrono::Utc::now() }};"
        )
        .unwrap();
    }
    writeln!(out, "        let aggregate_id = subject.entity_id();").unwrap();
    writeln!(out, "        let company = write.company_of(row);").unwrap();
    if !trig.after.is_empty() {
        writeln!(out, "        let _ = (&aggregate_id, company);").unwrap();
        if reads_context {
            writeln!(out, "        let _ = &ctx;").unwrap();
        }
        writeln!(out).unwrap();
        writeln!(
            out,
            "        backbone_core::write_hooks::in_trigger(async {{"
        )
        .unwrap();
        write!(out, "{}", trig.after).unwrap();
        writeln!(out, "            Ok(())").unwrap();
        writeln!(out, "        }})").unwrap();
        writeln!(out, "        .await?;").unwrap();
    }
    for (event, condition) in events.iter().zip(&conditions) {
        let triggers: Vec<String> = event.on.iter().map(trigger_condition).collect();
        let fires = if triggers.len() == 1 {
            triggers[0].clone()
        } else {
            format!(
                "({})",
                triggers
                    .iter()
                    .map(|t| format!("({t})"))
                    .collect::<Vec<_>>()
                    .join(" || ")
            )
        };
        let when = match condition {
            Some(c) => format!("{fires} && r::holds(&{})", c.rust),
            None => fires,
        };
        let on = event.on_text.join(" | ");
        writeln!(out).unwrap();
        writeln!(
            out,
            "        // `{}`: on {on} ({})",
            event.name, event.source
        )
        .unwrap();
        writeln!(out, "        if {when} {{").unwrap();
        let fields: Vec<String> = payload_of(schema, event)
            .iter()
            .map(|f| format!("{:?}", f.name))
            .collect();
        writeln!(
            out,
            "            let payload: declared::{} = payload_from_row(row, &[{}])?;",
            event.name,
            fields.join(", ")
        )
        .unwrap();
        writeln!(
            out,
            "            write.stage(payload.record_for(&aggregate_id, company, write)).await?;"
        )
        .unwrap();
        writeln!(out, "        }}").unwrap();
    }
    writeln!(out).unwrap();
    writeln!(out, "        Ok(())").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Field;

    fn schema(hook: &str) -> ModuleSchema {
        let mut schema = ModuleSchema::new("shop");
        let mut ticket = Model::new("Ticket");
        ticket.schema = Some("shop".into());
        for (name, ty) in [
            ("id", TypeRef::Primitive(PrimitiveType::Uuid)),
            ("title", TypeRef::Primitive(PrimitiveType::String)),
            ("priority", TypeRef::Primitive(PrimitiveType::String)),
            (
                "amount",
                TypeRef::Optional(Box::new(TypeRef::Primitive(PrimitiveType::Decimal))),
            ),
            ("status", TypeRef::Primitive(PrimitiveType::String)),
        ] {
            ticket.fields.push(Field::new(name, ty));
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

    const HOOK: &str = "name: Ticket\nmodel: Ticket\nstates:\n  field: status\n  values:\n    open: { initial: true }\n    closed: {}\n  transitions:\n    close: { from: open, to: closed }\nevents:\n  TicketOpened:\n    on: create\n    fields: [id, title, amount]\n  TicketClosed:\n    on: [transition(close), update(priority)]\n    if: \"priority == 'high' && $old.status == 'open'\"\n    fields: [id]\n  Hand:\n    when: staged by a verb\n    payload: [id, nope]\n    version: 4\n";

    #[test]
    fn every_declaration_becomes_a_typed_struct_with_its_name_version_and_aggregate() {
        let s = schema(HOOK);
        let declared = declared_module(&s).expect("a module with events");
        for expected in [
            "pub struct TicketOpened {\n    pub id: uuid::Uuid,\n    pub title: String,\n    pub amount: Option<rust_decimal::Decimal>,\n}",
            "impl DeclaredEvent for TicketOpened {\n    const NAME: &'static str = \"TicketOpened\";\n    const VERSION: i32 = 1;\n    const AGGREGATE: &'static str = \"Ticket\";",
            "pub struct TicketClosed {\n    pub id: uuid::Uuid,\n}",
            // A hand-staged declaration with no aggregate field still gets its struct: a name the
            // model has takes its type, one it lacks is a JSON value.
            "const VERSION: i32 = 4;",
            "pub const OUTBOX_SCHEMAS: &[&str] = &[\"shop\"];",
            "pub async fn migrate_outbox(",
        ] {
            assert!(declared.contains(expected), "{expected}\n---\n{declared}");
        }
        assert!(
            declared.contains(
                "pub struct Hand {\n    pub id: uuid::Uuid,\n    pub nope: serde_json::Value,\n}"
            ),
            "{declared}"
        );
    }

    #[test]
    fn the_write_hooks_stage_each_staged_event_when_its_trigger_and_condition_hold() {
        let s = schema(HOOK);
        let hooks = model_write_hooks(&s, &s.models[0], &Default::default())
            .expect("a model with staged events");
        for expected in [
            "pub struct TicketWriteHooks;",
            "impl WriteHooks<Ticket> for TicketWriteHooks",
            "Some(\"shop\")",
            "if matches!(change, Change::Created { .. }) {",
            "payload_from_row(row, &[\"id\", \"title\", \"amount\"])",
            "write.stage(payload.record_for(&aggregate_id, company, write)).await?;",
            "((matches!(change, Change::Transitioned { transition, .. } if transition == \"close\")) || (matches!(change, Change::Updated { .. } | Change::Transitioned { .. }) && change.field_changed(\"priority\")))",
            "r::holds(&r::and(",
            "let old_row = ",
        ] {
            assert!(hooks.contains(expected), "{expected}\n---\n{hooks}");
        }
        // The hand-staged declaration is not in the hooks.
        assert!(!hooks.contains("declared::Hand"));
        assert!(
            !hooks.contains("EvalCtx"),
            "no condition reads the writer or the time"
        );
    }

    #[test]
    fn a_model_with_no_staged_event_gets_no_hooks() {
        let s = schema("name: Ticket\nmodel: Ticket\nevents:\n  Hand:\n    fields: [id]\n");
        assert!(!model_has_write_hooks(&s, &s.models[0]));
        assert!(model_write_hooks(&s, &s.models[0], &Default::default()).is_none());
        // The typed payload is still generated.
        assert!(declared_module(&s).unwrap().contains("pub struct Hand {"));
        assert!(outbox_schemas(&s).is_empty());
    }

    #[test]
    fn declared_types_map_to_rust_and_unknown_ones_to_a_json_value() {
        assert_eq!(rust_type_of_declared("uuid").as_deref(), Some("uuid::Uuid"));
        assert_eq!(
            rust_type_of_declared("string[]").as_deref(),
            Some("Vec<String>")
        );
        assert_eq!(
            rust_type_of_declared("DateTime<Utc>").as_deref(),
            Some("chrono::DateTime<chrono::Utc>")
        );
        assert_eq!(
            rust_type_of_declared("Option<i64>").as_deref(),
            Some("Option<i64>")
        );
        assert_eq!(rust_type_of_declared("TicketStatus"), None);
    }
}
