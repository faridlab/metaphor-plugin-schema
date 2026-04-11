//! Kotlin code generation CLI subcommand.
//!
//! Wraps the merged-in `crate::kotlin` module (originally `metaphor-plugin-mobilegen`).
//! The actual generation logic lives in [`crate::kotlin`]; this file is only the
//! CLI parsing layer and a thin call into the generator.

use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::time::Duration;

use crate::kotlin::{
    config::{GenerationTarget, GeneratorConfig},
    generators::MobileGenerator,
    package_detector::PackageSource,
    parse_module_schema,
};

#[derive(Subcommand, Debug)]
pub enum KotlinAction {
    /// Generate Kotlin Multiplatform Mobile code from a module's schema.
    Generate {
        /// Module name to generate code for (directory name under modules dir).
        module: String,

        /// Module base path (where libs/modules/ is located).
        #[arg(long, default_value = "libs/modules")]
        module_path: PathBuf,

        /// Output directory for generated code.
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Kotlin package name (auto-detects from project if not provided).
        #[arg(short, long)]
        package: Option<String>,

        /// Generation targets (comma-separated). Pass `all` for everything.
        #[arg(short, long, default_value = "all", value_delimiter = ',')]
        target: Vec<GenerationTarget>,

        /// Skip files that already exist on disk.
        #[arg(long)]
        skip_existing: bool,

        /// Verbose output.
        #[arg(short, long)]
        verbose: bool,
    },
}

pub fn run(action: KotlinAction) -> Result<()> {
    match action {
        KotlinAction::Generate {
            module,
            module_path,
            output,
            package,
            target,
            skip_existing,
            verbose,
        } => run_generate(
            module,
            module_path,
            output,
            package,
            target,
            skip_existing,
            verbose,
        ),
    }
}

fn run_generate(
    module: String,
    module_path: PathBuf,
    output: Option<PathBuf>,
    package: Option<String>,
    target: Vec<GenerationTarget>,
    skip_existing: bool,
    verbose: bool,
) -> Result<()> {
    let cwd = std::env::current_dir().context("getting current dir")?;

    // Build the GeneratorConfig once. This is the same struct mobilegen's
    // standalone CLI used; we construct it directly here instead of going
    // through clap's top-level Parser (which would conflict with schema's Cli).
    let config = GeneratorConfig {
        module: module.clone(),
        app: "mobileapp".to_string(),
        output: output.clone(),
        package: package.clone(),
        target: target.clone(),
        module_path: module_path.clone(),
        skip_existing,
        verbose,
    };

    if verbose {
        eprintln!("{}: {:?}", "Config".cyan(), config);
    }

    println!(
        "{}",
        format!(
            "📱 Backbone Mobilegen v{}\n",
            env!("CARGO_PKG_VERSION")
        )
        .bright_blue()
    );

    // Resolve schema directory
    let module_base = if config.module_path.is_absolute() {
        config.module_path.clone()
    } else {
        cwd.join(&config.module_path)
    };
    let schema_dir = module_base.join(&config.module).join("schema");

    println!(
        "{} {}",
        "Reading schema from".dimmed(),
        schema_dir.display().to_string().cyan()
    );

    let schema = parse_module_schema(&schema_dir, &config.module)
        .map_err(|e| anyhow::anyhow!("parsing module schema: {e}"))?;

    println!(
        "{} {} models, {} enums",
        "Found".green(),
        schema.models.len(),
        schema.enums.len()
    );

    // Detect or use provided package
    let package_info = config.package_info();
    let source_display = match &package_info.source {
        PackageSource::GradleNamespace(path) => {
            format!("Gradle namespace ({})", path.display())
        }
        PackageSource::SqlDelightPackage(path) => {
            format!("SQLDelight package ({})", path.display())
        }
        PackageSource::ExistingKotlinFiles => "existing Kotlin files".to_string(),
        PackageSource::Default => "default".to_string(),
    };

    println!(
        "{} {} ({})",
        "Using package".dimmed(),
        package_info.base_package.cyan(),
        source_display.dimmed()
    );

    let mut generator = MobileGenerator::new(&package_info.base_package)
        .map_err(|e| anyhow::anyhow!("creating generator: {e}"))?;
    generator.skip_existing = config.skip_existing;

    let output_dir_raw = config.output_path();
    let output_dir = if output_dir_raw.is_absolute() {
        output_dir_raw
    } else {
        cwd.join(output_dir_raw)
    };

    println!(
        "{} {}",
        "Generating code to".dimmed(),
        output_dir.display().to_string().cyan()
    );

    // Indeterminate progress: MobileGenerator does its work in one batch, so
    // we drive a spinner rather than a fixed-length bar. This matches what the
    // original mobilegen CLI did before MobileGenerator started returning the
    // pre-batched count we'd need for a determinate bar.
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} [{elapsed_precise}] {msg}")
            .unwrap(),
    );
    pb.enable_steady_tick(Duration::from_millis(100));
    pb.set_message("generating Kotlin code...");

    let targets: Vec<GenerationTarget> = config.targets();
    let result = generator
        .generate(&schema, &targets, &output_dir)
        .map_err(|e| {
            pb.finish_and_clear();
            anyhow::anyhow!("generation failed: {e}")
        })?;

    pb.finish_with_message("Done!");

    // Summary
    println!("\n{}", "=== Generation Summary ===".bright_cyan());
    println!(
        "{} {} files generated",
        "✓".green(),
        result.total_generated()
    );

    if !result.skipped_files.is_empty() {
        let reason = if config.skip_existing {
            "skipped (--skip-existing or // <<< CUSTOM marker)"
        } else {
            "skipped (contain // <<< CUSTOM marker)"
        };
        println!(
            "{} {} files {}",
            "⚠".yellow(),
            result.skipped_files.len(),
            reason
        );
        if config.verbose {
            for path in &result.skipped_files {
                println!("  → {}", path.display());
            }
        }
    }

    if !result.stale_deleted_files.is_empty() {
        println!(
            "{} {} stale files removed (no longer generated by current schema)",
            "🗑".yellow(),
            result.stale_deleted_files.len()
        );
        if config.verbose {
            for path in &result.stale_deleted_files {
                println!("  ✗ {}", path.display());
            }
        }
    }

    if result.entities_count > 0 {
        println!("  • {} entities", result.entities_count);
    }
    if result.enums_count > 0 {
        println!("  • {} enums", result.enums_count);
    }
    if result.repositories_count > 0 {
        println!("  • {} repositories", result.repositories_count);
    }
    if result.usecases_count > 0 {
        println!("  • {} use cases", result.usecases_count);
    }
    if result.services_count > 0 {
        println!("  • {} services", result.services_count);
    }
    if result.mappers_count > 0 {
        println!("  • {} mappers", result.mappers_count);
    }
    if result.validators_count > 0 {
        println!("  • {} validators", result.validators_count);
    }
    if result.api_clients_count > 0 {
        println!("  • {} API clients", result.api_clients_count);
    }
    if result.viewmodels_count > 0 {
        println!("  • {} ViewModels", result.viewmodels_count);
    }
    if result.components_count > 0 {
        println!("  • {} components", result.components_count);
    }

    println!(
        "\n{} {}",
        "Output:".dimmed(),
        output_dir.display().to_string().cyan()
    );
    println!(
        "{} {}",
        "Package:".dimmed(),
        package_info.base_package.cyan()
    );

    Ok(())
}
