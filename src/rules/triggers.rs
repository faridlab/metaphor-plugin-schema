//! Declared hook triggers: the `triggers:` of a hook file and the `on_enter` / `on_exit` lists of
//! its states, read from the raw YAML and resolved against the model they belong to.
//!
//! An entry is a key and a list of action strings:
//!
//! ```yaml
//! triggers:
//!   before_create:                       # before_/after_ create | update | delete
//!     actions:
//!       - "set: opened_at = now()"
//!   after_update_status:                 # a field changed (after_update_<field>[_<value>])
//!     if: "status == 'resolved'"         # `condition:` is read as an alias
//!     actions:
//!       - "log(ticket.resolved, { ticket_id: id })"
//!       - "emit: TicketResolved(ticket_id: id)"
//!       - "send_email(resolved_notice, owner_email)"
//!       - "trigger: refresh_counters"    # anything else is a required hand method
//!   after_close:                         # after_<verb> of the model's state machine
//!   after_login:                         # after_<word> naming no transition: hand-fired
//!   sweep_stale:                         # a free name with `schedule:`
//!     schedule: "every 5.minutes"
//! states:
//!   values:
//!     running:
//!       on_enter: ["set: started_at = now()"]
//! ```
//!
//! What a key means is decided against the model ([`check_triggers`]): the field a key names must
//! exist, a verb must be a transition of one of its machines, a state must be one of its states.
//! The actions are compiled by [`super::trigger_compile`]; here they are only split into the
//! shapes the compiler knows ([`parse_action`]).

use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};

use super::SourceRef;

/// Whether a trigger runs before the row is written or after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Before,
    After,
}

impl Phase {
    pub fn text(&self) -> &'static str {
        match self {
            Phase::Before => "before",
            Phase::After => "after",
        }
    }
}

/// The write a trigger is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    Create,
    Update,
    Delete,
}

impl Op {
    pub fn text(&self) -> &'static str {
        match self {
            Op::Create => "create",
            Op::Update => "update",
            Op::Delete => "delete",
        }
    }
}

/// When a trigger fires, as resolved against its model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerWhen {
    /// A row is created, changed or deleted.
    Write { phase: Phase, op: Op },
    /// A row's field changed (to `value` when one is named).
    FieldChange {
        phase: Phase,
        field: String,
        value: Option<String>,
    },
    /// A state-machine verb moved a row.
    Transition { verb: String },
    /// A state-machine verb moved a row into the state.
    Enter(String),
    /// A state-machine verb moved a row out of the state.
    Exit(String),
    /// The key names no write, field or transition: a hand verb fires it (`fire_<name>`).
    HandFired,
    /// A cron or interval job.
    Scheduled,
}

impl TriggerWhen {
    /// The phase the generated hooks run it in.
    pub fn phase(&self) -> Option<Phase> {
        match self {
            TriggerWhen::Write { phase, .. } | TriggerWhen::FieldChange { phase, .. } => {
                Some(*phase)
            }
            TriggerWhen::Transition { .. } | TriggerWhen::Enter(_) | TriggerWhen::Exit(_) => {
                Some(Phase::After)
            }
            TriggerWhen::HandFired | TriggerWhen::Scheduled => None,
        }
    }

    /// The trigger as `schema rules` writes it: `after_update status`, `on_enter running`.
    pub fn text(&self) -> String {
        match self {
            TriggerWhen::Write { phase, op } => format!("{}_{}", phase.text(), op.text()),
            TriggerWhen::FieldChange {
                phase,
                field,
                value: None,
            } => format!("{}_update {field}", phase.text()),
            TriggerWhen::FieldChange {
                phase,
                field,
                value: Some(v),
            } => format!("{}_update {field} -> {v}", phase.text()),
            TriggerWhen::Transition { verb } => format!("after transition {verb}"),
            TriggerWhen::Enter(s) => format!("on_enter {s}"),
            TriggerWhen::Exit(s) => format!("on_exit {s}"),
            TriggerWhen::HandFired => "hand-fired".to_string(),
            TriggerWhen::Scheduled => "scheduled".to_string(),
        }
    }
}

/// Which list of a state a state hook came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StateList {
    Enter(String),
    Exit(String),
}

