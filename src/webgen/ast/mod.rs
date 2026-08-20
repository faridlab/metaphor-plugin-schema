//! AST (Abstract Syntax Tree) for schema definitions
//!
//! This module defines the AST structures for parsing and representing
//! Backbone Framework schema definitions (YAML-based).

pub mod entity;
pub mod state_machine;
pub mod workflow;

// Re-exports
pub use entity::{
    EntityDefinition, EnumDefinition, EnumVariant, FieldAttribute, FieldDefinition, FieldType,
    IndexDefinition, RelationDefinition, RelationType,
};
pub use state_machine::{
    ComputedField, HookSchema, PermissionRule, StateDefinition, StateMachine, TransitionDefinition,
    Trigger, ValidationRule,
};
pub use workflow::{
    CompensationStep, ContextVariable, WorkflowConfig, WorkflowSchema, WorkflowStep,
    WorkflowStepType, WorkflowTrigger,
};
