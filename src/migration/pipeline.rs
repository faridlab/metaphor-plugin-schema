//! Migration pipeline orchestrator
//!
//! Ties together database introspection, schema diffing, safety analysis,
//! and SQL generation into a single workflow.

use super::schema_diff::{ColumnChange, SchemaDiff, TableChange};

/// Result of running the migration pipeline.
pub struct MigrationResult {
    /// The computed diff between old and new schemas.
    pub diff: SchemaDiff,
    /// Generated SQL migration (UP + DOWN).
    pub sql: String,
    /// Safety classification of all operations.
    pub safety: SafetyAnalysis,
}

/// Classifies migration operations by risk level.
#[derive(Debug, Default)]
pub struct SafetyAnalysis {
    /// Operations that are safe to apply automatically (e.g., ADD COLUMN nullable).
    pub safe_operations: Vec<String>,
    /// Operations that require human review (e.g., SET NOT NULL on existing data).
    pub review_required: Vec<String>,
    /// Operations that may cause data loss (e.g., DROP COLUMN, DROP TABLE).
    pub destructive_operations: Vec<String>,
}

impl SafetyAnalysis {
    /// Build a safety analysis from a schema diff.
    pub fn from_diff(diff: &SchemaDiff) -> Self {
        let mut analysis = Self::default();

        // New tables are always safe
        for table in &diff.tables_added {
            analysis
                .safe_operations
                .push(format!("CREATE TABLE {}", table));
        }

        // Dropped tables are destructive
        for table in &diff.tables_removed {
            analysis
                .destructive_operations
                .push(format!("DROP TABLE {}", table));
        }

        // New enums are safe
        for enum_name in &diff.enums_added {
            analysis
                .safe_operations
                .push(format!("CREATE TYPE {}", enum_name));
        }

        // Dropped enums are destructive
        for enum_name in &diff.enums_removed {
            analysis
                .destructive_operations
                .push(format!("DROP TYPE {}", enum_name));
        }

        // Enum variant additions are safe, removals are destructive
        for (enum_name, change) in &diff.enum_changes {
            for variant in &change.variants_added {
                analysis
                    .safe_operations
                    .push(format!("ADD VALUE '{}' TO {}", variant, enum_name));
            }
            for variant in &change.variants_removed {
                analysis
                    .destructive_operations
                    .push(format!("REMOVE VALUE '{}' FROM {}", variant, enum_name));
            }
        }

        // Table changes
        for (table_name, change) in &diff.table_changes {
            Self::classify_table_change(&mut analysis, table_name, change);
        }

        analysis
    }

    fn classify_table_change(analysis: &mut SafetyAnalysis, table_name: &str, change: &TableChange) {
        // Added columns
        for col in &change.columns_added {
            if col.nullable || col.default.is_some() {
                analysis
                    .safe_operations
                    .push(format!("ADD COLUMN {}.{}", table_name, col.name));
            } else {
                // NOT NULL without default requires review (existing rows need a value)
                analysis.review_required.push(format!(
                    "ADD COLUMN {}.{} NOT NULL (existing rows need a default value)",
                    table_name, col.name
                ));
            }
        }

        // Removed columns are destructive
        for col_name in &change.columns_removed {
            analysis
                .destructive_operations
                .push(format!("DROP COLUMN {}.{}", table_name, col_name));
        }

        // Modified columns
        for col_change in &change.columns_modified {
            Self::classify_column_change(analysis, table_name, col_change);
        }

        // Added indexes are safe
        for idx in &change.indexes_added {
            analysis
                .safe_operations
                .push(format!("CREATE INDEX {} ON {}", idx.name, table_name));
        }

        // Removed indexes are review-required (may affect query performance)
        for idx_name in &change.indexes_removed {
            analysis.review_required.push(format!(
                "DROP INDEX {} ON {} (may affect query performance)",
                idx_name, table_name
            ));
        }
    }

    fn classify_column_change(
        analysis: &mut SafetyAnalysis,
        table_name: &str,
        change: &ColumnChange,
    ) {
        // Type changes
        if let (Some(old_type), Some(new_type)) = (&change.old_type, &change.new_type) {
            if is_safe_type_widening(old_type, new_type) {
                analysis.safe_operations.push(format!(
                    "ALTER {}.{} TYPE {} -> {} (safe widening)",
                    table_name, change.column_name, old_type, new_type
                ));
            } else {
                analysis.review_required.push(format!(
                    "ALTER {}.{} TYPE {} -> {} (may lose data or fail)",
                    table_name, change.column_name, old_type, new_type
                ));
            }
        }

        // Nullability changes
        if let Some(now_nullable) = change.nullable_changed {
            if now_nullable {
                // DROP NOT NULL is safe
                analysis.safe_operations.push(format!(
                    "ALTER {}.{} DROP NOT NULL",
                    table_name, change.column_name
                ));
            } else {
                // SET NOT NULL needs review (existing NULLs would fail)
                analysis.review_required.push(format!(
                    "ALTER {}.{} SET NOT NULL (existing NULL values will cause failure)",
                    table_name, change.column_name
                ));
            }
        }

        // Default changes are safe
        if change.default_changed.is_some() {
            analysis.safe_operations.push(format!(
                "ALTER {}.{} SET DEFAULT",
                table_name, change.column_name
            ));
        }
    }

