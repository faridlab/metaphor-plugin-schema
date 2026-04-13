//! Workflow YAML parser for .workflow.yaml files

use std::fs;
use std::path::Path;
use crate::webgen::ast::workflow::{
    WorkflowSchema, WorkflowTrigger, WorkflowConfig, WorkflowStep, WorkflowStepType,
    ContextVariable,
    RawWorkflowSchema, RawWorkflowStep,
};
use crate::webgen::{Error, Result};

/// Parser for workflow.yaml files
pub struct WorkflowParser;

impl WorkflowParser {
    /// Parse a single workflow.yaml file
    pub fn parse_file(path: &Path) -> Result<WorkflowSchema> {
        let content = fs::read_to_string(path)
            .map_err(|e| Error::Parse(format!("Failed to read {}: {}", path.display(), e)))?;

        Self::parse_content(&content, path)
    }

    /// Parse workflow schema from YAML content
    pub fn parse_content(content: &str, path: &Path) -> Result<WorkflowSchema> {
        let raw: RawWorkflowSchema = serde_yaml::from_str(content)
            .map_err(|e| Error::Parse(format!("Failed to parse YAML from {}: {}", path.display(), e)))?;

        let trigger = WorkflowTrigger {
            event: raw.trigger.event,
            extract: raw.trigger.extract,
        };

        let config = if let Some(raw_config) = raw.config {
            WorkflowConfig {
                timeout: raw_config.timeout,
                persistence: raw_config.persistence.unwrap_or(true),
            }
        } else {
            WorkflowConfig {
                timeout: None,
                persistence: true,
            }
        };

        let context = raw.context.unwrap_or_default()
            .into_iter()
            .map(|(name, value)| {
                let default_value = value.and_then(|v| v.as_str().map(|s| s.to_string()));
                let variable_type = None; // Could be inferred
                ContextVariable {
                    name,
                    default_value,
                    variable_type,
                }
            })
            .collect();

        let steps = raw.steps.into_iter()
            .map(Self::parse_workflow_step)
            .collect::<Result<Vec<_>>>()?;

        let compensation = raw.compensation.into_iter()
            .map(|c| c.into())
            .collect();

        Ok(WorkflowSchema {
            name: raw.name,
            description: raw.description,
            version: raw.version,
            trigger,
            config,
            context,
            steps,
            compensation,
        })
    }

    /// Parse a single workflow step
    fn parse_workflow_step(raw: RawWorkflowStep) -> Result<WorkflowStep> {
        let step_type = match raw.step_type.as_str() {
            "action" => WorkflowStepType::Action,
            "wait" => WorkflowStepType::Wait,
            "condition" => WorkflowStepType::Condition,
            "transition" => WorkflowStepType::Transition,
            "terminal" => WorkflowStepType::Terminal,
            _ => WorkflowStepType::Action, // Default
        };

        let wait_for = raw.wait_for.map(Into::into);
        let on_success = raw.on_success.map(Into::into);
        let on_failure = raw.on_failure.map(Into::into);
        let on_event = raw.on_event.map(Into::into);
        let on_timeout = raw.on_timeout.map(Into::into);

        let conditions = raw.conditions.map(|c| c.into_iter().map(Into::into).collect());

        Ok(WorkflowStep {
            name: raw.name,
            step_type,
            description: raw.description,
            action: raw.action,
            entity: raw.entity,
            params: raw.params.unwrap_or_default(),
            conditions,
            wait_for,
            on_success,
            on_failure,
            on_event,
            on_timeout,
        })
    }
}

/// Convenience function to parse a workflow file
pub fn parse_workflow_file(path: &Path) -> Result<WorkflowSchema> {
    WorkflowParser::parse_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_workflow_step_type() {
        let step_type = match "action" {
            "action" => WorkflowStepType::Action,
            "wait" => WorkflowStepType::Wait,
            "condition" => WorkflowStepType::Condition,
            "transition" => WorkflowStepType::Transition,
            "terminal" => WorkflowStepType::Terminal,
            _ => WorkflowStepType::Action,
        };
        assert_eq!(step_type, WorkflowStepType::Action);
    }
}