/// One trigger as a hook file declares it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeclaredTrigger {
    pub source: SourceRef,
    /// The key as written (`after_update_status`), or `on_enter running` for a state's list.
    pub key: String,
    /// The model the file names, tried when the entry names no `entity:`.
    pub target: String,
    pub alternatives: Vec<String>,
    /// `entity:`: the model the entry is about when the file names another.
    pub entity: Option<String>,
    /// The model it belongs to, once [`RuleRegister::bind`](super::RuleRegister::bind) found it.
    pub bound_model: Option<String>,
    /// `timing:`: names the event when the key is only a label.
    pub timing: Option<String>,
    /// `if:` (or its alias `condition:`), as written.
    pub condition: Option<String>,
    /// `schedule:`, as written.
    pub schedule: Option<String>,
    pub description: Option<String>,
    /// The action strings, as written.
    pub actions: Vec<String>,
    /// Set for a state's `on_enter` / `on_exit` list.
    pub state_list: Option<StateList>,
    /// When it fires, set by [`check_triggers`]; `None` while a problem keeps it unresolved.
    pub when: Option<TriggerWhen>,
    /// Why it cannot be generated, set by [`check_triggers`]: each one fails validation.
    pub problems: Vec<String>,
    /// What generation says without failing: an argument that could not compile and became a hand
    /// method, a log field left out.
    pub notes: Vec<String>,
}

/// What one file's `triggers:` and state lists declare.
pub fn read_triggers(
    file: &str,
    root: &Mapping,
    target: &str,
    alternatives: &[String],
) -> Vec<DeclaredTrigger> {
    fn get<'a>(m: &'a Mapping, key: &str) -> Option<&'a Value> {
        m.get(Value::String(key.to_string()))
    }
    fn text(m: &Mapping, key: &str) -> Option<String> {
        get(m, key).and_then(Value::as_str).map(str::to_string)
    }
    let source = SourceRef {
        file: file.to_string(),
        line: None,
    };
    let actions_of = |v: Option<&Value>| -> Vec<String> {
        match v {
            Some(Value::Sequence(items)) => items
                .iter()
                .filter_map(Value::as_str)
                .map(|s| s.trim().to_string())
                .collect(),
            Some(Value::String(s)) => vec![s.trim().to_string()],
            _ => Vec::new(),
        }
    };
    let mut out = Vec::new();
    if let Some(Value::Mapping(map)) = get(root, "triggers") {
        for (key, body) in map {
            let Some(key) = key.as_str() else { continue };
            let empty = Mapping::new();
            let body = body.as_mapping().unwrap_or(&empty);
            out.push(DeclaredTrigger {
                source: source.clone(),
                key: key.to_string(),
                target: target.to_string(),
                alternatives: alternatives.to_vec(),
                entity: text(body, "entity"),
                bound_model: None,
                timing: text(body, "timing"),
                condition: text(body, "if").or_else(|| text(body, "condition")),
                schedule: text(body, "schedule"),
                description: text(body, "description"),
                actions: actions_of(get(body, "actions")),
                state_list: None,
                when: None,
                problems: Vec::new(),
                notes: Vec::new(),
            });
        }
    }
    // The `on_enter` / `on_exit` lists of a machine's states (the map format's `values:` and the
    // list format's `states:`).
    let mut machines: Vec<&Mapping> = Vec::new();
    if let Some(Value::Mapping(states)) = get(root, "states") {
        machines.push(states);
    }
    if let Some(Value::Sequence(list)) = get(root, "state_machines") {
        machines.extend(list.iter().filter_map(Value::as_mapping));
    }
    for machine in machines {
        let machine_entity = text(machine, "entity");
        let states: Vec<(String, &Mapping)> =
            match get(machine, "values").or_else(|| get(machine, "states")) {
                Some(Value::Mapping(map)) => map
                    .iter()
                    .filter_map(|(k, v)| Some((k.as_str()?.to_string(), v.as_mapping()?)))
                    .collect(),
                Some(Value::Sequence(seq)) => seq
                    .iter()
                    .filter_map(Value::as_mapping)
                    .filter_map(|m| Some((text(m, "name")?, m)))
                    .collect(),
                _ => Vec::new(),
            };
        for (state, detail) in states {
            for (list, make) in [
                ("on_enter", StateList::Enter as fn(String) -> StateList),
                ("on_exit", StateList::Exit),
            ] {
                let actions = actions_of(get(detail, list));
                if actions.is_empty() {
                    continue;
                }
                out.push(DeclaredTrigger {
                    source: source.clone(),
                    key: format!("{list} {state}"),
                    target: target.to_string(),
                    alternatives: alternatives.to_vec(),
                    entity: machine_entity.clone(),
                    bound_model: None,
                    timing: None,
                    condition: None,
                    schedule: None,
                    description: None,
                    actions,
                    state_list: Some(make(state.clone())),
                    when: None,
                    problems: Vec::new(),
                    notes: Vec::new(),
                });
            }
        }
    }
    out
}

