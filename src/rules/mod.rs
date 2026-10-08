//! The rule register: every business rule and state machine a module's hook
//! files declare, with where it was declared, the model it governs, and how
//! it is enforced today.
//!
//! The legacy hook AST ([`crate::ast::Hook`]) is what the generators read, and
//! it keeps only part of what a hook file says: the list format loses its
//! `assert:` statements, its `state_machines:` and every key the parser does
//! not know, and a module-level file binds to no model at all. The register is
//! read from the raw YAML instead, so nothing a schema declares goes unseen:
//! what the generators do not enforce yet is reported as unenforced rather
//! than dropped. Generators do not read the register yet; adding it changes no
//! generated file.

pub mod reader;

use serde::{Deserialize, Serialize};

use crate::ast::Model;
use crate::utils::{to_pascal_case, to_snake_case};

/// Everything the hook files of one module declare.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuleRegister {
    pub rules: Vec<DeclaredRule>,
    pub machines: Vec<DeclaredMachine>,
    pub warnings: Vec<RegisterWarning>,
}

/// Where a declaration sits: the hook file, and the line when it can be found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRef {
    pub file: String,
    pub line: Option<usize>,
}

impl std::fmt::Display for SourceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}:{}", self.file, line),
            None => write!(f, "{}", self.file),
        }
    }
}

/// One rule as its hook file declares it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeclaredRule {
    pub source: SourceRef,
    /// The register's own identifier (`R1`, `G-MAIL-3`), when it carries one.
    pub id: Option<String>,
    pub name: String,
    /// The model the declaration names, as written: the rule's `entity:`, else
    /// the file's `name:`, `model:` or `module:`.
    pub target: String,
    /// The file's other names for its model, tried in order when `target`
    /// matches none.
    pub alternatives: Vec<String>,
    /// The model it governs, once [`RuleRegister::bind`] has matched a target.
    pub bound_model: Option<String>,
    pub body: RuleBody,
    /// When it applies, as written: `create`, `update`, a verb, or prose.
    pub when: Vec<String>,
    pub code: Option<String>,
    pub message: Option<String>,
    pub severity: Option<String>,
    /// The declared enforcement layer (ADR-0015), as written; `None` when the
    /// rule does not say, which the legacy parser reads as `db`.
    pub enforcement: Option<String>,
    pub justification: Option<String>,
    pub status: EnforcementStatus,
}

/// What a rule asks to be checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "text", rename_all = "snake_case")]
pub enum RuleBody {
    /// A `condition:` meant to be evaluated.
    Condition(String),
    /// An `assert:` statement: the invariant in words, not yet an expression.
    Assertion(String),
    /// A uniqueness or idempotency key (`unique_on:` with `on_conflict:`).
    UniqueOn {
        fields: Vec<String>,
        on_conflict: Option<String>,
    },
    /// Nothing to check: the rule names itself and says no more.
    Empty,
}

impl RuleBody {
    pub fn kind(&self) -> &'static str {
        match self {
            RuleBody::Condition(_) => "condition",
            RuleBody::Assertion(_) => "assertion",
            RuleBody::UniqueOn { .. } => "unique_on",
            RuleBody::Empty => "empty",
        }
    }
}

/// How a declared rule is enforced by generated code. Every rule is
/// `Unenforced` until the emitters that compile rules exist; the report
/// counts them so that number can only go down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnforcementStatus {
    Unenforced,
}

/// One state machine as its hook file declares it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeclaredMachine {
    pub source: SourceRef,
    pub target: String,
    pub alternatives: Vec<String>,
    pub bound_model: Option<String>,
    /// The field that holds the state.
    pub field: String,
    pub initial: Option<String>,
    pub states: Vec<String>,
    pub transitions: Vec<DeclaredTransition>,
    /// Named guards (`guards: { balanced: "total_debit == total_credit" }`).
    pub guards: Vec<(String, String)>,
    /// What a verb sets besides the state (`effects: { post: "set posted_at = now()" }`).
    pub effects: Vec<(String, String)>,
}

/// One transition of a declared machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeclaredTransition {
    /// The verb that moves it (`on:`, `name:`, `event:` or the map key).
    pub verb: Option<String>,
    pub from: Vec<String>,
    pub to: String,
    /// The guard as written: an expression, or the name of a guard in
    /// `guards:`, which [`DeclaredMachine::guard_expression`] resolves.
    pub guard: Option<String>,
    pub roles: Vec<String>,
    pub message: Option<String>,
}

