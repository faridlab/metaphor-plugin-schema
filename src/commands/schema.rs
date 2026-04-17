//! Schema command implementations

use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::time::Duration;

use crate::ast::{IndexType, ModelFile, ModuleSchema, PrimitiveType, TypeRef, HookFile, WorkflowFile};
use crate::generators::{generate_all_with_options, parse_targets, GenerationTarget, GenerationOptions};
use crate::git::{GitChangeDetector, ChangeSummary, ChangeType};
use crate::parser::{
    parse_model, parse_hook, parse_yaml_model, parse_yaml_hook_flexible, parse_yaml_workflow,
    parse_yaml_model_flexible, resolve_shared_types,
    HookParseResult, YamlHookIndexSchema, ModelParseResult, YamlField,
};
use crate::resolver::resolve_schema;
use indexmap::IndexMap;

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
        /// Module name to generate code for
        module: String,

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
        } => execute_generate(&module, &target, output, dry_run, force, split, changed, &base, validate, models.as_deref(), hooks.as_deref(), workflows.as_deref(), lenient),
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

fn execute_parse(path: &PathBuf, format: OutputFormat) -> Result<()> {
    println!("{} {}", "Parsing:".green().bold(), path.display());

    // Find all schema files
    let schema_files = find_schema_files(path)?;

    if schema_files.is_empty() {
        println!("{}", "No schema files found".yellow());
        return Ok(());
    }

    println!(
        "Found {} schema file(s)",
        schema_files.len().to_string().cyan()
    );

    for file in &schema_files {
        println!("  {} {}", "•".blue(), file.display());
    }

    println!();

    // Parse each schema file
    for file in &schema_files {
        let content = fs::read_to_string(file)
            .with_context(|| format!("Failed to read {}", file.display()))?;

        let filename = file.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if filename.ends_with(".model.schema") {
            println!("{} {}", "Parsing model:".cyan().bold(), file.display());
            match parse_model(&content) {
                Ok(model_file) => {
                    print_model_file(&model_file, &format);
                }
                Err(e) => {
                    println!("{}", e.format_with_source(&content, Some(filename)).red());
                }
            }
        } else if filename.ends_with(".hook.schema") || filename.ends_with(".workflow.schema") {
            println!("{} {}", "Parsing hook:".cyan().bold(), file.display());
            match parse_hook(&content) {
                Ok(hook_file) => {
                    print_hook_file(&hook_file, &format);
                }
                Err(e) => {
                    println!("{}", e.format_with_source(&content, Some(filename)).red());
                }
            }
        } else if filename.ends_with(".model.yaml") {
            println!("{} {}", "Parsing YAML model:".cyan().bold(), file.display());
            match parse_yaml_model(&content) {
                Ok(model_file) => {
                    print_model_file(&model_file, &format);
                }
                Err(e) => {
                    println!("{}", e.format_with_source(&content, Some(filename)).red());
                }
            }
        } else if filename.ends_with(".hook.yaml") {
            println!("{} {}", "Parsing YAML hook:".cyan().bold(), file.display());
            match parse_yaml_hook_flexible(&content) {
                Ok(HookParseResult::Hook(hook_file)) => {
                    print_hook_file(&hook_file, &format);
                }
                Ok(HookParseResult::Index(index_schema)) => {
                    print_hook_index(&index_schema, &format);
                }
                Err(e) => {
                    println!("{}", e.format_with_source(&content, Some(filename)).red());
                }
            }
        } else if filename.ends_with(".workflow.yaml") {
            println!("{} {}", "Parsing YAML workflow:".cyan().bold(), file.display());
            match parse_yaml_workflow(&content) {
                Ok(workflow_file) => {
                    print_workflow_file(&workflow_file, &format);
                }
                Err(e) => {
                    println!("{}", e.format_with_source(&content, Some(filename)).red());
                }
            }
        }

        println!();
    }

    Ok(())
}

fn print_model_file(model_file: &ModelFile, format: &OutputFormat) {
    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(model_file)
                    .unwrap_or_else(|_| "Failed to serialize".to_string())
            );
        }
        OutputFormat::Pretty => {
            for model in &model_file.models {
                println!("  {} {}", "Model:".green(), model.name.yellow());
                if let Some(ref collection) = model.collection {
                    println!("    Collection: {}", collection);
                }
                println!("    Fields: {}", model.fields.len());
                for field in &model.fields {
                    println!(
                        "      {} {}: {:?}",
                        "•".blue(),
                        field.name,
                        field.type_ref
                    );
                }
                if !model.relations.is_empty() {
                    println!("    Relations: {}", model.relations.len());
                    for rel in &model.relations {
                        println!(
                            "      {} {} -> {:?} ({:?})",
                            "•".blue(),
                            rel.name,
                            rel.target,
                            rel.relation_type
                        );
                    }
                }
            }

            if !model_file.enums.is_empty() {
                println!("  {} {}", "Enums:".green(), model_file.enums.len());
                for enum_def in &model_file.enums {
                    println!(
                        "    {} {} ({} variants)",
                        "•".blue(),
                        enum_def.name,
                        enum_def.variants.len()
                    );
                }
            }
        }
    }
}

fn print_hook_file(hook_file: &HookFile, format: &OutputFormat) {
    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(hook_file)
                    .unwrap_or_else(|_| "Failed to serialize".to_string())
            );
        }
        OutputFormat::Pretty => {
            for hook in &hook_file.hooks {
                println!("  {} {}", "Hook:".green(), hook.name.yellow());
                println!("    Model ref: {}", hook.model_ref);

                if let Some(ref sm) = hook.state_machine {
                    println!("    State Machine:");
                    println!("      Field: {}", sm.field);
                    println!("      States: {}", sm.states.len());
                    for state in &sm.states {
                        println!("        {} {}", "•".blue(), state.name);
                    }
                    println!("      Transitions: {}", sm.transitions.len());
                    for trans in &sm.transitions {
                        println!(
                            "        {} {:?} -> {}",
                            "•".blue(),
                            trans.from,
                            trans.to
                        );
                    }
                }

                println!("    Rules: {}", hook.rules.len());
                for rule in &hook.rules {
                    println!("      {} {}", "•".blue(), rule.name);
                }

                println!("    Triggers: {}", hook.triggers.len());
                for trigger in &hook.triggers {
                    println!("      {} {:?}", "•".blue(), trigger.event);
                }
            }
        }
    }
}

fn print_workflow_file(workflow_file: &WorkflowFile, format: &OutputFormat) {
    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(workflow_file)
                    .unwrap_or_else(|_| "Failed to serialize".to_string())
            );
        }
        OutputFormat::Pretty => {
            for workflow in &workflow_file.workflows {
                println!("  {} {}", "Workflow:".green(), workflow.name.yellow());
                if let Some(ref desc) = workflow.description {
                    println!("    Description: {}", desc);
                }
                println!("    Version: {}", workflow.version);
                println!("    Steps: {}", workflow.steps.len());
                for step in &workflow.steps {
                    println!("      {} {}", "•".blue(), step.name);
                }
            }
        }
    }
}

fn print_hook_index(index: &YamlHookIndexSchema, format: &OutputFormat) {
    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(index)
                    .unwrap_or_else(|_| "Failed to serialize".to_string())
            );
        }
        OutputFormat::Pretty => {
            println!("  {} (module configuration file)", "Hook Index:".green());

            if let Some(ref module) = index.module {
                println!("    Module: {}", module.yellow());
            }

            if let Some(version) = index.version {
                println!("    Version: {}", version);
            }

            if !index.imports.is_empty() {
                println!("    Imports: {}", index.imports.len());
                for import in &index.imports {
                    println!("      {} {}", "•".blue(), import);
                }
            }

            if !index.events.is_empty() {
                println!("    Domain Events: {}", index.events.len());
                for (name, event) in &index.events {
                    println!("      {} {} ({} fields)", "•".blue(), name, event.fields.len());
                }
            }

            if !index.scheduled_jobs.is_empty() {
                println!("    Scheduled Jobs: {}", index.scheduled_jobs.len());
                for (name, job) in &index.scheduled_jobs {
                    println!("      {} {} - {}", "•".blue(), name, job.schedule);
                }
            }
        }
    }
}

fn execute_validate(module: &str, warnings: bool) -> Result<()> {
    println!(
        "{} module: {}",
        "Validating".green().bold(),
        module.cyan()
    );

    if warnings {
        println!("  (including warnings)");
    }

    // Find schema path
    let schema_path = find_module_schema_path(module)?;
    let schema_files = find_schema_files(&schema_path)?;

    if schema_files.is_empty() {
        println!("{}", "No schema files found".yellow());
        return Ok(());
    }

    // Build module schema
    let (module_schema, parse_errors) = build_module_schema(module, &schema_files)?;

    if !parse_errors.is_empty() {
        for error in &parse_errors {
            println!("  {} {}", "Parse error:".red().bold(), error);
        }
        anyhow::bail!("Parsing failed with {} error(s)", parse_errors.len());
    }

    // Run resolver/validator
    match resolve_schema(&module_schema) {
        Ok(_resolved) => {
            println!("  {} All schemas are valid", "✓".green().bold());
        }
        Err(errors) => {
            for err in &errors {
                println!("  {} {}", "Error:".red().bold(), err);
            }
            println!();
            println!(
                "{} {} error(s)",
                "Validation failed:".red().bold(),
                errors.len()
            );
            anyhow::bail!("Validation failed with {} error(s)", errors.len());
        }
    }

    println!();
    println!("{} No issues found", "Validation passed:".green().bold());

    Ok(())
}

