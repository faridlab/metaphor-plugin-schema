//! Apply-order simulation over a module's migration chain.
//!
//! The defect class this guards: a migration edited after the fact to carry
//! DDL for a relation that a LATER migration creates. Long-lived databases
//! drift past the break (the relation already exists when the old migration
//! re-runs), so only a fresh-database chain apply hits it. Replaying the
//! chain from empty catches it cheaply, at generate time.
//!
//! Rules of the simulation:
//!
//! - DDL targets (`ALTER TABLE`, `CREATE INDEX/POLICY/TRIGGER ... ON`,
//!   `REFERENCES`, `EXECUTE FUNCTION`, DML targets) must already exist at
//!   the referencing statement's position in the chain.
//! - Relations qualified with a schema other than the module's own are
//!   cross-module references: exempt from violation (their apply order is
//!   owned by the service composition, not this module's chain) but
//!   reported as requirements.
//! - Function and `DO` bodies are dollar-quoted; PostgreSQL does not parse
//!   them at CREATE time, so references inside them do not count.
//! - `IF EXISTS` drops of absent relations are tolerated (Postgres semantics).
//! - Common-table-expression names (`name AS (`) are not relations and are
//!   skipped by the `FROM`/`JOIN` reference scan.

use std::collections::{BTreeSet, HashSet};

/// Label used for the in-flight migration appended to the chain during
/// generate-time checks.
pub const NEW_MIGRATION_LABEL: &str = "(this migration)";

/// A statement referencing a relation that does not exist at its position in
/// the chain.
#[derive(Debug, Clone, PartialEq)]
pub struct ChainViolation {
    /// Migration file the statement lives in (or [`NEW_MIGRATION_LABEL`]).
    pub file: String,
    /// The relation as written, lowercased.
    pub relation: String,
    /// Head of the offending statement, for humans.
    pub statement: String,
}

/// Result of replaying a migration chain.
#[derive(Debug, Default)]
pub struct ChainOrderReport {
    pub violations: Vec<ChainViolation>,
    /// Cross-module relations (foreign-schema-qualified) referenced by the
    /// chain; the service composition owns their apply order.
    pub external_requires: BTreeSet<String>,
}

/// Replay `migrations` in order against an empty database and report any
/// statement that references a relation not yet created at its position.
///
/// `own_schema` is the module's Postgres schema (`schema:` in
/// index.model.yaml); relations qualified with any other schema are treated
/// as cross-module.
pub fn simulate_chain(migrations: &[(String, String)], own_schema: &str) -> ChainOrderReport {
    let mut report = ChainOrderReport::default();
    let mut existing: HashSet<String> = HashSet::new();

    for (file, sql) in migrations {
        for stmt in split_statements(sql) {
            let tokens = tokenize(&stmt);
            let words: Vec<&str> = tokens.iter().map(|t| t.as_str()).collect();
            let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
            let lower_words: Vec<&str> = lower.iter().map(|s| s.as_str()).collect();
            let ctes = cte_names(&words);
            classify_statement(
                &lower_words, own_schema, file, &stmt, &ctes, &mut existing, &mut report,
            );
        }
    }

    report
}

/// Per-statement simulator state wrapper — exists to keep the borrow of
/// `existing`/`report` in one place.
struct Sim<'a> {
    own_schema: &'a str,
    file: &'a str,
    stmt: &'a str,
    ctes: &'a HashSet<String>,
    existing: &'a mut HashSet<String>,
    report: &'a mut ChainOrderReport,
    /// Relations already flagged missing by this statement — a relation can be
    /// referenced by both the verb arm and the FROM scan of one statement;
    /// flag it once.
    flagged: HashSet<String>,
}