    /// Returns true if there are no destructive or review-required operations.
    pub fn is_fully_safe(&self) -> bool {
        self.destructive_operations.is_empty() && self.review_required.is_empty()
    }

    /// Format a colored summary for CLI output.
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();

        if !self.safe_operations.is_empty() {
            lines.push(format!("  Safe operations: {}", self.safe_operations.len()));
            for op in &self.safe_operations {
                lines.push(format!("    + {}", op));
            }
        }

        if !self.review_required.is_empty() {
            lines.push(format!(
                "  Review required: {}",
                self.review_required.len()
            ));
            for op in &self.review_required {
                lines.push(format!("    ~ {}", op));
            }
        }

        if !self.destructive_operations.is_empty() {
            lines.push(format!(
                "  Destructive: {}",
                self.destructive_operations.len()
            ));
            for op in &self.destructive_operations {
                lines.push(format!("    - {}", op));
            }
        }

        if lines.is_empty() {
            "  No operations".to_string()
        } else {
            lines.join("\n")
        }
    }
}

/// Check if a type change is a safe widening (no data loss).
pub fn is_safe_type_widening(old_type: &str, new_type: &str) -> bool {
    let old = old_type.to_uppercase();
    let new = new_type.to_uppercase();

    // Same type is always safe
    if old == new {
        return true;
    }

    // Integer widening: SMALLINT -> INTEGER -> BIGINT
    let int_order = ["SMALLINT", "INTEGER", "BIGINT"];
    if let (Some(old_pos), Some(new_pos)) = (
        int_order.iter().position(|t| *t == old),
        int_order.iter().position(|t| *t == new),
    ) {
        return new_pos >= old_pos;
    }

    // Float widening: REAL -> DOUBLE PRECISION
    if old == "REAL" && new == "DOUBLE PRECISION" {
        return true;
    }

    // VARCHAR widening: VARCHAR(N) -> VARCHAR(M) where M >= N
    if let (Some(old_len), Some(new_len)) = (parse_varchar_len(&old), parse_varchar_len(&new)) {
        return new_len >= old_len;
    }

    // VARCHAR -> TEXT is always safe
    if old.starts_with("VARCHAR") && new == "TEXT" {
        return true;
    }

    // DECIMAL precision widening
    if let (Some((old_p, old_s)), Some((new_p, new_s))) =
        (parse_decimal_precision(&old), parse_decimal_precision(&new))
    {
        return new_p >= old_p && new_s >= old_s;
    }

    false
}

fn parse_varchar_len(type_str: &str) -> Option<i32> {
    let trimmed = type_str.trim();
    if let Some(rest) = trimmed.strip_prefix("VARCHAR(") {
        if let Some(num_str) = rest.strip_suffix(')') {
            return num_str.trim().parse().ok();
        }
    }
    None
}

