//! State Machine AST for hook.yaml schema definitions

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Hook schema containing state machine, rules, permissions, triggers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookSchema {
    pub name: String,
    pub model: String,
    pub state_machine: Option<StateMachine>,
    pub rules: Vec<ValidationRule>,
    pub permissions: HashMap<String, PermissionSet>,
    pub triggers: Vec<Trigger>,
    pub computed_fields: Vec<ComputedField>,
}

/// Permission set for a role
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionSet {
    pub allow: Vec<PermissionRule>,
    pub deny: Vec<PermissionRule>,
}

/// State machine definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateMachine {
    pub state_field: String,
    pub states: HashMap<String, StateDefinition>,
    pub transitions: Vec<TransitionDefinition>,
}

/// State definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDefinition {
    pub name: String,
    pub is_initial: bool,
    pub is_final: bool,
    pub on_enter: Vec<String>,
    pub on_exit: Vec<String>,
}

/// Transition definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionDefinition {
    pub name: String,
    pub from_state: String, // Can be a single state or comma-separated for multiple
    pub to_state: String,
    pub roles: Vec<String>,
    pub condition: Option<String>,
    pub message: Option<String>,
    pub on_transition: Vec<String>,
}

impl TransitionDefinition {
    /// Get all possible source states for this transition
    pub fn from_states(&self) -> Vec<String> {
        self.from_state
            .split(',')
            .map(|s| s.trim().to_string())
            .collect()
    }
}

/// Validation rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRule {
    pub name: String,
    pub when: Vec<String>, // create, update, delete
    pub condition: String,
    pub message: String,
    pub code: String,
}

/// Permission rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub action: String,
    pub condition: Option<String>,
}

/// Trigger definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trigger {
    pub name: String,
    pub trigger_type: TriggerType,
    pub actions: Vec<TriggerAction>,
    pub condition: Option<String>,
}

/// Trigger type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TriggerType {
    AfterCreate,
    AfterUpdate,
    AfterDelete,
    BeforeCreate,
    BeforeUpdate,
    BeforeDelete,
    Scheduled(String), // cron expression
    OnEvent(String),
}

impl TriggerType {
    /// Get the trigger type as a string key
    pub fn as_key(&self) -> String {
        match self {
            Self::AfterCreate => "after_create".to_string(),
            Self::AfterUpdate => "after_update".to_string(),
            Self::AfterDelete => "after_delete".to_string(),
            Self::BeforeCreate => "before_create".to_string(),
            Self::BeforeUpdate => "before_update".to_string(),
            Self::BeforeDelete => "before_delete".to_string(),
            Self::Scheduled(cron) => format!("scheduled_{}", cron.replace(' ', "_")),
            Self::OnEvent(event) => format!("on_event_{}", event),
        }
    }
}

/// Trigger action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerAction {
    pub action_type: String,
    pub params: HashMap<String, String>,
}

/// Computed field definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputedField {
    pub name: String,
    pub expression: String,
}

// YAML parsing structures

/// Raw hook schema from YAML
#[derive(Debug, Deserialize)]
pub(crate) struct RawHookSchema {
    pub name: Option<String>,
    pub model: Option<String>,
    pub states: Option<RawStates>,
    pub rules: Option<HashMap<String, RawValidationRule>>,
    pub permissions: Option<HashMap<String, RawPermissionSet>>,
    pub triggers: Option<RawTriggers>,
    pub computed: Option<HashMap<String, String>>,
}

/// Raw states section
#[derive(Debug, Deserialize)]
pub(crate) struct RawStates {
    pub field: String,
    pub values: HashMap<String, RawStateValue>,
    pub transitions: Option<HashMap<String, RawTransition>>,
}

/// Raw state value
#[derive(Debug, Deserialize)]
pub(crate) struct RawStateValue {
    #[serde(default)]
    pub initial: bool,
    #[serde(default)]
    pub r#final: bool,
    pub on_enter: Option<Vec<String>>,
    pub on_exit: Option<Vec<String>>,
}

/// Raw transition
#[derive(Debug, Deserialize)]
pub(crate) struct RawTransition {
    #[serde(rename = "from")]
    pub from_state: FromState,
    #[serde(rename = "to")]
    pub to_state: String,
    pub roles: Option<Vec<String>>,
    pub condition: Option<String>,
    pub message: Option<String>,
}

/// Helper type to deserialize `from` field which can be a string or array
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum FromState {
    Single(String),
    Multiple(Vec<String>),
}

impl FromState {
    /// Convert to comma-separated string
    pub fn to_csv(&self) -> String {
        match self {
            FromState::Single(s) => s.clone(),
            FromState::Multiple(v) => v.join(","),
        }
    }
}

/// Raw validation rule
#[derive(Debug, Deserialize)]
pub(crate) struct RawValidationRule {
    #[serde(default)]
    pub when: Vec<String>,
    pub condition: String,
    pub message: String,
    pub code: String,
}

/// Raw permission set
#[derive(Debug, Deserialize)]
pub(crate) struct RawPermissionSet {
    #[serde(default)]
    pub allow: Vec<RawPermission>,
    #[serde(default)]
    pub deny: Vec<RawPermission>,
}

/// Raw permission rule
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RawPermission {
    pub action: String,
    pub condition: Option<String>,
}

impl From<RawPermission> for PermissionRule {
    fn from(raw: RawPermission) -> Self {
        Self {
            action: raw.action,
            condition: raw.condition,
        }
    }
}

/// Raw triggers section - flexible map of trigger name to actions
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct RawTriggers {
    // Triggers can be any key with RawTriggerActions value
    #[serde(flatten)]
    pub triggers: HashMap<String, RawTriggerActions>,
}

/// Raw trigger actions container
#[derive(Debug, Deserialize)]
pub(crate) struct RawTriggerActions {
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(rename = "if", default)]
    pub r#if: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trigger_type_key() {
        assert_eq!(TriggerType::AfterCreate.as_key(), "after_create");
        assert_eq!(TriggerType::OnEvent("UserCreated".to_string()).as_key(), "on_event_UserCreated");
    }
}
