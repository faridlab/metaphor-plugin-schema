//! Declared events: the `events:` a schema file lists, and the optional `on:` and `if:` keys
//! that make the generated write hooks stage them.
//!
//! An event is declared under a top-level `events:` of a hook file, the model index or a
//! module config file, as a map keyed by name or as a list of `{ name: … }` entries. Three
//! shapes exist in the wild and all are read:
//!
//! ```yaml
//! events:
//!   QuotationAccepted:                       # prose and names
//!     when: a quotation is accepted
//!     payload: [quotation_id, company_id]
//!   UserCreatedEvent:                        # typed fields, as a map
//!     fields: { user_id: uuid, email: string }
//!   QuotaExceeded:                           # the full form
//!     aggregate: UserQuota
//!     version: 1
//!     storage: { store: true, retention: 1_year }
//!     fields: [{ name: user_id, type: Uuid }]
//! ```
//!
//! A declaration with no `on:` keeps its meaning: a typed payload struct and a helper hand code
//! calls to stage it. Nothing stages it for the module, and `schema rules` says so
//! (`EVENT HAND-STAGED`). With `on:` the generated write hooks stage it in the write's own
//! transaction (`EVENT STAGED`):
//!
//! ```yaml
//! events:
//!   TicketClosed:
//!     aggregate: Ticket
//!     on: transition(close)          # create | update | update(<field>) | delete
//!                                    # | transition(<verb>) | enter(<state>)
//!     if: "priority == 'high'"       # optional: the rule expression language
//!     fields: [id, priority]         # names of the aggregate's own fields
//! ```
//!
//! `on:` takes one trigger or a list. The register checks every name it uses against the model,
//! its machines and their states, so a declaration cannot promise an event nothing stages.

use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};

use super::expr::ExpressionCheck;
use super::SourceRef;

/// When a declared event is staged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventTrigger {
    /// A row of the aggregate is created.
    Create,
    /// A generic write changes a row of the aggregate.
    Update,
    /// A generic write changes the named field.
    UpdateField(String),
    /// A row of the aggregate is deleted (moved to the trash).
    Delete,
    /// A state-machine verb moves a row of the aggregate.
    Transition(String),
    /// A state-machine verb moves a row into the named state.
    Enter(String),
}

impl EventTrigger {
    /// Parse one trigger as written in `on:`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let (head, arg) = match text.split_once('(') {
            Some((head, rest)) => {
                let arg = rest
                    .strip_suffix(')')
                    .ok_or_else(|| format!("`{text}` is missing its closing parenthesis"))?;
                (head.trim(), Some(arg.trim()))
            }
            None => (text, None),
        };
        let named = |arg: Option<&str>, what: &str| -> Result<String, String> {
            match arg {
                Some(a) if !a.is_empty() => Ok(a.to_string()),
                _ => Err(format!("`{head}(…)` needs {what}, as `{head}(<{what}>)`")),
            }
        };
        match (head, arg) {
            ("create", None) => Ok(Self::Create),
            ("update", None) => Ok(Self::Update),
            ("delete", None) => Ok(Self::Delete),
            ("update", a) => Ok(Self::UpdateField(named(a, "field")?)),
            ("transition", a) => Ok(Self::Transition(named(a, "verb")?)),
            ("enter", a) => Ok(Self::Enter(named(a, "state")?)),
            _ => Err(format!(
                "`{text}` is not a trigger: use create, update, update(<field>), delete, \
                 transition(<verb>) or enter(<state>)"
            )),
        }
    }

    /// The trigger as written.
    pub fn text(&self) -> String {
        match self {
            Self::Create => "create".into(),
            Self::Update => "update".into(),
            Self::UpdateField(f) => format!("update({f})"),
            Self::Delete => "delete".into(),
            Self::Transition(v) => format!("transition({v})"),
            Self::Enter(s) => format!("enter({s})"),
        }
    }
}