fn parse_decimal_precision(type_str: &str) -> Option<(i32, i32)> {
    let trimmed = type_str.trim();
    if let Some(rest) = trimmed.strip_prefix("DECIMAL(") {
        if let Some(inner) = rest.strip_suffix(')') {
            let parts: Vec<&str> = inner.split(',').collect();
            if parts.len() == 2 {
                if let (Ok(p), Ok(s)) = (parts[0].trim().parse(), parts[1].trim().parse()) {
                    return Some((p, s));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::{ColumnSnapshot, EnumChange, SchemaDiff, TableChange};
    use indexmap::IndexMap;

    #[test]
    fn test_safe_type_widening_integers() {
        assert!(is_safe_type_widening("SMALLINT", "INTEGER"));
        assert!(is_safe_type_widening("INTEGER", "BIGINT"));
        assert!(is_safe_type_widening("SMALLINT", "BIGINT"));
        assert!(!is_safe_type_widening("BIGINT", "INTEGER"));
        assert!(!is_safe_type_widening("INTEGER", "SMALLINT"));
    }

    #[test]
    fn test_safe_type_widening_varchar() {
        assert!(is_safe_type_widening("VARCHAR(100)", "VARCHAR(255)"));
        assert!(is_safe_type_widening("VARCHAR(255)", "VARCHAR(255)"));
        assert!(!is_safe_type_widening("VARCHAR(255)", "VARCHAR(100)"));
        assert!(is_safe_type_widening("VARCHAR(255)", "TEXT"));
    }

    #[test]
    fn test_safe_type_widening_float() {
        assert!(is_safe_type_widening("REAL", "DOUBLE PRECISION"));
        assert!(!is_safe_type_widening("DOUBLE PRECISION", "REAL"));
    }

    #[test]
    fn test_safe_type_widening_decimal() {
        assert!(is_safe_type_widening("DECIMAL(5, 2)", "DECIMAL(10, 4)"));
        assert!(is_safe_type_widening("DECIMAL(10, 2)", "DECIMAL(19, 4)"));
        assert!(!is_safe_type_widening("DECIMAL(19, 4)", "DECIMAL(5, 2)"));
    }

    #[test]
    fn test_safe_type_widening_same_type() {
        assert!(is_safe_type_widening("UUID", "UUID"));
        assert!(is_safe_type_widening("BOOLEAN", "BOOLEAN"));
        assert!(is_safe_type_widening("TEXT", "TEXT"));
    }

    #[test]
    fn test_safety_analysis_new_table() {
        let diff = SchemaDiff {
            tables_added: vec!["users".to_string()],
            ..Default::default()
        };

        let analysis = SafetyAnalysis::from_diff(&diff);
        assert_eq!(analysis.safe_operations.len(), 1);
        assert!(analysis.destructive_operations.is_empty());
        assert!(analysis.is_fully_safe());
    }

    #[test]
    fn test_safety_analysis_drop_table() {
        let diff = SchemaDiff {
            tables_removed: vec!["old_table".to_string()],
            ..Default::default()
        };

        let analysis = SafetyAnalysis::from_diff(&diff);
        assert_eq!(analysis.destructive_operations.len(), 1);
        assert!(!analysis.is_fully_safe());
    }

    #[test]
    fn test_safety_analysis_add_nullable_column() {
        let mut table_changes = IndexMap::new();
        table_changes.insert(
            "users".to_string(),
            TableChange {
                table_name: "users".to_string(),
                columns_added: vec![ColumnSnapshot {
                    name: "bio".to_string(),
                    data_type: "TEXT".to_string(),
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

        let analysis = SafetyAnalysis::from_diff(&diff);
        assert_eq!(analysis.safe_operations.len(), 1);
        assert!(analysis.is_fully_safe());
    }

    #[test]
    fn test_safety_analysis_add_not_null_column() {
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

        let analysis = SafetyAnalysis::from_diff(&diff);
        assert_eq!(analysis.review_required.len(), 1);
        assert!(!analysis.is_fully_safe());
    }

    #[test]
    fn test_safety_analysis_add_not_null_with_default() {
        let mut table_changes = IndexMap::new();
        table_changes.insert(
            "users".to_string(),
            TableChange {
                table_name: "users".to_string(),
                columns_added: vec![ColumnSnapshot {
                    name: "status".to_string(),
                    data_type: "VARCHAR(50)".to_string(),
                    nullable: false,
                    default: Some("'active'".to_string()),
                    is_unique: false,
                }],
                ..Default::default()
            },
        );

        let diff = SchemaDiff {
            table_changes,
            ..Default::default()
        };

        let analysis = SafetyAnalysis::from_diff(&diff);
        // NOT NULL with default is safe
        assert_eq!(analysis.safe_operations.len(), 1);
        assert!(analysis.is_fully_safe());
    }

    #[test]
    fn test_safety_analysis_enum_changes() {
        let mut enum_changes = IndexMap::new();
        enum_changes.insert(
            "status".to_string(),
            EnumChange {
                enum_name: "status".to_string(),
                variants_added: vec!["archived".to_string()],
                variants_removed: vec!["deleted".to_string()],
            },
        );

        let diff = SchemaDiff {
            enum_changes,
            ..Default::default()
        };

        let analysis = SafetyAnalysis::from_diff(&diff);
        assert_eq!(analysis.safe_operations.len(), 1); // add variant
        assert_eq!(analysis.destructive_operations.len(), 1); // remove variant
        assert!(!analysis.is_fully_safe());
    }

    #[test]
    fn test_safety_analysis_type_widening() {
        let mut table_changes = IndexMap::new();
        table_changes.insert(
            "products".to_string(),
            TableChange {
                table_name: "products".to_string(),
                columns_modified: vec![ColumnChange {
                    column_name: "price".to_string(),
                    old_type: Some("DECIMAL(5, 2)".to_string()),
                    new_type: Some("DECIMAL(19, 4)".to_string()),
                    nullable_changed: None,
                    default_changed: None,
                }],
                ..Default::default()
            },
        );

        let diff = SchemaDiff {
            table_changes,
            ..Default::default()
        };

        let analysis = SafetyAnalysis::from_diff(&diff);
        assert_eq!(analysis.safe_operations.len(), 1);
        assert!(analysis.is_fully_safe());
    }
}
