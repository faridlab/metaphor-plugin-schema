//! Workflow AST definitions
//!
//! Defines AST nodes for workflow schemas including multi-entity orchestration,
//! saga pattern implementation, and business process management.
//!
//! Workflows orchestrate business processes across multiple entities and hooks,
//! implementing the Saga pattern for distributed transactions in a modular monolith.

use super::expressions::Expression;
use super::Span;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A workflow definition - multi-entity business process orchestration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Workflow {
    /// Workflow name (PascalCase)
    pub name: String,
    /// Workflow description
    pub description: Option<String>,
    /// Version number
    pub version: u32,
    /// Workflow trigger configuration
    pub trigger: WorkflowTrigger,
    /// Workflow configuration
    pub config: WorkflowConfig,
    /// Workflow context (variables available throughout workflow)
    pub context: HashMap<String, ContextVariable>,
    /// Workflow steps
    pub steps: Vec<Step>,
    /// Success handlers
    pub on_success: Vec<WorkflowHandler>,
    /// Failure handlers
    pub on_failure: Vec<WorkflowHandler>,
    /// Compensation steps (rollback on failure)
    pub compensation: Vec<CompensationStep>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl Workflow {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: 1,
            ..Default::default()
        }
    }

    /// Find a step by name
    pub fn find_step(&self, name: &str) -> Option<&Step> {
        self.steps.iter().find(|s| s.name == name)
    }

    /// Get all terminal steps
    pub fn terminal_steps(&self) -> Vec<&Step> {
        self.steps
            .iter()
            .filter(|s| matches!(s.step_type, StepType::Terminal { .. }))
            .collect()
    }

    /// Get the first step (entry point)
    pub fn entry_step(&self) -> Option<&Step> {
        self.steps.first()
    }
}

/// Workflow trigger configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowTrigger {
    /// Trigger on event
    pub event: Option<String>,
    /// Trigger on endpoint
    pub endpoint: Option<String>,
    /// Trigger on schedule (cron expression)
    pub schedule: Option<String>,
    /// Extract data from trigger
    pub extract: HashMap<String, String>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl WorkflowTrigger {
    pub fn event(name: impl Into<String>) -> Self {
        Self {
            event: Some(name.into()),
            ..Default::default()
        }
    }

    pub fn endpoint(path: impl Into<String>) -> Self {
        Self {
            endpoint: Some(path.into()),
            ..Default::default()
        }
    }

    pub fn schedule(cron: impl Into<String>) -> Self {
        Self {
            schedule: Some(cron.into()),
            ..Default::default()
        }
    }
}

/// Workflow configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowConfig {
    /// Maximum workflow duration
    pub timeout: Option<String>,
    /// Transaction mode
    pub transaction_mode: TransactionMode,
    /// Retry policy
    pub retry_policy: Option<RetryPolicy>,
    /// Action on timeout
    pub on_timeout: TimeoutAction,
    /// Whether to persist workflow state
    pub persistence: bool,
}

impl Default for WorkflowConfig {
    fn default() -> Self {
        Self {
            timeout: Some("24h".to_string()),
            transaction_mode: TransactionMode::Saga,
            retry_policy: None,
            on_timeout: TimeoutAction::Cancel,
            persistence: true,
        }
    }
}

/// Transaction mode for workflow execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionMode {
    /// All steps in single DB transaction (rollback on any failure)
    Atomic,
    /// Each step commits independently, uses compensation on failure
    #[default]
    Saga,
    /// Group atomic operations, saga between groups
    Hybrid,
}

/// Retry policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum retry attempts
    pub max_attempts: u32,
    /// Backoff strategy
    pub backoff: BackoffStrategy,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            backoff: BackoffStrategy::Exponential {
                initial: "1s".to_string(),
                max: "1m".to_string(),
            },
        }
    }
}

/// Backoff strategy for retries
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackoffStrategy {
    /// Fixed delay between retries
    Fixed(String),
    /// Linear increase (delay * attempt)
    Linear { initial: String },
    /// Exponential increase (delay * 2^attempt)
    Exponential { initial: String, max: String },
}

impl Default for BackoffStrategy {
    fn default() -> Self {
        Self::Exponential {
            initial: "1s".to_string(),
            max: "1m".to_string(),
        }
    }
}

