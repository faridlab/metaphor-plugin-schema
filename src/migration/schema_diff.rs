//! Schema diff detection and migration generation
//!
//! Compares database schemas and generates ALTER statements for changes.

use crate::ast::{CompanyFence, OrgFence};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::fmt::Write;

/// A snapshot of a database schema for comparison
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SchemaSnapshot {
    /// Tables in the schema
    pub tables: IndexMap<String, TableSnapshot>,
    /// Enums in the schema
    pub enums: IndexMap<String, EnumSnapshot>,
}

/// Snapshot of a table
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSnapshot {
    pub name: String,
    pub columns: IndexMap<String, ColumnSnapshot>,
    pub indexes: IndexMap<String, IndexSnapshot>,
    pub primary_key: Option<String>,
    /// Whether this table is company-fenced (ADR-0008): it has a `company_id` column not marked
    /// `@global`. Drives the RLS policy in the incremental migration path. `#[serde(default)]` so
    /// snapshots serialized before this field deserialize as `false` (introspected DB snapshots
    /// can't know `@global`; only the schema-derived `new` snapshot sets it, and only that side is
    /// read when emitting the fence).
    #[serde(default)]
    pub company_scoped: bool,
    /// The module-level fence declaration (ADR-0014) in force for this table; `None` on
    /// snapshots serialized before the key existed (and on introspected DB snapshots) means
    /// undeclared → `strict`, matching the legacy template.
    #[serde(default)]
    pub company_fence: Option<CompanyFence>,
    /// Whether this table is org-scoped (ADR-0028): it has an `org_unit_id` column.
    /// Drives the entitlement-union policy + kind guard in the incremental path.
    /// `#[serde(default)]` so snapshots serialized before the field deserialize as
    /// `false` — only the schema-derived `new` snapshot sets it.
    #[serde(default)]
    pub org_scoped: bool,
    /// The per-model `@org_root_shared` marker (ADR-0028): the kind guard admits the
    /// root node, the home for tenant-shared rows. Only meaningful together with
    /// `org_scoped`.
    #[serde(default)]
    pub org_root_shared: bool,
    /// The module-level org fence declaration (ADR-0028) in force for this table.
    /// `None` = undeclared (valid only pre-sweep — no `org_unit_id` column exists).
    #[serde(default)]
    pub org_fence: Option<OrgFence>,
}

/// Snapshot of a column
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnSnapshot {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    pub is_unique: bool,
}

/// Snapshot of an index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexSnapshot {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    pub index_type: String,
    /// Partial-index predicate in final SQL form (audit sub-keys already
    /// rewritten to their JSONB expressions), without the leading WHERE.
    /// Emitted after the column list wherever a diff path (re-)creates the
    /// index. Snapshots written before this field existed deserialize it as
    /// `None` — re-diffing against a fresh snapshot restores it.
    #[serde(default)]
    pub where_predicate: Option<String>,
}

/// Snapshot of an enum type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumSnapshot {
    pub name: String,
    pub variants: Vec<String>,
}

/// Result of comparing two schemas
#[derive(Debug, Clone, Default)]
pub struct SchemaDiff {
    /// Tables to add
    pub tables_added: Vec<String>,
    /// Tables to remove
    pub tables_removed: Vec<String>,
    /// Changes to existing tables
    pub table_changes: IndexMap<String, TableChange>,
    /// Enums to add
    pub enums_added: Vec<String>,
    /// Enums to remove
    pub enums_removed: Vec<String>,
    /// Enum changes (add/remove variants)
    pub enum_changes: IndexMap<String, EnumChange>,
}

/// Changes to a table
#[derive(Debug, Clone, Default)]
pub struct TableChange {
    pub table_name: String,
    pub columns_added: Vec<ColumnSnapshot>,
    pub columns_removed: Vec<String>,
    pub columns_modified: Vec<ColumnChange>,
    pub indexes_added: Vec<IndexSnapshot>,
    pub indexes_removed: Vec<String>,
    /// Possible column renames (heuristic: same type, one added + one removed)
    pub rename_candidates: Vec<RenameCandidate>,
    /// ADR-0014 posture flip on an existing table: (old, new) effective fence. Columns and
    /// indexes may be untouched — without this field the flip is invisible to the incremental
    /// path and the old policy template silently stays in place. `None` when the table is new,
    /// gains `company_id` in this same diff (the gain path installs the fence fresh), the
    /// posture is unchanged, or the table is being org re-keyed (the re-key owns its policy
    /// swap — a flip-to-`none` record here would wrongly disable RLS mid-re-key).
    pub fence_flip: Option<(CompanyFence, CompanyFence)>,
    /// A company→org scoping-key re-key (ADR-0028): `company_id` removed, `org_unit_id`
    /// added, both uuid, on an existing table. Detected structurally; the re-key owns its
    /// ordered emission (add nullable → backfill → NOT NULL → kind guard → re-keyed indexes
    /// → policy swap → drop the old column) and pulls both columns and their indexes out of
    /// the naive add/drop/rename paths so they can't double-emit or misorder.
    pub org_rekey: Option<OrgRekey>,
}

/// Everything the ordered re-key emission needs (ADR-0028), stashed at detection time
/// because the generic up/down paths only see the filtered `TableChange`.
#[derive(Debug, Clone)]
pub struct OrgRekey {
    /// The effective company posture being left behind (strict / shared_blank /
    /// shared_tree) — the down path restores this template.
    pub old_fence: CompanyFence,
    /// The per-model `@org_root_shared` marker from the new snapshot: the kind guard
    /// admits root nodes (the home for tenant-shared rows).
    pub org_root_shared: bool,
    /// Re-keyed indexes (they reference `org_unit_id`): built after the backfill in the
    /// up path — before it, uniqueness would hold over all-NULL keys — and dropped first
    /// in the down path.
    pub rekeyed_indexes: Vec<IndexSnapshot>,
    /// The `company_id` indexes the up path retires (they drop with the column); the
    /// down path recreates them from these definitions.
    pub replaced_indexes: Vec<IndexSnapshot>,
}

/// A possible column rename detected by matching types between added and removed columns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameCandidate {
    pub old_name: String,
    pub new_name: String,
    pub data_type: String,
}

/// A column modification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnChange {
    pub column_name: String,
    pub old_type: Option<String>,
    pub new_type: Option<String>,
    pub nullable_changed: Option<bool>,
    pub default_changed: Option<String>,
}

/// An index modification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexChange {
    pub index_name: String,
    pub old_columns: Vec<String>,
    pub new_columns: Vec<String>,
}

/// Changes to an enum type
#[derive(Debug, Clone, Default)]
pub struct EnumChange {
    pub enum_name: String,
    pub variants_added: Vec<String>,
    pub variants_removed: Vec<String>,
}

impl SchemaDiff {
    /// Check if there are any changes
    pub fn has_changes(&self) -> bool {
        !self.tables_added.is_empty()
            || !self.tables_removed.is_empty()
            || !self.table_changes.is_empty()
            || !self.enums_added.is_empty()
            || !self.enums_removed.is_empty()
            || !self.enum_changes.is_empty()
    }

    /// Check if there are any destructive changes
    pub fn has_destructive_changes(&self) -> bool {
        if !self.tables_removed.is_empty() || !self.enums_removed.is_empty() {
            return true;
        }

        for change in self.table_changes.values() {
            if !change.columns_removed.is_empty() {
                return true;
            }
        }

        for change in self.enum_changes.values() {
            if !change.variants_removed.is_empty() {
                return true;
            }
        }

        false
    }

    /// Get a summary of changes
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();

        if !self.tables_added.is_empty() {
            lines.push(format!("  + {} new table(s)", self.tables_added.len()));
        }
        if !self.tables_removed.is_empty() {
            lines.push(format!(
                "  - {} table(s) to remove",
                self.tables_removed.len()
            ));
        }
        if !self.table_changes.is_empty() {
            let mut total_cols_added = 0;
            let mut total_cols_removed = 0;
            let mut total_cols_modified = 0;

            for change in self.table_changes.values() {
                total_cols_added += change.columns_added.len();
                total_cols_removed += change.columns_removed.len();
                total_cols_modified += change.columns_modified.len();
            }

            if total_cols_added > 0 {
                lines.push(format!("  + {} column(s) to add", total_cols_added));
            }
            if total_cols_removed > 0 {
                lines.push(format!("  - {} column(s) to remove", total_cols_removed));
            }
            if total_cols_modified > 0 {
                lines.push(format!("  ~ {} column(s) modified", total_cols_modified));
            }

            let total_rekeys = self
                .table_changes
                .values()
                .filter(|c| c.org_rekey.is_some())
                .count();
            if total_rekeys > 0 {
                lines.push(format!(
                    "  ~ {} table(s) re-keying company_id to org_unit_id",
                    total_rekeys
                ));
            }
        }
        if !self.enums_added.is_empty() {
            lines.push(format!("  + {} new enum(s)", self.enums_added.len()));
        }
        if !self.enums_removed.is_empty() {
            lines.push(format!(
                "  - {} enum(s) to remove",
                self.enums_removed.len()
            ));
        }

        if lines.is_empty() {
            "No changes detected".to_string()
        } else {
            lines.join("\n")
        }
    }
}

