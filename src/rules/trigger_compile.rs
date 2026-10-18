//! Compile the actions of declared triggers into the steps the generated write hooks run.
//!
//! An action is *compiled* when the generator can write it, and *hand* when only Rust can:
//!
//! | Action | Compiled as | Hand when |
//! |---|---|---|
//! | `emit: Event(a: expr)` | the declared event, built from the arguments and staged on the write's transaction | an argument cannot compile |
//! | `log(name, { k: expr })` | a `tracing::info!` carrying the correlation id | never: a field that cannot compile is left out and said so |
//! | `set: f = expr`, `increment: f` | an assignment to the row about to be stored | not in a `before_*` trigger, or across entities |
//! | `send_email(..)`, `notify(..)` | a `NotificationRequested` outbox row | an argument cannot compile, or the text has a `WHERE` |
//! | `if: cond { action }` | the action behind the condition | the condition or the action is hand |
//! | everything else | | always: a required method of the generated trait |
//!
//! An entry's own `if:` is different: it decides whether anything of the entry runs, so it must
//! compile or the declaration is an error. The expressions are the rule language
//! ([`super::expr`]) over the row as stored, `$old`, `$actor` and `now()`.
//!
//! Both [`super::triggers::check_triggers`] (which wants the problems) and the generator (which
//! wants the steps) call [`compile_model`], so what validation accepts is what generation writes
//! and what `schema rules` labels.

use std::collections::HashSet;

use crate::ast::Model;

use super::compile::compile_event_condition;
use super::events::DeclaredEvent;
use super::triggers::{hand_slug, parse_action, DeclaredTrigger, ParsedAction, Phase, TriggerWhen};

/// A piece of the text of a `notify`: literal, or an expression to interpolate (`#{tier}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextPart {
    Lit(String),
    /// A `r::Val` expression.
    Expr(String),
}

/// One step of a trigger, in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// `tracing::info!`: the event name and its fields, each a `r::Val` expression.
    Log {
        name: String,
        fields: Vec<(String, String)>,
    },
    /// Stage the declared event, its payload built from the arguments (`r::Val` expressions).
    Emit {
        event: String,
        fields: Vec<(String, String)>,
    },
    /// Stage a `NotificationRequested`.
    Notify {
        /// `email` or `notify`.
        channel: &'static str,
        template: Option<String>,
        text: Vec<TextPart>,
        /// A `r::Val` expression for the recipients.
        to: String,
        data: Vec<(String, String)>,
    },
    /// Assign `rust` (a `r::Val` expression) to the row's `field` before it is stored.
    Set { field: String, rust: String },
    /// Run `then` when `condition` (a `r::Val` expression) holds.
    If { condition: String, then: Box<Step> },
    /// A required method of the generated trait.
    Hand {
        method: String,
        text: String,
        reason: Option<String>,
    },
}

impl Step {
    /// The hand methods this step requires.
    pub fn hand_methods(&self) -> Vec<&str> {
        match self {
            Step::Hand { method, .. } => vec![method.as_str()],
            Step::If { then, .. } => then.hand_methods(),
            _ => Vec::new(),
        }
    }

    /// Whether the step stages an outbox row.
    pub fn stages(&self) -> bool {
        match self {
            Step::Emit { .. } | Step::Notify { .. } => true,
            Step::If { then, .. } => then.stages(),
            _ => false,
        }
    }
}

/// What a trigger's expressions read beyond the row as stored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reads {
    pub old: bool,
    pub actor: bool,
    pub time: bool,
}

impl Reads {
    pub fn merge(&mut self, other: Reads) {
        self.old |= other.old;
        self.actor |= other.actor;
        self.time |= other.time;
    }
}

/// One trigger, compiled.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Compiled {
    /// The entry's own `if:` as a `r::Val` expression.
    pub condition: Option<String>,
    pub steps: Vec<Step>,
    pub reads: Reads,
    pub problems: Vec<String>,
    pub notes: Vec<String>,
}

