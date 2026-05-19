//! Phase 5: write the in-memory generated files to disk.
//!
//! For each generated file, in order:
//!
//! 1. **`user_owned` gate** — files matching the manifest are skipped
//!    wholesale (neither read, merged, nor written).
//! 2. **Dry-run branch** — print what would be written and continue.
//! 3. **Migration identity dedup** — for timestamp-prefixed migration
//!    filenames, skip if a sibling with the same `_<identity>.{up,down}.sql`
//!    suffix already exists (unless `--force`).
//! 4. **`exists()` gate** — skip if the file is on disk and `--force` was
//!    not passed.
//! 5. **Strategy routing** — route to [`super::super::merge`] when the path
//!    is a YAML config, a seed file, a `seed_order.yml`, or any `.rs`
//!    file; otherwise write the generated content as-is.
//!
//! Returns a [`WriteStats`] summary the caller uses to print the run
//! result and to decide whether to run post-generation validation.

use anyhow::{Context, Result};
use colored::Colorize;
use globset::GlobSet;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::Path;

use crate::generators::GeneratedOutput;

use super::super::merge::{
    detect_unprotected_custom_code, merge_rust_mod_custom, merge_seed_file, merge_seed_order,
    merge_yaml_config,
};
use super::super::migrations::{
    is_unstable_timestamped_migration, migration_identity_already_exists,
};

/// Counters reported after the write loop completes.
pub(super) struct WriteStats {
    pub created: usize,
    pub skipped: usize,
    pub custom_warnings: usize,
    pub user_owned_skipped: usize,
}

pub(super) fn write_generated_files(
    generated: &GeneratedOutput,
    output_dir: &Path,
    user_owned: &GlobSet,
    force: bool,
    dry_run: bool,
) -> Result<WriteStats> {
    println!();
    println!(
        "{} {} file(s) to generate",
        "Generated".green().bold(),
        generated.files.len()
    );

    let pb = ProgressBar::new(generated.files.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("█▓░"),
    );

    let mut stats = WriteStats {
        created: 0,
        skipped: 0,
        custom_warnings: 0,
        user_owned_skipped: 0,
    };

    for (path, content) in &generated.files {
        let full_path = output_dir.join(path);
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        pb.set_message(file_name.to_string());

        // (1) user_owned gate — match the *relative* path the manifest declared.
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
            stats.user_owned_skipped += 1;
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
            // (3) Migration identity dedup.
            if is_unstable_timestamped_migration(&full_path)
                && migration_identity_already_exists(&full_path)
                && !force
            {
                pb.println(format!(
                    "  {} {} (identity already migrated under a different timestamp)",
                    "Skipping:".yellow(),
                    full_path.display()
                ));
                stats.skipped += 1;
                pb.inc(1);
                continue;
            }

            // (4) exists() gate.
            if full_path.exists() && !force {
                pb.println(format!(
                    "  {} {} (use --force to overwrite)",
                    "Skipping:".yellow(),
                    full_path.display()
                ));
                stats.skipped += 1;
                pb.inc(1);
                continue;
            }

            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create directory {}", parent.display()))?;
            }

            // (5) Strategy routing.
            let final_content = route_merge(&full_path, content, &pb, &mut stats.custom_warnings)?;

            fs::write(&full_path, final_content)
                .with_context(|| format!("Failed to write {}", full_path.display()))?;

            pb.println(format!("  {} {}", "✓".green(), full_path.display()));
            stats.created += 1;
        }

        pb.inc(1);
    }

    pb.finish_and_clear();

    Ok(stats)
}

/// Route a generated file's content through the appropriate [`super::super::merge`]
/// strategy based on its path. Bumps `custom_warnings` when an `.rs` file
/// has unprotected custom code outside `// <<< CUSTOM` markers.
fn route_merge(
    full_path: &Path,
    content: &str,
    pb: &ProgressBar,
    custom_warnings: &mut usize,
) -> Result<String> {
    let path_str = full_path.to_string_lossy();

    if path_str.contains("config/application")
        && full_path.extension().and_then(|s| s.to_str()) == Some("yml")
    {
        return merge_yaml_config(content, full_path);
    }

    if path_str.contains("migrations/seeds/seed_order.yml") {
        return merge_seed_order(content, full_path);
    }

    if path_str.contains("migrations/seeds/")
        && full_path.extension().and_then(|s| s.to_str()) == Some("sql")
    {
        return merge_seed_file(content, full_path);
    }

    if full_path.extension().and_then(|s| s.to_str()) == Some("rs") {
        let warnings = detect_unprotected_custom_code(content, full_path);
        if !warnings.is_empty() {
            *custom_warnings += warnings.len();
            pb.println(format!(
                "  {} {} has {} unprotected custom line(s) that may be lost:",
                "⚠".yellow(),
                full_path.display(),
                warnings.len()
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
        return merge_rust_mod_custom(content, full_path);
    }

    // Default: write generated content as-is.
    Ok(content.to_string())
}