/// Compare two schema snapshots and return the differences
pub fn diff_schemas(old: &SchemaSnapshot, new: &SchemaSnapshot) -> SchemaDiff {
    let mut diff = SchemaDiff::default();

    // Find added and removed tables
    for table_name in new.tables.keys() {
        if !old.tables.contains_key(table_name) {
            diff.tables_added.push(table_name.clone());
        }
    }

    for table_name in old.tables.keys() {
        if !new.tables.contains_key(table_name) {
            diff.tables_removed.push(table_name.clone());
        }
    }

    // Compare existing tables
    for (table_name, new_table) in &new.tables {
        if let Some(old_table) = old.tables.get(table_name) {
            let change = diff_tables(old_table, new_table);
            if change.has_changes() {
                diff.table_changes.insert(table_name.clone(), change);
            }
        }
    }

    // Find added and removed enums
    for enum_name in new.enums.keys() {
        if !old.enums.contains_key(enum_name) {
            diff.enums_added.push(enum_name.clone());
        }
    }

    for enum_name in old.enums.keys() {
        if !new.enums.contains_key(enum_name) {
            diff.enums_removed.push(enum_name.clone());
        }
    }

    // Compare existing enums
    for (enum_name, new_enum) in &new.enums {
        if let Some(old_enum) = old.enums.get(enum_name) {
            let change = diff_enums(old_enum, new_enum);
            if change.has_changes() {
                diff.enum_changes.insert(enum_name.clone(), change);
            }
        }
    }

    diff
}

/// The fence posture a snapshot effectively carries: an unscoped table (no `company_id`, or
/// `@global`) is unfenced regardless of declaration; a scoped table with no declaration
/// (legacy snapshot) means `strict`, matching the legacy template the diff path emits.
fn effective_posture(table: &TableSnapshot) -> CompanyFence {
    if !table.company_scoped {
        return CompanyFence::None;
    }
    table.company_fence.unwrap_or(CompanyFence::Strict)
}

fn diff_tables(old: &TableSnapshot, new: &TableSnapshot) -> TableChange {
    let mut change = TableChange {
        table_name: new.name.clone(),
        ..Default::default()
    };

    // Find added and removed columns
    for (col_name, col) in &new.columns {
        if !old.columns.contains_key(col_name) {
            change.columns_added.push(col.clone());
        }
    }

    for col_name in old.columns.keys() {
        if !new.columns.contains_key(col_name) {
            change.columns_removed.push(col_name.clone());
        }
    }

    // Company→org re-key (ADR-0028): `company_id` among the removed, `org_unit_id`
    // among the added, both uuid, and the new snapshot is org-scoped. Detected before
    // the flip below because the re-key owns its policy swap: a computed flip to `none`
    // would emit `DISABLE ROW LEVEL SECURITY` on a table whose fence is being swapped,
    // not lifted.
    let is_rekey = new.org_scoped
        && change.columns_removed.iter().any(|c| {
            c == "company_id"
                && old
                    .columns
                    .get(c)
                    .is_some_and(|col| col.data_type.eq_ignore_ascii_case("uuid"))
        })
        && change.columns_added.iter().any(|c| {
            c.name == "org_unit_id" && c.data_type.eq_ignore_ascii_case("uuid")
        });

    // Posture flip on an existing table (ADR-0014): same columns, different fence
    // declaration → the RLS policy template must be re-emitted. Skipped when `company_id`
    // is newly added here — that path installs the fence from scratch, so a flip record
    // would only duplicate it.
    if !is_rekey && change.columns_added.iter().all(|c| c.name != "company_id") {
        let (old_posture, new_posture) = (effective_posture(old), effective_posture(new));
        if old_posture != new_posture {
            change.fence_flip = Some((old_posture, new_posture));
        }
    }

    // Detect rename candidates: match removed + added columns by type
    detect_rename_candidates(&mut change, old);

    // Compare existing columns
    for (col_name, new_col) in &new.columns {
        if let Some(old_col) = old.columns.get(col_name) {
            if let Some(col_change) = diff_columns(old_col, new_col) {
                change.columns_modified.push(col_change);
            }
        }
    }

    // Find added and removed indexes
    for (idx_name, idx) in &new.indexes {
        if !old.indexes.contains_key(idx_name) {
            change.indexes_added.push(idx.clone());
        }
    }

    for idx_name in old.indexes.keys() {
        if !new.indexes.contains_key(idx_name) {
            change.indexes_removed.push(idx_name.clone());
        }
    }

    // Populate the re-key and pull its columns/indexes out of the naive paths: the
    // ordered emission owns them, and the rename heuristic would otherwise suggest
    // `RENAME COLUMN company_id TO org_unit_id` — which skips the root-anchor backfill
    // for shared rows and the NOT NULL step entirely.
    if is_rekey {
        let rekeyed_indexes: Vec<IndexSnapshot> = change
            .indexes_added
            .iter()
            .filter(|idx| idx.columns.iter().any(|c| c == "org_unit_id"))
            .cloned()
            .collect();
        let replaced_indexes: Vec<IndexSnapshot> = old
            .indexes
            .values()
            .filter(|idx| idx.columns.iter().any(|c| c == "company_id"))
            .cloned()
            .collect();
        let replaced_names: Vec<String> =
            replaced_indexes.iter().map(|idx| idx.name.clone()).collect();
        change.columns_removed.retain(|c| c != "company_id");
        change.columns_added.retain(|c| c.name != "org_unit_id");
        change
            .indexes_added
            .retain(|idx| !idx.columns.iter().any(|c| c == "org_unit_id"));
        change.indexes_removed.retain(|name| !replaced_names.contains(name));
        // The rename heuristic will already have paired the two uuid columns —
        // retract that suggestion here, same reason as above.
        change
            .rename_candidates
            .retain(|r| !(r.old_name == "company_id" && r.new_name == "org_unit_id"));
        change.org_rekey = Some(OrgRekey {
            old_fence: effective_posture(old),
            org_root_shared: new.org_root_shared,
            rekeyed_indexes,
            replaced_indexes,
        });
    }

    change
}

/// Detect possible column renames by matching types between added and removed columns.
///
/// Heuristic: if exactly one removed column has the same data_type as exactly one
/// added column, suggest it as a rename candidate.
fn detect_rename_candidates(change: &mut TableChange, old: &TableSnapshot) {
    let mut used_added = std::collections::HashSet::new();
    let mut used_removed = std::collections::HashSet::new();

    for removed_name in &change.columns_removed {
        if used_removed.contains(removed_name) {
            continue;
        }
        if let Some(old_col) = old.columns.get(removed_name) {
            // Find added columns with matching type
            let matches: Vec<usize> = change
                .columns_added
                .iter()
                .enumerate()
                .filter(|(i, added)| {
                    !used_added.contains(i) && added.data_type == old_col.data_type
                })
                .map(|(i, _)| i)
                .collect();

            // Only suggest rename if exactly one match (unambiguous)
            if matches.len() == 1 {
                let idx = matches[0];
                let added = &change.columns_added[idx];
                change.rename_candidates.push(RenameCandidate {
                    old_name: removed_name.clone(),
                    new_name: added.name.clone(),
                    data_type: old_col.data_type.clone(),
                });
                used_added.insert(idx);
                used_removed.insert(removed_name.clone());
            }
        }
    }
}

fn diff_columns(old: &ColumnSnapshot, new: &ColumnSnapshot) -> Option<ColumnChange> {
    let mut change = ColumnChange {
        column_name: new.name.clone(),
        old_type: None,
        new_type: None,
        nullable_changed: None,
        default_changed: None,
    };

    let mut has_changes = false;

    if old.data_type != new.data_type {
        change.old_type = Some(old.data_type.clone());
        change.new_type = Some(new.data_type.clone());
        has_changes = true;
    }

    if old.nullable != new.nullable {
        change.nullable_changed = Some(new.nullable);
        has_changes = true;
    }

    if old.default != new.default {
        change.default_changed = new.default.clone();
        has_changes = true;
    }

    if has_changes {
        Some(change)
    } else {
        None
    }
}

fn diff_enums(old: &EnumSnapshot, new: &EnumSnapshot) -> EnumChange {
    let mut change = EnumChange {
        enum_name: new.name.clone(),
        ..Default::default()
    };

    for variant in &new.variants {
        if !old.variants.contains(variant) {
            change.variants_added.push(variant.clone());
        }
    }

    for variant in &old.variants {
        if !new.variants.contains(variant) {
            change.variants_removed.push(variant.clone());
        }
    }

    change
}

impl TableChange {
    fn has_changes(&self) -> bool {
        !self.columns_added.is_empty()
            || !self.columns_removed.is_empty()
            || !self.columns_modified.is_empty()
            || !self.indexes_added.is_empty()
            || !self.indexes_removed.is_empty()
            || self.fence_flip.is_some()
            || self.org_rekey.is_some()
    }
}

impl EnumChange {
    fn has_changes(&self) -> bool {
        !self.variants_added.is_empty() || !self.variants_removed.is_empty()
    }
}

/// ` WHERE ...` suffix for a partial index, or empty when the snapshot carries
/// no predicate.
fn index_where_suffix(idx: &IndexSnapshot) -> String {
    idx.where_predicate
        .as_deref()
        .map(|p| format!(" WHERE {}", p))
        .unwrap_or_default()
}

/// Generate a combined migration SQL (UP + DOWN) from a schema diff.
///
/// For separate files, use [`generate_up_migration`] and [`generate_down_migration`].
/// When `destructive` is true, DROP statements are emitted uncommented.
pub fn generate_migration(
    diff: &SchemaDiff,
    new_schema: &SchemaSnapshot,
    destructive: bool,
) -> String {
    let mut output = String::new();

    writeln!(output, "-- Migration generated by metaphor-schema").unwrap();
    writeln!(output, "-- WARNING: Review carefully before applying!").unwrap();
    writeln!(output).unwrap();

    // UP section
    writeln!(output, "-- ============================================").unwrap();
    writeln!(output, "-- UP Migration").unwrap();
    writeln!(output, "-- ============================================").unwrap();
    writeln!(output).unwrap();
    output.push_str(&generate_up_migration(diff, new_schema, destructive));

    // DOWN section
    writeln!(output).unwrap();
    writeln!(output, "-- ============================================").unwrap();
    writeln!(output, "-- DOWN Migration (Rollback)").unwrap();
    writeln!(output, "-- ============================================").unwrap();
    writeln!(output).unwrap();
    output.push_str(&generate_down_migration(diff));

    output
}

