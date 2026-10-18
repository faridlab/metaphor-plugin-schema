//! Compile a workflow that runs on the engine (`config.executor: engine`).
//!
//! The compiler reads the file as written ([`Workflow::source`]), checks every name it relies on
//! (steps, `next` targets, entities, state-machine verbs, events, expressions), and answers with
//! the workflow's *spec*, a JSON document in the shape of `backbone_core::workflow::WorkflowSpec`,
//! the typed dispatch the generated definition needs (`Arm`), and the hand-written surface the
//! module must implement: one method per bespoke verb, one per declared function.
//!
//! Anything the engine cannot honour is a problem in the answer, never a silent default: a function
//! call not listed under `functions:`, a bare name no step writes, a wait condition that is not an
//! equality on the event, an `update` or `delete` naming no record, a terminal status the engine
//! does not have, an entity the module lacks.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde_json::{json, Value as J};
use serde_yaml::Value as Y;

use crate::ast::expressions::{BinaryOp, Expression, FieldRef, Literal, UnaryOp};
use crate::ast::{Model, ModuleSchema, Workflow};
use crate::rules::expr::parse_dynamic;
use crate::utils::{enum_label, to_pascal_case, to_snake_case};

/// The actions the engine performs itself, without a hand method.
pub const ENGINE_VERBS: &[&str] = &[
    "audit_log",
    "emit_event",
    "send_email",
    "notify",
    "send_notification",
    "send_sms",
];

/// The functions every expression may call.
pub const BUILTIN_FUNCTIONS: &[&str] = &["now", "new_uuid", "coalesce", "len"];

/// What the typed dispatch of the generated definition does for one step.
#[derive(Debug, Clone, PartialEq)]
pub enum Arm {
    /// `create <Entity>`: the parameters make the entity's create DTO.
    Create { step: String, entity: String },
    /// `update <Entity>`: the parameters (but `id`) are the fields of a partial update.
    Update { step: String, entity: String },
    /// `delete <Entity>`.
    Delete { step: String, entity: String },
    /// `query <Entity>` (the first row) or `fetch <Entity>` with a filter (all rows): the parameters
    /// are equality filters.
    Query {
        step: String,
        entity: String,
        many: bool,
    },
    /// `find_by_id` or `fetch <Entity>` with an id.
    FindById { step: String, entity: String },
    /// A state-machine verb.
    Transition {
        step: String,
        entity: String,
        verb: String,
    },
    /// A bespoke verb: the generated `Actions` method of this name.
    Hand { step: String, method: String },
}

impl Arm {
    /// The step (or `compensate:<name>`) the arm answers.
    pub fn step(&self) -> &str {
        match self {
            Arm::Create { step, .. }
            | Arm::Update { step, .. }
            | Arm::Delete { step, .. }
            | Arm::Query { step, .. }
            | Arm::FindById { step, .. }
            | Arm::Transition { step, .. }
            | Arm::Hand { step, .. } => step,
        }
    }
}

/// One required method of the workflow's `Actions` trait for a bespoke verb.
#[derive(Debug, Clone, PartialEq)]
pub struct HandVerb {
    /// The method name.
    pub method: String,
    /// The verb as written.
    pub verb: String,
    /// The steps that call it.
    pub steps: Vec<String>,
}

/// The result of compiling one workflow.
#[derive(Debug, Clone, Default)]
pub struct Compiled {
    /// The spec, as `backbone_core::workflow::WorkflowSpec` reads it.
    pub spec: J,
    /// Everything that stops the workflow from running, each naming its step.
    pub problems: Vec<String>,
    /// Things worth knowing that do not stop it (an event no file declares, a hand verb).
    pub warnings: Vec<String>,
    /// The typed dispatch, one arm per step that performs an action or a transition.
    pub arms: Vec<Arm>,
    /// The bespoke verbs, in the order first used.
    pub hand: Vec<HandVerb>,
    /// The declared functions an expression calls, which the `Actions` trait implements.
    pub functions: Vec<String>,
    /// Events a step or a terminal stages.
    pub emits: Vec<String>,
    /// Events the workflow starts on or waits for.
    pub awaits: Vec<String>,
    /// Entities the dispatch writes through.
    pub entities: BTreeSet<String>,
    /// Whether the workflow runs end to end in one transaction (`persistence: false`).
    pub inline: bool,
}

impl Compiled {
    /// Every method the `Actions` trait requires: bespoke verbs, then functions.
    pub fn required_methods(&self) -> usize {
        self.hand.len() + self.functions.len()
    }
}

/// The method name of a declared function.
pub fn function_method(name: &str) -> String {
    format!("fn_{}", to_snake_case(name))
}

/// Whether `schema` has any workflow that runs on the engine.
pub fn has_engine_workflows(schema: &ModuleSchema) -> bool {
    schema.workflows.iter().any(Workflow::runs_on_engine)
}

/// The workflows of `schema` that run on the engine.
pub fn engine_workflows(schema: &ModuleSchema) -> impl Iterator<Item = &Workflow> {
    schema.workflows.iter().filter(|w| w.runs_on_engine())
}

/// Compile `wf`, a workflow of `schema` with `executor: engine`.
pub fn compile(schema: &ModuleSchema, wf: &Workflow) -> Compiled {
    let mut c = Compiler::new(schema, wf);
    c.run();
    c.finish()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    /// `event` is not in scope.
    Plain,
    /// The start event's extract, a wait's condition and its `on_event` outcome: `event` is the payload.
    Event,
}

struct Compiler<'a> {
    schema: &'a ModuleSchema,
    wf: &'a Workflow,
    problems: Vec<String>,
    warnings: Vec<String>,
    written: HashSet<String>,
    declared_functions: BTreeSet<String>,
    used_functions: BTreeSet<String>,
    arms: Vec<Arm>,
    hand: Vec<HandVerb>,
    emits: Vec<String>,
    awaits: Vec<String>,
    entities: BTreeSet<String>,
    step_names: Vec<String>,
    inline: bool,
    spec: J,
}

fn get<'v>(v: &'v Y, key: &str) -> Option<&'v Y> {
    v.get(key).filter(|x| !x.is_null())
}

fn text(v: &Y, key: &str) -> Option<String> {
    get(v, key).and_then(|x| match x {
        Y::String(s) => Some(s.clone()),
        Y::Number(n) => Some(n.to_string()),
        Y::Bool(b) => Some(b.to_string()),
        _ => None,
    })
}

fn entries(v: &Y) -> Vec<(String, &Y)> {
    match v {
        Y::Mapping(m) => m
            .iter()
            .filter_map(|(k, v)| k.as_str().map(|k| (k.to_string(), v)))
            .collect(),
        _ => Vec::new(),
    }
}

fn keys(v: &Y) -> Vec<String> {
    entries(v).into_iter().map(|(k, _)| k).collect()
}

/// `15m`, `24h`, `30d`, `2w`, `90s` as seconds.
pub fn parse_duration(text: &str) -> Option<i64> {
    let t = text.trim();
    let split = t.find(|c: char| !c.is_ascii_digit())?;
    let (digits, unit) = t.split_at(split);
    let n: i64 = digits.parse().ok()?;
    let secs = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        "w" => 604_800,
        _ => return None,
    };
    n.checked_mul(secs)
}

fn unit_seconds(unit: &str) -> Option<i64> {
    Some(match unit {
        "second" | "seconds" => 1,
        "minute" | "minutes" => 60,
        "hour" | "hours" => 3600,
        "day" | "days" => 86_400,
        "week" | "weeks" => 604_800,
        _ => return None,
    })
}

impl<'a> Compiler<'a> {
    fn new(schema: &'a ModuleSchema, wf: &'a Workflow) -> Self {
        Self {
            schema,
            wf,
            problems: Vec::new(),
            warnings: Vec::new(),
            written: HashSet::new(),
            declared_functions: wf.functions.iter().cloned().collect(),
            used_functions: BTreeSet::new(),
            arms: Vec::new(),
            hand: Vec::new(),
            emits: Vec::new(),
            awaits: Vec::new(),
            entities: BTreeSet::new(),
            step_names: Vec::new(),
            inline: false,
            spec: J::Null,
        }
    }

