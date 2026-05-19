//! Phase 0: `--changed` short-circuit.
//!
//! When the user passes `--changed`, the generator only runs if there are
//! schema changes since the git base ref. Returns `true` if generation
//! should proceed, `false` if the caller should short-circuit with success.

use anyhow::{Context, Result};
use colored::Colorize;

use crate::git::{ChangeSummary, GitChangeDetector};

/// Returns `Ok(true)` if changes were detected (continue generation),
/// `Ok(false)` if there are no changes (caller should return `Ok(())`).
pub(super) fn should_generate(module: &str, base: &str) -> Result<bool> {
    println!(
        "{} for module: {} (comparing against {})",
        "Checking for schema changes".cyan().bold(),
        module.cyan(),
        base.yellow()
    );

    let repo_root =
        GitChangeDetector::find_repo_root().context("Failed to find git repository root")?;

    let detector = GitChangeDetector::new(repo_root).with_base_ref(base);
    let changes = detector.get_changed_schemas(module)?;

    if changes.is_empty() {
        println!("  {} No schema changes detected", "✓".green());
        println!("  Use {} to force full generation", "--force".yellow());
        return Ok(false);
    }

    let summary = ChangeSummary::from_changes(&changes);
    println!("{}", summary.display());
    println!();

    let affected_targets = detector.get_affected_targets(&changes);
    println!(
        "  {} {}",
        "Affected targets:".blue(),
        affected_targets.join(", ").yellow()
    );
    println!();

    Ok(true)
}
