//! AST (Abstract Syntax Tree) for schema definitions
//!
//! This module defines the AST structures for parsing and representing
//! Backbone Framework schema definitions (YAML-based).

pub mod entity;
pub mod state_machine;
pub mod workflow;

// Re-exports
pub use entity::{EntityDefinition, FieldDefinition, FieldType, RelationDefinition, EnumDefinition, EnumVariant, FieldAttribute, RelationType, IndexDefinition};
pub use state_machine::{HookSchema, StateMachine, StateDefinition, TransitionDefinition, ValidationRule, PermissionRule, Trigger, ComputedField};
pub use workflow::{WorkflowSchema, WorkflowTrigger, WorkflowConfig, WorkflowStep, WorkflowStepType, ContextVariable, CompensationStep};
