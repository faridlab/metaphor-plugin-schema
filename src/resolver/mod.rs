//! Schema resolver and validator module
//!
//! This module handles:
//! - Type resolution (resolving custom types, enums, cross-module references)
//! - Reference resolution (ensuring all references are valid)
//! - Schema validation (business rules, completeness checks)
//! - Flow validation (step references, unreachable steps, terminal steps)

pub mod flow_resolver;
pub mod reference_resolver;
pub mod type_resolver;
pub mod validator;

pub use flow_resolver::FlowResolver;
pub use reference_resolver::ReferenceResolver;
pub use type_resolver::TypeResolver;
pub use validator::SchemaValidator;

use crate::ast::ModuleSchema;
use thiserror::Error;

/// Resolver error types
#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("Unknown type '{type_name}' at {location}")]
    UnknownType { type_name: String, location: String },

    #[error("Unknown model '{model_name}' referenced at {location}")]
    UnknownModel {
        model_name: String,
        location: String,
    },

    #[error("Unknown field '{field_name}' in model '{model_name}' at {location}")]
    UnknownField {
        field_name: String,
        model_name: String,
        location: String,
    },

    #[error("Circular reference detected: {path}")]
    CircularReference { path: String },

    #[error("Validation error: {message}")]
    ValidationError { message: String },

    #[error("State machine error: {message}")]
    StateMachineError { message: String },
}

impl ResolveError {
    pub fn unknown_type(type_name: impl Into<String>, location: impl Into<String>) -> Self {
        Self::UnknownType {
            type_name: type_name.into(),
            location: location.into(),
        }
    }

    pub fn unknown_model(model_name: impl Into<String>, location: impl Into<String>) -> Self {
        Self::UnknownModel {
            model_name: model_name.into(),
            location: location.into(),
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::ValidationError {
            message: message.into(),
        }
    }
}

/// Resolve and validate a module schema
pub fn resolve_schema(schema: &ModuleSchema) -> Result<ResolvedSchema, Vec<ResolveError>> {
    let mut errors = Vec::new();

    // Phase 1: Type resolution
    let type_resolver = TypeResolver::new(schema);
    if let Err(type_errors) = type_resolver.resolve() {
        errors.extend(type_errors);
    }

    // Phase 2: Reference resolution
    let ref_resolver = ReferenceResolver::new(schema);
    if let Err(ref_errors) = ref_resolver.resolve() {
        errors.extend(ref_errors);
    }

    // Phase 3: Validation
    let validator = SchemaValidator::new(schema);
    if let Err(validation_errors) = validator.validate() {
        errors.extend(validation_errors);
    }

    // Phase 4: Flow validation
    let flow_resolver = FlowResolver::new(schema);
    if let Err(flow_errors) = flow_resolver.resolve() {
        errors.extend(flow_errors);
    }

    if errors.is_empty() {
        Ok(ResolvedSchema {
            schema: schema.clone(),
        })
    } else {
        Err(errors)
    }
}

/// A fully resolved and validated schema
#[derive(Debug, Clone)]
pub struct ResolvedSchema {
    pub schema: ModuleSchema,
}