/// One action string, split into the shape the compiler knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedAction {
    /// `emit: Event(arg: expr, ..)`
    Emit {
        event: String,
        args: Vec<(String, String)>,
    },
    /// `log(name, { field: expr, .. })`
    Log {
        name: String,
        fields: Vec<(String, String)>,
    },
    /// `set: field = expr`
    Set { field: String, expr: String },
    /// `increment: field` or `increment: field by n`, of the row itself
    Increment { field: String, by: Option<String> },
    /// `send_email(template, to[, { .. }])`
    SendEmail {
        template: String,
        to: String,
        data: Vec<(String, String)>,
    },
    /// `notify(to, template-or-text[, { .. }])`
    Notify {
        to: String,
        message: String,
        template: bool,
        data: Vec<(String, String)>,
    },
    /// `if: cond { action }` or `if cond: action`
    If {
        condition: String,
        then: Box<ParsedAction>,
    },
    /// Anything else: a required method of the generated trait.
    Hand { verb: String, text: String },
}

/// Split `s` at the commas that are not inside brackets, braces, parentheses or quotes.
pub fn split_top(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut cur = String::new();
    for c in s.chars() {
        if let Some(q) = quote {
            cur.push(c);
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => {
                quote = Some(c);
                cur.push(c);
            }
            '(' | '[' | '{' => {
                depth += 1;
                cur.push(c);
            }
            ')' | ']' | '}' => {
                depth -= 1;
                cur.push(c);
            }
            c if c == sep && depth == 0 => out.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    if !cur.trim().is_empty() || !out.is_empty() {
        out.push(cur);
    }
    out.into_iter().map(|p| p.trim().to_string()).collect()
}

/// The byte index of the first `needle` outside brackets, braces, parentheses and quotes.
fn find_top(s: &str, needle: char) -> Option<usize> {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '(' | '[' | '{' => {
                if c == needle && depth == 0 {
                    return Some(i);
                }
                depth += 1;
            }
            ')' | ']' | '}' => depth -= 1,
            c if c == needle && depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `{ a: expr, b: expr }` as pairs; `None` when it is not an object literal of `name: expr`.
fn parse_object(s: &str) -> Option<Vec<(String, String)>> {
    let inner = s.trim().strip_prefix('{')?.strip_suffix('}')?;
    let mut out = Vec::new();
    for item in split_top(inner, ',') {
        if item.is_empty() {
            continue;
        }
        let (k, v) = item.split_once(':')?;
        let k = k.trim().trim_matches(|c| c == '"' || c == '\'');
        if !is_ident(k) {
            return None;
        }
        out.push((k.to_string(), v.trim().to_string()));
    }
    Some(out)
}

/// The call `name(args)` of `s` with nothing after the closing parenthesis: `(name, inner)`.
fn call_of(s: &str) -> Option<(String, String)> {
    let open = s.find('(')?;
    let name = s[..open].trim();
    let inner = s[open + 1..].trim_end().strip_suffix(')')?;
    // The parenthesis found must be the one the last one closes.
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for (i, c) in s[open..].char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 && open + i + 1 != s.trim_end().len() {
                    return None;
                }
            }
            _ => {}
        }
    }
    Some((name.to_string(), inner.to_string()))
}

fn hand(text: &str) -> ParsedAction {
    let verb: String = text
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    ParsedAction::Hand {
        verb,
        text: text.trim().to_string(),
    }
}

