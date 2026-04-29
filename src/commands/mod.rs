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

    /// Generate webapp code (TypeScript + React) via metaphor-webgen
    ///
    /// Example: metaphor-schema generate:webapp bersihir --target all
    #[command(name = "generate:webapp")]
    GenerateWebapp {
        /// Module name to generate code for
        module: String,

        /// Generation targets (comma-separated): all, hooks, schemas, forms, pages, types
        #[arg(short, long, default_value = "all")]
        target: String,

        /// Entity filter (only generate for specific entity)
        #[arg(long)]
        entity: Option<String>,

        /// Output directory (default: apps/webapp/src/)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Dry run - show what would be generated without writing files
        #[arg(long)]
        dry_run: bool,

        /// Force overwrite existing files
        #[arg(short, long)]
        force: bool,
    },
}
