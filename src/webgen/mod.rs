//! Webapp code generator (TypeScript + React).
//!
//! Merged from metaphor-plugin-webgen into metaphor-plugin-schema so that
//! `metaphor schema generate:webapp` works self-contained.

pub mod ast;
pub mod config;
pub mod config_file;
pub(crate) mod custom_blocks;
pub mod error;
pub mod generator;
pub mod generators;
pub mod parser;
pub mod templates;

// Re-exports
pub use ast::{
    EntityDefinition, EnumDefinition, FieldDefinition, FieldType, HookSchema, RelationDefinition,
    StateMachine, TransitionDefinition, WorkflowSchema, WorkflowStep,
};
pub use config::{Config, Target};
pub use error::{Error, Result};
pub use generator::Generator;
pub use generators::{DomainGenerationResult, DomainGenerator, TypeMapper};
pub use parser::{
    to_camel_case, to_pascal_case, to_snake_case, HookParser, ModelParser, ProtoEntity, ProtoField,
    WorkflowParser,
};
