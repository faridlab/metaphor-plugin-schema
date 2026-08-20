//! Parser module for schema definitions
//!
//! This module contains parsers for various schema file formats used in the Backbone Framework.

pub mod hook;
pub mod model;
pub mod proto;
pub mod workflow;

// Re-exports
pub use hook::{parse_hook_file, HookParser};
pub use model::{parse_model_file, ModelParser};
pub use proto::{
    pluralize, to_camel_case, to_kebab_case, to_pascal_case, to_snake_case, ProtoEntity,
    ProtoField, ProtoParser,
};
pub use workflow::{parse_workflow_file, WorkflowParser};
