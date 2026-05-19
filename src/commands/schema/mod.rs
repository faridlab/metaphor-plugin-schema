//! Schema command implementations

mod changed;
mod diff;
mod discovery;
mod generate;
mod manifest;
mod merge;
mod migration_cmd;
mod migrations;
mod module_loader;
mod parse;
mod validate;
mod watch;

pub(crate) use discovery::resolve_module_arg;
use changed::execute_changed;
use diff::execute_diff;
use generate::execute_generate;
use migration_cmd::{execute_migration, execute_status};
use parse::execute_parse;
use validate::execute_validate;
use watch::execute_watch;

use anyhow::Result;
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand, Debug)]
pub enum SchemaAction {
    /// Parse schema files and output AST (for debugging)
    ///
    /// Parses .model.yaml, .hook.yaml, and .workflow.yaml files and displays the
    /// resulting Abstract Syntax Tree (AST). Supports DDD extensions including:
    /// - entities: Enhanced models with methods, invariants, and implements traits
    /// - value_objects: Wrapper and composite value objects with validation
    /// - domain_services: Services with dependencies and async methods
    /// - event_sourced: Event sourcing configuration with snapshots
    /// - authorization: RBAC/ABAC permissions, roles, and policies
    Parse {
        /// Path to schema directory or file
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Output format: json or pretty
        #[arg(short, long, default_value = "pretty")]
        format: OutputFormat,
    },

    /// Validate schema files for correctness and consistency
    ///
    /// Performs comprehensive validation including:
    /// - Schema syntax and structure
    /// - Type references and model relationships
    /// - DDD entity-model associations
    /// - Value object field types
    /// - Domain service dependency resolution
    /// - Authorization permission/role consistency
    Validate {
        /// Module name or path to schema directory
        #[arg(default_value = ".")]
        module: String,

        /// Show warnings in addition to errors
        #[arg(short, long)]
        warnings: bool,
    },

    /// Generate code from schema files
    ///
    /// Generates code for all 31 targets organized in layers:
    /// - Data Layer: proto, rust, sql, repository, repository-trait
    /// - Business Logic: service, domain-service, auth, events, event-store,
    ///   state-machine, validator, permission, specification, cqrs, computed
    /// - API Layer: handler, grpc, openapi, dto
    /// - Infrastructure: trigger, flow, module, config, value-object,
    ///   projection, export, integration, event-subscription, versioning
    ///
    /// DDD features (entities, value_objects, domain_services, event_sourced,
    /// authorization) in YAML schemas are used to enhance generated code with
    /// methods, invariants, dependencies, and access control.
    Generate {
        /// Module name to generate code for (auto-detected from CWD if omitted)
        module: Option<String>,

        /// Generation targets (comma-separated)
        ///
        /// Available targets:
        /// - Data: proto, rust, sql, repository, repository-trait
        /// - Logic: service, domain-service, auth, events, event-store,
        ///   state-machine, validator, permission, specification, cqrs, computed
        /// - API: handler, grpc, openapi, dto
        /// - Infra: trigger, flow, module, config, value-object, projection,
        ///   export, integration, event-subscription, versioning
        /// - all: Generate all targets (default)
        #[arg(short, long, default_value = "all")]
        target: String,

        /// Output directory (default: module root, e.g., libs/modules/{module}/)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Dry run - show what would be generated without writing files
        #[arg(long)]
        dry_run: bool,

        /// Force overwrite existing files
        #[arg(short, long)]
        force: bool,

        /// Split output into multiple files (e.g., for OpenAPI: one file per entity)
        #[arg(long)]
        split: bool,

        /// Only generate for changed schemas (uses git to detect changes)
        #[arg(long)]
        changed: bool,

        /// Base git reference for change detection (default: HEAD)
        /// Examples: main, origin/main, HEAD~3
        #[arg(long, default_value = "HEAD")]
        base: String,

        /// Validate generated code by running cargo check after generation
        /// Fails the command if compilation errors are detected
        #[arg(long)]
        validate: bool,

        /// Filter: only generate for specific models (comma-separated)
        /// Example: --models Customer,Order,Payment
        #[arg(long)]
        models: Option<String>,

        /// Filter: only generate for specific hooks (comma-separated)
        /// Example: --hooks OrderHooks,CustomerHooks
        #[arg(long)]
        hooks: Option<String>,

        /// Filter: only generate for specific workflows (comma-separated)
        /// Example: --workflows OrderProcessing,CustomerRegistration
        #[arg(long)]
        workflows: Option<String>,

        /// Skip strict validation (useful with --models/--hooks/--workflows filters)
        /// Allows generation even if filtered items have missing references
        #[arg(long)]
        lenient: bool,
    },

