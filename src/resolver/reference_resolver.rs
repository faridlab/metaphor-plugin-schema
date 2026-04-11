//! Reference resolver
//!
//! Resolves all references between models, fields, and workflows.

use super::ResolveError;
use crate::ast::ModuleSchema;
use crate::utils::to_snake_case;
use std::collections::{HashMap, HashSet};

/// Resolves references in a schema
pub struct ReferenceResolver<'a> {
    schema: &'a ModuleSchema,
    model_fields: HashMap<String, HashSet<String>>,
}

impl<'a> ReferenceResolver<'a> {
    pub fn new(schema: &'a ModuleSchema) -> Self {
        let mut model_fields = HashMap::new();

        for model in &schema.models {
            let mut fields = HashSet::new();
            for field in &model.fields {
                fields.insert(field.name.clone());
            }
            for relation in &model.relations {
                fields.insert(relation.name.clone());
            }
            // Store by model name (e.g., "User")
            model_fields.insert(model.name.clone(), fields.clone());

            // Also store by snake_case filename pattern (e.g., "user.model.yaml" -> "User")
            // This supports workflows that reference by filename
            let snake_name = to_snake_case(&model.name);
            model_fields.insert(format!("{}.model.yaml", snake_name), fields);
        }

        Self {
            schema,
            model_fields,
        }
    }

    /// Convert PascalCase to snake_case (planned for future use)
    fn _model_ref_to_name(&self, model_ref: &str) -> String {
        // If it's a filename like "user.model.yaml", convert to model name "User"
        if let Some(base) = model_ref.strip_suffix(".model.yaml") {
            // Convert snake_case to PascalCase
            base.split('_')
                .map(|part| {
                    let mut chars = part.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(c) => c.to_uppercase().chain(chars).collect(),
                    }
                })
                .collect()
        } else {
            model_ref.to_string()
        }
    }

    /// Resolve all references in the schema
    pub fn resolve(&self) -> Result<(), Vec<ResolveError>> {
        let mut errors = Vec::new();

        // Check hook model references (warn and skip instead of error)
        for hook in &self.schema.hooks {
            if !self.model_fields.contains_key(&hook.model_ref) {
                eprintln!("  ⚠ Warning: Hook '{}' references unknown model '{}' — skipping validation",
                    hook.name, hook.model_ref);
                continue;
            }

            // Check state machine field reference
            if let Some(ref sm) = hook.state_machine {
                if let Some(fields) = self.model_fields.get(&hook.model_ref) {
                    if !fields.contains(&sm.field) {
                        errors.push(ResolveError::UnknownField {
                            field_name: sm.field.clone(),
                            model_name: hook.model_ref.clone(),
                            location: format!("hook {} state machine", hook.name),
                        });
                    }
                }
            }

            // TODO: Check field references in rules, permissions, triggers
        }

        // Check relation targets
        for model in &self.schema.models {
            for relation in &model.relations {
                let target_name = self.get_relation_target_name(&relation.target);
                // Skip empty target names (primitives, maps, cross-module refs)
                if target_name.is_empty() {
                    continue;
                }
                if !self.model_fields.contains_key(&target_name) {
                    errors.push(ResolveError::unknown_model(
                        &target_name,
                        format!("{}.{}", model.name, relation.name),
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Get the target model name from a type reference
    /// Returns empty string for types that should skip validation (primitives, maps, cross-module refs)
    fn get_relation_target_name(&self, type_ref: &crate::ast::TypeRef) -> String {
        match type_ref {
            crate::ast::TypeRef::Custom(name) => name.clone(),
            crate::ast::TypeRef::Array(inner) => self.get_relation_target_name(inner),
            crate::ast::TypeRef::Optional(inner) => self.get_relation_target_name(inner),
            // Cross-module references (e.g., sapiens.User) can't be validated locally
            crate::ast::TypeRef::ModuleRef { .. } => String::new(),
            _ => String::new(),
        }
    }
}