    fn finish(self) -> Compiled {
        Compiled {
            spec: self.spec,
            problems: self.problems,
            warnings: self.warnings,
            arms: self.arms,
            hand: self.hand,
            functions: self.used_functions.into_iter().collect(),
            emits: self.emits,
            awaits: self.awaits,
            entities: self.entities,
            inline: self.inline,
        }
    }

    fn problem(&mut self, at: &str, msg: impl AsRef<str>) {
        let place = if at.is_empty() {
            String::new()
        } else {
            format!(", {at}")
        };
        self.problems.push(format!(
            "workflow `{}`{place}: {}",
            self.wf.name,
            msg.as_ref()
        ));
    }

    fn unsupported_keys(&mut self, at: &str, node: &Y, known: &[&str]) {
        for k in keys(node) {
            if !known.contains(&k.as_str()) {
                self.problem(
                    at,
                    format!(
                        "`{k}` is not read by the engine; remove it or run the workflow by hand"
                    ),
                );
            }
        }
    }

    // ── expressions ───────────────────────────────────────────────────────────────────────

    fn expr(&mut self, at: &str, src: &str, place: Place) -> J {
        match parse_dynamic(src) {
            Err(e) => {
                self.problem(
                    at,
                    format!("`{src}` is not an expression the engine reads: {e}"),
                );
                J::Null
            }
            Ok(ast) => match self.lower(&ast, place) {
                Ok(j) => j,
                Err(e) => {
                    self.problem(at, format!("`{src}`: {e}"));
                    J::Null
                }
            },
        }
    }

