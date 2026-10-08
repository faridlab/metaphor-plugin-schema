//! The rule catalog: named rules a hook file writes under `check:`, each with
//! the parameters it takes and the layer that enforces it.
//!
//! ```yaml
//! rules:
//!   - id: R7
//!     name: approval_recorded
//!     check: { required_if: { field: approved_at, when: "status == 'approved'" } }
//!     error_code: approval_missing
//! ```
//!
//! Names follow Laravel's validation vocabulary where it has the rule
//! (`required_if`, `prohibited_if`), and the business rules it lacks are named
//! for what they hold (`children_sum_equals`, `no_overlap`, `locked_when`).
//! Parameters are named, never positional, so each is checked on its own.

use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::ast::Model;

use super::expr::{self, ExpressionCheck};

/// Which layer enforces a rule, decided by what it has to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    /// One row alone: a CHECK constraint, a sync check and a Zod refine.
    Row,
    /// Across rows of one table: a unique index or exclusion constraint.
    Set,
    /// Other rows or entities: a deferred constraint trigger and an async check.
    Graph,
    /// The row before and after the write: a BEFORE UPDATE trigger and a write guard.
    Transition,
    /// The person or the time of the write: a write guard only.
    Context,
    /// Hand-written code the rule names.
    Custom,
}

/// What one parameter holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Param {
    /// A field of the model.
    Field,
    /// Several fields of the model.
    Fields,
    /// A relation of the model (`lines`).
    Relation,
    /// An integer.
    Int,
    /// A number.
    Number,
    /// A string.
    Text,
    /// Several strings (enum values, states).
    Texts,
    /// An expression in the rule grammar.
    Expr,
    /// A comparison operator: `==`, `!=`, `<`, `<=`, `>`, `>=`.
    Op,
}

struct Spec {
    name: &'static str,
    layer: Layer,
    /// (parameter, kind, required)
    params: &'static [(&'static str, Param, bool)],
}

use Param::*;

/// The core catalog. Each entry is what its name says; the emitters that
/// compile them land with the enforcement phase.
const CATALOG: &[Spec] = &[
    // Presence and shape — one field of one row.
    Spec { name: "required", layer: Layer::Row, params: &[("field", Field, true)] },
    Spec { name: "not_blank", layer: Layer::Row, params: &[("field", Field, true)] },
    Spec { name: "email", layer: Layer::Row, params: &[("field", Field, true)] },
    Spec { name: "enum", layer: Layer::Row, params: &[("field", Field, true), ("values", Texts, true)] },
    Spec { name: "pattern", layer: Layer::Row, params: &[("field", Field, true), ("regex", Text, true)] },
    Spec { name: "min_length", layer: Layer::Row, params: &[("field", Field, true), ("min", Int, true)] },
    Spec { name: "max_length", layer: Layer::Row, params: &[("field", Field, true), ("max", Int, true)] },
    Spec { name: "length", layer: Layer::Row, params: &[("field", Field, true), ("equals", Int, true)] },
    Spec { name: "min", layer: Layer::Row, params: &[("field", Field, true), ("min", Number, true)] },
    Spec { name: "max", layer: Layer::Row, params: &[("field", Field, true), ("max", Number, true)] },
    Spec { name: "positive", layer: Layer::Row, params: &[("field", Field, true)] },
    Spec { name: "non_negative", layer: Layer::Row, params: &[("field", Field, true)] },
    // Several fields of one row.
    Spec { name: "required_if", layer: Layer::Row, params: &[("field", Field, true), ("when", Expr, true)] },
    Spec { name: "prohibited_if", layer: Layer::Row, params: &[("field", Field, true), ("when", Expr, true)] },
    Spec { name: "exactly_one_of", layer: Layer::Row, params: &[("fields", Fields, true)] },
    Spec { name: "compare", layer: Layer::Row, params: &[("field", Field, true), ("op", Op, true), ("other", Field, true)] },
    Spec { name: "expr", layer: Layer::Row, params: &[("expr", Expr, true)] },
    // Time of the write.
    Spec { name: "not_in_future", layer: Layer::Context, params: &[("field", Field, true)] },
    // Child rows and other entities.
    Spec { name: "min_items", layer: Layer::Graph, params: &[("relation", Relation, true), ("min", Int, true)] },
    Spec { name: "distinct_by", layer: Layer::Set, params: &[("relation", Relation, true), ("fields", Texts, true)] },
    Spec {
        name: "children_sum_equals",
        layer: Layer::Graph,
        params: &[("relation", Relation, true), ("child_field", Text, true), ("field", Field, true)],
    },
    Spec { name: "exists", layer: Layer::Graph, params: &[("field", Field, true), ("entity", Text, true)] },
    Spec {
        name: "unique",
        layer: Layer::Set,
        params: &[("fields", Fields, true), ("where", Expr, false), ("case_insensitive", Text, false)],
    },
    Spec {
        name: "no_overlap",
        layer: Layer::Set,
        params: &[("key", Fields, true), ("from", Field, true), ("to", Field, true)],
    },
    // The row before and after a write.
    Spec { name: "immutable", layer: Layer::Transition, params: &[("fields", Fields, true)] },
    Spec { name: "locked_when", layer: Layer::Transition, params: &[("states", Texts, true), ("except", Fields, false)] },
    Spec { name: "required_in_state", layer: Layer::Row, params: &[("field", Field, true), ("states", Texts, true)] },
    // The person writing.
    Spec {
        name: "segregation_of_duties",
        layer: Layer::Context,
        params: &[("field", Field, true), ("not", Field, false), ("not_actor", Text, false)],
    },
    Spec { name: "tenant_consistent", layer: Layer::Graph, params: &[("fields", Fields, true)] },
    // Hand-written code.
    Spec { name: "custom", layer: Layer::Custom, params: &[("handler", Text, true)] },
];

