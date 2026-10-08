//! Read one hook file's raw YAML into register declarations.
//!
//! Hook files come in two shapes. The map format names its model (`name:`,
//! `model:`) and keys rules by name; the list format names a `module:` or
//! `model:` and lists rules, often with `assert:` statements, and declares
//! its machines under `state_machines:`. This reader takes both, and every
//! key it does not know becomes a warning instead of disappearing.

use serde_yaml::{Mapping, Value};

use super::{DeclaredMachine, DeclaredRule, DeclaredTransition, EnforcementStatus, RegisterWarning, RuleBody, SourceRef};

/// What one hook file declares.
#[derive(Debug, Default)]
pub struct ReadHookFile {
    pub rules: Vec<DeclaredRule>,
    pub machines: Vec<DeclaredMachine>,
    pub warnings: Vec<RegisterWarning>,
}

/// Top-level keys: the ones read here, and the ones that carry no rule
/// (documentation, module wiring read by the index parser).
const TOP_LEVEL_KEYS: &[&str] = &[
    "name", "model", "module", "entity", "states", "state_machines", "rules", "permissions", "triggers",
    "computed", "version", "description", "note", "notes", "imports", "config", "events", "scheduled_jobs",
];
const RULE_KEYS: &[&str] = &[
    "name", "id", "when", "condition", "assert", "message", "code", "error_code", "severity", "enforcement",
    "justification", "entity", "unique_on", "on_conflict", "check", "note", "notes", "description",
];
const MACHINE_KEYS: &[&str] = &[
    "entity", "field", "initial", "states", "values", "transitions", "guards", "effects", "lifecycle",
    "description", "shape", "note", "notes",
];
const TRANSITION_KEYS: &[&str] = &[
    "from", "to", "on", "name", "event", "guard", "condition", "roles", "message", "id", "write", "note",
    "notes", "description", "actions", "on_failure",
];
const STATE_KEYS: &[&str] = &["name", "initial", "final", "on_enter", "on_exit", "description", "note", "notes"];

/// Read `content`, the text of the hook file `file`.
pub fn read_hook_file(file: &str, content: &str) -> ReadHookFile {
    let mut out = ReadHookFile::default();
    let lines = Lines::new(content);
    let at = |line: Option<usize>| SourceRef { file: file.to_string(), line };
    let root = match serde_yaml::from_str::<Value>(content) {
        Ok(Value::Mapping(m)) => m,
        // A file that is not valid YAML declares nothing anyone can read, and
        // the hook loader can mistake it for an index file and drop it whole,
        // so say so here rather than return an empty file.
        Err(e) => {
            out.warnings.push(RegisterWarning {
                source: at(e.location().map(|l| l.line())),
                message: format!("the file is not valid YAML, so nothing it declares is read: {e}"),
            });
            return out;
        }
        // An empty or non-mapping file declares nothing.
        _ => return out,
    };

    for key in root.keys().filter_map(Value::as_str) {
        if !TOP_LEVEL_KEYS.contains(&key) {
            out.warnings.push(RegisterWarning {
                source: at(lines.find(&[format!("{key}:")], 0)),
                message: format!("top-level key `{key}` is not read by the generator"),
            });
        }
    }

    // The file's names for its model, most specific first.
    let names: Vec<String> = ["entity", "name", "model", "module"]
        .iter()
        .filter_map(|k| str_at(&root, k))
        .collect();
    let Some((file_target, file_alternatives)) = names.split_first() else {
        return out;
    };
    let list_format = matches!(get(&root, "rules"), Some(Value::Sequence(_))) || get(&root, "name").is_none();

    if list_format {
        for key in ["triggers", "computed"] {
            if get(&root, key).is_some() {
                out.warnings.push(RegisterWarning {
                    source: at(lines.find(&[format!("{key}:")], 0)),
                    message: format!("`{key}:` is not read by the generator in a hook file of the list format"),
                });
            }
        }
    }

    read_rules(&root, file_target, file_alternatives, &lines, &at, &mut out);
    if let Some(Value::Mapping(states)) = get(&root, "states") {
        let line = lines.find(&["states:".to_string()], 0);
        read_machine(states, file_target, file_alternatives, at(line), "states", &mut out);
    }
    if let Some(machines) = get(&root, "state_machines") {
        match machines {
            Value::Sequence(seq) => {
                let mut cursor = lines.find(&["state_machines:".to_string()], 0).unwrap_or(0);
                for (i, machine) in seq.iter().enumerate() {
                    let Value::Mapping(m) = machine else { continue };
                    let entity = str_at(m, "entity");
                    let line = entity
                        .as_ref()
                        .and_then(|e| lines.find(&[format!("entity: {e}")], cursor))
                        .or_else(|| lines.find(&["field:".to_string()], cursor));
                    cursor = line.unwrap_or(cursor);
                    let (target, alternatives) = match &entity {
                        Some(e) => (e.as_str(), &[][..]),
                        None => (file_target.as_str(), file_alternatives),
                    };
                    read_machine(m, target, alternatives, at(line), &format!("state_machines[{i}]"), &mut out);
                }
            }
            _ => out.warnings.push(RegisterWarning {
                source: at(lines.find(&["state_machines:".to_string()], 0)),
                message: "`state_machines:` must be a list of machines".to_string(),
            }),
        }
    }
    out
}

