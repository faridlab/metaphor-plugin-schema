//! State machine generator
//!
//! Generates Rust state machine implementations from hook definitions.
//! Creates type-safe state transitions with role-based access control.

use super::{GenerateError, GeneratedOutput, Generator, build_generated_path, build_subdirectory_mod};
use crate::ast::hook::{StateMachine, Hook};
use crate::resolver::ResolvedSchema;
use crate::utils::{to_pascal_case, to_snake_case};
use std::fmt::Write;
use std::path::PathBuf;

/// Generates state machine implementations from hook definitions
pub struct StateMachineGenerator {
    /// Group generated files by model/domain
    group_by_domain: bool,
}

impl StateMachineGenerator {
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

    /// Generate state enum
    fn generate_state_enum(&self, hook: &Hook, sm: &StateMachine) -> Result<String, GenerateError> {
        let mut output = String::new();
        let name = &hook.name;

        writeln!(output, "use serde::{{Deserialize, Serialize}};").unwrap();
        writeln!(output, "use std::str::FromStr;").unwrap();
        writeln!(output).unwrap();

        // State enum
        writeln!(output, "/// States for {} workflow", name).unwrap();
        writeln!(output, "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]").unwrap();
        writeln!(output, "#[serde(rename_all = \"snake_case\")]").unwrap();
        writeln!(output, "pub enum {}State {{", name).unwrap();

        for state in &sm.states {
            let variant_name = to_pascal_case(&state.name);
            if state.initial {
                writeln!(output, "    /// Initial state").unwrap();
            }
            if state.final_state {
                writeln!(output, "    /// Final state").unwrap();
            }
            writeln!(output, "    {},", variant_name).unwrap();
        }

        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Default implementation (initial state)
        if let Some(initial) = sm.initial_state() {
            writeln!(output, "impl Default for {}State {{", name).unwrap();
            writeln!(output, "    fn default() -> Self {{").unwrap();
            writeln!(output, "        Self::{}", to_pascal_case(&initial.name)).unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output, "}}").unwrap();
            writeln!(output).unwrap();
        }