impl Compiled {
    /// Every hand method the trigger requires, in order.
    pub fn hand_methods(&self) -> Vec<&str> {
        self.steps.iter().flat_map(Step::hand_methods).collect()
    }
}

/// A Rust identifier from a key (`after_update_status`, `on_enter running`).
pub fn ident(key: &str) -> String {
    let mut out: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    out.trim_matches('_').to_string()
}

/// The prefix of a trigger's hand method names: its key as an identifier.
pub fn method_prefix(t: &DeclaredTrigger) -> String {
    match &t.when {
        Some(TriggerWhen::Scheduled) => format!("scheduled__{}", ident(&t.key)),
        _ => ident(&t.key),
    }
}

/// The name of the entry point a hand-fired trigger is run with: `fire_after_login`.
pub fn fire_name(t: &DeclaredTrigger) -> String {
    format!("fire_{}", ident(&t.key))
}

struct Ctx<'a> {
    model: &'a Model,
    enums: &'a HashSet<String>,
    events: &'a [DeclaredEvent],
    trigger: &'a DeclaredTrigger,
    phase: Option<Phase>,
    prefix: String,
    reads: Reads,
    notes: Vec<String>,
    problems: Vec<String>,
}

type Taken = HashSet<String>;

/// Compile every trigger bound to `model`, in file order, as `(index into triggers, compiled)`.
/// A trigger whose key could not be resolved is skipped: its problem is already recorded.
pub fn compile_model(
    triggers: &[DeclaredTrigger],
    model: &Model,
    enums: &HashSet<String>,
    events: &[DeclaredEvent],
) -> Vec<(usize, Compiled)> {
    let mut taken = Taken::new();
    let mut out = Vec::new();
    for (index, t) in triggers.iter().enumerate() {
        if t.bound_model.as_deref() != Some(model.name.as_str()) || t.when.is_none() {
            continue;
        }
        out.push((index, compile_trigger(t, model, enums, events, &mut taken)));
    }
    out
}

fn compile_trigger(
    t: &DeclaredTrigger,
    model: &Model,
    enums: &HashSet<String>,
    events: &[DeclaredEvent],
    taken: &mut Taken,
) -> Compiled {
    let when = t.when.clone().expect("a resolved trigger");
    let mut cx = Ctx {
        model,
        enums,
        events,
        trigger: t,
        phase: when.phase(),
        prefix: method_prefix(t),
        reads: Reads::default(),
        notes: Vec::new(),
        problems: Vec::new(),
    };
    let mut out = Compiled::default();

    if when == TriggerWhen::Scheduled {
        if t.condition.is_some() {
            cx.problems.push(format!(
                "scheduled trigger `{}` of {} has an `if:`, but a schedule has no row to test: put the condition in the method",
                t.key, model.name
            ));
        }
        // A job has no row, so what it does is one method; the actions it lists are its description.
        let method = unique(&mut *taken, cx.prefix.clone());
        out.steps.push(Step::Hand {
            method,
            text: t.actions.join("; "),
            reason: None,
        });
        out.problems = cx.problems;
        return out;
    }

    if let Some(condition) = &t.condition {
        match compile_event_condition(condition, model, enums) {
            Ok(c) => {
                cx.reads.merge(Reads {
                    old: c.reads_old,
                    actor: c.reads_actor,
                    time: c.reads_time,
                });
                out.condition = Some(c.rust);
            }
            Err(why) => cx.problems.push(format!(
                "trigger `{}` of {} has an `if:` the generator cannot compile ({why}): {condition}",
                t.key, model.name
            )),
        }
    }
    for text in &t.actions {
        let step = compile_action(&mut cx, &mut *taken, &parse_action(text), text);
        out.steps.push(step);
    }
    out.reads = cx.reads;
    out.problems = cx.problems;
    out.notes = cx.notes;
    out
}

fn unique(taken: &mut Taken, base: String) -> String {
    let mut name = base.clone();
    let mut n = 2;
    while !taken.insert(name.clone()) {
        name = format!("{base}_{n}");
        n += 1;
    }
    name
}