impl DeclaredMachine {
    /// The expression a transition's guard stands for: the named guard's
    /// expression when the guard is a name in `guards:`, else the guard itself.
    pub fn guard_expression<'a>(&'a self, guard: &'a str) -> &'a str {
        self.guards
            .iter()
            .find(|(name, _)| name == guard)
            .map(|(_, expr)| expr.as_str())
            .unwrap_or(guard)
    }
}

/// Something the register could not take as declared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterWarning {
    pub source: SourceRef,
    pub message: String,
}

impl std::fmt::Display for RegisterWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.source, self.message)
    }
}

impl RuleRegister {
    /// Add one hook file's declarations.
    pub fn read_file(&mut self, file: &str, content: &str) {
        let read = reader::read_hook_file(file, content);
        self.rules.extend(read.rules);
        self.machines.extend(read.machines);
        self.warnings.extend(read.warnings);
    }

    /// Match every declaration to the model it governs, warning for each one
    /// that names no model of this module, and for a file whose names point
    /// at two different models.
    pub fn bind(&mut self, models: &[Model]) {
        let mut warnings = Vec::new();
        for rule in &mut self.rules {
            rule.bound_model = bind_first(&rule.target, &rule.alternatives, models, &rule.source, &mut warnings);
            if rule.bound_model.is_none() {
                warnings.push(RegisterWarning {
                    source: rule.source.clone(),
                    message: format!(
                        "rule `{}` names `{}`, which is not a model of this module; \
                         give it an `entity:` naming the model it governs",
                        rule.name, rule.target
                    ),
                });
            }
        }
        for machine in &mut self.machines {
            machine.bound_model =
                bind_first(&machine.target, &machine.alternatives, models, &machine.source, &mut warnings);
            if machine.bound_model.is_none() {
                warnings.push(RegisterWarning {
                    source: machine.source.clone(),
                    message: format!(
                        "state machine on `{}` names `{}`, which is not a model of this module",
                        machine.field, machine.target
                    ),
                });
            }
        }
        warnings.dedup();
        self.warnings.extend(warnings);
    }

    pub fn unenforced_rules(&self) -> usize {
        self.rules.iter().filter(|r| r.status == EnforcementStatus::Unenforced).count()
    }
}

/// The first of `target` then `alternatives` that names a model. When a later
/// one names a different model the file is ambiguous, and that is reported.
fn bind_first(
    target: &str,
    alternatives: &[String],
    models: &[Model],
    source: &SourceRef,
    warnings: &mut Vec<RegisterWarning>,
) -> Option<String> {
    let mut bound: Option<String> = None;
    for candidate in std::iter::once(target).chain(alternatives.iter().map(String::as_str)) {
        let Some(model) = bind_target(candidate, models) else { continue };
        match &bound {
            None => bound = Some(model),
            Some(first) if *first != model => warnings.push(RegisterWarning {
                source: source.clone(),
                message: format!(
                    "the file names both `{first}` and `{model}`; its declarations are read as `{first}`"
                ),
            }),
            Some(_) => {}
        }
    }
    bound
}

/// The model a declaration's target names. A target is written as a model
/// name (`Journal`), a model file (`user.model.yaml`), or snake case
/// (`journal_line`); all three name the same model.
pub fn bind_target(target: &str, models: &[Model]) -> Option<String> {
    let stem = target.trim().trim_end_matches(".yaml").trim_end_matches(".model");
    let wanted = to_snake_case(&to_pascal_case(stem));
    models
        .iter()
        .find(|m| m.name == stem || to_snake_case(&m.name) == wanted)
        .map(|m| m.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn models(names: &[&str]) -> Vec<Model> {
        names.iter().map(|n| Model::new(*n)).collect()
    }

    #[test]
    fn a_target_binds_by_name_file_or_snake_case() {
        let models = models(&["Journal", "JournalLine", "User"]);
        assert_eq!(bind_target("Journal", &models).as_deref(), Some("Journal"));
        assert_eq!(bind_target("user.model.yaml", &models).as_deref(), Some("User"));
        assert_eq!(bind_target("journal_line", &models).as_deref(), Some("JournalLine"));
        assert_eq!(bind_target("accounting", &models), None);
    }

    #[test]
    fn a_rule_that_names_no_model_is_reported() {
        let mut register = RuleRegister::default();
        register.read_file(
            "accounting.hook.yaml",
            "module: accounting\nrules:\n  - id: R1\n    name: balanced\n    assert: \"debits equal credits\"\n",
        );
        register.bind(&models(&["Journal"]));
        assert_eq!(register.rules[0].bound_model, None);
        assert!(register.warnings.iter().any(|w| w.message.contains("not a model of this module")));
    }
}
