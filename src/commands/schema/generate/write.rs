//! Phase 5: write the in-memory generated files to disk.
//!
//! For each generated file, in order:
//!
//! 1. **`user_owned` gate** — files matching the manifest are skipped
//!    wholesale (neither read, merged, nor written).
//! 2. **Dry-run branch** — print what would be written and continue.
//! 3. **`exists()` gate** — skip if the file is on disk. Non-migration files
//!    are overwritten only under `--force`; **migration files are immutable
//!    history** and are never overwritten, even under `--force` (a schema
//!    change to an existing table lands as a NEW forward migration via
//!    `migration generate`, not by rewriting its applied CREATE — rewriting
//!    applied bytes would brick any DB that already applied them, sqlx
//!    `ChecksumMismatch`). Timestamp stability is handled once, upstream, by
//!    `stabilize_migration_timestamps` — this phase is a dumb writer.
//! 4. **Strategy routing** — route to [`super::super::merge`] when the path
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
            // (3) exists() gate. An existing file is skipped unless `--force`
            // asks to overwrite it — EXCEPT migrations, which are immutable
            // history and are never overwritten, even under `--force`. A schema
            // change to an existing table lands as a NEW forward migration
            // (`migration generate`), not by rewriting its applied CREATE;
            // overwriting applied bytes would brick any DB that already applied
            // them (sqlx ChecksumMismatch). Council 2026-07-28.
            let path_str = path.to_string_lossy();
            let is_migration = path_str.starts_with("migrations/") && path_str.ends_with(".sql");
            if full_path.exists() && (!force || is_migration) {
                let hint = if is_migration {
                    "(migration — immutable history, not overwritten even with --force)"
                } else {
                    "(use --force to overwrite)"
                };
                pb.println(format!(
                    "  {} {} {}",
                    "Skipping:".yellow(),
                    full_path.display(),
                    hint,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generators::GeneratedOutput;
    use globset::GlobSetBuilder;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "metaphor-schema-write-test-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("migrations")).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        dir
    }

    fn empty_user_owned() -> GlobSet {
        GlobSetBuilder::new().build().unwrap()
    }

    /// Council 2026-07-28 (regression): a migration already on disk must NEVER be
    /// overwritten — not even under `--force`. Rewriting an applied CREATE would
    /// brick any DB that already applied the original bytes (sqlx ChecksumMismatch);
    /// a schema change to an existing table lands as a NEW forward migration.
    #[test]
    fn force_does_not_overwrite_existing_migration() {
        let dir = scratch_dir("force-mig-immutable");
        let mig = PathBuf::from("migrations/20260101000000_create_foo.up.sql");
        let applied = "-- Generated by metaphor-schema\n-- APPLIED bytes\nCREATE TABLE buying.foo (id uuid);\n";
        std::fs::write(dir.join(&mig), applied).unwrap();

        // The generator now emits DIFFERENT bytes for the same path (a fold).
        let folded = "-- Generated by metaphor-schema\n-- FOLDED bytes\nCREATE TABLE buying.foo (id uuid, company_id uuid NOT NULL);\n".to_string();
        // A non-migration file that SHOULD still be overwritten under --force.
        let code = PathBuf::from("src/notes.txt");
        std::fs::write(dir.join(&code), "old").unwrap();

        let mut files = HashMap::new();
        files.insert(mig.clone(), folded);
        files.insert(code.clone(), "new".to_string());
        let generated = GeneratedOutput { files };

        let _ = write_generated_files(&generated, &dir, &empty_user_owned(), true, false).unwrap();

        // Migration kept its applied bytes (immutable).
        let on_disk = std::fs::read_to_string(dir.join(&mig)).unwrap();
        assert!(
            on_disk.contains("APPLIED bytes") && !on_disk.contains("FOLDED"),
            "--force must not overwrite an existing migration; got:\n{on_disk}"
        );
        // Non-migration file WAS overwritten (control).
        assert_eq!(
            std::fs::read_to_string(dir.join(&code)).unwrap(),
            "new",
            "--force should still overwrite non-migration files"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A migration NOT yet on disk (brand-new module / new table) is still written,
    /// so initial generation and new-forward-migration generation keep working.
    #[test]
    fn force_writes_brand_new_migration() {
        let dir = scratch_dir("force-mig-new");
        let mig = PathBuf::from("migrations/20260101000000_create_bar.up.sql");
        let bytes = "-- Generated by metaphor-schema\nCREATE TABLE buying.bar (id uuid);\n".to_string();
        let mut files = HashMap::new();
        files.insert(mig.clone(), bytes.clone());
        let generated = GeneratedOutput { files };

        let _ = write_generated_files(&generated, &dir, &empty_user_owned(), true, false).unwrap();

        assert_eq!(
            std::fs::read_to_string(dir.join(&mig)).unwrap(),
            bytes,
            "a migration not yet on disk should be written"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