/// Action on workflow timeout
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeoutAction {
    /// Cancel the workflow
    #[default]
    Cancel,
    /// Run compensation
    Compensate,
    /// Continue (mark as timed out but don't stop)
    Continue,
}

/// Context variable definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextVariable {
    /// Variable name
    pub name: String,
    /// Initial value (as expression or literal)
    pub initial_value: Option<Expression>,
    /// Variable type hint
    pub type_hint: Option<String>,
}

impl Default for ContextVariable {
    fn default() -> Self {
        Self {
            name: String::new(),
            initial_value: Some(Expression::Literal(super::expressions::Literal::Null)),
            type_hint: None,
        }
    }
}

/// A workflow step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    /// Step name (unique within workflow)
    pub name: String,
    /// Step type and configuration
    pub step_type: StepType,
    /// Condition to execute this step
    pub condition: Option<Expression>,
    /// Success handlers
    pub on_success: Option<StepOutcome>,
    /// Failure handlers
    pub on_failure: Option<StepFailure>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl Default for Step {
    fn default() -> Self {
        Self {
            name: String::new(),
            step_type: StepType::Action(ActionStep::default()),
            condition: None,
            on_success: None,
            on_failure: None,
            span: Span::default(),
        }
    }
}

impl Step {
    pub fn new(name: impl Into<String>, step_type: StepType) -> Self {
        Self {
            name: name.into(),
            step_type,
            ..Default::default()
        }
    }

    pub fn action(name: impl Into<String>, action: ActionStep) -> Self {
        Self::new(name, StepType::Action(action))
    }

    pub fn wait(name: impl Into<String>, wait: WaitStep) -> Self {
        Self::new(name, StepType::Wait(wait))
    }

    pub fn condition(name: impl Into<String>, cond: ConditionStep) -> Self {
        Self::new(name, StepType::Condition(cond))
    }

    pub fn parallel(name: impl Into<String>, parallel: ParallelStep) -> Self {
        Self::new(name, StepType::Parallel(parallel))
    }

    pub fn terminal(name: impl Into<String>, status: TerminalStatus) -> Self {
        Self::new(name, StepType::Terminal(TerminalStep { status, ..Default::default() }))
    }
}

/// Step type variants
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StepType {
    /// Execute an action
    Action(ActionStep),
    /// Wait for an event or condition
    Wait(WaitStep),
    /// Branch based on conditions
    Condition(ConditionStep),
    /// Execute multiple steps in parallel
    Parallel(ParallelStep),
    /// Iterate over a collection
    Loop(LoopStep),
    /// Call another workflow
    Subprocess(SubprocessStep),
    /// Wait for human input
    HumanTask(HumanTaskStep),
    /// Trigger a hook state transition
    Transition(TransitionStep),
    /// Group of steps in single transaction
    TransactionGroup(TransactionGroupStep),
    /// Terminal step (success/failure endpoint)
    Terminal(TerminalStep),
}

/// Action step - execute an action
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActionStep {
    /// Action name (e.g., "send_email", "create", "validate")
    pub action: String,
    /// Target entity (for CRUD actions)
    pub entity: Option<String>,
    /// Entity ID expression
    pub id: Option<Expression>,
    /// Action parameters
    pub params: HashMap<String, Expression>,
    /// Validation rules to apply
    pub rules: Vec<String>,
    /// Idempotency key expression
    pub idempotency_key: Option<Expression>,
    /// Compensation action
    pub compensation: Option<CompensationAction>,
}

impl ActionStep {
    pub fn new(action: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            ..Default::default()
        }
    }

    pub fn with_entity(mut self, entity: impl Into<String>) -> Self {
        self.entity = Some(entity.into());
        self
    }

    pub fn with_param(mut self, key: impl Into<String>, value: Expression) -> Self {
        self.params.insert(key.into(), value);
        self
    }
}

/// Compensation action for an action step
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompensationAction {
    /// Compensation action name
    pub action: String,
    /// Compensation parameters
    pub params: HashMap<String, Expression>,
}