impl Sim<'_> {
    fn require(&mut self, token: &str) {
        if self.ctes.contains(&token.to_lowercase()) {
            return;
        }
        let Some(key) = relation_key(token, self.own_schema) else {
            return;
        };
        let lower = token.to_lowercase();
        let own = self.own_schema.to_lowercase();
        let internal = lower.split('.').count() == 1
            || lower
                .split('.')
                .next()
                .is_some_and(|s| s.eq_ignore_ascii_case(&own));
        if internal {
            if !self.existing.contains(&key) && self.flagged.insert(key.clone()) {
                self.report.violations.push(ChainViolation {
                    file: self.file.to_string(),
                    relation: key,
                    statement: statement_head(self.stmt),
                });
            }
        } else {
            self.report.external_requires.insert(key);
        }
    }

    fn create(&mut self, token: &str) {
        if let Some(key) = relation_key(token, self.own_schema) {
            self.existing.insert(key);
        }
    }

    /// Functions live in their own namespace: a trigger may share its name
    /// with a function (`DROP TRIGGER g` must not unregister `CREATE
    /// FUNCTION g`), so function keys carry an `fn:` prefix.
    fn require_fn(&mut self, token: &str) {
        let Some(key) = relation_key(token, self.own_schema) else {
            return;
        };
        let lower = token.to_lowercase();
        let own = self.own_schema.to_lowercase();
        let internal = lower.split('.').count() == 1
            || lower
                .split('.')
                .next()
                .is_some_and(|s| s.eq_ignore_ascii_case(&own));
        let fn_key = format!("fn:{key}");
        if internal {
            if !self.existing.contains(&fn_key) && self.flagged.insert(fn_key) {
                self.report.violations.push(ChainViolation {
                    file: self.file.to_string(),
                    relation: key,
                    statement: statement_head(self.stmt),
                });
            }
        } else {
            self.report.external_requires.insert(key);
        }
    }

    fn create_fn(&mut self, token: &str) {
        if let Some(key) = relation_key(token, self.own_schema) {
            self.existing.insert(format!("fn:{key}"));
        }
    }

    fn drop_fn(&mut self, token: &str) {
        if let Some(key) = relation_key(token, self.own_schema) {
            self.existing.remove(&format!("fn:{key}"));
        }
    }

    /// Remove a dropped relation. An `IF EXISTS` drop of an absent relation is
    /// a no-op in Postgres; a bare drop of an absent relation errors loudly
    /// at apply time, so the simulation stays quiet either way.
    fn drop_rel(&mut self, token: &str) {
        if let Some(key) = relation_key(token, self.own_schema) {
            self.existing.remove(&key);
        }
    }
}

