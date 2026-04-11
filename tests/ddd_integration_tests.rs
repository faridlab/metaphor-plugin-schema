//! DDD Integration Tests for metaphor-schema
//!
//! This file serves as the entry point for integration tests that verify
//! the complete DDD feature pipeline:
//!
//! 1. **Parsing**: YAML files with DDD features → AST nodes
//! 2. **Generation**: AST nodes → Generated code
//!
//! # Test Modules
//!
//! - `integration::ddd_parsing` - Tests YAML parsing of DDD features
//! - `integration::generator_pipeline` - Tests code generation from DDD AST

mod integration;
