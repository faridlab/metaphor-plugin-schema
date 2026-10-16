//! The rule ratchet: what a module's hook files leave unenforced, counted, so the numbers can
//! only go down.
//!
//! Each module commits `schema/rules.baseline.yaml`. `schema doctor` measures the module again
//! and fails when a count rises, or when an unenforced rule appears that the baseline does not
//! list, so a new rule cannot land unenforced even while an old one is fixed in the same change.
//! `schema rules --update-baseline` writes the module's current numbers after a fix.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{EnforcementStatus, RuleRegister};

/// Where a module keeps its baseline, relative to its root.
pub const FILE: &str = "schema/rules.baseline.yaml";

/// A module's ratchet numbers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Baseline {
    /// Rules no generated guard, trigger or named verb enforces.
    pub unenforced: usize,
    /// Things the register could not take as declared: unknown keys, unbound models, claims that
    /// name no function.
    pub warnings: usize,
    /// State machines that do not say whether generated or hand-written verbs own them.
    pub machines_without_verbs: usize,
    /// Rules carrying an `id` that no test of the module mentions.
    pub untested_rule_ids: usize,
    /// The unenforced rules, each as `<hook file>: <rule name>`.
    #[serde(default)]
    pub unenforced_rules: BTreeSet<String>,
    /// The rules only the service holds whose table the module's hand-written SQL also writes,
    /// each as `<hook file>: <rule name>`: a raw write never meets them. Absent from a baseline
    /// written before it was measured, which compares nothing until it is next written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bypassable_rules: Option<BTreeSet<String>>,
    /// The guarded transitions nothing visible holds — no generated verb compiles them and no
    /// verified `enforced_by` names the hand code that checks them — each as
    /// `<hook file>: <Entity>.<field> <verb>`. Absent from an older baseline, which compares
    /// nothing until it is next written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unclaimed_guards: Option<BTreeSet<String>>,
}

pub(crate) fn rule_key(file: &str, name: &str) -> String {
    format!("{file}: {name}")
}

/// Measure the module whose register this is, with its sources under `root`.
pub fn measure(register: &RuleRegister, root: &Path) -> Baseline {
    let unenforced_rules: BTreeSet<String> = register
        .rules
        .iter()
        .filter(|r| r.status == EnforcementStatus::Unenforced)
        .map(|r| rule_key(&r.source.file, &r.name))
        .collect();
    let tests = test_texts(root);
    let untested_rule_ids = register
        .rules
        .iter()
        .filter_map(|r| r.id.as_deref())
        .filter(|id| !tests.iter().any(|t| mentions(t, id)))
        .count();
    Baseline {
        unenforced: register.unenforced_rules(),
        warnings: register.warnings.len(),
        machines_without_verbs: register.machines.iter().filter(|m| m.verbs.is_none()).count(),
        untested_rule_ids,
        unenforced_rules,
        bypassable_rules: None,
        unclaimed_guards: Some(register.unclaimed_guards()),
    }
}

/// Measure the module `schema` describes, with its sources under `root`: [`measure`], plus the
/// service-only rules a raw-SQL writer walks past.
pub fn measure_module(schema: &crate::ast::ModuleSchema, root: &Path) -> Baseline {
    Baseline {
        bypassable_rules: Some(super::writers::bypassable_keys(schema, root)),
        ..measure(&schema.rule_register, root)
    }
}

/// Whether `text` names `id` as a word of its own (`R1` is not `R10`).
fn mentions(text: &str, id: &str) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    text.match_indices(id).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + id.len()..].chars().next();
        !before.is_some_and(word) && !after.is_some_and(word)
    })
}

/// The module's tests, as text: everything under `tests/` and `features/`, and the source files
/// that carry a test module.
fn test_texts(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for dir in ["tests", "features"] {
        for entry in walkdir::WalkDir::new(root.join(dir)).into_iter().flatten() {
            if entry.file_type().is_file() {
                if let Ok(t) = std::fs::read_to_string(entry.path()) {
                    out.push(t);
                }
            }
        }
    }
    for entry in walkdir::WalkDir::new(root.join("src")).into_iter().flatten() {
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == "rs") {
            if let Ok(t) = std::fs::read_to_string(entry.path()) {
                if t.contains("#[cfg(test)]") || t.contains("#[test]") || t.contains("#[tokio::test]") {
                    out.push(t);
                }
            }
        }
    }
    out
}