/// Classify one statement, updating `existing` (creates/drops) and `report`
/// (violations/external requires).
fn classify_statement(
    words: &[&str],
    own_schema: &str,
    file: &str,
    stmt: &str,
    ctes: &HashSet<String>,
    existing: &mut HashSet<String>,
    report: &mut ChainOrderReport,
) {
    let mut sim = Sim {
        own_schema,
        file,
        stmt,
        ctes,
        existing,
        report,
        flagged: HashSet::new(),
    };

    // ---- FROM/JOIN reference scan (every plain statement) ----------------
    for (k, w) in words.iter().enumerate() {
        if *w == "from" || *w == "join" {
            if let Some(rel) = words.get(k + 1) {
                if *rel == "(" || ctes.contains(&rel.to_lowercase()) {
                    continue;
                }
                // function call: FROM generate_series(...) — name followed by "("
                if words.get(k + 2) == Some(&"(") {
                    continue;
                }
                sim.require(rel);
            }
        }
    }

    // ---- verb dispatch ---------------------------------------------------
    match words.first().copied() {
        Some("create") => {
            let mut i = 1;
            while i < words.len()
                && matches!(
                    words[i],
                    "or" | "replace" | "global" | "temporary" | "temp" | "unlogged" | "unique"
                )
            {
                i += 1;
            }
            match words.get(i).copied() {
                Some("table") | Some("type") => {
                    if let Some(rel) = name_after(words, i + 1, &["if", "not", "exists"]) {
                        sim.create(&rel);
                    }
                }
                Some("function") | Some("procedure") => {
                    if let Some(rel) = name_after(words, i + 1, &["if", "not", "exists"]) {
                        sim.create_fn(&rel);
                    }
                }
                Some("index") => {
                    // CREATE [UNIQUE] INDEX [CONCURRENTLY] [IF NOT EXISTS] [name] ON <rel>
                    let mut j = i + 1;
                    skip_words(&mut j, words, &["concurrently", "if", "not", "exists"]);
                    if words.get(j).is_some_and(|w| *w != "on") {
                        j += 1; // the index name
                    }
                    if let Some(off) = find_word(&words[j.min(words.len())..], "on") {
                        if let Some(rel) = words.get(j + off + 1) {
                            sim.require(rel);
                        }
                    }
                }
                Some("policy") => {
                    // CREATE POLICY <name> ON <rel>
                    if let Some(off) = find_word(&words[i + 1..], "on") {
                        if let Some(rel) = words.get(i + 1 + off + 1) {
                            sim.require(rel);
                        }
                    }
                }
                Some("trigger") => {
                    // CREATE TRIGGER <name> ... ON <rel> ... EXECUTE FUNCTION <fn>
                    if let Some(off) = find_word(&words[i + 1..], "on") {
                        if let Some(rel) = words.get(i + 1 + off + 1) {
                            sim.require(rel);
                        }
                    }
                    for (k, w) in words.iter().enumerate() {
                        if (*w == "function" || *w == "procedure")
                            && words.get(k.wrapping_sub(1)) == Some(&"execute")
                        {
                            if let Some(rel) = words.get(k + 1) {
                                sim.require_fn(rel);
                            }
                        }
                    }
                }
                Some("materialized") => {
                    // CREATE MATERIALIZED VIEW <name> AS ...
                    if words.get(i + 1) == Some(&"view") {
                        if let Some(rel) = name_after(words, i + 2, &["if", "not", "exists"]) {
                            sim.create(&rel);
                        }
                    }
                }
                Some("view") | Some("sequence") | Some("domain") | Some("schema")
                | Some("extension") => {
                    if let Some(rel) = name_after(words, i + 1, &["if", "not", "exists"]) {
                        sim.create(&rel);
                    }
                }
                _ => {}
            }
        }
        Some("alter") => {
            if words.get(1) == Some(&"table") {
                if let Some(rel) = name_after(words, 2, &["if", "exists"]) {
                    sim.require(&rel);
                }
            }
            for (k, w) in words.iter().enumerate() {
                if *w == "references" {
                    if let Some(rel) = words.get(k + 1) {
                        sim.require(rel);
                    }
                }
            }
        }
        Some("drop") => {
            let mut i = 1;
            while i < words.len()
                && !matches!(
                    words[i],
                    "table" | "index" | "policy" | "trigger" | "view" | "sequence" | "type"
                        | "schema" | "extension" | "function" | "procedure" | "domain"
                        | "materialized"
                )
            {
                i += 1;
            }
            let mut j = i + 1;
            while matches!(words.get(j), Some(&"if") | Some(&"exists")) {
                j += 1;
            }
            match words.get(i).copied() {
                Some("table") | Some("view") | Some("sequence") | Some("type") | Some("domain") => {
                    // comma-separated relation list; CASCADE/RESTRICT may trail
                    while let Some(rel) = words.get(j) {
                        if matches!(*rel, "cascade" | "restrict") {
                            break;
                        }
                        sim.drop_rel(rel);
                        j += 1;
                        if words.get(j) == Some(&",") {
                            j += 1;
                        } else {
                            break;
                        }
                    }
                }
                Some("policy") | Some("trigger") | Some("index") => {
                    // Policy and trigger names are per-table objects, not
                    // chain-visible relations — dropping one unregisters
                    // nothing. Indexes are schema-level; dropping one does.
                    if words.get(i) == Some(&"index") {
                        if let Some(rel) = words.get(j) {
                            sim.drop_rel(rel);
                        }
                    }
                    // `ON <rel>` trailer — the relation itself must exist
                    if let Some(off) = find_word(&words[(j + 1).min(words.len())..], "on") {
                        if let Some(rel) = words.get(j + 1 + off + 1) {
                            sim.require(rel);
                        }
                    }
                }
                Some("function") | Some("procedure") => {
                    if let Some(rel) = words.get(j) {
                        sim.drop_fn(rel);
                    }
                }
                _ => {}
            }
        }
        Some("update") => {
            let mut j = 1;
            if words.get(j) == Some(&"only") {
                j += 1;
            }
            if let Some(rel) = words.get(j) {
                sim.require(rel);
            }
        }
        Some("insert") => {
            if words.get(1) == Some(&"into") {
                if let Some(rel) = words.get(2) {
                    sim.require(rel);
                }
            }
        }
        Some("delete") | Some("truncate") => {
            let mut j = 1;
            if matches!(words.get(j), Some(&"from") | Some(&"table")) {
                j += 1;
            }
            if let Some(rel) = words.get(j) {
                sim.require(rel);
            }
        }
        _ => {}
    }
}

