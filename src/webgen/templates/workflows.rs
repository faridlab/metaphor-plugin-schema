//! Workflow UI templates for generating React components

use crate::webgen::ast::workflow::{WorkflowSchema, WorkflowStep};
use crate::webgen::parser::to_pascal_case;

/// Workflow component templates
pub struct WorkflowTemplates;

impl WorkflowTemplates {
    /// Generate workflow tracker component
    pub fn generate_workflow_tracker(workflow: &WorkflowSchema) -> String {
        let workflow_pascal = to_pascal_case(&workflow.name);
        let workflow_snake = crate::webgen::parser::to_snake_case(&workflow.name);
        let description = workflow.description.lines().next().unwrap_or("Workflow automation");

        let hooks = Self::generate_workflow_hooks(workflow);
        let controls = Self::generate_workflow_controls(workflow);
        let stepper_items = Self::generate_stepper_items(workflow);

        // Build using string concatenation to avoid format! escaping issues
        let mut result = String::from(r#"import { useMemo } from 'react';
import { Box, Typography, Stepper, Step, StepLabel, StepContent, Button, Sheet, Alert, CircularProgress } from '@/components/ui';
import { CheckCircle, Schedule, Error } from '@/components/ui';

"#);
        result.push_str(&hooks);
        result.push_str(r#"
/**
 * "#);
        result.push_str(&workflow_pascal);
        result.push_str(r#" Workflow Tracker Component
 *
 * Generated workflow tracker for "#);
        result.push_str(&workflow.name);
        result.push_str(r#"
 */
export interface "#);
        result.push_str(&workflow_snake);
        result.push_str(r#"TrackerProps {
  workflowId: string;
  onComplete?: (result: "#);
        result.push_str(&workflow_pascal);
        result.push_str(r#"Result) => void;
  onError?: (error: Error) => void;
}

export function "#);
        result.push_str(&workflow_pascal);
        result.push_str(r#"Tracker({ workflowId, onComplete, onError }: "#);
        result.push_str(&workflow_snake);
        result.push_str(r#"TrackerProps) {
  const { workflow, currentStep, stepStatuses, isLoading, error, cancelWorkflow, retryStep } = use"#);
        result.push_str(&workflow_pascal);
        result.push_str(r#"Workflow(workflowId);

  const activeStep = useMemo(() => {
    return workflow?.steps.findIndex(step => step.name === currentStep?.name) ?? -1;
  }, [workflow, currentStep]);

  if (isLoading) {
    return (
      <Box display="flex" justifyContent="center" alignItems="center" minHeight={200}>
        <CircularProgress />
      </Box>
    );
  }

  if (error) {
    return (
      <Alert severity="error">
        Failed to load workflow: {error.message}
      </Alert>
    );
  }

  return (
    <Box sx={{ maxWidth: 800, margin: '0 auto' }}>
      <Typography variant="h5" gutterBottom>
        "#);
        result.push_str(&workflow_pascal);
        result.push_str(r#" Workflow
      </Typography>
      <Typography variant="body2" color="text.secondary" paragraph>
        "#);
        result.push_str(description);
        result.push_str(r#"
      </Typography>

"#);
        result.push_str(&controls);
        result.push_str(r#"
      <Stepper activeStep={activeStep} orientation="vertical">
        "#);
        result.push_str(&stepper_items);
        result.push_str(r#"
      </Stepper>
    </Box>
  );
}

// <<< CUSTOM: Add custom workflow UI elements here
// END CUSTOM
"#);
        result
    }

    /// Generate stepper items for workflow steps
    fn generate_stepper_items(workflow: &WorkflowSchema) -> String {
        let mut items = String::new();
        let empty_desc = String::from("");

        for step in &workflow.steps {
            let step_label = Self::step_label(step);
            let step_desc = step.description.as_ref().unwrap_or(&empty_desc);

            items.push_str(r#"        <Step key=""#);
            items.push_str(&step.name);
            items.push_str(r#"">
          <StepLabel>"#);
            items.push_str(&step_label);
            items.push_str(r#"</StepLabel>
          <StepContent>
            <Typography variant="body2" color="text.secondary">
              "#);
            items.push_str(step_desc);
            items.push_str(r#"
            </Typography>
          </StepContent>
        </Step>
"#);
        }

        items
    }

    /// Generate step label
    fn step_label(step: &WorkflowStep) -> String {
        to_pascal_case(&step.name)
    }

    /// Generate workflow hooks
    fn generate_workflow_hooks(workflow: &WorkflowSchema) -> String {
        let workflow_pascal = to_pascal_case(&workflow.name);
        let workflow_snake = crate::webgen::parser::to_snake_case(&workflow.name);

        let mut result = String::from(r#"import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';

/**
 * Hook for managing "#);
        result.push_str(&workflow_pascal);
        result.push_str(r#" workflow state
 */
export function use"#);
        result.push_str(&workflow_pascal);
        result.push_str(r#"Workflow(workflowId: string) {
  const queryClient = useQueryClient();

  // Query workflow status
  const { data: workflow, isLoading, error } = useQuery({
    queryKey: ['"#);
        result.push_str(&workflow_snake);
        result.push_str(r#"', workflowId],
    queryFn: () => fetchWorkflowStatus(workflowId),
    refetchInterval: (data) => {
      // Auto-poll while workflow is in progress
      return data?.status === 'in_progress' ? 2000 : false;
    },
  });

  // Cancel workflow mutation
  const cancelMutation = useMutation({
    mutationFn: () => cancelWorkflow(workflowId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['"#);
        result.push_str(&workflow_snake);
        result.push_str(r#"', workflowId] });
    },
  });

  const currentStep = workflow?.currentStep;
  const stepStatuses = workflow?.steps?.reduce((acc, step) => {
    acc[step.name] = step.status;
    return acc;
  }, {}) ?? {};

  return {
    workflow,
    currentStep,
    stepStatuses,
    isLoading,
    error,
    cancelWorkflow: () => cancelMutation.mutate(),
  };
}

// TODO: Implement fetchWorkflowStatus and cancelWorkflow
async function fetchWorkflowStatus(workflowId: string) {
  // API call implementation
}

async function cancelWorkflow(workflowId: string) {
  // API call implementation
}
"#);
        result
    }

    /// Generate workflow control buttons
    fn generate_workflow_controls(_workflow: &WorkflowSchema) -> String {
        String::from(r#"      <Box sx={{ mb: 3, display: 'flex', gap: 1 }}>
        <Button
          variant="outlined"
          color="error"
          onClick={cancelWorkflow}
          disabled={!workflow || workflow.status === 'completed' || workflow.status === 'failed'}
        >
          Cancel Workflow
        </Button>
      </Box>"#)
    }

    /// Generate workflow API hooks file
    pub fn generate_workflow_api(workflow: &WorkflowSchema) -> String {
        let workflow_pascal = to_pascal_case(&workflow.name);
        let workflow_snake = crate::webgen::parser::to_snake_case(&workflow.name);

        let mut result = String::from(r#"import { useQuery } from '@tanstack/react-query';

/**
 * API hooks for "#);
        result.push_str(&workflow_pascal);
        result.push_str(r#" workflow
 */

export function use"#);
        result.push_str(&workflow_pascal);
        result.push_str(r#"WorkflowApi() {
  return {
    getWorkflowStatus: (workflowId: string) => {
      return fetch(`/api/workflows/"#);
        result.push_str(&workflow_snake);
        result.push_str(r#"/${workflowId}`).then(r => r.json());
    },
    cancelWorkflow: (workflowId: string) => {
      return fetch(`/api/workflows/"#);
        result.push_str(&workflow_snake);
        result.push_str(r#"/${workflowId}/cancel`, { method: 'POST' }).then(r => r.json());
    },
    startWorkflow: (input: "#);
        result.push_str(&workflow_pascal);
        result.push_str(r#"Input) => {
      return fetch(`/api/workflows/"#);
        result.push_str(&workflow_snake);
        result.push_str(r#"`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(input),
      }).then(r => r.json());
    },
  };
}

export interface "#);
        result.push_str(&workflow_pascal);
        result.push_str(r#"Input {
  // TODO: Define workflow input based on trigger context
  [key: string]: any;
}

export interface "#);
        result.push_str(&workflow_pascal);
        result.push_str(r#"Result {
  workflowId: string;
  status: 'completed' | 'failed' | 'expired';
  result?: any;
}

// <<< CUSTOM: Add custom API methods here
// END CUSTOM
"#);
        result
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_step_label_generation() {
        // Test step label generation
    }
}