fn read_rules(
    root: &Mapping,
    file_target: &str,
    file_alternatives: &[String],
    lines: &Lines,
    at: &dyn Fn(Option<usize>) -> SourceRef,
    out: &mut ReadHookFile,
) {
    // (name, body) pairs from either format.
    let entries: Vec<(String, &Mapping)> = match get(root, "rules") {
        Some(Value::Mapping(map)) => map
            .iter()
            .filter_map(|(k, v)| Some((k.as_str()?.to_string(), v.as_mapping()?)))
            .collect(),
        Some(Value::Sequence(seq)) => seq
            .iter()
            .filter_map(Value::as_mapping)
            .map(|m| (str_at(m, "name").unwrap_or_else(|| "unnamed".to_string()), m))
            .collect(),
        _ => return,
    };
    let mut cursor = lines.find(&["rules:".to_string()], 0).unwrap_or(0);
    for (name, rule) in entries {
        let id = str_at(rule, "id");
        let mut needles = vec![format!("name: {name}"), format!("{name}:")];
        if let Some(id) = &id {
            needles.insert(0, format!("id: {id}"));
        }
        let line = lines.find(&needles, cursor);
        cursor = line.unwrap_or(cursor);
        let source = at(line);

        for key in rule.keys().filter_map(Value::as_str) {
            if !RULE_KEYS.contains(&key) {
                out.warnings.push(RegisterWarning {
                    source: source.clone(),
                    message: format!("rule `{name}`: key `{key}` is not read by the generator"),
                });
            }
        }

        let body = if let Some(check) = get(rule, "check") {
            RuleBody::Catalog(super::catalog::read(check))
        } else if let Some(c) = str_at(rule, "condition").filter(|c| !c.trim().is_empty()) {
            RuleBody::Condition(c)
        } else if let Some(a) = str_at(rule, "assert").filter(|a| !a.trim().is_empty()) {
            RuleBody::Assertion(a)
        } else if let Some(fields) = get(rule, "unique_on") {
            RuleBody::UniqueOn {
                fields: strings(fields),
                on_conflict: str_at(rule, "on_conflict"),
            }
        } else {
            out.warnings.push(RegisterWarning {
                source: source.clone(),
                message: format!("rule `{name}` declares nothing to check"),
            });
            RuleBody::Empty
        };

        let entity = str_at(rule, "entity");
        let (target, alternatives) = match entity {
            Some(e) => (e, Vec::new()),
            None => (file_target.to_string(), file_alternatives.to_vec()),
        };
        out.rules.push(DeclaredRule {
            source,
            id,
            name,
            target,
            alternatives,
            bound_model: None,
            body,
            when: get(rule, "when").map(strings).unwrap_or_default(),
            code: str_at(rule, "code").or_else(|| str_at(rule, "error_code")),
            message: str_at(rule, "message"),
            severity: str_at(rule, "severity"),
            enforcement: str_at(rule, "enforcement"),
            justification: str_at(rule, "justification"),
            status: EnforcementStatus::Unenforced,
            expression: super::expr::ExpressionCheck::NotAnExpression,
        });
    }
}

