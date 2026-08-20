//! Schema diff detection and migration generation
//!
//! Compares database schemas and generates ALTER statements for changes.

use crate::ast::CompanyFence;
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
    /// gains `company_id` in this same diff (the gain path installs the fence fresh), or the
    /// posture is unchanged.
    pub fence_flip: Option<(CompanyFence, CompanyFence)>,
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

    // Posture flip on an existing table (ADR-0014): same columns, different fence
    // declaration → the RLS policy template must be re-emitted. Skipped when `company_id`
    // is newly added here — that path installs the fence from scratch, so a flip record
    // would only duplicate it.
    if change.columns_added.iter().all(|c| c.name != "company_id") {
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
    }
}

impl EnumChange {
    fn has_changes(&self) -> bool {
        !self.variants_added.is_empty() || !self.variants_removed.is_empty()
    }
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
                    "CREATE {}INDEX IF NOT EXISTS {} ON {} ({});",
                    unique,
                    idx.name,
                    table_name,
                    idx.columns.join(", ")
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
                "-- CREATE {}INDEX CONCURRENTLY IF NOT EXISTS {} ON {} ({});",
                unique,
                idx.name,
                table_name,
                idx.columns.join(", ")
            )
            .unwrap();
            writeln!(
                output,
                "CREATE {}INDEX IF NOT EXISTS {} ON {} ({});",
                unique,
                idx.name,
                table_name,
                idx.columns.join(", ")
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
}