/// One field of a declared event's payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventField {
    pub name: String,
    /// The type as written (`uuid`, `string?`, `Uuid`, `DateTime<Utc>`); `None` when the
    /// declaration only names the field, which then takes the aggregate's own type.
    pub ty: Option<String>,
}

/// How a declared event reaches the outbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventStatus {
    /// The generated write hooks stage it in the write's transaction (it has `on:`).
    Staged,
    /// Hand code stages it; the generator only supplies its typed payload.
    HandStaged,
}

/// One event as a schema file declares it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeclaredEvent {
    pub source: SourceRef,
    pub name: String,
    /// The model the file names (a hook file's `name:` or `model:`), tried when the event names
    /// no `aggregate:`. Empty for a declaration in a file that names none.
    pub target: String,
    pub alternatives: Vec<String>,
    /// The aggregate as written.
    pub aggregate: Option<String>,
    /// The model it belongs to, once [`RuleRegister::bind`](super::RuleRegister::bind) has found it.
    pub bound_model: Option<String>,
    pub description: Option<String>,
    /// The declared payload version (1 when it declares none).
    pub version: i32,
    pub fields: Vec<EventField>,
    /// The prose `when:` of the older declaration shape, kept as documentation.
    pub when_text: Option<String>,
    /// The triggers, parsed; empty when the event has no `on:`.
    pub on: Vec<EventTrigger>,
    /// The triggers as written, so a malformed one can be reported.
    pub on_text: Vec<String>,
    /// The `if:` condition, as written.
    pub condition: Option<String>,
    /// Whether `if:` parses and resolves against the aggregate; set by [`check_events`].
    pub condition_check: ExpressionCheck,
    /// Keys the declaration carries that nothing enforces (`storage`, `retention`, `migrations`),
    /// kept as documentation and reported by `schema doctor`.
    pub not_enforced: Vec<String>,
    /// Why the declaration cannot be generated, set by [`check_events`]. A staged event with a
    /// problem fails validation; one that is only hand-staged is never a problem.
    pub problems: Vec<String>,
}

impl DeclaredEvent {
    /// Whether the generated hooks stage this event, or hand code does.
    pub fn status(&self) -> EventStatus {
        if self.on_text.is_empty() {
            EventStatus::HandStaged
        } else {
            EventStatus::Staged
        }
    }
}

/// What one file's `events:` declares.
pub fn read_events(
    file: &str,
    root: &Mapping,
    target: Option<&str>,
    alternatives: &[String],
) -> Vec<DeclaredEvent> {
    fn get<'a>(m: &'a Mapping, key: &str) -> Option<&'a Value> {
        m.get(Value::String(key.to_string()))
    }
    let entries: Vec<(String, Value)> = match get(root, "events") {
        Some(Value::Mapping(map)) => map
            .iter()
            .filter_map(|(k, v)| Some((k.as_str()?.to_string(), v.clone())))
            .collect(),
        Some(Value::Sequence(items)) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(name) => Some((name.clone(), Value::Null)),
                Value::Mapping(m) => {
                    let name = get(m, "name")?.as_str()?.to_string();
                    Some((name, item.clone()))
                }
                _ => None,
            })
            .collect(),
        _ => return Vec::new(),
    };
    entries
        .into_iter()
        .map(|(name, body)| read_event(file, name, &body, target, alternatives))
        .collect()
}