fn execute_generate(
    module: &str,
    target: &str,
    output: Option<PathBuf>,
    dry_run: bool,
    force: bool,
    split: bool,
    changed: bool,
    base: &str,
    validate: bool,
    models_filter: Option<&str>,
    hooks_filter: Option<&str>,
    workflows_filter: Option<&str>,
    lenient: bool,
) -> Result<()> {
    // If --changed flag is set, check for changes first
    if changed {
        println!(
            "{} for module: {} (comparing against {})",
            "Checking for schema changes".cyan().bold(),
            module.cyan(),
            base.yellow()
        );

        let repo_root = GitChangeDetector::find_repo_root()
            .context("Failed to find git repository root")?;

        let detector = GitChangeDetector::new(repo_root).with_base_ref(base);
        let changes = detector.get_changed_schemas(module)?;

        if changes.is_empty() {
            println!("  {} No schema changes detected", "✓".green());
            println!("  Use {} to force full generation", "--force".yellow());
            return Ok(());
        }

        // Show what changed
        let summary = ChangeSummary::from_changes(&changes);
        println!("{}", summary.display());
        println!();

        // Get affected targets
        let affected_targets = detector.get_affected_targets(&changes);
        println!(
            "  {} {}",
            "Affected targets:".blue(),
            affected_targets.join(", ").yellow()
        );
        println!();
    }

    println!(
        "{} code for module: {}",
        "Generating".green().bold(),
        module.cyan()
    );

    let targets = parse_targets(target);
    let target_names: Vec<&str> = targets
        .iter()
        .map(|t| match t {
            GenerationTarget::Proto => "proto",
            GenerationTarget::Rust => "rust",
            GenerationTarget::Sql => "sql",
            GenerationTarget::Repository => "repository",
            GenerationTarget::RepositoryTrait => "repository-trait",
            GenerationTarget::Service => "service",
            GenerationTarget::DomainService => "domain-service",
            GenerationTarget::UseCase => "usecase",
            GenerationTarget::Auth => "auth",
            GenerationTarget::Events => "events",
            GenerationTarget::StateMachine => "state-machine",
            GenerationTarget::Validator => "validator",
            // TODO: GenerationTarget::Permission => "permission",
            GenerationTarget::Handler => "handler",
            GenerationTarget::Grpc => "grpc",
            GenerationTarget::Graphql => "graphql",
            GenerationTarget::OpenApi => "openapi",
            GenerationTarget::Trigger => "trigger",
            GenerationTarget::Flow => "flow",
            GenerationTarget::Module => "module",
            GenerationTarget::Config => "config",
            GenerationTarget::ValueObject => "value-object",
            GenerationTarget::Specification => "specification",
            GenerationTarget::Cqrs => "cqrs",
            GenerationTarget::Computed => "computed",
            GenerationTarget::Projection => "projection",
            GenerationTarget::EventStore => "event-store",
            GenerationTarget::Export => "export",
            GenerationTarget::Integration => "integration",
            GenerationTarget::EventSubscription => "event-subscription",
            GenerationTarget::Dto => "dto",
            GenerationTarget::Versioning => "versioning",
            GenerationTarget::BulkOperations => "bulk-operations",
            GenerationTarget::Seeder => "seeder",
            GenerationTarget::IntegrationTest => "integration-test",
            GenerationTarget::AuditTriggers => "audit-triggers",
            // Framework compliance generators
            GenerationTarget::AppState => "app-state",
            GenerationTarget::RoutesComposer => "routes-composer",
            GenerationTarget::HandlersModule => "handlers-module",
        })
        .collect();

    println!("  Targets: {}", target_names.join(", ").yellow());

    if dry_run {
        println!("  {}", "(dry run - no files will be written)".yellow());
    }

    if force {
        println!("  {}", "(force - will overwrite existing files)".yellow());
    }

    if changed {
        println!("  {}", "(changed only - using git to detect changes)".cyan());
    }

    // Find schema path
    let schema_path = find_module_schema_path(module)?;
    let schema_files = find_schema_files(&schema_path)?;

    if schema_files.is_empty() {
        println!("{}", "No schema files found".yellow());
        return Ok(());
    }

    // Build module schema
    let (mut module_schema, parse_errors) = build_module_schema(module, &schema_files)?;

    if !parse_errors.is_empty() {
        for error in &parse_errors {
            println!("  {} {}", "Parse error:".red().bold(), error);
        }
        anyhow::bail!("Parsing failed with {} error(s)", parse_errors.len());
    }

    // Display generators config if present
    if let Some(ref config) = module_schema.generators_config {
        if let Some(ref enabled) = config.enabled {
            println!("  Enabled generators: {}", enabled.join(", ").yellow());
        }
        if let Some(ref disabled) = config.disabled {
            println!("  Disabled generators: {}", disabled.join(", ").yellow());
        }
    }

    // Apply model filter if specified
    if let Some(filter) = models_filter {
        let filter_names: Vec<&str> = filter.split(',').map(|s| s.trim()).collect();
        let original_count = module_schema.models.len();
        module_schema.models.retain(|model| {
            filter_names.iter().any(|f| {
                model.name.eq_ignore_ascii_case(f) ||
                model.name.to_lowercase().contains(&f.to_lowercase())
            })
        });
        let filtered_count = module_schema.models.len();
        println!(
            "  {} Filtered models: {} -> {} (filter: {})",
            "🔍".cyan(),
            original_count,
            filtered_count,
            filter.yellow()
        );

        // Also filter entities to match
        module_schema.entities.retain(|entity| {
            filter_names.iter().any(|f| {
                entity.name.eq_ignore_ascii_case(f) ||
                entity.name.to_lowercase().contains(&f.to_lowercase())
            })
        });
    }

    // Apply hooks filter if specified
    if let Some(filter) = hooks_filter {
        let filter_names: Vec<&str> = filter.split(',').map(|s| s.trim()).collect();
        let original_count = module_schema.hooks.len();
        module_schema.hooks.retain(|hook| {
            filter_names.iter().any(|f| {
                hook.name.eq_ignore_ascii_case(f) ||
                hook.name.to_lowercase().contains(&f.to_lowercase())
            })
        });
        let filtered_count = module_schema.hooks.len();
        println!(
            "  {} Filtered hooks: {} -> {} (filter: {})",
            "🔍".cyan(),
            original_count,
            filtered_count,
            filter.yellow()
        );
    }

    // Apply workflow filter if specified
    if let Some(filter) = workflows_filter {
        let filter_names: Vec<&str> = filter.split(',').map(|s| s.trim()).collect();
        let original_count = module_schema.workflows.len();
        module_schema.workflows.retain(|workflow| {
            filter_names.iter().any(|f| {
                workflow.name.eq_ignore_ascii_case(f) ||
                workflow.name.to_lowercase().contains(&f.to_lowercase())
            })
        });
        let filtered_count = module_schema.workflows.len();
        println!(
            "  {} Filtered workflows: {} -> {} (filter: {})",
            "🔍".cyan(),
            original_count,
            filtered_count,
            filter.yellow()
        );
    }

    // Check if filtering is active
    let is_filtered = models_filter.is_some() || hooks_filter.is_some() || workflows_filter.is_some();

    // Resolve schemas (with lenient mode for filtered generation)
    let resolved = match resolve_schema(&module_schema) {
        Ok(resolved) => resolved,
        Err(errors) => {
            if lenient || is_filtered {
                // In lenient mode or when filtering, show warnings but continue
                println!("  {} {} validation warning(s) (lenient mode)", "⚠".yellow(), errors.len());
                if !errors.is_empty() {
                    println!("    {} Use --lenient to suppress these warnings", "Tip:".blue());
                }
                // Create a basic resolved schema without strict validation
                crate::resolver::ResolvedSchema {
                    schema: module_schema.clone(),
                }
            } else {
                for err in &errors {
                    println!("  {} {}", "Error:".red().bold(), err);
                }
                anyhow::bail!("Schema validation failed with {} error(s)", errors.len());
            }
        }
    };

    // Generate code with progress bar
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    spinner.set_message("Generating code...");
    spinner.enable_steady_tick(Duration::from_millis(100));

    let options = GenerationOptions {
        split,
        group_by_domain: true,  // Always use grouped structure
    };
    let generated = generate_all_with_options(&resolved, &targets, &options)?;

    spinner.finish_and_clear();

    // Determine output directory
    // Default: module root (libs/modules/{module}/) so generated files are editable
    // The schema_path is usually libs/modules/{module}/schema, so parent is the module root
    let output_dir = output.unwrap_or_else(|| {
        schema_path
            .parent()
            .unwrap_or(&schema_path)
            .to_path_buf()
    });

    // Clean up stale generated migration files before writing new ones.
    // When force is enabled and we're generating migration files, remove old
    // generated .up.sql migrations to prevent duplicate sequence numbers.
    if force {
        let has_migration_files = generated.files.keys().any(|p| {
            p.to_string_lossy().starts_with("migrations/")
                && p.to_string_lossy().ends_with(".up.sql")
        });

        if has_migration_files {
            let migrations_dir = output_dir.join("migrations");
            if migrations_dir.exists() {
                // Collect generated migration filenames (just the file names, not full paths)
                let generated_migration_names: std::collections::HashSet<String> = generated
                    .files
                    .keys()
                    .filter_map(|p| {
                        let s = p.to_string_lossy();
                        if s.starts_with("migrations/") && s.ends_with(".up.sql") {
                            p.file_name().map(|n| n.to_string_lossy().to_string())
                        } else {
                            None
                        }
                    })
                    .collect();

                // With --force, remove ALL existing numbered migration files before writing.
                // This cleanly handles all stale file scenarios:
                //   - Old sequence numbers (e.g., 011_xxx when new gen produces 006_xxx)
                //   - Renamed models (e.g., file_access_log → access_log)
                //   - Legacy .sql format (without .up.sql suffix)
                //   - Orphaned files from removed generators or models
                //   - Cross-generator collisions (SQL vs audit triggers)
                // The generator will then write fresh files with correct numbering.
                if let Ok(entries) = fs::read_dir(&migrations_dir) {
                    let mut removed = 0;
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();

                        // Target numbered migration files: NNN_*.up.sql or NNN_*.sql
                        let is_numbered_migration = name.len() > 4
                            && name[..3].chars().all(|c| c.is_ascii_digit())
                            && name.ends_with(".sql")
                            && !name.ends_with(".down.sql");

                        if !is_numbered_migration {
                            continue;
                        }

                        // Skip files that exactly match a newly generated file
                        if generated_migration_names.contains(&name) {
                            continue;
                        }

                        if let Err(e) = fs::remove_file(entry.path()) {
                            eprintln!(
                                "  {} Failed to remove stale migration {}: {}",
                                "⚠".yellow(),
                                name,
                                e
                            );
                        } else {
                            removed += 1;
                        }
                    }
                    if removed > 0 {
                        println!(
                            "  {} Removed {} stale migration file(s)",
                            "🧹".to_string().green(),
                            removed
                        );
                    }
                }
            }
        }
    }

    println!();
    println!(
        "{} {} file(s) to generate",
        "Generated".green().bold(),
        generated.files.len()
    );

    // Create progress bar for writing files
    let pb = ProgressBar::new(generated.files.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("█▓░"),
    );

    let mut created = 0;
    let mut skipped = 0;
    let mut custom_warnings = 0;

    // Write or display generated files
    for (path, content) in &generated.files {
        let full_path = output_dir.join(path);
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        pb.set_message(file_name.to_string());

        if dry_run {
            pb.println(format!(
                "  {} {} ({} bytes)",
                "Would create:".blue(),
                full_path.display(),
                content.len()
            ));
        } else {
            // Check if file exists
            if full_path.exists() && !force {
                pb.println(format!(
                    "  {} {} (use --force to overwrite)",
                    "Skipping:".yellow(),
                    full_path.display()
                ));
                skipped += 1;
                pb.inc(1);
                continue;
            }

            // Create parent directories
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create directory {}", parent.display()))?;
            }

            // Smart merge for specific file types:
            // 1. config/application*.yml - merge YAML preserving USER values (database.url never overwritten!)
            // 2. migrations/seeds/*_seed.sql - preserve custom seed data after marker
            // 3. migrations/seeds/seed_order.yml - append new seeds to existing list
            let path_str = full_path.to_string_lossy();
            let final_content = if path_str.contains("config/application")
                && full_path.extension().and_then(|s| s.to_str()) == Some("yml") {
                // YAML config merge - USER config takes precedence over generated
                merge_yaml_config(content, &full_path)?
            } else if path_str.contains("migrations/seeds/seed_order.yml") {
                // Seed order merge - append new seeds to existing list
                merge_seed_order(content, &full_path)?
            } else if path_str.contains("migrations/seeds/")
                && full_path.extension().and_then(|s| s.to_str()) == Some("sql") {
                // SQL seed file merge - preserve custom data after marker
                merge_seed_file(content, &full_path)?
            } else if full_path.extension().and_then(|s| s.to_str()) == Some("rs") {
                // Detect unprotected custom code before merge
                let warnings = detect_unprotected_custom_code(content, &full_path);
                if !warnings.is_empty() {
                    custom_warnings += warnings.len();
                    pb.println(format!(
                        "  {} {} has {} unprotected custom line(s) that may be lost:",
                        "⚠".yellow(), full_path.display(), warnings.len()
                    ));
                    for (idx, line) in warnings.iter().take(5).enumerate() {
                        pb.println(format!("    {}. {}", idx + 1, line.trim()));
                    }
                    if warnings.len() > 5 {
                        pb.println(format!("    ... and {} more", warnings.len() - 5));
                    }
                    pb.println(format!(
                        "    {} Wrap custom code with `// <<< CUSTOM CODE START >>>` markers",
                        "Tip:".cyan()
                    ));
                }
                // Rust file merge - preserve // <<< CUSTOM blocks in all .rs files
                merge_rust_mod_custom(content, &full_path)?
            } else {
                // Default: use generated content as-is
                content.clone()
            };

            // Write file
            fs::write(&full_path, final_content)
                .with_context(|| format!("Failed to write {}", full_path.display()))?;

            pb.println(format!("  {} {}", "✓".green(), full_path.display()));
            created += 1;
        }

        pb.inc(1);
    }

    pb.finish_and_clear();

    println!();
    if dry_run {
        println!("{} {} file(s) would be created", "Dry run:".blue().bold(), generated.files.len());
    } else {
        println!(
            "{} {} created, {} skipped{}",
            "Complete:".green().bold(),
            created.to_string().green(),
            skipped.to_string().yellow(),
            if custom_warnings > 0 {
                format!(", {} custom code warning(s)", custom_warnings.to_string().yellow())
            } else {
                String::new()
            }
        );
    }

    // Post-generation validation: run cargo check to verify compilation
    if validate && !dry_run && created > 0 {
        println!();
        println!("{}", "Validating generated code...".cyan().bold());

        // Determine the package name from module name
        let package_name = format!("backbone-{}", module.to_lowercase().replace('-', "_"));

        // Run cargo check on the module
        let result = std::process::Command::new("cargo")
            .args(["check", "--package", &package_name])
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    println!("  {} Generated code compiles successfully", "✓".green());
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    println!("  {} Generated code has compilation errors:", "✗".red());
                    // Print first 20 lines of errors
                    for line in stderr.lines().take(20) {
                        println!("    {}", line.red());
                    }
                    if stderr.lines().count() > 20 {
                        println!("    {} ...", "...".dimmed());
                    }
                    anyhow::bail!("Compilation failed. Please fix the schema generator or the schema definitions.");
                }
            }
            Err(e) => {
                println!("  {} Failed to run cargo check: {}", "Warning:".yellow(), e);
                println!("  {} Skipping validation", "→".dimmed());
            }
        }
    }

    Ok(())
}

