//! CLI command definitions and handlers

pub mod kotlin;
pub mod schema;
pub mod webapp;
pub mod workspace;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// metaphor-plugin-schema - Schema-driven code generator (Rust + Kotlin + Webapp)
#[derive(Parser, Debug)]
#[command(name = "metaphor-schema")]
#[command(author)]
#[command(version)]
#[command(about = "Schema-driven code generator (Rust server-side, Kotlin Multiplatform mobile, TypeScript+React webapp)", long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Schema operations: parse, validate, generate (server-side: Rust, SQL, etc.)
    Schema {
        #[command(subcommand)]
        action: schema::SchemaAction,
    },

    /// Kotlin Multiplatform Mobile code generation
    Kotlin {
        #[command(subcommand)]
        action: kotlin::KotlinAction,
    },

    /// Generate server-side Rust code (alias for `schema generate`)
    ///
    /// Example: metaphor-schema generate:rust bersihir --target all
    ///
    /// Also accessible as plain `generate` (defaults to Rust target).
    ///
    /// When invoked from inside a workspace project directory (a subdir of any
    /// project listed in `metaphor.yaml`), MODULE is optional and auto-detects
    /// to the current project. Pass it explicitly to target a different module.
    #[command(name = "generate:rust", alias = "generate")]
    GenerateRust {
        /// Module name to generate code for (auto-detected from CWD if omitted)
        module: Option<String>,

        /// Generation targets (comma-separated)
        #[arg(short, long, default_value = "all")]
        target: String,

        /// Output directory
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Dry run - show what would be generated without writing files
        #[arg(long)]
        dry_run: bool,

        /// Force overwrite existing files
        #[arg(short, long)]
        force: bool,

        /// Only generate for changed schemas (uses git to detect changes)
        #[arg(long)]
        changed: bool,

        /// Base git reference for change detection
        #[arg(long, default_value = "HEAD")]
        base: String,

        /// Filter: only generate for specific models (comma-separated)
        #[arg(long)]
        models: Option<String>,

        /// Filter: only generate for specific hooks (comma-separated)
        #[arg(long)]
        hooks: Option<String>,

        /// Filter: only generate for specific workflows (comma-separated)
        #[arg(long)]
        workflows: Option<String>,

        /// Skip strict validation
        #[arg(long)]
        lenient: bool,
    },

    /// Generate Kotlin Multiplatform Mobile code
    ///
    /// Example: metaphor-schema generate:kotlin bersihir-service --output bersihir-mobile-laundry
    ///
    /// Inside a Metaphor workspace (a directory containing metaphor.yaml), the
    /// MODULE arg can be either a project name (e.g. `bersihir-service`) or a
    /// schema `module:` value (e.g. `bersihir`). The generator also walks the
    /// project's transitive schema-module dependencies and emits Kotlin code
    /// for each in one shot — pass `--no-deps` to opt out.
    ///
    /// Output destination: pass a workspace project name via `--output`
    /// (resolves to `<project>/shared/src/commonMain/kotlin`) OR a raw
    /// filesystem path via `--output-path`. The two are mutually exclusive.
    #[command(name = "generate:kotlin")]
    GenerateKotlin {
        /// Module identifier — project name or schema `module:` value
        /// (auto-detected from CWD if omitted)
        module: Option<String>,

        /// Module base path (legacy fallback; ignored when in a workspace)
        #[arg(long, default_value = "libs/modules")]
        module_path: PathBuf,

        /// Workspace project name (resolves to its mobile source root)
        #[arg(short, long, conflicts_with = "output_path")]
        output: Option<String>,

        /// Raw filesystem path to write generated code to
        #[arg(long, conflicts_with = "output")]
        output_path: Option<PathBuf>,

        /// Kotlin package name (auto-detects from project if not provided)
        #[arg(short, long)]
        package: Option<String>,

        /// Generation targets (comma-separated). Pass `all` for everything.
        #[arg(short, long, default_value = "all", value_delimiter = ',')]
        target: Vec<crate::kotlin::config::GenerationTarget>,

        /// Skip files that already exist on disk
        #[arg(long)]
        skip_existing: bool,

        /// Do not also generate transitive schema-module dependencies
        #[arg(long)]
        no_deps: bool,

        /// Verbose output
        #[arg(short, long)]
        verbose: bool,
    },

    /// Generate webapp code (TypeScript + React) via metaphor-webgen.
    ///
    /// Workspace "app" mode (recommended) — resolves the app + its modules from
    /// metaphor.yaml and fans out, no per-app script:
    ///   metaphor schema generate:webapp --output bersihir-webapp-admin
    ///   (run from a module dir, e.g. apps/bersihir-service; or pass the module:
    ///    metaphor schema generate:webapp bersihir --output bersihir-webapp-admin)
    ///
    /// Single-module mode — one module into a raw path:
    ///   metaphor schema generate:webapp bersihir --schema-dir ./schema --output ./out
    #[command(name = "generate:webapp")]
    GenerateWebapp {
        /// Module name. Optional in workspace "app" mode (auto-detected from the
        /// current project dir, then fanned out across its module deps).
        module: Option<String>,

        /// Generation targets (comma-separated). Default = the framework-free
        /// Clean Architecture stack. Legacy MUI/hooks targets are opt-in:
        /// contracts, application, infrastructure | domain | hooks, schemas,
        /// forms, pages, types | all.
        #[arg(short, long, default_value = "contracts,application,infrastructure")]
        target: String,

        /// Entity filter (only generate for specific entity)
        #[arg(long)]
        entity: Option<String>,

        /// Output: a workspace APP NAME (→ `<app>/src/generated`, multi-module
        /// fan-out) or a raw directory path (single module). Default app layout
        /// is `apps/<name>/src/generated`.
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Explicit schema directory (containing models/, hooks/). Overrides the
        /// default `libs/modules/<module>/schema`. Use when the schema lives
        /// outside the modules dir, e.g. apps/bersihir-service/schema.
        #[arg(long)]
        schema_dir: Option<PathBuf>,

        /// Import root alias generated app/infrastructure code uses to reference
        /// the generated tree (default: @/generated).
        #[arg(long)]
        import_alias: Option<String>,

        /// Also generate gRPC clients (nice-grpc-web). Off by default; the REST
        /// API client is always generated.
        #[arg(long)]
        with_grpc: bool,

        /// Dry run - show what would be generated without writing files
        #[arg(long)]
        dry_run: bool,

        /// Force overwrite existing files
        #[arg(short, long)]
        force: bool,
    },
}