/// Generate UP migration SQL only.
///
/// Enhanced with safety patterns:
/// - **Safe NOT NULL**: 3-step pattern (ADD NULL → UPDATE DEFAULT → SET NOT NULL)
/// - **Type widening annotations**: Comments indicating safe vs review-required changes
/// - **CONCURRENTLY indexes**: Suggested for non-blocking index creation on existing tables
/// - **Rename candidates**: Suggested `RENAME COLUMN` for matching type pairs
///
/// When `destructive` is true, DROP statements are emitted uncommented (active SQL).
/// When false (default), they are commented out for safety.
pub fn generate_up_migration(
    diff: &SchemaDiff,
    new_schema: &SchemaSnapshot,
    destructive: bool,
) -> String {
    use super::pipeline::is_safe_type_widening;

    let mut output = String::new();

    // The shared_tree helper is `CREATE OR REPLACE` — emitting it once per migration before
    // the first policy that reads it is the convention (company_subtree_helper_sql docs).
    // Both new-table and flip emissions below share this flag.
    let mut subtree_helper_emitted = false;

    // Create new enums first (tables may reference them)
    for enum_name in &diff.enums_added {
        if let Some(enum_def) = new_schema.enums.get(enum_name) {
            writeln!(output, "-- Create enum {}", enum_name).unwrap();
            let variants: Vec<String> = enum_def
                .variants
                .iter()
                .map(|v| format!("'{}'", v))
                .collect();
            writeln!(
                output,
                "CREATE TYPE {} AS ENUM ({});",
                enum_name,
                variants.join(", ")
            )
            .unwrap();
            writeln!(output).unwrap();
        }
    }

    // Add enum variants
    for (enum_name, change) in &diff.enum_changes {
        for variant in &change.variants_added {
            writeln!(
                output,
                "ALTER TYPE {} ADD VALUE IF NOT EXISTS '{}';",
                enum_name, variant
            )
            .unwrap();
        }
    }

    // Create new tables
    for table_name in &diff.tables_added {
        if let Some(table) = new_schema.tables.get(table_name) {
            writeln!(output, "-- Create table {}", table_name).unwrap();
            writeln!(output, "CREATE TABLE IF NOT EXISTS {} (", table_name).unwrap();

            let mut column_defs = Vec::new();
            for col in table.columns.values() {
                let mut col_def = format!("    {} {}", col.name, col.data_type);
                if !col.nullable {
                    col_def.push_str(" NOT NULL");
                }
                if let Some(default) = &col.default {
                    col_def.push_str(&format!(" DEFAULT {}", default));
                }
                column_defs.push(col_def);
            }

            if let Some(pk) = &table.primary_key {
                column_defs.push(format!("    PRIMARY KEY ({})", pk));
            }

            writeln!(output, "{}", column_defs.join(",\n")).unwrap();
            writeln!(output, ");").unwrap();

            // Create indexes for new tables (standard, not CONCURRENTLY)
            for idx in table.indexes.values() {
                let unique = if idx.unique { "UNIQUE " } else { "" };
                writeln!(
                    output,
                    "CREATE {}INDEX IF NOT EXISTS {} ON {} ({}){};",
                    unique,
                    idx.name,
                    table_name,
                    idx.columns.join(", "),
                    index_where_suffix(idx)
                )
                .unwrap();
            }

            // A new company-scoped table gets its RLS fence in the same migration (ADR-0008) — the
            // full-regen path emits this too; without it, a table added via the incremental path would
            // ship UNFENCED. The template follows the module declaration (ADR-0014); an undeclared
            // (or pre-declaration) snapshot means `strict`.
            if table.company_scoped && table.company_fence != Some(CompanyFence::None) {
                writeln!(output, "-- Company RLS fence (ADR-0008)").unwrap();
                let fence = table.company_fence.unwrap_or(CompanyFence::Strict);
                if matches!(fence, CompanyFence::SharedTree) && !subtree_helper_emitted {
                    let (helper, _) = crate::generators::sql::company_subtree_helper_sql();
                    output.push_str(&helper);
                    writeln!(output).unwrap();
                    subtree_helper_emitted = true;
                }
                let (up, _down) = crate::generators::sql::company_rls_sql(
                    &fence,
                    table_name,
                    &format!("{table_name}_company_isolation"),
                );
                output.push_str(&up);
            }

            writeln!(output).unwrap();
        }
    }

    // Alter existing tables
    for (table_name, change) in &diff.table_changes {
        // Rename candidates (commented out — user confirms)
        if !change.rename_candidates.is_empty() {
            writeln!(output).unwrap();
            writeln!(
                output,
                "-- POSSIBLE RENAMES in {} (uncomment if this is a rename, not add+drop):",
                table_name
            )
            .unwrap();
            for rename in &change.rename_candidates {
                writeln!(
                    output,
                    "-- ALTER TABLE {} RENAME COLUMN {} TO {};  -- type: {}",
                    table_name, rename.old_name, rename.new_name, rename.data_type
                )
                .unwrap();
            }
            writeln!(output).unwrap();
        }

        // Company→org scoping-key re-key (ADR-0028): the ordered swap. The sequence is
        // load-bearing — backfill must precede both SET NOT NULL and the re-keyed unique
        // indexes (uniqueness over all-NULL keys enforces nothing), the kind guard must
        // precede any write the swapped policy allows, and the old column drops last
        // (its dependent indexes go with it). RLS stays ENABLED+FORCE throughout: the
        // fence is being swapped, not lifted. All org statements come from the same
        // shared helpers the full-regen path uses, so the two can never drift.
        if let Some(rekey) = &change.org_rekey {
            let bare = table_name.rsplit('.').next().unwrap_or(table_name);
            writeln!(output).unwrap();
            writeln!(
                output,
                "-- Org re-key (ADR-0028): swap the tenant scoping key on {table_name}"
            )
            .unwrap();
            writeln!(
                output,
                "ALTER TABLE {table_name} ADD COLUMN IF NOT EXISTS org_unit_id UUID;"
            )
            .unwrap();
            writeln!(
                output,
                "UPDATE {table_name} SET org_unit_id = company_id WHERE company_id IS NOT NULL;"
            )
            .unwrap();
            if rekey.old_fence == CompanyFence::SharedBlank {
                writeln!(
                    output,
                    "-- shared_blank rows (NULL company_id) anchor on the root node; the\n\
                     -- entitlement union always contains it, so shared rows stay shared (ADR-0028)."
                )
                .unwrap();
                writeln!(
                    output,
                    "UPDATE {table_name} SET org_unit_id =\n\
                     (SELECT id FROM organization.org_units WHERE kind = 'root' LIMIT 1)\n\
                     WHERE org_unit_id IS NULL;"
                )
                .unwrap();
            }
            writeln!(
                output,
                "ALTER TABLE {table_name} ALTER COLUMN org_unit_id SET NOT NULL;"
            )
            .unwrap();
            let (guard_up, _) =
                crate::generators::sql::org_kind_guard_sql(table_name, rekey.org_root_shared);
            output.push_str(&guard_up);
            writeln!(output).unwrap();
            for idx in &rekey.rekeyed_indexes {
                let unique = if idx.unique { "UNIQUE " } else { "" };
                writeln!(
                    output,
                    "CREATE {unique}INDEX IF NOT EXISTS {} ON {} ({}){};",
                    idx.name,
                    table_name,
                    idx.columns.join(", "),
                    index_where_suffix(idx)
                )
                .unwrap();
            }
            writeln!(
                output,
                "DROP POLICY IF EXISTS {bare}_company_isolation ON {table_name};"
            )
            .unwrap();
            let (policy_up, _) = crate::generators::sql::org_rls_sql(
                table_name,
                &format!("{bare}_org_unit_isolation"),
            );
            output.push_str(&policy_up);
            writeln!(output).unwrap();
            writeln!(
                output,
                "ALTER TABLE {table_name} DROP COLUMN company_id;"
            )
            .unwrap();
            writeln!(output).unwrap();
        }

        // Add columns — with safe NOT NULL 3-step pattern
        for col in &change.columns_added {
            if !col.nullable && col.default.is_none() {
                writeln!(
                    output,
                    "-- Add NOT NULL column {}.{} (3-step safe pattern)",
                    table_name, col.name
                )
                .unwrap();
                writeln!(output, "-- Step 1: Add column as nullable").unwrap();
                writeln!(
                    output,
                    "ALTER TABLE {} ADD COLUMN IF NOT EXISTS {} {};",
                    table_name, col.name, col.data_type
                )
                .unwrap();
                writeln!(
                    output,
                    "-- Step 2: Backfill existing rows (adjust default value as needed)"
                )
                .unwrap();
                let backfill_default = default_value_for_type(&col.data_type);
                writeln!(
                    output,
                    "UPDATE {} SET {} = {} WHERE {} IS NULL;",
                    table_name, col.name, backfill_default, col.name
                )
                .unwrap();
                writeln!(output, "-- Step 3: Set NOT NULL constraint").unwrap();
                writeln!(
                    output,
                    "ALTER TABLE {} ALTER COLUMN {} SET NOT NULL;",
                    table_name, col.name
                )
                .unwrap();
            } else {
                writeln!(output, "-- Add column {}.{}", table_name, col.name).unwrap();
                let mut col_def = format!(
                    "ALTER TABLE {} ADD COLUMN IF NOT EXISTS {} {}",
                    table_name, col.name, col.data_type
                );
                if !col.nullable {
                    col_def.push_str(" NOT NULL");
                }
                if let Some(default) = &col.default {
                    col_def.push_str(&format!(" DEFAULT {}", default));
                }
                writeln!(output, "{};", col_def).unwrap();
            }
        }

        // A `company_id` column added to an existing table makes it newly company-scoped — install
        // the RLS fence now (ADR-0008), respecting `@global` via the snapshot's `company_scoped`
        // and the module declaration (ADR-0014) via `company_fence`.
        if change.columns_added.iter().any(|c| c.name == "company_id") {
            let newly_scoped = new_schema
                .tables
                .get(table_name)
                .map(|t| t.company_scoped && t.company_fence != Some(CompanyFence::None))
                .unwrap_or(false);
            let fence = new_schema
                .tables
                .get(table_name)
                .and_then(|t| t.company_fence)
                .unwrap_or(CompanyFence::Strict);
            if newly_scoped {
                writeln!(
                    output,
                    "-- Company RLS fence (ADR-0008): table became company-scoped"
                )
                .unwrap();
                if matches!(fence, CompanyFence::SharedTree) {
                    let (helper, _) = crate::generators::sql::company_subtree_helper_sql();
                    output.push_str(&helper);
                    writeln!(output).unwrap();
                }
                let (up, _down) = crate::generators::sql::company_rls_sql(
                    &fence,
                    table_name,
                    &format!("{table_name}_company_isolation"),
                );
                output.push_str(&up);
            }
        }

        // Posture flip on an existing fenced table (ADR-0014): drop the old policy and
        // install the new template. `company_rls_sql` re-enables RLS and DROP POLICY IF
        // EXISTS before CREATE, so flipping between fenced postures is one call; flipping
        // to `none` must instead strip the policy and disable RLS entirely.
        if let Some((old_fence, new_fence)) = &change.fence_flip {
            writeln!(
                output,
                "-- Company fence posture flip (ADR-0014): {old_fence:?} -> {new_fence:?}"
            )
            .unwrap();
            if *new_fence == CompanyFence::None {
                writeln!(
                    output,
                    "DROP POLICY IF EXISTS {table_name}_company_isolation ON {table_name};"
                )
                .unwrap();
                writeln!(
                    output,
                    "ALTER TABLE {table_name} NO FORCE ROW LEVEL SECURITY;"
                )
                .unwrap();
                writeln!(
                    output,
                    "ALTER TABLE {table_name} DISABLE ROW LEVEL SECURITY;"
                )
                .unwrap();
            } else {
                if matches!(new_fence, CompanyFence::SharedTree) && !subtree_helper_emitted {
                    let (helper, _) = crate::generators::sql::company_subtree_helper_sql();
                    output.push_str(&helper);
                    writeln!(output).unwrap();
                    subtree_helper_emitted = true;
                }
                let (up, _down) = crate::generators::sql::company_rls_sql(
                    new_fence,
                    table_name,
                    &format!("{table_name}_company_isolation"),
                );
                output.push_str(&up);
            }
            writeln!(output).unwrap();
        }

        // Modify columns — with type widening annotations
        for col_change in &change.columns_modified {
            if let (Some(old_type), Some(new_type)) = (&col_change.old_type, &col_change.new_type) {
                if is_safe_type_widening(old_type, new_type) {
                    writeln!(
                        output,
                        "-- [SAFE] Type widening: {} -> {}",
                        old_type, new_type
                    )
                    .unwrap();
                } else {
                    writeln!(
                        output,
                        "-- [REVIEW] Type change: {} -> {} (may lose data or fail on existing values)",
                        old_type, new_type
                    )
                    .unwrap();
                }
                writeln!(
                    output,
                    "ALTER TABLE {} ALTER COLUMN {} TYPE {};",
                    table_name, col_change.column_name, new_type
                )
                .unwrap();
            }

            if let Some(nullable) = col_change.nullable_changed {
                if nullable {
                    writeln!(
                        output,
                        "-- [SAFE] Allow NULLs for {}.{}",
                        table_name, col_change.column_name
                    )
                    .unwrap();
                    writeln!(
                        output,
                        "ALTER TABLE {} ALTER COLUMN {} DROP NOT NULL;",
                        table_name, col_change.column_name
                    )
                    .unwrap();
                } else {
                    writeln!(
                        output,
                        "-- [REVIEW] Setting NOT NULL on {}.{} — ensure no NULL values exist",
                        table_name, col_change.column_name
                    )
                    .unwrap();
                    writeln!(
                        output,
                        "ALTER TABLE {} ALTER COLUMN {} SET NOT NULL;",
                        table_name, col_change.column_name
                    )
                    .unwrap();
                }
            }

            if let Some(default) = &col_change.default_changed {
                writeln!(
                    output,
                    "ALTER TABLE {} ALTER COLUMN {} SET DEFAULT {};",
                    table_name, col_change.column_name, default
                )
                .unwrap();
            }
        }

        // Add indexes on existing tables — suggest CONCURRENTLY
        for idx in &change.indexes_added {
            let unique = if idx.unique { "UNIQUE " } else { "" };
            writeln!(
                output,
                "-- NOTE: Consider using CONCURRENTLY for non-blocking index creation"
            )
            .unwrap();
            writeln!(
                output,
                "-- CREATE {}INDEX CONCURRENTLY IF NOT EXISTS {} ON {} ({}){};",
                unique,
                idx.name,
                table_name,
                idx.columns.join(", "),
                index_where_suffix(idx)
            )
            .unwrap();
            writeln!(
                output,
                "CREATE {}INDEX IF NOT EXISTS {} ON {} ({}){};",
                unique,
                idx.name,
                table_name,
                idx.columns.join(", "),
                index_where_suffix(idx)
            )
            .unwrap();
        }
    }

    // Destructive changes — commented out unless --destructive flag is used
    if diff.has_destructive_changes() {
        let prefix = if destructive { "" } else { "-- " };

        writeln!(output).unwrap();
        writeln!(output, "-- ============================================").unwrap();
        if destructive {
            writeln!(output, "-- DESTRUCTIVE CHANGES (--destructive enabled)").unwrap();
        } else {
            writeln!(output, "-- DESTRUCTIVE CHANGES (commented out for safety)").unwrap();
            writeln!(
                output,
                "-- Use --destructive to uncomment, or uncomment manually after reviewing"
            )
            .unwrap();
        }
        writeln!(output, "-- ============================================").unwrap();
        writeln!(output).unwrap();

        for (table_name, change) in &diff.table_changes {
            for col_name in &change.columns_removed {
                writeln!(
                    output,
                    "{}ALTER TABLE {} DROP COLUMN {};",
                    prefix, table_name, col_name
                )
                .unwrap();
            }
        }

        for (_, change) in &diff.table_changes {
            for idx_name in &change.indexes_removed {
                writeln!(output, "{}DROP INDEX IF EXISTS {};", prefix, idx_name).unwrap();
            }
        }

        for table_name in &diff.tables_removed {
            writeln!(
                output,
                "{}DROP TABLE IF EXISTS {} CASCADE;",
                prefix, table_name
            )
            .unwrap();
        }

        for enum_name in &diff.enums_removed {
            writeln!(
                output,
                "{}DROP TYPE IF EXISTS {} CASCADE;",
                prefix, enum_name
            )
            .unwrap();
        }
    }

    output
}

