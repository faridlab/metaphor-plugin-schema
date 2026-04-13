//! State Machine UI templates for generating React components

use crate::webgen::ast::state_machine::{HookSchema};
use crate::webgen::parser::to_pascal_case;
use std::collections::HashMap;

/// State machine component templates
pub struct StateMachineTemplates;

impl StateMachineTemplates {
    /// Clean model name by removing .model.yaml suffix and converting to valid identifier
    fn clean_model_name(model: &str) -> String {
        model
            .trim_end_matches(".model.yaml")
            .trim_end_matches(".yaml")
            .trim_end_matches(".yml")
            .to_string()
    }

    /// Generate state badge component
    pub fn generate_state_badge(schema: &HookSchema) -> String {
        let clean_model = Self::clean_model_name(&schema.model);
        let model_pascal = to_pascal_case(&clean_model);
        let model_snake = crate::webgen::parser::to_snake_case(&clean_model);

        let state_configs = Self::generate_state_configs(&schema.state_machine.as_ref().unwrap().states);

        let mut result = String::from(r#"import { Chip } from '@/components/ui';

/**
 * State badge component for "#);
        result.push_str(&model_pascal);
        result.push_str(r#"
 *
 * Generated from hook schema: "#);
        result.push_str(&schema.name);
        result.push_str(r#"
 */
export interface "#);
        result.push_str(&model_snake);
        result.push_str(r#"StateBadgeProps {
  state: string;
  label?: string;
}

export function "#);
        result.push_str(&model_pascal);
        result.push_str(r#"StateBadge({ state, label }: "#);
        result.push_str(&model_snake);
        result.push_str(r#"StateBadgeProps) {
  const stateConfig = getStateConfig(state);

  return (
    <Chip
      label={label ?? stateConfig.label}
      color={stateConfig.color}
      size="small"
      variant="outlined"
      sx={{ fontWeight: 'medium' }}
    />
  );
}

interface StateConfig {
  label: string;
  color: 'success' | 'warning' | 'error' | 'default' | 'info' | 'primary' | 'secondary';
}

function getStateConfig(state: string): StateConfig {
  const configs: Record<string, StateConfig> = {
"#);
        result.push_str(&state_configs);
        result.push_str(r#"  };

  return configs[state] || { label: state, color: 'default' };
}

// <<< CUSTOM: Add custom state configurations here
// END CUSTOM
"#);
        result
    }

    /// Generate state configurations
    fn generate_state_configs(states: &HashMap<String, crate::webgen::ast::state_machine::StateDefinition>) -> String {
        let mut configs = String::new();

        for (name, state) in states {
            let color = if state.is_initial {
                "'info'"
            } else if state.is_final {
                "'success'"
            } else {
                "'default'"
            };

            configs.push_str("    '");
            configs.push_str(name);
            configs.push_str("': { label: '");
            configs.push_str(&to_pascal_case(name));
            configs.push_str("', color: ");
            configs.push_str(color);
            configs.push_str(" },\n");
        }

        configs
    }

    /// Generate state transition button component
    pub fn generate_transition_buttons(schema: &HookSchema) -> String {
        let clean_model = Self::clean_model_name(&schema.model);
        let model_pascal = to_pascal_case(&clean_model);
        let model_snake = crate::webgen::parser::to_snake_case(&clean_model);

        let transition_map = Self::generate_transition_map(schema);

        let mut result = String::from(r#"import { Button } from '@/components/ui';
import { ArrowForward } from '@/components/ui';

/**
 * State transition button for "#);
        result.push_str(&model_pascal);
        result.push_str(r#"
 *
 * Generated from hook schema: "#);
        result.push_str(&schema.name);
        result.push_str(r#"
 */
export interface "#);
        result.push_str(&model_snake);
        result.push_str(r#"TransitionButtonsProps {
  currentState: string;
  onTransition?: (transition: string) => void;
  disabled?: boolean;
}

export function "#);
        result.push_str(&model_pascal);
        result.push_str(r#"TransitionButtons({
  currentState,
  onTransition,
  disabled = false,
}: "#);
        result.push_str(&model_snake);
        result.push_str(r#"TransitionButtonsProps) {
  const allowedTransitions = getAllowedTransitions(currentState);

  if (allowedTransitions.length === 0) {
    return null;
  }

  return (
    <>
      {allowedTransitions.map((transition) => (
        <Button
          key={transition.name}
          variant="outlined"
          startIcon={<ArrowForward />}
          onClick={() => onTransition?.(transition.name)}
          disabled={disabled}
          size="small"
          sx={{ mr: 1 }}
        >
          {transition.label}
        </Button>
      ))}
    </>
  );
}

interface TransitionInfo {
  name: string;
  label: string;
}

function getAllowedTransitions(currentState: string): TransitionInfo[] {
  const transitions: Record<string, TransitionInfo[]> = {
"#);
        result.push_str(&transition_map);
        result.push_str(r#"  };

  return transitions[currentState] || [];
}

// <<< CUSTOM: Add custom transition logic here
// END CUSTOM
"#);
        result
    }

    /// Generate transition map
    fn generate_transition_map(schema: &HookSchema) -> String {
        let mut map = String::new();

        if let Some(sm) = &schema.state_machine {
            for transition in &sm.transitions {
                for from_state in transition.from_states() {
                    map.push_str("    '");
                    map.push_str(&from_state);
                    map.push_str("': [\n");
                    map.push_str("      { name: '");
                    map.push_str(&transition.name);
                    map.push_str("', label: '");
                    map.push_str(&to_pascal_case(&transition.name));
                    map.push_str("' },\n");
                    map.push_str("    ],\n");
                }
            }
        }

        map
    }

    /// Generate state machine hook
    pub fn generate_state_machine_hook(schema: &HookSchema) -> String {
        let clean_model = Self::clean_model_name(&schema.model);
        let model_pascal = to_pascal_case(&clean_model);
        let model_snake = crate::webgen::parser::to_snake_case(&clean_model);

        let mut result = String::from(r#"import { useMutation, useQueryClient } from '@tanstack/react-query';

/**
 * Hook for managing "#);
        result.push_str(&model_pascal);
        result.push_str(r#" state transitions
 *
 * Generated from hook schema: "#);
        result.push_str(&schema.name);
        result.push_str(r#"
 */
export function use"#);
        result.push_str(&model_pascal);
        result.push_str(r#"StateTransitions() {
  const queryClient = useQueryClient();

  const transitionMutation = useMutation({
    mutationFn: async ({ entityId, transition }: { entityId: string; transition: string }) => {
      const response = await fetch(`/api/"#);
        result.push_str(&model_snake);
        result.push_str(r#"/${entityId}/transition`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ transition }),
      });
      return response.json();
    },
    onSuccess: (data, variables) => {
      queryClient.invalidateQueries({
        queryKey: ['"#);
        result.push_str(&model_snake);
        result.push_str(r#"', variables.entityId],
      });
    },
  });

  const transitionState = async (entityId: string, transition: string) => {
    return transitionMutation.mutateAsync({ entityId, transition });
  };

  return {
    transitionState,
    isLoading: transitionMutation.isPending,
    error: transitionMutation.error,
  };
}

// <<< CUSTOM: Add custom state machine logic here
// END CUSTOM
"#);
        result
    }

    /// Generate state history component
    pub fn generate_state_history(schema: &HookSchema) -> String {
        let clean_model = Self::clean_model_name(&schema.model);
        let model_pascal = to_pascal_case(&clean_model);
        let model_snake = crate::webgen::parser::to_snake_case(&clean_model);

        let mut result = String::from(r#"import { Box, Typography, Divider } from '@/components/ui';
import { Event } from '@/components/ui';

/**
 * State history component for "#);
        result.push_str(&model_pascal);
        result.push_str(r#"
 *
 * Displays the history of state transitions for an entity
 */
export interface "#);
        result.push_str(&model_snake);
        result.push_str(r#"StateHistoryProps {
  history: StateHistoryEntry[];
}

export function "#);
        result.push_str(&model_pascal);
        result.push_str(r#"StateHistory({ history }: "#);
        result.push_str(&model_snake);
        result.push_str(r#"StateHistoryProps) {
  if (history.length === 0) {
    return (
      <Typography variant="body2" color="text.secondary">
        No state history available
      </Typography>
    );
  }

  return (
    <Timeline>
      {history.map((entry, index) => (
        <TimelineItem key={entry.id || index}>
          <TimelineOppositeContent>
            <Typography variant="body2" color="text.secondary">
              {new Date(entry.timestamp).toLocaleString()}
            </Typography>
          </TimelineOppositeContent>
          <TimelineSeparator>
            <TimelineDot color="info" />
            {index < history.length - 1 && <TimelineSeparator />}
          </TimelineSeparator>
          <TimelineContent>
            <Typography variant="body2">
              {entry.transition}: {entry.fromState} → {entry.toState}
            </Typography>
            {entry.actor && (
              <Typography variant="caption" color="text.secondary">
                by {entry.actor}
              </Typography>
            )}
          </TimelineContent>
        </TimelineItem>
      ))}
    </Timeline>
  );
}

export interface StateHistoryEntry {
  id?: string;
  timestamp: string;
  transition: string;
  fromState: string;
  toState: string;
  actor?: string;
}

// <<< CUSTOM: Add custom history display logic here
// END CUSTOM
"#);
        result
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_state_config_generation() {
        // Test state config generation
    }
}
