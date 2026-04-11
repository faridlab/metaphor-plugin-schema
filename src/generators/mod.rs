//! Code generators module
//!
//! This module contains all code generators that transform the AST
//! into various output formats.

pub mod auth;
pub mod audit_triggers;
pub mod bulk_operations;
pub mod computed;
pub mod config;
pub mod cqrs;
pub mod domain_service;
pub mod events;
pub mod flow;
pub mod graphql;
pub mod grpc;
pub mod handler;
pub mod module;
pub mod openapi;
// TODO: pub mod permission;
pub mod proto;
pub mod repository;
pub mod repository_trait;
pub mod rust;
pub mod service;
pub mod specification;
pub mod sql;
pub mod state_machine;
pub mod trigger;
pub mod usecase;
pub mod validator;
pub mod value_object;
pub mod projection;
pub mod event_store;
pub mod export;
pub mod integration;
pub mod event_subscription;
pub mod dto;
pub mod seeder;
pub mod integration_test;
pub mod versioning;

// Framework compliance generators
pub mod app_state;
pub mod routes_composer;
pub mod handlers_module;

pub use auth::AuthGenerator;
pub use audit_triggers::AuditTriggersGenerator;
pub use bulk_operations::BulkOperationsGenerator;
pub use computed::ComputedGenerator;
pub use config::ConfigGenerator;
pub use cqrs::CqrsGenerator;
pub use domain_service::DomainServiceGenerator;
pub use events::EventsGenerator;
pub use flow::FlowGenerator;
pub use graphql::GraphqlGenerator;
pub use grpc::GrpcGenerator;
pub use handler::HandlerGenerator;
pub use module::ModuleGenerator;
pub use openapi::OpenApiGenerator;
// TODO: pub use permission::PermissionGenerator;
pub use proto::ProtoGenerator;
pub use repository::RepositoryGenerator;
pub use repository_trait::RepositoryTraitGenerator;
pub use rust::RustGenerator;
pub use service::ServiceGenerator;
pub use specification::SpecificationGenerator;
pub use sql::SqlGenerator;
pub use state_machine::StateMachineGenerator;
pub use trigger::TriggerGenerator;
pub use usecase::UseCaseGenerator;
pub use validator::ValidatorGenerator;
pub use value_object::ValueObjectGenerator;
pub use projection::ProjectionGenerator;
pub use event_store::EventStoreGenerator;
pub use export::ExportGenerator;
pub use integration::IntegrationGenerator;
pub use event_subscription::EventSubscriptionGenerator;
pub use dto::DtoGenerator;
pub use seeder::SeederGenerator;
pub use integration_test::IntegrationTestGenerator;
pub use versioning::VersioningGenerator;

// Framework compliance generators
pub use app_state::AppStateGenerator;
pub use routes_composer::RoutesComposerGenerator;
pub use handlers_module::HandlersModuleGenerator;

use crate::resolver::ResolvedSchema;
use crate::utils::to_snake_case;
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

/// Generator error types
#[derive(Debug, Error)]
pub enum GenerateError {
    #[error("Template error: {0}")]
    TemplateError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Generation error: {0}")]
    GenerationError(String),
}

/// Output from a generator
#[derive(Debug, Default)]
pub struct GeneratedOutput {
    /// Map of file path to content
    pub files: HashMap<PathBuf, String>,
}

impl GeneratedOutput {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_file(&mut self, path: impl Into<PathBuf>, content: impl Into<String>) {
        self.files.insert(path.into(), content.into());
    }

    pub fn merge(&mut self, other: GeneratedOutput) {
        self.files.extend(other.files);
    }
}

/// Trait for code generators
pub trait Generator {
    /// Generate code from a resolved schema
    fn generate(&self, schema: &ResolvedSchema) -> Result<GeneratedOutput, GenerateError>;

    /// Get the generator name
    fn name(&self) -> &'static str;
}

/// Generation options
#[derive(Debug, Clone, Default)]
pub struct GenerationOptions {
    /// Split output into multiple files (e.g., for OpenAPI: one file per entity)
    pub split: bool,

    /// Group generated files by model/domain (creates subdirectories per model)
    /// Example: src/application/commands/user/user_commands.rs instead of src/application/commands/user_commands.rs
    pub group_by_domain: bool,
}