/// Wait step - wait for event or timeout
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WaitStep {
    /// Event to wait for (single event mode)
    pub event: Option<String>,
    /// Multiple events to wait for (multi-event mode)
    pub events: Vec<WaitEvent>,
    /// Condition for event matching (single event mode)
    pub condition: Option<Expression>,
    /// Timeout duration
    pub timeout: Option<String>,
    /// Handler for event received (single event mode)
    pub on_event: Option<StepOutcome>,
    /// Handler for timeout
    pub on_timeout: Option<StepOutcome>,
}

/// Wait event configuration (for multi-event wait)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WaitEvent {
    /// Event name
    pub event: String,
    /// Condition for this event
    pub condition: Option<Expression>,
    /// Next step when this event is received
    pub next: Option<String>,
    /// Variables to set when this event is received
    pub set: HashMap<String, Expression>,
}

impl WaitStep {
    pub fn for_event(event: impl Into<String>) -> Self {
        Self {
            event: Some(event.into()),
            ..Default::default()
        }
    }

    pub fn with_timeout(mut self, timeout: impl Into<String>) -> Self {
        self.timeout = Some(timeout.into());
        self
    }
}

/// Condition step - branching logic
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConditionStep {
    /// Condition branches
    pub conditions: Vec<ConditionBranch>,
}

/// A condition branch
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConditionBranch {
    /// Condition expression (None for else branch)
    pub condition: Option<Expression>,
    /// Next step name
    pub next: String,
    /// Variables to set
    pub set: HashMap<String, Expression>,
}

/// Parallel step - execute branches concurrently
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParallelStep {
    /// Parallel branches
    pub branches: Vec<ParallelBranch>,
    /// Join strategy
    pub join: JoinStrategy,
    /// Handler when all/any branches complete
    pub on_complete: Option<StepOutcome>,
}

/// A parallel branch
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParallelBranch {
    /// Branch name
    pub name: String,
    /// Condition to execute this branch
    pub condition: Option<Expression>,
    /// Steps in this branch
    pub steps: Vec<Step>,
}

/// Join strategy for parallel execution
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinStrategy {
    /// Wait for all branches
    #[default]
    All,
    /// Continue when any branch completes
    Any,
    /// Continue when N of M branches complete
    NOfM { n: u32, m: u32 },
}

/// Loop step - iterate over collection
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoopStep {
    /// Collection expression
    pub foreach: Expression,
    /// Loop variable name
    pub as_var: String,
    /// Index variable name (optional)
    pub index_var: Option<String>,
    /// Steps to execute for each item
    pub steps: Vec<Step>,
    /// Handler when loop completes
    pub on_complete: Option<StepOutcome>,
}

/// Subprocess step - call another workflow
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubprocessStep {
    /// Workflow name to call
    pub workflow: String,
    /// Parameters to pass
    pub params: HashMap<String, Expression>,
    /// Whether to wait for completion
    pub wait: bool,
}

impl SubprocessStep {
    pub fn new(workflow: impl Into<String>) -> Self {
        Self {
            workflow: workflow.into(),
            wait: true,
            ..Default::default()
        }
    }
}

/// Human task step - wait for human input
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HumanTaskStep {
    /// Task configuration
    pub task: TaskConfig,
    /// Handler when task completes
    pub on_complete: Vec<ConditionBranch>,
    /// Handler for timeout
    pub on_timeout: Option<TaskTimeoutAction>,
}

/// Human task configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskConfig {
    /// Task title
    pub title: Expression,
    /// Task description
    pub description: Option<Expression>,
    /// Assignee (user ID expression)
    pub assignee: Option<Expression>,
    /// Assignee role
    pub assignee_role: Option<String>,
    /// Department filter
    pub department: Option<Expression>,
    /// Form fields
    pub form: Option<TaskForm>,
    /// Task timeout
    pub timeout: Option<String>,
    /// Reminder interval
    pub reminder: Option<String>,
}

/// Task form definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskForm {
    /// Form fields
    pub fields: Vec<TaskFormField>,
}

/// Task form field
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskFormField {
    /// Field name
    pub name: String,
    /// Field type
    pub field_type: String,
    /// Whether required
    pub required: bool,
    /// Default value
    pub default: Option<Expression>,
    /// Field label
    pub label: Option<String>,
    /// Validation rules
    pub validation: Vec<String>,
}

