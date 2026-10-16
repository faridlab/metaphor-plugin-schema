//! Raw-SQL writers, and the rules they walk past.
//!
//! A rule held only by the service — compiled into the write guard without a database
//! form, kept in shadow, or enforced by a named verb — refuses a write that goes through
//! the generated service. Hand-written SQL that inserts, updates or deletes the same
//! table never meets it. The registry is every `INSERT INTO`, `UPDATE … SET` and
//! `DELETE FROM` in the module's hand-written sources; a service-only rule whose table
//! is on it, for a kind of write the rule applies to, is bypassable.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

use regex::Regex;

use super::{DeclaredRule, EnforcementStatus};
use crate::ast::ModuleSchema;

/// The marker every generated file carries in its header.
const GENERATED: &str = "by metaphor-schema. Do not edit manually.";

/// A kind of write a statement makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Write {
    Insert,
    Update,
    Delete,
}

/// The tables hand-written sources under `src/` write in raw SQL, by bare table name,
/// each with the kinds of write and where they happen (`<file>:<line>`).
pub fn raw_sql_writers(root: &Path) -> BTreeMap<String, BTreeMap<Write, Vec<String>>> {
    // A quoted identifier inside a Rust string literal reads `\"name\"`.
    let ident = r#"\\?"?([A-Za-z_][A-Za-z0-9_]*)\\?"?"#;
    let target = format!(r"(?:{ident}\s*\.\s*)?{ident}");
    let patterns = [
        (Write::Insert, Regex::new(&format!(r"(?i)\binsert\s+into\s+{target}")).expect("a valid pattern")),
        (Write::Update, Regex::new(&format!(r"(?i)\bupdate\s+(?:only\s+)?{target}\s+(?:as\s+\w+\s+|\w+\s+)?set\b")).expect("a valid pattern")),
        (Write::Delete, Regex::new(&format!(r"(?i)\bdelete\s+from\s+(?:only\s+)?{target}")).expect("a valid pattern")),
    ];
    let mut out: BTreeMap<String, BTreeMap<Write, Vec<String>>> = BTreeMap::new();
    for entry in walkdir::WalkDir::new(root.join("src")).sort_by_file_name().into_iter().flatten() {
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        if text.lines().take(12).any(|l| l.contains(GENERATED)) {
            continue;
        }
        let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        for (i, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for (kind, re) in &patterns {
                for cap in re.captures_iter(line) {
                    let table = cap[2].to_ascii_lowercase();
                    out.entry(table).or_default().entry(*kind).or_default().push(format!("{rel}:{}", i + 1));
                }
            }
        }
    }
    out
}

/// The rules of `schema` only the service holds whose table a raw-SQL writer of the module
/// writes, for a kind of write the rule applies to, each as `<hook file>: <rule name>`, with
/// the writers that walk past it.
pub fn bypassable_rules(schema: &ModuleSchema, root: &Path) -> BTreeMap<String, Vec<String>> {
    let writers = raw_sql_writers(root);
    let mut out = BTreeMap::new();
    if writers.is_empty() {
        return out;
    }
    for model in &schema.models {
        let Some(writes) = writers.get(&model.collection_name().to_ascii_lowercase()) else { continue };
        let held = database_held(schema, model);
        for rule in schema.rule_register.rules.iter().filter(|r| r.bound_model.as_deref() == Some(model.name.as_str())) {
            if !matches!(rule.status, EnforcementStatus::Guard | EnforcementStatus::Shadow | EnforcementStatus::Verb)
                || (rule.status == EnforcementStatus::Guard && held.contains(&rule.name))
            {
                continue;
            }
            // A write inside a file the rule's `enforced_by` names is the enforcer itself.
            let enforcers: Vec<&str> = rule
                .enforced_by
                .as_deref()
                .map(|c| c.split(',').filter_map(|one| one.trim().rsplit_once("::").map(|(file, _)| file)).collect())
                .unwrap_or_default();
            let past: Vec<String> = writes
                .iter()
                .filter(|(kind, _)| applies_to(rule, **kind))
                .flat_map(|(_, at)| at.iter().cloned())
                .filter(|at| !enforcers.iter().any(|f| at.rsplit_once(':').is_some_and(|(file, _)| file == *f)))
                .collect();
            if !past.is_empty() {
                out.insert(super::baseline::rule_key(&rule.source.file, &rule.name), past);
            }
        }
    }
    out
}