/// Build file path for generated code, optionally grouping by domain
///
/// # Examples
/// ```ignore
/// // Flat structure (group_by_domain = false):
/// build_generated_path("src/application/commands", "User", "user_commands.rs", false)
/// // => "src/application/commands/user_commands.rs"
///
/// // Grouped structure (group_by_domain = true):
/// build_generated_path("src/application/commands", "User", "user_commands.rs", true)
/// // => "src/application/commands/user/user_commands.rs"
/// ```
pub fn build_generated_path(
    base_dir: &str,
    model_name: &str,
    file_name: &str,
    group_by_domain: bool,
) -> PathBuf {
    if group_by_domain {
        let model_snake = to_snake_case(model_name);
        PathBuf::from(format!("{}/{}/{}", base_dir, model_snake, file_name))
    } else {
        PathBuf::from(format!("{}/{}", base_dir, file_name))
    }
}

/// Generate mod.rs content for a model subdirectory
///
/// Creates the index file for a model-specific subdirectory that re-exports
/// the generated items.
///
/// # Examples
/// ```ignore
/// build_subdirectory_mod("User", "user_commands", "user")
/// // Returns:
/// // //! User module
/// // //!
/// // //! Auto-generated by metaphor-schema. Do not edit manually.
/// //
/// // pub mod user_commands;
/// //
/// // pub use user_commands::*;
/// ```
pub fn build_subdirectory_mod(model_name: &str, module_file: &str) -> String {
    let model_pascal = if model_name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
        model_name.to_string()
    } else {
        // Convert snake_case to PascalCase for display
        model_name
            .split('_')
            .map(|s| {
                let mut chars = s.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => {
                        first.to_uppercase().collect::<String>() + chars.as_str()
                    }
                }
            })
            .collect::<String>()
    };

    format!(
        r#"//! {} module
//!
//! Auto-generated by metaphor-schema. Do not edit manually.

pub mod {};

pub use {}::*;
"#,
        model_pascal, module_file, module_file
    )
}

/// Build the path to a subdirectory's mod.rs file
///
/// Creates the PathBuf for a model subdirectory's mod.rs file.
///
/// # Examples
/// ```ignore
/// // For model "User" in "src/application/commands":
/// build_subdirectory_mod_path("src/application/commands", "User")
/// // => PathBuf("src/application/commands/user/mod.rs")
/// ```
pub fn build_subdirectory_mod_path(base_dir: &str, model_name: &str) -> PathBuf {
    let model_snake = to_snake_case(model_name);
    let mut path = PathBuf::from(base_dir);
    path.push(&model_snake);
    path.push("mod.rs");
    path
}

/// Generate mod.rs content for a parent directory with grouped subdirectories
///
/// Creates the parent mod.rs that declares all model subdirectories.
///
/// # Example
/// ```ignore
/// // For src/application/commands/mod.rs with models: User, Role
/// build_parent_mod_with_groups(&["User", "Role"])
/// // Returns:
/// // pub mod user;
/// // pub mod role;
/// ```
pub fn build_parent_mod_with_groups(models: &[String]) -> String {
    let mut content = String::new();
    for model in models {
        let model_snake = to_snake_case(model);
        content.push_str(&format!("pub mod {};\n", model_snake));
    }
    content
}

/// Helper for caching model name conversions during code generation
///
/// For large schemas (100+ models), this avoids repeated string allocations.
/// Use during generation loops to cache snake_case conversions.
///
/// # Example
/// ```ignore
/// let mut cache = NameCache::new();
/// for model in &models {
///     let snake = cache.get_snake(&model.name);
///     // Use cached snake_case value
/// }
/// ```
#[derive(Debug, Default)]
pub struct NameCache {
    snake_cache: std::collections::HashMap<String, String>,
    pascal_cache: std::collections::HashMap<String, String>,
}

impl NameCache {
    /// Create a new empty cache
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or compute snake_case for a model name
    pub fn get_snake(&mut self, name: &str) -> &str {
        if !self.snake_cache.contains_key(name) {
            self.snake_cache.insert(name.to_string(), to_snake_case(name));
        }
        self.snake_cache.get(name).map(|s| s.as_str()).unwrap()
    }