    /// Show diff between schema and existing generated code
    Diff {
        /// Module name
        module: String,

        /// Base git reference for comparison
        #[arg(long, default_value = "HEAD")]
        base: String,
    },

    /// Watch schema files and regenerate on changes
    Watch {
        /// Module name to watch
        module: String,

        /// Generation targets (comma-separated)
        #[arg(short, long, default_value = "all")]
        target: String,

        /// Output directory (default: module root, e.g., libs/modules/{module}/)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Generate database migration from schema changes
    Migration {
        /// Module name
        module: String,

        /// Output file for the migration (optional)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Include destructive changes (DROP statements)
        #[arg(long)]
        destructive: bool,

        /// Database URL for live introspection (falls back to DATABASE_URL env)
        #[arg(long, env = "DATABASE_URL")]
        database_url: Option<String>,

        /// Preview migration SQL without writing files
        #[arg(long)]
        preview: bool,

        /// Only generate safe operations (skip destructive changes)
        #[arg(long)]
        safe_only: bool,
    },

    /// Show which schema files have changed (uses git)
    Changed {
        /// Module name (optional, shows all modules if not specified)
        module: Option<String>,

        /// Base git reference for comparison (default: HEAD)
        #[arg(long, default_value = "HEAD")]
        base: String,

        /// Show affected output files
        #[arg(long)]
        outputs: bool,

        /// Show affected generation targets
        #[arg(long)]
        targets: bool,
    },

    /// Show schema drift between YAML definitions and database/snapshot
    ///
    /// Read-only check that shows what migrations would be needed without
    /// generating any files. Useful for CI checks and status monitoring.
    Status {
        /// Module name
        module: String,

        /// Database URL for live introspection (falls back to DATABASE_URL env)
        #[arg(long, env = "DATABASE_URL")]
        database_url: Option<String>,
    },
}

#[derive(Debug, Clone, Default, clap::ValueEnum)]
pub enum OutputFormat {
    Json,
    #[default]
    Pretty,
}

/// Execute a schema action
pub fn execute(action: SchemaAction) -> Result<()> {
    match action {
        SchemaAction::Parse { path, format } => execute_parse(&path, format),
        SchemaAction::Validate { module, warnings } => execute_validate(&module, warnings),
        SchemaAction::Generate {
            module,
            target,
            output,
            dry_run,
            force,
            split,
            changed,
            base,
            validate,
            models,
            hooks,
            workflows,
            lenient,
        } => {
            let module = resolve_module_arg(module, "schema generate")?;
            execute_generate(&module, &target, output, dry_run, force, split, changed, &base, validate, models.as_deref(), hooks.as_deref(), workflows.as_deref(), lenient)
        },
        SchemaAction::Diff { module, base } => execute_diff(&module, &base),
        SchemaAction::Watch {
            module,
            target,
            output,
        } => execute_watch(&module, &target, output),
        SchemaAction::Migration {
            module,
            output,
            destructive,
            database_url,
            preview,
            safe_only,
        } => execute_migration(&module, output, destructive, database_url, preview, safe_only),
        SchemaAction::Changed {
            module,
            base,
            outputs,
            targets,
        } => execute_changed(module.as_deref(), &base, outputs, targets),
        SchemaAction::Status {
            module,
            database_url,
        } => execute_status(&module, database_url),
    }
}