/// Normalize a relation token to its chain-tracking key: unqualified or
/// own-schema-qualified names map to the bare name; foreign-schema-qualified
/// names keep the schema prefix.
fn relation_key(token: &str, own_schema: &str) -> Option<String> {
    let cleaned = token.trim_matches('"');
    let parts: Vec<&str> = cleaned.split('.').collect();
    match parts.as_slice() {
        [name] => Some(name.to_lowercase()),
        [schema, name] if schema.eq_ignore_ascii_case(own_schema) => {
            Some(name.to_lowercase())
        }
        [schema, name] => Some(format!("{}.{}", schema.to_lowercase(), name.to_lowercase())),
        _ => None,
    }
}

/// First line of a statement, bounded for display.
fn statement_head(stmt: &str) -> String {
    let first_line = stmt.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let trimmed = first_line.trim();
    let head: String = trimmed.chars().take(72).collect();
    if head.len() < trimmed.len() {
        format!("{head}…")
    } else {
        head
    }
}

/// Skip filler words starting at `j`.
fn skip_words(j: &mut usize, words: &[&str], fillers: &[&str]) {
    while words.get(*j).map(|w| fillers.contains(w)).unwrap_or(false) {
        *j += 1;
    }
}

/// Find the index of `word` in `words`, if present.
fn find_word(words: &[&str], word: &str) -> Option<usize> {
    words.iter().position(|w| *w == word)
}

/// Name token at/after `start`, skipping the given filler words first.
fn name_after(words: &[&str], start: usize, fillers: &[&str]) -> Option<String> {
    let mut j = start;
    skip_words(&mut j, words, fillers);
    words.get(j).map(|s| s.to_string())
}

/// Split SQL into statements, honoring dollar-quoted bodies and single-quoted
/// strings, dropping `--` comments outside quotes.
fn split_statements(sql: &str) -> Vec<String> {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut i = 0;
    let mut in_string = false;
    let mut dollar_tag: Option<String> = None;

    while i < chars.len() {
        let c = chars[i];
        if in_string {
            cur.push(c);
            if c == '\'' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if let Some(tag) = &dollar_tag {
            if c == '$' {
                let end = std::cmp::min(i + tag.chars().count(), chars.len());
                let seq: String = chars[i..end].iter().collect();
                if seq == *tag {
                    for _ in 0..tag.chars().count() {
                        cur.push(chars[i]);
                        i += 1;
                    }
                    dollar_tag = None;
                    continue;
                }
            }
            cur.push(c);
            i += 1;
            continue;
        }
        match c {
            '\'' => {
                in_string = true;
                cur.push(c);
                i += 1;
            }
            '-' if i + 1 < chars.len() && chars[i + 1] == '-' => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '$' => {
                let mut j = i + 1;
                while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                if j < chars.len() && chars[j] == '$' {
                    let tag: String = chars[i..=j].iter().collect();
                    cur.extend(chars[i..=j].iter().copied());
                    i = j + 1;
                    dollar_tag = Some(tag);
                } else {
                    cur.push(c);
                    i += 1;
                }
            }
            ';' => {
                let trimmed = cur.trim().to_string();
                if !trimmed.is_empty() {
                    out.push(trimmed);
                }
                cur.clear();
                i += 1;
            }
            _ => {
                cur.push(c);
                i += 1;
            }
        }
    }

    let trimmed = cur.trim().to_string();
    if !trimmed.is_empty() {
        out.push(trimmed);
    }
    out
}

/// Word tokens: runs of identifier characters (letters, digits, underscore,
/// dot, quote), plus whole single-quoted literals and whole dollar-quoted
/// bodies, plus one-character punctuation tokens so lookahead can detect
/// calls and subqueries. Dollar-quoted bodies stay opaque — PostgreSQL does
/// not parse them at CREATE time, so neither does the simulation.
fn tokenize(stmt: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = stmt.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            let mut lit = String::from("'");
            i += 1;
            while i < chars.len() {
                lit.push(chars[i]);
                if chars[i] == '\'' {
                    // '' is an escaped quote inside the literal
                    if chars.get(i + 1) == Some(&'\'') {
                        lit.push('\'');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            tokens.push(lit);
            continue;
        }
        if c == '$' {
            // opening $tag$ — consume through the matching close as one token
            let mut j = i + 1;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            if j < chars.len() && chars[j] == '$' {
                let tag: Vec<char> = chars[i..=j].to_vec();
                let tag_len = tag.len();
                let mut end = None;
                let mut k = j + 1;
                while k + tag_len <= chars.len() {
                    if chars[k..k + tag_len] == tag[..] {
                        end = Some(k + tag_len);
                        break;
                    }
                    k += 1;
                }
                let stop = end.unwrap_or(chars.len());
                tokens.push(chars[i..stop].iter().collect());
                i = stop;
                continue;
            }
            tokens.push(c.to_string());
            i += 1;
            continue;
        }
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '"' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric()
                    || chars[i] == '_'
                    || chars[i] == '.'
                    || chars[i] == '"')
            {
                i += 1;
            }
            tokens.push(chars[start..i].iter().collect());
        } else {
            if !c.is_whitespace() {
                tokens.push(c.to_string());
            }
            i += 1;
        }
    }
    tokens
}