    /// Get or compute PascalCase for a model name
    pub fn get_pascal(&mut self, name: &str) -> &str {
        if !self.pascal_cache.contains_key(name) {
            self.pascal_cache.insert(name.to_string(), to_pascal_case(name));
        }
        self.pascal_cache.get(name).map(|s| s.as_str()).unwrap()
    }

    /// Clear the cache (useful between generation batches)
    pub fn clear(&mut self) {
        self.snake_cache.clear();
        self.pascal_cache.clear();
    }

    /// Get the number of cached entries
    pub fn len(&self) -> usize {
        self.snake_cache.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.snake_cache.is_empty()
    }
}

/// Performance metrics for code generation
///
/// Optionally used to track generation time and file counts.
/// Enabled via "telemetry" feature flag.
#[cfg(feature = "telemetry")]
#[derive(Debug, Default)]
pub struct GenerationMetrics {
    /// Time taken for each generator
    pub generator_times: std::collections::HashMap<String, std::time::Duration>,
    /// Number of files generated per generator
    pub file_counts: std::collections::HashMap<String, usize>,
    /// Total files generated
    pub total_files: usize,
}

#[cfg(feature = "telemetry")]
impl GenerationMetrics {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self::default()
    }

    /// Record generation time for a generator
    pub fn record_time(&mut self, generator_name: &str, duration: std::time::Duration) {
        self.generator_times
            .entry(generator_name.to_string())
            .and_modify(|t| *t += duration)
            .or_insert(duration);
    }

    /// Record file count for a generator
    pub fn record_files(&mut self, generator_name: &str, count: usize) {
        self.file_counts
            .entry(generator_name.to_string())
            .and_modify(|c| *c += count)
            .or_insert(count);
        self.total_files += count;
    }

    /// Get a summary of metrics
    pub fn summary(&self) -> String {
        let mut summary = String::new();
        summary.push_str("=== Code Generation Metrics ===\n");
        summary.push_str(&format!("Total files generated: {}\n\n", self.total_files));

        summary.push_str("Generator times:\n");
        let mut times: Vec<_> = self.generator_times.iter().collect();
        times.sort_by(|a, b| b.1.cmp(a.1));
        for (name, duration) in times.iter().take(10) {
            summary.push_str(&format!("  {:<20}: {:?}\n", name, duration));
        }

        summary.push_str("\nFile counts:\n");
        let mut counts: Vec<_> = self.file_counts.iter().collect();
        counts.sort_by(|a, b| b.1.cmp(a.1));
        for (name, count) in counts.iter().take(10) {
            summary.push_str(&format!("  {:<20}: {} files\n", name, count));
        }

        summary
    }
}

/// Simple timing helper for code generation
///
/// Usage:
/// ```ignore
/// let _timer = Timer::new("cqrs");
/// // ... generation code ...
/// // Timer drops and records time if telemetry is enabled
/// ```
pub struct Timer {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    start: std::time::Instant,
}

impl Timer {
    /// Start a new timer
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            start: std::time::Instant::now(),
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        #[cfg(feature = "telemetry")]
        {
            let duration = self.start.elapsed();
            // In a real implementation, this would update a global metrics store
            // For now, we just log it
            if duration.as_millis() > 100 {
                eprintln!("Warning: Generator '{}' took {:?}", self.name, duration);
            }
        }
    }
}

/// Convert a string to PascalCase
///
/// This is a centralized utility for consistent PascalCase conversion
/// across all generators.
fn to_pascal_case(name: &str) -> String {
    name.split('_')
        .map(|s| {
            let mut chars = s.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    first.to_uppercase().collect::<String>() + chars.as_str()
                }
            }
        })
        .collect::<String>()
}

/// Generate all code for a schema
pub fn generate_all(
    schema: &ResolvedSchema,
    targets: &[GenerationTarget],
) -> Result<GeneratedOutput, GenerateError> {
    generate_all_with_options(schema, targets, &GenerationOptions::default())
}

