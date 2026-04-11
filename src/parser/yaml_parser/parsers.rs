//! Parser functions: YAML file parsing and format detection

use super::types::*;
use anyhow::{Context, Result};
use indexmap::IndexMap;
use std::path::Path;

/// Parse a YAML workflow schema file (multi-step business processes)
pub fn parse_workflow_yaml(path: &Path) -> Result<YamlWorkflowSchema> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;

    serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse YAML workflow: {}", path.display()))
}

/// Parse YAML workflow schema from string (multi-step business processes)
pub fn parse_workflow_yaml_str(content: &str) -> Result<YamlWorkflowSchema> {
    serde_yaml::from_str(content).with_context(|| "Failed to parse YAML workflow schema")
}

/// Parse a YAML model schema file
pub fn parse_model_yaml(path: &Path) -> Result<YamlModelSchema> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;

    serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse YAML: {}", path.display()))
}

/// Parse a YAML hook schema file (entity lifecycle behaviors)
pub fn parse_hook_yaml(path: &Path) -> Result<YamlHookSchema> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;

    // Preprocess content to strip comments before parsing
    let processed_content = strip_yaml_comments(&content);
    serde_yaml::from_str(&processed_content)
        .with_context(|| format!("Failed to parse YAML: {}", path.display()))
}

/// Parse YAML model schema from string
pub fn parse_model_yaml_str(content: &str) -> Result<YamlModelSchema> {
    serde_yaml::from_str(content).with_context(|| "Failed to parse YAML model schema")
}

/// Strip comments from YAML content before parsing
/// This handles comments that appear before required fields like 'name:'
fn strip_yaml_comments(content: &str) -> String {
    let mut result_lines = Vec::new();
    let mut found_yaml_content = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Check if this line contains a YAML field (has a colon)
        let is_yaml_field = trimmed.contains(':') && !trimmed.starts_with('#');

        // If we haven't found YAML content yet, check if this line is a YAML field
        if !found_yaml_content
            && (is_yaml_field || (!trimmed.starts_with('#') && !trimmed.is_empty())) {
            found_yaml_content = true;
        }

        // Once we find YAML content, keep all lines
        if found_yaml_content {
            result_lines.push(line);
        }
        // Otherwise skip leading comments and empty lines
    }

    // If we didn't find any YAML content, return original to avoid losing data
    if result_lines.is_empty() {
        content.to_string()
    } else {
        result_lines.join("\n")
    }
}

/// Parse YAML hook schema from string (entity lifecycle behaviors)
pub fn parse_hook_yaml_str(content: &str) -> Result<YamlHookSchema> {
    // Preprocess content to strip comments before parsing
    let processed_content = strip_yaml_comments(content);
    serde_yaml::from_str(&processed_content).with_context(|| "Failed to parse YAML hook schema")
}

/// Parse YAML hook index schema from string
pub fn parse_hook_index_yaml_str(content: &str) -> Result<YamlHookIndexSchema> {
    serde_yaml::from_str(content).with_context(|| "Failed to parse YAML hook index schema")
}

/// Result of parsing a hook YAML file - either standard hook or index file
#[derive(Debug, Clone)]
pub enum YamlHookParseResult {
    /// Standard hook file
    Hook(YamlHookSchema),
    /// Index/module configuration file
    Index(YamlHookIndexSchema),
}

/// Check if the content is a hook index file
pub fn is_hook_index_file(content: &str) -> bool {
    // Preprocess content to skip leading comments
    let processed_content = strip_yaml_comments(content);

    // Index files have 'module:' or 'imports:' at the top level
    // and do NOT have required 'name:' field
    let has_module = processed_content.lines().any(|l| l.trim().starts_with("module:"));
    let has_imports = processed_content.lines().any(|l| l.trim().starts_with("imports:"));
    let has_events = processed_content.lines().any(|l| l.trim() == "events:");
    let has_scheduled_jobs = processed_content.lines().any(|l| l.trim() == "scheduled_jobs:");

    (has_module || has_imports || has_events || has_scheduled_jobs) &&
        !processed_content.lines().take(10).any(|l| l.trim().starts_with("name:"))
}