fn execute_diff(module: &str, base: &str) -> Result<()> {
    println!(
        "{} for module: {} (comparing against {})",
        "Showing diff".green().bold(),
        module.cyan(),
        base.yellow()
    );

    // Find schema path
    let schema_path = find_module_schema_path(module)?;
    let schema_files = find_schema_files(&schema_path)?;

    if schema_files.is_empty() {
        println!("{}", "No schema files found".yellow());
        return Ok(());
    }

    // Build module schema
    let (module_schema, _) = build_module_schema(module, &schema_files)?;

    // Resolve schemas
    let resolved = resolve_schema(&module_schema)
        .map_err(|_| anyhow::anyhow!("Schema validation failed"))?;

    // Generate all code
    let targets = GenerationTarget::all();
    let generated = generate_all_with_options(&resolved, &targets, &GenerationOptions::default())?;

    // Determine output directory (module root)
    let output_dir = schema_path
        .parent()
        .unwrap_or(&schema_path)
        .to_path_buf();

    println!();
    let mut changes = 0;

    for (path, new_content) in &generated.files {
        let full_path = output_dir.join(path);

        if !full_path.exists() {
            println!("  {} {}", "New file:".green(), full_path.display());
            changes += 1;
            continue;
        }

        let existing_content = fs::read_to_string(&full_path).unwrap_or_default();

        if existing_content != *new_content {
            println!("  {} {}", "Modified:".yellow(), full_path.display());

            // Show simple line count diff
            let old_lines = existing_content.lines().count();
            let new_lines = new_content.lines().count();
            let diff = new_lines as i64 - old_lines as i64;

            if diff > 0 {
                println!(
                    "    {} lines, {} lines",
                    format!("+{}", diff).green(),
                    "-0".to_string().red()
                );
            } else if diff < 0 {
                println!(
                    "    {} lines, {} lines",
                    "+0".green(),
                    format!("{}", diff).red()
                );
            } else {
                println!("    Content changed (same line count)");
            }

            changes += 1;
        }
    }

    if changes == 0 {
        println!("  {} Generated code is up to date", "✓".green());
    } else {
        println!();
        println!(
            "{} {} file(s) would change",
            "Summary:".cyan().bold(),
            changes
        );
        println!("  Run {} to update", "backbone schema generate".yellow());
    }

    Ok(())
}

/// Build a ModuleSchema from schema files (supports both .schema and .yaml formats)
fn build_module_schema(
    module_name: &str,
    schema_files: &[PathBuf],
) -> Result<(ModuleSchema, Vec<String>)> {
    let mut module_schema = ModuleSchema::new(module_name);
    let mut errors = Vec::new();

    // First pass: collect shared_types from index files
    let mut resolved_shared_types: IndexMap<String, IndexMap<String, YamlField>> = IndexMap::new();

    for file in schema_files {
        let content = fs::read_to_string(file)
            .with_context(|| format!("Failed to read {}", file.display()))?;

        let filename = file.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Process index.model.yaml files first
        if filename == "index.model.yaml" {
            match parse_yaml_model_flexible(&content) {
                Ok(ModelParseResult::Index(index_schema)) => {
                    // Set module name from index if available
                    if let Some(name) = &index_schema.module {
                        module_schema.name = name.clone();
                    }
                    // Propagate generators config from index.model.yaml
                    if let Some(config) = &index_schema.config {
                        module_schema.generators_config = config.generators.clone();
                    }
                    // Resolve shared types (handles composition like [Timestamps, Actors])
                    resolved_shared_types = resolve_shared_types(&index_schema.shared_types);
                }
                Ok(ModelParseResult::Model(_)) => {
                    // index.model.yaml parsed as regular model - unusual but ok
                }
                Err(e) => errors.push(e.format_with_source(&content, Some(filename))),
            }
        }
    }

    // Store resolved shared types in module schema for use during generation
    module_schema.shared_types = resolved_shared_types.clone();

    // Second pass: parse all other files
    for file in schema_files {
        let content = fs::read_to_string(file)
            .with_context(|| format!("Failed to read {}", file.display()))?;

        let filename = file.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Skip index.model.yaml - already processed
        if filename == "index.model.yaml" {
            continue;
        }

        // Legacy .schema format
        if filename.ends_with(".model.schema") {
            match parse_model(&content) {
                Ok(model_file) => module_schema.merge_model_file(model_file),
                Err(e) => errors.push(e.format_with_source(&content, Some(filename))),
            }
        } else if filename.ends_with(".hook.schema") || filename.ends_with(".workflow.schema") {
            match parse_hook(&content) {
                Ok(hook_file) => module_schema.merge_hook_file(hook_file),
                Err(e) => errors.push(e.format_with_source(&content, Some(filename))),
            }
        }
        // New YAML format
        else if filename.ends_with(".model.yaml") {
            match crate::parser::parse_model_yaml_str(&content) {
                Ok(yaml_schema) => {
                    // Extract enums first (before consuming yaml_schema)
                    // Check for duplicate enum names across schema files
                    let enums: Vec<_> = yaml_schema.enums.to_vec();
                    for yaml_enum in enums {
                        let enum_def = yaml_enum.into_enum();
                        if let Some(existing) = module_schema.enums.iter().find(|e| e.name == enum_def.name) {
                            errors.push(format!(
                                "Duplicate enum '{}' defined in '{}' — already defined with {} variant(s). \
                                 Rename one to avoid conflicts (e.g., '{}' → 'Template{}').",
                                enum_def.name, filename,
                                existing.variants.len(),
                                enum_def.name, enum_def.name,
                            ));
                        } else {
                            module_schema.enums.push(enum_def);
                        }
                    }

                    // =================================================================
                    // DDD EXTENSIONS: Extract entities, value objects, domain services,
                    // event sourcing configs, and authorization before consuming yaml_schema
                    // =================================================================

                    // Extract entities (enhanced models with behavior)
                    let entities: Vec<_> = yaml_schema.entities
                        .iter()
                        .map(|(name, entity)| entity.clone().into_entity(name.clone()))
                        .collect();

                    // Extract value objects
                    let value_objects: Vec<_> = yaml_schema.value_objects
                        .iter()
                        .map(|(name, vo)| vo.clone().into_value_object(name.clone()))
                        .collect();

                    // Extract domain services
                    let domain_services: Vec<_> = yaml_schema.domain_services
                        .iter()
                        .map(|(name, ds)| ds.clone().into_domain_service(name.clone()))
                        .collect();

                    // Extract event sourcing configs
                    let event_sourced: Vec<_> = yaml_schema.event_sourced
                        .iter()
                        .map(|(name, es)| es.clone().into_event_sourced(name.clone()))
                        .collect();

                    // Extract authorization config
                    let authorization = yaml_schema.authorization
                        .as_ref()
                        .map(|auth| auth.clone().into_authorization());

                    // Extract use cases (application layer operations)
                    let usecases: Vec<_> = yaml_schema.usecases
                        .iter()
                        .map(|(name, uc)| uc.clone().into_usecase(name.clone()))
                        .collect();

                    // Extract domain events
                    let events: Vec<_> = yaml_schema.events
                        .iter()
                        .map(|(name, ev)| ev.clone().into_domain_event(name.clone()))
                        .collect();

                    // Merge DDD extensions into module schema
                    module_schema.merge_ddd_extensions(
                        entities,
                        value_objects,
                        domain_services,
                        event_sourced,
                        authorization,
                        usecases,
                        events,
                    );

                    // =================================================================
                    // CQRS & PRESENTATION EXTENSIONS: Extract projections, services,
                    // handlers, subscriptions, integrations, presentation, DTOs,
                    // versioning, and repository traits
                    // =================================================================

                    // Extract CQRS projections (read models)
                    let projections: Vec<_> = yaml_schema.projections
                        .iter()
                        .map(|(name, proj)| proj.clone().into_projection(name.clone()))
                        .collect();

                    // Extract application services
                    let services: Vec<_> = yaml_schema.services
                        .iter()
                        .map(|(name, svc)| svc.clone().into_app_service(name.clone()))
                        .collect();

                    // Extract event handlers
                    let handlers: Vec<_> = yaml_schema.handlers
                        .iter()
                        .map(|(name, h)| h.clone().into_handler(name.clone()))
                        .collect();

                    // Extract event subscriptions (cross-module)
                    let subscriptions: Vec<_> = yaml_schema.subscribes_to
                        .iter()
                        .flat_map(|(module, events_map)| {
                            events_map.iter().map(move |(event, sub)| {
                                sub.clone().into_subscription(module.clone(), event.clone())
                            })
                        })
                        .collect();

                    // Extract integration adapters (ACL)
                    let integrations: Vec<_> = yaml_schema.integration
                        .iter()
                        .map(|(name, intg)| intg.clone().into_integration(name.clone()))
                        .collect();

                    // Extract presentation layer config
                    let presentation = yaml_schema.presentation
                        .as_ref()
                        .map(|p| p.clone().into_presentation());

                    // Extract DTOs
                    let dtos: Vec<_> = yaml_schema.dtos
                        .iter()
                        .map(|(name, dto)| dto.clone().into_dto(name.clone()))
                        .collect();

                    // Extract versioning config
                    let versioning = yaml_schema.versioning
                        .as_ref()
                        .map(|v| v.clone().into_versioning());

                    // Extract repository traits
                    let traits: Vec<_> = yaml_schema.traits
                        .iter()
                        .map(|(name, tr)| tr.clone().into_repository_trait(name.clone()))
                        .collect();

                    // Merge CQRS & presentation extensions into module schema
                    module_schema.merge_cqrs_extensions(
                        projections,
                        services,
                        handlers,
                        subscriptions,
                        integrations,
                        presentation,
                        dtos,
                        versioning,
                        traits,
                    );

                    // Convert with shared types context for extends and JSONB type support
                    let models = yaml_schema.into_models_with_context(&resolved_shared_types);
                    for model in models {
                        if let Some(_existing) = module_schema.models.iter().find(|m| m.name == model.name) {
                            errors.push(format!(
                                "Duplicate model '{}' defined in '{}' — already defined in another schema file. \
                                 Each model name must be unique within a module.",
                                model.name, filename,
                            ));
                        } else {
                            module_schema.models.push(model);
                        }
                    }
                }
                Err(e) => errors.push(format!("{}: {}", filename, e)),
            }
        } else if filename.ends_with(".hook.yaml") {
            match parse_yaml_hook_flexible(&content) {
                Ok(HookParseResult::Hook(hook_file)) => {
                    module_schema.merge_hook_file(hook_file)
                }
                Ok(HookParseResult::Index(index_schema)) => {
                    // Index files are module-level config, not actual hooks
                    // We can store the metadata if needed, but skip hook merging
                    if let Some(module_name) = &index_schema.module {
                        module_schema.name = module_name.clone();
                    }
                    // Events from index files could be added to module schema in the future
                }
                Err(e) => errors.push(e.format_with_source(&content, Some(filename))),
            }
        } else if filename.ends_with(".workflow.yaml") {
            match parse_yaml_workflow(&content) {
                Ok(workflow_file) => module_schema.merge_workflow_file(workflow_file),
                Err(e) => errors.push(e.format_with_source(&content, Some(filename))),
            }
        }
    }

    Ok((module_schema, errors))
}