/// A `check:` as the register keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogCheck {
    pub rule: String,
    /// The parameters as written, rendered as YAML scalars or lists.
    pub params: Vec<(String, String)>,
    /// `None` when the rule is not in the catalog.
    pub layer: Option<Layer>,
    /// Set when the `check:` itself is not shaped as one rule with named
    /// parameters; nothing else about it is checked then.
    pub malformed: Option<String>,
    /// What is wrong with it; empty when it is complete and every name resolves.
    pub problems: Vec<String>,
}

impl CatalogCheck {
    pub fn is_sound(&self) -> bool {
        self.layer.is_some() && self.problems.is_empty()
    }
}

/// Read a `check:` value: a map of exactly one catalog rule to its parameters.
pub fn read(value: &Value) -> CatalogCheck {
    let Some(map) = value.as_mapping() else {
        return unsound("check", "`check:` must map one catalog rule to its parameters");
    };
    if map.len() != 1 {
        return unsound("check", "`check:` names exactly one catalog rule; write one rule per `check:`");
    }
    let (rule, params) = map.iter().next().expect("one entry");
    let Some(rule) = rule.as_str() else {
        return unsound("check", "a catalog rule is named by a string");
    };
    let params: Vec<(String, String)> = match params {
        Value::Mapping(p) => p
            .iter()
            .filter_map(|(k, v)| Some((k.as_str()?.to_string(), render(v))))
            .collect(),
        Value::Null => Vec::new(),
        _ => return unsound(rule, "parameters are named: write `{ field: x, … }`"),
    };
    CatalogCheck { rule: rule.to_string(), params, layer: None, malformed: None, problems: Vec::new() }
}

fn unsound(rule: &str, problem: &str) -> CatalogCheck {
    CatalogCheck {
        rule: rule.to_string(),
        params: Vec::new(),
        layer: None,
        malformed: Some(problem.to_string()),
        problems: vec![problem.to_string()],
    }
}

fn render(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Sequence(items) => items.iter().map(render).collect::<Vec<_>>().join(", "),
        Value::Null => String::new(),
        other => serde_yaml::to_string(other).unwrap_or_default().trim().to_string(),
    }
}

/// Check a read `check:` against the catalog and the model it is bound to.
pub fn validate(check: &mut CatalogCheck, model: Option<&Model>) {
    if let Some(problem) = &check.malformed {
        check.layer = None;
        check.problems = vec![problem.clone()];
        return;
    }
    check.problems.clear();
    let Some(spec) = CATALOG.iter().find(|s| s.name == check.rule) else {
        check.layer = None;
        check.problems.push(format!("`{}` is not a catalog rule", check.rule));
        return;
    };
    check.layer = Some(spec.layer);
    for (name, _) in &check.params {
        if !spec.params.iter().any(|(p, _, _)| p == name) {
            check.problems.push(format!("`{}` takes no parameter `{name}`", spec.name));
        }
    }
    for (name, kind, required) in spec.params {
        let Some(value) = check.params.iter().find(|(p, _)| p == name).map(|(_, v)| v.clone()) else {
            if *required {
                check.problems.push(format!("`{}` needs `{name}`", spec.name));
            }
            continue;
        };
        if let Some(problem) = check_param(*kind, name, &value, model) {
            check.problems.push(problem);
        }
    }
}