    fn lower(&mut self, e: &Expression, place: Place) -> Result<J, String> {
        use Expression as E;
        Ok(match e {
            E::Literal(l) => json!({ "lit": match l {
                Literal::String(s) => J::String(s.clone()),
                Literal::Int(i) => J::from(*i),
                Literal::Float(f) => J::from(*f),
                Literal::Bool(b) => J::Bool(*b),
                Literal::Null => J::Null,
            } }),
            E::FieldRef(_) => self.path(e, place)?,
            E::MemberAccess { object, member } => {
                if let E::Literal(Literal::Int(n)) = object.as_ref() {
                    if let Some(unit) = unit_seconds(member) {
                        return Ok(json!({ "dur": n * unit }));
                    }
                }
                if member == "length" {
                    let obj = self.lower(object, place)?;
                    return Ok(json!({ "method": { "obj": obj, "name": "length", "args": [] } }));
                }
                self.path(e, place)?
            }
            E::MethodCall {
                object,
                method,
                args,
            } if method == "[]" => {
                let _ = (object, args);
                self.path(e, place)?
            }
            E::MethodCall {
                object,
                method,
                args,
            } => {
                if !["contains", "includes", "starts_with", "ends_with"].contains(&method.as_str())
                {
                    return Err(format!("the method `.{method}()` is not part of the workflow expression set (contains, includes, starts_with, ends_with, length)"));
                }
                let obj = self.lower(object, place)?;
                let mut lowered = Vec::new();
                for a in args {
                    lowered.push(self.lower(a, place)?);
                }
                json!({ "method": { "obj": obj, "name": method, "args": lowered } })
            }
            E::Unary { op, expr } => {
                let inner = self.lower(expr, place)?;
                match op {
                    UnaryOp::Not => json!({ "not": inner }),
                    UnaryOp::Neg => json!({ "neg": inner }),
                }
            }
            E::Binary { left, op, right } => {
                let l = self.lower(left, place)?;
                let r = self.lower(right, place)?;
                match op {
                    BinaryOp::In => json!({ "in": { "l": l, "r": r, "negate": false } }),
                    BinaryOp::NotIn => json!({ "in": { "l": l, "r": r, "negate": true } }),
                    BinaryOp::Contains
                    | BinaryOp::StartsWith
                    | BinaryOp::EndsWith
                    | BinaryOp::Matches => return Err(
                        "`matches` and the word operators are not in the workflow expression set"
                            .into(),
                    ),
                    other => {
                        let op = match other {
                            BinaryOp::Eq => "eq",
                            BinaryOp::Ne => "ne",
                            BinaryOp::Lt => "lt",
                            BinaryOp::Le => "le",
                            BinaryOp::Gt => "gt",
                            BinaryOp::Ge => "ge",
                            BinaryOp::And => "and",
                            BinaryOp::Or => "or",
                            BinaryOp::Add => "add",
                            BinaryOp::Sub => "sub",
                            BinaryOp::Mul => "mul",
                            BinaryOp::Div => "div",
                            BinaryOp::Mod => "mod",
                            _ => unreachable!("handled above"),
                        };
                        json!({ "bin": { "op": op, "l": l, "r": r } })
                    }
                }
            }
            E::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                let c = self.lower(condition, place)?;
                let t = self.lower(then_expr, place)?;
                let el = self.lower(else_expr, place)?;
                json!({ "if": { "c": c, "t": t, "e": el } })
            }
            E::Array(items) => {
                let mut out = Vec::new();
                for i in items {
                    out.push(self.lower(i, place)?);
                }
                json!({ "array": out })
            }
            E::FunctionCall { name, args } => {
                let mut lowered = Vec::new();
                for a in args {
                    lowered.push(self.lower(a, place)?);
                }
                match name.as_str() {
                    "now" if args.is_empty() => J::String("now".into()),
                    "now" => return Err("`now()` takes no arguments".into()),
                    n if BUILTIN_FUNCTIONS.contains(&n) => json!({ "call": { "name": n, "args": lowered } }),
                    n if self.declared_functions.contains(n) => {
                        self.used_functions.insert(n.to_string());
                        json!({ "call": { "name": n, "args": lowered } })
                    }
                    n => {
                        return Err(format!(
                            "the function `{n}` is not in the expression set (now, new_uuid, coalesce, len); list it under `functions:` and implement it on the workflow's Actions trait"
                        ))
                    }
                }
            }
            E::Context(c) => {
                return Err(format!("`${c}` is not available in a workflow expression"))
            }
            E::Aggregate { .. } | E::Raw(_) => {
                return Err("this expression form is not in the workflow expression set".into())
            }
        })
    }

    /// A path: `context.a.b`, `event.x`, `result[0].id`, a bare name a step writes, or `now`.
    fn path(&mut self, e: &Expression, place: Place) -> Result<J, String> {
        let (root, segs) = flatten(e).ok_or_else(|| match context_root(e) {
            Some(c) => format!("`${c}` is not available in a workflow expression"),
            None => "this is not a path the engine can read".to_string(),
        })?;
        let json_segs = |segs: &[Seg]| -> Vec<J> {
            segs.iter()
                .map(|s| match s {
                    Seg::Key(k) => json!({ "key": k }),
                    Seg::Index(i) => json!({ "index": i }),
                })
                .collect()
        };
        match root.as_str() {
            "now" if segs.is_empty() => Ok(J::String("now".into())),
            "context" | "result" | "error" | "config" | "steps" => {
                Ok(json!({ "path": { "root": root, "path": json_segs(&segs) } }))
            }
            "event" => {
                if place == Place::Plain {
                    return Err("`event` is only readable in `extract`, a wait's condition and its `on_event` outcome".into());
                }
                Ok(json!({ "path": { "root": "event", "path": json_segs(&segs) } }))
            }
            name if self.written.contains(name) => {
                let mut all = vec![Seg::Key(name.to_string())];
                all.extend(segs);
                Ok(json!({ "path": { "root": "context", "path": json_segs(&all) } }))
            }
            name => Err(format!(
                "`{name}` is not a name the workflow writes (`context:`, `trigger.extract` or a `set:`); write `context.{name}` if it is a context key"
            )),
        }
    }

    /// A string value: a literal, or text with `{{ }}` holes.
    fn template(&mut self, at: &str, s: &str, place: Place) -> J {
        if !s.contains("{{") {
            return json!({ "lit": s });
        }
        let mut parts: Vec<J> = Vec::new();
        let mut rest = s;
        while let Some(open) = rest.find("{{") {
            if open > 0 {
                parts.push(json!({ "text": &rest[..open] }));
            }
            let after = &rest[open + 2..];
            let Some(close) = after.find("}}") else {
                self.problem(at, format!("`{s}` opens `{{{{` and never closes it"));
                return J::Null;
            };
            let inner = after[..close].trim();
            let compiled = self.expr(at, inner, place);
            parts.push(json!({ "expr": compiled }));
            rest = &after[close + 2..];
        }
        if !rest.is_empty() {
            parts.push(json!({ "text": rest }));
        }
        json!({ "template": parts })
    }

    /// A parameter value of any YAML shape.
    fn tpl(&mut self, at: &str, v: &Y, place: Place) -> J {
        match v {
            Y::String(s) => self.template(at, s, place),
            Y::Mapping(_) => {
                let mut out = Vec::new();
                for (k, v) in entries(v) {
                    let inner = self.tpl(at, v, place);
                    out.push(json!([k, inner]));
                }
                json!({ "map": out })
            }
            Y::Sequence(items) => {
                let mut out = Vec::new();
                for i in items {
                    out.push(self.tpl(at, i, place));
                }
                json!({ "list": out })
            }
            other => json!({ "lit": serde_json::to_value(other).unwrap_or(J::Null) }),
        }
    }

    fn params(&mut self, at: &str, node: Option<&Y>, place: Place) -> Vec<(String, J)> {
        let mut out = Vec::new();
        if let Some(n) = node {
            for (k, v) in entries(n) {
                out.push((k, self.tpl(at, v, place)));
            }
        }
        out
    }

    fn pairs(pairs: Vec<(String, J)>) -> J {
        J::Array(pairs.into_iter().map(|(k, v)| json!([k, v])).collect())
    }

    /// A duration: `15m` as seconds, or text with a hole read at run time.
    fn duration(&mut self, at: &str, raw: &str) -> J {
        if raw.contains("{{") {
            let t = self.template(at, raw, Place::Plain);
            return match t.get("template") {
                Some(parts) => json!({ "tpl": parts }),
                None => J::Null,
            };
        }
        match parse_duration(raw) {
            Some(s) => json!({ "secs": s }),
            None => {
                self.problem(
                    at,
                    format!("`{raw}` is not a duration (30s, 15m, 24h, 7d, 2w)"),
                );
                J::Null
            }
        }
    }

    // ── structure ─────────────────────────────────────────────────────────────────────────

    fn run(&mut self) {
        let Some(src) = self.wf.source.clone() else {
            self.problem(
                "",
                "the file was not kept as read, so the engine cannot compile it",
            );
            return;
        };
        self.unsupported_keys(
            "",
            &src,
            &[
                "name",
                "description",
                "version",
                "trigger",
                "config",
                "context",
                "steps",
                "compensation",
                "implemented_by",
                "functions",
            ],
        );
        if let Some(on) = get(&src, "on_success").or_else(|| get(&src, "on_failure")) {
            let _ = on;
            self.problem(
                "",
                "workflow-level `on_success` / `on_failure` handlers are not run by the engine",
            );
        }
        // Names the workflow writes: context keys, extracted keys, and every `set:` key anywhere.
        if let Some(c) = get(&src, "context") {
            self.written.extend(keys(c));
        }
        if let Some(t) = get(&src, "trigger").and_then(|t| get(t, "extract")) {
            self.written.extend(keys(t));
        }
        collect_set_keys(&src, &mut self.written);

        let trigger = get(&src, "trigger");
        let trigger_event = trigger.and_then(|t| text(t, "event"));
        let Some(trigger_event) = trigger_event else {
            self.problem(
                "",
                "no `trigger.event`: the engine starts an instance on an event",
            );
            return;
        };
        if let Some(t) = trigger {
            self.unsupported_keys("trigger", t, &["event", "extract"]);
        }
        self.awaits.push(trigger_event.clone());

        let mut extract = Vec::new();
        if let Some(e) = trigger.and_then(|t| get(t, "extract")) {
            for (k, v) in entries(e) {
                let at = format!("extract `{k}`");
                let Some(s) = v.as_str() else {
                    self.problem(&at, "an extract is an expression written as text");
                    continue;
                };
                let compiled = self.expr(&at, s, Place::Event);
                extract.push((k, compiled));
            }
        }

        let mut context = Vec::new();
        if let Some(c) = get(&src, "context") {
            for (k, v) in entries(c) {
                context.push(json!([k, serde_json::to_value(v).unwrap_or(J::Null)]));
            }
        }

        let config = get(&src, "config");
        if let Some(c) = config {
            self.unsupported_keys(
                "config",
                c,
                &[
                    "timeout",
                    "persistence",
                    "executor",
                    "retry",
                    "retry_policy",
                ],
            );
        }
        let persistence = config
            .and_then(|c| c.get("persistence"))
            .and_then(Y::as_bool)
            .unwrap_or(true);
        self.inline = !persistence;
        let timeout = config
            .and_then(|c| text(c, "timeout"))
            .map(|t| self.duration("config.timeout", &t));
        let retry = config
            .and_then(|c| get(c, "retry").or_else(|| get(c, "retry_policy")))
            .map(|r| self.retry("config.retry", r));

        let version = get(&src, "version").and_then(Y::as_i64).unwrap_or(1);

        // Steps: names first, so a `next` can be checked against all of them.
        let steps = match get(&src, "steps") {
            Some(Y::Sequence(s)) if !s.is_empty() => s.clone(),
            _ => {
                self.problem("", "no steps");
                return;
            }
        };
        for s in &steps {
            match text(s, "name") {
                Some(n) => {
                    if self.step_names.contains(&n) {
                        self.problem(&format!("step `{n}`"), "the name is used by two steps");
                    }
                    self.step_names.push(n);
                }
                None => self.problem("", "a step has no name"),
            }
        }
        let mut compiled_steps = Vec::new();
        for s in &steps {
            if let Some(js) = self.step(s) {
                compiled_steps.push(js);
            }
        }

        let mut compensation = Vec::new();
        if let Some(Y::Sequence(list)) = get(&src, "compensation") {
            for c in list {
                if let Some(js) = self.compensation(c) {
                    compensation.push(js);
                }
            }
        }

        if self.inline {
            for s in &steps {
                if text(s, "type").as_deref() == Some("wait") {
                    let n = text(s, "name").unwrap_or_default();
                    self.problem(&format!("step `{n}`"), "a workflow with `persistence: false` runs in one transaction and cannot wait");
                }
            }
            if !compensation.is_empty() {
                self.problem("", "a workflow with `persistence: false` runs in one transaction: a failure undoes it, so it has no `compensation`");
            }
        }

        let mut spec = json!({
            "name": to_pascal_case(&self.wf.name),
            "version": version,
            "trigger_event": trigger_event,
            "extract": Self::pairs(extract),
            "context": context,
            "persistence": persistence,
            "steps": compiled_steps,
            "compensation": compensation,
        });
        if let Some(t) = timeout.filter(|t| !t.is_null()) {
            spec["timeout"] = t;
        }
        if let Some(r) = retry.filter(|r| !r.is_null()) {
            spec["retry"] = r;
        }
        self.spec = spec;

        // Events: what the module itself publishes must be declared; others may come from elsewhere.
        let declared: BTreeSet<&str> = self
            .schema
            .rule_register
            .events
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        for e in self.emits.clone() {
            if !declared.contains(e.as_str()) {
                self.problem(
                    "",
                    format!("it emits `{e}`, which no file of the module declares under `events:`"),
                );
            }
        }
        for e in self.awaits.clone().into_iter().collect::<BTreeSet<_>>() {
            if !declared.contains(e.as_str()) {
                self.warnings.push(format!(
                    "workflow `{}` starts on or waits for `{e}`, which this module does not declare under `events:`: it must be published by the module that owns it",
                    self.wf.name
                ));
            }
        }
        for f in self.declared_functions.clone() {
            if !self.used_functions.contains(&f) {
                self.warnings.push(format!(
                    "workflow `{}` lists the function `{f}` and no expression calls it",
                    self.wf.name
                ));
            }
        }
    }

    fn retry(&mut self, at: &str, r: &Y) -> J {
        self.unsupported_keys(at, r, &["max_attempts", "backoff"]);
        let attempts = get(r, "max_attempts").and_then(Y::as_u64).unwrap_or(3);
        let backoff = match get(r, "backoff") {
            None => json!("exponential"),
            Some(Y::String(s)) if s == "exponential" => json!("exponential"),
            Some(Y::String(s)) if s == "fixed" => json!({ "fixed": 5 }),
            Some(Y::String(s)) => match parse_duration(s) {
                Some(secs) => json!({ "fixed": secs }),
                None => {
                    self.problem(
                        at,
                        format!("backoff `{s}` is not `exponential` or a duration"),
                    );
                    json!("exponential")
                }
            },
            Some(_) => {
                self.problem(at, "backoff is `exponential` or a duration");
                json!("exponential")
            }
        };
        json!({ "max_attempts": attempts, "backoff": backoff })
    }

    fn next_target(&mut self, at: &str, next: &str) {
        if !self.step_names.iter().any(|n| n == next) {
            self.problem(at, format!("`next: {next}` names no step"));
        }
    }

    fn outcome(&mut self, at: &str, node: &Y, place: Place) -> J {
        self.unsupported_keys(at, node, &["set", "next"]);
        let mut set = Vec::new();
        if let Some(s) = get(node, "set") {
            for (k, v) in entries(s) {
                let inner = self.tpl(&format!("{at}, set `{k}`"), v, place);
                set.push((k, inner));
            }
        }
        let next = text(node, "next");
        if let Some(n) = &next {
            self.next_target(at, n);
        }
        let mut out = json!({ "set": Self::pairs(set) });
        if let Some(n) = next {
            out["next"] = J::String(n);
        }
        out
    }

    fn model(&self, name: &str) -> Option<&'a Model> {
        self.schema.models.iter().find(|m| m.name == name)
    }

    fn step(&mut self, s: &Y) -> Option<J> {
        let name = text(s, "name")?;
        let at = format!("step `{name}`");
        let kind = text(s, "type").unwrap_or_else(|| "action".to_string());
        let guard = get(s, "condition")
            .and_then(Y::as_str)
            .map(|c| self.expr(&format!("{at}, condition"), c, Place::Plain));
        let mut out = match kind.as_str() {
            "action" => self.action_step(&name, &at, s)?,
            "transition" => self.transition_step(&name, &at, s)?,
            "condition" => self.condition_step(&at, s)?,
            "wait" => self.wait_step(&at, s)?,
            "terminal" => self.terminal_step(&at, s)?,
            other => {
                self.problem(&at, format!("a `{other}` step is not run by the engine (action, transition, condition, wait and terminal are)"));
                return None;
            }
        };
        out["name"] = J::String(name);
        if let Some(g) = guard.filter(|g| !g.is_null()) {
            if matches!(kind.as_str(), "action" | "transition") {
                out["guard"] = g;
            } else {
                self.problem(
                    &at,
                    "only an action or a transition step takes a `condition`",
                );
            }
        }
        if let Some(f) = get(s, "on_failure") {
            out["on_failure"] = self.failure(&at, f);
        }
        if let Some(t) = get(s, "timeout").and_then(Y::as_str) {
            match parse_duration(t) {
                Some(secs) => out["call_timeout"] = J::from(secs),
                None => self.problem(&at, format!("`timeout: {t}` is not a duration")),
            }
        }
        Some(out)
    }

    fn failure(&mut self, at: &str, f: &Y) -> J {
        let at = format!("{at}, on_failure");
        self.unsupported_keys(&at, f, &["next", "retry", "backoff", "on_exhausted"]);
        let mut out = json!({});
        if let Some(n) = text(f, "next") {
            self.next_target(&at, &n);
            out["next"] = J::String(n);
        }
        if let Some(r) = get(f, "retry").and_then(Y::as_u64) {
            let backoff = text(f, "backoff");
            let mut spec = Y::Mapping(Default::default());
            if let Y::Mapping(m) = &mut spec {
                m.insert(Y::String("max_attempts".into()), Y::Number(r.into()));
                if let Some(b) = backoff {
                    m.insert(Y::String("backoff".into()), Y::String(b));
                }
            }
            out["retry"] = self.retry(&at, &spec);
        }
        if let Some(o) = get(f, "on_exhausted") {
            out["on_exhausted"] = self.outcome(&format!("{at}, on_exhausted"), o, Place::Plain);
        }
        out
    }

    fn id_of(&mut self, at: &str, s: &Y, params: &mut Vec<(String, J)>) -> Option<J> {
        if let Some(i) = get(s, "id").and_then(Y::as_str) {
            return Some(self.template(&format!("{at}, id"), i, Place::Plain));
        }
        // `params.id` also names the record; it stays a parameter and the engine reads it from there.
        let _ = params;
        None
    }

    fn action_step(&mut self, name: &str, at: &str, s: &Y) -> Option<J> {
        self.unsupported_keys(
            at,
            s,
            &[
                "name",
                "type",
                "action",
                "entity",
                "id",
                "params",
                "filter",
                "limit",
                "order",
                "on_success",
                "on_failure",
                "on_timeout",
                "timeout",
                "condition",
                "idempotent",
            ],
        );
        let Some(verb) = text(s, "action") else {
            self.problem(at, "an action step names no `action`");
            return None;
        };
        let entity = text(s, "entity");
        let mut params = self.params(at, get(s, "params"), Place::Plain);
        let id = self.id_of(at, s, &mut params);
        let has_id = id.is_some() || params.iter().any(|(k, _)| k == "id");
        // `fetch` carries its filter beside the parameters.
        let has_filter = get(s, "filter").is_some();
        if let Some(f) = get(s, "filter") {
            for (k, v) in entries(f) {
                let inner = self.tpl(at, v, Place::Plain);
                params.push((k, inner));
            }
        }
        if let Some(l) = get(s, "limit").and_then(Y::as_u64) {
            params.push(("_limit".into(), json!({ "lit": l })));
        }
        if let Some(o) = text(s, "order") {
            params.push(("_order".into(), json!({ "lit": o })));
        }

        let on_success = match get(s, "on_success") {
            Some(o) => self.outcome(&format!("{at}, on_success"), o, Place::Plain),
            None => {
                self.problem(at, "an action step needs `on_success` with a `next`");
                json!({ "set": [] })
            }
        };
        let on_timeout = get(s, "on_timeout")
            .map(|o| self.outcome(&format!("{at}, on_timeout"), o, Place::Plain));

        if ENGINE_VERBS.contains(&verb.as_str()) {
            if verb == "emit_event" {
                match params
                    .iter()
                    .find(|(k, _)| k == "event")
                    .and_then(|(_, v)| v.get("lit"))
                    .and_then(J::as_str)
                {
                    Some(ev) => self.emits.push(ev.to_string()),
                    None => self.problem(
                        at,
                        "`emit_event` needs an `event` parameter written as a plain name",
                    ),
                }
            }
        } else {
            self.dispatch(name, at, &verb, entity.as_deref(), has_id, has_filter);
        }

        let mut out = json!({
            "kind": "action",
            "action": verb,
            "params": Self::pairs(params),
            "on_success": on_success,
        });
        if let Some(e) = entity {
            out["entity"] = J::String(e);
        }
        if let Some(i) = id {
            out["id"] = i;
        }
        if let Some(t) = on_timeout {
            out["on_timeout"] = t;
        }
        Some(out)
    }

    /// Decide how a non-engine action is performed, and record the arm.
    fn dispatch(
        &mut self,
        step: &str,
        at: &str,
        verb: &str,
        entity: Option<&str>,
        has_id: bool,
        has_filter: bool,
    ) {
        let entity_verbs = ["create", "update", "delete", "query", "find_by_id", "fetch"];
        if entity_verbs.contains(&verb) {
            let Some(entity) = entity else {
                self.problem(at, format!("`{verb}` names no `entity`"));
                return;
            };
            if self.model(entity).is_none() {
                self.problem(
                    at,
                    format!("`{verb}` names the entity `{entity}`, which this module does not have: write the step as a bespoke verb that calls the module that owns it"),
                );
                return;
            }
            self.entities.insert(entity.to_string());
            let (step, entity) = (step.to_string(), entity.to_string());
            match verb {
                "create" => self.arms.push(Arm::Create { step, entity }),
                "update" | "delete" if !has_id => self.problem(
                    at,
                    format!("`{verb}` names no record: give the step an `id:` (the parameters would otherwise have to say which keys select the row and which are values)"),
                ),
                "update" => self.arms.push(Arm::Update { step, entity }),
                "delete" => self.arms.push(Arm::Delete { step, entity }),
                "query" => self.arms.push(Arm::Query { step, entity, many: false }),
                _ if has_id => self.arms.push(Arm::FindById { step, entity }),
                "fetch" if has_filter => self.arms.push(Arm::Query { step, entity, many: true }),
                _ => self.problem(at, format!("`{verb}` names no record: give it an `id:` or, for `fetch`, a `filter:`")),
            }
            return;
        }
        // A bespoke verb: one required method of the Actions trait.
        let method = crate::utils::escape_rust_keyword(&to_snake_case(verb));
        match self.hand.iter_mut().find(|h| h.verb == verb) {
            Some(h) => h.steps.push(step.to_string()),
            None => self.hand.push(HandVerb {
                method: method.clone(),
                verb: verb.to_string(),
                steps: vec![step.to_string()],
            }),
        }
        self.arms.push(Arm::Hand {
            step: step.to_string(),
            method,
        });
    }

    fn transition_step(&mut self, name: &str, at: &str, s: &Y) -> Option<J> {
        self.unsupported_keys(
            at,
            s,
            &[
                "name",
                "type",
                "entity",
                "id",
                "transition",
                "params",
                "on_success",
                "on_failure",
                "condition",
                "timeout",
            ],
        );
        if get(s, "filter").is_some() {
            self.problem(at, "a transition selected by `filter` is not run by the engine: name the record with `id:`");
        }
        let entity = text(s, "entity");
        let verb = text(s, "transition");
        let (Some(entity), Some(verb)) = (entity, verb) else {
            self.problem(at, "a transition step names an `entity` and a `transition`");
            return None;
        };
        let id = get(s, "id")
            .and_then(Y::as_str)
            .map(|i| self.template(&format!("{at}, id"), i, Place::Plain));
        if id.is_none() {
            self.problem(at, "a transition step names its record with `id:`");
        }
        let params = self.params(at, get(s, "params"), Place::Plain);
        let on_success = match get(s, "on_success") {
            Some(o) => self.outcome(&format!("{at}, on_success"), o, Place::Plain),
            None => {
                self.problem(at, "a transition step needs `on_success` with a `next`");
                json!({ "set": [] })
            }
        };
        self.check_transition(at, &entity, &verb);
        self.entities.insert(entity.clone());
        self.arms.push(Arm::Transition {
            step: name.to_string(),
            entity: entity.clone(),
            verb: verb.clone(),
        });
        Some(json!({
            "kind": "transition",
            "entity": entity,
            "id": id.unwrap_or(J::Null),
            "verb": verb,
            "params": Self::pairs(params),
            "on_success": on_success,
        }))
    }

    /// The entity must have a machine with that verb, on an enum state field.
    fn check_transition(&mut self, at: &str, entity: &str, verb: &str) {
        let Some(model) = self.model(entity) else {
            self.problem(
                at,
                format!("the entity `{entity}` is not a model of this module"),
            );
            return;
        };
        let hook = self
            .schema
            .hooks
            .iter()
            .find(|h| h.model_ref == entity && h.state_machine.is_some());
        let Some(sm) = hook.and_then(|h| h.state_machine.as_ref()) else {
            self.problem(
                at,
                format!("`{entity}` has no state machine, so `{verb}` is not a transition"),
            );
            return;
        };
        if !sm.transitions.iter().any(|t| t.name == verb) {
            let known: Vec<&str> = sm.transitions.iter().map(|t| t.name.as_str()).collect();
            self.problem(
                at,
                format!(
                    "`{entity}`'s machine has no transition `{verb}` (it has {})",
                    known.join(", ")
                ),
            );
        }
        if !crate::rules::compile::state_field_is_enum(model, &sm.field) {
            self.problem(at, format!("the state field `{}` of `{entity}` is not an enum, so no transition route is generated for it", sm.field));
        }
    }

    fn condition_step(&mut self, at: &str, s: &Y) -> Option<J> {
        self.unsupported_keys(at, s, &["name", "type", "conditions", "condition"]);
        let Some(Y::Sequence(branches)) = get(s, "conditions") else {
            self.problem(at, "a condition step lists `conditions`");
            return None;
        };
        let mut out = Vec::new();
        let mut has_else = false;
        for b in branches {
            self.unsupported_keys(at, b, &["if", "else", "next", "set"]);
            let Some(next) = text(b, "next") else {
                self.problem(at, "a branch of a condition names a `next`");
                continue;
            };
            self.next_target(at, &next);
            let when = match (get(b, "if").and_then(Y::as_str), get(b, "else")) {
                (Some(c), _) => Some(self.expr(&format!("{at}, if"), c, Place::Plain)),
                (None, Some(_)) => {
                    has_else = true;
                    None
                }
                (None, None) => {
                    self.problem(at, "a branch has neither `if` nor `else`");
                    continue;
                }
            };
            let mut set = Vec::new();
            if let Some(m) = get(b, "set") {
                for (k, v) in entries(m) {
                    let inner = self.tpl(at, v, Place::Plain);
                    set.push((k, inner));
                }
            }
            let mut branch = json!({ "set": Self::pairs(set), "next": next });
            if let Some(w) = when {
                branch["when"] = w;
            }
            out.push(branch);
        }
        if !has_else {
            self.warnings.push(format!("workflow `{}`, {at}: no `else` branch, so an instance where no condition holds fails", self.wf.name));
        }
        Some(json!({ "kind": "condition", "branches": out }))
    }

    fn wait_step(&mut self, at: &str, s: &Y) -> Option<J> {
        self.unsupported_keys(
            at,
            s,
            &["name", "type", "wait_for", "on_event", "on_timeout"],
        );
        let Some(w) = get(s, "wait_for") else {
            self.problem(at, "a wait step has `wait_for`");
            return None;
        };
        self.unsupported_keys(
            &format!("{at}, wait_for"),
            w,
            &["event", "condition", "timeout"],
        );
        let event = text(w, "event");
        let timeout =
            text(w, "timeout").map(|t| self.duration(&format!("{at}, wait_for.timeout"), &t));
        let mut key = Vec::new();
        if let Some(ev) = &event {
            self.awaits.push(ev.clone());
            if let Some(cond) = text(w, "condition") {
                key = self.wait_key(at, &cond);
            }
        } else if get(w, "condition").is_some() {
            self.problem(at, "a wait with no `event` cannot have a `condition`");
        }
        if event.is_none() && timeout.is_none() {
            self.problem(at, "a wait names an `event`, a `timeout`, or both");
        }
        let on_event =
            get(s, "on_event").map(|o| self.outcome(&format!("{at}, on_event"), o, Place::Event));
        let on_timeout = get(s, "on_timeout")
            .map(|o| self.outcome(&format!("{at}, on_timeout"), o, Place::Plain));
        if event.is_some() && on_event.is_none() {
            self.problem(at, "a wait for an event names `on_event` with a `next`");
        }
        if timeout.is_some() && on_timeout.is_none() {
            self.problem(at, "a wait with a timeout names `on_timeout` with a `next`");
        }
        let mut out = json!({ "kind": "wait", "key": key });
        if let Some(e) = event {
            out["event"] = J::String(e);
        }
        if let Some(t) = timeout.filter(|t| !t.is_null()) {
            out["timeout"] = t;
        }
        if let Some(o) = on_event {
            out["on_event"] = o;
        }
        if let Some(o) = on_timeout {
            out["on_timeout"] = o;
        }
        Some(out)
    }

    /// `event.a == context.b && event.c == context.d` as the equalities that key the wait. Anything
    /// else is an error: an instance is found by key, not by evaluating a condition against every
    /// waiting instance.
    fn wait_key(&mut self, at: &str, cond: &str) -> Vec<J> {
        let at = format!("{at}, wait_for.condition");
        let ast = match parse_dynamic(cond) {
            Ok(a) => a,
            Err(e) => {
                self.problem(
                    &at,
                    format!("`{cond}` is not an expression the engine reads: {e}"),
                );
                return Vec::new();
            }
        };
        let mut conjuncts = Vec::new();
        split_and(&ast, &mut conjuncts);
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for c in conjuncts {
            let Expression::Binary {
                left,
                op: BinaryOp::Eq,
                right,
            } = c
            else {
                self.problem(&at, format!("`{cond}` is not a wait key: every part must be an equality `event.<field> == <context expression>` joined by `&&`"));
                return Vec::new();
            };
            let (event_side, other) = match (event_field(left), event_field(right)) {
                (Some(f), None) => (f, right.as_ref()),
                (None, Some(f)) => (f, left.as_ref()),
                _ => {
                    self.problem(
                        &at,
                        format!(
                            "`{cond}`: each equality needs `event.<field>` on exactly one side"
                        ),
                    );
                    return Vec::new();
                }
            };
            if !seen.insert(event_side.clone()) {
                self.problem(&at, format!("`event.{event_side}` is compared twice"));
                return Vec::new();
            }
            match self.lower(other, Place::Plain) {
                Ok(context) => out.push(json!({ "event_field": event_side, "context": context })),
                Err(e) => {
                    self.problem(&at, format!("`{cond}`: {e}"));
                    return Vec::new();
                }
            }
        }
        out.sort_by(|a, b| a["event_field"].as_str().cmp(&b["event_field"].as_str()));
        out
    }

    fn terminal_step(&mut self, at: &str, s: &Y) -> Option<J> {
        self.unsupported_keys(
            at,
            s,
            &[
                "name",
                "type",
                "status",
                "reason",
                "result",
                "emit",
                "compensate",
            ],
        );
        let status = text(s, "status").unwrap_or_else(|| "completed".into());
        if !["completed", "failed", "expired", "cancelled"].contains(&status.as_str()) {
            self.problem(at, format!("the terminal status `{status}` is not one the engine has (completed, failed, expired, cancelled)"));
        }
        let reason = get(s, "reason").map(|r| self.tpl(at, r, Place::Plain));
        let result = get(s, "result").map(|r| self.tpl(at, r, Place::Plain));
        let emit = get(s, "emit").map(|e| {
            self.unsupported_keys(&format!("{at}, emit"), e, &["event", "data"]);
            let event = text(e, "event").unwrap_or_default();
            if event.is_empty() {
                self.problem(at, "`emit` names an `event`");
            } else {
                self.emits.push(event.clone());
            }
            let data = get(e, "data")
                .map(|d| self.tpl(at, d, Place::Plain))
                .unwrap_or_else(|| json!({ "map": [] }));
            json!({ "event": event, "data": data })
        });
        let compensate = get(s, "compensate").and_then(Y::as_bool).unwrap_or(false);
        if compensate
            && self
                .wf
                .source
                .as_ref()
                .and_then(|v| get(v, "compensation"))
                .is_none()
        {
            self.problem(
                at,
                "`compensate: true` but the workflow has no `compensation:`",
            );
        }
        let mut out = json!({ "kind": "terminal", "status": status, "compensate": compensate });
        if let Some(r) = reason {
            out["reason"] = r;
        }
        if let Some(r) = result {
            out["result"] = r;
        }
        if let Some(e) = emit {
            out["emit"] = e;
        }
        Some(out)
    }

    fn compensation(&mut self, c: &Y) -> Option<J> {
        let name = text(c, "name")?;
        let at = format!("compensation `{name}`");
        self.unsupported_keys(
            &at,
            c,
            &[
                "name",
                "condition",
                "action",
                "entity",
                "id",
                "params",
                "transition",
            ],
        );
        let verb = text(c, "action");
        let transition = text(c, "transition");
        let action = match (&verb, &transition) {
            (Some(v), _) => v.clone(),
            (None, Some(_)) => "transition".to_string(),
            _ => {
                self.problem(&at, "a compensation names an `action`");
                return None;
            }
        };
        let when = get(c, "condition")
            .and_then(Y::as_str)
            .map(|e| self.expr(&format!("{at}, condition"), e, Place::Plain));
        let mut params = self.params(&at, get(c, "params"), Place::Plain);
        let id = self.id_of(&at, c, &mut params);
        let has_id = id.is_some() || params.iter().any(|(k, _)| k == "id");
        let entity = text(c, "entity");
        let label = format!("compensate:{name}");
        if let Some(t) = &transition {
            match &entity {
                Some(e) => {
                    self.check_transition(&at, e, t);
                    self.entities.insert(e.clone());
                    self.arms.push(Arm::Transition {
                        step: label.clone(),
                        entity: e.clone(),
                        verb: t.clone(),
                    });
                }
                None => self.problem(&at, "a transition compensation names an `entity`"),
            }
            if !has_id {
                self.problem(&at, "a transition compensation names its record with `id:`");
            }
        } else if ENGINE_VERBS.contains(&action.as_str()) {
            if action == "emit_event" {
                match params
                    .iter()
                    .find(|(k, _)| k == "event")
                    .and_then(|(_, v)| v.get("lit"))
                    .and_then(J::as_str)
                {
                    Some(ev) => self.emits.push(ev.to_string()),
                    None => self.problem(
                        &at,
                        "`emit_event` needs an `event` parameter written as a plain name",
                    ),
                }
            }
        } else {
            self.dispatch(&label, &at, &action, entity.as_deref(), has_id, false);
        }
        let mut out = json!({ "name": name, "action": action, "params": Self::pairs(params) });
        if let Some(w) = when.filter(|w| !w.is_null()) {
            out["when"] = w;
        }
        if let Some(e) = entity {
            out["entity"] = J::String(e);
        }
        if let Some(i) = id {
            out["id"] = i;
        }
        if let Some(t) = transition {
            out["verb"] = J::String(t);
        }
        Some(out)
    }
}