/// Normalize a line for comparison by trimming whitespace and removing `.clone()` calls.
/// Centralised so future normalisation improvements apply everywhere.
fn normalize_line(line: &str) -> String {
    line.trim().replace(".clone()", "").replace("  ", " ")
}

/// Walk backwards from `start_index` to find the nearest non-empty, non-CUSTOM line.
fn find_anchor_line(existing_lines: &[&str], start_index: usize) -> Option<String> {
    if start_index == 0 {
        return None;
    }
    let mut j = start_index - 1;
    loop {
        let prev = existing_lines[j].trim();
        if !prev.is_empty() && !prev.contains("// <<< CUSTOM") && !prev.contains("END CUSTOM") {
            // Skip pure closing braces — they are not unique enough as anchors
            // and can match at wrong positions in the regenerated content.
            if matches!(prev, "}" | "})" | "});" | "}," | "};" | "}" ) {
                if j == 0 { return None; }
                j -= 1;
                continue;
            }
            return Some(existing_lines[j].to_string());
        }
        if j == 0 {
            return None;
        }
        j -= 1;
    }
}

/// Phase 2 collection: scan existing content for `// <<< CUSTOM` markers and collect
/// each block together with its anchor line. Paired `END CUSTOM` blocks are preserved
/// verbatim; inline markers filter out lines already present in `generated_content`.
fn collect_custom_blocks(existing_content: &str, generated_content: &str) -> Vec<(Option<String>, Vec<String>)> {
    let mut custom_blocks: Vec<(Option<String>, Vec<String>)> = Vec::new();
    let existing_lines: Vec<&str> = existing_content.lines().collect();
    let generated_lines: Vec<&str> = generated_content.lines().collect();
    let mut i = 0;

    while i < existing_lines.len() {
        let line = existing_lines[i];
        // Skip lines that are part of CUSTOM METHODS START/END blocks (handled in Phase 1)
        if line.contains("CUSTOM METHODS START") || line.contains("CUSTOM METHODS END") {
            i += 1;
            continue;
        }
        if line.contains("// <<< CUSTOM") {
            // Find the preceding non-empty generated line as an anchor
            let anchor = find_anchor_line(&existing_lines, i);

            // Check if this custom block has a paired END CUSTOM marker
            let has_end_marker = existing_lines[i+1..].iter()
                .take_while(|l| !l.contains("// <<< CUSTOM"))
                .any(|l| l.contains("END CUSTOM"));

            let mut block_lines = vec![line.to_string()];
            i += 1;

            if has_end_marker {
                // Paired block: preserve ALL lines verbatim until END CUSTOM.
                // The `has_end_marker` scan above already confirmed a closing
                // marker exists before the next `// <<< CUSTOM`, so we loop
                // without an arbitrary line cap — capping here truncates
                // legitimate multi-line blocks (e.g. large `matches!` macros).
                while i < existing_lines.len() {
                    let next = existing_lines[i];
                    if next.contains("END CUSTOM") {
                        block_lines.push(next.to_string());
                        i += 1;
                        break;
                    }
                    block_lines.push(next.to_string());
                    i += 1;
                }
            } else {
                // Inline marker: filter lines that already exist in generated content
                while i < existing_lines.len() {
                    let next = existing_lines[i];
                    // Stop at empty lines or next custom block
                    if next.trim().is_empty() || next.contains("// <<< CUSTOM") {
                        break;
                    }
                    // Include any line that is NOT in the generated content
                    let is_generated = generated_lines.iter()
                        .any(|gl| gl.trim() == next.trim());
                    if !is_generated {
                        block_lines.push(next.to_string());
                    }
                    i += 1;
                }
            }

            // Skip empty paired blocks (just markers, no content) to prevent accumulation
            let has_content = block_lines.iter().any(|l| {
                !l.contains("// <<< CUSTOM") && !l.contains("END CUSTOM") && !l.trim().is_empty()
            });
            if has_content {
                custom_blocks.push((anchor, block_lines));
            }
        } else {
            i += 1;
        }
    }

    custom_blocks
}

/// Insert previously collected custom blocks into `result_lines`, using fuzzy
/// anchor matching and dedup checks to avoid duplicating content.
fn insert_custom_blocks(result_lines: &mut Vec<String>, custom_blocks: &[(Option<String>, Vec<String>)]) {
    for (anchor, block_lines) in custom_blocks {
        // Dedup: skip if the first real content line already exists in the result
        let first_content_line = block_lines.iter()
            .find(|l| !l.contains("// <<< CUSTOM") && !l.contains("END CUSTOM") && !l.trim().is_empty());
        if let Some(content_line) = first_content_line {
            let content_normalized = normalize_line(content_line);
            let already_in_result = result_lines.iter()
                .any(|rl| normalize_line(rl) == content_normalized);
            if already_in_result {
                eprintln!("  Custom block already present (dedup), skipping");
                continue;
            }
        }

        let insert_pos = if let Some(anchor_line) = anchor {
            // Find the anchor line in generated content.
            // Use multiple matching strategies to handle minor differences
            // between existing and regenerated lines (whitespace, .clone(), etc.)
            let anchor_trimmed = anchor_line.trim();
            let anchor_normalized = normalize_line(anchor_line);

            // 1. Exact match
            let pos = result_lines.iter().rposition(|l| l == anchor_line)
                // 2. Trimmed match (handles trailing whitespace / indentation changes)
                .or_else(|| result_lines.iter().rposition(|l| l.trim() == anchor_trimmed))
                // 3. Normalized match (handles .clone() differences)
                .or_else(|| result_lines.iter().rposition(|l| {
                    normalize_line(l) == anchor_normalized
                }));

            pos.map(|p| p + 1)
        } else {
            None
        };

        if let Some(pos) = insert_pos {
            // Check if a custom block placeholder already exists at this position
            let has_custom_at_pos = pos < result_lines.len()
                && result_lines[pos].contains("// <<< CUSTOM");
            if has_custom_at_pos {
                // Check if the existing block is an empty placeholder (// <<< CUSTOM followed by // END CUSTOM)
                let is_empty_placeholder = pos + 1 < result_lines.len()
                    && result_lines[pos + 1].contains("END CUSTOM");
                if is_empty_placeholder {
                    // Replace the empty placeholder with our custom content
                    result_lines.remove(pos + 1); // remove // END CUSTOM
                    result_lines.remove(pos);      // remove // <<< CUSTOM
                    for (j, custom_line) in block_lines.iter().enumerate() {
                        result_lines.insert(pos + j, custom_line.clone());
                    }
                }
                // If not empty, skip (content already present)
            } else {
                for (j, custom_line) in block_lines.iter().enumerate() {
                    result_lines.insert(pos + j, custom_line.clone());
                }
            }
        } else {
            // No anchor found, append at end
            eprintln!("  Warning: No anchor found for custom block, appending at end of file");
            for custom_line in block_lines {
                result_lines.push(custom_line.clone());
            }
        }
    }
}

/// Merge Rust mod.rs files, preserving lines marked with `// <<< CUSTOM`
///
/// Scans the existing file for lines containing `// <<< CUSTOM` and any
/// non-empty lines immediately following them (the custom module/use declarations).
/// These custom blocks are appended to the newly generated mod.rs content,
/// preserving their position relative to the preceding generated line.
///
/// For paired markers like `// <<< CUSTOM METHODS START >>>` and
/// `// <<< CUSTOM METHODS END >>>`, ALL lines between the markers are preserved
/// verbatim (no filtering against generated content).
fn merge_rust_mod_custom(generated_content: &str, existing_path: &Path) -> Result<String> {
    if !existing_path.exists() {
        return Ok(generated_content.to_string());
    }

    let existing_content = fs::read_to_string(existing_path)
        .with_context(|| format!("Failed to read existing mod file: {:?}", existing_path))?;

    // Phase 1: paired CUSTOM METHODS blocks
    let result = merge_custom_methods_block(generated_content, &existing_content);

    // Phase 2: single-line // <<< CUSTOM markers
    let custom_blocks = collect_custom_blocks(&existing_content, &result);
    if custom_blocks.is_empty() {
        return Ok(result);
    }

    let mut result_lines: Vec<String> = result.lines().map(|l| l.to_string()).collect();
    insert_custom_blocks(&mut result_lines, &custom_blocks);
    Ok(result_lines.join("\n"))
}