/// Generate DOWN (rollback) migration SQL only.
pub fn generate_down_migration(diff: &SchemaDiff) -> String {
    let mut output = String::new();

    for table_name in &diff.tables_added {
        writeln!(output, "DROP TABLE IF EXISTS {} CASCADE;", table_name).unwrap();
    }

    for (table_name, change) in &diff.table_changes {
        // Reverse a company→org re-key (ADR-0028): drop the org fence artifacts, then
        // rebuild company_id by walking each row's node up the org tree to its
        // company-kind ancestor. Best-effort by design — rows whose chain has no
        // company ancestor fail loudly rather than being silently coerced to NULLs
        // past the NOT NULL below. The old company policy and indexes are recreated
        // from the stashed pre-re-key definitions.
        if let Some(rekey) = &change.org_rekey {
            let bare = table_name.rsplit('.').next().unwrap_or(table_name);
            writeln!(
                output,
                "-- Reverse the org re-key on {table_name} (ADR-0028)"
            )
            .unwrap();
            writeln!(
                output,
                "DROP POLICY IF EXISTS {bare}_org_unit_isolation ON {table_name};"
            )
            .unwrap();
            let (_guard_up, guard_down) =
                crate::generators::sql::org_kind_guard_sql(table_name, rekey.org_root_shared);
            output.push_str(&guard_down);
            for idx in &rekey.rekeyed_indexes {
                writeln!(output, "DROP INDEX IF EXISTS {};", idx.name).unwrap();
            }
            writeln!(
                output,
                "ALTER TABLE {table_name} ADD COLUMN IF NOT EXISTS company_id UUID;"
            )
            .unwrap();
            writeln!(
                output,
                "UPDATE {table_name} w SET company_id = (\n\
                 \x20   WITH RECURSIVE up AS (\n\
                 \x20       SELECT o.id, o.parent_id, o.kind::text AS kind\n\
                 \x20       FROM organization.org_units o WHERE o.id = w.org_unit_id\n\
                 \x20       UNION ALL\n\
                 \x20       SELECT p.id, p.parent_id, p.kind::text\n\
                 \x20       FROM organization.org_units p JOIN up u ON p.id = u.parent_id\n\
                 \x20   )\n\
                 \x20   SELECT up.id FROM up WHERE up.kind = 'company' LIMIT 1\n\
                 );"
            )
            .unwrap();
            writeln!(
                output,
                "DO $$\n\
                 BEGIN\n\
                 \x20   IF EXISTS (SELECT 1 FROM {table_name} WHERE company_id IS NULL) THEN\n\
                 \x20       RAISE EXCEPTION '{bare} rows exist whose org_unit chain has no company ancestor; cannot reverse the re-key';\n\
                 \x20   END IF;\n\
                 END $$;"
            )
            .unwrap();
            writeln!(
                output,
                "ALTER TABLE {table_name} ALTER COLUMN company_id SET NOT NULL;"
            )
            .unwrap();
            writeln!(
                output,
                "ALTER TABLE {table_name} DROP COLUMN org_unit_id;"
            )
            .unwrap();
            for idx in &rekey.replaced_indexes {
                let unique = if idx.unique { "UNIQUE " } else { "" };
                writeln!(
                    output,
                    "CREATE {unique}INDEX IF NOT EXISTS {} ON {} ({}){};",
                    idx.name,
                    table_name,
                    idx.columns.join(", "),
                    index_where_suffix(idx)
                )
                .unwrap();
            }
            if rekey.old_fence != CompanyFence::None {
                if matches!(rekey.old_fence, CompanyFence::SharedTree) {
                    let (helper, _) = crate::generators::sql::company_subtree_helper_sql();
                    output.push_str(&helper);
                }
                let (up, _down) = crate::generators::sql::company_rls_sql(
                    &rekey.old_fence,
                    table_name,
                    &format!("{bare}_company_isolation"),
                );
                output.push_str(&up);
            }
            writeln!(output).unwrap();
        }

        for col in &change.columns_added {
            writeln!(
                output,
                "ALTER TABLE {} DROP COLUMN IF EXISTS {};",
                table_name, col.name
            )
            .unwrap();
        }

        // Reverse a posture flip: strip whatever the flip installed, restore the old
        // template. Flipping back to an unfenced posture (old = None) just strips.
        if let Some((old_fence, _new_fence)) = &change.fence_flip {
            writeln!(
                output,
                "DROP POLICY IF EXISTS {table_name}_company_isolation ON {table_name};"
            )
            .unwrap();
            if *old_fence != CompanyFence::None {
                if matches!(old_fence, CompanyFence::SharedTree) {
                    let (helper, _) = crate::generators::sql::company_subtree_helper_sql();
                    output.push_str(&helper);
                }
                let (up, _down) = crate::generators::sql::company_rls_sql(
                    old_fence,
                    table_name,
                    &format!("{table_name}_company_isolation"),
                );
                output.push_str(&up);
            }
        }
    }

    for enum_name in &diff.enums_added {
        writeln!(output, "DROP TYPE IF EXISTS {} CASCADE;", enum_name).unwrap();
    }

    output
}