/// Generate all code for a schema with options
pub fn generate_all_with_options(
    schema: &ResolvedSchema,
    targets: &[GenerationTarget],
    options: &GenerationOptions,
) -> Result<GeneratedOutput, GenerateError> {
    let mut output = GeneratedOutput::new();

    // Apply generators config from schema (enabled/disabled targets)
    let targets = filter_targets_by_config(targets, &schema.schema.generators_config);

    // Define target batches to avoid race conditions with model resolution
    let batches = [
        // Batch 1: Data Layer - fundamental data structures
        vec![
            GenerationTarget::Proto,
            GenerationTarget::Rust,
            GenerationTarget::Sql,
            GenerationTarget::Repository,
            GenerationTarget::RepositoryTrait,
        ],
        // Batch 2: Business Logic Layer - services and business rules
        vec![
            GenerationTarget::Service,
            GenerationTarget::DomainService,
            GenerationTarget::UseCase,
            GenerationTarget::Auth,
            GenerationTarget::Events,
            GenerationTarget::StateMachine,
            GenerationTarget::Validator,
            // TODO: GenerationTarget::Permission,
            GenerationTarget::Specification,
            GenerationTarget::Cqrs,
            GenerationTarget::Computed,
            GenerationTarget::BulkOperations,
        ],
        // Batch 3: API & Infrastructure Layer - external interfaces
        vec![
            GenerationTarget::Handler,
            GenerationTarget::Grpc,
            GenerationTarget::Graphql,
            GenerationTarget::OpenApi,
            GenerationTarget::Trigger,
            GenerationTarget::Flow,
            GenerationTarget::Module,
            GenerationTarget::Config,
            GenerationTarget::ValueObject,
            GenerationTarget::Projection,
            GenerationTarget::EventStore,
            GenerationTarget::Export,
            GenerationTarget::Integration,
            GenerationTarget::EventSubscription,
            GenerationTarget::Dto,
            GenerationTarget::Versioning,
            GenerationTarget::Seeder,
            GenerationTarget::IntegrationTest,
            GenerationTarget::AuditTriggers,
            // Framework compliance generators
            GenerationTarget::AppState,
            GenerationTarget::RoutesComposer,
            GenerationTarget::HandlersModule,
        ],
    ];

    // Process targets in batches to avoid race conditions
    for batch_targets in batches.iter() {
        let mut batch_output = GeneratedOutput::new();

        for target in batch_targets {
            if targets.contains(target) {
                let generator_output = match target {
                    GenerationTarget::Proto => ProtoGenerator::new().generate(schema)?,
                    GenerationTarget::Rust => RustGenerator::new().generate(schema)?,
                    GenerationTarget::Sql => SqlGenerator::new().generate(schema)?,
                    GenerationTarget::Repository => RepositoryGenerator::new().generate(schema)?,
                    GenerationTarget::RepositoryTrait => RepositoryTraitGenerator::new().generate(schema)?,
                    GenerationTarget::Service => ServiceGenerator::new().generate(schema)?,
                    GenerationTarget::DomainService => DomainServiceGenerator::new().generate(schema)?,
                    GenerationTarget::UseCase => UseCaseGenerator::new().generate(schema)?,
                    GenerationTarget::Auth => AuthGenerator::new().generate(schema)?,
                    GenerationTarget::Events => EventsGenerator::new().generate(schema)?,
                    GenerationTarget::StateMachine => StateMachineGenerator::new().generate(schema)?,
                    GenerationTarget::Validator => ValidatorGenerator::new().generate(schema)?,
                    // TODO: GenerationTarget::Permission => PermissionGenerator::new().generate(schema)?,
                    GenerationTarget::Handler => HandlerGenerator::new().generate(schema)?,
                    GenerationTarget::Grpc => GrpcGenerator::new().generate(schema)?,
                    GenerationTarget::Graphql => GraphqlGenerator::new().generate(schema)?,
                    GenerationTarget::OpenApi => OpenApiGenerator::new().with_split(options.split).generate(schema)?,
                    GenerationTarget::Trigger => TriggerGenerator::new().generate(schema)?,
                    GenerationTarget::Flow => FlowGenerator::new().generate(schema)?,
                    GenerationTarget::Module => ModuleGenerator::new().generate(schema)?,
                    GenerationTarget::Config => ConfigGenerator::new().generate(schema)?,
                    GenerationTarget::ValueObject => ValueObjectGenerator::new().generate(schema)?,
                    GenerationTarget::Specification => SpecificationGenerator::new().generate(schema)?,
                    GenerationTarget::Cqrs => CqrsGenerator::new().generate(schema)?,
                    GenerationTarget::Computed => ComputedGenerator::new().generate(schema)?,
                    GenerationTarget::Projection => ProjectionGenerator::new().generate(schema)?,
                    GenerationTarget::EventStore => EventStoreGenerator::new().generate(schema)?,
                    GenerationTarget::Export => ExportGenerator::new().generate(schema)?,
                    GenerationTarget::Integration => IntegrationGenerator::new().generate(schema)?,
                    GenerationTarget::EventSubscription => EventSubscriptionGenerator::new().generate(schema)?,
                    GenerationTarget::Dto => DtoGenerator::new().generate(schema)?,
                    GenerationTarget::Versioning => VersioningGenerator::new().generate(schema)?,
                    GenerationTarget::BulkOperations => BulkOperationsGenerator::new().generate(schema)?,
                    GenerationTarget::Seeder => SeederGenerator::new().generate(schema)?,
                    GenerationTarget::IntegrationTest => IntegrationTestGenerator::new().generate(schema)?,
                    GenerationTarget::AuditTriggers => AuditTriggersGenerator::new().generate(schema)?,
                    // Framework compliance generators
                    GenerationTarget::AppState => AppStateGenerator::new().generate(schema)?,
                    GenerationTarget::RoutesComposer => RoutesComposerGenerator::new().generate(schema)?,
                    GenerationTarget::HandlersModule => HandlersModuleGenerator::new().generate(schema)?,
                };
                batch_output.merge(generator_output);
            }
        }

        // Merge batch output into final output
        output.merge(batch_output);
    }

    Ok(output)
}