        // Display implementation
        writeln!(output, "impl std::fmt::Display for {}State {{", name).unwrap();
        writeln!(output, "    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{").unwrap();
        writeln!(output, "        match self {{").unwrap();
        for state in &sm.states {
            writeln!(
                output,
                "            Self::{} => write!(f, \"{}\"),",
                to_pascal_case(&state.name),
                state.name
            ).unwrap();
        }
        writeln!(output, "        }}").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // FromStr implementation
        writeln!(output, "impl FromStr for {}State {{", name).unwrap();
        writeln!(output, "    type Err = StateMachineError;").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "    fn from_str(s: &str) -> Result<Self, Self::Err> {{").unwrap();
        writeln!(output, "        match s.to_lowercase().as_str() {{").unwrap();
        for state in &sm.states {
            writeln!(
                output,
                "            \"{}\" => Ok(Self::{}),",
                state.name.to_lowercase(),
                to_pascal_case(&state.name)
            ).unwrap();
        }
        writeln!(
            output,
            "            _ => Err(StateMachineError::InvalidState(s.to_string())),"
        ).unwrap();
        writeln!(output, "        }}").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // State methods
        writeln!(output, "impl {}State {{", name).unwrap();
        writeln!(output, "    /// Check if this is the initial state").unwrap();
        writeln!(output, "    pub fn is_initial(&self) -> bool {{").unwrap();
        if let Some(initial) = sm.initial_state() {
            writeln!(output, "        matches!(self, Self::{})", to_pascal_case(&initial.name)).unwrap();
        } else {
            writeln!(output, "        false").unwrap();
        }
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Check if this is a final state").unwrap();
        writeln!(output, "    pub fn is_final(&self) -> bool {{").unwrap();
        let final_states: Vec<_> = sm.states.iter().filter(|s| s.final_state).collect();
        if final_states.is_empty() {
            writeln!(output, "        false").unwrap();
        } else {
            writeln!(output, "        matches!(self, {})",
                final_states.iter()
                    .map(|s| format!("Self::{}", to_pascal_case(&s.name)))
                    .collect::<Vec<_>>()
                    .join(" | ")
            ).unwrap();
        }
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Get all possible states").unwrap();
        writeln!(output, "    pub fn all() -> Vec<Self> {{").unwrap();
        writeln!(output, "        vec![").unwrap();
        for state in &sm.states {
            writeln!(output, "            Self::{},", to_pascal_case(&state.name)).unwrap();
        }
        writeln!(output, "        ]").unwrap();
        writeln!(output, "    }}").unwrap();

        writeln!(output, "}}").unwrap();

        Ok(output)
    }

    /// Generate transition enum
    fn generate_transition_enum(&self, hook: &Hook, sm: &StateMachine) -> Result<String, GenerateError> {
        let mut output = String::new();
        let name = &hook.name;

        // Transition enum
        writeln!(output, "/// Transitions for {} workflow", name).unwrap();
        writeln!(output, "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]").unwrap();
        writeln!(output, "#[serde(rename_all = \"snake_case\")]").unwrap();
        writeln!(output, "pub enum {}Transition {{", name).unwrap();

        for transition in &sm.transitions {
            let variant_name = to_pascal_case(&transition.name);
            writeln!(output, "    /// {} -> {}", transition.from.join(", "), transition.to).unwrap();
            writeln!(output, "    {},", variant_name).unwrap();
        }

        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Display implementation
        writeln!(output, "impl std::fmt::Display for {}Transition {{", name).unwrap();
        writeln!(output, "    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{").unwrap();
        writeln!(output, "        match self {{").unwrap();
        for transition in &sm.transitions {
            writeln!(
                output,
                "            Self::{} => write!(f, \"{}\"),",
                to_pascal_case(&transition.name),
                transition.name
            ).unwrap();
        }
        writeln!(output, "        }}").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // FromStr implementation
        writeln!(output, "impl FromStr for {}Transition {{", name).unwrap();
        writeln!(output, "    type Err = StateMachineError;").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "    fn from_str(s: &str) -> Result<Self, Self::Err> {{").unwrap();
        writeln!(output, "        match s.to_lowercase().as_str() {{").unwrap();
        for transition in &sm.transitions {
            writeln!(
                output,
                "            \"{}\" => Ok(Self::{}),",
                transition.name.to_lowercase(),
                to_pascal_case(&transition.name)
            ).unwrap();
        }
        writeln!(
            output,
            "            _ => Err(StateMachineError::InvalidTransition(s.to_string())),"
        ).unwrap();
        writeln!(output, "        }}").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Transition methods
        writeln!(output, "impl {}Transition {{", name).unwrap();
        writeln!(output, "    /// Get the target state of this transition").unwrap();
        writeln!(output, "    pub fn target_state(&self) -> {}State {{", name).unwrap();
        writeln!(output, "        match self {{").unwrap();
        for transition in &sm.transitions {
            writeln!(
                output,
                "            Self::{} => {}State::{},",
                to_pascal_case(&transition.name),
                name,
                to_pascal_case(&transition.to)
            ).unwrap();
        }
        writeln!(output, "        }}").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Get all transitions").unwrap();
        writeln!(output, "    pub fn all() -> Vec<Self> {{").unwrap();
        writeln!(output, "        vec![").unwrap();
        for transition in &sm.transitions {
            writeln!(output, "            Self::{},", to_pascal_case(&transition.name)).unwrap();
        }
        writeln!(output, "        ]").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Get allowed roles for this transition").unwrap();
        writeln!(output, "    pub fn allowed_roles(&self) -> &'static [&'static str] {{").unwrap();
        writeln!(output, "        match self {{").unwrap();
        for transition in &sm.transitions {
            if transition.allowed_roles.is_empty() {
                writeln!(
                    output,
                    "            Self::{} => &[],",
                    to_pascal_case(&transition.name)
                ).unwrap();
            } else {
                writeln!(
                    output,
                    "            Self::{} => &[{}],",
                    to_pascal_case(&transition.name),
                    transition.allowed_roles.iter()
                        .map(|r| format!("\"{}\"", r))
                        .collect::<Vec<_>>()
                        .join(", ")
                ).unwrap();
            }
        }
        writeln!(output, "        }}").unwrap();
        writeln!(output, "    }}").unwrap();

        writeln!(output, "}}").unwrap();

        Ok(output)
    }

    /// Generate state machine struct
    fn generate_state_machine_struct(&self, hook: &Hook, sm: &StateMachine) -> Result<String, GenerateError> {
        let mut output = String::new();
        let name = &hook.name;

        // Import StateMachineError from the parent module (defined once in mod.rs)
        writeln!(output, "use super::StateMachineError;").unwrap();
        writeln!(output).unwrap();

        // State machine struct
        writeln!(output, "/// State machine for {} workflow", name).unwrap();
        writeln!(output, "#[derive(Debug, Clone)]").unwrap();
        writeln!(output, "pub struct {}StateMachine {{", name).unwrap();
        writeln!(output, "    current_state: {}State,", name).unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Implementation
        writeln!(output, "impl {}StateMachine {{", name).unwrap();
        writeln!(output, "    /// Create a new state machine with initial state").unwrap();
        writeln!(output, "    pub fn new() -> Self {{").unwrap();
        writeln!(output, "        Self {{").unwrap();
        writeln!(output, "            current_state: {}State::default(),", name).unwrap();
        writeln!(output, "        }}").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Create from an existing state").unwrap();
        writeln!(output, "    pub fn from_state(state: {}State) -> Self {{", name).unwrap();
        writeln!(output, "        Self {{ current_state: state }}").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Get the current state").unwrap();
        writeln!(output, "    pub fn current_state(&self) -> {}State {{", name).unwrap();
        writeln!(output, "        self.current_state").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Determine which final states have NO outgoing transitions (truly terminal)
        let states_with_outgoing: std::collections::HashSet<&str> = sm.transitions.iter()
            .flat_map(|t| t.from.iter().map(|s| s.as_str()))
            .collect();
        let terminal_states: Vec<_> = sm.states.iter()
            .filter(|s| s.final_state && !states_with_outgoing.contains(s.name.as_str()) && s.name != "*")
            .collect();

        // Can transition check
        writeln!(output, "    /// Check if a transition is allowed from the current state").unwrap();
        writeln!(output, "    pub fn can_transition(&self, transition: {}Transition) -> bool {{", name).unwrap();
        // Only add is_final guard if there are truly terminal states (final + no outgoing transitions)
        if !terminal_states.is_empty() {
            let terminal_match = terminal_states.iter()
                .map(|s| format!("{}State::{}", name, to_pascal_case(&s.name)))
                .collect::<Vec<_>>()
                .join(" | ");
            writeln!(output, "        if matches!(self.current_state, {}) {{", terminal_match).unwrap();
            writeln!(output, "            return false;").unwrap();
            writeln!(output, "        }}").unwrap();
            writeln!(output).unwrap();
        }
        writeln!(output, "        match (self.current_state, transition) {{").unwrap();

        for transition in &sm.transitions {
            let trans_variant = to_pascal_case(&transition.name);
            for from_state in &transition.from {
                if from_state == "*" {
                    // Wildcard: allowed from any non-final state
                    writeln!(
                        output,
                        "            (_, {}Transition::{}) => true,",
                        name, trans_variant
                    ).unwrap();
                } else {
                    writeln!(
                        output,
                        "            ({}State::{}, {}Transition::{}) => true,",
                        name, to_pascal_case(from_state), name, trans_variant
                    ).unwrap();
                }
            }
        }

        writeln!(output, "            _ => false,").unwrap();
        writeln!(output, "        }}").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Can transition with role check
        writeln!(output, "    /// Check if a transition is allowed for a given role").unwrap();
        writeln!(output, "    pub fn can_transition_with_role(&self, transition: {}Transition, role: &str) -> bool {{", name).unwrap();
        writeln!(output, "        if !self.can_transition(transition) {{").unwrap();
        writeln!(output, "            return false;").unwrap();
        writeln!(output, "        }}").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "        let allowed_roles = transition.allowed_roles();").unwrap();
        writeln!(output, "        if allowed_roles.is_empty() {{").unwrap();
        writeln!(output, "            return true; // No role restriction").unwrap();
        writeln!(output, "        }}").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "        allowed_roles.iter().any(|r| *r == role || *r == \"*\")").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Apply transition
        writeln!(output, "    /// Apply a transition, returning the new state").unwrap();
        writeln!(
            output,
            "    pub fn transition(&mut self, transition: {}Transition) -> Result<{}State, StateMachineError> {{",
            name, name
        ).unwrap();
        writeln!(output, "        if !self.can_transition(transition) {{").unwrap();
        writeln!(output, "            return Err(StateMachineError::TransitionNotAllowed {{").unwrap();
        writeln!(output, "                transition: transition.to_string(),").unwrap();
        writeln!(output, "                from: self.current_state.to_string(),").unwrap();
        writeln!(output, "            }});").unwrap();
        writeln!(output, "        }}").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "        self.current_state = transition.target_state();").unwrap();
        writeln!(output, "        Ok(self.current_state)").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Apply transition with role
        writeln!(output, "    /// Apply a transition with role check").unwrap();
        writeln!(
            output,
            "    pub fn transition_with_role(&mut self, transition: {}Transition, role: &str) -> Result<{}State, StateMachineError> {{",
            name, name
        ).unwrap();
        writeln!(output, "        // Check basic transition validity first").unwrap();
        writeln!(output, "        if !self.can_transition(transition) {{").unwrap();
        writeln!(output, "            return Err(StateMachineError::TransitionNotAllowed {{").unwrap();
        writeln!(output, "                transition: transition.to_string(),").unwrap();
        writeln!(output, "                from: self.current_state.to_string(),").unwrap();
        writeln!(output, "            }});").unwrap();
        writeln!(output, "        }}").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "        // Check role authorization").unwrap();
        writeln!(output, "        if !self.can_transition_with_role(transition, role) {{").unwrap();
        writeln!(output, "            return Err(StateMachineError::RoleNotAuthorized {{").unwrap();
        writeln!(output, "                role: role.to_string(),").unwrap();
        writeln!(output, "                transition: transition.to_string(),").unwrap();
        writeln!(output, "            }});").unwrap();
        writeln!(output, "        }}").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "        self.current_state = transition.target_state();").unwrap();
        writeln!(output, "        Ok(self.current_state)").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Get available transitions
        writeln!(output, "    /// Get all available transitions from the current state").unwrap();
        writeln!(
            output,
            "    pub fn available_transitions(&self) -> Vec<{}Transition> {{",
            name
        ).unwrap();
        writeln!(
            output,
            "        {}Transition::all()",
            name
        ).unwrap();
        writeln!(output, "            .into_iter()").unwrap();
        writeln!(output, "            .filter(|t| self.can_transition(*t))").unwrap();
        writeln!(output, "            .collect()").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Get available transitions for role
        writeln!(output, "    /// Get all available transitions for a given role").unwrap();
        writeln!(
            output,
            "    pub fn available_transitions_for_role(&self, role: &str) -> Vec<{}Transition> {{",
            name
        ).unwrap();
        writeln!(
            output,
            "        {}Transition::all()",
            name
        ).unwrap();
        writeln!(output, "            .into_iter()").unwrap();
        writeln!(output, "            .filter(|t| self.can_transition_with_role(*t, role))").unwrap();
        writeln!(output, "            .collect()").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Direct state transition (for entity transition_to method)
        writeln!(output, "    /// Attempt to transition directly to a target state.").unwrap();
        writeln!(output, "    ///").unwrap();
        writeln!(output, "    /// Finds any transition that leads to the target state and applies it.").unwrap();
        writeln!(output, "    /// Returns Err if no valid transition leads from current state to target.").unwrap();
        writeln!(
            output,
            "    pub fn transition_to_state(&mut self, target: {name}State) -> Result<{name}State, StateMachineError> {{",
            name = name
        ).unwrap();
        writeln!(
            output,
            "        let valid = {name}Transition::all().into_iter()",
            name = name
        ).unwrap();
        writeln!(output, "            .filter(|t| self.can_transition(*t))").unwrap();
        writeln!(output, "            .find(|t| t.target_state() == target);").unwrap();
        writeln!(output, "        match valid {{").unwrap();
        writeln!(output, "            Some(t) => self.transition(t),").unwrap();
        writeln!(output, "            None => Err(StateMachineError::TransitionNotAllowed {{").unwrap();
        writeln!(output, "                transition: target.to_string(),").unwrap();
        writeln!(output, "                from: self.current_state.to_string(),").unwrap();
        writeln!(output, "            }}),").unwrap();
        writeln!(output, "        }}").unwrap();
        writeln!(output, "    }}").unwrap();

        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Default implementation
        writeln!(output, "impl Default for {}StateMachine {{", name).unwrap();
        writeln!(output, "    fn default() -> Self {{").unwrap();
        writeln!(output, "        Self::new()").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output, "}}").unwrap();

        Ok(output)
    }

    /// Generate complete state machine file
    fn generate_hook_state_machine(&self, hook: &Hook) -> Result<String, GenerateError> {
        let sm = match &hook.state_machine {
            Some(sm) => sm,
            None => return Ok(String::new()), // No state machine defined
        };

        let mut output = String::new();
        let name = &hook.name;

        // Header
        writeln!(output, "//! State machine for {} workflow", name).unwrap();
        writeln!(output, "//!").unwrap();
        writeln!(output, "//! Generated by metaphor-schema").unwrap();
        writeln!(output).unwrap();

        // Generate state enum
        let state_enum = self.generate_state_enum(hook, sm)?;
        output.push_str(&state_enum);
        writeln!(output).unwrap();

        // Generate transition enum
        let transition_enum = self.generate_transition_enum(hook, sm)?;
        output.push_str(&transition_enum);
        writeln!(output).unwrap();

        // Generate state machine struct
        let sm_struct = self.generate_state_machine_struct(hook, sm)?;
        output.push_str(&sm_struct);

        // Generate tests
        writeln!(output).unwrap();
        self.generate_tests(&mut output, hook, sm)?;

        Ok(output)
    }

    /// Generate unit tests
    fn generate_tests(&self, output: &mut String, hook: &Hook, sm: &StateMachine) -> Result<(), GenerateError> {
        let name = &hook.name;

        writeln!(output, "#[cfg(test)]").unwrap();
        writeln!(output, "mod tests {{").unwrap();
        writeln!(output, "    use super::*;").unwrap();
        writeln!(output).unwrap();

        // Test initial state
        writeln!(output, "    #[test]").unwrap();
        writeln!(output, "    fn test_initial_state() {{").unwrap();
        writeln!(output, "        let sm = {}StateMachine::new();", name).unwrap();
        if let Some(initial) = sm.initial_state() {
            writeln!(
                output,
                "        assert_eq!(sm.current_state(), {}State::{});",
                name,
                to_pascal_case(&initial.name)
            ).unwrap();
        }
        writeln!(output, "        assert!(sm.current_state().is_initial());").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Test valid transitions
        if !sm.transitions.is_empty() {
            let first_transition = &sm.transitions[0];
            let wildcard = "*".to_string();
            let from_state = first_transition.from.first().unwrap_or(&wildcard);

            writeln!(output, "    #[test]").unwrap();
            writeln!(output, "    fn test_valid_transition() {{").unwrap();

            if from_state == "*" {
                writeln!(output, "        let mut sm = {}StateMachine::new();", name).unwrap();
            } else {
                writeln!(
                    output,
                    "        let mut sm = {}StateMachine::from_state({}State::{});",
                    name, name, to_pascal_case(from_state)
                ).unwrap();
            }

            writeln!(
                output,
                "        assert!(sm.can_transition({}Transition::{}));",
                name,
                to_pascal_case(&first_transition.name)
            ).unwrap();

            writeln!(
                output,
                "        let result = sm.transition({}Transition::{});",
                name,
                to_pascal_case(&first_transition.name)
            ).unwrap();
            writeln!(output, "        assert!(result.is_ok());").unwrap();
            writeln!(
                output,
                "        assert_eq!(sm.current_state(), {}State::{});",
                name,
                to_pascal_case(&first_transition.to)
            ).unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();
        }

        // Test invalid transition
        // Find a (state, transition) pair that is NOT allowed
        writeln!(output, "    #[test]").unwrap();
        writeln!(output, "    fn test_invalid_transition() {{").unwrap();

        // Strategy: find a state + transition combo where the transition's `from` does not include that state
        let mut found_invalid = false;
        'outer: for state in &sm.states {
            for transition in &sm.transitions {
                let is_valid = transition.from.iter().any(|f| f == &state.name || f == "*");
                if !is_valid {
                    writeln!(
                        output,
                        "        let mut sm = {}StateMachine::from_state({}State::{});",
                        name, name, to_pascal_case(&state.name)
                    ).unwrap();
                    writeln!(
                        output,
                        "        // {} is not valid from {} state",
                        to_pascal_case(&transition.name), to_pascal_case(&state.name)
                    ).unwrap();
                    writeln!(
                        output,
                        "        let result = sm.transition({}Transition::{});",
                        name,
                        to_pascal_case(&transition.name)
                    ).unwrap();
                    writeln!(output, "        assert!(result.is_err());").unwrap();
                    found_invalid = true;
                    break 'outer;
                }
            }
        }
        if !found_invalid {
            writeln!(output, "        // All transitions are valid from all states (wildcard), test passes").unwrap();
            writeln!(output, "        assert!(true);").unwrap();
        }

        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Test state enum parsing
        writeln!(output, "    #[test]").unwrap();
        writeln!(output, "    fn test_state_parsing() {{").unwrap();
        if let Some(state) = sm.states.first() {
            writeln!(
                output,
                "        let state: {}State = \"{}\".parse().unwrap();",
                name,
                state.name.to_lowercase()
            ).unwrap();
            writeln!(
                output,
                "        assert_eq!(state, {}State::{});",
                name,
                to_pascal_case(&state.name)
            ).unwrap();
        }
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Test available transitions
        writeln!(output, "    #[test]").unwrap();
        writeln!(output, "    fn test_available_transitions() {{").unwrap();
        writeln!(output, "        let sm = {}StateMachine::new();", name).unwrap();
        writeln!(output, "        let available = sm.available_transitions();").unwrap();
        writeln!(output, "        // Should have at least some transitions available from initial state").unwrap();
        writeln!(output, "        assert!(!available.is_empty() || sm.current_state().is_final());").unwrap();
        writeln!(output, "    }}").unwrap();

        writeln!(output, "}}").unwrap();

        Ok(())
    }
}

