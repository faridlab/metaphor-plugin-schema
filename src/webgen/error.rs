//! Error types for metaphor-webgen

use std::path::PathBuf;
use thiserror::Error;

/// Result type for metaphor-webgen
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during code generation
#[derive(Error, Debug)]
pub enum Error {
    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Proto file not found
    #[error("Proto directory not found: {0}")]
    ProtoNotFound(PathBuf),

    /// Webapp source directory not found
    #[error("Webapp source directory not found: {0}")]
    WebappNotFound(PathBuf),

    /// Invalid module name
    #[error("Invalid module name: {0}")]
    InvalidModule(String),

    /// No entities found
    #[error("No entities found in proto files")]
    NoEntitiesFound,

    /// Template error
    #[error("Template error: {0}")]
    Template(String),

    /// Parse error
    #[error("Parse error: {0}")]
    Parse(String),

    /// Generation error
    #[error("Generation error: {0}")]
    Generation(String),

    /// File write error
    #[error("Failed to write file {path}: {message}")]
    WriteError { path: PathBuf, message: String },

    /// Invalid target
    #[error("Invalid target: {0}. Valid targets: all, hooks, schemas, forms, pages, types")]
    InvalidTarget(String),
}

impl Error {
    /// Create a write error
    pub fn write_error(path: PathBuf, message: impl std::fmt::Display) -> Self {
        Self::WriteError {
            path,
            message: message.to_string(),
        }
    }
}