/// Merge paired `// <<< CUSTOM METHODS START >>>` / `// <<< CUSTOM METHODS END >>>` blocks.
///
/// Extracts ALL content between the markers from the existing file and replaces
/// the corresponding block in the generated content. Unlike the single-line custom
/// marker merge, this preserves every line verbatim (no filtering).
///
/// **Migration support**: If the existing file has an empty custom block but has
/// DDD method implementations in the old generated section (outside custom markers),
/// those are migrated into the custom block automatically.
fn merge_custom_methods_block(generated_content: &str, existing_content: &str) -> String {
    let start_marker = "// <<< CUSTOM METHODS START >>>";
    let end_marker = "// <<< CUSTOM METHODS END >>>";

    // Extract custom methods block from existing file
    let existing_lines: Vec<&str> = existing_content.lines().collect();
    let start_idx = existing_lines.iter().position(|l| l.contains(start_marker));
    let end_idx = existing_lines.iter().position(|l| l.contains(end_marker));

    let existing_block = match (start_idx, end_idx) {
        (Some(s), Some(e)) if e > s => {
            // Check if block has real content (not just placeholder comment)
            let inner_lines: Vec<&str> = existing_lines[s + 1..e]
                .iter()
                .copied()
                .filter(|l| !l.trim().is_empty())
                .collect();
            let has_real_content = inner_lines.iter().any(|l| {
                let trimmed = l.trim();
                // Real content = not a comment, or a comment with actual code guidance
                // Exclude pure placeholder comments like "// Add custom entity methods here"
                !trimmed.starts_with("//")
                    || trimmed.starts_with("/// ")
                    || trimmed.contains("TODO")
            });
            if has_real_content {
                // Preserve ALL lines between markers (inclusive of markers)
                Some(existing_lines[s..=e].to_vec())
            } else {
                // Empty custom block - check if old DDD section has implementations
                migrate_old_ddd_section(&existing_lines, s, e)
            }
        }
        _ => None,
    };

    // If no existing custom content, return generated as-is
    let existing_block = match existing_block {
        Some(b) => b,
        None => return generated_content.to_string(),
    };

    // Replace the generated CUSTOM METHODS block with the existing one
    let gen_lines: Vec<&str> = generated_content.lines().collect();
    let gen_start = gen_lines.iter().position(|l| l.contains(start_marker));
    let gen_end = gen_lines.iter().position(|l| l.contains(end_marker));

    match (gen_start, gen_end) {
        (Some(gs), Some(ge)) if ge > gs => {
            let mut result_lines: Vec<String> = Vec::new();
            // Lines before the generated CUSTOM block
            for line in &gen_lines[..gs] {
                result_lines.push(line.to_string());
            }
            // Insert existing custom block
            for line in &existing_block {
                result_lines.push(line.to_string());
            }
            // Lines after the generated CUSTOM block
            for line in &gen_lines[ge + 1..] {
                result_lines.push(line.to_string());
            }
            result_lines.join("\n")
        }
        _ => generated_content.to_string(),
    }
}

/// Migrate DDD method implementations from the old generated section into the
/// custom methods block. This handles the one-time transition from the old format
/// (DDD methods in generated section) to the new format (DDD methods in custom block).
///
/// Looks for a `// DDD Entity Methods` section before the custom markers that has
/// real implementations (not `todo!()` stubs).
fn migrate_old_ddd_section<'a>(
    existing_lines: &[&'a str],
    custom_start: usize,
    custom_end: usize,
) -> Option<Vec<&'a str>> {
    // Find the DDD Entity Methods section before the custom block
    let ddd_header_idx = existing_lines[..custom_start].iter().position(|l| {
        l.contains("DDD Entity Methods")
    });

    let ddd_start = match ddd_header_idx {
        Some(idx) => {
            // Walk backwards to find the section separator (// ====...)
            let mut start = idx;
            if start > 0 && existing_lines[start - 1].contains("// ===") {
                start -= 1;
            }
            start
        }
        None => return None,
    };

    // Check if any DDD method has a real implementation (not todo!())
    let ddd_section = &existing_lines[ddd_start..custom_start];
    let has_implementations = ddd_section.iter().any(|l| {
        let trimmed = l.trim();
        // Look for actual code, not just stubs
        (trimmed.starts_with("if ") || trimmed.starts_with("match ") ||
         trimmed.starts_with("self.") || trimmed.starts_with("let ") ||
         trimmed.starts_with("return ") || trimmed.starts_with("errors.push") ||
         trimmed.contains("!self.") || trimmed.contains(".is_") ||
         trimmed.contains(".max(") || trimmed.contains(".min("))
        && !trimmed.contains("todo!(")
    });

    if !has_implementations {
        return None;
    }

    // Also collect any check_invariants section
    let _invariants_idx = existing_lines[ddd_start..custom_start].iter().position(|l| {
        l.contains("fn check_invariants")
    }).map(|i| i + ddd_start);

    // Build the migrated block
    let mut block: Vec<&str> = Vec::new();
    block.push(existing_lines[custom_start]); // START marker

    // Add DDD section content
    block.push("");
    for line in &existing_lines[ddd_start..custom_start] {
        block.push(line);
    }

    // If there's a check_invariants that was separate, it's already included
    // since it's between ddd_start and custom_start

    block.push(existing_lines[custom_end]); // END marker

    Some(block)
}

/// Detect lines in an existing file that are NOT in the generated content
/// and NOT inside `// <<< CUSTOM` blocks. These lines would be silently lost
/// during regeneration unless wrapped in custom code markers.
fn detect_unprotected_custom_code(generated_content: &str, existing_path: &Path) -> Vec<String> {
    if !existing_path.exists() {
        return Vec::new();
    }

    let existing_content = match fs::read_to_string(existing_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let generated_lines: std::collections::HashSet<&str> = generated_content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    // Boilerplate patterns to ignore (common Rust syntax that varies between generations)
    let boilerplate_prefixes = [
        "use ", "pub use ", "mod ", "pub mod ", "#[", "//", "}", "{",
        "pub struct ", "pub enum ", "pub trait ", "pub fn ", "pub async fn ",
        "impl ", "fn ", "async fn ", "struct ", "enum ", "trait ",
        "pub type ", "type ", "pub const ", "const ", "pub static ", "static ",
        "super::", "self::", "crate::", "extern ", "where ",
    ];

    let mut in_custom_block = false;
    let mut unprotected: Vec<String> = Vec::new();

    for line in existing_content.lines() {
        let trimmed = line.trim();

        // Track custom block boundaries
        if trimmed.contains("// <<< CUSTOM") {
            in_custom_block = true;
            continue;
        }
        if trimmed.contains("// END CUSTOM") || (in_custom_block && trimmed.is_empty()) {
            in_custom_block = false;
            continue;
        }

        // Skip lines inside custom blocks (already protected)
        if in_custom_block {
            continue;
        }

        // Skip empty/whitespace lines
        if trimmed.is_empty() {
            continue;
        }

        // Skip lines that exist in generated content
        if generated_lines.contains(trimmed) {
            continue;
        }

        // Skip common boilerplate
        if boilerplate_prefixes.iter().any(|p| trimmed.starts_with(p)) {
            continue;
        }

        // Skip closing braces and single-char lines
        if trimmed.len() <= 2 {
            continue;
        }

        // This line is custom, not protected, and not boilerplate
        unprotected.push(line.to_string());
    }

    unprotected
}

/// Extract custom seed data from existing seed file content
/// Returns content below the `-- <<< CUSTOM SEED DATA >>>` marker
/// Strips common trailing comments like "-- Add your custom seed data below"
fn extract_custom_seed_data(content: &str) -> Option<String> {
    let marker = "-- <<< CUSTOM SEED DATA >>>";
    content.find(marker).map(|pos| {
        let after_marker = &content[pos + marker.len()..];
        let trimmed = after_marker.trim();
        // Remove common trailing comments if present
        let trailing_comments = [
            "-- Add your custom seed data below",
            "-- Add your custom seed data",
            "Add your custom seed data below",
        ];
        for comment in trailing_comments {
            if let Some(stripped) = trimmed.strip_prefix(comment) {
                return stripped.trim().to_string();
            }
        }
        trimmed.to_string()
    })
}

/// Merge generated seed content with existing custom seed data
fn merge_seed_file(generated_content: &str, existing_path: &Path) -> Result<String> {
    if !existing_path.exists() {
        return Ok(generated_content.to_string());
    }

    let existing_content = fs::read_to_string(existing_path)
        .with_context(|| format!("Failed to read existing seed file: {:?}", existing_path))?;

    // Extract custom data from existing file
    if let Some(custom_data) = extract_custom_seed_data(&existing_content) {
        // Append custom data to generated content
        Ok(format!("{}\n\n{}", generated_content.trim_end(), custom_data))
    } else {
        // No custom data, use generated content
        Ok(generated_content.to_string())
    }
}

/// Merge YAML config files, preserving user customizations
/// For config/application*.yml files:
/// - Preserves user-defined values that aren't in the generated content
/// - User's existing config ALWAYS takes precedence over generated config
/// - Only adds new keys from generated that don't exist in existing
fn merge_yaml_config(generated_content: &str, existing_path: &Path) -> Result<String> {
    if !existing_path.exists() {
        return Ok(generated_content.to_string());
    }

    let existing_content = fs::read_to_string(existing_path)
        .with_context(|| format!("Failed to read existing config file: {:?}", existing_path))?;

    // Parse both YAMLs
    let generated_value: serde_yaml::Value = serde_yaml::from_str(generated_content)
        .with_context(|| format!("Failed to parse generated config: {:?}", existing_path))?;
    let existing_value: serde_yaml::Value = serde_yaml::from_str(&existing_content)
        .with_context(|| format!("Failed to parse existing config: {:?}", existing_path))?;

    // IMPORTANT: Existing (user) config takes precedence over generated
    // This ensures database.url and other user settings are never overwritten
    let merged = deep_merge_yaml_preserve_user(existing_value, generated_value);

    serde_yaml::to_string(&merged)
        .with_context(|| format!("Failed to serialize merged config: {:?}", existing_path))
}

/// Deep merge two YAML values, with `user` (existing) taking precedence over `generated`
/// - Keys in `user` are ALWAYS kept (never overwritten)
/// - New keys from `generated` that don't exist in `user` are added
/// - For nested mappings, recursively merge with user taking precedence
fn deep_merge_yaml_preserve_user(user: serde_yaml::Value, generated: serde_yaml::Value) -> serde_yaml::Value {
    match (user, generated) {
        (serde_yaml::Value::Mapping(user_map), serde_yaml::Value::Mapping(generated_map)) => {
            let mut merged = user_map.clone();

            for (key, generated_value) in generated_map {
                let merged_value = match merged.get(&key) {
                    // User has this key - merge recursively if both are mappings
                    Some(user_value) => {
                        if let (serde_yaml::Value::Mapping(_), serde_yaml::Value::Mapping(_)) =
                            (user_value, &generated_value)
                        {
                            deep_merge_yaml_preserve_user(user_value.clone(), generated_value)
                        } else {
                            // User value takes precedence (not a mapping)
                            user_value.clone()
                        }
                    }
                    // User doesn't have this key - add from generated
                    None => generated_value,
                };
                merged.insert(key, merged_value);
            }
            serde_yaml::Value::Mapping(merged)
        }
        (user_value, _) => user_value,
    }
}

/// Extract seed names from YAML content (lines starting with '-')
fn extract_seed_names(content: &str) -> std::collections::HashSet<String> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed.strip_prefix('-')?.trim().to_string().into()
        })
        .collect()
}