/// Generation targets
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationTarget {
    Proto,
    Rust,
    Sql,
    Repository,
    RepositoryTrait,
    Service,
    DomainService,
    UseCase,
    Auth,
    Events,
    StateMachine,
    Validator,
    // TODO: Permission,
    Handler,
    Grpc,
    Graphql,
    OpenApi,
    Trigger,
    Flow,
    Module,
    Config,
    ValueObject,
    Specification,
    Cqrs,
    Computed,
    Projection,
    EventStore,
    Export,
    Integration,
    EventSubscription,
    Dto,
    Versioning,
    BulkOperations,
    Seeder,
    IntegrationTest,
    AuditTriggers,
    // Framework compliance targets
    AppState,
    RoutesComposer,
    HandlersModule,
}

impl GenerationTarget {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "proto" | "protobuf" => Some(Self::Proto),
            "rust" => Some(Self::Rust),
            "sql" | "migration" | "migrations" => Some(Self::Sql),
            "repository" | "repo" => Some(Self::Repository),
            "repository-trait" | "repository_trait" | "repo-trait" => Some(Self::RepositoryTrait),
            "service" | "services" | "svc" => Some(Self::Service),
            "domain-service" | "domain_service" | "domain-svc" => Some(Self::DomainService),
            "usecase" | "usecases" | "use-case" | "use_case" | "interactor" | "interactors" => Some(Self::UseCase),
            "auth" | "authentication" | "authorization" => Some(Self::Auth),
            "events" | "domain-events" | "messaging" => Some(Self::Events),
            "state-machine" | "statemachine" | "sm" => Some(Self::StateMachine),
            "validator" | "validation" => Some(Self::Validator),
            // TODO: "permission" | "permissions" | "perm" => Some(Self::Permission),
            "handler" | "handlers" | "rest" => Some(Self::Handler),
            "grpc" | "tonic" => Some(Self::Grpc),
            "graphql" | "gql" => Some(Self::Graphql),
            "openapi" | "swagger" => Some(Self::OpenApi),
            "trigger" | "triggers" => Some(Self::Trigger),
            "workflow" | "workflows" | "flow" | "flows" | "saga" | "orchestration" => Some(Self::Flow),
            "module" | "mod" | "lib" => Some(Self::Module),
            "config" | "configuration" | "settings" => Some(Self::Config),
            "value-object" | "value_object" | "vo" => Some(Self::ValueObject),
            "specification" | "spec" | "specifications" => Some(Self::Specification),
            "cqrs" | "command" | "commands" | "query" | "queries" => Some(Self::Cqrs),
            "computed" | "computed-fields" | "computed_fields" | "virtual" => Some(Self::Computed),
            "projection" | "projections" | "read-model" | "read_model" => Some(Self::Projection),
            "event-store" | "event_store" | "eventstore" => Some(Self::EventStore),
            "export" | "exports" | "public-api" => Some(Self::Export),
            "integration" | "acl" | "anti-corruption" => Some(Self::Integration),
            "event-subscription" | "event_subscription" | "subscription" | "subscriptions" => Some(Self::EventSubscription),
            "dto" | "dtos" | "data-transfer" | "transfer-objects" => Some(Self::Dto),
            "versioning" | "version" | "api-version" | "api-versioning" => Some(Self::Versioning),
            "bulk-operations" | "bulk_operations" | "bulk" | "batch" => Some(Self::BulkOperations),
            "seeder" | "seeders" | "seed" | "seeds" => Some(Self::Seeder),
            "integration-test" | "integration_test" | "test" | "tests" => Some(Self::IntegrationTest),
            "audit-triggers" | "audit_triggers" | "audit-trigger" | "audit_trigger" => Some(Self::AuditTriggers),
            // Framework compliance generators
            "app-state" | "app_state" | "appstate" => Some(Self::AppState),
            "routes-composer" | "routes_composer" => Some(Self::RoutesComposer),
            "handlers-module" | "handlers_module" => Some(Self::HandlersModule),
            _ => None,
        }
    }

    pub fn all() -> Vec<Self> {
        vec![
            Self::Proto,
            Self::Rust,
            Self::Sql,
            Self::Repository,
            Self::RepositoryTrait,
            Self::Service,
            Self::DomainService,
            Self::UseCase,
            Self::Auth,
            Self::Events,
            Self::StateMachine,
            Self::Validator,
            // TODO: Self::Permission,
            Self::Handler,
            Self::Grpc,
            Self::Graphql,
            Self::OpenApi,
            Self::Trigger,
            Self::Flow,
            Self::Module,
            Self::Config,
            Self::ValueObject,
            Self::Specification,
            Self::Cqrs,
            Self::Computed,
            Self::Projection,
            Self::EventStore,
            Self::Export,
            Self::Integration,
            Self::EventSubscription,
            Self::Dto,
            Self::Versioning,
            Self::BulkOperations,
            Self::Seeder,
            Self::IntegrationTest,
            Self::AuditTriggers,
            // Framework compliance generators
            Self::AppState,
            Self::RoutesComposer,
            Self::HandlersModule,
        ]
    }
}

