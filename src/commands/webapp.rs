//! Webapp code generation — uses the embedded `webgen` module.
//!
//! `metaphor schema generate:webapp` generates TypeScript + React code
//! from schema definitions, using the webgen engine merged into this crate.

use anyhow::Result;
use colored::Colorize;
use std::path::PathBuf;

use crate::webgen::{Config, Generator};

/// Run webapp generation using the embedded webgen engine.
pub fn run(
    module: &str,
    target: &str,
    entity: Option<&str>,
    output: Option<&PathBuf>,
    dry_run: bool,
    force: bool,
) -> Result<()> {
    println!(
        "{}",
        format!("🌐 Generating webapp code for module: {}", module)
            .bright_cyan()
            .bold()
    );
    println!();

    // Build config
    let mut config = Config::new(module);

    if target != "all" {
        config = config.with_targets_str(target);
    }

    if let Some(entity_name) = entity {
        config = config.with_entity(Some(entity_name.to_string()));
    }

    if let Some(output_dir) = output {
        config = config.with_output_dir(output_dir.clone());
    }

    config = config.with_dry_run(dry_run).with_force(force);

    // Run generation
    let generator = Generator::new(config)?;

    if dry_run {
        println!("{}", "DRY RUN - No files will be written".yellow());
        println!();
    }

    let result = generator.generate()?;

    println!();
    println!("{}", "✅ Webapp code generation complete!".green().bold());
    println!();
    println!("{}", result.summary().bright_white());

    if !result.dry_run_files.is_empty() {
        println!();
        println!("{}", "Files that would be generated:".dimmed());
        for path in &result.dry_run_files {
            let path_str = path.strip_prefix("apps/webapp/src/")
                .unwrap_or(path)
                .display();
            println!("  • {}", path_str);
        }
    }

    if !result.files_generated.is_empty() {
        println!();
        println!("{}", "Generated files:".dimmed());
        for path in &result.files_generated {
            let path_str = path.strip_prefix("apps/webapp/src/")
                .unwrap_or(path)
                .display();
            println!("  • {}", path_str);
        }
    }

    Ok(())
}