/// Parse a hook YAML file that could be either standard hook or index
pub fn parse_hook_yaml_flexible(content: &str) -> Result<YamlHookParseResult> {
    // First, check if it looks like an index file
    if is_hook_index_file(content) {
        if let Ok(index) = parse_hook_index_yaml_str(content) {
            return Ok(YamlHookParseResult::Index(index));
        }
    }

    // Try parsing as standard hook (map-based format with name: field)
    match parse_hook_yaml_str(content) {
        Ok(hook) => Ok(YamlHookParseResult::Hook(hook)),
        Err(_) => {
            // Standard parsing failed — try list-based format (model: X, rules as sequence)
            if let Some(hook) = parse_hook_yaml_list_format(content) {
                return Ok(YamlHookParseResult::Hook(hook));
            }

            // If list-format also failed, try index as fallback
            if let Ok(index) = parse_hook_index_yaml_str(content) {
                Ok(YamlHookParseResult::Index(index))
            } else {
                // Return a descriptive error
                anyhow::bail!("Failed to parse hook YAML: not a valid hook (map or list format) or index file")
            }
        }
    }
}

/// Convert a `states:` mapping from list-format hook YAML into a `YamlStateMachine`.
///
/// Returns `None` when the mapping has no non-empty `field:` key.
/// Entries that lack required fields (`name:` for states, `to:` for transitions)
/// are silently skipped rather than being replaced with fallback values.
fn parse_list_format_state_machine(states_map: &serde_yaml::Mapping) -> Option<YamlStateMachine> {
    use serde_yaml::Value;

    let field = states_map
        .get(&Value::String("field".into()))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)?;

    let values = parse_list_format_states(states_map);
    let transitions = parse_list_format_transitions(states_map);

    Some(YamlStateMachine { field, values, transitions })
}

/// Convert `states.values:` sequence into `IndexMap<name, YamlState>`.
/// Entries missing a `name:` field are skipped.
fn parse_list_format_states(states_map: &serde_yaml::Mapping) -> IndexMap<String, YamlState> {
    use serde_yaml::Value;

    let mut values = IndexMap::new();
    let seq = match states_map.get(&Value::String("values".into())) {
        Some(Value::Sequence(s)) => s,
        _ => return values,
    };

    for entry in seq {
        let vm = match entry.as_mapping() {
            Some(m) => m,
            None => continue,
        };
        let name = match vm.get(&Value::String("name".into())).and_then(|v| v.as_str()) {
            Some(n) => n.to_string(),
            None => continue,   // `name:` is required — skip malformed entries
        };
        let initial = vm.get(&Value::String("initial".into())).and_then(|v| v.as_bool());
        let final_state = vm.get(&Value::String("final".into())).and_then(|v| v.as_bool());
        values.insert(name, YamlState::Full { initial, final_state, on_enter: vec![], on_exit: vec![] });
    }

    values
}

/// Convert `states.transitions:` sequence into `IndexMap<name, YamlTransition>`.
/// Entries missing a `to:` field are skipped.
fn parse_list_format_transitions(states_map: &serde_yaml::Mapping) -> IndexMap<String, YamlTransition> {
    use serde_yaml::Value;

    let mut transitions = IndexMap::new();
    let seq = match states_map.get(&Value::String("transitions".into())) {
        Some(Value::Sequence(s)) => s,
        _ => return transitions,
    };

    for entry in seq {
        let tm = match entry.as_mapping() {
            Some(m) => m,
            None => continue,
        };
        let to = match tm.get(&Value::String("to".into())).and_then(|v| v.as_str()) {
            Some(t) => t.to_string(),
            None => continue,   // `to:` is required — skip malformed entries
        };
        // Use `name:` as key when present; fall back to `event:` (common in bersihir hooks)
        let key = tm.get(&Value::String("name".into()))
            .or_else(|| tm.get(&Value::String("event".into())))
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| format!("to_{}", to));
        let from_val = tm.get(&Value::String("from".into())).cloned()
            .unwrap_or(Value::String("*".to_string()));
        let from = if let Some(seq) = from_val.as_sequence() {
            YamlStateList::Multiple(seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        } else {
            YamlStateList::Single(from_val.as_str().unwrap_or("*").to_string())
        };
        let roles = match tm.get(&Value::String("roles".into())) {
            Some(Value::Sequence(seq)) => seq.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
            _ => vec![],
        };
        transitions.insert(key, YamlTransition { from, to, roles, condition: None, message: None });
    }

    transitions
}