/// Parse generation targets from comma-separated string
pub fn parse_targets(s: &str) -> Vec<GenerationTarget> {
    if s.to_lowercase() == "all" {
        return GenerationTarget::all();
    }

    s.split(',')
        .filter_map(|t| GenerationTarget::from_str(t.trim()))
        .collect()
}

/// Filter targets based on module-level generators config.
///
/// - If `enabled` is set, only those targets pass through (whitelist).
/// - If `disabled` is set, those targets are removed (blacklist).
/// - If neither is set, all targets pass through unchanged.
fn filter_targets_by_config(
    targets: &[GenerationTarget],
    config: &Option<crate::parser::yaml_parser::GeneratorsConfig>,
) -> Vec<GenerationTarget> {
    // Whitelist mode: only keep explicitly listed targets — respects user's exact choice
    // (no implicit CQRS opt-in override — if user listed Cqrs, they want it)
    if let Some(enabled) = config.as_ref().and_then(|c| c.enabled.as_ref()) {
        let enabled_targets: Vec<GenerationTarget> = enabled
            .iter()
            .filter_map(|s| GenerationTarget::from_str(s.trim()))
            .collect();
        return targets
            .iter()
            .filter(|t| enabled_targets.contains(t))
            .cloned()
            .collect();
    }

    // Blacklist mode: remove explicitly disabled targets
    let mut filtered: Vec<GenerationTarget> = if let Some(disabled) = config.as_ref().and_then(|c| c.disabled.as_ref()) {
        let disabled_targets: Vec<GenerationTarget> = disabled
            .iter()
            .filter_map(|s| GenerationTarget::from_str(s.trim()))
            .collect();
        targets
            .iter()
            .filter(|t| !disabled_targets.contains(t))
            .cloned()
            .collect()
    } else {
        targets.to_vec()
    };

    // CQRS opt-in: skip Cqrs and Projection unless cqrs = true
    // Applies regardless of whether a config exists — default is opt-out
    let cqrs_enabled = config.as_ref().and_then(|c| c.cqrs) == Some(true);
    if !cqrs_enabled {
        filtered.retain(|t| !matches!(t, GenerationTarget::Cqrs | GenerationTarget::Projection));
    }

    filtered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::yaml_parser::GeneratorsConfig;

    fn all_targets() -> Vec<GenerationTarget> {
        GenerationTarget::all()
    }

    fn make_config(enabled: Option<Vec<&str>>, disabled: Option<Vec<&str>>, cqrs: Option<bool>) -> Option<GeneratorsConfig> {
        Some(GeneratorsConfig {
            enabled: enabled.map(|v| v.iter().map(|s| s.to_string()).collect()),
            disabled: disabled.map(|v| v.iter().map(|s| s.to_string()).collect()),
            cqrs,
        })
    }

    #[test]
    fn test_cqrs_skipped_by_default() {
        // Explicit config with cqrs: None → skip
        let result = filter_targets_by_config(&all_targets(), &make_config(None, None, None));
        assert!(!result.contains(&GenerationTarget::Cqrs));
        assert!(!result.contains(&GenerationTarget::Projection));
    }

    #[test]
    fn test_cqrs_skipped_when_no_config() {
        // No config at all → CQRS still skipped (opt-in applies universally)
        let result = filter_targets_by_config(&all_targets(), &None);
        assert!(!result.contains(&GenerationTarget::Cqrs));
        assert!(!result.contains(&GenerationTarget::Projection));
        // All other targets present
        assert!(result.contains(&GenerationTarget::Rust));
        assert!(result.contains(&GenerationTarget::Repository));
    }

    #[test]
    fn test_cqrs_skipped_when_false() {
        let result = filter_targets_by_config(&all_targets(), &make_config(None, None, Some(false)));
        assert!(!result.contains(&GenerationTarget::Cqrs));
        assert!(!result.contains(&GenerationTarget::Projection));
    }

    #[test]
    fn test_cqrs_included_when_true() {
        let result = filter_targets_by_config(&all_targets(), &make_config(None, None, Some(true)));
        assert!(result.contains(&GenerationTarget::Cqrs));
        assert!(result.contains(&GenerationTarget::Projection));
    }

    #[test]
    fn test_disabled_blacklist_still_applies_cqrs_opt_in() {
        // disabled: [handler] + no cqrs: true → handler gone, Cqrs/Projection also skipped
        let result = filter_targets_by_config(&all_targets(), &make_config(None, Some(vec!["handler"]), None));
        assert!(!result.contains(&GenerationTarget::Handler));
        assert!(!result.contains(&GenerationTarget::Cqrs));
        assert!(!result.contains(&GenerationTarget::Projection));
    }

    #[test]
    fn test_disabled_blacklist_with_cqrs_true_includes_cqrs() {
        // disabled: [handler] + cqrs: true → handler gone, Cqrs/Projection kept
        let result = filter_targets_by_config(&all_targets(), &make_config(None, Some(vec!["handler"]), Some(true)));
        assert!(!result.contains(&GenerationTarget::Handler));
        assert!(result.contains(&GenerationTarget::Cqrs));
        assert!(result.contains(&GenerationTarget::Projection));
    }

    #[test]
    fn test_enabled_whitelist_respects_explicit_cqrs() {
        // enabled: [rust, cqrs] → only those two, no implicit filtering
        let result = filter_targets_by_config(&all_targets(), &make_config(Some(vec!["rust", "cqrs"]), None, None));
        assert!(result.contains(&GenerationTarget::Rust));
        assert!(result.contains(&GenerationTarget::Cqrs));
        assert!(!result.contains(&GenerationTarget::Repository));
        assert!(!result.contains(&GenerationTarget::Projection));
    }
}