/// Split one action string into the shape the compiler knows; anything it does not recognise
/// (or recognises but cannot take apart) is a [`ParsedAction::Hand`].
pub fn parse_action(text: &str) -> ParsedAction {
    let text = text.trim();
    // `if: cond { action }`, `if cond: action`
    if let Some(rest) = text
        .strip_prefix("if:")
        .or_else(|| text.strip_prefix("if "))
    {
        let rest = rest.trim();
        if let Some(open) = find_top(rest, '{') {
            if let Some(body) = rest[open..]
                .trim_end()
                .strip_prefix('{')
                .and_then(|b| b.strip_suffix('}'))
            {
                let condition = rest[..open].trim().to_string();
                if !condition.is_empty() {
                    return ParsedAction::If {
                        condition,
                        then: Box::new(parse_action(body)),
                    };
                }
            }
        } else if text.starts_with("if ") {
            if let Some(colon) = find_top(rest, ':') {
                let (condition, body) = (rest[..colon].trim(), rest[colon + 1..].trim());
                if !condition.is_empty() && !body.is_empty() {
                    return ParsedAction::If {
                        condition: condition.to_string(),
                        then: Box::new(parse_action(body)),
                    };
                }
            }
        }
        return hand(text);
    }
    // `verb: rest`
    if let Some((verb, rest)) = text.split_once(':') {
        let verb = verb.trim();
        let rest = rest.trim();
        match verb {
            "emit" => {
                if let Some((event, inner)) = call_of(rest) {
                    let mut args = Vec::new();
                    for item in split_top(&inner, ',') {
                        if item.is_empty() {
                            continue;
                        }
                        let Some((k, v)) = item.split_once(':') else {
                            return hand(text);
                        };
                        if !is_ident(k.trim()) {
                            return hand(text);
                        }
                        args.push((k.trim().to_string(), v.trim().to_string()));
                    }
                    if is_ident(event.trim()) {
                        return ParsedAction::Emit {
                            event: event.trim().to_string(),
                            args,
                        };
                    }
                } else if is_ident(rest) {
                    return ParsedAction::Emit {
                        event: rest.to_string(),
                        args: Vec::new(),
                    };
                }
                return hand(text);
            }
            "set" => {
                if let Some((field, expr)) = rest.split_once('=') {
                    let (field, expr) = (field.trim(), expr.trim());
                    if is_ident(field) && !expr.is_empty() && !expr.starts_with('=') {
                        return ParsedAction::Set {
                            field: field.to_string(),
                            expr: expr.to_string(),
                        };
                    }
                }
                return hand(text);
            }
            "increment" => {
                // Another entity's row (`f in Model where ..`, `f on Model where ..`) is not the row's own.
                let (field, by) = match rest.split_once(" by ") {
                    Some((f, n)) => (f.trim(), Some(n.trim().to_string())),
                    None => (rest, None),
                };
                if is_ident(field) {
                    return ParsedAction::Increment {
                        field: field.to_string(),
                        by,
                    };
                }
                return hand(text);
            }
            _ => {}
        }
    }
    // `verb(args)`
    if let Some((name, inner)) = call_of(text) {
        let args = split_top(&inner, ',');
        match name.as_str() {
            "log" if !args.is_empty() => {
                let event = args[0].trim();
                if args.len() == 1 && !event.is_empty() {
                    return ParsedAction::Log {
                        name: event.to_string(),
                        fields: Vec::new(),
                    };
                }
                if args.len() >= 2 {
                    if let Some(fields) = parse_object(&args[1..].join(", ")) {
                        return ParsedAction::Log {
                            name: event.to_string(),
                            fields,
                        };
                    }
                }
                return hand(text);
            }
            "send_email" if args.len() >= 2 => {
                let data = match args.get(2) {
                    Some(d) => match parse_object(d) {
                        Some(d) => d,
                        None => return hand(text),
                    },
                    None => Vec::new(),
                };
                if is_template(&args[0]) {
                    return ParsedAction::SendEmail {
                        template: args[0].clone(),
                        to: args[1].clone(),
                        data,
                    };
                }
                return hand(text);
            }
            "notify" if args.len() >= 2 => {
                let to = args[0].clone();
                let (middle, data) = match args
                    .last()
                    .filter(|l| l.starts_with('{') && args.len() >= 3)
                {
                    Some(last) => match parse_object(last) {
                        Some(d) => (args[1..args.len() - 1].join(", "), d),
                        None => return hand(text),
                    },
                    None => (args[1..].join(", "), Vec::new()),
                };
                let template = is_template(&middle);
                return ParsedAction::Notify {
                    to,
                    message: middle,
                    template,
                    data,
                };
            }
            _ => {}
        }
    }
    hand(text)
}

fn is_template(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
}

