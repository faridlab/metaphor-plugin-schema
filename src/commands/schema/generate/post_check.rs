//! Phase 6 (optional): run `cargo check` on the generated module to verify
//! the output compiles. Triggered by `--validate` and only runs when files
//! were actually written (i.e. not in `--dry-run`).
//!
//! Failures bubble up as an error so CI gates that pass `--validate` will
//! fail the build. The package name is derived from the module name with
//! the `backbone-` prefix convention.

use anyhow::Result;
use colored::Colorize;

pub(super) fn run_cargo_check(module: &str) -> Result<()> {
    println!();
    println!("{}", "Validating generated code...".cyan().bold());

    let package_name = format!("backbone-{}", module.to_lowercase().replace('-', "_"));

    let result = std::process::Command::new("cargo")
        .args(["check", "--package", &package_name])
        .output();

    match result {
        Ok(output) => {
            if output.status.success() {
                println!("  {} Generated code compiles successfully", "✓".green());
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                println!("  {} Generated code has compilation errors:", "✗".red());
                for line in stderr.lines().take(20) {
                    println!("    {}", line.red());
                }
                if stderr.lines().count() > 20 {
                    println!("    {} ...", "...".dimmed());
                }
                anyhow::bail!(
                    "Compilation failed. Please fix the schema generator or the schema definitions."
                );
            }
        }
        Err(e) => {
            println!("  {} Failed to run cargo check: {}", "Warning:".yellow(), e);
            println!("  {} Skipping validation", "→".dimmed());
            Ok(())
        }
    }
}
