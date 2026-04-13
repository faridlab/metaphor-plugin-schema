//! Hook YAML parser for .hook.yaml files

use std::fs;
use std::path::Path;
use crate::webgen::ast::state_machine::{
    HookSchema, StateMachine, StateDefinition, TransitionDefinition,
    ValidationRule, PermissionSet, Trigger, TriggerType,
    TriggerAction, ComputedField,
    RawHookSchema, RawStates, RawValidationRule,
    RawPermissionSet, RawPermission, RawTriggers, RawTriggerActions,
};
use crate::webgen::{Error, Result};
use std::collections::HashMap;

/// Parser for hook.yaml files
pub struct HookParser;

impl HookParser {
    /// Parse a single hook.yaml file
    pub fn parse_file(path: &Path) -> Result<HookSchema> {
        let content = fs::read_to_string(path)
            .map_err(|e| Error::Parse(format!("Failed to read {}: {}", path.display(), e)))?;

        Self::parse_content(&content, path)
    }

    /// Parse hook schema from YAML content
    pub fn parse_content(content: &str, path: &Path) -> Result<HookSchema> {
        let raw: RawHookSchema = serde_yaml::from_str(content)
            .map_err(|e| Error::Parse(format!("Failed to parse YAML from {}: {}", path.display(), e)))?;

        // Use filename for name if not specified
        let name = raw.name.unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string()
        });

        let model = raw.model.unwrap_or_else(|| name.clone());

        let state_machine = raw.states.map(Self::parse_state_machine);
        let rules = Self::parse_validation_rules(&raw.rules);
        let permissions = Self::parse_permissions(&raw.permissions);
        let triggers = Self::parse_triggers(&raw.triggers);
        let computed_fields = Self::parse_computed_fields(&raw.computed);

        Ok(HookSchema {
            name,
            model,
            state_machine,
            rules,
            permissions,
            triggers,
            computed_fields,
        })
    }

    /// Parse state machine from states section
    fn parse_state_machine(raw: RawStates) -> StateMachine {
        let state_field = raw.field;
        let mut states = HashMap::new();
        let mut transitions = Vec::new();

        // Parse states
        for (name, value) in raw.values {
            states.insert(name.clone(), StateDefinition {
                name: name.clone(),
                is_initial: value.initial,
                is_final: value.r#final,
                on_enter: value.on_enter.unwrap_or_default(),
                on_exit: value.on_exit.unwrap_or_default(),
            });
        }

        // Parse transitions
        if let Some(raw_transitions) = raw.transitions {
            for (name, trans) in raw_transitions {
                transitions.push(TransitionDefinition {
                    name,
                    from_state: trans.from_state.to_csv(),
                    to_state: trans.to_state,
                    roles: trans.roles.unwrap_or_default(),
                    condition: trans.condition,
                    message: trans.message,
                    on_transition: Vec::new(), // Could be parsed if needed
                });
            }
        }

        StateMachine {
            state_field,
            states,
            transitions,
        }
    }

    /// Parse validation rules
    fn parse_validation_rules(rules: &Option<HashMap<String, RawValidationRule>>) -> Vec<ValidationRule> {
        rules.as_ref()
            .map(|map| map.values().map(|r| ValidationRule {
                name: String::new(), // Name from key
                when: r.when.clone(),
                condition: r.condition.clone(),
                message: r.message.clone(),
                code: r.code.clone(),
            }).collect())
            .unwrap_or_default()
    }

    /// Parse permissions
    fn parse_permissions(permissions: &Option<HashMap<String, RawPermissionSet>>) -> HashMap<String, PermissionSet> {
        permissions.as_ref()
            .map(|map| map.iter().map(|(role, set)| {
                let allow = set.allow.iter().map(|p| RawPermission::clone(p).into()).collect();
                let deny = set.deny.iter().map(|p| RawPermission::clone(p).into()).collect();
                (role.clone(), PermissionSet { allow, deny })
            }).collect())
            .unwrap_or_default()
    }

    /// Parse triggers - flexible parsing that handles any trigger name
    fn parse_triggers(triggers: &Option<RawTriggers>) -> Vec<Trigger> {
        let mut result = Vec::new();

        if let Some(raw) = triggers {
            for (trigger_name, raw_actions) in &raw.triggers {
                let trigger_type = Self::infer_trigger_type(trigger_name);
                result.push(Self::parse_trigger_actions(
                    trigger_type,
                    raw_actions,
                    trigger_name,
                ));
            }
        }

        result
    }

    /// Infer trigger type from trigger name
    fn infer_trigger_type(name: &str) -> TriggerType {
        let name_lower = name.to_lowercase();

        if name_lower.starts_with("after_create") {
            TriggerType::AfterCreate
        } else if name_lower.starts_with("after_update") {
            TriggerType::AfterUpdate
        } else if name_lower.starts_with("after_delete") {
            TriggerType::AfterDelete
        } else if name_lower.starts_with("before_create") {
            TriggerType::BeforeCreate
        } else if name_lower.starts_with("before_update") {
            TriggerType::BeforeUpdate
        } else if name_lower.starts_with("before_delete") {
            TriggerType::BeforeDelete
        } else if name_lower.starts_with("after_") {
            // Custom after event
            let event = name.strip_prefix("after_").unwrap_or(name);
            TriggerType::OnEvent(event.to_string())
        } else if name_lower.starts_with("before_") {
            // Custom before event
            let event = name.strip_prefix("before_").unwrap_or(name);
            TriggerType::OnEvent(event.to_string())
        } else {
            // Default to OnEvent for custom triggers
            TriggerType::OnEvent(name.to_string())
        }
    }

    /// Parse trigger actions from raw trigger actions
    fn parse_trigger_actions(trigger_type: TriggerType, raw: &RawTriggerActions, name: &str) -> Trigger {
        let actions = raw.actions.iter()
            .map(|a| Self::parse_action_string(a))
            .collect();

        Trigger {
            name: name.to_string(),
            trigger_type,
            actions,
            condition: raw.r#if.clone(),
        }
    }

    /// Parse a single action string into a TriggerAction
    fn parse_action_string(action_str: &str) -> TriggerAction {
        // Parse action strings like "send_email(...)", "emit: EventName", "log(message)"
        let mut params = HashMap::new();

        let (action_type, params_str) = if let Some(colon_pos) = action_str.find(':') {
            // Format: "action_type: params"
            let action_type = action_str[..colon_pos].trim().to_string();
            let params_str = action_str[colon_pos + 1..].trim().to_string();
            (action_type, params_str)
        } else if let Some(paren_pos) = action_str.find('(') {
            // Format: "function(...)"
            let action_type = action_str[..paren_pos].trim().to_string();
            let params_str = if action_str.ends_with(')') {
                action_str[paren_pos + 1..action_str.len() - 1].trim().to_string()
            } else {
                String::new()
            };
            (action_type, params_str)
        } else {
            // Simple action without params
            (action_str.trim().to_string(), String::new())
        };

        // Parse simple key-value pairs if present
        if !params_str.is_empty() {
            // Very basic parsing - could be enhanced
            for pair in params_str.split(',') {
                let pair = pair.trim();
                if let Some(eq_pos) = pair.find('=') {
                    let key = pair[..eq_pos].trim().to_string();
                    let value = pair[eq_pos + 1..].trim().to_string();
                    params.insert(key, value);
                }
            }
        }

        TriggerAction {
            action_type,
            params,
        }
    }

    /// Parse computed fields
    fn parse_computed_fields(computed: &Option<HashMap<String, String>>) -> Vec<ComputedField> {
        computed.as_ref()
            .map(|map| map.iter().map(|(name, expr)| ComputedField {
                name: name.clone(),
                expression: expr.clone(),
            }).collect())
            .unwrap_or_default()
    }
}

/// Convenience function to parse a hook file
pub fn parse_hook_file(path: &Path) -> Result<HookSchema> {
    HookParser::parse_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_action_string() {
        let action = HookParser::parse_action_string("send_email(template, to)");
        assert_eq!(action.action_type, "send_email");

        let action = HookParser::parse_action_string("emit: PasswordResetRequestedEvent");
        assert_eq!(action.action_type, "emit");
    }
}
