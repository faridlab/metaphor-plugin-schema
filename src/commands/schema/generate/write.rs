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
use walkdir::WalkDir;

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
    pub orphans: usize,
    /// Retired generated files removed this run (see [`is_retired_generated`]).
    pub retired: usize,
}

pub(super) fn write_generated_files(
    generated: &GeneratedOutput,
    output_dir: &Path,
    user_owned: &GlobSet,
    force: bool,
    dry_run: bool,
    full_run: bool,
    merge_custom: bool,
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
        orphans: 0,
        retired: 0,
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
            // A module on the extension layout keeps no hand code in a generated Rust file, so
            // that file is written whole: there is nothing to merge back (ADR-0031). Its YAML
            // config and seed files still merge as before.
            let is_rs = full_path.extension().and_then(|s| s.to_str()) == Some("rs");
            let final_content = if !merge_custom && is_rs {
                content.clone()
            } else {
                route_merge(&full_path, content, &pb, &mut stats.custom_warnings)?
            };

            fs::write(&full_path, final_content)
                .with_context(|| format!("Failed to write {}", full_path.display()))?;

            pb.println(format!("  {} {}", "✓".green(), full_path.display()));
            stats.created += 1;
        }

        pb.inc(1);
    }

    pb.finish_and_clear();

    // Orphan report — only on a full run (all targets, no model/hook/workflow
    // filters, no split). A filtered run emits a partial file set, and every
    // file outside the filter would look orphaned.
    if !dry_run && full_run {
        let found = find_orphan_source_files(generated, output_dir, user_owned);
        // Output a generator emitted earlier and no longer does (a disabled target, a
        // retired model) is removed when it is provably untouched generated code that
        // git can restore; everything else stays and is reported below.
        let mut orphans = Vec::new();
        for rel in found {
            if is_retired_generated(output_dir, &rel) && fs::remove_file(output_dir.join(&rel)).is_ok() {
                println!("  {} removed retired generated file {}", "-".dimmed(), rel);
                stats.retired += 1;
            } else {
                orphans.push(rel);
            }
        }
        if !orphans.is_empty() {
            stats.orphans = orphans.len();
            println!();
            println!(
                "{} {} orphaned source file(s) — on disk, neither generated nor declared user_owned:",
                "!".yellow().bold(),
                orphans.len()
            );
            for rel in &orphans {
                println!("  {} {}", "•".yellow(), rel);
            }
            println!(
                "  {} these never regenerate and are never wiped; retire them, or declare them user_owned in metaphor.codegen.yaml",
                "Tip:".cyan()
            );
        }
    }

    Ok(stats)
}

/// An orphan is retired generated output, safe to remove, only when all of these
/// hold: it carries the generator's do-not-edit header (a "Safe to edit" scaffold
/// does not qualify), it holds no CUSTOM block with code in it, and git tracks it
/// unchanged against HEAD, so removing it loses nothing git does not keep.
fn is_retired_generated(output_dir: &Path, rel: &str) -> bool {
    let Ok(text) = fs::read_to_string(output_dir.join(rel)) else {
        return false;
    };
    let header = text
        .lines()
        .take(12)
        .any(|l| l.contains("by metaphor-schema. Do not edit manually."));
    if !header {
        return false;
    }
    if holds_custom_code(&text) {
        return false;
    }
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(output_dir)
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    git(&["ls-files", "--error-unmatch", "--", rel]) && git(&["diff", "--quiet", "HEAD", "--", rel])
}

/// Whether a CUSTOM zone of `text` holds code. Only a marker line opens or closes a zone: a
/// doc comment that names the marker ("extend in the `// <<< CUSTOM` zone") is prose.
fn holds_custom_code(text: &str) -> bool {
    let mut in_custom = false;
    for line in text.lines() {
        let t = line.trim();
        let marker = t.starts_with("// <<< CUSTOM") || t.starts_with("// END CUSTOM");
        if marker && t.starts_with("// <<< CUSTOM") && !t.contains("END") {
            in_custom = true;
        } else if marker {
            in_custom = false;
        } else if in_custom && !t.is_empty() && !t.starts_with("//") {
            return true;
        }
    }
    false
}

