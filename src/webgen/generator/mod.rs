//! Code generator module

pub mod base;
pub mod enhanced;

// Re-exports for backward compatibility
pub use base::{Generator, GenerationResult as BaseGenerationResult};
pub use enhanced::{EnhancedGenerator, GenerationResult as EnhancedGenerationResult};