/// Action on task timeout
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTimeoutAction {
    /// Action type
    pub action: TaskTimeoutActionType,
    /// Parameters
    pub params: HashMap<String, Expression>,
    /// Next step
    pub next: Option<String>,
}

impl Default for TaskTimeoutAction {
    fn default() -> Self {
        Self {
            action: TaskTimeoutActionType::Cancel,
            params: HashMap::new(),
            next: None,
        }
    }
}

/// Task timeout action type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskTimeoutActionType {
    /// Cancel the task
    #[default]
    Cancel,
    /// Escalate to another user
    Escalate,
    /// Reassign to another user
    Reassign,
    /// Auto-approve
    AutoApprove,
    /// Auto-reject
    AutoReject,
}

/// Transition step - trigger hook state transition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransitionStep {
    /// Target entity type
    pub entity: String,
    /// Entity ID expression
    pub id: Expression,
    /// Transition name
    pub transition: String,
    /// Transition parameters
    pub params: HashMap<String, Expression>,
}

impl TransitionStep {
    pub fn new(entity: impl Into<String>, transition: impl Into<String>) -> Self {
        Self {
            entity: entity.into(),
            transition: transition.into(),
            ..Default::default()
        }
    }
}

/// Transaction group step - atomic step group
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransactionGroupStep {
    /// Steps in this transaction group
    pub steps: Vec<Step>,
}

/// Terminal step - workflow endpoint
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TerminalStep {
    /// Terminal status
    pub status: TerminalStatus,
    /// Reason for failure (if failed)
    pub reason: Option<Expression>,
    /// Events to emit
    pub emit: Option<EmitConfig>,
    /// Actions to execute
    pub actions: Vec<ActionStep>,
    /// Whether to trigger compensation
    pub compensate: bool,
}

/// Terminal status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalStatus {
    /// Workflow completed successfully
    #[default]
    Success,
    /// Workflow failed
    Failed,
    /// Workflow was cancelled
    Cancelled,
    /// Workflow timed out
    TimedOut,
}

/// Event emission configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmitConfig {
    /// Event name
    pub event: String,
    /// Event data
    pub data: HashMap<String, Expression>,
}

/// Step outcome - what happens after a step
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StepOutcome {
    /// Variables to set
    pub set: HashMap<String, Expression>,
    /// Next step name
    pub next: Option<String>,
    /// Log configuration
    pub log: Option<LogConfig>,
}

/// Log configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogConfig {
    /// Log level
    pub level: LogLevel,
    /// Log message expression
    pub message: Expression,
}

/// Log levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

/// Step failure handler
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StepFailure {
    /// Number of retries
    pub retry: Option<u32>,
    /// Backoff strategy
    pub backoff: Option<BackoffStrategy>,
    /// Handler when retries exhausted
    pub on_exhausted: Option<StepOutcome>,
    /// Next step on failure (without retry)
    pub next: Option<String>,
}

/// Compensation step - rollback action
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompensationStep {
    /// Compensation name
    pub name: Option<String>,
    /// Condition to execute this compensation
    pub condition: Option<Expression>,
    /// Compensation type
    pub compensation_type: CompensationType,
}

/// Compensation type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CompensationType {
    /// Simple action
    Action {
        action: String,
        entity: Option<String>,
        id: Option<Expression>,
        params: HashMap<String, Expression>,
        #[serde(rename = "where")]
        where_clause: Option<Expression>,
    },
    /// Loop compensation
    Loop {
        foreach: Expression,
        as_var: String,
        steps: Vec<CompensationStep>,
    },
    /// Transition compensation
    Transition {
        entity: String,
        id: Expression,
        transition: String,
        params: HashMap<String, Expression>,
    },
}

impl Default for CompensationType {
    fn default() -> Self {
        Self::Action {
            action: String::new(),
            entity: None,
            id: None,
            params: HashMap::new(),
            where_clause: None,
        }
    }
}

/// Workflow handler - success/failure handlers
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WorkflowHandler {
    /// Emit an event
    Emit { emit: String, data: Option<HashMap<String, Expression>> },
    /// Notify someone
    Notify { notify: String, message: Option<Expression> },
    /// Execute an action
    Action { action: String, params: HashMap<String, Expression> },
}

impl Default for WorkflowHandler {
    fn default() -> Self {
        Self::Emit { emit: String::new(), data: None }
    }
}
