//! Workflow orchestration generator
//!
//! Generates Rust workflow executor implementations from workflow definitions.
//! Creates type-safe workflow execution with saga pattern support, compensation,
//! and human task handling.

use super::{GenerateError, GeneratedOutput, Generator, build_generated_path, build_subdirectory_mod};
use crate::ast::workflow::{
    Workflow, StepType,
};
use crate::resolver::ResolvedSchema;
use crate::utils::{to_pascal_case, to_snake_case};
use std::fmt::Write;
use std::path::PathBuf;

/// Generates workflow executor implementations from workflow definitions
pub struct FlowGenerator {
    /// Group generated files by model/domain
    group_by_domain: bool,
}

impl FlowGenerator {
    pub fn new() -> Self {
        Self {
            group_by_domain: false,  // Keep flat - only one file per entity
        }
    }

    /// Set whether to group files by domain
    pub fn with_group_by_domain(mut self, group: bool) -> Self {
        self.group_by_domain = group;
        self
    }

    /// Generate flow definition struct
    fn generate_flow_definition(&self, flow: &Workflow) -> Result<String, GenerateError> {
        let mut output = String::new();
        let name = &flow.name;

        // Flow status enum
        writeln!(output, "/// Execution status for {} flow", name).unwrap();
        writeln!(output, "#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]").unwrap();
        writeln!(output, "#[serde(rename_all = \"snake_case\")]").unwrap();
        writeln!(output, "pub enum {}FlowStatus {{", name).unwrap();
        writeln!(output, "    /// Flow is pending execution").unwrap();
        writeln!(output, "    Pending,").unwrap();
        writeln!(output, "    /// Flow is currently running").unwrap();
        writeln!(output, "    Running,").unwrap();
        writeln!(output, "    /// Flow is waiting for an event or condition").unwrap();
        writeln!(output, "    Waiting,").unwrap();
        writeln!(output, "    /// Flow completed successfully").unwrap();
        writeln!(output, "    Completed,").unwrap();
        writeln!(output, "    /// Flow failed").unwrap();
        writeln!(output, "    Failed,").unwrap();
        writeln!(output, "    /// Flow was cancelled").unwrap();
        writeln!(output, "    Cancelled,").unwrap();
        writeln!(output, "    /// Flow is compensating (rolling back)").unwrap();
        writeln!(output, "    Compensating,").unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Step enum
        writeln!(output, "/// Steps in {} flow", name).unwrap();
        writeln!(output, "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]").unwrap();
        writeln!(output, "#[serde(rename_all = \"snake_case\")]").unwrap();
        writeln!(output, "pub enum {}FlowStep {{", name).unwrap();

        for step in &flow.steps {
            let step_variant = to_pascal_case(&step.name);
            writeln!(output, "    /// {}", step.name).unwrap();
            writeln!(output, "    {},", step_variant).unwrap();
        }

        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Flow instance struct
        writeln!(output, "/// Instance of {} flow execution", name).unwrap();
        writeln!(output, "#[derive(Debug, Clone, Serialize, Deserialize)]").unwrap();
        writeln!(output, "pub struct {}FlowInstance {{", name).unwrap();
        writeln!(output, "    /// Unique instance ID").unwrap();
        writeln!(output, "    pub id: String,").unwrap();
        writeln!(output, "    /// Current status").unwrap();
        writeln!(output, "    pub status: {}FlowStatus,", name).unwrap();
        writeln!(output, "    /// Current step").unwrap();
        writeln!(output, "    pub current_step: Option<{}FlowStep>,", name).unwrap();
        writeln!(output, "    /// Flow context (variables)").unwrap();
        writeln!(output, "    pub context: serde_json::Value,").unwrap();
        writeln!(output, "    /// Completed steps").unwrap();
        writeln!(output, "    pub completed_steps: Vec<{}FlowStep>,", name).unwrap();
        writeln!(output, "    /// Error if failed").unwrap();
        writeln!(output, "    pub error: Option<String>,").unwrap();
        writeln!(output, "    /// Created timestamp").unwrap();
        writeln!(output, "    pub created_at: chrono::DateTime<chrono::Utc>,").unwrap();
        writeln!(output, "    /// Updated timestamp").unwrap();
        writeln!(output, "    pub updated_at: chrono::DateTime<chrono::Utc>,").unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Flow instance implementation
        writeln!(output, "impl {}FlowInstance {{", name).unwrap();
        writeln!(output, "    /// Create a new flow instance").unwrap();
        writeln!(output, "    pub fn new(id: impl Into<String>) -> Self {{").unwrap();
        writeln!(output, "        let now = chrono::Utc::now();").unwrap();
        writeln!(output, "        Self {{").unwrap();
        writeln!(output, "            id: id.into(),").unwrap();
        writeln!(output, "            status: {}FlowStatus::Pending,", name).unwrap();
        writeln!(output, "            current_step: None,").unwrap();
        writeln!(output, "            context: serde_json::json!({{}}),").unwrap();
        writeln!(output, "            completed_steps: Vec::new(),").unwrap();
        writeln!(output, "            error: None,").unwrap();
        writeln!(output, "            created_at: now,").unwrap();
        writeln!(output, "            updated_at: now,").unwrap();
        writeln!(output, "        }}").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Check if flow is complete").unwrap();
        writeln!(output, "    pub fn is_complete(&self) -> bool {{").unwrap();
        writeln!(output, "        matches!(").unwrap();
        writeln!(output, "            self.status,").unwrap();
        writeln!(output, "            {}FlowStatus::Completed | {}FlowStatus::Failed | {}FlowStatus::Cancelled", name, name, name).unwrap();
        writeln!(output, "        )").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Check if flow is running").unwrap();
        writeln!(output, "    pub fn is_running(&self) -> bool {{").unwrap();
        writeln!(output, "        matches!(").unwrap();
        writeln!(output, "            self.status,").unwrap();
        writeln!(output, "            {}FlowStatus::Running | {}FlowStatus::Waiting", name, name).unwrap();
        writeln!(output, "        )").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Set a context variable").unwrap();
        writeln!(output, "    pub fn set_context(&mut self, key: &str, value: serde_json::Value) {{").unwrap();
        writeln!(output, "        if let serde_json::Value::Object(ref mut map) = self.context {{").unwrap();
        writeln!(output, "            map.insert(key.to_string(), value);").unwrap();
        writeln!(output, "        }}").unwrap();
        writeln!(output, "        self.updated_at = chrono::Utc::now();").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Get a context variable").unwrap();
        writeln!(output, "    pub fn get_context(&self, key: &str) -> Option<&serde_json::Value> {{").unwrap();
        writeln!(output, "        if let serde_json::Value::Object(ref map) = self.context {{").unwrap();
        writeln!(output, "            map.get(key)").unwrap();
        writeln!(output, "        }} else {{").unwrap();
        writeln!(output, "            None").unwrap();
        writeln!(output, "        }}").unwrap();
        writeln!(output, "    }}").unwrap();

        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Phase 1 Category C: implement WorkflowContext from backbone-core.
        // The entity field is not present on FlowInstance (it stores JSON context),
        // so we satisfy the interface by returning a todo placeholder — implementors
        // can add an entity field in the CUSTOM section.
        writeln!(output, "// Phase 1: FlowInstance satisfies backbone_core::flow::WorkflowContext.").unwrap();
        writeln!(output, "// The entity() accessor requires the entity to be carried in the instance;").unwrap();
        writeln!(output, "// add `pub entity: {name}` in the // <<< CUSTOM section and remove the todo!.", name = name).unwrap();
        writeln!(output, "#[allow(unused_variables)]").unwrap();
        writeln!(output, "impl WorkflowContext<{name}FlowInstance> for {name}FlowInstance {{", name = name).unwrap();
        writeln!(output, "    fn entity(&self) -> &{name}FlowInstance {{ self }}", name = name).unwrap();
        writeln!(output, "    fn set_var(&mut self, key: &str, value: serde_json::Value) {{").unwrap();
        writeln!(output, "        self.set_context(key, value);").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output, "    fn get_var(&self, key: &str) -> Option<&serde_json::Value> {{").unwrap();
        writeln!(output, "        self.get_context(key)").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output, "}}").unwrap();

        Ok(output)
    }

    /// Generate step handler trait
    fn generate_step_handler_trait(&self, flow: &Workflow) -> Result<String, GenerateError> {
        let mut output = String::new();
        let name = &flow.name;

        writeln!(output, "/// Step handler trait for {} flow", name).unwrap();
        writeln!(output, "#[async_trait::async_trait]").unwrap();
        writeln!(output, "pub trait {}StepHandler: Send + Sync {{", name).unwrap();

        // Generate handler methods for each step type
        for step in &flow.steps {
            let method_name = to_snake_case(&step.name);

            match &step.step_type {
                StepType::Action(_) => {
                    writeln!(output, "    /// Handle {} step", step.name).unwrap();
                    writeln!(output, "    async fn handle_{}(", method_name).unwrap();
                    writeln!(output, "        &self,").unwrap();
                    writeln!(output, "        instance: &mut {}FlowInstance,", name).unwrap();
                    writeln!(output, "    ) -> Result<Option<{}FlowStep>, FlowError>;", name).unwrap();
                    writeln!(output).unwrap();
                }
                StepType::Wait(_) => {
                    writeln!(output, "    /// Handle wait step {} (returns when event received or timeout)", step.name).unwrap();
                    writeln!(output, "    async fn handle_{}(", method_name).unwrap();
                    writeln!(output, "        &self,").unwrap();
                    writeln!(output, "        instance: &mut {}FlowInstance,", name).unwrap();
                    writeln!(output, "    ) -> Result<Option<{}FlowStep>, FlowError>;", name).unwrap();
                    writeln!(output).unwrap();
                }
                StepType::Condition(_) => {
                    writeln!(output, "    /// Evaluate condition for {} step", step.name).unwrap();
                    writeln!(output, "    async fn evaluate_{}(", method_name).unwrap();
                    writeln!(output, "        &self,").unwrap();
                    writeln!(output, "        instance: &{}FlowInstance,", name).unwrap();
                    writeln!(output, "    ) -> Result<{}FlowStep, FlowError>;", name).unwrap();
                    writeln!(output).unwrap();
                }
                StepType::HumanTask(_) => {
                    writeln!(output, "    /// Create human task for {} step", step.name).unwrap();
                    writeln!(output, "    async fn create_task_{}(", method_name).unwrap();
                    writeln!(output, "        &self,").unwrap();
                    writeln!(output, "        instance: &mut {}FlowInstance,", name).unwrap();
                    writeln!(output, "    ) -> Result<String, FlowError>; // Returns task ID").unwrap();
                    writeln!(output).unwrap();
                    writeln!(output, "    /// Handle task completion for {}", step.name).unwrap();
                    writeln!(output, "    async fn complete_task_{}(", method_name).unwrap();
                    writeln!(output, "        &self,").unwrap();
                    writeln!(output, "        instance: &mut {}FlowInstance,", name).unwrap();
                    writeln!(output, "        decision: &str,").unwrap();
                    writeln!(output, "        form_data: serde_json::Value,").unwrap();
                    writeln!(output, "    ) -> Result<Option<{}FlowStep>, FlowError>;", name).unwrap();
                    writeln!(output).unwrap();
                }
                StepType::Terminal(_) => {
                    writeln!(output, "    /// Handle terminal step {}", step.name).unwrap();
                    writeln!(output, "    async fn handle_{}(", method_name).unwrap();
                    writeln!(output, "        &self,").unwrap();
                    writeln!(output, "        instance: &mut {}FlowInstance,", name).unwrap();
                    writeln!(output, "    ) -> Result<(), FlowError>;").unwrap();
                    writeln!(output).unwrap();
                }
                _ => {
                    writeln!(output, "    /// Handle {} step", step.name).unwrap();
                    writeln!(output, "    async fn handle_{}(", method_name).unwrap();
                    writeln!(output, "        &self,").unwrap();
                    writeln!(output, "        instance: &mut {}FlowInstance,", name).unwrap();
                    writeln!(output, "    ) -> Result<Option<{}FlowStep>, FlowError>;", name).unwrap();
                    writeln!(output).unwrap();
                }
            }
        }

        // Compensation handlers
        if !flow.compensation.is_empty() {
            writeln!(output, "    /// Execute compensation (rollback)").unwrap();
            writeln!(output, "    async fn compensate(").unwrap();
            writeln!(output, "        &self,").unwrap();
            writeln!(output, "        instance: &mut {}FlowInstance,", name).unwrap();
            writeln!(output, "    ) -> Result<(), FlowError>;").unwrap();
            writeln!(output).unwrap();
        }

        writeln!(output, "}}").unwrap();

        Ok(output)
    }

    /// Generate flow executor
    fn generate_flow_executor(&self, flow: &Workflow) -> Result<String, GenerateError> {
        let mut output = String::new();
        let name = &flow.name;

        // Flow executor struct
        writeln!(output, "/// Executor for {} flow", name).unwrap();
        writeln!(output, "pub struct {}FlowExecutor<H: {}StepHandler> {{", name, name).unwrap();
        writeln!(output, "    handler: Arc<H>,").unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Implementation
        writeln!(output, "impl<H: {}StepHandler> {}FlowExecutor<H> {{", name, name).unwrap();
        writeln!(output, "    /// Create a new flow executor").unwrap();
        writeln!(output, "    pub fn new(handler: Arc<H>) -> Self {{").unwrap();
        writeln!(output, "        Self {{ handler }}").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Start a new flow instance").unwrap();
        writeln!(output, "    pub async fn start(&self, instance_id: impl Into<String>) -> Result<{}FlowInstance, FlowError> {{", name).unwrap();
        writeln!(output, "        let mut instance = {}FlowInstance::new(instance_id);", name).unwrap();
        writeln!(output, "        instance.status = {}FlowStatus::Running;", name).unwrap();

        // Set initial step
        if let Some(first_step) = flow.steps.first() {
            writeln!(output, "        instance.current_step = Some({}FlowStep::{});", name, to_pascal_case(&first_step.name)).unwrap();
        }

        writeln!(output, "        Ok(instance)").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Execute step method
        writeln!(output, "    /// Execute the current step").unwrap();
        writeln!(output, "    pub async fn execute_step(&self, instance: &mut {}FlowInstance) -> Result<(), FlowError> {{", name).unwrap();
        writeln!(output, "        let current_step = match instance.current_step {{").unwrap();
        writeln!(output, "            Some(step) => step,").unwrap();
        writeln!(output, "            None => return Err(FlowError::NoCurrentStep),").unwrap();
        writeln!(output, "        }};").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "        let next_step = match current_step {{").unwrap();

        for step in &flow.steps {
            let step_variant = to_pascal_case(&step.name);
            let method_name = to_snake_case(&step.name);

            match &step.step_type {
                StepType::Condition(_) => {
                    writeln!(output, "            {}FlowStep::{} => {{", name, step_variant).unwrap();
                    writeln!(output, "                let next = self.handler.evaluate_{}(instance).await?;", method_name).unwrap();
                    writeln!(output, "                Some(next)").unwrap();
                    writeln!(output, "            }}").unwrap();
                }
                StepType::Terminal(_) => {
                    writeln!(output, "            {}FlowStep::{} => {{", name, step_variant).unwrap();
                    writeln!(output, "                self.handler.handle_{}(instance).await?;", method_name).unwrap();
                    writeln!(output, "                None // Terminal step").unwrap();
                    writeln!(output, "            }}").unwrap();
                }
                _ => {
                    writeln!(output, "            {}FlowStep::{} => {{", name, step_variant).unwrap();
                    writeln!(output, "                self.handler.handle_{}(instance).await?", method_name).unwrap();
                    writeln!(output, "            }}").unwrap();
                }
            }
        }

        writeln!(output, "        }};").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "        // Mark current step as completed").unwrap();
        writeln!(output, "        instance.completed_steps.push(current_step);").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "        // Move to next step").unwrap();
        writeln!(output, "        match next_step {{").unwrap();
        writeln!(output, "            Some(next) => {{").unwrap();
        writeln!(output, "                instance.current_step = Some(next);").unwrap();
        writeln!(output, "            }}").unwrap();
        writeln!(output, "            None => {{").unwrap();
        writeln!(output, "                instance.current_step = None;").unwrap();
        writeln!(output, "                instance.status = {}FlowStatus::Completed;", name).unwrap();
        writeln!(output, "            }}").unwrap();
        writeln!(output, "        }}").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "        instance.updated_at = chrono::Utc::now();").unwrap();
        writeln!(output, "        Ok(())").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Run to completion method
        writeln!(output, "    /// Run the flow to completion").unwrap();
        writeln!(output, "    pub async fn run(&self, instance: &mut {}FlowInstance) -> Result<(), FlowError> {{", name).unwrap();
        writeln!(output, "        while !instance.is_complete() && instance.status != {}FlowStatus::Waiting {{", name).unwrap();
        writeln!(output, "            self.execute_step(instance).await?;").unwrap();
        writeln!(output, "        }}").unwrap();
        writeln!(output, "        Ok(())").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Cancel method
        writeln!(output, "    /// Cancel the flow").unwrap();
        writeln!(output, "    pub fn cancel(&self, instance: &mut {}FlowInstance) {{", name).unwrap();
        writeln!(output, "        instance.status = {}FlowStatus::Cancelled;", name).unwrap();
        writeln!(output, "        instance.updated_at = chrono::Utc::now();").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Fail method
        writeln!(output, "    /// Mark flow as failed").unwrap();
        writeln!(output, "    pub fn fail(&self, instance: &mut {}FlowInstance, error: impl Into<String>) {{", name).unwrap();
        writeln!(output, "        instance.status = {}FlowStatus::Failed;", name).unwrap();
        writeln!(output, "        instance.error = Some(error.into());").unwrap();
        writeln!(output, "        instance.updated_at = chrono::Utc::now();").unwrap();
        writeln!(output, "    }}").unwrap();

        writeln!(output, "}}").unwrap();

        Ok(output)
    }

    /// Generate complete flow file
    fn generate_flow_file(&self, flow: &Workflow) -> Result<String, GenerateError> {
        let mut output = String::new();
        let name = &flow.name;

        // Header
        writeln!(output, "//! {} flow implementation", name).unwrap();
        writeln!(output, "//!").unwrap();
        writeln!(output, "//! Generated by metaphor-schema").unwrap();
        if let Some(ref desc) = flow.description {
            writeln!(output, "//!").unwrap();
            // Each line of the description needs the //! prefix
            for line in desc.lines() {
                writeln!(output, "//! {}", line).unwrap();
            }
        }
        writeln!(output).unwrap();

        // Imports
        writeln!(output, "use serde::{{Deserialize, Serialize}};").unwrap();
        writeln!(output, "use std::sync::Arc;").unwrap();
        writeln!(output, "use chrono;").unwrap();
        writeln!(output, "use backbone_core::flow::{{WorkflowStep, WorkflowContext}};").unwrap();
        writeln!(output).unwrap();

        // Error type
        writeln!(output, "/// Error type for flow execution").unwrap();
        writeln!(output, "#[derive(Debug, Clone, thiserror::Error)]").unwrap();
        writeln!(output, "pub enum FlowError {{").unwrap();
        writeln!(output, "    #[error(\"No current step to execute\")]").unwrap();
        writeln!(output, "    NoCurrentStep,").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "    #[error(\"Step execution failed: {{0}}\")]").unwrap();
        writeln!(output, "    StepFailed(String),").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "    #[error(\"Condition evaluation failed: {{0}}\")]").unwrap();
        writeln!(output, "    ConditionFailed(String),").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "    #[error(\"Compensation failed: {{0}}\")]").unwrap();
        writeln!(output, "    CompensationFailed(String),").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "    #[error(\"Flow timed out\")]").unwrap();
        writeln!(output, "    Timeout,").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "    #[error(\"Flow cancelled\")]").unwrap();
        writeln!(output, "    Cancelled,").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "    #[error(\"Invalid state transition: {{from}} -> {{to}}\")]").unwrap();
        writeln!(output, "    InvalidTransition {{ from: String, to: String }},").unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Generate flow definition
        let definition = self.generate_flow_definition(flow)?;
        output.push_str(&definition);
        writeln!(output).unwrap();

        // Generate step handler trait
        let handler_trait = self.generate_step_handler_trait(flow)?;
        output.push_str(&handler_trait);
        writeln!(output).unwrap();

        // Generate flow executor
        let executor = self.generate_flow_executor(flow)?;
        output.push_str(&executor);
        writeln!(output).unwrap();

        // Generate tests
        self.generate_tests(&mut output, flow)?;

        Ok(output)
    }

    /// Generate unit tests
    fn generate_tests(&self, output: &mut String, flow: &Workflow) -> Result<(), GenerateError> {
        let name = &flow.name;

        writeln!(output, "#[cfg(test)]").unwrap();
        writeln!(output, "mod tests {{").unwrap();
        writeln!(output, "    use super::*;").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    #[test]").unwrap();
        writeln!(output, "    fn test_flow_instance_creation() {{").unwrap();
        writeln!(output, "        let instance = {}FlowInstance::new(\"test-1\");", name).unwrap();
        writeln!(output, "        assert_eq!(instance.id, \"test-1\");").unwrap();
        writeln!(output, "        assert_eq!(instance.status, {}FlowStatus::Pending);", name).unwrap();
        writeln!(output, "        assert!(!instance.is_complete());").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    #[test]").unwrap();
        writeln!(output, "    fn test_flow_context() {{").unwrap();
        writeln!(output, "        let mut instance = {}FlowInstance::new(\"test-2\");", name).unwrap();
        writeln!(output, "        instance.set_context(\"key\", serde_json::json!(\"value\"));").unwrap();
        writeln!(output, "        let value = instance.get_context(\"key\");").unwrap();
        writeln!(output, "        assert_eq!(value, Some(&serde_json::json!(\"value\")));").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    #[test]").unwrap();
        writeln!(output, "    fn test_flow_status_transitions() {{").unwrap();
        writeln!(output, "        let mut instance = {}FlowInstance::new(\"test-3\");", name).unwrap();
        writeln!(output, "        assert!(!instance.is_running());").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "        instance.status = {}FlowStatus::Running;", name).unwrap();
        writeln!(output, "        assert!(instance.is_running());").unwrap();
        writeln!(output, "        assert!(!instance.is_complete());").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "        instance.status = {}FlowStatus::Completed;", name).unwrap();
        writeln!(output, "        assert!(instance.is_complete());").unwrap();
        writeln!(output, "        assert!(!instance.is_running());").unwrap();
        writeln!(output, "    }}").unwrap();

        writeln!(output, "}}").unwrap();

        Ok(())
    }
}