impl Default for StateMachineGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl Generator for StateMachineGenerator {
    fn generate(&self, schema: &ResolvedSchema) -> Result<GeneratedOutput, GenerateError> {
        let mut output = GeneratedOutput::new();

        // Generate state machines for each hook with state machine
        for hook in &schema.schema.hooks {
            self.generate_hook_file(&mut output, hook)?;
        }

        // Generate mod.rs if we have any state machines
        let hooks_with_sm: Vec<_> = schema.schema.hooks
            .iter()
            .filter(|h| h.state_machine.is_some())
            .collect();

        if hooks_with_sm.is_empty() {
            return Ok(output);
        }

        let mod_content = generate_state_machine_mod(&hooks_with_sm, self.group_by_domain);
        output.add_file(
            PathBuf::from("src/domain/state_machine/mod.rs"),
            mod_content,
        );

        Ok(output)
    }

    fn name(&self) -> &'static str {
        "state_machine"
    }
}

impl StateMachineGenerator {
    /// Generate a hook file if it has a state machine
    fn generate_hook_file(&self, output: &mut GeneratedOutput, hook: &Hook) -> Result<(), GenerateError> {
        if hook.state_machine.is_none() {
            return Ok(());
        }

        let content = self.generate_hook_state_machine(hook)?;
        if content.is_empty() {
            return Ok(());
        }

        let file_name = format!("{}_state_machine.rs", to_snake_case(&hook.name));
        let path = build_generated_path("src/domain/state_machine", &hook.name, &file_name, self.group_by_domain);
        output.add_file(path, content);

        // Generate subdirectory mod.rs if grouping by domain
        if self.group_by_domain {
            let mod_path = PathBuf::from(format!("src/domain/state_machine/{}/mod.rs", to_snake_case(&hook.name)));
            let sub_mod_content = build_subdirectory_mod(&hook.name, &file_name.replace(".rs", ""));
            output.add_file(mod_path, sub_mod_content);
        }

        Ok(())
    }
}

