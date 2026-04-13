//! Template definitions for generated code

pub mod base;
pub mod enhanced;
pub mod workflows;
pub mod state_machines;
pub mod routing;

// Re-export commonly used items
pub use base::{
    HookTemplate, SchemaTemplate, FormTemplate, PageTemplate, TemplateReplacer
};
pub use enhanced::{FormTemplates, TableTemplates};
pub use workflows::WorkflowTemplates;
pub use state_machines::StateMachineTemplates;
pub use routing::RoutingTemplates;