impl Default for FlowGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl Generator for FlowGenerator {
    fn generate(&self, schema: &ResolvedSchema) -> Result<GeneratedOutput, GenerateError> {
        let mut output = GeneratedOutput::new();

        // Generate workflow files for each workflow definition
        for workflow in &schema.schema.workflows {
            let file_name = format!("{}_workflow.rs", to_snake_case(&workflow.name));
            let content = self.generate_flow_file(workflow)?;
            let path = build_generated_path("src/application/workflows", &workflow.name, &file_name, self.group_by_domain);
            output.add_file(path, content);

            // Generate subdirectory mod.rs if grouping by domain
            if self.group_by_domain {
                let mod_path = PathBuf::from(format!("src/application/workflows/{}/mod.rs", to_snake_case(&workflow.name)));
                let sub_mod_content = build_subdirectory_mod(&workflow.name, &file_name.replace(".rs", ""));
                output.add_file(mod_path, sub_mod_content);
            }
        }

        // Generate mod.rs if we have any workflows
        if !schema.schema.workflows.is_empty() {
            let mut mod_content = String::new();

            for workflow in &schema.schema.workflows {
                let snake_name = to_snake_case(&workflow.name);
                if self.group_by_domain {
                    writeln!(mod_content, "mod {};", snake_name).unwrap();
                } else {
                    writeln!(mod_content, "mod {}_workflow;", snake_name).unwrap();
                }
            }
            writeln!(mod_content).unwrap();

            // Only export FlowError from the first workflow to avoid conflicts
            let mut first = true;
            for workflow in &schema.schema.workflows {
                let name = &workflow.name;
                let snake_name = to_snake_case(name);
                if first {
                    if self.group_by_domain {
                        writeln!(
                            mod_content,
                            "pub use {}::{{{}FlowStatus, {}FlowStep, {}FlowInstance, {}StepHandler, {}FlowExecutor, FlowError}};",
                            snake_name,
                            name, name, name, name, name
                        ).unwrap();
                    } else {
                        writeln!(
                            mod_content,
                            "pub use {}_workflow::{{{}FlowStatus, {}FlowStep, {}FlowInstance, {}StepHandler, {}FlowExecutor, FlowError}};",
                            snake_name,
                            name, name, name, name, name
                        ).unwrap();
                    }
                    first = false;
                } else {
                    if self.group_by_domain {
                        writeln!(
                            mod_content,
                            "pub use {}::{{{}FlowStatus, {}FlowStep, {}FlowInstance, {}StepHandler, {}FlowExecutor}};",
                            snake_name,
                            name, name, name, name, name
                        ).unwrap();
                    } else {
                        writeln!(
                            mod_content,
                            "pub use {}_workflow::{{{}FlowStatus, {}FlowStep, {}FlowInstance, {}StepHandler, {}FlowExecutor}};",
                            snake_name,
                            name, name, name, name, name
                        ).unwrap();
                    }
                }
            }

            output.add_file(
                PathBuf::from("src/application/workflows/mod.rs"),
                mod_content,
            );
        }

        Ok(output)
    }

