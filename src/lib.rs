#![allow(clippy::only_used_in_recursion)]
#![allow(clippy::should_implement_trait)]
#![allow(clippy::too_many_arguments)]

//! # Backbone Schema
//!
//! Schema-driven code generator for the Backbone Framework.
//!
//! This crate parses `*.model.yaml`, `*.hook.yaml`, and `*.workflow.yaml` files
//! and generates **31 different targets** organized into 4 layers:
//!
//! ## Data Layer (5 generators)
//! - Protocol Buffer definitions (`proto`)
//! - Rust structs and enums (`rust`)
//! - SQL migrations (`sql`)
//! - Repository implementations (`repository`)
//! - Repository traits (`repository-trait`)
//!
//! ## Business Logic Layer (11 generators)
//! - Application services (`service`)
//! - Domain services (`domain-service`)
//! - Use cases (`usecase`)
//! - Authentication/authorization (`auth`)
//! - Domain events (`events`)
//! - State machines (`state-machine`)
//! - Validators (`validator`)
//! - Permission checks (`permission`)
//! - Business specifications (`specification`)
//! - CQRS implementations (`cqrs`)
//! - Computed fields (`computed`)
//!
//! ## API Layer (4 generators)
//! - REST handlers (`handler`)
//! - gRPC services with streaming (`grpc`)
//! - OpenAPI specifications (`openapi`)
//! - DTOs (`dto`)
//!
//! ## Infrastructure Layer (11 generators)
//! - Event triggers (`trigger`)
//! - Workflow orchestration (`flow`)
//! - Module code (`module`)
//! - Configuration (`config`)
//! - Value objects (`value-object`)
//! - Projections (`projection`)
//! - Event store (`event-store`)
//! - Public exports (`export`)
//! - Integration adapters (`integration`)
//! - Event subscriptions (`event-subscription`)
//! - API versioning (`versioning`)
//!
//! ## Usage
//!
//! ```bash
//! # Parse and validate schemas
//! backbone schema parse libs/modules/sapiens/schema/
//!
//! # Generate all 31 targets
//! backbone schema generate sapiens
//!
//! # Generate specific targets
//! backbone schema generate sapiens --target proto,rust,sql,repository
//!
//! # Generate only changed schemas (git-aware)
//! backbone schema generate sapiens --changed
//! ```

pub mod ast;
pub mod commands;
pub mod generators;
pub mod git;
pub mod kotlin;
pub mod merge;
pub mod migration;
pub mod parser;
pub mod resolver;
pub mod utils;
pub mod webgen;

/// Re-export commonly used types
pub use ast::{Model, Hook, Workflow};
pub use git::{GitChangeDetector, ChangedSchema, ChangeSummary};
pub use merge::{MergeStrategy, OpenApiMerger};
pub use migration::{SchemaDiff, SchemaSnapshot, diff_schemas, generate_migration};
pub use parser::{parse_model, parse_hook};

/// Re-export audit metadata constant for use by other generators
pub use parser::yaml_parser::AUDIT_METADATA_TYPE_NAME;
