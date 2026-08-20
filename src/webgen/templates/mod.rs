//! Template definitions for generated code

pub mod base;
pub mod enhanced;
pub mod routing;
pub mod state_machines;
pub mod workflows;

// Re-export commonly used items
pub use base::{FormTemplate, HookTemplate, PageTemplate, SchemaTemplate, TemplateReplacer};
pub use enhanced::{FormTemplates, TableTemplates};
pub use routing::RoutingTemplates;
pub use state_machines::StateMachineTemplates;
pub use workflows::WorkflowTemplates;