    fn name(&self) -> &'static str {
        "flow"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::workflow::*;

    fn create_test_workflow() -> Workflow {
        Workflow {
            name: "OrderProcessing".to_string(),
            description: Some("Process an order through validation, payment, and fulfillment".to_string()),
            steps: vec![
                Step {
                    name: "validate_order".to_string(),
                    step_type: StepType::Action(ActionStep {
                        action: "validate".to_string(),
                        entity: Some("Order".to_string()),
                        ..Default::default()
                    }),
                    on_success: Some(StepOutcome {
                        next: Some("process_payment".to_string()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                Step {
                    name: "process_payment".to_string(),
                    step_type: StepType::Action(ActionStep {
                        action: "charge".to_string(),
                        entity: Some("Payment".to_string()),
                        ..Default::default()
                    }),
                    on_success: Some(StepOutcome {
                        next: Some("complete".to_string()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                Step {
                    name: "complete".to_string(),
                    step_type: StepType::Terminal(TerminalStep {
                        status: TerminalStatus::Success,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn test_generate_flow_definition() {
        let generator = FlowGenerator::new();
        let workflow = create_test_workflow();

        let result = generator.generate_flow_definition(&workflow);
        assert!(result.is_ok());

        let content = result.unwrap();
        assert!(content.contains("pub enum OrderProcessingFlowStatus"));
        assert!(content.contains("pub enum OrderProcessingFlowStep"));
        assert!(content.contains("pub struct OrderProcessingFlowInstance"));
        assert!(content.contains("ValidateOrder"));
        assert!(content.contains("ProcessPayment"));
        assert!(content.contains("Complete"));
    }

    #[test]
    fn test_generate_step_handler_trait() {
        let generator = FlowGenerator::new();
        let workflow = create_test_workflow();

        let result = generator.generate_step_handler_trait(&workflow);
        assert!(result.is_ok());

        let content = result.unwrap();
        assert!(content.contains("pub trait OrderProcessingStepHandler"));
        assert!(content.contains("handle_validate_order"));
        assert!(content.contains("handle_process_payment"));
        assert!(content.contains("handle_complete"));
    }

    #[test]
    fn test_generate_flow_executor() {
        let generator = FlowGenerator::new();
        let workflow = create_test_workflow();

        let result = generator.generate_flow_executor(&workflow);
        assert!(result.is_ok());

        let content = result.unwrap();
        assert!(content.contains("pub struct OrderProcessingFlowExecutor"));
        assert!(content.contains("pub async fn start"));
        assert!(content.contains("pub async fn execute_step"));
        assert!(content.contains("pub async fn run"));
    }
}