/// Get a sensible default value for a SQL type (used in safe NOT NULL backfill).
fn default_value_for_type(sql_type: &str) -> &'static str {
    let upper = sql_type.to_uppercase();

    // Handle parameterized types first (VARCHAR(N), DECIMAL(P,S), etc.)
    if upper.starts_with("VARCHAR") || upper.starts_with("CHAR") {
        return "''";
    }
    if upper.starts_with("DECIMAL") || upper.starts_with("NUMERIC") {
        return "0";
    }

    match upper.as_str() {
        "TEXT" => "''",
        "INTEGER" | "BIGINT" | "SMALLINT" | "REAL" | "DOUBLE PRECISION" => "0",
        "BOOLEAN" => "false",
        "UUID" => "gen_random_uuid()",
        "TIMESTAMPTZ" | "TIMESTAMP" => "NOW()",
        "DATE" => "CURRENT_DATE",
        "TIME" | "TIMETZ" => "CURRENT_TIME",
        "JSONB" | "JSON" => "'{}'::jsonb",
        "BYTEA" => "'\\x'::bytea",
        "INET" => "'0.0.0.0'::inet",
        "INTERVAL" => "'0'::interval",
        _ => "NULL", // fallback — user must adjust
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_empty_schemas() {
        let old = SchemaSnapshot::default();
        let new = SchemaSnapshot::default();
        let diff = diff_schemas(&old, &new);
        assert!(!diff.has_changes());
    }

    #[test]
    fn test_diff_new_table() {
        let old = SchemaSnapshot::default();
        let mut new = SchemaSnapshot::default();
        new.tables.insert(
            "users".to_string(),
            TableSnapshot {
                name: "users".to_string(),
                columns: IndexMap::new(),
                indexes: IndexMap::new(),
                primary_key: None,
                company_scoped: false,
                company_fence: None,
                org_scoped: false,
                org_root_shared: false,
                org_fence: None,
            },
        );

        let diff = diff_schemas(&old, &new);
        assert!(diff.has_changes());
        assert_eq!(diff.tables_added, vec!["users"]);
    }

    fn company_id_col() -> ColumnSnapshot {
        ColumnSnapshot {
            name: "company_id".to_string(),
            data_type: "UUID".to_string(),
            nullable: false,
            default: None,
            is_unique: false,
        }
    }

    #[test]
    fn diff_up_fences_a_new_company_scoped_table() {
        // The gap this closes: a company-scoped table added via the incremental (diff) path must
        // ship WITH its RLS fence, exactly as the full-regen path emits it.
        let old = SchemaSnapshot::default();
        let mut new = SchemaSnapshot::default();
        let mut cols = IndexMap::new();
        cols.insert("company_id".to_string(), company_id_col());
        new.tables.insert(
            "invoices".to_string(),
            TableSnapshot {
                name: "invoices".to_string(),
                columns: cols,
                indexes: IndexMap::new(),
                primary_key: Some("id".to_string()),
                company_scoped: true,
                company_fence: None,
                org_scoped: false,
                org_root_shared: false,
                org_fence: None,
            },
        );
        let diff = diff_schemas(&old, &new);
        let up = generate_up_migration(&diff, &new, false);
        assert!(
            up.contains("ENABLE ROW LEVEL SECURITY"),
            "must enable RLS:\n{up}"
        );
        assert!(
            up.contains("FORCE  ROW LEVEL SECURITY"),
            "must FORCE RLS:\n{up}"
        );
        assert!(
            up.contains("NULLIF(current_setting('app.company_id', true), '')::uuid"),
            "must use the fail-closed fence:\n{up}"
        );
        assert!(
            up.contains("invoices_company_isolation"),
            "policy named per table:\n{up}"
        );
    }

    #[test]
    fn diff_up_honors_the_module_fence_declaration() {
        // ADR-0014 on the incremental path: the snapshot's company_fence selects
        // the template; a pre-declaration snapshot (None) means strict — the
        // legacy bytes above are the proof for that arm.
        let old = SchemaSnapshot::default();
        let mut cols = IndexMap::new();
        cols.insert("company_id".to_string(), company_id_col());

        let mut shared_blank = SchemaSnapshot::default();
        shared_blank.tables.insert(
            "partners".to_string(),
            TableSnapshot {
                name: "partners".to_string(),
                columns: cols.clone(),
                indexes: IndexMap::new(),
                primary_key: Some("id".to_string()),
                company_scoped: true,
                company_fence: Some(CompanyFence::SharedBlank),
                org_scoped: false,
                org_root_shared: false,
                org_fence: None,
            },
        );
        let up = generate_up_migration(&diff_schemas(&old, &shared_blank), &shared_blank, false);
        assert!(
            up.contains("OR company_id IS NULL"),
            "shared_blank must add the shared-NULL arm:\n{up}"
        );

        let mut shared_tree = SchemaSnapshot::default();
        shared_tree.tables.insert(
            "branches".to_string(),
            TableSnapshot {
                name: "branches".to_string(),
                columns: cols,
                indexes: IndexMap::new(),
                primary_key: Some("id".to_string()),
                company_scoped: true,
                company_fence: Some(CompanyFence::SharedTree),
                org_scoped: false,
                org_root_shared: false,
                org_fence: None,
            },
        );
        let up = generate_up_migration(&diff_schemas(&old, &shared_tree), &shared_tree, false);
        assert!(
            up.contains("organization.company_subtree("),
            "shared_tree must read the subtree helper:\n{up}"
        );
        assert!(
            up.contains("CREATE OR REPLACE FUNCTION organization.company_subtree"),
            "the diff path must ship the helper with the policy:\n{up}"
        );

        let mut unfenced = SchemaSnapshot::default();
        unfenced.tables.insert(
            "messages".to_string(),
            TableSnapshot {
                name: "messages".to_string(),
                columns: IndexMap::new(),
                indexes: IndexMap::new(),
                primary_key: Some("id".to_string()),
                company_scoped: false, // none-fence modules validate to this shape
                company_fence: Some(CompanyFence::None),
                org_scoped: false,
                org_root_shared: false,
                org_fence: None,
            },
        );
        let up = generate_up_migration(&diff_schemas(&old, &unfenced), &unfenced, false);
        assert!(
            !up.contains("ROW LEVEL SECURITY"),
            "a none-fence module must not be fenced:\n{up}"
        );
    }

    #[test]
    fn diff_up_leaves_an_unscoped_new_table_unfenced() {
        let old = SchemaSnapshot::default();
        let mut new = SchemaSnapshot::default();
        new.tables.insert(
            "currencies".to_string(),
            TableSnapshot {
                name: "currencies".to_string(),
                columns: IndexMap::new(),
                indexes: IndexMap::new(),
                primary_key: Some("id".to_string()),
                company_scoped: false,
                company_fence: None, // reference data / @global — no fence
                org_scoped: false,
                org_root_shared: false,
                org_fence: None,
            },
        );
        let diff = diff_schemas(&old, &new);
        let up = generate_up_migration(&diff, &new, false);
        assert!(
            !up.contains("ROW LEVEL SECURITY"),
            "an unscoped table must not be fenced:\n{up}"
        );
    }

    #[test]
    fn diff_up_fences_a_table_that_gains_company_id() {
        // Adding `company_id` to an existing table makes it newly company-scoped → fence it now.
        let mut old_cols = IndexMap::new();
        old_cols.insert(
            "id".to_string(),
            ColumnSnapshot {
                name: "id".to_string(),
                data_type: "UUID".to_string(),
                nullable: false,
                default: None,
                is_unique: false,
            },
        );
        let mut old = SchemaSnapshot::default();
        old.tables.insert(
            "orders".to_string(),
            TableSnapshot {
                name: "orders".to_string(),
                columns: old_cols.clone(),
                indexes: IndexMap::new(),
                primary_key: Some("id".to_string()),
                company_scoped: false,
                company_fence: None,
                org_scoped: false,
                org_root_shared: false,
                org_fence: None,
            },
        );
        let mut new_cols = old_cols.clone();
        new_cols.insert("company_id".to_string(), company_id_col());
        let mut new = SchemaSnapshot::default();
        new.tables.insert(
            "orders".to_string(),
            TableSnapshot {
                name: "orders".to_string(),
                columns: new_cols,
                indexes: IndexMap::new(),
                primary_key: Some("id".to_string()),
                company_scoped: true,
                company_fence: None,
                org_scoped: false,
                org_root_shared: false,
                org_fence: None,
            },
        );
        let diff = diff_schemas(&old, &new);
        let up = generate_up_migration(&diff, &new, false);
        assert!(
            up.contains("orders_company_isolation"),
            "must fence a table that gains company_id:\n{up}"
        );
    }

    #[test]
    fn diff_detects_a_posture_flip_with_no_column_changes() {
        // The gap this closes: identical columns, different declaration → the incremental
        // path must still see a change (previously the flip was invisible and the old
        // policy template silently stayed in place).
        let mut cols = IndexMap::new();
        cols.insert("company_id".to_string(), company_id_col());
        let snapshot = |fence: Option<CompanyFence>| {
            let mut s = SchemaSnapshot::default();
            s.tables.insert(
                "branches".to_string(),
                TableSnapshot {
                    name: "branches".to_string(),
                    columns: cols.clone(),
                    indexes: IndexMap::new(),
                    primary_key: Some("id".to_string()),
                    company_scoped: true,
                    company_fence: fence,
                    org_scoped: false,
                    org_root_shared: false,
                    org_fence: None,
                },
            );
            s
        };
        // Legacy snapshot (undeclared) is effectively strict; declare shared_tree now.
        let diff = diff_schemas(&snapshot(None), &snapshot(Some(CompanyFence::SharedTree)));
        assert!(
            diff.table_changes.contains_key("branches"),
            "a posture flip alone must register as a table change"
        );
        assert_eq!(
            diff.table_changes["branches"].fence_flip,
            Some((CompanyFence::Strict, CompanyFence::SharedTree))
        );
    }

    #[test]
    fn diff_up_re_emits_the_policy_on_a_flip_to_shared_blank() {
        let mut cols = IndexMap::new();
        cols.insert("company_id".to_string(), company_id_col());
        let snapshot = |fence: CompanyFence| {
            let mut s = SchemaSnapshot::default();
            s.tables.insert(
                "partners".to_string(),
                TableSnapshot {
                    name: "partners".to_string(),
                    columns: cols.clone(),
                    indexes: IndexMap::new(),
                    primary_key: Some("id".to_string()),
                    company_scoped: true,
                    company_fence: Some(fence),
                    org_scoped: false,
                    org_root_shared: false,
                    org_fence: None,
                },
            );
            s
        };
        let old = snapshot(CompanyFence::Strict);
        let new = snapshot(CompanyFence::SharedBlank);
        let diff = diff_schemas(&old, &new);
        let up = generate_up_migration(&diff, &new, false);
        assert!(
            up.contains("posture flip (ADR-0014): Strict -> SharedBlank"),
            "flip banner:\n{up}"
        );
        assert!(
            up.contains("OR company_id IS NULL"),
            "new template must be the shared_blank one:\n{up}"
        );
        let down = generate_down_migration(&diff);
        assert!(
            down.contains("DROP POLICY IF EXISTS partners_company_isolation"),
            "rollback strips the flipped-in policy:\n{down}"
        );
    }

    #[test]
    fn diff_up_ships_the_subtree_helper_once_on_a_flip_to_shared_tree() {
        let mut cols = IndexMap::new();
        cols.insert("company_id".to_string(), company_id_col());
        let snapshot = |fence: CompanyFence| {
            let mut s = SchemaSnapshot::default();
            for name in ["branches", "departments"] {
                s.tables.insert(
                    name.to_string(),
                    TableSnapshot {
                        name: name.to_string(),
                        columns: cols.clone(),
                        indexes: IndexMap::new(),
                        primary_key: Some("id".to_string()),
                        company_scoped: true,
                        company_fence: Some(fence),
                        org_scoped: false,
                        org_root_shared: false,
                        org_fence: None,
                    },
                );
            }
            s
        };
        let old = snapshot(CompanyFence::Strict);
        let new = snapshot(CompanyFence::SharedTree);
        let diff = diff_schemas(&old, &new);
        let up = generate_up_migration(&diff, &new, false);
        assert!(
            up.contains("organization.company_subtree("),
            "shared_tree policy must read the helper:\n{up}"
        );
        assert_eq!(
            up.matches("CREATE OR REPLACE FUNCTION organization.company_subtree")
                .count(),
            1,
            "helper must appear exactly once even with two flipped tables:\n{up}"
        );
        assert_eq!(
            up.matches("branches_company_isolation").count() >= 1
                && up.matches("departments_company_isolation").count() >= 1,
            true,
            "each flipped table gets its policy:\n{up}"
        );
    }

    #[test]
    fn diff_up_strips_the_fence_on_a_flip_to_none() {
        let mut cols = IndexMap::new();
        cols.insert("company_id".to_string(), company_id_col());
        let snapshot = |fence: CompanyFence| {
            let mut s = SchemaSnapshot::default();
            s.tables.insert(
                "notes".to_string(),
                TableSnapshot {
                    name: "notes".to_string(),
                    columns: cols.clone(),
                    indexes: IndexMap::new(),
                    primary_key: Some("id".to_string()),
                    company_scoped: true,
                    company_fence: Some(fence),
                    org_scoped: false,
                    org_root_shared: false,
                    org_fence: None,
                },
            );
            s
        };
        let old = snapshot(CompanyFence::Strict);
        let new = snapshot(CompanyFence::None);
        let diff = diff_schemas(&old, &new);
        let up = generate_up_migration(&diff, &new, false);
        assert!(
            up.contains("DROP POLICY IF EXISTS notes_company_isolation"),
            "flip to none must drop the policy:\n{up}"
        );
        assert!(
            up.contains("ALTER TABLE notes DISABLE ROW LEVEL SECURITY;"),
            "flip to none must disable RLS:\n{up}"
        );
        assert!(
            !up.contains("CREATE POLICY"),
            "flip to none must not re-create a policy:\n{up}"
        );
    }

    #[test]
    fn test_diff_new_column() {
        let mut old_columns = IndexMap::new();
        old_columns.insert(
            "id".to_string(),
            ColumnSnapshot {
                name: "id".to_string(),
                data_type: "UUID".to_string(),
                nullable: false,
                default: None,
                is_unique: false,
            },
        );

        let mut new_columns = old_columns.clone();
        new_columns.insert(
            "email".to_string(),
            ColumnSnapshot {
                name: "email".to_string(),
                data_type: "VARCHAR(255)".to_string(),
                nullable: false,
                default: None,
                is_unique: true,
            },
        );

        let mut old = SchemaSnapshot::default();
        old.tables.insert(
            "users".to_string(),
            TableSnapshot {
                name: "users".to_string(),
                columns: old_columns,
                indexes: IndexMap::new(),
                primary_key: Some("id".to_string()),
                company_scoped: false,
                company_fence: None,
                org_scoped: false,
                org_root_shared: false,
                org_fence: None,
            },
        );

        let mut new = SchemaSnapshot::default();
        new.tables.insert(
            "users".to_string(),
            TableSnapshot {
                name: "users".to_string(),
                columns: new_columns,
                indexes: IndexMap::new(),
                primary_key: Some("id".to_string()),
                company_scoped: false,
                company_fence: None,
                org_scoped: false,
                org_root_shared: false,
                org_fence: None,
            },
        );

        let diff = diff_schemas(&old, &new);
        assert!(diff.has_changes());
        assert!(diff.table_changes.contains_key("users"));
        assert_eq!(diff.table_changes["users"].columns_added.len(), 1);
        assert_eq!(diff.table_changes["users"].columns_added[0].name, "email");
    }

    #[test]
    fn test_destructive_changes() {
        let mut old = SchemaSnapshot::default();
        old.tables.insert(
            "users".to_string(),
            TableSnapshot {
                name: "users".to_string(),
                columns: IndexMap::new(),
                indexes: IndexMap::new(),
                primary_key: None,
                company_scoped: false,
                company_fence: None,
                org_scoped: false,
                org_root_shared: false,
                org_fence: None,
            },
        );

        let new = SchemaSnapshot::default();

        let diff = diff_schemas(&old, &new);
        assert!(diff.has_destructive_changes());
        assert_eq!(diff.tables_removed, vec!["users"]);
    }

    #[test]
    fn test_rename_candidate_detected() {
        let mut old_columns = IndexMap::new();
        old_columns.insert(
            "id".to_string(),
            ColumnSnapshot {
                name: "id".to_string(),
                data_type: "UUID".to_string(),
                nullable: false,
                default: None,
                is_unique: false,
            },
        );
        old_columns.insert(
            "first_name".to_string(),
            ColumnSnapshot {
                name: "first_name".to_string(),
                data_type: "VARCHAR(255)".to_string(),
                nullable: false,
                default: None,
                is_unique: false,
            },
        );

        let mut new_columns = IndexMap::new();
        new_columns.insert(
            "id".to_string(),
            ColumnSnapshot {
                name: "id".to_string(),
                data_type: "UUID".to_string(),
                nullable: false,
                default: None,
                is_unique: false,
            },
        );
        new_columns.insert(
            "full_name".to_string(),
            ColumnSnapshot {
                name: "full_name".to_string(),
                data_type: "VARCHAR(255)".to_string(),
                nullable: false,
                default: None,
                is_unique: false,
            },
        );

        let mut old = SchemaSnapshot::default();
        old.tables.insert(
            "users".to_string(),
            TableSnapshot {
                name: "users".to_string(),
                columns: old_columns,
                indexes: IndexMap::new(),
                primary_key: Some("id".to_string()),
                company_scoped: false,
                company_fence: None,
                org_scoped: false,
                org_root_shared: false,
                org_fence: None,
            },
        );

        let mut new = SchemaSnapshot::default();
        new.tables.insert(
            "users".to_string(),
            TableSnapshot {
                name: "users".to_string(),
                columns: new_columns,
                indexes: IndexMap::new(),
                primary_key: Some("id".to_string()),
                company_scoped: false,
                company_fence: None,
                org_scoped: false,
                org_root_shared: false,
                org_fence: None,
            },
        );

        let diff = diff_schemas(&old, &new);
        let change = &diff.table_changes["users"];

        // Should detect rename candidate: first_name -> full_name (same VARCHAR(255) type)
        assert_eq!(change.rename_candidates.len(), 1);
        assert_eq!(change.rename_candidates[0].old_name, "first_name");
        assert_eq!(change.rename_candidates[0].new_name, "full_name");
        assert_eq!(change.rename_candidates[0].data_type, "VARCHAR(255)");
    }

    #[test]
    fn test_no_rename_candidate_different_types() {
        let mut old_columns = IndexMap::new();
        old_columns.insert(
            "age".to_string(),
            ColumnSnapshot {
                name: "age".to_string(),
                data_type: "INTEGER".to_string(),
                nullable: false,
                default: None,
                is_unique: false,
            },
        );

        let mut new_columns = IndexMap::new();
        new_columns.insert(
            "bio".to_string(),
            ColumnSnapshot {
                name: "bio".to_string(),
                data_type: "TEXT".to_string(),
                nullable: true,
                default: None,
                is_unique: false,
            },
        );

        let mut old = SchemaSnapshot::default();
        old.tables.insert(
            "users".to_string(),
            TableSnapshot {
                name: "users".to_string(),
                columns: old_columns,
                indexes: IndexMap::new(),
                primary_key: None,
                company_scoped: false,
                company_fence: None,
                org_scoped: false,
                org_root_shared: false,
                org_fence: None,
            },
        );

        let mut new = SchemaSnapshot::default();
        new.tables.insert(
            "users".to_string(),
            TableSnapshot {
                name: "users".to_string(),
                columns: new_columns,
                indexes: IndexMap::new(),
                primary_key: None,
                company_scoped: false,
                company_fence: None,
                org_scoped: false,
                org_root_shared: false,
                org_fence: None,
            },
        );

        let diff = diff_schemas(&old, &new);
        let change = &diff.table_changes["users"];

        // Different types — no rename candidate
        assert!(change.rename_candidates.is_empty());
    }

    #[test]
    fn test_generate_up_migration_contains_alter() {
        let mut table_changes = IndexMap::new();
        table_changes.insert(
            "orders".to_string(),
            TableChange {
                table_name: "orders".to_string(),
                columns_added: vec![ColumnSnapshot {
                    name: "status".to_string(),
                    data_type: "VARCHAR(50)".to_string(),
                    nullable: true,
                    default: None,
                    is_unique: false,
                }],
                ..Default::default()
            },
        );

        let diff = SchemaDiff {
            table_changes,
            ..Default::default()
        };
        let schema = SchemaSnapshot::default();

        let up = generate_up_migration(&diff, &schema, false);
        assert!(up.contains("ALTER TABLE orders ADD COLUMN IF NOT EXISTS status VARCHAR(50)"));
    }

    #[test]
    fn test_generate_down_migration_drops_added() {
        let mut table_changes = IndexMap::new();
        table_changes.insert(
            "orders".to_string(),
            TableChange {
                table_name: "orders".to_string(),
                columns_added: vec![ColumnSnapshot {
                    name: "status".to_string(),
                    data_type: "VARCHAR(50)".to_string(),
                    nullable: true,
                    default: None,
                    is_unique: false,
                }],
                ..Default::default()
            },
        );

        let diff = SchemaDiff {
            table_changes,
            tables_added: vec!["new_table".to_string()],
            ..Default::default()
        };

        let down = generate_down_migration(&diff);
        assert!(down.contains("DROP TABLE IF EXISTS new_table CASCADE"));
        assert!(down.contains("ALTER TABLE orders DROP COLUMN IF EXISTS status"));
    }

    #[test]
    fn test_generate_up_safe_not_null_pattern() {
        let mut table_changes = IndexMap::new();
        table_changes.insert(
            "users".to_string(),
            TableChange {
                table_name: "users".to_string(),
                columns_added: vec![ColumnSnapshot {
                    name: "email".to_string(),
                    data_type: "VARCHAR(255)".to_string(),
                    nullable: false,
                    default: None,
                    is_unique: false,
                }],
                ..Default::default()
            },
        );

        let diff = SchemaDiff {
            table_changes,
            ..Default::default()
        };
        let schema = SchemaSnapshot::default();

        let up = generate_up_migration(&diff, &schema, false);
        // Should use 3-step safe pattern
        assert!(up.contains("Step 1: Add column as nullable"));
        assert!(up.contains("Step 2: Backfill existing rows"));
        assert!(up.contains("Step 3: Set NOT NULL constraint"));
        assert!(up.contains("ALTER TABLE users ADD COLUMN IF NOT EXISTS email VARCHAR(255);"));
        assert!(up.contains("UPDATE users SET email = '' WHERE email IS NULL;"));
        assert!(up.contains("ALTER TABLE users ALTER COLUMN email SET NOT NULL;"));
    }

    #[test]
    fn test_default_value_for_type() {
        assert_eq!(default_value_for_type("VARCHAR(255)"), "''");
        assert_eq!(default_value_for_type("TEXT"), "''");
        assert_eq!(default_value_for_type("INTEGER"), "0");
        assert_eq!(default_value_for_type("BIGINT"), "0");
        assert_eq!(default_value_for_type("BOOLEAN"), "false");
        assert_eq!(default_value_for_type("UUID"), "gen_random_uuid()");
        assert_eq!(default_value_for_type("TIMESTAMPTZ"), "NOW()");
        assert_eq!(default_value_for_type("JSONB"), "'{}'::jsonb");
        assert_eq!(default_value_for_type("DECIMAL(19, 4)"), "0");
        assert_eq!(default_value_for_type("UNKNOWN_TYPE"), "NULL");
    }

    #[test]
    fn test_rename_candidate_in_up_migration() {
        let mut table_changes = IndexMap::new();
        let mut change = TableChange {
            table_name: "users".to_string(),
            columns_added: vec![ColumnSnapshot {
                name: "full_name".to_string(),
                data_type: "VARCHAR(255)".to_string(),
                nullable: false,
                default: None,
                is_unique: false,
            }],
            columns_removed: vec!["first_name".to_string()],
            ..Default::default()
        };
        change.rename_candidates.push(RenameCandidate {
            old_name: "first_name".to_string(),
            new_name: "full_name".to_string(),
            data_type: "VARCHAR(255)".to_string(),
        });
        table_changes.insert("users".to_string(), change);

        let diff = SchemaDiff {
            table_changes,
            ..Default::default()
        };
        let schema = SchemaSnapshot::default();

        let up = generate_up_migration(&diff, &schema, false);
        assert!(up.contains("POSSIBLE RENAMES"));
        assert!(up.contains("RENAME COLUMN first_name TO full_name"));
    }

    // ── company→org re-key (ADR-0028) ───────────────────────────────────────────

    fn rekey_col(name: &str, nullable: bool) -> ColumnSnapshot {
        ColumnSnapshot {
            name: name.to_string(),
            data_type: "UUID".to_string(),
            nullable,
            default: None,
            is_unique: false,
        }
    }

    fn rekey_text_col(name: &str) -> ColumnSnapshot {
        ColumnSnapshot {
            name: name.to_string(),
            data_type: "TEXT".to_string(),
            nullable: false,
            default: None,
            is_unique: false,
        }
    }

    fn rekey_index(cols: &[&str], unique: bool, name: &str) -> IndexSnapshot {
        IndexSnapshot {
            name: name.to_string(),
            columns: cols.iter().map(|c| c.to_string()).collect(),
            unique,
            index_type: if unique { "unique" } else { "btree" }.to_string(),
            where_predicate: None,
        }
    }

    /// A company-fenced table (strict, unique `(company_id, code)`) becoming an
    /// org-scoped table (`org_unit_id`, unique `(org_unit_id, code)`) — the
    /// inventory warehouses pilot shape.
    fn rekey_pair() -> (SchemaSnapshot, SchemaSnapshot) {
        let mut old_cols = IndexMap::new();
        old_cols.insert("id".to_string(), rekey_col("id", false));
        old_cols.insert("company_id".to_string(), rekey_col("company_id", false));
        old_cols.insert("code".to_string(), rekey_text_col("code"));
        let mut old_idx = IndexMap::new();
        old_idx.insert(
            "idx_warehouses_company_id_code".to_string(),
            rekey_index(&["company_id", "code"], true, "idx_warehouses_company_id_code"),
        );
        let old = SchemaSnapshot {
            tables: IndexMap::from([(
                "warehouses".to_string(),
                TableSnapshot {
                    name: "warehouses".to_string(),
                    columns: old_cols,
                    indexes: old_idx,
                    primary_key: Some("id".to_string()),
                    company_scoped: true,
                    company_fence: Some(CompanyFence::Strict),
                    org_scoped: false,
                    org_root_shared: false,
                    org_fence: None,
                },
            )]),
            ..Default::default()
        };

        let mut new_cols = IndexMap::new();
        new_cols.insert("id".to_string(), rekey_col("id", false));
        new_cols.insert("org_unit_id".to_string(), rekey_col("org_unit_id", false));
        new_cols.insert("code".to_string(), rekey_text_col("code"));
        let mut new_idx = IndexMap::new();
        new_idx.insert(
            "idx_warehouses_org_unit_id_code".to_string(),
            rekey_index(&["org_unit_id", "code"], true, "idx_warehouses_org_unit_id_code"),
        );
        let new = SchemaSnapshot {
            tables: IndexMap::from([(
                "warehouses".to_string(),
                TableSnapshot {
                    name: "warehouses".to_string(),
                    columns: new_cols,
                    indexes: new_idx,
                    primary_key: Some("id".to_string()),
                    company_scoped: false,
                    company_fence: Some(CompanyFence::None),
                    org_scoped: true,
                    org_root_shared: false,
                    org_fence: Some(OrgFence::Strict),
                },
            )]),
            ..Default::default()
        };
        (old, new)
    }

    #[test]
    fn rekey_is_detected_and_the_naive_paths_suppressed() {
        let (old, new) = rekey_pair();
        let diff = diff_schemas(&old, &new);
        let change = &diff.table_changes["warehouses"];
        let rekey = change.org_rekey.as_ref().expect("the column pair is a re-key");
        assert_eq!(rekey.old_fence, CompanyFence::Strict);
        assert!(
            change.columns_removed.is_empty(),
            "company_id leaves the naive drop path — the re-key owns its removal"
        );
        assert!(
            change.columns_added.iter().all(|c| c.name != "org_unit_id"),
            "org_unit_id leaves the naive add path — the re-key owns its addition"
        );
        assert!(
            change.fence_flip.is_none(),
            "a flip-to-none record would DISABLE RLS mid-re-key"
        );
        assert!(
            change.rename_candidates.is_empty(),
            "RENAME COLUMN would skip the backfill and the NOT NULL step entirely"
        );
        assert!(
            change.indexes_added.is_empty() && change.indexes_removed.is_empty(),
            "both index generations ride the ordered re-key emission"
        );
        assert_eq!(rekey.rekeyed_indexes.len(), 1, "the (org_unit_id, code) unique");
        assert_eq!(rekey.replaced_indexes.len(), 1, "the (company_id, code) unique");
    }

    /// The emitted order matches the hand-proven pilot migration: add nullable →
    /// backfill → NOT NULL → kind guard → re-keyed indexes → policy swap → drop the
    /// old column. RLS is never disabled — the fence is swapped, not lifted.
    #[test]
    fn rekey_up_emits_the_pilot_order() {
        let (old, new) = rekey_pair();
        let diff = diff_schemas(&old, &new);
        let up = generate_up_migration(&diff, &new, false);
        let pos = |needle: &str| {
            up.find(needle)
                .unwrap_or_else(|| panic!("missing `{needle}` in:\n{up}"))
        };
        let add = pos("ADD COLUMN IF NOT EXISTS org_unit_id UUID");
        let backfill = pos("SET org_unit_id = company_id");
        let not_null = pos("ALTER COLUMN org_unit_id SET NOT NULL");
        let guard = pos("warehouses_org_unit_kind_guard");
        let index = pos("CREATE UNIQUE INDEX IF NOT EXISTS idx_warehouses_org_unit_id_code");
        let drop_old_policy = pos("DROP POLICY IF EXISTS warehouses_company_isolation");
        let new_policy = pos("CREATE POLICY warehouses_org_unit_isolation");
        let drop_col = pos("DROP COLUMN company_id");
        assert!(add < backfill, "add before backfill");
        assert!(backfill < not_null, "backfill before NOT NULL");
        assert!(not_null < guard, "NOT NULL before the kind guard");
        assert!(guard < index, "kind guard before the re-keyed indexes");
        assert!(index < drop_old_policy, "indexes before the policy swap");
        assert!(drop_old_policy < new_policy, "old policy drops before the org policy lands");
        assert!(new_policy < drop_col, "the org policy is live before the old column drops");
        assert!(
            !up.contains("DISABLE ROW LEVEL SECURITY") && !up.contains("NO FORCE"),
            "RLS stays enabled and forced throughout:\n{up}"
        );
        assert!(
            up.contains("NOT IN ('company', 'branch')"),
            "default kind guard:\n{up}"
        );
        assert!(
            up.contains("org_unit_id = ANY(string_to_array(current_setting('app.scope_unit_ids', true), ',')::uuid[])"),
            "the proven entitlement-union predicate:\n{up}"
        );
    }

    /// A `shared_blank` source anchors its shared (NULL company) rows on the root
    /// node instead of copying NULLs forward.
    #[test]
    fn rekey_from_shared_blank_anchors_shared_rows_on_the_root() {
        let (mut old, new) = rekey_pair();
        old.tables
            .values_mut()
            .for_each(|t| {
                t.company_fence = Some(CompanyFence::SharedBlank);
                t.columns.get_mut("company_id").unwrap().nullable = true;
            });
        let diff = diff_schemas(&old, &new);
        let change = &diff.table_changes["warehouses"];
        let rekey = change.org_rekey.as_ref().expect("still a re-key");
        assert_eq!(rekey.old_fence, CompanyFence::SharedBlank);
        let up = generate_up_migration(&diff, &new, false);
        let anchor = pos_of(&up, "kind = 'root'");
        let not_null = pos_of(&up, "ALTER COLUMN org_unit_id SET NOT NULL");
        assert!(
            anchor < not_null,
            "shared rows must anchor on the root before NOT NULL:\n{up}"
        );
        // ...and a strict source never emits the anchor.
        let (old_strict, new_strict) = rekey_pair();
        let strict_up = generate_up_migration(&diff_schemas(&old_strict, &new_strict), &new_strict, false);
        assert!(
            !strict_up.contains("kind = 'root'"),
            "strict sources copy only real company rows:\n{strict_up}"
        );
    }

    fn pos_of(haystack: &str, needle: &str) -> usize {
        haystack
            .find(needle)
            .unwrap_or_else(|| panic!("missing `{needle}` in:\n{haystack}"))
    }

    /// The down path walks the org tree back to the company ancestor, fails loudly
    /// on unmappable rows, and restores the old company fence + indexes.
    #[test]
    fn rekey_down_walks_back_and_restores_the_company_fence() {
        let (old, new) = rekey_pair();
        let diff = diff_schemas(&old, &new);
        let down = generate_down_migration(&diff);
        assert!(
            down.contains("WITH RECURSIVE"),
            "company_id is rebuilt by walking the org tree:\n{down}"
        );
        assert!(
            down.contains("RAISE EXCEPTION"),
            "unmappable rows fail loudly, never silently NULL:\n{down}"
        );
        assert!(
            down.contains("DROP COLUMN org_unit_id"),
            "the org column leaves on the way down:\n{down}"
        );
        assert!(
            down.contains("idx_warehouses_company_id_code"),
            "the replaced index is recreated from its stashed definition:\n{down}"
        );
        assert!(
            down.contains("warehouses_company_isolation"),
            "the strict company policy template is restored:\n{down}"
        );
        assert!(
            down.contains("DROP POLICY IF EXISTS warehouses_org_unit_isolation"),
            "the org policy drops first:\n{down}"
        );
    }

    /// A partial unique (`WHERE deleted_at IS NULL` in DSL form) keeps its
    /// predicate on both legs of the re-key — the up side re-creates the
    /// re-keyed unique with it, the down side restores the company unique
    /// with it. Without the predicate the diff-emitted unique is silently
    /// stricter than the declaration (soft-deleted rows collide).
    #[test]
    fn rekeyed_partial_indexes_keep_their_where_predicate() {
        let (mut old, mut new) = rekey_pair();
        let pred = "(metadata->>'deleted_at') IS NULL".to_string();
        new.tables
            .values_mut()
            .for_each(|t| {
                t.indexes
                    .get_mut("idx_warehouses_org_unit_id_code")
                    .unwrap()
                    .where_predicate = Some(pred.clone());
            });
        old.tables
            .values_mut()
            .for_each(|t| {
                t.indexes
                    .get_mut("idx_warehouses_company_id_code")
                    .unwrap()
                    .where_predicate = Some(pred.clone());
            });

        let diff = diff_schemas(&old, &new);
        let up = generate_up_migration(&diff, &new, false);
        let down = generate_down_migration(&diff);
        let expected = " WHERE (metadata->>'deleted_at') IS NULL;";
        assert!(
            up.contains(&format!(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_warehouses_org_unit_id_code ON warehouses (org_unit_id, code){expected}"
            )),
            "the re-keyed unique keeps its partial predicate:\n{up}"
        );
        assert!(
            down.contains(&format!(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_warehouses_company_id_code ON warehouses (company_id, code){expected}"
            )),
            "the restored company unique keeps its partial predicate:\n{down}"
        );
    }
}