fn hand(cx: &mut Ctx, taken: &mut Taken, text: &str, reason: Option<String>) -> Step {
    let method = unique(taken, format!("{}__{}", cx.prefix, hand_slug(text)));
    if let Some(r) = &reason {
        cx.notes.push(format!(
            "trigger `{}` of {}: `{text}` is a hand method (`{method}`): {r}",
            cx.trigger.key, cx.model.name
        ));
    }
    Step::Hand {
        method,
        text: text.to_string(),
        reason,
    }
}

/// Compile one expression to a `r::Val` expression, noting what it reads.
fn expr(cx: &mut Ctx, source: &str) -> Result<String, String> {
    let c = compile_event_condition(source, cx.model, cx.enums)?;
    cx.reads.merge(Reads {
        old: c.reads_old,
        actor: c.reads_actor,
        time: c.reads_time,
    });
    Ok(c.rust)
}

fn compile_pairs(
    cx: &mut Ctx,
    pairs: &[(String, String)],
) -> Result<Vec<(String, String)>, String> {
    pairs
        .iter()
        .map(|(k, v)| {
            expr(cx, v)
                .map(|r| (k.clone(), r))
                .map_err(|why| format!("`{k}: {v}`: {why}"))
        })
        .collect()
}

fn compile_action(cx: &mut Ctx, taken: &mut Taken, action: &ParsedAction, text: &str) -> Step {
    match action {
        ParsedAction::Hand { .. } => hand(cx, taken, text, None),
        ParsedAction::Emit { event, args } => {
            let Some(declared) = cx.events.iter().find(|e| &e.name == event) else {
                cx.problems.push(format!(
                    "trigger `{}` of {} emits `{event}`, which no schema file declares under `events:`: declare it, for example\n    events:\n      {event}:\n        aggregate: {}\n        fields: {{ {} }}",
                    cx.trigger.key,
                    cx.model.name,
                    cx.model.name,
                    args.iter().map(|(k, _)| format!("{k}: <type>")).collect::<Vec<_>>().join(", ")
                ));
                return hand(cx, taken, text, None);
            };
            for (name, _) in args {
                if !declared.fields.iter().any(|f| &f.name == name) {
                    cx.problems.push(format!(
                        "trigger `{}` of {} emits `{event}` with `{name}`, which the event does not declare (it declares {})",
                        cx.trigger.key,
                        cx.model.name,
                        declared.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(", ")
                    ));
                }
            }
            for field in &declared.fields {
                if args.iter().any(|(n, _)| n == &field.name) {
                    continue;
                }
                let optional = match &field.ty {
                    Some(t) => t.trim().ends_with('?') || t.trim().starts_with("Option<"),
                    None => {
                        declared.bound_model.as_deref() == Some(cx.model.name.as_str())
                            && cx
                                .model
                                .find_field(&field.name)
                                .is_some_and(|f| f.type_ref.is_optional())
                    }
                };
                if !optional {
                    cx.problems.push(format!(
                        "trigger `{}` of {} emits `{event}` without its field `{}`: give it `{}: <expr>`",
                        cx.trigger.key, cx.model.name, field.name, field.name
                    ));
                }
            }
            match compile_pairs(cx, args) {
                Ok(fields) => Step::Emit {
                    event: event.clone(),
                    fields,
                },
                Err(why) => hand(cx, taken, text, Some(why)),
            }
        }
        ParsedAction::Log { name, fields } => {
            let mut kept = Vec::new();
            for (k, v) in fields {
                match expr(cx, v) {
                    Ok(r) => kept.push((k.clone(), r)),
                    Err(why) => cx.notes.push(format!(
                        "trigger `{}` of {}: log field `{k}` is left out (`{v}`: {why})",
                        cx.trigger.key, cx.model.name
                    )),
                }
            }
            Step::Log {
                name: name.clone(),
                fields: kept,
            }
        }
        ParsedAction::Set {
            field,
            expr: source,
        } => {
            if cx.phase != Some(Phase::Before) {
                return hand(cx, taken, text, Some("a `set:` outside a `before_*` trigger would need a second write of the row".into()));
            }
            if cx.model.find_field(field).is_none() {
                cx.problems.push(format!(
                    "trigger `{}` of {} sets `{field}`, which {} has no field of",
                    cx.trigger.key, cx.model.name, cx.model.name
                ));
                return hand(cx, taken, text, None);
            }
            if matches!(
                cx.trigger.when,
                Some(TriggerWhen::Write {
                    op: super::triggers::Op::Delete,
                    ..
                })
            ) {
                cx.problems.push(format!(
                    "trigger `{}` of {} sets `{field}` before a delete, which stores nothing",
                    cx.trigger.key, cx.model.name
                ));
                return hand(cx, taken, text, None);
            }
            let bare = source.trim();
            let is_label = bare.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && bare.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
                && !matches!(bare, "true" | "false" | "null")
                && cx.model.find_field(bare).is_none();
            let rust = if is_label {
                Ok(format!("r::text({bare:?})"))
            } else {
                expr(cx, source)
            };
            match rust {
                Ok(rust) => Step::Set {
                    field: field.clone(),
                    rust,
                },
                Err(why) => {
                    cx.problems.push(format!(
                        "trigger `{}` of {} has `set: {field} = {source}` that cannot compile ({why}): a value set before the write must be an expression the rule language can say",
                        cx.trigger.key, cx.model.name
                    ));
                    hand(cx, taken, text, None)
                }
            }
        }
        ParsedAction::Increment { field, by } => {
            if cx.phase != Some(Phase::Before) {
                return hand(cx, taken, text, Some("an `increment:` outside a `before_*` trigger would need a second write of the row".into()));
            }
            if cx.model.find_field(field).is_none() {
                cx.problems.push(format!(
                    "trigger `{}` of {} increments `{field}`, which {} has no field of",
                    cx.trigger.key, cx.model.name, cx.model.name
                ));
                return hand(cx, taken, text, None);
            }
            let source = format!("{field} + {}", by.as_deref().unwrap_or("1"));
            match expr(cx, &source) {
                Ok(rust) => Step::Set {
                    field: field.clone(),
                    rust,
                },
                Err(why) => {
                    cx.problems.push(format!(
                        "trigger `{}` of {} has `{text}` that cannot compile ({why})",
                        cx.trigger.key, cx.model.name
                    ));
                    hand(cx, taken, text, None)
                }
            }
        }
        ParsedAction::SendEmail { template, to, data } => {
            let compiled = expr(cx, to).and_then(|to| compile_pairs(cx, data).map(|d| (to, d)));
            match compiled {
                Ok((to, data)) => Step::Notify {
                    channel: "email",
                    template: Some(template.clone()),
                    text: Vec::new(),
                    to,
                    data,
                },
                Err(why) => hand(cx, taken, text, Some(why)),
            }
        }
        ParsedAction::Notify {
            to,
            message,
            template,
            data,
        } => {
            let parts = if *template {
                Ok(Vec::new())
            } else {
                text_parts(cx, message)
            };
            let compiled = expr(cx, to)
                .and_then(|to| compile_pairs(cx, data).map(|d| (to, d)))
                .and_then(|(to, d)| parts.map(|p| (to, d, p)));
            match compiled {
                Ok((to, data, text_parts)) => Step::Notify {
                    channel: "notify",
                    template: template.then(|| message.clone()),
                    text: text_parts,
                    to,
                    data,
                },
                Err(why) => hand(cx, taken, text, Some(why)),
            }
        }
        ParsedAction::If { condition, then } => match expr(cx, condition) {
            Err(why) => hand(
                cx,
                taken,
                text,
                Some(format!("its condition cannot compile ({why})")),
            ),
            Ok(condition) => {
                // The inner action is compiled against a throwaway method table: if it is hand, the
                // whole action is one hand method (its condition is the hand code's to test).
                let mut scratch = taken.clone();
                let inner_text = match &**then {
                    ParsedAction::Hand { text, .. } => text.clone(),
                    _ => text.to_string(),
                };
                let inner = compile_action(cx, &mut scratch, then, &inner_text);
                if inner.hand_methods().is_empty() {
                    Step::If {
                        condition,
                        then: Box::new(inner),
                    }
                } else {
                    // Drop the notes the scratch compile made for the inner hand action.
                    hand(
                        cx,
                        taken,
                        text,
                        Some("its action is hand code, so the whole conditional is".into()),
                    )
                }
            }
        },
    }
}