/// Append new seeds that aren't in the existing file to the result
fn append_new_seeds(
    generated_seeds: &std::collections::HashSet<String>,
    existing_seeds: &std::collections::HashSet<String>,
    result: &mut String,
) {
    let new_seeds: Vec<_> = generated_seeds
        .iter()
        .filter(|seed| !existing_seeds.contains(*seed))
        .collect();

    if new_seeds.is_empty() {
        return;
    }

    result.push_str("\n# Newly added seeds (preserve manual order above):\n");
    for seed in new_seeds {
        result.push_str(&format!("- {}\n", seed));
    }
}

/// Process a single line from seed_order.yml during merge
fn process_seed_order_line(
    line: &str,
    in_seed_list: &mut bool,
    existing_seeds: &mut std::collections::HashSet<String>,
    result: &mut String,
) {
    let trimmed = line.trim();

    // Copy header comments as-is
    if trimmed.starts_with('#') || trimmed.is_empty() {
        result.push_str(line);
        result.push('\n');
        return;
    }

    // Track seed entries
    if trimmed.starts_with('-') {
        *in_seed_list = true;
        if let Some(seed_name) = trimmed.strip_prefix('-').map(|s| s.trim()) {
            existing_seeds.insert(seed_name.to_string());
        }
        result.push_str(line);
        result.push('\n');
        return;
    }

    // Copy lines before seed list starts
    if !*in_seed_list {
        result.push_str(line);
        result.push('\n');
    }
}

/// Merge seed_order.yml, appending new seeds to existing list
/// Preserves existing order and user customizations, only adds new entries
fn merge_seed_order(generated_content: &str, existing_path: &Path) -> Result<String> {
    if !existing_path.exists() {
        return Ok(generated_content.to_string());
    }

    let existing_content = fs::read_to_string(existing_path)
        .with_context(|| format!("Failed to read existing seed_order.yml: {:?}", existing_path))?;

    let generated_seeds = extract_seed_names(generated_content);

    let mut result = String::new();
    let mut existing_seeds = std::collections::HashSet::new();
    let mut in_seed_list = false;

    for line in existing_content.lines() {
        process_seed_order_line(line, &mut in_seed_list, &mut existing_seeds, &mut result);
    }

    append_new_seeds(&generated_seeds, &existing_seeds, &mut result);

    Ok(result)
}

#[cfg(test)]
mod merge_tests {
    use super::*;

    #[test]
    fn test_deep_merge_yaml_preserve_user_adds_new_keys() {
        let user = serde_yaml::from_str::<serde_yaml::Value>("a: 1").unwrap();
        let generated = serde_yaml::from_str::<serde_yaml::Value>("b: 2").unwrap();
        let result = deep_merge_yaml_preserve_user(user, generated);

        assert!(result.get("a").is_some());
        assert!(result.get("b").is_some());
    }

    #[test]
    fn test_deep_merge_yaml_user_takes_precedence() {
        let user = serde_yaml::from_str::<serde_yaml::Value>("a: 1\nb: 2").unwrap();
        let generated = serde_yaml::from_str::<serde_yaml::Value>("b: 999\nc: 3").unwrap();
        let result = deep_merge_yaml_preserve_user(user, generated);

        // User's value for 'b' should win (not generated's 999)
        let b_value = result.get("b").unwrap().as_i64().unwrap();
        assert_eq!(b_value, 2);
        assert!(result.get("c").is_some());
    }

    #[test]
    fn test_deep_merge_yaml_database_url_never_overridden() {
        let user = serde_yaml::from_str::<serde_yaml::Value>(
            "database:\n  url: postgresql://user:pass@localhost/db\n  max_connections: 10"
        ).unwrap();
        let generated = serde_yaml::from_str::<serde_yaml::Value>(
            "database:\n  url: postgresql://default:123@host/defaultdb\n  pool_size: 5"
        ).unwrap();
        let result = deep_merge_yaml_preserve_user(user, generated);

        let db = result.get("database").unwrap().as_mapping().unwrap();
        // User's URL should be preserved
        assert_eq!(
            db.get(&serde_yaml::Value::String("url".to_string())).unwrap().as_str().unwrap(),
            "postgresql://user:pass@localhost/db"
        );
        // User's max_connections preserved
        assert_eq!(
            db.get(&serde_yaml::Value::String("max_connections".to_string())).unwrap().as_i64().unwrap(),
            10
        );
        // New key from generated added
        assert!(db.get(&serde_yaml::Value::String("pool_size".to_string())).is_some());
    }

    #[test]
    fn test_deep_merge_yaml_recursive_merge() {
        let user = serde_yaml::from_str::<serde_yaml::Value>("server:\n  host: localhost\n  port: 3000").unwrap();
        let generated = serde_yaml::from_str::<serde_yaml::Value>("server:\n  port: 8080\n  ssl: true").unwrap();
        let result = deep_merge_yaml_preserve_user(user, generated);

        let server = result.get("server").unwrap().as_mapping().unwrap();
        assert_eq!(server.get(&serde_yaml::Value::String("host".to_string())).unwrap(), &serde_yaml::Value::String("localhost".to_string()));
        // User's port (3000) wins over generated (8080)
        assert_eq!(server.get(&serde_yaml::Value::String("port".to_string())).unwrap(), &serde_yaml::Value::String("3000".to_string()));
        assert!(server.get(&serde_yaml::Value::String("ssl".to_string())).is_some());
    }

    #[test]
    fn test_extract_seed_names() {
        let content = "# Header\n- seed1\n- seed2\n# Comment\n- seed3";
        let seeds = extract_seed_names(content);

        assert_eq!(seeds.len(), 3);
        assert!(seeds.contains("seed1"));
        assert!(seeds.contains("seed2"));
        assert!(seeds.contains("seed3"));
    }

    #[test]
    fn test_merge_seed_order_nonexistent_file() {
        let generated = "- first_seed\n- second_seed\n- third_seed\n- new_seed\n";

        let result = merge_seed_order(generated, Path::new("nonexistent.yml")).unwrap();
        // When file doesn't exist, returns generated content
        assert!(result.contains("new_seed"));
    }

    #[test]
    fn test_append_new_seeds() {
        let generated = "- seed1\n- seed2\n- seed3\n";
        let existing = "- seed1\n- seed2\n";

        let generated_seeds = extract_seed_names(generated);
        let existing_seeds = extract_seed_names(existing);
        let mut result = String::from("- seed1\n- seed2\n");

        append_new_seeds(&generated_seeds, &existing_seeds, &mut result);

        assert!(result.contains("seed3"));
        assert!(result.contains("Newly added seeds"));
    }

    #[test]
    fn test_append_new_seeds_empty_when_all_exist() {
        let generated = "- seed1\n- seed2\n";
        let existing = "- seed1\n- seed2\n";

        let generated_seeds = extract_seed_names(generated);
        let existing_seeds = extract_seed_names(existing);
        let mut result = String::from("- seed1\n- seed2\n");

        append_new_seeds(&generated_seeds, &existing_seeds, &mut result);

        // Should not add the "Newly added seeds" comment when no new seeds
        assert!(!result.contains("Newly added seeds"));
    }
}

/// Find all schema files in a directory
fn find_schema_files(path: &PathBuf) -> Result<Vec<PathBuf>> {
    use walkdir::WalkDir;

    let mut files = Vec::new();

    if path.is_file() {
        if is_schema_file(path) {
            files.push(path.clone());
        }
        return Ok(files);
    }

    if !path.exists() {
        return Ok(files);
    }

    for entry in WalkDir::new(path)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() && is_schema_file(path) {
            files.push(path.to_path_buf());
        }
    }

    files.sort();
    Ok(files)
}

/// Check if a path is a schema file (supports both .schema and .yaml formats)
fn is_schema_file(path: &std::path::Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    // Legacy format
    name.ends_with(".model.schema") || name.ends_with(".hook.schema") || name.ends_with(".workflow.schema") ||
    // New YAML format
    name.ends_with(".model.yaml") || name.ends_with(".hook.yaml") || name.ends_with(".workflow.yaml")
}

/// Find the schema path for a module
fn find_module_schema_path(module: &str) -> Result<PathBuf> {
    // Check common locations
    let candidates = [
        PathBuf::from(format!("libs/modules/{}/schema", module)),
        PathBuf::from(format!("libs/modules/{}", module)),
        PathBuf::from(format!("modules/{}/schema", module)),
        PathBuf::from(format!("modules/{}", module)),
        PathBuf::from(module), // Direct path
    ];

    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.clone());
        }
    }

    // If module looks like a path, use it directly
    let path = PathBuf::from(module);
    if path.exists() {
        return Ok(path);
    }

    // Return first candidate as default (will show "no files found" later)
    Ok(candidates[0].clone())
}