enum Seg {
    Key(String),
    Index(usize),
}

/// A path as its root name and what follows: `context.user.id`, `result[0].id`.
fn flatten(e: &Expression) -> Option<(String, Vec<Seg>)> {
    match e {
        Expression::FieldRef(FieldRef::Simple(n)) => Some((n.clone(), Vec::new())),
        Expression::FieldRef(FieldRef::Path(parts)) => {
            let (first, rest) = parts.split_first()?;
            Some((
                first.clone(),
                rest.iter().map(|p| Seg::Key(p.clone())).collect(),
            ))
        }
        Expression::MemberAccess { object, member } => {
            let (root, mut segs) = flatten(object)?;
            segs.push(Seg::Key(member.clone()));
            Some((root, segs))
        }
        Expression::MethodCall {
            object,
            method,
            args,
        } if method == "[]" => {
            let (root, mut segs) = flatten(object)?;
            match args.first() {
                Some(Expression::Literal(Literal::Int(i))) if *i >= 0 => {
                    segs.push(Seg::Index(*i as usize))
                }
                Some(Expression::Literal(Literal::String(k))) => segs.push(Seg::Key(k.clone())),
                _ => return None,
            }
            Some((root, segs))
        }
        _ => None,
    }
}

/// The `$name` a member chain starts from, when it does.
fn context_root(e: &Expression) -> Option<String> {
    match e {
        Expression::Context(c) => Some(c.clone()),
        Expression::MemberAccess { object, .. } | Expression::MethodCall { object, .. } => {
            context_root(object)
        }
        _ => None,
    }
}

