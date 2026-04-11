//! Workflow schema resolver and validator
//!
//! Validates workflow definitions including:
//! - Step name uniqueness
//! - Step reference validity (next steps exist)
//! - Hook transition references
//! - Event references
//! - Unreachable step detection
//! - Terminal step presence

use crate::ast::workflow::{
    Workflow, Step, StepType, CompensationStep, CompensationType,
    ParallelStep, LoopStep, ConditionStep, TransactionGroupStep,
};
use crate::ast::ModuleSchema;
use super::ResolveError;
use std::collections::HashSet;

/// Workflow resolver for validating workflow schemas
pub struct FlowResolver<'a> {
    schema: &'a ModuleSchema,
}

impl<'a> FlowResolver<'a> {
    pub fn new(schema: &'a ModuleSchema) -> Self {
        Self { schema }
    }

    /// Resolve and validate all workflows in the schema
    pub fn resolve(&self) -> Result<(), Vec<ResolveError>> {
        let mut errors = Vec::new();

        for workflow in &self.schema.workflows {
            if let Err(workflow_errors) = self.validate_workflow(workflow) {
                errors.extend(workflow_errors);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Validate a single workflow
    fn validate_workflow(&self, workflow: &Workflow) -> Result<(), Vec<ResolveError>> {
        let mut errors = Vec::new();

        // Check for duplicate step names in the original list
        let mut seen_names = HashSet::new();
        for step in &workflow.steps {
            if !seen_names.insert(step.name.clone()) {
                errors.push(ResolveError::ValidationError {
                    message: format!("Duplicate step name '{}' in workflow '{}'", step.name, workflow.name),
                });
            }
        }

        // Collect all step names (including nested)
        let step_names = self.collect_step_names(&workflow.steps);

        // Validate each step
        for step in &workflow.steps {
            if let Err(step_errors) = self.validate_step(step, workflow, &step_names) {
                errors.extend(step_errors);
            }
        }

        // Check for unreachable steps
        if let Some(unreachable) = self.find_unreachable_steps(workflow, &step_names) {
            for step_name in unreachable {
                errors.push(ResolveError::ValidationError {
                    message: format!(
                        "Unreachable step '{}' in workflow '{}' - no step transitions to it",
                        step_name, workflow.name
                    ),
                });
            }
        }

        // Check for at least one terminal step
        if !self.has_terminal_step(workflow) {
            errors.push(ResolveError::ValidationError {
                message: format!(
                    "Workflow '{}' has no terminal step - workflows must have at least one terminal step",
                    workflow.name
                ),
            });
        }

        // Validate compensation steps
        for comp in &workflow.compensation {
            if let Err(comp_errors) = self.validate_compensation(comp, workflow) {
                errors.extend(comp_errors);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Validate a step
    fn validate_step(
        &self,
        step: &Step,
        workflow: &Workflow,
        valid_steps: &HashSet<String>,
    ) -> Result<(), Vec<ResolveError>> {
        let mut errors = Vec::new();

        // Validate step name
        if step.name.is_empty() {
            errors.push(ResolveError::ValidationError {
                message: format!("Step in flow '{}' has empty name", workflow.name),
            });
        }

        // Validate next step references in on_success
        if let Some(ref outcome) = step.on_success {
            if let Some(ref next) = outcome.next {
                if !valid_steps.contains(next) {
                    errors.push(ResolveError::ValidationError {
                        message: format!(
                            "Step '{}' in flow '{}' references non-existent step '{}' in on_success",
                            step.name, workflow.name, next
                        ),
                    });
                }
            }
        }

        // Validate next step references in on_failure
        if let Some(ref failure) = step.on_failure {
            if let Some(ref next) = failure.next {
                if !valid_steps.contains(next) {
                    errors.push(ResolveError::ValidationError {
                        message: format!(
                            "Step '{}' in flow '{}' references non-existent step '{}' in on_failure",
                            step.name, workflow.name, next
                        ),
                    });
                }
            }
            if let Some(ref exhausted) = failure.on_exhausted {
                if let Some(ref next) = exhausted.next {
                    if !valid_steps.contains(next) {
                        errors.push(ResolveError::ValidationError {
                            message: format!(
                                "Step '{}' in flow '{}' references non-existent step '{}' in on_exhausted",
                                step.name, workflow.name, next
                            ),
                        });
                    }
                }
            }
        }

        // Validate step type specific references
        match &step.step_type {
            StepType::Condition(cond) => {
                errors.extend(self.validate_condition_step(cond, workflow, valid_steps));
            }
            StepType::Parallel(parallel) => {
                errors.extend(self.validate_parallel_step(parallel, workflow, valid_steps));
            }
            StepType::Loop(loop_step) => {
                errors.extend(self.validate_loop_step(loop_step, workflow, valid_steps));
            }
            StepType::Wait(wait) => {
                // Validate single event mode
                if let Some(ref on_event) = wait.on_event {
                    if let Some(ref next) = on_event.next {
                        if !valid_steps.contains(next) {
                            errors.push(ResolveError::ValidationError {
                                message: format!(
                                    "Step '{}' in flow '{}' references non-existent step '{}' in on_event",
                                    step.name, workflow.name, next
                                ),
                            });
                        }
                    }
                }
                // Validate multi-event mode
                for event in &wait.events {
                    if let Some(ref next) = event.next {
                        if !valid_steps.contains(next) {
                            errors.push(ResolveError::ValidationError {
                                message: format!(
                                    "Step '{}' in flow '{}' references non-existent step '{}' in wait_for.events",
                                    step.name, workflow.name, next
                                ),
                            });
                        }
                    }
                }
                if let Some(ref on_timeout) = wait.on_timeout {
                    if let Some(ref next) = on_timeout.next {
                        if !valid_steps.contains(next) {
                            errors.push(ResolveError::ValidationError {
                                message: format!(
                                    "Step '{}' in flow '{}' references non-existent step '{}' in on_timeout",
                                    step.name, workflow.name, next
                                ),
                            });
                        }
                    }
                }
            }
            StepType::Subprocess(subprocess) => {
                // Validate that referenced workflow exists
                let workflow_exists = self.schema.workflows.iter().any(|w| w.name == subprocess.workflow);
                if !workflow_exists && !subprocess.workflow.is_empty() {
                    // Note: This might be a cross-module reference, so we just warn
                    // In a full implementation, we'd check external imports
                }
            }
            StepType::Transition(transition) => {
                // Validate that referenced entity exists
                let entity_exists = self.schema.models.iter().any(|m| m.name == transition.entity);
                if !entity_exists && !transition.entity.is_empty() {
                    errors.push(ResolveError::ValidationError {
                        message: format!(
                            "Step '{}' in flow '{}' references non-existent entity '{}'",
                            step.name, workflow.name, transition.entity
                        ),
                    });
                }

                // Validate that referenced transition exists in hook
                if entity_exists {
                    let hook = self.schema.hooks.iter().find(|h| h.model_ref == transition.entity);
                    if let Some(h) = hook {
                        if let Some(ref sm) = h.state_machine {
                            let transition_exists = sm.transitions.iter()
                                .any(|t| t.name == transition.transition);
                            if !transition_exists && !transition.transition.is_empty() {
                                errors.push(ResolveError::ValidationError {
                                    message: format!(
                                        "Step '{}' in workflow '{}' references non-existent transition '{}' for entity '{}'",
                                        step.name, workflow.name, transition.transition, transition.entity
                                    ),
                                });
                            }
                        }
                    }
                }
            }
            StepType::HumanTask(task) => {
                // Validate human task branches
                for branch in &task.on_complete {
                    if !branch.next.is_empty() && !valid_steps.contains(&branch.next) {
                        errors.push(ResolveError::ValidationError {
                            message: format!(
                                "Step '{}' in flow '{}' references non-existent step '{}' in on_complete",
                                step.name, workflow.name, branch.next
                            ),
                        });
                    }
                }
                if let Some(ref timeout) = task.on_timeout {
                    if let Some(ref next) = timeout.next {
                        if !valid_steps.contains(next) {
                            errors.push(ResolveError::ValidationError {
                                message: format!(
                                    "Step '{}' in flow '{}' references non-existent step '{}' in on_timeout",
                                    step.name, workflow.name, next
                                ),
                            });
                        }
                    }
                }
            }
            StepType::TransactionGroup(group) => {
                errors.extend(self.validate_transaction_group(group, workflow, valid_steps));
            }
            _ => {}
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Validate condition step
    fn validate_condition_step(
        &self,
        cond: &ConditionStep,
        workflow: &Workflow,
        valid_steps: &HashSet<String>,
    ) -> Vec<ResolveError> {
        let mut errors = Vec::new();

        for branch in &cond.conditions {
            if !branch.next.is_empty() && !valid_steps.contains(&branch.next) {
                errors.push(ResolveError::ValidationError {
                    message: format!(
                        "Condition branch in flow '{}' references non-existent step '{}'",
                        workflow.name, branch.next
                    ),
                });
            }
        }

        // Check that at least one branch has a condition or is an else branch
        if cond.conditions.is_empty() {
            errors.push(ResolveError::ValidationError {
                message: format!(
                    "Condition step in flow '{}' has no branches",
                    workflow.name
                ),
            });
        }

        errors
    }

    /// Validate parallel step
    fn validate_parallel_step(
        &self,
        parallel: &ParallelStep,
        workflow: &Workflow,
        valid_steps: &HashSet<String>,
    ) -> Vec<ResolveError> {
        let mut errors = Vec::new();

        if parallel.branches.is_empty() {
            errors.push(ResolveError::ValidationError {
                message: format!(
                    "Parallel step in workflow '{}' has no branches",
                    workflow.name
                ),
            });
        }

        // Validate nested steps in branches
        for branch in &parallel.branches {
            for nested_step in &branch.steps {
                // Collect names from nested steps
                let nested_names = self.collect_step_names(&branch.steps);
                let combined_steps: HashSet<_> = valid_steps.union(&nested_names).cloned().collect();

                if let Err(nested_errors) = self.validate_step(nested_step, workflow, &combined_steps) {
                    errors.extend(nested_errors);
                }
            }
        }

        if let Some(ref on_complete) = parallel.on_complete {
            if let Some(ref next) = on_complete.next {
                if !valid_steps.contains(next) {
                    errors.push(ResolveError::ValidationError {
                        message: format!(
                            "Parallel step in workflow '{}' references non-existent step '{}' in on_complete",
                            workflow.name, next
                        ),
                    });
                }
            }
        }

        errors
    }

    /// Validate loop step
    fn validate_loop_step(
        &self,
        loop_step: &LoopStep,
        workflow: &Workflow,
        valid_steps: &HashSet<String>,
    ) -> Vec<ResolveError> {
        let mut errors = Vec::new();

        if loop_step.as_var.is_empty() {
            errors.push(ResolveError::ValidationError {
                message: format!(
                    "Loop step in workflow '{}' has no loop variable defined",
                    workflow.name
                ),
            });
        }

        // Validate nested steps
        let nested_names = self.collect_step_names(&loop_step.steps);
        let combined_steps: HashSet<_> = valid_steps.union(&nested_names).cloned().collect();

        for nested_step in &loop_step.steps {
            if let Err(nested_errors) = self.validate_step(nested_step, workflow, &combined_steps) {
                errors.extend(nested_errors);
            }
        }

        if let Some(ref on_complete) = loop_step.on_complete {
            if let Some(ref next) = on_complete.next {
                if !valid_steps.contains(next) {
                    errors.push(ResolveError::ValidationError {
                        message: format!(
                            "Loop step in workflow '{}' references non-existent step '{}' in on_complete",
                            workflow.name, next
                        ),
                    });
                }
            }
        }

        errors
    }

    /// Validate transaction group
    fn validate_transaction_group(
        &self,
        group: &TransactionGroupStep,
        workflow: &Workflow,
        valid_steps: &HashSet<String>,
    ) -> Vec<ResolveError> {
        let mut errors = Vec::new();

        if group.steps.is_empty() {
            errors.push(ResolveError::ValidationError {
                message: format!(
                    "Transaction group in workflow '{}' has no steps",
                    workflow.name
                ),
            });
        }

        // Validate nested steps
        let nested_names = self.collect_step_names(&group.steps);
        let combined_steps: HashSet<_> = valid_steps.union(&nested_names).cloned().collect();

        for nested_step in &group.steps {
            if let Err(nested_errors) = self.validate_step(nested_step, workflow, &combined_steps) {
                errors.extend(nested_errors);
            }
        }

        errors
    }

    /// Validate compensation step
    fn validate_compensation(
        &self,
        comp: &CompensationStep,
        workflow: &Workflow,
    ) -> Result<(), Vec<ResolveError>> {
        let mut errors = Vec::new();

        match &comp.compensation_type {
            CompensationType::Action { entity, .. } => {
                if let Some(entity_name) = entity {
                    let entity_exists = self.schema.models.iter().any(|m| m.name == *entity_name);
                    if !entity_exists && !entity_name.is_empty() {
                        errors.push(ResolveError::ValidationError {
                            message: format!(
                                "Compensation in workflow '{}' references non-existent entity '{}'",
                                workflow.name, entity_name
                            ),
                        });
                    }
                }
            }
            CompensationType::Transition { entity, transition, .. } => {
                let entity_exists = self.schema.models.iter().any(|m| m.name == *entity);
                if !entity_exists && !entity.is_empty() {
                    errors.push(ResolveError::ValidationError {
                        message: format!(
                            "Compensation transition in workflow '{}' references non-existent entity '{}'",
                            workflow.name, entity
                        ),
                    });
                }

                // Validate transition exists in hook
                if entity_exists {
                    let hook = self.schema.hooks.iter().find(|h| h.model_ref == *entity);
                    if let Some(h) = hook {
                        if let Some(ref sm) = h.state_machine {
                            let transition_exists = sm.transitions.iter()
                                .any(|t| t.name == *transition);
                            if !transition_exists && !transition.is_empty() {
                                errors.push(ResolveError::ValidationError {
                                    message: format!(
                                        "Compensation in workflow '{}' references non-existent transition '{}' for entity '{}'",
                                        workflow.name, transition, entity
                                    ),
                                });
                            }
                        }
                    }
                }
            }
            CompensationType::Loop { steps, .. } => {
                for nested_comp in steps {
                    if let Err(nested_errors) = self.validate_compensation(nested_comp, workflow) {
                        errors.extend(nested_errors);
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Collect all step names from a list of steps (including nested steps)
    fn collect_step_names(&self, steps: &[Step]) -> HashSet<String> {
        let mut names = HashSet::new();

        for step in steps {
            names.insert(step.name.clone());

            // Collect from nested steps
            match &step.step_type {
                StepType::Parallel(parallel) => {
                    for branch in &parallel.branches {
                        names.extend(self.collect_step_names(&branch.steps));
                    }
                }
                StepType::Loop(loop_step) => {
                    names.extend(self.collect_step_names(&loop_step.steps));
                }
                StepType::TransactionGroup(group) => {
                    names.extend(self.collect_step_names(&group.steps));
                }
                _ => {}
            }
        }

        names
    }

    /// Check if workflow has at least one terminal step
    fn has_terminal_step(&self, workflow: &Workflow) -> bool {
        self.steps_have_terminal(&workflow.steps)
    }

    fn steps_have_terminal(&self, steps: &[Step]) -> bool {
        for step in steps {
            if matches!(step.step_type, StepType::Terminal(_)) {
                return true;
            }

            // Check nested steps
            match &step.step_type {
                StepType::Parallel(parallel) => {
                    for branch in &parallel.branches {
                        if self.steps_have_terminal(&branch.steps) {
                            return true;
                        }
                    }
                }
                StepType::Loop(loop_step) => {
                    if self.steps_have_terminal(&loop_step.steps) {
                        return true;
                    }
                }
                StepType::TransactionGroup(group) => {
                    if self.steps_have_terminal(&group.steps) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Find unreachable steps (steps that no other step transitions to)
    fn find_unreachable_steps(&self, workflow: &Workflow, all_steps: &HashSet<String>) -> Option<Vec<String>> {
        if workflow.steps.is_empty() {
            return None;
        }

        // The first step is always reachable (entry point)
        let mut reachable = HashSet::new();
        if let Some(first) = workflow.steps.first() {
            reachable.insert(first.name.clone());
        }

        // Find all steps that are referenced as "next"
        for step in &workflow.steps {
            self.collect_reachable_from_step(step, &mut reachable);
        }

        // Find steps that are not reachable
        let unreachable: Vec<_> = all_steps
            .iter()
            .filter(|name| !reachable.contains(*name))
            .cloned()
            .collect();

        if unreachable.is_empty() {
            None
        } else {
            Some(unreachable)
        }
    }

    fn collect_reachable_from_step(&self, step: &Step, reachable: &mut HashSet<String>) {
        // From on_success
        if let Some(ref outcome) = step.on_success {
            if let Some(ref next) = outcome.next {
                reachable.insert(next.clone());
            }
        }

        // From on_failure
        if let Some(ref failure) = step.on_failure {
            if let Some(ref next) = failure.next {
                reachable.insert(next.clone());
            }
            if let Some(ref exhausted) = failure.on_exhausted {
                if let Some(ref next) = exhausted.next {
                    reachable.insert(next.clone());
                }
            }
        }

        // From step type specific transitions
        match &step.step_type {
            StepType::Condition(cond) => {
                for branch in &cond.conditions {
                    if !branch.next.is_empty() {
                        reachable.insert(branch.next.clone());
                    }
                }
            }
            StepType::Parallel(parallel) => {
                if let Some(ref on_complete) = parallel.on_complete {
                    if let Some(ref next) = on_complete.next {
                        reachable.insert(next.clone());
                    }
                }
                // The first step in each parallel branch is reachable
                for branch in &parallel.branches {
                    if let Some(first) = branch.steps.first() {
                        reachable.insert(first.name.clone());
                    }
                    for nested in &branch.steps {
                        self.collect_reachable_from_step(nested, reachable);
                    }
                }
            }
            StepType::Loop(loop_step) => {
                if let Some(ref on_complete) = loop_step.on_complete {
                    if let Some(ref next) = on_complete.next {
                        reachable.insert(next.clone());
                    }
                }
                // The first step in a loop is always reachable (entry point)
                if let Some(first) = loop_step.steps.first() {
                    reachable.insert(first.name.clone());
                }
                for nested in &loop_step.steps {
                    self.collect_reachable_from_step(nested, reachable);
                }
            }
            StepType::Wait(wait) => {
                // Single event mode
                if let Some(ref on_event) = wait.on_event {
                    if let Some(ref next) = on_event.next {
                        reachable.insert(next.clone());
                    }
                }
                // Multi-event mode - collect all next steps from events array
                for event in &wait.events {
                    if let Some(ref next) = event.next {
                        reachable.insert(next.clone());
                    }
                }
                if let Some(ref on_timeout) = wait.on_timeout {
                    if let Some(ref next) = on_timeout.next {
                        reachable.insert(next.clone());
                    }
                }
            }
            StepType::HumanTask(task) => {
                for branch in &task.on_complete {
                    if !branch.next.is_empty() {
                        reachable.insert(branch.next.clone());
                    }
                }
                if let Some(ref timeout) = task.on_timeout {
                    if let Some(ref next) = timeout.next {
                        reachable.insert(next.clone());
                    }
                }
            }
            StepType::TransactionGroup(group) => {
                // The first step in a transaction group is reachable
                if let Some(first) = group.steps.first() {
                    reachable.insert(first.name.clone());
                }
                for nested in &group.steps {
                    self.collect_reachable_from_step(nested, reachable);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::workflow::*;

    fn create_test_schema_with_workflow(workflow: Workflow) -> ModuleSchema {
        ModuleSchema {
            name: "test".to_string(),
            workflows: vec![workflow],
            ..Default::default()
        }
    }

    #[test]
    fn test_valid_simple_workflow() {
        let workflow = Workflow {
            name: "TestWorkflow".to_string(),
            steps: vec![
                Step {
                    name: "start".to_string(),
                    step_type: StepType::Action(ActionStep {
                        action: "do_something".to_string(),
                        ..Default::default()
                    }),
                    on_success: Some(StepOutcome {
                        next: Some("end".to_string()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                Step {
                    name: "end".to_string(),
                    step_type: StepType::Terminal(TerminalStep {
                        status: TerminalStatus::Success,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let schema = create_test_schema_with_workflow(workflow);
        let resolver = FlowResolver::new(&schema);
        assert!(resolver.resolve().is_ok());
    }

    #[test]
    fn test_duplicate_step_names() {
        let workflow = Workflow {
            name: "TestWorkflow".to_string(),
            steps: vec![
                Step {
                    name: "step1".to_string(),
                    step_type: StepType::Action(ActionStep::default()),
                    ..Default::default()
                },
                Step {
                    name: "step1".to_string(), // Duplicate!
                    step_type: StepType::Terminal(TerminalStep::default()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let schema = create_test_schema_with_workflow(workflow);
        let resolver = FlowResolver::new(&schema);
        let result = resolver.resolve();
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| e.to_string().contains("Duplicate step name")));
    }

    #[test]
    fn test_invalid_step_reference() {
        let workflow = Workflow {
            name: "TestWorkflow".to_string(),
            steps: vec![
                Step {
                    name: "start".to_string(),
                    step_type: StepType::Action(ActionStep::default()),
                    on_success: Some(StepOutcome {
                        next: Some("nonexistent".to_string()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                Step {
                    name: "end".to_string(),
                    step_type: StepType::Terminal(TerminalStep::default()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let schema = create_test_schema_with_workflow(workflow);
        let resolver = FlowResolver::new(&schema);
        let result = resolver.resolve();
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| e.to_string().contains("non-existent step")));
    }

    #[test]
    fn test_no_terminal_step() {
        let workflow = Workflow {
            name: "TestWorkflow".to_string(),
            steps: vec![
                Step {
                    name: "start".to_string(),
                    step_type: StepType::Action(ActionStep::default()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let schema = create_test_schema_with_workflow(workflow);
        let resolver = FlowResolver::new(&schema);
        let result = resolver.resolve();
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| e.to_string().contains("no terminal step")));
    }
}
