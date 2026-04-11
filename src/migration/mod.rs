//! Database migration utilities
//!
//! Provides schema comparison, safety analysis, and migration generation for PostgreSQL.

mod schema_diff;
mod pipeline;

#[cfg(feature = "database")]
mod database_introspector;

pub use schema_diff::{
    ColumnChange, ColumnSnapshot, EnumChange, EnumSnapshot, IndexChange, IndexSnapshot,
    RenameCandidate, SchemaDiff, SchemaSnapshot, TableChange, TableSnapshot,
    diff_schemas, generate_migration, generate_up_migration, generate_down_migration,
};

pub use pipeline::{SafetyAnalysis, MigrationResult, is_safe_type_widening};

#[cfg(feature = "database")]
pub use database_introspector::{DatabaseIntrospector, normalize_pg_type};
