//! Workflow AST for workflow.yaml schema definitions

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Workflow schema definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowSchema {
    pub name: String,
    pub description: String,
    pub version: u32,
    pub trigger: WorkflowTrigger,
    pub config: WorkflowConfig,
    pub context: Vec<ContextVariable>,
    pub steps: Vec<WorkflowStep>,
    pub compensation: Vec<CompensationStep>,
}

/// Workflow trigger
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTrigger {
    pub event: String,
    pub extract: HashMap<String, String>,
}

/// Workflow configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowConfig {
    pub timeout: Option<String>,
    pub persistence: bool,
}

/// Context variable
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextVariable {
    pub name: String,
    pub default_value: Option<String>,
    pub variable_type: Option<String>,
}

/// Workflow step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub name: String,
    pub step_type: WorkflowStepType,
    pub description: Option<String>,
    pub action: Option<String>,
    pub entity: Option<String>,
    pub params: HashMap<String, String>,
    pub conditions: Option<Vec<StepCondition>>,
    pub wait_for: Option<WaitFor>,
    pub on_success: Option<StepTransition>,
    pub on_failure: Option<StepTransition>,
    pub on_event: Option<StepTransition>,
    pub on_timeout: Option<StepTransition>,
}

/// Workflow step type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkflowStepType {
    Action,
    Wait,
    Condition,
    Transition,
    Terminal,
}

/// Step condition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepCondition {
    #[serde(rename = "if")]
    pub condition: String,
    #[serde(rename = "else")]
    pub otherwise: Option<bool>,
    pub next: String,
}

/// Wait for configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitFor {
    pub event: Option<String>,
    pub condition: Option<String>,
    pub timeout: Option<String>,
}

/// Step transition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepTransition {
    pub next: String,
    #[serde(default)]
    pub set: HashMap<String, String>,
}

/// Compensation step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompensationStep {
    pub name: String,
    pub condition: Option<String>,
    pub action: String,
    pub params: HashMap<String, String>,
}

// YAML parsing structures

/// Raw workflow schema from YAML
#[derive(Debug, Deserialize)]
pub(crate) struct RawWorkflowSchema {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: u32,
    pub trigger: RawWorkflowTrigger,
    #[serde(default)]
    pub config: Option<RawWorkflowConfig>,
    #[serde(default)]
    pub context: Option<HashMap<String, Option<serde_yaml::Value>>>,
    pub steps: Vec<RawWorkflowStep>,
    #[serde(default)]
    pub compensation: Vec<RawCompensationStep>,
}

/// Raw workflow trigger
#[derive(Debug, Deserialize)]
pub(crate) struct RawWorkflowTrigger {
    pub event: String,
    pub extract: HashMap<String, String>,
}

/// Raw workflow config
#[derive(Debug, Deserialize)]
pub(crate) struct RawWorkflowConfig {
    #[serde(default)]
    pub timeout: Option<String>,
    #[serde(default)]
    pub persistence: Option<bool>,
}

impl Default for RawWorkflowConfig {
    fn default() -> Self {
        Self {
            timeout: None,
            persistence: Some(true),
        }
    }
}

/// Raw workflow step
#[derive(Debug, Deserialize)]
pub(crate) struct RawWorkflowStep {
    pub name: String,
    #[serde(rename = "type")]
    pub step_type: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub entity: Option<String>,
    #[serde(default)]
    pub params: Option<HashMap<String, String>>,
    #[serde(default)]
    pub conditions: Option<Vec<RawStepCondition>>,
    #[serde(default)]
    pub wait_for: Option<RawWaitFor>,
    #[serde(default)]
    pub on_success: Option<RawStepTransition>,
    #[serde(default)]
    pub on_failure: Option<RawStepTransition>,
    #[serde(default)]
    pub on_event: Option<RawStepTransition>,
    #[serde(default)]
    pub on_timeout: Option<RawStepTransition>,
}

/// Raw step condition
#[derive(Debug, Deserialize)]
pub(crate) struct RawStepCondition {
    #[serde(rename = "if")]
    pub condition: String,
    #[serde(rename = "else")]
    pub otherwise: Option<bool>,
    pub next: String,
}

/// Raw wait for
#[derive(Debug, Deserialize)]
pub(crate) struct RawWaitFor {
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub timeout: Option<String>,
}

/// Raw step transition
#[derive(Debug, Deserialize)]
pub(crate) struct RawStepTransition {
    pub next: String,
    #[serde(default)]
    pub set: Option<HashMap<String, String>>,
}

/// Raw compensation step
#[derive(Debug, Deserialize)]
pub(crate) struct RawCompensationStep {
    pub name: String,
    #[serde(default)]
    pub condition: Option<String>,
    pub action: String,
    #[serde(default)]
    pub params: Option<HashMap<String, String>>,
}

impl From<RawStepCondition> for StepCondition {
    fn from(raw: RawStepCondition) -> Self {
        Self {
            condition: raw.condition,
            otherwise: raw.otherwise,
            next: raw.next,
        }
    }
}

impl From<RawWaitFor> for WaitFor {
    fn from(raw: RawWaitFor) -> Self {
        Self {
            event: raw.event,
            condition: raw.condition,
            timeout: raw.timeout,
        }
    }
}

impl From<RawStepTransition> for StepTransition {
    fn from(raw: RawStepTransition) -> Self {
        Self {
            next: raw.next,
            set: raw.set.unwrap_or_default(),
        }
    }
}

impl From<RawCompensationStep> for CompensationStep {
    fn from(raw: RawCompensationStep) -> Self {
        Self {
            name: raw.name,
            condition: raw.condition,
            action: raw.action,
            params: raw.params.unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_step_type() {
        assert_eq!(WorkflowStepType::Action, WorkflowStepType::Action);
        assert_eq!(WorkflowStepType::Terminal, WorkflowStepType::Terminal);
    }
}
