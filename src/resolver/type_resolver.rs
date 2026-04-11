//! Type resolver
//!
//! Resolves all type references in the schema to their definitions.

use super::ResolveError;
use crate::ast::{ModuleSchema, PrimitiveType, TypeRef};
use std::collections::HashSet;

/// Resolves types in a schema
pub struct TypeResolver<'a> {
    schema: &'a ModuleSchema,
    known_types: HashSet<String>,
}

impl<'a> TypeResolver<'a> {
    pub fn new(schema: &'a ModuleSchema) -> Self {
        let mut known_types = HashSet::new();

        // Add all enum names
        for enum_def in &schema.enums {
            known_types.insert(enum_def.name.clone());
        }

        // Add all type def names
        for type_def in &schema.type_defs {
            known_types.insert(type_def.name.clone());
        }

        // Add all model names (for relation references)
        for model in &schema.models {
            known_types.insert(model.name.clone());
        }

        Self {
            schema,
            known_types,
        }
    }

    /// Resolve all types in the schema
    pub fn resolve(&self) -> Result<(), Vec<ResolveError>> {
        let mut errors = Vec::new();

        // Check all field types in models
        for model in &self.schema.models {
            for field in &model.fields {
                if let Err(e) = self.resolve_type(&field.type_ref, &model.name, &field.name) {
                    errors.push(e);
                }
            }

            for relation in &model.relations {
                if let Err(e) = self.resolve_type(&relation.target, &model.name, &relation.name) {
                    errors.push(e);
                }
            }
        }

        // Check all type def field types
        for type_def in &self.schema.type_defs {
            for field in &type_def.fields {
                if let Err(e) = self.resolve_type(&field.type_ref, &type_def.name, &field.name) {
                    errors.push(e);
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Resolve a single type reference
    fn resolve_type(
        &self,
        type_ref: &TypeRef,
        context_model: &str,
        context_field: &str,
    ) -> Result<(), ResolveError> {
        match type_ref {
            TypeRef::Primitive(_) => Ok(()),
            TypeRef::Custom(name) => {
                if self.known_types.contains(name) {
                    Ok(())
                } else {
                    Err(ResolveError::unknown_type(
                        name,
                        format!("{}.{}", context_model, context_field),
                    ))
                }
            }
            TypeRef::Array(inner) => self.resolve_type(inner, context_model, context_field),
            TypeRef::Optional(inner) => self.resolve_type(inner, context_model, context_field),
            TypeRef::Map { key, value } => {
                self.resolve_type(key, context_model, context_field)?;
                self.resolve_type(value, context_model, context_field)
            }
            TypeRef::ModuleRef { .. } => {
                // For cross-module references, we can't validate here
                // This would require loading other module schemas
                // For now, we'll accept them and validate at generation time
                Ok(())
            }
        }
    }

    /// Check if a type name is known
    pub fn is_known_type(&self, name: &str) -> bool {
        PrimitiveType::from_str(name).is_some() || self.known_types.contains(name)
    }
}
