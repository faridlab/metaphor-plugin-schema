//! `metaphor schema watch` — observe a module's schema directory and
//! re-run `execute_generate` on every debounced change.
//!
//! Each iteration is a `--force` regen (overwrite existing) so the watcher
//! reflects what the user would see if they ran `metaphor schema generate`
//! manually. Errors are reported but do not stop the watcher — fix and save
//! again.

use anyhow::{Context, Result};
use colored::Colorize;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::Duration;

use super::discovery::{find_module_schema_path, is_schema_file};
use super::execute_generate;

pub(super) fn execute_watch(module: &str, target: &str, output: Option<PathBuf>) -> Result<()> {
    use notify::RecursiveMode;
    use notify_debouncer_mini::new_debouncer;

    println!(
        "{} schema files for module: {}",
        "Watching".green().bold(),
        module.cyan()
    );
    println!("  Press {} to stop", "Ctrl+C".yellow());
    println!();

    let schema_path = find_module_schema_path(module)?;

    if !schema_path.exists() {
        anyhow::bail!("Schema path does not exist: {}", schema_path.display());
    }

    println!("  {} {}", "Watching:".blue(), schema_path.display());
    println!();

    println!("{}", "Running initial generation...".cyan());
    if let Err(e) = execute_generate(
        module,
        target,
        output.clone(),
        false,
        true,
        false,
        false,
        "HEAD",
        false,
        None,
        None,
        None,
        false,
    ) {
        println!("  {} {}", "Error:".red().bold(), e);
    }
    println!();

    let (tx, rx) = channel();

    let mut debouncer =
        new_debouncer(Duration::from_millis(500), tx).context("Failed to create file watcher")?;

    debouncer
        .watcher()
        .watch(&schema_path, RecursiveMode::Recursive)
        .context("Failed to start watching")?;

    println!("{}", "Waiting for changes...".dimmed());

    loop {
        match rx.recv() {
            Ok(Ok(events)) => {
                let schema_changed = events.iter().any(|event| is_schema_file(&event.path));

                if schema_changed {
                    println!();
                    println!(
                        "{} {}",
                        "Change detected:".yellow().bold(),
                        chrono::Local::now().format("%H:%M:%S")
                    );

                    for event in &events {
                        if is_schema_file(&event.path) {
                            let file_name = event
                                .path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("unknown");
                            println!("  {} {}", "Modified:".blue(), file_name);
                        }
                    }

                    println!();

                    match execute_generate(
                        module,
                        target,
                        output.clone(),
                        false,
                        true,
                        false,
                        false,
                        "HEAD",
                        false,
                        None,
                        None,
                        None,
                        false,
                    ) {
                        Ok(()) => {
                            println!();
                            println!("{}", "Waiting for changes...".dimmed());
                        }
                        Err(e) => {
                            println!("  {} {}", "Error:".red().bold(), e);
                            println!();
                            println!("{}", "Fix the error and save again...".yellow());
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                println!("  {} {:?}", "Watch error:".red(), e);
            }
            Err(e) => {
                println!("  {} {}", "Channel error:".red(), e);
                break;
            }
        }
    }

    Ok(())
}