/// Names of common-table expressions in a statement: every word token
/// directly followed by `AS (`. In valid SQL that shape only occurs for CTE
/// definitions, so it is a safe over-approximation.
fn cte_names(words: &[&str]) -> HashSet<String> {
    let mut names = HashSet::new();
    for (i, tok) in words.iter().enumerate() {
        if tok.eq_ignore_ascii_case("as") && words.get(i + 1) == Some(&"(") {
            if let Some(prev) = i.checked_sub(1).and_then(|p| words.get(p)) {
                let plain_ident = !prev.is_empty()
                    && prev
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '"');
                if plain_ident {
                    names.insert(prev.trim_matches('"').to_lowercase());
                }
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(migrations: &[(&str, &str)]) -> ChainOrderReport {
        let owned: Vec<(String, String)> = migrations
            .iter()
            .map(|(f, s)| (f.to_string(), s.to_string()))
            .collect();
        simulate_chain(&owned, "inventory")
    }

    #[test]
    fn in_order_chain_passes() {
        let report = check(&[
            ("001.up.sql", "CREATE TABLE inventory.warehouses (id uuid PRIMARY KEY);"),
            ("002.up.sql", "ALTER TABLE inventory.warehouses ADD COLUMN code text;"),
            (
                "003.up.sql",
                "CREATE UNIQUE INDEX idx_w ON inventory.warehouses (code);
                 CREATE POLICY p ON inventory.warehouses USING (true);",
            ),
        ]);
        assert!(report.violations.is_empty(), "{report:?}");
    }

    #[test]
    fn reference_to_a_later_created_relation_is_flagged() {
        let report = check(&[
            (
                "001.up.sql",
                "ALTER TABLE inventory.later_table ADD COLUMN x integer;",
            ),
            ("002.up.sql", "CREATE TABLE inventory.later_table (id uuid);"),
        ]);
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].file, "001.up.sql");
        assert_eq!(report.violations[0].relation, "later_table");
    }

    #[test]
    fn dropped_relations_are_forgotten() {
        let report = check(&[
            ("001.up.sql", "CREATE TABLE t (id uuid);"),
            ("002.up.sql", "DROP TABLE t;"),
            ("003.up.sql", "CREATE INDEX i ON t (id);"),
        ]);
        assert_eq!(report.violations.len(), 1, "{report:?}");
        assert_eq!(report.violations[0].relation, "t");
    }

    #[test]
    fn trigger_checks_both_table_and_function() {
        let report = check(&[
            ("001.up.sql", "CREATE TABLE inventory.warehouses (id uuid);"),
            (
                "002.up.sql",
                "CREATE TRIGGER trg BEFORE INSERT ON inventory.warehouses
                 FOR EACH ROW EXECUTE FUNCTION later_guard();",
            ),
            (
                "003.up.sql",
                "CREATE FUNCTION later_guard() RETURNS trigger AS $$ BEGIN RETURN NEW; END $$ LANGUAGE plpgsql;",
            ),
        ]);
        assert_eq!(report.violations.len(), 1, "{report:?}");
        assert_eq!(report.violations[0].relation, "later_guard");
    }

    #[test]
    fn dollar_quoted_bodies_are_not_parsed() {
        let report = check(&[
            (
                "001.up.sql",
                "DO $$
                 BEGIN
                   IF EXISTS (SELECT 1 FROM not_yet_created) THEN RAISE EXCEPTION 'x'; END IF;
                 END $$;",
            ),
            (
                "002.up.sql",
                "CREATE FUNCTION g() RETURNS trigger AS $body$
                 BEGIN
                   SELECT count(*) INTO v FROM also_not_created;
                   RETURN NEW;
                 END $body$ LANGUAGE plpgsql;",
            ),
        ]);
        assert!(report.violations.is_empty(), "{report:?}");
    }

    #[test]
    fn cross_schema_references_are_requirements_not_violations() {
        let report = check(&[
            ("001.up.sql", "CREATE TABLE inventory.warehouses (id uuid);"),
            (
                "002.up.sql",
                "UPDATE inventory.warehouses SET org_unit_id =
                 (SELECT id FROM organization.org_units WHERE kind = 'root' LIMIT 1);",
            ),
        ]);
        assert!(report.violations.is_empty(), "{report:?}");
        assert!(
            report.external_requires.contains("organization.org_units"),
            "{report:?}"
        );
    }

    #[test]
    fn unqualified_names_default_to_the_module_schema() {
        let report = check(&[
            ("001.up.sql", "CREATE TABLE warehouses (id uuid);"),
            ("002.up.sql", "UPDATE warehouses SET x = 1 WHERE company_id IS NOT NULL;"),
        ]);
        assert!(report.violations.is_empty(), "{report:?}");
    }

    #[test]
    fn insert_and_delete_targets_are_checked() {
        let report = check(&[
            ("001.up.sql", "INSERT INTO missing (id) VALUES (gen_random_uuid());"),
            ("002.up.sql", "DELETE FROM also_missing;"),
        ]);
        assert_eq!(report.violations.len(), 2, "{report:?}");
    }

    #[test]
    fn references_clause_is_checked() {
        let report = check(&[
            ("001.up.sql", "CREATE TABLE a (id uuid);"),
            (
                "002.up.sql",
                "ALTER TABLE a ADD CONSTRAINT fk FOREIGN KEY (b) REFERENCES b(id);",
            ),
        ]);
        assert_eq!(report.violations.len(), 1, "{report:?}");
        assert_eq!(report.violations[0].relation, "b");
    }

    #[test]
    fn cte_names_are_not_relations() {
        let report = check(&[(
            "001.up.sql",
            "CREATE TABLE warehouses (id uuid, org_unit_id uuid);
             UPDATE warehouses w SET org_unit_id = (
               WITH RECURSIVE up AS (
                 SELECT o.id FROM organization.org_units o WHERE o.id = w.org_unit_id
               )
               SELECT up.id FROM up LIMIT 1
             );",
        )]);
        assert!(report.violations.is_empty(), "{report:?}");
        assert!(
            report.external_requires.contains("organization.org_units"),
            "{report:?}"
        );
    }

    #[test]
    fn function_from_scan_skips_function_calls() {
        let report = check(&[(
            "001.up.sql",
            "CREATE TABLE t (id uuid);
             INSERT INTO t SELECT * FROM generate_series(1, 10);",
        )]);
        assert!(report.violations.is_empty(), "{report:?}");
    }

    #[test]
    fn drop_if_exists_on_absent_relation_is_tolerated() {
        let report = check(&[(
            "001.up.sql",
            "DROP TABLE IF EXISTS never_created; DROP POLICY IF EXISTS p ON absent_table;",
        )]);
        // DROP TABLE IF EXISTS on an absent relation is a no-op; the POLICY
        // variant still needs its ON table to exist.
        assert_eq!(report.violations.len(), 1, "{report:?}");
        assert_eq!(report.violations[0].relation, "absent_table");
    }

    #[test]
    fn comments_and_strings_do_not_confuse_the_splitter() {
        let report = check(&[(
            "001.up.sql",
            "-- a comment with a semicolon; and the phrase ALTER TABLE ghost
             CREATE TABLE t (note text DEFAULT 'x;y;FROM ghost2');",
        )]);
        assert!(report.violations.is_empty(), "{report:?}");
    }

    #[test]
    fn drop_trigger_never_unregisters_a_function_of_the_same_name() {
        // The generated org fence names the trigger and its guard function the
        // same; triggers live in a per-table namespace, so DROP TRIGGER must
        // not unregister CREATE FUNCTION's key.
        let report = check(&[(
            "001.up.sql",
            "CREATE TABLE warehouses (id uuid);
             CREATE OR REPLACE FUNCTION warehouses_org_unit_kind_guard() RETURNS trigger AS $$ BEGIN RETURN NEW; END $$ LANGUAGE plpgsql;
             DROP TRIGGER IF EXISTS warehouses_org_unit_kind_guard ON warehouses;
             CREATE TRIGGER warehouses_org_unit_kind_guard BEFORE INSERT ON warehouses
               FOR EACH ROW EXECUTE FUNCTION warehouses_org_unit_kind_guard();",
        )]);
        assert!(report.violations.is_empty(), "{report:?}");
    }

    #[test]
    fn the_rekey_migration_shape_passes() {
        // The generated org re-key, compressed to its load-bearing shape:
        // backfill + root anchor + kind guard + trigger + indexes + policy
        // swap + column drop, referencing organization.org_units cross-schema.
        let report = check(&[
            (
                "001.up.sql",
                "CREATE TABLE inventory.warehouses (
                   id uuid PRIMARY KEY,
                   company_id uuid NOT NULL,
                   code text,
                   name text,
                   metadata jsonb
                 );
                 CREATE UNIQUE INDEX idx_warehouses_company_id_code
                   ON inventory.warehouses (company_id, code)
                   WHERE (metadata->>'deleted_at') IS NULL;",
            ),
            (
                "002.up.sql",
                "ALTER TABLE inventory.warehouses ADD COLUMN IF NOT EXISTS org_unit_id uuid;
                 UPDATE inventory.warehouses SET org_unit_id = company_id WHERE company_id IS NOT NULL;
                 UPDATE inventory.warehouses SET org_unit_id =
                   (SELECT id FROM organization.org_units WHERE kind = 'root' LIMIT 1)
                   WHERE org_unit_id IS NULL;
                 ALTER TABLE inventory.warehouses ALTER COLUMN org_unit_id SET NOT NULL;
                 CREATE OR REPLACE FUNCTION inventory.warehouses_org_unit_kind_guard() RETURNS trigger AS $$
                 DECLARE v_kind text;
                 BEGIN
                   SELECT kind::text INTO v_kind FROM organization.org_units WHERE id = NEW.org_unit_id;
                   IF v_kind IS NULL THEN RAISE EXCEPTION 'unknown node'; END IF;
                   RETURN NEW;
                 END $$ LANGUAGE plpgsql;
                 DROP TRIGGER IF EXISTS trg ON inventory.warehouses;
                 CREATE TRIGGER trg BEFORE INSERT OR UPDATE OF org_unit_id ON inventory.warehouses
                   FOR EACH ROW EXECUTE FUNCTION inventory.warehouses_org_unit_kind_guard();
                 CREATE UNIQUE INDEX idx_warehouses_org ON inventory.warehouses (org_unit_id, code);
                 DROP POLICY IF EXISTS warehouses_company_isolation ON inventory.warehouses;
                 CREATE POLICY warehouses_org_unit_isolation ON inventory.warehouses
                   FOR ALL USING (org_unit_id = ANY(string_to_array(current_setting('app.scope_unit_ids', true), ',')::uuid[]));
                 ALTER TABLE inventory.warehouses DROP COLUMN company_id;",
            ),
        ]);
        assert!(report.violations.is_empty(), "{report:?}");
        assert!(
            report.external_requires.contains("organization.org_units"),
            "{report:?}"
        );
    }
}
