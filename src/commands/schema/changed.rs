//! `metaphor schema changed` — show schema files that have changed since
//! a git base ref, plus (optionally) the generated outputs and generator
//! targets that would need to re-run.
//!
//! Read-only inspection. Used by CI pipelines that want to scope regen to
//! affected modules, and by humans who want to know what's about to change
//! before committing.

use anyhow::{Context, Result};
use colored::Colorize;

use crate::git::{ChangeSummary, ChangeType, GitChangeDetector};

pub(super) fn execute_changed(
    module: Option<&str>,
    base: &str,
    show_outputs: bool,
    show_targets: bool,
) -> Result<()> {
    println!(
        "{} (comparing against {})",
        "Detecting schema changes".green().bold(),
        base.yellow()
    );

    let repo_root =
        GitChangeDetector::find_repo_root().context("Failed to find git repository root")?;

    let detector = GitChangeDetector::new(repo_root).with_base_ref(base);

    let changes = if let Some(mod_name) = module {
        println!("  Module: {}", mod_name.cyan());
        detector.get_changed_schemas(mod_name)?
    } else {
        println!("  Scanning all modules...");
        detector.get_all_changed_schemas()?
    };

    println!();

    if changes.is_empty() {
        println!("  {} No schema changes detected", "✓".green());
        return Ok(());
    }

    let summary = ChangeSummary::from_changes(&changes);
    println!("{}", summary.display());
    println!();

    println!("{}", "Changed files:".blue().bold());
    for change in &changes {
        let change_indicator = match change.change_type {
            ChangeType::Added => "+".green(),
            ChangeType::Modified => "M".yellow(),
            ChangeType::Deleted => "-".red(),
            ChangeType::Renamed => "R".cyan(),
            ChangeType::Untracked => "?".dimmed(),
        };
        println!("  {} {}", change_indicator, change.path.display());
    }
    println!();

    if show_outputs {
        let outputs = detector.get_all_affected_outputs(&changes);
        println!("{}", "Affected output files:".blue().bold());
        for output in &outputs {
            println!(
                "  {} {} ({})",
                "→".cyan(),
                output.path.display(),
                output.target.yellow()
            );
        }
        println!();
    }

    if show_targets {
        let targets = detector.get_affected_targets(&changes);
        println!("{}", "Generation targets needed:".blue().bold());
        println!("  {}", targets.join(", ").yellow());
        println!();
        println!(
            "  {} backbone schema generate {} --target {}",
            "Run:".green(),
            module.unwrap_or("<module>"),
            targets.join(",")
        );
    }

    Ok(())
}
