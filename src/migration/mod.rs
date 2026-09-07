//! Database migration utilities
//!
//! Provides schema comparison, safety analysis, and migration generation for PostgreSQL.

mod chain_order;
mod pipeline;
mod schema_diff;

#[cfg(feature = "database")]
mod database_introspector;

pub use chain_order::{simulate_chain, ChainOrderReport, ChainViolation, NEW_MIGRATION_LABEL};
pub use schema_diff::{
    diff_schemas, generate_down_migration, generate_migration, generate_up_migration, ColumnChange,
    ColumnSnapshot, EnumChange, EnumSnapshot, IndexChange, IndexSnapshot, RenameCandidate,
    SchemaDiff, SchemaSnapshot, TableChange, TableSnapshot,
};

pub use pipeline::{is_safe_type_widening, MigrationResult, SafetyAnalysis};

#[cfg(feature = "database")]
pub use database_introspector::{normalize_pg_type, DatabaseIntrospector};