/// Watch schema files and regenerate on changes
fn execute_watch(module: &str, target: &str, output: Option<PathBuf>) -> Result<()> {
    use notify::RecursiveMode;
    use notify_debouncer_mini::new_debouncer;

    println!(
        "{} schema files for module: {}",
        "Watching".green().bold(),
        module.cyan()
    );
    println!("  Press {} to stop", "Ctrl+C".yellow());
    println!();

    // Find schema path
    let schema_path = find_module_schema_path(module)?;

    if !schema_path.exists() {
        anyhow::bail!("Schema path does not exist: {}", schema_path.display());
    }

    println!("  {} {}", "Watching:".blue(), schema_path.display());
    println!();

    // Initial generation
    println!("{}", "Running initial generation...".cyan());
    if let Err(e) = execute_generate(module, target, output.clone(), false, true, false, false, "HEAD", false, None, None, None, false) {
        println!("  {} {}", "Error:".red().bold(), e);
    }
    println!();

    // Set up file watcher
    let (tx, rx) = channel();

    let mut debouncer = new_debouncer(Duration::from_millis(500), tx)
        .context("Failed to create file watcher")?;

    debouncer
        .watcher()
        .watch(&schema_path, RecursiveMode::Recursive)
        .context("Failed to start watching")?;

    println!("{}", "Waiting for changes...".dimmed());

    // Watch loop
    loop {
        match rx.recv() {
            Ok(Ok(events)) => {
                // Check if any schema files changed
                let schema_changed = events.iter().any(|event| {
                    is_schema_file(&event.path)
                });

                if schema_changed {
                    println!();
                    println!(
                        "{} {}",
                        "Change detected:".yellow().bold(),
                        chrono::Local::now().format("%H:%M:%S")
                    );

                    // Show which files changed
                    for event in &events {
                        if is_schema_file(&event.path) {
                            let file_name = event.path.file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("unknown");
                            println!("  {} {}", "Modified:".blue(), file_name);
                        }
                    }

                    println!();

                    // Regenerate
                    match execute_generate(module, target, output.clone(), false, true, false, false, "HEAD", false, None, None, None, false) {
                        Ok(()) => {
                            println!();
                            println!("{}", "Waiting for changes...".dimmed());
                        }
                        Err(e) => {
                            println!("  {} {}", "Error:".red().bold(), e);
                            println!();
                            println!(
                                "{}",
                                "Fix the error and save again...".yellow()
                            );
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                println!("  {} {:?}", "Watch error:".red(), e);
            }
            Err(e) => {
                println!("  {} {}", "Channel error:".red(), e);
                break;
            }
        }
    }

    Ok(())
}

/// Generate database migration from schema changes
fn execute_migration(
    module: &str,
    output: Option<PathBuf>,
    destructive: bool,
    database_url: Option<String>,
    preview: bool,
    safe_only: bool,
) -> Result<()> {
    use crate::migration::{
        SafetyAnalysis, diff_schemas, generate_migration,
    };

    println!(
        "{} for module: {}",
        "Generating migration".green().bold(),
        module.cyan()
    );

    // Find schema path
    let schema_path = find_module_schema_path(module)?;
    let schema_files = find_schema_files(&schema_path)?;

    if schema_files.is_empty() {
        println!("{}", "No schema files found".yellow());
        return Ok(());
    }

    // Build module schema
    let (module_schema, parse_errors) = build_module_schema(module, &schema_files)?;

    if !parse_errors.is_empty() {
        for error in &parse_errors {
            println!("  {}", error.red());
        }
        anyhow::bail!("Parsing failed");
    }

    // Resolve schemas
    let resolved = resolve_schema(&module_schema)
        .map_err(|e| anyhow::anyhow!("Schema validation failed: {:?}", e))?;

    // Build schema snapshot from resolved models
    let new_schema = build_schema_snapshot(&resolved);

    // Get the "old" schema — either from live database or snapshot file
    let old_schema = get_old_schema(&schema_path, database_url.as_deref())?;

    // Diff schemas
    let diff = diff_schemas(&old_schema, &new_schema);

    if !diff.has_changes() {
        println!("  {} No schema changes detected", "✓".green());
        return Ok(());
    }

    // Safety analysis
    let safety = SafetyAnalysis::from_diff(&diff);

    // Show summary
    println!();
    println!("{}", "Schema changes detected:".yellow().bold());
    println!("{}", diff.summary());
    println!();
    println!("{}", "Safety analysis:".blue().bold());
    println!("{}", safety.summary());

    if diff.has_destructive_changes() {
        println!();
        println!(
            "{}",
            "WARNING: Destructive changes detected (data loss possible)!"
                .red()
                .bold()
        );
        if safe_only {
            println!(
                "{}",
                "  --safe-only: Destructive changes will be excluded from migration"
                    .yellow()
            );
        }
        if !destructive && !safe_only {
            println!(
                "{}",
                "  Use --destructive to uncomment DROP statements in migration output"
                    .yellow()
            );
        }
    }

    // Show rename candidates
    for change in diff.table_changes.values() {
        for rename in &change.rename_candidates {
            println!(
                "  {} Possible rename in {}: {} -> {} (type: {})",
                "?".cyan(),
                change.table_name,
                rename.old_name,
                rename.new_name,
                rename.data_type
            );
        }
    }

    // Generate migration SQL (separate UP and DOWN)
    let up_sql = crate::migration::generate_up_migration(&diff, &new_schema, destructive);
    let down_sql = crate::migration::generate_down_migration(&diff);

    // Preview mode: just print the SQL and exit
    if preview {
        println!();
        println!("{}", "UP Migration (preview):".green().bold());
        println!("{}", "─".repeat(60));
        println!("{}", up_sql);
        println!("{}", "─".repeat(60));
        if !down_sql.trim().is_empty() {
            println!();
            println!("{}", "DOWN Migration (preview):".yellow().bold());
            println!("{}", "─".repeat(60));
            println!("{}", down_sql);
            println!("{}", "─".repeat(60));
        }
        return Ok(());
    }

    // Output migration files
    if let Some(output_path) = output {
        // Single file mode: write combined migration
        let combined = generate_migration(&diff, &new_schema, destructive);
        fs::write(&output_path, &combined)?;
        println!();
        println!(
            "{} {}",
            "Migration written to:".green(),
            output_path.display()
        );
    } else {
        // Generate timestamped separate UP/DOWN files
        let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
        let migrations_dir = schema_path
            .parent()
            .unwrap_or(&schema_path)
            .join("migrations");

        fs::create_dir_all(&migrations_dir)?;

        let up_file =
            migrations_dir.join(format!("{}_{}_migration.up.sql", timestamp, module));
        let down_file =
            migrations_dir.join(format!("{}_{}_migration.down.sql", timestamp, module));

        // UP file with header
        let up_content = format!(
            "-- Migration generated by metaphor-schema\n-- WARNING: Review carefully before applying!\n\n{}",
            up_sql
        );
        fs::write(&up_file, &up_content)?;

        // DOWN file with header
        let down_content = format!(
            "-- Rollback migration generated by metaphor-schema\n\n{}",
            down_sql
        );
        fs::write(&down_file, &down_content)?;

        println!();
        println!(
            "{} {}",
            "UP migration written to:".green(),
            up_file.display()
        );
        println!(
            "{} {}",
            "DOWN migration written to:".green(),
            down_file.display()
        );
    }

    // Save new schema snapshot
    let snapshot_path = schema_path
        .parent()
        .unwrap_or(&schema_path)
        .join(".schema_snapshot.json");
    let snapshot_json = serde_json::to_string_pretty(&new_schema)?;
    fs::write(&snapshot_path, &snapshot_json)?;
    println!(
        "{} {}",
        "Schema snapshot saved to:".blue(),
        snapshot_path.display()
    );

    Ok(())
}

/// Get the "old" schema for diffing — from live database (if URL provided) or snapshot file.
fn get_old_schema(schema_path: &Path, database_url: Option<&str>) -> Result<crate::migration::SchemaSnapshot> {
    // If database URL is provided, introspect live database
    #[cfg(feature = "database")]
    if let Some(url) = database_url {
        println!(
            "  {} {}",
            "Introspecting database:".blue(),
            url.split('@').last().unwrap_or("***")
        );

        let introspector = crate::migration::DatabaseIntrospector::new(url);
        let rt = tokio::runtime::Runtime::new()
            .context("Failed to create tokio runtime")?;
        let schema = rt.block_on(introspector.introspect("public"))?;

        println!(
            "  {} Found {} tables, {} enums",
            "✓".green(),
            schema.tables.len(),
            schema.enums.len()
        );

        return Ok(schema);
    }

    #[cfg(not(feature = "database"))]
    if database_url.is_some() {
        anyhow::bail!(
            "Database introspection requires the 'database' feature. \
             Rebuild with: cargo build -p metaphor-schema --features database"
        );
    }

    // Fall back to snapshot file
    let snapshot_path = schema_path
        .parent()
        .unwrap_or(schema_path)
        .join(".schema_snapshot.json");

    if snapshot_path.exists() {
        let content = fs::read_to_string(&snapshot_path)?;
        Ok(serde_json::from_str(&content).unwrap_or_default())
    } else {
        Ok(crate::migration::SchemaSnapshot::default())
    }
}

/// Show schema drift between YAML definitions and database/snapshot (read-only).
fn execute_status(module: &str, database_url: Option<String>) -> Result<()> {
    use crate::migration::{SafetyAnalysis, diff_schemas};

    println!(
        "{} for module: {}",
        "Checking schema status".green().bold(),
        module.cyan()
    );

    // Find schema path
    let schema_path = find_module_schema_path(module)?;
    let schema_files = find_schema_files(&schema_path)?;

    if schema_files.is_empty() {
        println!("{}", "No schema files found".yellow());
        return Ok(());
    }

    // Build module schema
    let (module_schema, parse_errors) = build_module_schema(module, &schema_files)?;

    if !parse_errors.is_empty() {
        for error in &parse_errors {
            println!("  {}", error.red());
        }
        anyhow::bail!("Parsing failed");
    }

    // Resolve schemas
    let resolved = resolve_schema(&module_schema)
        .map_err(|e| anyhow::anyhow!("Schema validation failed: {:?}", e))?;

    // Build schema snapshot from resolved models
    let new_schema = build_schema_snapshot(&resolved);

    // Get old schema
    let old_schema = get_old_schema(&schema_path, database_url.as_deref())?;

    // Diff
    let diff = diff_schemas(&old_schema, &new_schema);

    if !diff.has_changes() {
        println!();
        println!("  {} Schema is up to date — no drift detected", "✓".green());
        return Ok(());
    }

    // Safety analysis
    let safety = SafetyAnalysis::from_diff(&diff);

    // Show summary
    println!();
    println!("{}", "Schema drift detected:".yellow().bold());
    println!("{}", diff.summary());
    println!();
    println!("{}", "Safety analysis:".blue().bold());
    println!("{}", safety.summary());

    // Show rename candidates
    for change in diff.table_changes.values() {
        for rename in &change.rename_candidates {
            println!(
                "  {} Possible rename in {}: {} -> {} (type: {})",
                "?".cyan(),
                change.table_name,
                rename.old_name,
                rename.new_name,
                rename.data_type
            );
        }
    }

    if diff.has_destructive_changes() {
        println!();
        println!(
            "{}",
            "WARNING: Destructive changes detected!".red().bold()
        );
    }

    println!();
    println!(
        "Run {} to generate migration files.",
        format!("metaphor-schema schema migration {}", module).cyan()
    );

    // Signal drift via error so callers (CLI, CI) can handle appropriately
    anyhow::bail!("Schema drift detected — {} change(s) pending", {
        let mut count = diff.tables_added.len() + diff.tables_removed.len();
        for change in diff.table_changes.values() {
            count += change.columns_added.len()
                + change.columns_removed.len()
                + change.columns_modified.len();
        }
        count += diff.enums_added.len() + diff.enums_removed.len();
        count
    });
}

/// Build a [`SchemaSnapshot`] from a resolved schema.
///
/// Converts all models and enums into snapshot format for diffing against
/// a live database or a previous snapshot file.
fn build_schema_snapshot(resolved: &crate::resolver::ResolvedSchema) -> crate::migration::SchemaSnapshot {
    use crate::migration::{ColumnSnapshot, EnumSnapshot, IndexSnapshot, SchemaSnapshot, TableSnapshot};

    let mut snapshot = SchemaSnapshot::default();

    for model in &resolved.schema.models {
        let table_name = model.collection_name();
        let mut columns = indexmap::IndexMap::new();
        let mut primary_key = None;

        for field in &model.fields {
            let sql_type = type_to_sql(&field.type_ref);
            let nullable = field.type_ref.is_optional();

            if field.is_primary_key() {
                primary_key = Some(field.name.clone());
            }

            columns.insert(
                field.name.clone(),
                ColumnSnapshot {
                    name: field.name.clone(),
                    data_type: sql_type,
                    nullable,
                    default: None,
                    is_unique: field.is_unique(),
                },
            );
        }

        let mut indexes = indexmap::IndexMap::new();
        for index in &model.indexes {
            let idx_name = format!("idx_{}_{}", table_name, index.fields.join("_"));
            indexes.insert(
                idx_name.clone(),
                IndexSnapshot {
                    name: idx_name,
                    columns: index.fields.clone(),
                    unique: matches!(index.index_type, IndexType::Unique),
                    index_type: match index.index_type {
                        IndexType::Index => "btree".to_string(),
                        IndexType::Unique => "unique".to_string(),
                        IndexType::Fulltext => "gin".to_string(),
                        IndexType::Gin => "gin".to_string(),
                    },
                },
            );
        }

        snapshot.tables.insert(
            table_name.clone(),
            TableSnapshot {
                name: table_name,
                columns,
                indexes,
                primary_key,
            },
        );
    }

    for enum_def in &resolved.schema.enums {
        let enum_name = enum_def.name.to_lowercase();
        let variants: Vec<String> = enum_def.variants.iter().map(|v| v.name.clone()).collect();
        snapshot.enums.insert(
            enum_name.clone(),
            EnumSnapshot {
                name: enum_name,
                variants,
            },
        );
    }

    snapshot
}

/// Convert a TypeRef to SQL type
fn type_to_sql(type_ref: &TypeRef) -> String {
    match type_ref {
        TypeRef::Primitive(p) => match p {
            PrimitiveType::String => "VARCHAR(255)".to_string(),
            PrimitiveType::Int => "INTEGER".to_string(),
            PrimitiveType::Int32 => "INTEGER".to_string(),
            PrimitiveType::Int64 => "BIGINT".to_string(),
            PrimitiveType::Float => "REAL".to_string(),
            PrimitiveType::Float32 => "REAL".to_string(),
            PrimitiveType::Float64 => "DOUBLE PRECISION".to_string(),
            PrimitiveType::Bool => "BOOLEAN".to_string(),
            PrimitiveType::Uuid => "UUID".to_string(),
            PrimitiveType::Email => "VARCHAR(255)".to_string(),
            PrimitiveType::Url => "TEXT".to_string(),
            PrimitiveType::Phone => "VARCHAR(50)".to_string(),
            PrimitiveType::Slug => "VARCHAR(255)".to_string(),
            PrimitiveType::Ip => "INET".to_string(),
            PrimitiveType::IpV4 => "INET".to_string(),
            PrimitiveType::IpV6 => "INET".to_string(),
            PrimitiveType::Mac => "MACADDR".to_string(),
            PrimitiveType::DateTime => "TIMESTAMPTZ".to_string(),
            PrimitiveType::Date => "DATE".to_string(),
            PrimitiveType::Time => "TIME".to_string(),
            PrimitiveType::Duration => "INTERVAL".to_string(),
            PrimitiveType::Timestamp => "TIMESTAMPTZ".to_string(),
            PrimitiveType::Json => "JSONB".to_string(),
            PrimitiveType::Markdown => "TEXT".to_string(),
            PrimitiveType::Html => "TEXT".to_string(),
            PrimitiveType::Bytes => "BYTEA".to_string(),
            PrimitiveType::Binary => "BYTEA".to_string(),
            PrimitiveType::Base64 => "TEXT".to_string(),
            PrimitiveType::Money => "DECIMAL(19, 4)".to_string(),
            PrimitiveType::Decimal => "DECIMAL".to_string(),
            PrimitiveType::Percentage => "DECIMAL(5, 2)".to_string(),
        },
        TypeRef::Custom(name) => name.to_uppercase(),
        TypeRef::Array(inner) => format!("{}[]", type_to_sql(inner)),
        TypeRef::Optional(inner) => type_to_sql(inner),
        TypeRef::Map { .. } => "JSONB".to_string(),
        TypeRef::ModuleRef { name, .. } => name.to_uppercase(),
    }
}

/// Show changed schema files using git
fn execute_changed(module: Option<&str>, base: &str, show_outputs: bool, show_targets: bool) -> Result<()> {
    println!(
        "{} (comparing against {})",
        "Detecting schema changes".green().bold(),
        base.yellow()
    );

    let repo_root = GitChangeDetector::find_repo_root()
        .context("Failed to find git repository root")?;

    let detector = GitChangeDetector::new(repo_root).with_base_ref(base);

    let changes = if let Some(mod_name) = module {
        println!("  Module: {}", mod_name.cyan());
        detector.get_changed_schemas(mod_name)?
    } else {
        println!("  Scanning all modules...");
        detector.get_all_changed_schemas()?
    };

    println!();

    if changes.is_empty() {
        println!("  {} No schema changes detected", "✓".green());
        return Ok(());
    }

    // Show summary
    let summary = ChangeSummary::from_changes(&changes);
    println!("{}", summary.display());
    println!();

    // Show individual changes
    println!("{}", "Changed files:".blue().bold());
    for change in &changes {
        let change_indicator = match change.change_type {
            ChangeType::Added => "+".green(),
            ChangeType::Modified => "M".yellow(),
            ChangeType::Deleted => "-".red(),
            ChangeType::Renamed => "R".cyan(),
            ChangeType::Untracked => "?".dimmed(),
        };
        println!("  {} {}", change_indicator, change.path.display());
    }
    println!();

    // Show affected outputs if requested
    if show_outputs {
        let outputs = detector.get_all_affected_outputs(&changes);
        println!("{}", "Affected output files:".blue().bold());
        for output in &outputs {
            println!("  {} {} ({})", "→".cyan(), output.path.display(), output.target.yellow());
        }
        println!();
    }

    // Show affected targets if requested
    if show_targets {
        let targets = detector.get_affected_targets(&changes);
        println!("{}", "Generation targets needed:".blue().bold());
        println!("  {}", targets.join(", ").yellow());
        println!();
        println!(
            "  {} backbone schema generate {} --target {}",
            "Run:".green(),
            module.unwrap_or("<module>"),
            targets.join(",")
        );
    }

    Ok(())
}

#[cfg(test)]
mod merge_custom_tests {
    use super::*;
    use std::io::Write;

    fn write_temp(content: &str) -> (tempfile::NamedTempFile, std::path::PathBuf) {
        let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        f.write_all(content.as_bytes()).unwrap();
        let path = f.path().to_path_buf();
        (f, path)
    }

    #[test]
    fn test_inline_custom_marker_preserved() {
        let existing = "mod foo;\nmod foo_custom; // <<< CUSTOM - Extension\nmod bar;\n";
        let generated = "mod foo;\nmod bar;\n";
        let (_tmp, path) = write_temp(existing);

        let result = merge_rust_mod_custom(generated, &path).unwrap();
        assert!(result.contains("foo_custom"), "custom mod declaration should be preserved");
        assert!(result.contains("// <<< CUSTOM"), "custom marker should be preserved");
    }

    #[test]
    fn test_paired_custom_block_preserved() {
        let existing = "let x = 1;\n// <<< CUSTOM - Custom init\nlet y = 2;\nlet z = 3;\n// END CUSTOM\nlet w = 4;\n";
        let generated = "let x = 1;\nlet w = 4;\n";
        let (_tmp, path) = write_temp(existing);

        let result = merge_rust_mod_custom(generated, &path).unwrap();
        assert!(result.contains("let y = 2;"), "custom line y should be preserved");
        assert!(result.contains("let z = 3;"), "custom line z should be preserved");
        assert!(result.contains("// END CUSTOM"), "END CUSTOM marker should be present");
    }

    #[test]
    fn test_no_duplicate_custom_blocks() {
        let existing = "mod foo;\n// <<< CUSTOM - Extension\nmod bar;\n";
        let generated = "mod foo;\nmod bar;\n";
        let (_tmp, path) = write_temp(existing);

        let result = merge_rust_mod_custom(generated, &path).unwrap();
        let count = result.lines().filter(|l| l.trim() == "mod bar;").count();
        assert_eq!(count, 1, "mod bar; should appear only once after dedup");
    }

    #[test]
    fn test_fuzzy_anchor_matching_with_clone() {
        let existing = "\
let repo = Arc::new(Repo::new(pool.clone()));
let service = Arc::new(Service::new(repo.clone()));
// <<< CUSTOM
let custom = CustomService::new(repo);
// END CUSTOM
";
        let generated = "\
let repo = Arc::new(Repo::new(pool.clone()));
let service = Arc::new(Service::new(repo));
";
        let (_tmp, path) = write_temp(existing);

        let result = merge_rust_mod_custom(generated, &path).unwrap();
        assert!(result.contains("CustomService"), "custom block should be placed via fuzzy anchor");
        let svc_pos = result.find("let service").unwrap();
        let custom_pos = result.find("CustomService").unwrap();
        assert!(svc_pos < custom_pos, "custom block should come after anchor line");
    }

    #[test]
    fn test_missing_end_custom_truncation() {
        // END CUSTOM exists but is beyond 200 lines, triggering truncation
        let mut existing = String::from("let x = 1;\n// <<< CUSTOM\n");
        for i in 0..250 {
            existing.push_str(&format!("let line_{} = {};\n", i, i));
        }
        existing.push_str("// END CUSTOM\n");
        let generated = "let x = 1;\n";
        let (_tmp, path) = write_temp(&existing);

        let result = merge_rust_mod_custom(generated, &path).unwrap();
        let custom_lines: Vec<&str> = result.lines()
            .filter(|l| l.starts_with("let line_"))
            .collect();
        assert!(custom_lines.len() <= 200, "should truncate at 200 lines, got {}", custom_lines.len());
    }

    #[test]
    fn test_no_custom_markers_returns_generated() {
        let existing = "mod foo;\nmod bar;\n";
        let generated = "mod foo;\nmod baz;\n";
        let (_tmp, path) = write_temp(existing);

        let result = merge_rust_mod_custom(generated, &path).unwrap();
        assert_eq!(result, generated, "should return generated content unchanged");
    }

    #[test]
    fn test_empty_existing_file() {
        let existing = "";
        let generated = "mod foo;\nmod bar;\n";
        let (_tmp, path) = write_temp(existing);

        let result = merge_rust_mod_custom(generated, &path).unwrap();
        assert_eq!(result, generated, "should return generated content for empty existing file");
    }
}