fn check_param(kind: Param, name: &str, value: &str, model: Option<&Model>) -> Option<String> {
    let has_field = |f: &str| model.is_some_and(|m| m.fields.iter().any(|x| x.name == f));
    let has_relation = |r: &str| model.is_some_and(|m| m.relations.iter().any(|x| x.name == r));
    match kind {
        Field => (!has_field(value)).then(|| format!("`{name}`: `{value}` is not a field of the model")),
        Fields => {
            let missing: Vec<&str> = value.split(", ").filter(|f| !has_field(f)).collect();
            (!missing.is_empty()).then(|| format!("`{name}`: {} not a field of the model", missing.join(", ")))
        }
        Relation => (!has_relation(value)).then(|| format!("`{name}`: `{value}` is not a relation of the model")),
        Int => value.parse::<i64>().is_err().then(|| format!("`{name}` must be a whole number")),
        Number => value.parse::<f64>().is_err().then(|| format!("`{name}` must be a number")),
        Text | Texts => value.is_empty().then(|| format!("`{name}` is empty")),
        Op => (!["==", "!=", "<", "<=", ">", ">="].contains(&value))
            .then(|| format!("`{name}` must be one of ==, !=, <, <=, >, >=")),
        Expr => match expr::check(value, model) {
            ExpressionCheck::Ready => None,
            ExpressionCheck::Unparsed { error } => Some(format!("`{name}`: {error}")),
            ExpressionCheck::Unresolved { names, functions } => Some(format!(
                "`{name}`: unknown {}",
                names.into_iter().chain(functions).collect::<Vec<_>>().join(", ")
            )),
            ExpressionCheck::NotAnExpression => None,
        },
    }
}

/// The catalog rules, for documentation and the report.
pub fn names() -> impl Iterator<Item = (&'static str, Layer)> {
    CATALOG.iter().map(|s| (s.name, s.layer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Field, PrimitiveType, TypeRef};

    fn period() -> Model {
        let mut m = Model::new("FiscalPeriod");
        for f in ["status", "approved_at", "start_date", "end_date", "created_by", "approved_by"] {
            m.fields.push(Field::new(f, TypeRef::Primitive(PrimitiveType::String)));
        }
        m
    }

    fn checked(yaml: &str) -> CatalogCheck {
        let mut c = read(&serde_yaml::from_str(yaml).unwrap());
        validate(&mut c, Some(&period()));
        c
    }

    #[test]
    fn a_complete_check_is_sound_and_carries_its_layer() {
        let c = checked("{ required_if: { field: approved_at, when: \"status == 'approved'\" } }");
        assert!(c.is_sound(), "{:?}", c.problems);
        assert_eq!(c.layer, Some(Layer::Row));
        assert_eq!(checked("{ no_overlap: { key: [status], from: start_date, to: end_date } }").layer, Some(Layer::Set));
        assert!(checked("{ segregation_of_duties: { field: approved_by, not: created_by } }").is_sound());
    }

    #[test]
    fn each_problem_is_named() {
        let c = checked("{ required_if: { field: approved_on, whn: \"status == 'x'\" } }");
        assert_eq!(
            c.problems,
            vec![
                "`required_if` takes no parameter `whn`".to_string(),
                "`field`: `approved_on` is not a field of the model".to_string(),
                "`required_if` needs `when`".to_string(),
            ]
        );
        assert_eq!(checked("{ requird: { field: status } }").problems, vec!["`requird` is not a catalog rule".to_string()]);
        assert!(checked("{ compare: { field: start_date, op: \"=<\", other: end_date } }")
            .problems
            .contains(&"`op` must be one of ==, !=, <, <=, >, >=".to_string()));
        assert!(checked("{ required: { field: status }, email: { field: status } }")
            .problems[0]
            .contains("exactly one catalog rule"));
    }
}
