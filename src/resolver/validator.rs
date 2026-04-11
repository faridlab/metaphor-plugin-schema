//! Schema validator
//!
//! Validates schema completeness and correctness.

use super::ResolveError;
use crate::ast::ModuleSchema;
use std::collections::HashSet;

/// Validates a schema for correctness
pub struct SchemaValidator<'a> {
    schema: &'a ModuleSchema,
}

impl<'a> SchemaValidator<'a> {
    pub fn new(schema: &'a ModuleSchema) -> Self {
        Self { schema }
    }

    /// Validate the schema
    pub fn validate(&self) -> Result<(), Vec<ResolveError>> {
        let mut errors = Vec::new();

        // PHASE 2: Check for duplicate model names
        let mut model_names = HashSet::new();
        for model in &self.schema.models {
            if !model_names.insert(&model.name) {
                errors.push(ResolveError::validation(format!(
                    "Schema has duplicate model name '{}'",
                    model.name
                )));
            }
        }

        // Validate models
        for model in &self.schema.models {
            errors.extend(self.validate_model(model));
        }

        // Validate enums
        for enum_def in &self.schema.enums {
            errors.extend(self.validate_enum(enum_def));
        }

        // Validate hooks (entity lifecycle)
        for hook in &self.schema.hooks {
            errors.extend(self.validate_hook(hook));
        }

        // Validate workflows (business processes)
        for workflow in &self.schema.workflows {
            errors.extend(self.validate_workflow(workflow));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn validate_model(&self, model: &crate::ast::Model) -> Vec<ResolveError> {
        let mut errors = Vec::new();

        // Check for primary key
        let has_id = model.fields.iter().any(|f| f.has_attribute("id"));
        if !has_id {
            errors.push(ResolveError::validation(format!(
                "Model '{}' has no primary key field (missing @id attribute)",
                model.name
            )));
        }

        // Collect known model names for relation validation
        let known_models: HashSet<_> = self.schema.models.iter()
            .map(|m| m.name.as_str())
            .collect();

        let known_types: HashSet<_> = self.schema.enums.iter()
            .map(|e| e.name.as_str())
            .chain(self.schema.type_defs.iter().map(|t| t.name.as_str()))
            .collect();

        // Check for duplicate field names and validate each field
        let mut field_names = HashSet::new();
        for field in &model.fields {
            if !field_names.insert(&field.name) {
                errors.push(ResolveError::validation(format!(
                    "Model '{}' has duplicate field '{}'",
                    model.name, field.name
                )));
            }

            // PHASE 2: Check that fields ending with _id have @foreign_key attribute
            // Skip check if @exclude_from_foreign_key_check attribute is present
            let skip_fk_check = field.has_attribute("exclude_from_foreign_key_check");
            if field.name.ends_with("_id") && !field.has_attribute("foreign_key") && !skip_fk_check {
                errors.push(ResolveError::validation(format!(
                    "Model '{}' field '{}' ends with '_id' but missing @foreign_key(Model.field) attribute (use @exclude_from_foreign_key_check for non-reference IDs)",
                    model.name, field.name
                )));
            }

            // PHASE 2: Check that metadata fields use 'json' type, not custom types
            if field.name == "metadata" {
                if let crate::ast::TypeRef::Custom(ref type_name) = field.type_ref {
                    if type_name != "json" && type_name != "Json" {
                        errors.push(ResolveError::validation(format!(
                            "Model '{}' metadata field must use 'type: json' not 'type: {}'",
                            model.name, type_name
                        )));
                    }
                }
            }
        }

        // Validate index fields reference actual columns or valid JSONB expressions
        errors.extend(self.validate_indexes(model, &field_names));

        // Check for duplicate relation names and validate relations
        for relation in &model.relations {
            if !field_names.insert(&relation.name) {
                errors.push(ResolveError::validation(format!(
                    "Model '{}' has duplicate relation/field name '{}'",
                    model.name, relation.name
                )));
            }

            // PHASE 2: Check that relation targets reference existing models
            let target_name = match &relation.target {
                crate::ast::TypeRef::Custom(name) => name.as_str(),
                crate::ast::TypeRef::Optional(inner) => {
                    if let crate::ast::TypeRef::Custom(name) = inner.as_ref() {
                        name.as_str()
                    } else {
                        continue;
                    }
                }
                crate::ast::TypeRef::Array(inner) => {
                    if let crate::ast::TypeRef::Custom(name) = inner.as_ref() {
                        name.as_str()
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };

            // Skip if it's a known enum or type def (not a model relation)
            if known_types.contains(target_name) {
                continue;
            }

            // Check if it's a known model
            if !known_models.contains(target_name) {
                errors.push(ResolveError::validation(format!(
                    "Model '{}' relation '{}' references unknown model '{}'",
                    model.name, relation.name, target_name
                )));
            }
        }

        errors
    }

    /// Validate that index fields reference actual columns or valid JSONB expressions
    fn validate_indexes(&self, model: &crate::ast::Model, field_names: &HashSet<&String>) -> Vec<ResolveError> {
        let mut errors = Vec::new();

        // Known sub-keys of audit_metadata JSONB fields
        const AUDIT_METADATA_KEYS: &[&str] = &[
            "created_at", "updated_at", "deleted_at",
            "created_by", "updated_by", "deleted_by",
        ];

        // Collect JSONB field info for sub-key resolution
        let has_audit_metadata = model.fields.iter().any(|f| f.has_attribute("audit_metadata"));

        // Collect JSONB default keys for data fields
        let jsonb_data_keys: HashSet<String> = model.fields.iter()
            .filter(|f| {
                matches!(f.type_ref, crate::ast::TypeRef::Primitive(crate::ast::PrimitiveType::Json))
                    || matches!(&f.type_ref, crate::ast::TypeRef::Optional(inner)
                        if matches!(inner.as_ref(), crate::ast::TypeRef::Primitive(crate::ast::PrimitiveType::Json)))
            })
            .filter_map(|f| f.default_value())
            .flat_map(|v| {
                // Extract the default string from the AttributeValue
                let default_str = match v {
                    crate::ast::AttributeValue::String(s) => s.clone(),
                    other => format!("{:?}", other),
                };
                // Extract JSON keys: find "key": patterns
                let mut keys = Vec::new();
                let mut chars = default_str.chars().peekable();
                while let Some(ch) = chars.next() {
                    if ch == '"' {
                        let mut key = String::new();
                        while let Some(&next) = chars.peek() {
                            if next == '"' {
                                chars.next();
                                break;
                            }
                            key.push(next);
                            chars.next();
                        }
                        // Check if followed by ':' (it's a JSON key)
                        // Skip whitespace
                        while let Some(&next) = chars.peek() {
                            if next == ' ' || next == '\t' {
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        if let Some(&':') = chars.peek() {
                            keys.push(key);
                        }
                    }
                }
                keys
            })
            .collect();

        for index in &model.indexes {
            for field_name in &index.fields {
                // Skip JSONB expressions (e.g., "(data->>'field')")
                if field_name.contains("->>") {
                    continue;
                }

                // Check if it's a real column
                if field_names.contains(field_name) {
                    continue;
                }

                // Check if it's a known audit_metadata sub-key
                if has_audit_metadata && AUDIT_METADATA_KEYS.contains(&field_name.as_str()) {
                    continue; // Valid — generator will resolve to ((metadata->>'field'))
                }

                // Check if it's a known JSONB data sub-key
                if jsonb_data_keys.contains(field_name.as_str()) {
                    continue; // Valid — generator will resolve to ((data->>'field'))
                }

                // Unknown field in index
                errors.push(ResolveError::validation(format!(
                    "Model '{}' index references unknown field '{}'. \
                     Available columns: {}",
                    model.name,
                    field_name,
                    model.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(", ")
                )));
            }
        }

        errors
    }

    fn validate_enum(&self, enum_def: &crate::ast::EnumDef) -> Vec<ResolveError> {
        let mut errors = Vec::new();

        // Check for at least one variant
        if enum_def.variants.is_empty() {
            errors.push(ResolveError::validation(format!(
                "Enum '{}' has no variants",
                enum_def.name
            )));
        }

        // PHASE 2: Check for at least one default variant
        let has_default = enum_def.variants.iter()
            .any(|v| v.attributes.iter().any(|a| a.name == "default"));
        if !has_default && !enum_def.variants.is_empty() {
            errors.push(ResolveError::validation(format!(
                "Enum '{}' has no default variant (add 'default: true' to one variant)",
                enum_def.name
            )));
        }

        // Check for duplicate variant names
        let mut variant_names = HashSet::new();
        for variant in &enum_def.variants {
            if !variant_names.insert(&variant.name) {
                errors.push(ResolveError::validation(format!(
                    "Enum '{}' has duplicate variant '{}'",
                    enum_def.name, variant.name
                )));
            }
        }

        errors
    }

    fn validate_hook(&self, hook: &crate::ast::Hook) -> Vec<ResolveError> {
        let mut errors = Vec::new();

        // Validate state machine if present
        if let Some(ref sm) = hook.state_machine {
            errors.extend(self.validate_state_machine(sm, &hook.name));
        }

        // Validate rules have conditions and messages
        for rule in &hook.rules {
            if rule.message.is_empty() {
                errors.push(ResolveError::validation(format!(
                    "Rule '{}' in hook '{}' has no message",
                    rule.name, hook.name
                )));
            }
        }

        errors
    }

    fn validate_workflow(&self, workflow: &crate::ast::Workflow) -> Vec<ResolveError> {
        let mut errors = Vec::new();

        // Validate workflow has at least one step
        if workflow.steps.is_empty() {
            errors.push(ResolveError::validation(format!(
                "Workflow '{}' has no steps",
                workflow.name
            )));
        }

        errors
    }

    fn validate_state_machine(
        &self,
        sm: &crate::ast::StateMachine,
        workflow_name: &str,
    ) -> Vec<ResolveError> {
        let mut errors = Vec::new();

        // Check for at least one state
        if sm.states.is_empty() {
            errors.push(ResolveError::StateMachineError {
                message: format!(
                    "State machine in workflow '{}' has no states",
                    workflow_name
                ),
            });
            return errors;
        }

        // Check for exactly one initial state
        let initial_count = sm.states.iter().filter(|s| s.initial).count();
        if initial_count == 0 {
            errors.push(ResolveError::StateMachineError {
                message: format!(
                    "State machine in workflow '{}' has no initial state (use @initial)",
                    workflow_name
                ),
            });
        } else if initial_count > 1 {
            errors.push(ResolveError::StateMachineError {
                message: format!(
                    "State machine in workflow '{}' has {} initial states (should be 1)",
                    workflow_name, initial_count
                ),
            });
        }

        // Check for at least one final state
        let final_count = sm.states.iter().filter(|s| s.final_state).count();
        if final_count == 0 {
            errors.push(ResolveError::StateMachineError {
                message: format!(
                    "State machine in workflow '{}' has no final state (use @final)",
                    workflow_name
                ),
            });
        }

        // Check transition states exist
        let state_names: HashSet<_> = sm.states.iter().map(|s| s.name.as_str()).collect();

        for transition in &sm.transitions {
            // Check source states
            for from in &transition.from {
                if from != "*" && !state_names.contains(from.as_str()) {
                    errors.push(ResolveError::StateMachineError {
                        message: format!(
                            "Transition '{}' in workflow '{}' references unknown source state '{}'",
                            transition.name, workflow_name, from
                        ),
                    });
                }
            }

            // Check target state
            if !state_names.contains(transition.to.as_str()) {
                errors.push(ResolveError::StateMachineError {
                    message: format!(
                        "Transition '{}' in workflow '{}' references unknown target state '{}'",
                        transition.name, workflow_name, transition.to
                    ),
                });
            }
        }

        // Check for unreachable states (states that can't be reached from initial)
        // TODO: Implement reachability analysis

        errors
    }
}