/// Parse hook YAML in list-based format (used by most hook files)
///
/// This handles the common format where:
/// - Top level has `model:` instead of `name:`
/// - `rules:` is a sequence of items with `- name:` instead of a map
/// - `states.values:` and `states.transitions:` are sequences
/// - `triggers:` and `computed:` are sequences
fn parse_hook_yaml_list_format(content: &str) -> Option<YamlHookSchema> {
    use serde_yaml::Value;
    // Only need serde_yaml::Value for direct field extraction

    let processed = strip_yaml_comments(content);
    let value: Value = serde_yaml::from_str(&processed).ok()?;
    let mapping = value.as_mapping()?;

    // Must have `model:` field to be a hook file
    let model_name = mapping.get(&Value::String("model".into()))?.as_str()?;

    // Build the YamlHookSchema directly with only name, model, and rules
    // States/triggers/computed use complex nested types that differ between
    // list and map formats — they're not needed for handler generation
    let mut rules = IndexMap::new();

    if let Some(rules_val) = mapping.get(&Value::String("rules".into())) {
        if let Some(rules_seq) = rules_val.as_sequence() {
            for rule in rules_seq {
                if let Some(rule_mapping) = rule.as_mapping() {
                    let name = rule_mapping
                        .get(&Value::String("name".into()))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unnamed");

                    // Extract `when` — can be string or sequence
                    let when: Vec<String> = match rule_mapping.get(&Value::String("when".into())) {
                        Some(Value::String(s)) => vec![s.clone()],
                        Some(Value::Sequence(seq)) => seq
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect(),
                        _ => vec![],
                    };

                    let condition = rule_mapping
                        .get(&Value::String("condition".into()))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let message = rule_mapping
                        .get(&Value::String("message".into()))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let code = rule_mapping
                        .get(&Value::String("code".into()))
                        .and_then(|v| v.as_str())
                        .map(String::from);

                    let severity = rule_mapping
                        .get(&Value::String("severity".into()))
                        .and_then(|v| v.as_str())
                        .map(String::from);

                    rules.insert(name.to_string(), YamlRule {
                        when,
                        condition,
                        message,
                        code,
                        severity,
                    });
                }
            }
        }
    }

    // Copy permissions as-is (already map format in YAML)
    let permissions: IndexMap<String, YamlPermission> = mapping
        .get(&Value::String("permissions".into()))
        .and_then(|v| serde_yaml::from_value(v.clone()).ok())
        .unwrap_or_default();

    // Parse the `states:` section.  The list format uses sequences for
    // `values` and `transitions`, but YamlStateMachine expects maps.  We
    // build a minimal YamlStateMachine carrying at least the `field` name
    // (e.g. "status") so Phase 2 state-machine enforcement can detect which
    // field is controlled.
    let states: Option<YamlStateMachine> = mapping
        .get(&Value::String("states".into()))
        .and_then(|v| v.as_mapping())
        .and_then(parse_list_format_state_machine);

    Some(YamlHookSchema {
        name: model_name.to_string(),
        model: model_name.to_string(),
        states,
        rules,
        permissions,
        triggers: IndexMap::new(),
        computed: IndexMap::new(),
    })
}

/// Parse YAML model index schema from string
pub fn parse_model_index_yaml_str(content: &str) -> Result<YamlModelIndexSchema> {
    serde_yaml::from_str(content).with_context(|| "Failed to parse YAML model index schema")
}

/// Result of parsing a model YAML file - either standard model or index file
#[derive(Debug, Clone)]
pub enum YamlModelParseResult {
    /// Standard model file with models/enums
    Model(Box<YamlModelSchema>),
    /// Index/module configuration file with shared_types
    Index(YamlModelIndexSchema),
}

/// Check if the content is a model index file (index.model.yaml)
pub fn is_model_index_file(content: &str) -> bool {
    // Index files have 'module:', 'shared_types:', 'imports:', or 'config:'
    // and do NOT have 'models:' section at the top level
    let has_module = content.lines().any(|l| l.trim().starts_with("module:"));
    let has_shared_types = content.lines().any(|l| l.trim().starts_with("shared_types:"));
    let has_imports = content.lines().any(|l| l.trim().starts_with("imports:"));
    let has_config = content.lines().any(|l| l.trim() == "config:");
    let has_models = content.lines().any(|l| l.trim().starts_with("models:"));

    (has_module || has_shared_types || has_imports || has_config) && !has_models
}

/// Parse a model YAML file that could be either standard model or index
pub fn parse_model_yaml_flexible(content: &str) -> Result<YamlModelParseResult> {
    // First, check if it looks like an index file
    if is_model_index_file(content) {
        if let Ok(index) = parse_model_index_yaml_str(content) {
            return Ok(YamlModelParseResult::Index(index));
        }
    }

    // Try parsing as standard model
    match parse_model_yaml_str(content) {
        Ok(model) => Ok(YamlModelParseResult::Model(Box::new(model))),
        Err(e) => {
            // If standard parsing failed, try index as fallback
            if let Ok(index) = parse_model_index_yaml_str(content) {
                Ok(YamlModelParseResult::Index(index))
            } else {
                Err(e)
            }
        }
    }
}