fn read_machine(
    m: &Mapping,
    target: &str,
    alternatives: &[String],
    source: SourceRef,
    path: &str,
    out: &mut ReadHookFile,
) {
    for key in m.keys().filter_map(Value::as_str) {
        if !MACHINE_KEYS.contains(&key) {
            out.warnings.push(RegisterWarning {
                source: source.clone(),
                message: format!("{path}: key `{key}` is not read by the generator"),
            });
        }
    }
    let Some(field) = str_at(m, "field").filter(|f| !f.is_empty()) else {
        out.warnings.push(RegisterWarning {
            source,
            message: format!("{path}: a state machine needs the `field:` that holds its state"),
        });
        return;
    };

    // States: `states:` (list format machines) or `values:` (hook `states:` blocks),
    // each a list of names, a list of maps, or a map keyed by name.
    let mut initial = str_at(m, "initial");
    let mut states = Vec::new();
    let state_list = get(m, "values").or_else(|| get(m, "states"));
    let mut take_state = |name: String, detail: Option<&Mapping>| {
        if let Some(d) = detail {
            for key in d.keys().filter_map(Value::as_str) {
                if !STATE_KEYS.contains(&key) {
                    out.warnings.push(RegisterWarning {
                        source: source.clone(),
                        message: format!("{path}: state `{name}`: key `{key}` is not read by the generator"),
                    });
                }
            }
            if initial.is_none() && matches!(get(d, "initial"), Some(Value::Bool(true))) {
                initial = Some(name.clone());
            }
        }
        states.push(name);
    };
    match state_list {
        Some(Value::Sequence(seq)) => {
            for s in seq {
                match s {
                    Value::String(name) => take_state(name.clone(), None),
                    Value::Mapping(d) => {
                        if let Some(name) = str_at(d, "name") {
                            take_state(name, Some(d));
                        }
                    }
                    _ => {}
                }
            }
        }
        Some(Value::Mapping(map)) => {
            for (k, v) in map {
                if let Some(name) = k.as_str() {
                    take_state(name.to_string(), v.as_mapping());
                }
            }
        }
        _ => {}
    }

    let guards = pairs(get(m, "guards"));
    let effects = pairs(get(m, "effects"));

    let mut transitions = Vec::new();
    let entries: Vec<(Option<String>, &Mapping)> = match get(m, "transitions") {
        Some(Value::Sequence(seq)) => seq.iter().filter_map(Value::as_mapping).map(|t| (None, t)).collect(),
        Some(Value::Mapping(map)) => map
            .iter()
            .filter_map(|(k, v)| Some((k.as_str().map(String::from), v.as_mapping()?)))
            .collect(),
        _ => Vec::new(),
    };
    for (key, t) in entries {
        let verb = key.or_else(|| str_at(t, "on")).or_else(|| str_at(t, "name")).or_else(|| str_at(t, "event"));
        let label = verb.clone().unwrap_or_else(|| "a transition".to_string());
        for k in t.keys().filter_map(Value::as_str) {
            if !TRANSITION_KEYS.contains(&k) {
                out.warnings.push(RegisterWarning {
                    source: source.clone(),
                    message: format!("{path}: transition `{label}`: key `{k}` is not read by the generator"),
                });
            }
        }
        let to = match get(t, "to") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Null) => crate::ast::NULL_STATE.to_string(),
            _ => {
                out.warnings.push(RegisterWarning {
                    source: source.clone(),
                    message: format!("{path}: transition `{label}` has no `to:` state"),
                });
                continue;
            }
        };
        let from = match get(t, "from") {
            None => vec!["*".to_string()],
            Some(Value::Null) => vec![crate::ast::NULL_STATE.to_string()],
            Some(v) => strings(v),
        };
        let guard = str_at(t, "guard").or_else(|| str_at(t, "condition"));
        if let Some(g) = &guard {
            let is_name = g.chars().all(|c| c.is_alphanumeric() || c == '_');
            if is_name && !guards.iter().any(|(name, _)| name == g) && !looks_like_call_or_flag(g) {
                out.warnings.push(RegisterWarning {
                    source: source.clone(),
                    message: format!("{path}: transition `{label}` names guard `{g}`, which `guards:` does not define"),
                });
            }
        }
        transitions.push(DeclaredTransition {
            verb,
            from,
            to,
            guard,
            roles: get(t, "roles").map(strings).unwrap_or_default(),
            message: str_at(t, "message"),
            guard_check: super::expr::ExpressionCheck::NotAnExpression,
        });
    }

    out.machines.push(DeclaredMachine {
        source,
        target: target.to_string(),
        alternatives: alternatives.to_vec(),
        bound_model: None,
        field,
        initial,
        states,
        transitions,
        guards,
        effects,
    });
}