/// What makes `now` worse than `baseline`: a count that rose, or an unenforced rule the baseline
/// does not list. Empty when the module is as good or better.
pub fn regressions(baseline: &Baseline, now: &Baseline) -> Vec<String> {
    let mut out = Vec::new();
    for (what, was, is) in [
        ("unenforced rules", baseline.unenforced, now.unenforced),
        ("register warnings", baseline.warnings, now.warnings),
        ("state machines without a verbs declaration", baseline.machines_without_verbs, now.machines_without_verbs),
        ("rule ids no test mentions", baseline.untested_rule_ids, now.untested_rule_ids),
    ] {
        if is > was {
            out.push(format!("{what} rose from {was} to {is}"));
        }
    }
    for rule in now.unenforced_rules.difference(&baseline.unenforced_rules) {
        out.push(format!("a new rule is unenforced: {rule}"));
    }
    if let (Some(was), Some(is)) = (&baseline.unclaimed_guards, &now.unclaimed_guards) {
        for t in is.difference(was) {
            out.push(format!("a transition guard is held by nothing the register can see: {t}"));
        }
    }
    if let (Some(was), Some(is)) = (&baseline.bypassable_rules, &now.bypassable_rules) {
        for rule in is.difference(was) {
            out.push(format!("a rule only the service holds now has a raw-SQL writer: {rule}"));
        }
    }
    out
}

/// The baseline committed in the module at `root`, if any.
pub fn read(root: &Path) -> anyhow::Result<Option<Baseline>> {
    let path = root.join(FILE);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)?;
    Ok(Some(serde_yaml::from_str(&text)?))
}

/// Write `baseline` as the module's committed baseline.
pub fn write(root: &Path, baseline: &Baseline) -> anyhow::Result<()> {
    let header = "# The rule ratchet (schema doctor): these counts may only go down. Regenerate with\n\
                  # `metaphor schema rules . --update-baseline` after enforcing a rule.\n";
    std::fs::write(root.join(FILE), format!("{header}{}", serde_yaml::to_string(baseline)?))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn register(hook: &str) -> RuleRegister {
        let mut r = RuleRegister::default();
        r.read_file("orders.hook.yaml", hook);
        r
    }

    #[test]
    fn a_new_unenforced_rule_fails_even_when_the_count_stays_level() {
        let root = tempfile::tempdir().unwrap();
        let before = register("name: Orders\nrules:\n  old_rule:\n    id: R1\n    when: [submit]\n    condition: \"x > 0\"\n");
        let after = register("name: Orders\nrules:\n  new_rule:\n    id: R2\n    when: [submit]\n    condition: \"x > 0\"\n");
        let base = measure(&before, root.path());
        let now = measure(&after, root.path());
        assert_eq!((base.unenforced, now.unenforced), (1, 1));
        let problems = regressions(&base, &now);
        assert_eq!(problems, vec!["a new rule is unenforced: orders.hook.yaml: new_rule".to_string()]);
        assert!(regressions(&base, &base).is_empty(), "unchanged is not a regression");
    }

    #[test]
    fn a_rule_id_counts_as_tested_only_when_a_test_names_it_as_a_word() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("tests")).unwrap();
        std::fs::write(root.path().join("tests/orders.rs"), "// covers R1 and R10\n").unwrap();
        let r = register("name: Orders\nrules:\n  a:\n    id: R1\n    when: [submit]\n    condition: \"x > 0\"\n  b:\n    id: R2\n    when: [submit]\n    condition: \"x > 0\"\n  c:\n    id: R1-2\n    when: [submit]\n    condition: \"x > 0\"\n");
        assert_eq!(measure(&r, root.path()).untested_rule_ids, 2, "R2 and R1-2 are untested; R10 is not R1-2");
        assert!(mentions("see R1.", "R1") && !mentions("R10", "R1") && !mentions("XR1", "R1"));
    }

    #[test]
    fn a_newly_bypassable_rule_fails_once_the_baseline_records_the_set() {
        let set = |xs: &[&str]| Some(xs.iter().map(|x| x.to_string()).collect::<BTreeSet<_>>());
        let now = Baseline { bypassable_rules: set(&["a.hook.yaml: r1", "a.hook.yaml: r2"]), ..Default::default() };
        assert!(regressions(&Baseline::default(), &now).is_empty(), "an older baseline compares nothing");
        let was = Baseline { bypassable_rules: set(&["a.hook.yaml: r1"]), ..Default::default() };
        assert_eq!(regressions(&was, &now), ["a rule only the service holds now has a raw-SQL writer: a.hook.yaml: r2"]);
    }

    #[test]
    fn a_newly_unclaimed_guard_fails_once_the_baseline_records_the_set() {
        let set = |xs: &[&str]| Some(xs.iter().map(|x| x.to_string()).collect::<BTreeSet<_>>());
        let now = Baseline { unclaimed_guards: set(&["o.hook.yaml: Order.status ship"]), ..Default::default() };
        assert!(regressions(&Baseline::default(), &now).is_empty(), "an older baseline compares nothing");
        let was = Baseline { unclaimed_guards: set(&[]), ..Default::default() };
        assert_eq!(regressions(&was, &now), ["a transition guard is held by nothing the register can see: o.hook.yaml: Order.status ship"]);
    }

    #[test]
    fn a_count_that_rises_is_reported_and_one_that_falls_is_not() {
        let base = Baseline { unenforced: 3, warnings: 2, ..Default::default() };
        let now = Baseline { unenforced: 2, warnings: 3, ..Default::default() };
        assert_eq!(regressions(&base, &now), vec!["register warnings rose from 2 to 3".to_string()]);
    }
}