/// The rule names of `model` the database holds as well: a refusing row rule with an SQL
/// form (a `CHECK`), and a uniqueness rule (a unique index).
fn database_held(schema: &ModuleSchema, model: &crate::ast::Model) -> HashSet<String> {
    let mut held: HashSet<String> = super::compile::model_row_rules(schema, model)
        .into_iter()
        .filter(|r| r.refuse && r.layer == "row" && !r.reads_old && r.sql.is_some())
        .map(|r| r.name)
        .collect();
    held.extend(super::compile::model_uniqueness_rules(schema, model).into_iter().map(|u| u.name));
    held
}

/// Whether a rule applying `when` it says is one a write of `kind` must meet: an insert meets
/// a create rule, a delete a delete rule, and an update every other one (an update or a verb,
/// which moves a stored row). A rule that says nothing applies to every write.
fn applies_to(rule: &DeclaredRule, kind: Write) -> bool {
    if rule.when.is_empty() {
        return true;
    }
    rule.when.iter().any(|w| match kind {
        Write::Insert => w == "create",
        Write::Delete => w == "delete",
        Write::Update => w != "create" && w != "delete",
    })
}

/// The set the ratchet keeps.
pub fn bypassable_keys(schema: &ModuleSchema, root: &Path) -> BTreeSet<String> {
    bypassable_rules(schema, root).into_keys().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn the_registry_names_each_raw_write_and_skips_generated_files_and_comments() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "src/application/service/close_write_service.rs",
            "let q = \"INSERT INTO accounting.journals (id) VALUES ($1)\";\n\
             sqlx::query(\"UPDATE \\\"fiscal_periods\\\" SET status = 'locked'\");\n\
             // DELETE FROM journals WHERE true\n\
             sqlx::query(\"delete from only accounting.journal_lines where id = $1\");\n\
             let n = repo.update(entity).await; // update the row, set nothing\n",
        );
        write(root, "src/infrastructure/persistence/gen.rs", "// Generated by metaphor-schema. Do not edit manually.\nconst Q: &str = \"INSERT INTO ledgers (id) VALUES ($1)\";\n");
        let w = raw_sql_writers(root);
        assert_eq!(w.keys().cloned().collect::<Vec<_>>(), ["fiscal_periods", "journal_lines", "journals"]);
        assert_eq!(w["journals"][&Write::Insert], ["src/application/service/close_write_service.rs:1"]);
        assert!(w["fiscal_periods"].contains_key(&Write::Update));
        assert!(w["journal_lines"].contains_key(&Write::Delete));
    }

    #[test]
    fn a_write_inside_the_rules_own_enforcer_is_no_bypass() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "src/repo.rs", "pub async fn bump() { q(\"UPDATE lines SET qty = qty + 1 WHERE qty < cap\") }\n");
        write(root, "src/other.rs", "fn f() { q(\"UPDATE lines SET qty = 99\") }\n");
        let mut schema = ModuleSchema::new("m");
        let mut model = crate::ast::Model::new("Line");
        model.collection = Some("lines".into());
        schema.models.push(model);
        schema.rule_register.read_file(
            "line.hook.yaml",
            "name: Line\nrules:\n  capped:\n    when: [draw]\n    assert: \"qty <= cap\"\n    enforcement: service\n    enforced_by: \"src/repo.rs::bump\"\n",
        );
        let models = schema.models.clone();
        schema.rule_register.bind(&models);
        schema.rule_register.verify_enforced_by(root);
        let found = bypassable_rules(&schema, root);
        assert_eq!(found.get("line.hook.yaml: capped").map(Vec::as_slice), Some(&["src/other.rs:1".to_string()][..]), "{found:?}");
    }

    #[test]
    fn a_rule_meets_only_the_writes_it_applies_to() {
        let mut r = super::super::RuleRegister::default();
        r.read_file("orders.hook.yaml", "name: Order\nrules:\n  on_create:\n    when: [create]\n    condition: \"x > 0\"\n  on_submit:\n    when: [submit]\n    condition: \"x > 0\"\n  always:\n    condition: \"x > 0\"\n");
        let by = |n: &str| r.rules.iter().find(|x| x.name == n).unwrap().clone();
        assert!(applies_to(&by("on_create"), Write::Insert) && !applies_to(&by("on_create"), Write::Update));
        assert!(applies_to(&by("on_submit"), Write::Update) && !applies_to(&by("on_submit"), Write::Insert));
        assert!(applies_to(&by("always"), Write::Delete));
    }
}