/// A bare word guard in a map-format hook is usually a boolean field of the
/// row (`is_verified`), not a reference into `guards:`; only a list-format
/// machine, which declares `guards:`, is checked for undefined names.
fn looks_like_call_or_flag(g: &str) -> bool {
    g.starts_with("is_") || g.starts_with("has_") || g.starts_with("can_")
}

fn get<'a>(m: &'a Mapping, key: &str) -> Option<&'a Value> {
    m.get(Value::String(key.to_string()))
}

fn str_at(m: &Mapping, key: &str) -> Option<String> {
    match get(m, key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// A string or a list of strings.
fn strings(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => vec![s.clone()],
        Value::Sequence(seq) => seq
            .iter()
            .filter_map(|x| match x {
                Value::String(s) => Some(s.clone()),
                Value::Null => Some(crate::ast::NULL_STATE.to_string()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A map of names to expressions, in declaration order.
fn pairs(v: Option<&Value>) -> Vec<(String, String)> {
    match v {
        Some(Value::Mapping(map)) => map
            .iter()
            .filter_map(|(k, v)| Some((k.as_str()?.to_string(), v.as_str()?.to_string())))
            .collect(),
        _ => Vec::new(),
    }
}

/// Line lookup over the file text. YAML values carry no positions, so a
/// declaration is found by its identifying text, searching forward from the
/// previous one so repeated names resolve in order.
struct Lines<'a> {
    lines: Vec<&'a str>,
}

impl<'a> Lines<'a> {
    fn new(content: &'a str) -> Self {
        Self { lines: content.lines().collect() }
    }

    /// The 1-based line, at or after `from`, holding any of `needles`.
    fn find(&self, needles: &[String], from: usize) -> Option<usize> {
        let start = from.saturating_sub(1);
        self.lines
            .iter()
            .enumerate()
            .skip(start)
            .find(|(_, l)| {
                let l = l.trim_start().trim_start_matches("- ").trim_start_matches("{ ");
                needles.iter().any(|n| l.starts_with(n.as_str()) || l.contains(&format!(" {n}")))
            })
            .map(|(i, _)| i + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST_FORMAT: &str = r#"
module: accounting
state_machines:
  - entity: Journal
    field: status
    initial: draft
    states: [draft, approved, posted]
    transitions:
      - { from: draft, to: approved, on: approve }
      - { from: approved, to: posted, on: post, guard: balanced }
    guards:
      balanced: "total_debit == total_credit"
    effects:
      post: "set posted_at = now()"
rules:
  - id: R1
    name: double_entry_balanced
    when: posting
    assert: "sum(lines.debit) == sum(lines.credit)"
    error_code: unbalanced
  - id: R6
    name: idempotent_source
    unique_on: [company_id, source_type]
    on_conflict: return_existing
  - name: typo
    asert: "x"
"#;

    #[test]
    fn the_list_format_keeps_its_assertions_ids_codes_and_keys() {
        let read = read_hook_file("journal.hook.yaml", LIST_FORMAT);
        let r1 = &read.rules[0];
        assert_eq!(r1.id.as_deref(), Some("R1"));
        assert_eq!(r1.body, RuleBody::Assertion("sum(lines.debit) == sum(lines.credit)".into()));
        assert_eq!(r1.code.as_deref(), Some("unbalanced"));
        assert_eq!(r1.when, vec!["posting".to_string()]);
        assert_eq!(r1.target, "accounting");
        assert_eq!(r1.source.line, Some(16), "the line of `- id: R1`");
        assert_eq!(
            read.rules[1].body,
            RuleBody::UniqueOn { fields: vec!["company_id".into(), "source_type".into()], on_conflict: Some("return_existing".into()) }
        );
        let warnings: Vec<_> = read.warnings.iter().map(|w| w.message.as_str()).collect();
        assert!(warnings.contains(&"rule `typo`: key `asert` is not read by the generator"), "{warnings:?}");
        assert!(warnings.contains(&"rule `typo` declares nothing to check"), "{warnings:?}");
    }

    #[test]
    fn a_state_machines_list_keeps_its_entity_guards_and_effects() {
        let read = read_hook_file("journal.hook.yaml", LIST_FORMAT);
        let m = &read.machines[0];
        assert_eq!((m.target.as_str(), m.field.as_str(), m.initial.as_deref()), ("Journal", "status", Some("draft")));
        assert_eq!(m.states, vec!["draft", "approved", "posted"]);
        let post = &m.transitions[1];
        assert_eq!(post.verb.as_deref(), Some("post"));
        assert_eq!(post.guard.as_deref(), Some("balanced"));
        assert_eq!(m.guard_expression("balanced"), "total_debit == total_credit");
        assert_eq!(m.effects, vec![("post".to_string(), "set posted_at = now()".to_string())]);
    }

    #[test]
    fn the_map_format_reads_rules_keyed_by_name_and_its_states_block() {
        let read = read_hook_file(
            "user.hook.yaml",
            r#"
name: User
model: user.model.yaml
states:
  field: status
  values:
    active: { initial: true }
    suspended: { final: true }
  transitions:
    suspend: { from: active, to: suspended, roles: [admin], message: "x" }
rules:
  email_unique:
    when: [create, update]
    condition: "count(User, email == $this.email) == 0"
    message: "taken"
    code: EMAIL_TAKEN
"#,
        );
        let rule = &read.rules[0];
        assert_eq!(rule.name, "email_unique");
        assert!(matches!(&rule.body, RuleBody::Condition(c) if c.starts_with("count(")));
        assert_eq!((rule.target.as_str(), rule.alternatives.as_slice()), ("User", &["user.model.yaml".to_string()][..]));
        let m = &read.machines[0];
        assert_eq!((m.initial.as_deref(), m.states.len()), (Some("active"), 2));
        assert_eq!(m.transitions[0].verb.as_deref(), Some("suspend"));
        assert!(read.warnings.is_empty(), "{:?}", read.warnings);
    }

    #[test]
    fn an_undefined_named_guard_and_an_unknown_top_level_key_are_reported() {
        let read = read_hook_file(
            "x.hook.yaml",
            "module: m\njobs: {}\nstate_machines:\n  - entity: A\n    field: s\n    states: [a, b]\n    transitions:\n      - { from: a, to: b, on: go, guard: ready }\n",
        );
        let warnings: Vec<_> = read.warnings.iter().map(|w| w.message.clone()).collect();
        assert!(warnings.iter().any(|w| w == "top-level key `jobs` is not read by the generator"), "{warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("names guard `ready`")), "{warnings:?}");
    }

    #[test]
    fn a_file_that_is_not_valid_yaml_is_reported_not_skipped() {
        let read = read_hook_file(
            "notification.hook.yaml",
            "name: Notification\nmodel: notification.model.yaml\ntriggers:\n  cleanup:\n    actions: [a]\n    actions: [b]\n",
        );
        assert!(read.rules.is_empty());
        let w = &read.warnings[0];
        assert!(w.message.starts_with("the file is not valid YAML"), "{}", w.message);
        assert!(w.message.contains("triggers.cleanup: duplicate entry with key \"actions\""), "{}", w.message);
        // serde_yaml places the error at the mapping that holds the duplicate.
        assert_eq!(w.source.line, Some(5), "{}", w.message);
    }
}
