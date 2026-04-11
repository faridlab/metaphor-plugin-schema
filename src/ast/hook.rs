//! Hook AST definitions
//!
//! Defines AST nodes for entity lifecycle hooks including state machines, rules,
//! permissions, triggers, and computed fields.

use super::expressions::Expression;
use super::Span;
use serde::{Deserialize, Serialize};

/// An entity hook definition (lifecycle behaviors for a model)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Hook {
    /// Hook name (usually matches model name)
    pub name: String,
    /// Reference to the model this hook is for
    pub model_ref: String,
    /// State machine definition
    pub state_machine: Option<StateMachine>,
    /// Validation rules
    pub rules: Vec<Rule>,
    /// Permission definitions
    pub permissions: Vec<Permission>,
    /// Trigger definitions
    pub triggers: Vec<Trigger>,
    /// Computed field definitions
    pub computed_fields: Vec<ComputedField>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl Hook {
    pub fn new(name: impl Into<String>, model_ref: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            model_ref: model_ref.into(),
            ..Default::default()
        }
    }
}

/// State machine definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StateMachine {
    /// The field that holds the state
    pub field: String,
    /// Available states
    pub states: Vec<State>,
    /// State transitions
    pub transitions: Vec<Transition>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl StateMachine {
    /// Get the initial state
    pub fn initial_state(&self) -> Option<&State> {
        self.states.iter().find(|s| s.initial)
    }

    /// Find a state by name
    pub fn find_state(&self, name: &str) -> Option<&State> {
        self.states.iter().find(|s| s.name == name)
    }

    /// Get all transitions from a given state
    pub fn transitions_from(&self, state: &str) -> Vec<&Transition> {
        self.transitions
            .iter()
            .filter(|t| t.from.contains(&state.to_string()) || t.from.contains(&"*".to_string()))
            .collect()
    }
}

/// A state in the state machine
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    /// State name
    pub name: String,
    /// Whether this is the initial state
    pub initial: bool,
    /// Whether this is a final state
    pub final_state: bool,
    /// On-enter actions
    pub on_enter: Vec<Action>,
    /// On-exit actions
    pub on_exit: Vec<Action>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl State {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    pub fn initial(mut self) -> Self {
        self.initial = true;
        self
    }

    pub fn final_state(mut self) -> Self {
        self.final_state = true;
        self
    }
}

/// A state transition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Transition {
    /// Transition name (action name)
    pub name: String,
    /// Source states (can be multiple, or "*" for any)
    pub from: Vec<String>,
    /// Target state
    pub to: String,
    /// Roles allowed to perform this transition
    pub allowed_roles: Vec<String>,
    /// Guard condition
    pub guard: Option<Expression>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl Transition {
    pub fn new(name: impl Into<String>, from: Vec<String>, to: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            from,
            to: to.into(),
            ..Default::default()
        }
    }

    pub fn with_roles(mut self, roles: Vec<String>) -> Self {
        self.allowed_roles = roles;
        self
    }
}

/// A validation rule
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Rule {
    /// Rule name
    pub name: String,
    /// When to apply this rule (create, update, delete, or specific transitions)
    pub when: Vec<RuleWhen>,
    /// Condition that must be true for validation to pass
    pub condition: Expression,
    /// Error message when validation fails
    pub message: String,
    /// Error code (optional)
    pub code: Option<String>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

/// When a rule should be applied
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleWhen {
    /// On create
    Create,
    /// On update
    Update,
    /// On delete
    Delete,
    /// On specific state transition
    Transition(String),
    /// Always
    #[default]
    Always,
}

/// A permission definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Permission {
    /// Role name
    pub role: String,
    /// Allowed actions
    pub actions: Vec<PermissionAction>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl Permission {
    pub fn new(role: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            ..Default::default()
        }
    }

    pub fn with_action(mut self, action: PermissionAction) -> Self {
        self.actions.push(action);
        self
    }
}

/// A permission action
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionAction {
    /// Action type
    pub action: ActionType,
    /// Whether allowed or denied
    pub allowed: bool,
    /// Field restrictions (for read/update)
    pub fields: Option<FieldRestriction>,
    /// Condition for this permission
    pub condition: Option<Expression>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

/// Field restrictions for permissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FieldRestriction {
    /// Only these fields
    Only(Vec<String>),
    /// All except these fields
    Except(Vec<String>),
}

/// Action types for permissions
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    /// Create new records
    Create,
    /// Read records
    #[default]
    Read,
    /// Update records
    Update,
    /// Delete records
    Delete,
    /// List records
    List,
    /// Restore soft-deleted records
    Restore,
    /// All CRUD actions
    All,
    /// Custom action
    Custom(String),
}

impl ActionType {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "create" => Self::Create,
            "read" | "get" => Self::Read,
            "update" => Self::Update,
            "delete" => Self::Delete,
            "list" => Self::List,
            "restore" => Self::Restore,
            "all" | "*" => Self::All,
            other => Self::Custom(other.to_string()),
        }
    }
}

/// A trigger definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Trigger {
    /// Trigger event
    pub event: TriggerEvent,
    /// Actions to perform
    pub actions: Vec<Action>,
    /// Condition for this trigger
    pub condition: Option<Expression>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

/// Trigger events
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerEvent {
    /// Before creating a record
    BeforeCreate,
    /// After creating a record
    #[default]
    AfterCreate,
    /// Before updating a record
    BeforeUpdate,
    /// After updating a record
    AfterUpdate,
    /// Before deleting a record
    BeforeDelete,
    /// After deleting a record
    AfterDelete,
    /// On state transition
    OnTransition(String),
    /// On entering a state
    OnEnterState(String),
    /// On exiting a state
    OnExitState(String),
}

impl TriggerEvent {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "before_create" => Some(Self::BeforeCreate),
            "after_create" => Some(Self::AfterCreate),
            "before_update" => Some(Self::BeforeUpdate),
            "after_update" => Some(Self::AfterUpdate),
            "before_delete" => Some(Self::BeforeDelete),
            "after_delete" => Some(Self::AfterDelete),
            _ => None,
        }
    }
}

/// An action to perform in triggers or state hooks
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Action {
    /// Action type
    pub action_type: ActionKind,
    /// Action arguments
    pub args: Vec<Expression>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

/// Types of actions
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    /// Send an email
    SendEmail,
    /// Send a notification
    Notify,
    /// Call a webhook
    Webhook,
    /// Execute a function/handler
    #[default]
    Execute,
    /// Log an event
    Log,
    /// Emit an event
    Emit,
    /// Custom action
    Custom(String),
}

impl ActionKind {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "send_email" | "email" => Self::SendEmail,
            "notify" | "notification" => Self::Notify,
            "webhook" | "call" => Self::Webhook,
            "execute" | "exec" | "run" => Self::Execute,
            "log" => Self::Log,
            "emit" | "event" => Self::Emit,
            other => Self::Custom(other.to_string()),
        }
    }
}

/// A computed field definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComputedField {
    /// Field name
    pub name: String,
    /// Field type (inferred or explicit)
    pub field_type: Option<String>,
    /// Computation expression
    pub expression: Expression,
    /// Whether this is persisted to the database
    pub persisted: bool,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl ComputedField {
    pub fn new(name: impl Into<String>, expression: Expression) -> Self {
        Self {
            name: name.into(),
            expression,
            ..Default::default()
        }
    }

    pub fn persisted(mut self) -> Self {
        self.persisted = true;
        self
    }
}
