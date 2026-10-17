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
            // A claim naming a migration's object (a trigger, a constraint) is the database holding
            // the rule, and no raw-SQL writer walks past the database. The claim is verified first,
            // so the object stands in that migration.
            if enforcers.iter().any(|f| f.starts_with("migrations/")) && rule.enforced_by.as_deref().is_some_and(|c| super::claim_holds(root, c)) {
                continue;
            }
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

/// Raw-SQL statements in the module's hand-written sources that give an enum column a literal
/// its enum does not have: `SET status = 'cancelled'`, or `status = '...'` in a `WHERE`, in an
/// `UPDATE` or `DELETE` of the column's table. Postgres refuses the value at run time ("invalid
/// input value for enum"), so each is named as `<file>:<line> uses '<literal>' for
/// <table>.<column>, which <Enum> does not have (<labels>)`.
pub fn enum_literal_mismatches(schema: &ModuleSchema, root: &Path) -> Vec<String> {
    use crate::ast::TypeRef;
    // Each table's enum columns, with the enum's name and the labels the SQL type holds.
    let mut columns: BTreeMap<String, BTreeMap<String, (String, Vec<String>)>> = BTreeMap::new();
    for model in &schema.models {
        for field in &model.fields {
            let name = match &field.type_ref {
                TypeRef::Custom(n) => n,
                TypeRef::Optional(inner) => match inner.as_ref() {
                    TypeRef::Custom(n) => n,
                    _ => continue,
                },
                _ => continue,
            };
            let Some(e) = schema.enums.iter().find(|e| &e.name == name) else { continue };
            let labels = e.variants.iter().map(|v| crate::utils::enum_label(&v.name)).collect();
            columns.entry(model.collection_name().to_ascii_lowercase()).or_default().insert(field.name.to_ascii_lowercase(), (e.name.clone(), labels));
        }
    }
    let mut out = Vec::new();
    if columns.is_empty() {
        return out;
    }
    let ident = r#"\\?"?([A-Za-z_][A-Za-z0-9_]*)\\?"?"#;
    let target = format!(r"(?:{ident}\s*\.\s*)?{ident}");
    let statements = [
        Regex::new(&format!(r"(?i)\bupdate\s+(?:only\s+)?{target}\s")).expect("a valid pattern"),
        Regex::new(&format!(r"(?i)\bdelete\s+from\s+(?:only\s+)?{target}")).expect("a valid pattern"),
    ];
    // `[qualifier.]column = '<literal>'`
    let literal = Regex::new(r"(?:([A-Za-z_][A-Za-z0-9_]*)\s*\.\s*)?\b([A-Za-z_][A-Za-z0-9_]*)\s*=\s*'([^']*)'").expect("a valid pattern");
    let alias = Regex::new(r"(?i)^\s*(?:as\s+)?([A-Za-z_][A-Za-z0-9_]*)").expect("a valid pattern");
    for entry in walkdir::WalkDir::new(root.join("src")).sort_by_file_name().into_iter().flatten() {
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        if text.lines().take(12).any(|l| l.contains(GENERATED)) {
            continue;
        }
        // Comment lines blanked, so positions still give line numbers.
        let text: String = text
            .split_inclusive('\n')
            .map(|l| if l.trim_start().starts_with("//") { l.chars().map(|c| if c == '\n' { c } else { ' ' }).collect() } else { l.to_string() })
            .collect();
        let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        let mut found: Vec<(usize, String)> = Vec::new();
        for re in &statements {
            for cap in re.captures_iter(&text) {
                let Some(enum_columns) = columns.get(&cap[2].to_ascii_lowercase()) else { continue };
                let table = cap[2].to_ascii_lowercase();
                // The statement runs to the end of its string literal.
                let start = cap.get(0).map_or(0, |m| m.end());
                let rest = &text[start..];
                let end = rest
                    .char_indices()
                    .find(|&(i, c)| c == '"' && !rest[..i].ends_with('\\'))
                    .map_or(rest.len(), |(i, _)| i)
                    .min(4000);
                let end = (0..=end).rev().find(|&i| rest.is_char_boundary(i)).unwrap_or(0);
                // The name the statement gives its table (`UPDATE appraisals a`), if any: a column
                // another qualifier names belongs to another table.
                let own = alias
                    .captures(&rest[..end])
                    .map(|c| c[1].to_ascii_lowercase())
                    .filter(|a| !matches!(a.as_str(), "set" | "where" | "using" | "from" | "returning"));
                for lit in literal.captures_iter(&rest[..end]) {
                    if let Some(q) = lit.get(1).map(|q| q.as_str().to_ascii_lowercase()) {
                        if q != table && own.as_deref() != Some(q.as_str()) {
                            continue;
                        }
                    }
                    let column = lit[2].to_ascii_lowercase();
                    let Some((enum_name, labels)) = enum_columns.get(&column) else { continue };
                    // A value built at run time (`'{to}'` in a `format!`) is not a literal.
                    if labels.iter().any(|l| l == &lit[3]) || lit[3].contains('{') {
                        continue;
                    }
                    let at = start + lit.get(0).map_or(0, |m| m.start());
                    let line = text[..at].matches('\n').count() + 1;
                    found.push((
                        line,
                        format!(
                            "{rel}:{line} uses '{}' for {table}.{column}, which {enum_name} does not have ({})",
                            &lit[3],
                            labels.join(", ")
                        ),
                    ));
                }
            }
        }
        found.sort();
        out.extend(found.into_iter().map(|(_, f)| f));
    }
    out
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
    fn a_rule_the_database_holds_through_a_claimed_migration_object_has_no_bypass() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "src/repo.rs", "pub async fn close() { q(\"UPDATE lines SET status = 'done'\") }\n");
        write(root, "src/service.rs", "pub async fn retire() {}\n");
        write(root, "migrations/1_close.up.sql", "CREATE TRIGGER lines_close BEFORE UPDATE ON lines FOR EACH ROW EXECUTE FUNCTION f();\n");
        let mut schema = ModuleSchema::new("m");
        let mut model = crate::ast::Model::new("Line");
        model.collection = Some("lines".into());
        schema.models.push(model);
        schema.rule_register.read_file(
            "line.hook.yaml",
            "name: Line\nrules:\n  closed:\n    when: [close]\n    assert: \"no open children\"\n    enforcement: both\n    enforced_by: \"src/service.rs::retire, migrations/1_close.up.sql::lines_close\"\n",
        );
        let models = schema.models.clone();
        schema.rule_register.bind(&models);
        schema.rule_register.verify_enforced_by(root);
        assert!(bypassable_rules(&schema, root).is_empty());
    }

    /// A raw write that sets or matches an enum column with a literal its enum lacks fails at
    /// run time ("invalid input value for enum"), so it is named with its place.
    #[test]
    fn a_raw_write_of_a_literal_its_enum_lacks_is_named() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "src/application/service/cycle_service.rs",
            "fn a() { q(\"UPDATE performance.appraisal_cycles SET status = 'cancelled' WHERE id = $1\") }\n\
             fn b() { q(\"UPDATE appraisal_cycles SET status = 'closed', note = 'cancelled' WHERE status = 'open'\") }\n\
             fn c() {\n    q(r#\"UPDATE appraisal_cycles\n        SET status = 'archived'\n      WHERE id = $1\"#)\n}\n\
             fn d() { q(\"DELETE FROM appraisal_cycles WHERE status='Closed'\") }\n\
             fn e(to: &str) { q(&format!(\"UPDATE appraisal_cycles SET status = '{to}'\")) }\n\
             fn f() { q(\"UPDATE appraisal_cycles c SET status = 'open' WHERE EXISTS (SELECT 1 FROM reviews r WHERE r.status = 'pending') AND c.status = 'draft'\") }\n\
             // UPDATE appraisal_cycles SET status = 'ghost'\n",
        );
        write(root, "src/gen.rs", "// Generated by metaphor-schema. Do not edit manually.\nconst Q: &str = \"UPDATE appraisal_cycles SET status = 'nope'\";\n");
        let mut schema = ModuleSchema::new("performance");
        let mut model = crate::ast::Model::new("AppraisalCycle");
        model.collection = Some("appraisal_cycles".into());
        model.fields.push(crate::ast::Field { name: "status".into(), type_ref: crate::ast::TypeRef::Custom("CycleStatus".into()), ..Default::default() });
        model.fields.push(crate::ast::Field { name: "note".into(), type_ref: crate::ast::TypeRef::Primitive(crate::ast::PrimitiveType::String), ..Default::default() });
        schema.models.push(model);
        let mut e = crate::ast::EnumDef::new("CycleStatus");
        for v in ["Open", "Closed"] {
            e.variants.push(crate::ast::EnumVariant { name: v.into(), ..Default::default() });
        }
        schema.enums.push(e);
        let found = enum_literal_mismatches(&schema, root);
        assert_eq!(
            found,
            [
                "src/application/service/cycle_service.rs:1 uses 'cancelled' for appraisal_cycles.status, which CycleStatus does not have (open, closed)",
                "src/application/service/cycle_service.rs:5 uses 'archived' for appraisal_cycles.status, which CycleStatus does not have (open, closed)",
                "src/application/service/cycle_service.rs:8 uses 'Closed' for appraisal_cycles.status, which CycleStatus does not have (open, closed)",
                "src/application/service/cycle_service.rs:10 uses 'draft' for appraisal_cycles.status, which CycleStatus does not have (open, closed)",
            ],
            "{found:#?}"
        );
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
