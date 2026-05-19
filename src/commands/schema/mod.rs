//! Schema command implementations

mod changed;
mod diff;
mod discovery;
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
use migration_cmd::{execute_migration, execute_status};
use parse::execute_parse;
use validate::execute_validate;
use watch::execute_watch;

use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use crate::generators::{generate_all_with_options, parse_targets, GenerationTarget, GenerationOptions};
use crate::git::{GitChangeDetector, ChangeSummary};
use crate::resolver::resolve_schema;
use discovery::{find_module_schema_path, find_schema_files};
use manifest::load_user_owned_globs;
use module_loader::build_module_schema;
use merge::{
    detect_unprotected_custom_code, merge_rust_mod_custom, merge_seed_file, merge_seed_order,
    merge_yaml_config,
};
use migrations::{
    is_generator_authored_migration, is_unstable_timestamped_migration,
    migration_identity_already_exists,
};

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



pub(super) fn execute_generate(
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

    // Load user_owned manifest once for both the migration cleanup pass and
    // the write loop below. Missing manifest → empty GlobSet (no-op gate),
    // preserving today's behavior for repos that haven't adopted the manifest.
    let user_owned = load_user_owned_globs(&output_dir)?;

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

                // With --force, sweep stale migrations the generator authored —
                // but ONLY files that carry the `-- Generated by metaphor-schema`
                // header. Hand-written migrations (audit triggers, backfills,
                // ad-hoc data fixes) are missing that marker and therefore
                // survive cleanup. Files matched by `user_owned` in
                // metaphor.codegen.yaml are also preserved unconditionally.
                if let Ok(entries) = fs::read_dir(&migrations_dir) {
                    let mut removed = 0;
                    let mut preserved_user = 0;
                    let mut preserved_handwritten = 0;
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

                        let rel_path = PathBuf::from("migrations").join(&name);
                        if user_owned.is_match(&rel_path) {
                            preserved_user += 1;
                            continue;
                        }

                        // Only delete files we (the generator) authored.
                        // Hand-written migrations don't carry the marker.
                        if !is_generator_authored_migration(&entry.path()) {
                            preserved_handwritten += 1;
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
                    if preserved_handwritten > 0 {
                        println!(
                            "  {} Preserved {} hand-written migration file(s) (no generator marker)",
                            "•".cyan(),
                            preserved_handwritten
                        );
                    }
                    if preserved_user > 0 {
                        println!(
                            "  {} Preserved {} user-owned migration file(s)",
                            "•".cyan(),
                            preserved_user
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
    let mut user_owned_skipped = 0;

    // `user_owned` was loaded above (shared with the migration cleanup pass).
    // Any generated file whose relative path matches a user_owned glob is
    // skipped wholesale — neither read, merged, nor written. This is the
    // contract that lets application code own files inside the generator's
    // output tree without losing them on regen.

    // Write or display generated files
    for (path, content) in &generated.files {
        let full_path = output_dir.join(path);
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        pb.set_message(file_name.to_string());

        // User-owned gate: never touch files the manifest reserves for hand-wired code.
        // Match against the relative path (as declared in the manifest), not the absolute one.
        if user_owned.is_match(path) {
            if dry_run {
                pb.println(format!(
                    "  {} {} (user-owned, would skip)",
                    "•".cyan(),
                    full_path.display()
                ));
            } else {
                pb.println(format!(
                    "  {} {} (user-owned, preserved)",
                    "•".cyan(),
                    full_path.display()
                ));
            }
            user_owned_skipped += 1;
            pb.inc(1);
            continue;
        }

        if dry_run {
            pb.println(format!(
                "  {} {} ({} bytes)",
                "Would create:".blue(),
                full_path.display(),
                content.len()
            ));
        } else {
            // Same-identity check for unstable-timestamp filenames (migrations).
            // Migration files are emitted with a fresh timestamp on every regen,
            // so the exact-path `exists()` check below would always miss them
            // and write a duplicate. Treat any sibling file with the same
            // `_<identity>.up|down.sql` suffix as "already present".
            if is_unstable_timestamped_migration(&full_path)
                && migration_identity_already_exists(&full_path)
                && !force
            {
                pb.println(format!(
                    "  {} {} (identity already migrated under a different timestamp)",
                    "Skipping:".yellow(),
                    full_path.display()
                ));
                skipped += 1;
                pb.inc(1);
                continue;
            }

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
        let user_owned_part = if user_owned_skipped > 0 {
            format!(", {} user-owned preserved", user_owned_skipped.to_string().cyan())
        } else {
            String::new()
        };
        println!(
            "{} {} created, {} skipped{}{}",
            "Complete:".green().bold(),
            created.to_string().green(),
            skipped.to_string().yellow(),
            user_owned_part,
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