/// The identifier part of a hand action's name: `trigger: invalidate_all_sessions` reads
/// `invalidate_all_sessions`, `update: StoredFile(..) WHERE ..` reads `update_stored_file`,
/// `validate_workflow_structure()` reads itself.
pub fn hand_slug(text: &str) -> String {
    use crate::utils::to_snake_case;
    let text = text.trim();
    let verb: String = text
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    let rest =
        text[verb.len()..].trim_start_matches(|c: char| c == ':' || c == '(' || c.is_whitespace());
    let second: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
        .collect();
    let slug = match verb.as_str() {
        "trigger" if !second.is_empty() => to_snake_case(&second),
        "create" | "update" | "delete" | "queue" | "enqueue" | "dispatch" | "schedule_job"
        | "call" | "notify" | "send_email" | "emit" | "log" | "set" | "increment" | "if"
            if !second.is_empty() =>
        {
            format!("{verb}_{}", to_snake_case(&second))
        }
        "" => "action".to_string(),
        _ => to_snake_case(&verb),
    };
    let slug: String = slug
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .chars()
        .take(64)
        .collect();
    let slug = slug.trim_end_matches('_').to_string();
    if slug.is_empty() {
        "action".to_string()
    } else if slug.starts_with(|c: char| c.is_ascii_digit()) {
        format!("a_{slug}")
    } else {
        slug
    }
}

/// Resolve every trigger against its model, and compile its actions to learn what is wrong.
///
/// A key is read as, in order: `schedule:` (a job), a write (`before_` or `after_` with `create`,
/// `update` or `delete`), `timing:` when the key is only a label, `after_update_<field>[_<value>]`
/// (the longest field of the model the key spells), `after_<verb>` of a state machine of the model,
/// then `after_<word>` as a hand-fired trigger. Anything else is a label that names no event. The
/// problems are recorded on the trigger and fail validation unless the module opted out
/// (`generators: legacy_triggers: true`).
pub fn check_triggers(
    register: &mut super::RuleRegister,
    models: &[crate::ast::Model],
    enums: &std::collections::HashSet<String>,
) {
    let machines = register.machines.clone();
    let events = register.events.clone();
    for trigger in &mut register.triggers {
        trigger.problems.clear();
        trigger.notes.clear();
        trigger.when = None;
    }
    // Resolution first, then one compile per model so method names are unique across its triggers.
    for index in 0..register.triggers.len() {
        let t = &register.triggers[index];
        let Some(model) = t
            .bound_model
            .as_ref()
            .and_then(|n| models.iter().find(|m| &m.name == n))
        else {
            let target = t.entity.clone().unwrap_or_else(|| t.target.clone());
            let message = format!(
                "trigger `{}` names `{target}`, which is not a model of this module: give it an `entity:` naming the model it is about",
                t.key
            );
            register.triggers[index].problems.push(message);
            continue;
        };
        let (when, mut problems) = resolve_when(t, model, &machines);
        let t = &mut register.triggers[index];
        t.when = when;
        t.problems.append(&mut problems);
    }
    for model in models {
        let compiled =
            super::trigger_compile::compile_model(&register.triggers, model, enums, &events);
        for (index, c) in compiled {
            let t = &mut register.triggers[index];
            t.problems.extend(c.problems);
            t.notes.extend(c.notes);
        }
    }
}