/// Generate mod.rs content for state machines
fn generate_state_machine_mod(hooks: &[&Hook], group_by_domain: bool) -> String {
    let mut mod_content = String::new();

    // Module declarations
    for hook in hooks {
        let snake = to_snake_case(&hook.name);
        if group_by_domain {
            writeln!(mod_content, "mod {};", snake).unwrap();
        } else {
            writeln!(mod_content, "mod {}_state_machine;", snake).unwrap();
        }
    }
    writeln!(mod_content).unwrap();

    // StateMachineError is defined ONCE here in mod.rs, shared by all entity state machine files
    writeln!(mod_content, "/// Shared error type for all state machines in this module").unwrap();
    writeln!(mod_content, "#[derive(Debug, Clone, thiserror::Error)]").unwrap();
    writeln!(mod_content, "pub enum StateMachineError {{").unwrap();
    writeln!(mod_content, "    #[error(\"Invalid state: {{0}}\")]").unwrap();
    writeln!(mod_content, "    InvalidState(String),").unwrap();
    writeln!(mod_content).unwrap();
    writeln!(mod_content, "    #[error(\"Invalid transition: {{0}}\")]").unwrap();
    writeln!(mod_content, "    InvalidTransition(String),").unwrap();
    writeln!(mod_content).unwrap();
    writeln!(mod_content, "    #[error(\"Transition '{{transition}}' not allowed from state '{{from}}'\")]").unwrap();
    writeln!(mod_content, "    TransitionNotAllowed {{").unwrap();
    writeln!(mod_content, "        transition: String,").unwrap();
    writeln!(mod_content, "        from: String,").unwrap();
    writeln!(mod_content, "    }},").unwrap();
    writeln!(mod_content).unwrap();
    writeln!(mod_content, "    #[error(\"Role '{{role}}' not authorized for transition '{{transition}}'\")]").unwrap();
    writeln!(mod_content, "    RoleNotAuthorized {{").unwrap();
    writeln!(mod_content, "        role: String,").unwrap();
    writeln!(mod_content, "        transition: String,").unwrap();
    writeln!(mod_content, "    }},").unwrap();
    writeln!(mod_content).unwrap();
    writeln!(mod_content, "    #[error(\"Guard condition failed for transition '{{0}}'\")]").unwrap();
    writeln!(mod_content, "    GuardFailed(String),").unwrap();
    writeln!(mod_content).unwrap();
    writeln!(mod_content, "    #[error(\"Cannot transition from final state '{{0}}'\")]").unwrap();
    writeln!(mod_content, "    FinalStateReached(String),").unwrap();
    writeln!(mod_content, "}}").unwrap();
    writeln!(mod_content).unwrap();

    // Re-exports (no longer need to include StateMachineError from individual files)
    for hook in hooks.iter() {
        let name = &hook.name;
        let snake = to_snake_case(name);
        let exports = format!("{}State, {}Transition, {}StateMachine", name, name, name);
        if group_by_domain {
            writeln!(mod_content, "pub use {}::{{{}}};", snake, exports).unwrap();
        } else {
            writeln!(mod_content, "pub use {}_state_machine::{{{}}};", snake, exports).unwrap();
        }
    }

    mod_content
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::hook::{State, Transition, Hook, StateMachine};

    fn create_test_hook() -> Hook {
        let mut hook = Hook::new("User", "User");
        hook.state_machine = Some(StateMachine {
            field: "status".to_string(),
            states: vec![
                State {
                    name: "pending".to_string(),
                    initial: true,
                    ..Default::default()
                },
                State {
                    name: "active".to_string(),
                    ..Default::default()
                },
                State {
                    name: "inactive".to_string(),
                    ..Default::default()
                },
                State {
                    name: "suspended".to_string(),
                    final_state: true,
                    ..Default::default()
                },
            ],
            transitions: vec![
                Transition {
                    name: "activate".to_string(),
                    from: vec!["pending".to_string()],
                    to: "active".to_string(),
                    allowed_roles: vec!["admin".to_string()],
                    ..Default::default()
                },
                Transition {
                    name: "deactivate".to_string(),
                    from: vec!["active".to_string()],
                    to: "inactive".to_string(),
                    allowed_roles: vec!["admin".to_string()],
                    ..Default::default()
                },
                Transition {
                    name: "reactivate".to_string(),
                    from: vec!["inactive".to_string()],
                    to: "active".to_string(),
                    allowed_roles: vec!["admin".to_string()],
                    ..Default::default()
                },
                Transition {
                    name: "suspend".to_string(),
                    from: vec!["active".to_string(), "inactive".to_string()],
                    to: "suspended".to_string(),
                    allowed_roles: vec!["admin".to_string()],
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        hook
    }

    #[test]
    fn test_generate_state_enum() {
        let generator = StateMachineGenerator::new();
        let hook = create_test_hook();
        let sm = hook.state_machine.as_ref().unwrap();

        let result = generator.generate_state_enum(&hook, sm);
        assert!(result.is_ok());

        let content = result.unwrap();
        assert!(content.contains("pub enum UserState"));
        assert!(content.contains("Pending"));
        assert!(content.contains("Active"));
        assert!(content.contains("Suspended"));
        assert!(content.contains("impl Default for UserState"));
    }

    #[test]
    fn test_generate_transition_enum() {
        let generator = StateMachineGenerator::new();
        let hook = create_test_hook();
        let sm = hook.state_machine.as_ref().unwrap();

        let result = generator.generate_transition_enum(&hook, sm);
        assert!(result.is_ok());

        let content = result.unwrap();
        assert!(content.contains("pub enum UserTransition"));
        assert!(content.contains("Activate"));
        assert!(content.contains("Deactivate"));
        assert!(content.contains("allowed_roles"));
    }

    #[test]
    fn test_generate_state_machine_struct() {
        let generator = StateMachineGenerator::new();
        let hook = create_test_hook();
        let sm = hook.state_machine.as_ref().unwrap();

        let result = generator.generate_state_machine_struct(&hook, sm);
        assert!(result.is_ok());

        let content = result.unwrap();
        assert!(content.contains("pub struct UserStateMachine"));
        assert!(content.contains("can_transition"));
        assert!(content.contains("transition_with_role"));
        assert!(content.contains("StateMachineError"));
    }
}
