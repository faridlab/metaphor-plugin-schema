//! Code generator module

pub mod base;
pub mod enhanced;

// Re-exports for backward compatibility
pub use base::{GenerationResult as BaseGenerationResult, Generator};
pub use enhanced::{EnhancedGenerator, GenerationResult as EnhancedGenerationResult};