/// Walk `src/` and `tests/` for `.rs` files that no generator emits and the
/// manifest does not declare user-owned: the silently-immortal class (dropped
/// emitters, retired models, scaffold leftovers). Relative forward-slash
/// paths, sorted for stable output.
fn find_orphan_source_files(
    generated: &GeneratedOutput,
    output_dir: &Path,
    user_owned: &GlobSet,
) -> Vec<String> {
    let mut orphans = Vec::new();
    for root in ["src", "tests"] {
        let root_path = output_dir.join(root);
        if !root_path.is_dir() {
            continue;
        }
        for entry in WalkDir::new(&root_path).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("rs") {
                continue;
            }
            let Ok(rel) = path.strip_prefix(output_dir) else {
                continue;
            };
            if generated.files.contains_key(rel) || user_owned.is_match(rel) {
                continue;
            }
            orphans.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    orphans.sort();
    orphans
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

        let _ = write_generated_files(&generated, &dir, &empty_user_owned(), true, false, true, true)
            .unwrap();

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
        let bytes =
            "-- Generated by metaphor-schema\nCREATE TABLE buying.bar (id uuid);\n".to_string();
        let mut files = HashMap::new();
        files.insert(mig.clone(), bytes.clone());
        let generated = GeneratedOutput { files };

        let _ = write_generated_files(&generated, &dir, &empty_user_owned(), true, false, true, true)
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(dir.join(&mig)).unwrap(),
            bytes,
            "a migration not yet on disk should be written"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The orphan report: a `.rs` file on disk that no generator emits and the
    /// manifest does not declare user-owned is reported; generated and
    /// user-owned files are not.
    #[test]
    fn full_run_reports_orphaned_source_files() {
        let dir = scratch_dir("orphan-report");
        std::fs::create_dir_all(dir.join("src/application/subscriptions")).unwrap();

        let generated_rs = PathBuf::from("src/lib.rs");
        std::fs::write(dir.join(&generated_rs), "generated").unwrap();
        let orphan = PathBuf::from("src/application/subscriptions/mod.rs");
        std::fs::write(dir.join(&orphan), "stale").unwrap();
        let owned = PathBuf::from("src/application/service/hand_written.rs");
        std::fs::create_dir_all(dir.join(&owned).parent().unwrap()).unwrap();
        std::fs::write(dir.join(&owned), "hand").unwrap();

        let mut files = HashMap::new();
        files.insert(generated_rs, "// emitted".to_string());
        let generated = GeneratedOutput { files };

        let mut user_owned = GlobSetBuilder::new();
        user_owned.add(globset::Glob::new("src/application/service/**").unwrap());
        let user_owned = user_owned.build().unwrap();

        let stats =
            write_generated_files(&generated, &dir, &user_owned, true, false, true, true).unwrap();

        assert_eq!(
            stats.orphans, 1,
            "exactly the stale subscriptions file should be reported"
        );

        // A non-full run (filtered) never reports: the partial emission would
        // flag everything outside the filter.
        let stats_filtered =
            write_generated_files(&generated, &dir, &user_owned, true, false, false, true).unwrap();
        assert_eq!(stats_filtered.orphans, 0, "filtered runs skip the report");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Retired generated output is removed: an orphan that carries the generator's
    /// do-not-edit header, holds no CUSTOM content and is committed unchanged in git.
    /// Anything else stays and is reported, since deleting it could lose work.
    #[test]
    fn full_run_removes_only_retired_generated_files_git_can_restore() {
        let dir = scratch_dir("retired-removal");
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git").args(args).current_dir(&dir).output().unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "t"]);
        let write = |rel: &str, body: &str| {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        let header = "//! Old handlers\n//!\n//! Generated by metaphor-schema. Do not edit manually.\n";
        write("src/presentation/http/old_handler.rs", &format!("{header}pub fn old() {{}}\n"));
        write("src/presentation/http/scaffold.rs", "//! Generated by metaphor-schema. Safe to edit.\npub fn s() {}\n");
        write("src/presentation/http/kept_custom.rs", &format!("{header}// <<< CUSTOM\npub fn hand() {{}}\n// END CUSTOM\n"));
        write("src/presentation/http/edited.rs", &format!("{header}pub fn e() {{}}\n"));
        git(&["add", "-A"]);
        git(&["commit", "-qm", "seed"]);
        write("src/presentation/http/edited.rs", &format!("{header}pub fn e() {{ /* local edit */ }}\n"));
        write("src/presentation/http/untracked.rs", &format!("{header}pub fn u() {{}}\n"));

        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/lib.rs"), "// emitted".to_string());
        let generated = GeneratedOutput { files };
        let stats = write_generated_files(&generated, &dir, &empty_user_owned(), true, false, true, true).unwrap();

        assert!(!dir.join("src/presentation/http/old_handler.rs").exists(), "retired output is removed");
        for kept in ["scaffold.rs", "kept_custom.rs", "edited.rs", "untracked.rs"] {
            assert!(dir.join("src/presentation/http").join(kept).exists(), "{kept} must stay");
        }
        assert_eq!(stats.retired, 1);
        assert_eq!(stats.orphans, 4, "what stays is still reported");
        let _ = std::fs::remove_dir_all(&dir);
    }

}

#[cfg(test)]
mod custom_code_tests {
    use super::holds_custom_code;

    #[test]
    fn a_doc_comment_naming_the_marker_opens_no_zone() {
        let text = "//! Extend in the `// <<< CUSTOM` zone.\n\nuse a::b;\npub fn f() {}\n// <<< CUSTOM\n// END CUSTOM\n";
        assert!(!holds_custom_code(text));
    }

    #[test]
    fn code_inside_a_zone_is_hand_code() {
        assert!(holds_custom_code("// <<< CUSTOM\nfn mine() {}\n// END CUSTOM\n"));
        assert!(!holds_custom_code("// <<< CUSTOM\n// a note\n// END CUSTOM\n"));
    }
}