fn split_and<'e>(e: &'e Expression, out: &mut Vec<&'e Expression>) {
    match e {
        Expression::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            split_and(left, out);
            split_and(right, out);
        }
        other => out.push(other),
    }
}

/// `event.<field>` with one key and nothing else.
fn event_field(e: &Expression) -> Option<String> {
    match flatten(e) {
        Some((root, segs)) if root == "event" => match segs.as_slice() {
            [Seg::Key(k)] => Some(k.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn collect_set_keys(v: &Y, out: &mut HashSet<String>) {
    match v {
        Y::Mapping(m) => {
            for (k, v) in m {
                if k.as_str() == Some("set") {
                    out.extend(keys(v));
                }
                collect_set_keys(v, out);
            }
        }
        Y::Sequence(items) => items.iter().for_each(|i| collect_set_keys(i, out)),
        _ => {}
    }
}

/// The steps a state-machine verb moves a record to: `(field, state label, nullable)`.
pub fn transition_target(
    schema: &ModuleSchema,
    entity: &str,
    verb: &str,
) -> Option<(String, Option<String>, String)> {
    let hook = schema
        .hooks
        .iter()
        .find(|h| h.model_ref == entity && h.state_machine.is_some())?;
    let sm = hook.state_machine.as_ref()?;
    let t = sm.transitions.iter().find(|t| t.name == verb)?;
    let to = if t.to == crate::ast::NULL_STATE {
        None
    } else {
        Some(enum_label(&t.to))
    };
    Some((sm.field.clone(), to, hook.type_prefix()))
}

/// The service type of `entity` as the module builds it (a model named like its own service is
/// `AppService`).
pub fn service_type(schema: &ModuleSchema, entity: &str) -> String {
    let service = format!("{entity}Service");
    if schema.models.iter().any(|m| m.name == service) {
        format!("{entity}AppService")
    } else {
        service
    }
}

/// Everything the module's own `ModuleSchema` needs to say about an engine workflow, per name:
/// compile each once.
pub fn compile_all(schema: &ModuleSchema) -> BTreeMap<String, Compiled> {
    engine_workflows(schema)
        .map(|w| (w.name.clone(), compile(schema, w)))
        .collect()
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::ast::{
        hook::{StateMachine, Transition},
        Field, Hook, PrimitiveType, TypeRef,
    };
    use crate::parser::parse_yaml_workflow;
    use crate::rules::events::read_events;

    pub(crate) fn model(name: &str, fields: &[&str]) -> Model {
        let mut m = Model::new(name);
        m.schema = Some("hr".into());
        for f in fields {
            m.fields
                .push(Field::new(*f, TypeRef::Primitive(PrimitiveType::String)));
        }
        m
    }

    /// A module with a `Ticket` and an `Account` model, a machine on `Ticket.status`, and one
    /// declared event; `yaml` is the workflow under test.
    pub(crate) fn schema_with(yaml: &str) -> ModuleSchema {
        let mut s = ModuleSchema::new("support");
        let mut ticket = model("Ticket", &["title"]);
        ticket
            .fields
            .push(Field::new("status", TypeRef::Custom("TicketStatus".into())));
        s.models.push(ticket);
        s.models.push(model("Account", &["name"]));
        let mut hook = Hook::new("ticket", "Ticket");
        hook.state_machine = Some(StateMachine {
            field: "status".into(),
            transitions: vec![Transition {
                name: "close".into(),
                from: vec!["open".into()],
                to: "closed".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        s.hooks.push(hook);
        let declared: serde_yaml::Mapping = serde_yaml::from_str(
            "events:\n  TicketClosed:\n    aggregate: Ticket\n    description: closed",
        )
        .unwrap();
        s.rule_register.events.extend(read_events(
            "ticket.hook.yaml",
            &declared,
            Some("Ticket"),
            &[],
        ));
        let file = parse_yaml_workflow(yaml).expect("the fixture parses");
        s.workflows.extend(file.workflows);
        s
    }

    pub(crate) fn compiled(yaml: &str) -> Compiled {
        let s = schema_with(yaml);
        compile(&s, &s.workflows[0])
    }

    pub(crate) const BASE: &str = r#"
name: Escalation
version: 1
trigger:
  event: TicketRaised
  extract:
    ticket_id: "event.ticket_id"
config:
  executor: engine
  timeout: 7d
context:
  attempts: 0
steps:
  - name: note
    type: action
    action: create
    entity: Account
    params:
      name: "ticket {{ context.ticket_id }}"
    on_success:
      set: { account: "{{ result }}" }
      next: wait_ack
  - name: wait_ack
    type: wait
    wait_for:
      event: TicketAcked
      condition: "event.user_id == context.owner && event.ticket_id == context.ticket_id"
      timeout: 15m
    on_event: { next: close }
    on_timeout: { next: page }
  - name: page
    type: action
    action: notify_on_call
    params: { severity: high }
    on_success: { next: close }
  - name: close
    type: transition
    entity: Ticket
    id: "{{ context.ticket_id }}"
    transition: close
    on_success: { next: done }
  - name: done
    type: terminal
    status: completed
    emit:
      event: TicketClosed
      data: { ticket: "{{ context.ticket_id }}" }
"#;
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    #[test]
    fn a_workflow_in_the_closed_set_compiles_to_a_spec_with_its_dispatch() {
        let c = compiled(BASE);
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        let spec = &c.spec;
        assert_eq!(spec["name"], "Escalation");
        assert_eq!(spec["trigger_event"], "TicketRaised");
        assert_eq!(spec["persistence"], true);
        assert_eq!(spec["timeout"], serde_json::json!({ "secs": 7 * 86_400 }));
        assert_eq!(spec["extract"][0][0], "ticket_id");
        let wait = &spec["steps"][1];
        assert_eq!(wait["kind"], "wait");
        assert_eq!(wait["event"], "TicketAcked");
        assert_eq!(wait["timeout"], serde_json::json!({ "secs": 900 }));
        // The two equalities become the key, sorted by event field.
        let fields: Vec<&str> = wait["key"]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| k["event_field"].as_str().unwrap())
            .collect();
        assert_eq!(fields, vec!["ticket_id", "user_id"]);
        assert_eq!(wait["key"][1]["context"]["path"]["root"], "context");
        // A template with one hole keeps the expression; text around holes is text.
        let params = &spec["steps"][0]["params"][0];
        assert_eq!(params[0], "name");
        assert!(params[1]["template"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("ticket "));
        assert_eq!(c.arms.len(), 3, "{:?}", c.arms);
        assert!(c.arms.contains(&Arm::Create {
            step: "note".into(),
            entity: "Account".into()
        }));
        assert!(c.arms.contains(&Arm::Transition {
            step: "close".into(),
            entity: "Ticket".into(),
            verb: "close".into()
        }));
        assert_eq!(
            c.hand,
            vec![HandVerb {
                method: "notify_on_call".into(),
                verb: "notify_on_call".into(),
                steps: vec!["page".into()]
            }]
        );
        assert_eq!(c.emits, vec!["TicketClosed".to_string()]);
        assert_eq!(c.required_methods(), 1);
        assert!(c.entities.contains("Ticket") && c.entities.contains("Account"));
    }

    #[test]
    fn a_step_timeout_bounds_its_hand_call_and_a_waits_timeout_stays_part_of_the_wait() {
        let c = compiled(&BASE.replace("params: { severity: high }", "params: { severity: high }\n    timeout: 30s"));
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        assert_eq!(c.spec["steps"][2]["call_timeout"], 30);
        assert!(c.spec["steps"][2].get("timeout").is_none(), "the step's own `timeout` key is never the hand call's");
        assert_eq!(c.spec["steps"][1]["timeout"], serde_json::json!({ "secs": 900 }), "the wait keeps its own");
        assert!(c.spec["steps"][1].get("call_timeout").is_none());
    }

    #[test]
    fn a_function_not_listed_under_functions_is_refused_and_a_listed_one_becomes_a_method() {
        let with_call = BASE.replace(
            "params: { severity: high }",
            "params: { to: \"{{ get_user_email(context.ticket_id) }}\" }",
        );
        let c = compiled(&with_call);
        assert!(
            c.problems
                .iter()
                .any(|p| p.contains("get_user_email") && p.contains("`functions:`")),
            "{:?}",
            c.problems
        );
        let declared = format!("{with_call}\nfunctions:\n  - get_user_email\n");
        let c = compiled(&declared);
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        assert_eq!(c.functions, vec!["get_user_email".to_string()]);
        assert_eq!(function_method("get_user_email"), "fn_get_user_email");
        assert_eq!(c.required_methods(), 2, "one verb and one function");
    }

    #[test]
    fn a_wait_condition_that_is_not_an_equality_key_is_refused() {
        for (cond, says) in [
            ("event.count > 3", "equality"),
            ("event.a == 1 || event.b == 2", "equality"),
            ("context.a == context.b", "exactly one side"),
            ("event.a.b == context.x", "exactly one side"),
        ] {
            let yaml = BASE.replace(
                "event.user_id == context.owner && event.ticket_id == context.ticket_id",
                cond,
            );
            let c = compiled(&yaml);
            assert!(
                c.problems
                    .iter()
                    .any(|p| p.contains("wait_for.condition") && p.contains(says)),
                "{cond}: {:?}",
                c.problems
            );
        }
        // The reversed spelling keys the same way.
        let c = compiled(&BASE.replace(
            "event.user_id == context.owner && event.ticket_id == context.ticket_id",
            "context.ticket_id == event.ticket_id",
        ));
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        assert_eq!(c.spec["steps"][1]["key"][0]["event_field"], "ticket_id");
    }

    #[test]
    fn a_bare_name_reads_the_context_key_a_workflow_writes_and_nothing_else() {
        let yaml = BASE.replace(
            "params: { severity: high }",
            "params: { who: \"{{ account }}\", what: \"{{ nobody }}\" }",
        );
        let c = compiled(&yaml);
        let bad: Vec<&String> = c.problems.iter().filter(|p| p.contains("nobody")).collect();
        assert_eq!(bad.len(), 1, "{:?}", c.problems);
        assert!(
            bad[0].contains("not a name the workflow writes"),
            "{}",
            bad[0]
        );
        assert!(
            !c.problems.iter().any(|p| p.contains("`account`")),
            "a name a `set:` writes resolves: {:?}",
            c.problems
        );
    }

    #[test]
    fn what_the_engine_cannot_honour_is_a_named_problem() {
        let cases: Vec<(String, &str)> = vec![
            // update with no record
            (
                BASE.replace(
                    "action: create\n    entity: Account",
                    "action: update\n    entity: Account",
                ),
                "names no record",
            ),
            // an entity of another module
            (
                BASE.replace("entity: Account", "entity: Stranger"),
                "does not have",
            ),
            // a terminal status the engine lacks
            (
                BASE.replace("status: completed", "status: archived"),
                "terminal status `archived`",
            ),
            // an event nobody declares
            (
                BASE.replace("event: TicketClosed", "event: TicketBurned"),
                "TicketBurned",
            ),
            // a verb the machine does not have
            (
                BASE.replace("transition: close", "transition: explode"),
                "no transition `explode`",
            ),
            // a step type the engine does not run
            (
                BASE.replace("type: terminal", "type: parallel"),
                "`parallel` step",
            ),
            // an unread key
            (
                BASE.replace("action: create\n", "action: create\n    rules: [a]\n"),
                "`rules` is not read",
            ),
            // a dangling next
            (
                BASE.replace(
                    "next: close }\n    on_timeout",
                    "next: nowhere }\n    on_timeout",
                ),
                "names no step",
            ),
        ];
        for (yaml, says) in cases {
            let c = compiled(&yaml);
            assert!(
                c.problems.iter().any(|p| p.contains(says)),
                "wanted `{says}` in {:?}",
                c.problems
            );
        }
    }

    #[test]
    fn expressions_outside_the_set_are_refused_with_the_reason() {
        for (src, says) in [
            ("a = b", "write `==`"),
            ("context.x matches 'a'", "workflow expression set"),
            (
                "context.a.upper()",
                "not part of the workflow expression set",
            ),
            ("$actor.id == 1", "not available"),
            ("event.a == 1", "only readable in"),
        ] {
            let yaml = BASE.replace(
                "params: { severity: high }",
                &format!("params: {{ v: \"{{{{ {src} }}}}\" }}"),
            );
            let c = compiled(&yaml);
            assert!(
                c.problems.iter().any(|p| p.contains(says)),
                "{src}: wanted `{says}` in {:?}",
                c.problems
            );
        }
    }

    #[test]
    fn a_workflow_without_persistence_cannot_wait_and_one_with_a_deadline_template_compiles() {
        let c =
            compiled(&BASE.replace("executor: engine", "executor: engine\n  persistence: false"));
        assert!(
            c.problems.iter().any(|p| p.contains("cannot wait")),
            "{:?}",
            c.problems
        );
        let c = compiled(&BASE.replace("timeout: 15m", "timeout: \"{{ context.minutes }}m\""));
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        assert!(c.spec["steps"][1]["timeout"]["tpl"].is_array());
        let c = compiled(&BASE.replace("timeout: 15m", "timeout: soon"));
        assert!(
            c.problems.iter().any(|p| p.contains("not a duration")),
            "{:?}",
            c.problems
        );
    }

    #[test]
    fn compensation_conditions_and_transitions_compile() {
        let yaml = format!(
            "{BASE}\ncompensation:\n  - name: undo_account\n    condition: \"steps.note.completed == true\"\n    action: delete\n    entity: Account\n    id: \"{{{{ context.account.id }}}}\"\n  - name: reopen\n    action: transition\n    entity: Ticket\n    transition: close\n    id: \"{{{{ context.ticket_id }}}}\"\n"
        );
        let c = compiled(&yaml);
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        assert_eq!(c.spec["compensation"][0]["name"], "undo_account");
        assert_eq!(
            c.spec["compensation"][0]["when"]["bin"]["l"]["path"]["root"],
            "steps"
        );
        assert!(c.arms.contains(&Arm::Delete {
            step: "compensate:undo_account".into(),
            entity: "Account".into()
        }));
        // A compensation that deletes by `user_id` alone names no record.
        let by_filter = format!("{BASE}\ncompensation:\n  - name: undo\n    action: delete\n    entity: Account\n    params: {{ user_id: x }}\n");
        assert!(compiled(&by_filter)
            .problems
            .iter()
            .any(|p| p.contains("names no record")));
    }

    #[test]
    fn the_validator_reports_executors_implemented_by_and_the_missing_extension_layout() {
        let s = schema_with(BASE);
        let p = crate::resolver::validator::workflow_problems(&s);
        assert!(
            p.iter().any(|m| m.contains("extension layout")),
            "a bespoke verb needs the layout: {p:?}"
        );
        let mut s = schema_with(BASE);
        s.generators_config = Some(crate::parser::yaml_parser::GeneratorsConfig {
            extensions: Some(true),
            ..Default::default()
        });
        assert!(crate::resolver::validator::workflow_problems(&s).is_empty());
        let mut s = schema_with(&BASE.replace("executor: engine", "executor: robot"));
        let p = crate::resolver::validator::workflow_problems(&s);
        assert!(
            p.iter().any(|m| m.contains("neither `engine` nor `hand`")),
            "{p:?}"
        );
        s = schema_with(&BASE.replace("executor: engine", "executor: hand\n  persistence: true"));
        s.workflows[0].implemented_by = Some("not a path".into());
        let p = crate::resolver::validator::workflow_problems(&s);
        assert!(p.iter().any(|m| m.contains("<file>::<function>")), "{p:?}");
        s.workflows[0].implemented_by = Some("src/a.rs::run".into());
        assert!(crate::resolver::validator::workflow_problems(&s).is_empty());
        s.workflows[0].executor = Some("engine".into());
        assert!(crate::resolver::validator::workflow_problems(&s)
            .iter()
            .any(|m| m.contains("say one")));
    }

    #[test]
    fn a_workflow_documented_or_implemented_by_hand_is_not_an_engine_workflow() {
        let s = schema_with(&BASE.replace("executor: engine", "executor: hand"));
        assert!(!has_engine_workflows(&s));
        assert!(crate::generators::workflows::mod_file(&s).is_none());
        let warnings = crate::resolver::validator::workflow_warnings(&s);
        assert!(
            warnings.iter().any(|w| w.contains("documentation")),
            "{warnings:?}"
        );
        let mut s = s;
        s.workflows[0].implemented_by = Some("src/a.rs::run".into());
        assert!(
            crate::resolver::validator::workflow_warnings(&s).is_empty(),
            "a documented saga names its hand function"
        );
    }
}
