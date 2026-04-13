//! Webapp code generator (TypeScript + React).
//!
//! Merged from metaphor-plugin-webgen into metaphor-plugin-schema so that
//! `metaphor schema generate:webapp` works self-contained.

pub mod ast;
pub mod config;
pub mod config_file;
pub mod error;
pub mod generator;
pub mod generators;
pub mod parser;
pub mod templates;

// Re-exports
pub use config::{Config, Target};
pub use error::{Error, Result};
pub use generator::Generator;
pub use ast::{
    EntityDefinition, FieldDefinition, FieldType, RelationDefinition, EnumDefinition,
    HookSchema, StateMachine, TransitionDefinition,
    WorkflowSchema, WorkflowStep,
};
pub use parser::{
    ProtoEntity, ProtoField,
    to_snake_case, to_pascal_case, to_camel_case,
    ModelParser, HookParser, WorkflowParser,
};
pub use generators::{
    DomainGenerator, DomainGenerationResult, TypeMapper,
};
