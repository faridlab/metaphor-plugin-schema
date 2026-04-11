//! Integration tests for metaphor-schema
//!
//! This module tests the complete pipeline:
//! - YAML → AST parsing
//! - DDD features (entities, value objects, domain services, etc.)
//! - Generator pipeline (AST → generated code)
//!
//! # Test Organization
//!
//! - `ddd_parsing` - Tests for DDD YAML parsing into AST
//! - `generator_pipeline` - Tests for generators using DDD AST

pub mod ddd_parsing;
pub mod generator_pipeline;
