//! `metaphor schema generate` — the schema command's main orchestrator.
//!
//! The body lives in phase-specific submodules. This file is purely the
//! pipeline:
//!
//! 1. [`change_detect`] — `--changed` short-circuit when nothing has moved.
//! 2. [`announce`] — print the run banner.
//! 3. [`load`] — discover, parse, filter, resolve the schema.
//! 4. `generate_all_with_options` — produce in-memory file contents.
//! 5. [`migration_cleanup`] — under `--force`, sweep stale migrations.
//! 6. [`write`] — write each file to disk, gated by `user_owned` and the
//!    merge strategies.
//! 7. summary print.
//! 8. [`post_check`] — optional `cargo check` on the result.

mod announce;
mod change_detect;
mod load;
mod migration_cleanup;
mod post_check;
mod write;

use anyhow::Result;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::time::Duration;

use crate::generators::{generate_all_with_options, parse_targets, GenerationOptions};

use super::manifest::load_user_owned_globs;

use announce::announce_run;
use change_detect::should_generate;
use load::{load_and_resolve, LoadedSchema};
use migration_cleanup::cleanup_stale_migrations;
use post_check::run_cargo_check;
use write::{write_generated_files, WriteStats};

#[allow(clippy::too_many_arguments)]
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
    if changed && !should_generate(module, base)? {
        return Ok(());
    }

    let targets = parse_targets(target);
    announce_run(module, &targets, dry_run, force, changed);

    let loaded = match load_and_resolve(
        module,
        models_filter,
        hooks_filter,
        workflows_filter,
        lenient,
    )? {
        Some(s) => s,
        None => return Ok(()),
    };
    let LoadedSchema {
        schema_path,
        resolved,
    } = loaded;

    let generated = run_generators(&resolved, &targets, split)?;

    let output_dir = output.unwrap_or_else(|| {
        schema_path
            .parent()
            .unwrap_or(&schema_path)
            .to_path_buf()
    });

    let user_owned = load_user_owned_globs(&output_dir)?;

    if force {
        cleanup_stale_migrations(&output_dir, &generated, &user_owned);
    }

    let stats = write_generated_files(&generated, &output_dir, &user_owned, force, dry_run)?;

    print_summary(&stats, generated.files.len(), dry_run);

    if validate && !dry_run && stats.created > 0 {
        run_cargo_check(module)?;
    }

    Ok(())
}

/// Phase 3: run every selected generator and collect the produced files
/// into a single in-memory map. A spinner advertises the work but the
/// generators themselves are synchronous.
fn run_generators(
    resolved: &crate::resolver::ResolvedSchema,
    targets: &[crate::generators::GenerationTarget],
    split: bool,
) -> Result<crate::generators::GeneratedOutput> {
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
        group_by_domain: true,
    };
    let generated = generate_all_with_options(resolved, targets, &options)?;

    spinner.finish_and_clear();
    Ok(generated)
}

/// Phase 7: print the post-run summary line using the write-loop counters.
fn print_summary(stats: &WriteStats, total_files: usize, dry_run: bool) {
    println!();
    if dry_run {
        println!(
            "{} {} file(s) would be created",
            "Dry run:".blue().bold(),
            total_files
        );
        return;
    }

    let user_owned_part = if stats.user_owned_skipped > 0 {
        format!(
            ", {} user-owned preserved",
            stats.user_owned_skipped.to_string().cyan()
        )
    } else {
        String::new()
    };
    let warnings_part = if stats.custom_warnings > 0 {
        format!(
            ", {} custom code warning(s)",
            stats.custom_warnings.to_string().yellow()
        )
    } else {
        String::new()
    };

    println!(
        "{} {} created, {} skipped{}{}",
        "Complete:".green().bold(),
        stats.created.to_string().green(),
        stats.skipped.to_string().yellow(),
        user_owned_part,
        warnings_part,
    );
}
