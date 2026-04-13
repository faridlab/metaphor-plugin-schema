//! Parser module for schema definitions
//!
//! This module contains parsers for various schema file formats used in the Backbone Framework.

pub mod model;
pub mod hook;
pub mod workflow;
pub mod proto;

// Re-exports
pub use model::{ModelParser, parse_model_file};
pub use hook::{HookParser, parse_hook_file};
pub use workflow::{WorkflowParser, parse_workflow_file};
pub use proto::{ProtoParser, ProtoEntity, ProtoField, to_snake_case, to_pascal_case, to_camel_case, to_kebab_case, pluralize};