fn resolve_when(
    t: &DeclaredTrigger,
    model: &crate::ast::Model,
    machines: &[super::DeclaredMachine],
) -> (Option<TriggerWhen>, Vec<String>) {
    let mut problems = Vec::new();
    let own_machines = || {
        machines
            .iter()
            .filter(|m| m.bound_model.as_deref() == Some(model.name.as_str()))
    };
    let at = format!("{} of {}", t.key, model.name);
    if let Some(list) = &t.state_list {
        let state = match list {
            StateList::Enter(s) | StateList::Exit(s) => s,
        };
        if !own_machines().any(|m| m.states.contains(state)) {
            problems.push(format!(
                "trigger `{}` of {} names a state no machine of it declares",
                t.key, model.name
            ));
            return (None, problems);
        }
        return (
            Some(match list {
                StateList::Enter(s) => TriggerWhen::Enter(s.clone()),
                StateList::Exit(s) => TriggerWhen::Exit(s.clone()),
            }),
            problems,
        );
    }
    if t.schedule.is_some() {
        if t.schedule.as_deref().is_some_and(|s| s.trim().is_empty()) {
            problems.push(format!("trigger `{at}` has an empty `schedule:`"));
        }
        return (Some(TriggerWhen::Scheduled), problems);
    }
    let key = t.key.as_str();
    let write_of = |event: &str| -> Option<TriggerWhen> {
        let (phase, rest) = match event.strip_prefix("before_") {
            Some(r) => (Phase::Before, r),
            None => (Phase::After, event.strip_prefix("after_")?),
        };
        let op = match rest {
            "create" => Op::Create,
            "update" => Op::Update,
            "delete" => Op::Delete,
            _ => return None,
        };
        Some(TriggerWhen::Write { phase, op })
    };
    if let Some(when) = write_of(key) {
        return (Some(when), problems);
    }
    // A key that is only a label says its event with `timing:`.
    let named = if key.starts_with("before_") || key.starts_with("after_") {
        key.to_string()
    } else if let Some(timing) = &t.timing {
        if let Some(when) = write_of(timing) {
            return (Some(when), problems);
        }
        problems.push(format!(
            "trigger `{at}` has `timing: {timing}`, which is not before_ or after_ create, update or delete"
        ));
        return (None, problems);
    } else {
        problems.push(format!(
            "trigger `{at}` is a label that names no event: key it `before_`/`after_` create, update or delete, `after_update_<field>`, `after_<verb>`, \
             or give it `timing:` or `schedule:`"
        ));
        return (None, problems);
    };
    // `after_update_<field>[_<value>]`, `before_update_<field>`
    for (phase, prefix) in [
        (Phase::After, "after_update_"),
        (Phase::Before, "before_update_"),
    ] {
        if let Some(rest) = named.strip_prefix(prefix) {
            let best = model
                .fields
                .iter()
                .filter(|f| rest == f.name || rest.starts_with(&format!("{}_", f.name)))
                .max_by_key(|f| f.name.len());
            return match best {
                Some(field) => {
                    let value = rest
                        .strip_prefix(&field.name)
                        .and_then(|v| v.strip_prefix('_'))
                        .map(str::to_string);
                    (
                        Some(TriggerWhen::FieldChange {
                            phase,
                            field: field.name.clone(),
                            value,
                        }),
                        problems,
                    )
                }
                None => {
                    problems.push(format!(
                        "trigger `{at}` is keyed on a field change, but {} has no field that `{rest}` spells (the key is `{prefix}<field>[_<value>]`)",
                        model.name
                    ));
                    (None, problems)
                }
            };
        }
    }
    if let Some(verb) = named.strip_prefix("after_") {
        if own_machines().any(|m| {
            m.transitions
                .iter()
                .any(|tr| tr.verb.as_deref() == Some(verb))
        }) {
            return (
                Some(TriggerWhen::Transition {
                    verb: verb.to_string(),
                }),
                problems,
            );
        }
        return (Some(TriggerWhen::HandFired), problems);
    }
    problems.push(format!(
        "trigger `{at}` is keyed `{named}`: only `after_<word>` can name a hand verb; `before_` takes create, update, delete or update_<field>"
    ));
    (None, problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Field, Model, PrimitiveType, TypeRef};
    use crate::rules::RuleRegister;

    #[test]
    fn an_action_string_is_split_into_the_shape_the_compiler_knows() {
        assert_eq!(
            parse_action("emit: UserCreatedEvent(user_id: id, email: email)"),
            ParsedAction::Emit {
                event: "UserCreatedEvent".into(),
                args: vec![
                    ("user_id".into(), "id".into()),
                    ("email".into(), "email".into())
                ]
            }
        );
        assert_eq!(
            parse_action("log(user.created, { user_id: id, ip: $context.ip, nested: f(a, b) })"),
            ParsedAction::Log {
                name: "user.created".into(),
                fields: vec![
                    ("user_id".into(), "id".into()),
                    ("ip".into(), "$context.ip".into()),
                    ("nested".into(), "f(a, b)".into())
                ]
            }
        );
        assert_eq!(
            parse_action("log(job.queued)"),
            ParsedAction::Log {
                name: "job.queued".into(),
                fields: vec![]
            }
        );
        assert_eq!(
            parse_action("set: locked_until = now() + 15.minutes"),
            ParsedAction::Set {
                field: "locked_until".into(),
                expr: "now() + 15.minutes".into()
            }
        );
        assert_eq!(
            parse_action("send_email(tier_changed, user.email, { old_tier: $old.tier })"),
            ParsedAction::SendEmail {
                template: "tier_changed".into(),
                to: "user.email".into(),
                data: vec![("old_tier".into(), "$old.tier".into())]
            }
        );
        assert_eq!(
            parse_action("notify([user_id], Your plan changed to #{tier}, now)"),
            ParsedAction::Notify {
                to: "[user_id]".into(),
                message: "Your plan changed to #{tier}, now".into(),
                template: false,
                data: vec![]
            }
        );
        assert_eq!(
            parse_action("notify(mentions, comment_mention, { comment_id: id })"),
            ParsedAction::Notify {
                to: "mentions".into(),
                message: "comment_mention".into(),
                template: true,
                data: vec![("comment_id".into(), "id".into())]
            }
        );
        assert_eq!(
            parse_action("if: mentions != null && mentions.length > 0 { notify(mentions, tmpl, { id: id }) }"),
            ParsedAction::If {
                condition: "mentions != null && mentions.length > 0".into(),
                then: Box::new(ParsedAction::Notify {
                    to: "mentions".into(),
                    message: "tmpl".into(),
                    template: true,
                    data: vec![("id".into(), "id".into())]
                })
            }
        );
        assert!(
            matches!(parse_action("if risk_score > 0.8: require_mfa"), ParsedAction::If { condition, .. } if condition == "risk_score > 0.8")
        );
        assert_eq!(
            parse_action("increment: attempts"),
            ParsedAction::Increment {
                field: "attempts".into(),
                by: None
            }
        );
        assert_eq!(
            parse_action("increment: attempts by 2"),
            ParsedAction::Increment {
                field: "attempts".into(),
                by: Some("2".into())
            }
        );
    }

    #[test]
    fn what_the_compiler_does_not_know_is_hand() {
        for text in [
            "trigger: invalidate_all_sessions",
            "update: StoredFile(status: 'x') WHERE id == file_id",
            "increment: usage_count in User where id == user_id",
            "validate_workflow_structure()",
            "call: Svc.run(a: 1)",
            "notify([user_id], upload_expiring, { id: id }) WHERE status == 'x'",
            "queue: Job(id: id)",
            "emit: Bad Name(x: 1)",
        ] {
            assert!(
                matches!(parse_action(text), ParsedAction::Hand { .. }),
                "{text}: {:?}",
                parse_action(text)
            );
        }
    }

    #[test]
    fn a_hand_action_names_its_method_by_what_it_does() {
        assert_eq!(
            hand_slug("trigger: invalidate_all_sessions"),
            "invalidate_all_sessions"
        );
        assert_eq!(
            hand_slug("trigger: cleanup_expired_tokens(hours: 24)"),
            "cleanup_expired_tokens"
        );
        assert_eq!(
            hand_slug("update: StoredFile(a: 1) WHERE id == x"),
            "update_stored_file"
        );
        assert_eq!(
            hand_slug("create: FileAccessLog(file_id: id)"),
            "create_file_access_log"
        );
        assert_eq!(
            hand_slug("validate_workflow_structure()"),
            "validate_workflow_structure"
        );
        assert_eq!(
            hand_slug("call: StorageProvisioningService.create_bucket_directory(path: p)"),
            "call_storage_provisioning_service_create_bucket_directory"
        );
        assert_eq!(
            hand_slug("schedule_job(expire_verification, run_at: now())"),
            "schedule_job_expire_verification"
        );
        assert_eq!(hand_slug("revoke_if: attempts >= max"), "revoke_if");
    }

    #[test]
    fn the_shapes_of_an_entry_and_a_state_list_are_read() {
        let doc = "name: Job\nmodel: job.model.yaml\nstates:\n  field: status\n  values:\n    pending:\n      initial: true\n      on_enter:\n        - \"log(job.queued)\"\n    running:\n      on_enter: [\"set: started_at = now()\"]\ntriggers:\n  after_create:\n    actions: [\"log(a)\"]\n    if: \"x == 1\"\n  update_stats:\n    timing: after_update\n    condition: \"y == 2\"\n    actions: [\"update: S(a: 1)\"]\n  sweep:\n    schedule: \"*/5 * * * *\"\n    description: sweeps\n    actions: [\"trigger: sweep_now\"]\n  after_workflow_start:\n    entity: WorkflowRun\n    actions: [\"log(x)\"]\n";
        let root = serde_yaml::from_str::<Value>(doc).unwrap();
        let t = read_triggers(
            "job.hook.yaml",
            root.as_mapping().unwrap(),
            "Job",
            &["job.model.yaml".to_string()],
        );
        let keys: Vec<&str> = t.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "after_create",
                "update_stats",
                "sweep",
                "after_workflow_start",
                "on_enter pending",
                "on_enter running"
            ]
        );
        assert_eq!(t[0].condition.as_deref(), Some("x == 1"));
        assert_eq!(
            (t[1].timing.as_deref(), t[1].condition.as_deref()),
            (Some("after_update"), Some("y == 2")),
            "condition: reads as if:"
        );
        assert_eq!(
            (t[2].schedule.as_deref(), t[2].description.as_deref()),
            (Some("*/5 * * * *"), Some("sweeps"))
        );
        assert_eq!(t[3].entity.as_deref(), Some("WorkflowRun"));
        assert_eq!(t[4].state_list, Some(StateList::Enter("pending".into())));
        assert_eq!(t[5].actions, vec!["set: started_at = now()".to_string()]);
    }

    fn job() -> Model {
        let mut m = Model::new("Job");
        for name in ["id", "status", "tier", "is_primary", "started_at"] {
            m.fields
                .push(Field::new(name, TypeRef::Primitive(PrimitiveType::String)));
        }
        m
    }

    fn resolved(doc: &str) -> RuleRegister {
        let mut r = RuleRegister::default();
        r.read_file("job.hook.yaml", doc);
        let models = vec![job()];
        r.bind(&models);
        r.check(&models);
        r.mark_compiled(&models, &Default::default());
        r
    }

    #[test]
    fn a_key_is_resolved_against_the_model_it_belongs_to() {
        let r = resolved(
            "name: Job\nmodel: Job\nstates:\n  field: status\n  values:\n    pending: { initial: true }\n    done: {}\n  transitions:\n    finish: { from: pending, to: done }\ntriggers:\n  before_create:\n    actions: [\"log(a)\"]\n  after_update_tier:\n    actions: [\"log(b)\"]\n  after_update_is_primary_true:\n    actions: [\"log(c)\"]\n  after_finish:\n    actions: [\"log(d)\"]\n  after_login:\n    actions: [\"log(e)\"]\n  sweep:\n    schedule: \"every 5.minutes\"\n    actions: [\"trigger: x\"]\n  after_update_nope:\n    actions: [\"log(f)\"]\n  loose_label:\n    actions: [\"log(g)\"]\n",
        );
        let when: Vec<(String, Option<TriggerWhen>)> = r
            .triggers
            .iter()
            .map(|t| (t.key.clone(), t.when.clone()))
            .collect();
        assert_eq!(
            when[0].1,
            Some(TriggerWhen::Write {
                phase: Phase::Before,
                op: Op::Create
            })
        );
        assert_eq!(
            when[1].1,
            Some(TriggerWhen::FieldChange {
                phase: Phase::After,
                field: "tier".into(),
                value: None
            })
        );
        assert_eq!(
            when[2].1,
            Some(TriggerWhen::FieldChange {
                phase: Phase::After,
                field: "is_primary".into(),
                value: Some("true".into())
            }),
            "the longest field the key spells, the rest is the value"
        );
        assert_eq!(
            when[3].1,
            Some(TriggerWhen::Transition {
                verb: "finish".into()
            })
        );
        assert_eq!(when[4].1, Some(TriggerWhen::HandFired));
        assert_eq!(when[5].1, Some(TriggerWhen::Scheduled));
        assert_eq!(when[6].1, None);
        assert!(
            r.triggers[6]
                .problems
                .iter()
                .any(|p| p.contains("no field that `nope` spells")),
            "{:?}",
            r.triggers[6].problems
        );
        assert!(
            r.triggers[7]
                .problems
                .iter()
                .any(|p| p.contains("names no event")),
            "{:?}",
            r.triggers[7].problems
        );
        assert!(
            r.triggers[..6].iter().all(|t| t.problems.is_empty()),
            "{:?}",
            r.triggers.iter().map(|t| &t.problems).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_trigger_naming_no_model_or_no_state_is_a_problem() {
        let r = resolved(
            "name: Job\nmodel: Job\nstates:\n  field: status\n  values:\n    pending: { initial: true }\ntriggers:\n  after_x:\n    entity: Ghost\n    actions: [\"log(a)\"]\n",
        );
        assert!(
            r.triggers[0]
                .problems
                .iter()
                .any(|p| p.contains("`Ghost`, which is not a model")),
            "{:?}",
            r.triggers[0].problems
        );
        let r = resolved("name: Job\nmodel: Job\nstates:\n  field: status\n  values:\n    pending:\n      initial: true\n      on_enter: [\"log(a)\"]\n  transitions:\n    go: { from: pending, to: pending }\n");
        assert_eq!(
            r.triggers[0].when,
            Some(TriggerWhen::Enter("pending".into()))
        );
    }
}