/// `Your plan changed to #{tier}` as literal and expression parts.
fn text_parts(cx: &mut Ctx, message: &str) -> Result<Vec<TextPart>, String> {
    let mut parts = Vec::new();
    let mut rest = message;
    while let Some(start) = rest.find("#{") {
        if start > 0 {
            parts.push(TextPart::Lit(rest[..start].to_string()));
        }
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            return Err(format!("`#{{` in `{message}` is not closed"));
        };
        parts.push(TextPart::Expr(expr(cx, after[..end].trim())?));
        rest = &after[end + 1..];
    }
    if !rest.is_empty() {
        parts.push(TextPart::Lit(rest.to_string()));
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Field, Model, PrimitiveType, TypeRef};
    use crate::rules::RuleRegister;

    fn model() -> Model {
        let mut m = Model::new("Ticket");
        for (name, ty) in [
            ("id", PrimitiveType::Uuid),
            ("title", PrimitiveType::String),
            ("status", PrimitiveType::String),
            ("attempts", PrimitiveType::Int),
            ("email", PrimitiveType::String),
            ("opened_at", PrimitiveType::DateTime),
        ] {
            m.fields.push(Field::new(name, TypeRef::Primitive(ty)));
        }
        m
    }

    fn register(hook: &str) -> RuleRegister {
        let mut r = RuleRegister::default();
        r.read_file("ticket.hook.yaml", hook);
        let models = vec![model()];
        r.bind(&models);
        r.check(&models);
        r.mark_compiled(&models, &Default::default());
        r
    }

    const EVENTS: &str = "events:\n  TicketMade:\n    aggregate: Ticket\n    fields: { ticket_id: uuid, title: string, note: \"string?\" }\n";

    fn doc(triggers: &str) -> String {
        format!("name: Ticket\nmodel: Ticket\n{EVENTS}triggers:\n{triggers}")
    }

    #[test]
    fn compiled_actions_become_steps_and_the_rest_become_hand_methods_in_file_order() {
        let r = register(&doc(
            "  after_create:\n    actions:\n      - \"log(ticket.created, { id: id, who: $context.user })\"\n      - \"emit: TicketMade(ticket_id: id, title: title)\"\n      - \"send_email(welcome, email)\"\n      - \"trigger: refresh\"\n      - \"notify([id], Ticket #{title} made)\"\n",
        ));
        let t = &r.triggers[0];
        assert!(t.problems.is_empty(), "{:?}", t.problems);
        let c = &super::compile_model(&r.triggers, &model(), &Default::default(), &r.events)[0].1;
        assert!(
            matches!(&c.steps[0], Step::Log { name, fields } if name == "ticket.created" && fields.len() == 1),
            "the $context field is left out: {:?}",
            c.steps[0]
        );
        assert!(
            matches!(&c.steps[1], Step::Emit { event, fields } if event == "TicketMade" && fields.len() == 2)
        );
        assert!(
            matches!(&c.steps[2], Step::Notify { channel: "email", template: Some(t), .. } if t == "welcome")
        );
        assert!(
            matches!(&c.steps[3], Step::Hand { method, .. } if method == "after_create__refresh")
        );
        assert!(
            matches!(&c.steps[4], Step::Notify { channel: "notify", text, .. } if text.len() == 3)
        );
        assert_eq!(c.hand_methods(), vec!["after_create__refresh"]);
        assert!(
            c.notes
                .iter()
                .any(|n| n.contains("log field `who` is left out")),
            "{:?}",
            c.notes
        );
    }

    #[test]
    fn an_action_whose_arguments_cannot_compile_becomes_a_hand_method_and_says_why() {
        let r = register(&doc(
            "  after_create:\n    actions:\n      - \"emit: TicketMade(ticket_id: id, title: $context.title)\"\n      - \"send_email(welcome, user.email)\"\n      - \"if: owner.active == true { notify([id], hello) }\"\n",
        ));
        let c = &super::compile_model(&r.triggers, &model(), &Default::default(), &r.events)[0].1;
        assert_eq!(
            c.steps
                .iter()
                .filter(|s| matches!(s, Step::Hand { .. }))
                .count(),
            3,
            "{:?}",
            c.steps
        );
        assert!(
            c.notes.iter().any(|n| n.contains("$context.title")),
            "{:?}",
            c.notes
        );
        assert!(
            r.triggers[0].problems.is_empty(),
            "a hand fallback is not an error: {:?}",
            r.triggers[0].problems
        );
    }

    #[test]
    fn set_compiles_only_before_the_write_and_a_label_is_text() {
        let r = register(&doc(
            "  before_create:\n    actions:\n      - \"set: status = open\"\n      - \"set: opened_at = now()\"\n      - \"increment: attempts by 2\"\n  after_update_status:\n    actions:\n      - \"set: title = 'x'\"\n",
        ));
        let all = super::compile_model(&r.triggers, &model(), &Default::default(), &r.events);
        let before = &all[0].1;
        assert!(
            matches!(&before.steps[0], Step::Set { field, rust } if field == "status" && rust == "r::text(\"open\")"),
            "{:?}",
            before.steps[0]
        );
        assert!(matches!(&before.steps[1], Step::Set { field, .. } if field == "opened_at"));
        assert!(matches!(&before.steps[2], Step::Set { field, .. } if field == "attempts"));
        assert!(before.reads.time, "now() reads the time");
        assert!(
            matches!(&all[1].1.steps[0], Step::Hand { method, .. } if method == "after_update_status__set_title")
        );
    }

    #[test]
    fn what_cannot_be_said_is_an_error_not_a_guess() {
        let r = register(&doc(
            "  after_create:\n    if: \"status IN ('a','b')\"\n    actions:\n      - \"emit: Nobody(x: id)\"\n      - \"emit: TicketMade(ticket_id: id, bogus: id)\"\n  before_create:\n    actions:\n      - \"set: nope = 1\"\n      - \"set: title = owner.name\"\n  nameless:\n    actions: [\"log(a)\"]\n",
        ));
        let problems: Vec<String> = r.triggers.iter().flat_map(|t| t.problems.clone()).collect();
        for needle in [
            "an `if:` the generator cannot compile",
            "emits `Nobody`, which no schema file declares under `events:`",
            "with `bogus`, which the event does not declare",
            "without its field `title`",
            "sets `nope`, which Ticket has no field of",
            "`set: title = owner.name` that cannot compile",
            "names no event",
        ] {
            assert!(
                problems.iter().any(|p| p.contains(needle)),
                "{needle} not in:\n{}",
                problems.join("\n")
            );
        }
    }

    #[test]
    fn method_names_stay_unique_within_a_model_and_a_schedule_is_one_method() {
        let r = register(&doc(
            "  after_create:\n    actions: [\"trigger: a\", \"trigger: a\"]\n  sweep:\n    schedule: \"every 5.minutes\"\n    actions: [\"update: Ticket(status: 'x') WHERE id == id\", \"log(swept)\"]\n  after_login:\n    actions: [\"trigger: a\"]\n",
        ));
        let all = super::compile_model(&r.triggers, &model(), &Default::default(), &r.events);
        assert_eq!(
            all[0].1.hand_methods(),
            vec!["after_create__a", "after_create__a_2"]
        );
        assert_eq!(all[1].1.hand_methods(), vec!["scheduled__sweep"]);
        assert_eq!(all[2].1.hand_methods(), vec!["after_login__a"]);
    }
}
