//! `metaphor schema migration` and `metaphor schema status` — generate
//! database migration SQL from schema drift, or report drift without
//! generating anything.
//!
//! Drift is computed by diffing the current resolved schema against an
//! "old" snapshot, where the old snapshot comes from either a live
//! database introspection (when `database_url` is provided and the
//! `database` feature is on) or a previously-saved `.schema_snapshot.json`
//! file alongside the schema directory.
//!
//! Layout:
//!
//! - [`migration`] — write migration SQL (`metaphor schema migration`).
//! - [`status`] — read-only drift check that exits non-zero on drift.
//! - [`snapshot`] — shared helpers used by both: load the old snapshot,
//!   build a new one from a resolved schema, map [`crate::ast::TypeRef`]
//!   to its PostgreSQL spelling.

mod migration;
mod snapshot;
mod status;

pub(super) use migration::execute_migration;
pub(super) use status::execute_status;
