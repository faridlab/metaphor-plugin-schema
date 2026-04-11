//! Error types for backbone-mobilegen

use std::path::PathBuf;
use thiserror::Error;

/// Result type alias for backbone-mobilegen
pub type Result<T> = std::result::Result<T, MobileGenError>;

/// Error types for mobile code generation
#[derive(Debug, Error)]
pub enum MobileGenError {
    /// Schema parsing error
    #[error("Schema parsing error: {0}")]
    SchemaParse(String),

    /// Template rendering error
    #[error("Template error: {0}")]
    Template(String),

    /// File I/O error
    #[error("File error: {0}")]
    FileIo(#[from] std::io::Error),

    /// Handlebars template error
    #[error("Handlebars error: {0}")]
    Handlebars(#[from] handlebars::RenderError),

    /// Handlebars template registration error
    #[error("Template registration error: {0}")]
    TemplateError(#[from] handlebars::TemplateError),

    /// JSON serialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Unknown type reference
    #[error("Unknown type: {0}")]
    UnknownType(String),

    /// Missing required field
    #[error("Missing required field: {0} in {1}")]
    MissingField(String, String),

    /// Invalid output path
    #[error("Invalid output path: {0}")]
    InvalidOutputPath(PathBuf),

    /// Generation conflict
    #[error("Generation conflict: {0}")]
    Conflict(String),

    /// Schema parsing error (anyhow)
    #[error("Schema parsing error: {0}")]
    ParseError(#[from] anyhow::Error),
}

impl MobileGenError {
    /// Create a template error with context
    pub fn template(msg: impl Into<String>) -> Self {
        Self::Template(msg.into())
    }

    /// Create a schema parsing error with context
    pub fn schema_parse(msg: impl Into<String>) -> Self {
        Self::SchemaParse(msg.into())
    }
}
