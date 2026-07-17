//! Schema command implementations

mod changed;
mod diff;
mod discovery;
mod doctor;
mod generate;
mod manifest;
mod merge;
mod migration_cmd;
mod migrations;
mod module_loader;
mod openapi_collect;
mod parse;
mod undeclared;
mod validate;
mod validate_workspace;
mod watch;

pub(crate) use discovery::resolve_module_arg;
use changed::execute_changed;
use diff::execute_diff;
use doctor::execute_doctor;
use generate::execute_generate;
use migration_cmd::{execute_migration, execute_status};
use parse::execute_parse;
use undeclared::execute_undeclared;
use validate::execute_validate;
use validate_workspace::execute_validate_workspace;
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

    /// Validate cross-module foreign keys across the whole workspace
    ///
    /// Per-module `validate` cannot see other modules, so a `@foreign_key(other.Entity.id)`
    /// pointing at a nonexistent entity passes. This loads every module in `metaphor.yaml` and
    /// reports every cross-module foreign key whose target module or entity does not exist.
    ValidateWorkspace,

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

    /// Check hand-written aggregator files against the current schema
    ///
    /// Scans every `.rs` file listed under `user_owned:` in
    /// `metaphor.codegen.yaml` for references to handler-route functions
    /// (`create_<name>_routes`) that the generator won't emit — either
    /// because the model opted out via `config.generators.disabled` or
    /// because the model was renamed/removed.
    ///
    /// Run this BEFORE `metaphor schema generate -f` to know what you'll
    /// need to fix by hand. Read-only; never writes files. Exits non-zero
    /// when drift is found, so it slots into CI as a guard.
    Doctor {
        /// Module name (auto-detected from CWD if omitted)
        module: Option<String>,
    },
    /// Find hand-written files sitting undeclared in generator-owned trees
    ///
    /// `user_owned:` in `metaphor.codegen.yaml` is the ONLY thing that stops
    /// `schema generate -f` from overwriting a file. A hand-written `.rs` in
    /// `src/` that no `user_owned` glob matches is therefore live code-loss
    /// risk. This walks `src/`, classifies each file as generated (banner or
    /// generator-claimed path) vs hand-written, and reports every hand-written
    /// file no glob protects — with the `user_owned:` line to paste in.
    ///
    /// Distinct from `doctor`: doctor finds handler drift, this finds
    /// code-loss risk. Read-only; exits non-zero on findings so CI can gate.
    Undeclared {
        /// Module name (auto-detected from CWD if omitted)
        module: Option<String>,
    },
    /// Vendor composed modules' generated OpenAPI specs into a consumer app
    /// (for serving via Swagger UI). Reads the `openapi_vendor` section of the
    /// app's `metaphor.codegen.yaml`. Run from the app directory.
    OpenapiCollect {
        /// Consumer app/project name (auto-detected from CWD if omitted)
        module: Option<String>,
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
        SchemaAction::ValidateWorkspace => execute_validate_workspace(),
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
        SchemaAction::Doctor { module } => {
            let module = resolve_module_arg(module, "schema doctor")?;
            execute_doctor(&module)
        }
        SchemaAction::Undeclared { module } => {
            let module = resolve_module_arg(module, "schema undeclared")?;
            execute_undeclared(&module)
        }
        SchemaAction::OpenapiCollect { module } => {
            openapi_collect::execute_openapi_collect(module)
        }
    }
}