fn read_event(
    file: &str,
    name: String,
    body: &Value,
    target: Option<&str>,
    alternatives: &[String],
) -> DeclaredEvent {
    let empty = Mapping::new();
    let map = body.as_mapping().unwrap_or(&empty);
    let key = |k: &str| map.get(Value::String(k.to_string()));
    let text = |k: &str| key(k).and_then(Value::as_str).map(str::to_string);

    let mut fields: Vec<EventField> = Vec::new();
    let mut add = |field: EventField| {
        if !fields.iter().any(|f| f.name == field.name) {
            fields.push(field);
        }
    };
    match key("fields") {
        // `fields: { user_id: uuid, email: { type: string } }`
        Some(Value::Mapping(m)) => {
            for (k, v) in m {
                let Some(name) = k.as_str() else { continue };
                let ty = match v {
                    Value::String(t) => Some(t.clone()),
                    Value::Mapping(inner) => inner
                        .get(Value::String("type".into()))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    _ => None,
                };
                add(EventField {
                    name: name.to_string(),
                    ty,
                });
            }
        }
        // `fields: [id, project_id]` or `fields: [{ name: user_id, type: Uuid }]`
        Some(Value::Sequence(items)) => {
            for item in items {
                match item {
                    Value::String(name) => add(EventField {
                        name: name.clone(),
                        ty: None,
                    }),
                    Value::Mapping(m) => {
                        let Some(name) =
                            m.get(Value::String("name".into())).and_then(Value::as_str)
                        else {
                            continue;
                        };
                        let ty = m
                            .get(Value::String("type".into()))
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        add(EventField {
                            name: name.to_string(),
                            ty,
                        });
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    // `payload: [quotation_id, company_id]`
    if let Some(Value::Sequence(items)) = key("payload") {
        for name in items.iter().filter_map(Value::as_str) {
            add(EventField {
                name: name.to_string(),
                ty: None,
            });
        }
    }

    let on_text: Vec<String> = match key("on") {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Sequence(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    };
    let on = on_text
        .iter()
        .filter_map(|t| EventTrigger::parse(t).ok())
        .collect();
    let not_enforced = ["storage", "retention", "migrations"]
        .iter()
        .filter(|k| key(k).is_some())
        .map(|k| k.to_string())
        .collect();

    DeclaredEvent {
        source: SourceRef {
            file: file.to_string(),
            line: None,
        },
        name,
        target: target.unwrap_or_default().to_string(),
        alternatives: alternatives.to_vec(),
        aggregate: text("aggregate"),
        bound_model: None,
        description: text("description"),
        version: key("version")
            .and_then(Value::as_i64)
            .filter(|v| *v > 0)
            .unwrap_or(1) as i32,
        fields,
        when_text: text("when"),
        on,
        on_text,
        condition: text("if"),
        condition_check: ExpressionCheck::NotAnExpression,
        not_enforced,
        problems: Vec::new(),
    }
}

/// Check every staged event against the model it belongs to: the aggregate exists, `on:` names
/// real fields, verbs and states, `if:` parses and resolves, and every payload field is a field
/// of the aggregate (the hooks read it from the row). Each failure is recorded as a problem on
/// the event, and fails validation when the event is staged.
pub fn check_events(
    register: &mut super::RuleRegister,
    models: &[crate::ast::Model],
    enums: &std::collections::HashSet<String>,
) {
    let machines = register.machines.clone();
    for event in &mut register.events {
        event.problems.clear();
        event.condition_check = ExpressionCheck::NotAnExpression;
        let staged = event.status() == EventStatus::Staged;
        let bad = event
            .on_text
            .iter()
            .filter_map(|t| EventTrigger::parse(t).err())
            .collect::<Vec<_>>();
        event.problems.extend(bad);
        if !staged {
            if event.condition.is_some() {
                event.problems.push(format!(
                    "event `{}` has an `if:` but no `on:`: the condition only decides whether a triggered event is staged",
                    event.name
                ));
            }
            continue;
        }
        let Some(model) = event
            .bound_model
            .as_ref()
            .and_then(|n| models.iter().find(|m| &m.name == n))
        else {
            event.problems.push(format!(
                "event `{}` has `on:` but names no model of this module: give it an `aggregate:`",
                event.name
            ));
            continue;
        };
        for trigger in &event.on {
            match trigger {
                EventTrigger::UpdateField(field) if model.find_field(field).is_none() => {
                    event.problems.push(format!(
                        "event `{}` is staged `on: update({field})`, but {} has no field `{field}`",
                        event.name, model.name
                    ))
                }
                EventTrigger::Transition(verb) => {
                    let known = machines
                        .iter()
                        .filter(|m| m.bound_model.as_deref() == Some(model.name.as_str()))
                        .any(|m| {
                            m.transitions
                                .iter()
                                .any(|t| t.verb.as_deref() == Some(verb.as_str()))
                        });
                    if !known {
                        event.problems.push(format!(
                            "event `{}` is staged `on: transition({verb})`, but no state machine of {} has a verb `{verb}`",
                            event.name, model.name
                        ));
                    }
                }
                EventTrigger::Enter(state) => {
                    let known = machines
                        .iter()
                        .filter(|m| m.bound_model.as_deref() == Some(model.name.as_str()))
                        .any(|m| m.states.contains(state));
                    if !known {
                        event.problems.push(format!(
                            "event `{}` is staged `on: enter({state})`, but no state machine of {} has a state `{state}`",
                            event.name, model.name
                        ));
                    }
                }
                _ => {}
            }
        }
        if event
            .on
            .iter()
            .any(|t| matches!(t, EventTrigger::Transition(_) | EventTrigger::Enter(_)))
            && !machines
                .iter()
                .any(|m| m.bound_model.as_deref() == Some(model.name.as_str()))
        {
            event.problems.push(format!(
                "event `{}` is staged on a transition, but {} declares no state machine",
                event.name, model.name
            ));
        }
        for field in &event.fields {
            if model.find_field(&field.name).is_none() {
                event.problems.push(format!(
                    "event `{}` is staged by the write hooks, which read its payload from the row, but {} has no field `{}`",
                    event.name, model.name, field.name
                ));
            }
        }
        if let Some(condition) = &event.condition {
            event.condition_check = super::expr::check(condition, Some(model));
            if event.condition_check != ExpressionCheck::Ready {
                event.problems.push(format!(
                    "event `{}` has an `if:` the rule language cannot compile ({}): {condition}",
                    event.name,
                    event.condition_check.label()
                ));
            } else if let Err(why) =
                super::compile::compile_event_condition(condition, model, enums)
            {
                event.condition_check = ExpressionCheck::Unresolved {
                    names: Vec::new(),
                    functions: Vec::new(),
                };
                event.problems.push(format!(
                    "event `{}` has an `if:` the hooks cannot evaluate ({why}): {condition}",
                    event.name
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Field, Model, PrimitiveType, TypeRef};
    use crate::rules::RuleRegister;

    fn ticket() -> Model {
        let mut m = Model::new("Ticket");
        for (name, ty) in [
            ("id", PrimitiveType::Uuid),
            ("title", PrimitiveType::String),
            ("priority", PrimitiveType::String),
        ] {
            m.fields.push(Field::new(name, TypeRef::Primitive(ty)));
        }
        m
    }

    const HOOK: &str = "name: Ticket\nmodel: Ticket\nstates:\n  field: status\n  values:\n    open: { initial: true }\n    closed: {}\n  transitions:\n    close: { from: open, to: closed }\nevents:\n  TicketClosed:\n    on: transition(close)\n    if: \"priority == 'high'\"\n    fields: [id, priority]\n";

    fn register(hook: &str) -> RuleRegister {
        let mut r = RuleRegister::default();
        r.read_file("ticket.hook.yaml", hook);
        let mut m = ticket();
        m.fields.push(Field::new(
            "status",
            TypeRef::Primitive(PrimitiveType::String),
        ));
        let models = vec![m];
        r.bind(&models);
        r.check(&models);
        r.mark_compiled(&models, &Default::default());
        r
    }

    #[test]
    fn a_trigger_is_one_of_six_forms() {
        assert_eq!(EventTrigger::parse("create"), Ok(EventTrigger::Create));
        assert_eq!(
            EventTrigger::parse(" update( status ) "),
            Ok(EventTrigger::UpdateField("status".into()))
        );
        assert_eq!(
            EventTrigger::parse("transition(close)"),
            Ok(EventTrigger::Transition("close".into()))
        );
        assert_eq!(
            EventTrigger::parse("enter(closed)"),
            Ok(EventTrigger::Enter("closed".into()))
        );
        assert_eq!(EventTrigger::parse("delete"), Ok(EventTrigger::Delete));
        assert!(EventTrigger::parse("transition()").is_err());
        assert!(EventTrigger::parse("restore").is_err());
        assert!(EventTrigger::parse("update(status").is_err());
    }

    #[test]
    fn the_three_declaration_shapes_in_use_all_read() {
        let doc = "events:\n  QuotationAccepted:\n    when: a quotation is accepted\n    payload: [quotation_id, company_id]\n  UserCreatedEvent:\n    fields: { user_id: uuid, email: { type: string } }\n  QuotaExceeded:\n    description: over quota\n    aggregate: UserQuota\n    version: 2\n    storage: { store: true, retention: 1_year }\n    fields: [{ name: user_id, type: Uuid }]\n";
        let root = serde_yaml::from_str::<Value>(doc).unwrap();
        let events = read_events("x.yaml", root.as_mapping().unwrap(), None, &[]);
        assert_eq!(events.len(), 3);
        assert_eq!(
            events[0]
                .fields
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            ["quotation_id", "company_id"]
        );
        assert_eq!(
            events[0].when_text.as_deref(),
            Some("a quotation is accepted")
        );
        assert_eq!(
            events[1].fields[1],
            EventField {
                name: "email".into(),
                ty: Some("string".into())
            }
        );
        assert_eq!(
            (events[2].version, events[2].aggregate.as_deref()),
            (2, Some("UserQuota"))
        );
        assert_eq!(events[2].not_enforced, ["storage"]);
        assert!(events.iter().all(|e| e.status() == EventStatus::HandStaged));
        // The list form, with plain names among the entries.
        let doc = "events:\n  - name: TicketOpened\n    fields: [id, project_id]\n  - Plain\n";
        let root = serde_yaml::from_str::<Value>(doc).unwrap();
        let events = read_events("x.yaml", root.as_mapping().unwrap(), None, &[]);
        assert_eq!(
            events.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["TicketOpened", "Plain"]
        );
    }

    #[test]
    fn a_declaration_that_names_real_things_is_staged_without_problems() {
        let r = register(HOOK);
        let e = &r.events[0];
        assert_eq!(e.status(), EventStatus::Staged);
        assert_eq!(e.bound_model.as_deref(), Some("Ticket"));
        assert_eq!(e.on, vec![EventTrigger::Transition("close".into())]);
        assert!(e.problems.is_empty(), "{:?}", e.problems);
    }

    #[test]
    fn every_name_an_on_or_if_uses_is_checked_against_the_model() {
        let bad = HOOK
            .replace(
                "transition(close)",
                "[transition(shut), enter(void), update(nope)]",
            )
            .replace("priority == 'high'", "colour == 'red'")
            .replace("[id, priority]", "[id, owner]");
        let r = register(&bad);
        let problems = r.events[0].problems.join("\n");
        for needle in [
            "verb `shut`",
            "state `void`",
            "no field `nope`",
            "no field `owner`",
            "`if:`",
        ] {
            assert!(problems.contains(needle), "{needle} not in:\n{problems}");
        }
        // A staged event with no aggregate fails too; a hand-staged one never does.
        let r = register(
            "module: m\nevents:\n  Lonely:\n    on: create\n  Hand:\n    fields: [whatever]\n",
        );
        assert!(
            r.events[0].problems.iter().any(|p| p.contains("aggregate")),
            "{:?}",
            r.events[0].problems
        );
        assert!(r.events[1].problems.is_empty());
    }

    #[test]
    fn an_if_without_an_on_is_a_mistake() {
        let r = register(
            "name: Ticket\nmodel: Ticket\nevents:\n  Orphan:\n    if: \"priority == 'high'\"\n",
        );
        assert!(r.events[0]
            .problems
            .iter()
            .any(|p| p.contains("`if:` but no `on:`")));
    }
}
