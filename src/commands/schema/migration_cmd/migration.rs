//! `metaphor schema migration` — generate database migration SQL from
//! schema drift.
//!
//! Pipeline:
//!
//! 1. Load and resolve the module's schema.
//! 2. Build a [`SchemaSnapshot`] from the resolved schema (the "new" side).
//! 3. Fetch the "old" snapshot from disk (or live introspection).
//! 4. Diff and run safety analysis.
//! 5. Emit either a `--preview` to stdout, a single `--output` file, or
//!    timestamped paired `*.up.sql` / `*.down.sql` files in `migrations/`.
//! 6. Save the new snapshot to `.schema_snapshot.json` for the next run.

use anyhow::Result;
use colored::Colorize;
use std::fs;
use std::path::PathBuf;

use crate::resolver::resolve_schema;

use super::super::discovery::{find_module_schema_path, find_schema_files};
use super::super::module_loader::build_module_schema;
use super::snapshot::{build_schema_snapshot, get_old_schema};

/// Generate database migration SQL by diffing the current schema against a
/// snapshot (or a live database when `database_url` is provided).
pub(in crate::commands::schema) fn execute_migration(
    module: &str,
    output: Option<PathBuf>,
    destructive: bool,
    database_url: Option<String>,
    preview: bool,
    safe_only: bool,
) -> Result<()> {
    use crate::migration::{diff_schemas, generate_migration, SafetyAnalysis};

    println!(
        "{} for module: {}",
        "Generating migration".green().bold(),
        module.cyan()
    );

    let schema_path = find_module_schema_path(module)?;
    let schema_files = find_schema_files(&schema_path)?;

    if schema_files.is_empty() {
        println!("{}", "No schema files found".yellow());
        return Ok(());
    }

    let (module_schema, parse_errors) = build_module_schema(module, &schema_files)?;

    if !parse_errors.is_empty() {
        for error in &parse_errors {
            println!("  {}", error.red());
        }
        anyhow::bail!("Parsing failed");
    }

    let resolved = resolve_schema(&module_schema)
        .map_err(|e| anyhow::anyhow!("Schema validation failed: {:?}", e))?;

    let new_schema = build_schema_snapshot(&resolved);
    // The module's own Postgres schema (index.model.yaml `schema:`); models may
    // override per-model, so take the first non-empty one as the introspection
    // target. Bare table names in both snapshots then line up 1:1.
    let pg_schema = module_schema
        .models
        .iter()
        .find_map(|m| m.schema.as_deref().filter(|s| !s.is_empty()))
        .unwrap_or("public");
    let old_schema = get_old_schema(&schema_path, database_url.as_deref(), pg_schema)?;

    let diff = diff_schemas(&old_schema, &new_schema);

    if !diff.has_changes() {
        println!("  {} No schema changes detected", "✓".green());
        return Ok(());
    }

    let safety = SafetyAnalysis::from_diff(&diff);

    println!();
    println!("{}", "Schema changes detected:".yellow().bold());
    println!("{}", diff.summary());
    println!();
    println!("{}", "Safety analysis:".blue().bold());
    println!("{}", safety.summary());

    if diff.has_destructive_changes() {
        println!();
        println!(
            "{}",
            "WARNING: Destructive changes detected (data loss possible)!"
                .red()
                .bold()
        );
        if safe_only {
            println!(
                "{}",
                "  --safe-only: Destructive changes will be excluded from migration".yellow()
            );
        }
        if !destructive && !safe_only {
            println!(
                "{}",
                "  Use --destructive to uncomment DROP statements in migration output".yellow()
            );
        }
    }

    for change in diff.table_changes.values() {
        for rename in &change.rename_candidates {
            println!(
                "  {} Possible rename in {}: {} -> {} (type: {})",
                "?".cyan(),
                change.table_name,
                rename.old_name,
                rename.new_name,
                rename.data_type
            );
        }
    }

    let up_sql = crate::migration::generate_up_migration(&diff, &new_schema, destructive);
    let down_sql = crate::migration::generate_down_migration(&diff);

    // Chain apply-order guard: replay the module's existing up-chain plus
    // this migration. A statement referencing a relation no earlier migration
    // (nor this one) creates is a fresh-database chain break — long-lived
    // databases drift past it, only the empty-database apply hits it.
    let migrations_dir = schema_path
        .parent()
        .unwrap_or(&schema_path)
        .join("migrations");
    let mut chain: Vec<(String, String)> = Vec::new();
    if migrations_dir.exists() {
        let mut up_files: Vec<std::path::PathBuf> = fs::read_dir(&migrations_dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().ends_with(".up.sql"))
                    .unwrap_or(false)
            })
            .collect();
        up_files.sort();
        for path in up_files {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let sql = fs::read_to_string(&path)?;
            chain.push((name, sql));
        }
    }
    chain.push((
        crate::migration::NEW_MIGRATION_LABEL.to_string(),
        up_sql.clone(),
    ));
    let chain_report = crate::migration::simulate_chain(&chain, pg_schema);

    for req in &chain_report.external_requires {
        println!(
            "  {} cross-module relation required at apply time: {} (apply order owned by the service composition)",
            "ℹ".blue(),
            req.cyan()
        );
    }
    let (new_violations, old_violations): (Vec<_>, Vec<_>) = chain_report
        .violations
        .iter()
        .partition(|v| v.file == crate::migration::NEW_MIGRATION_LABEL);
    if !old_violations.is_empty() {
        println!(
            "{}",
            "WARNING: pre-existing chain-order violations (fresh-database applies break; the fix belongs in a NEW migration, never edited into an old file):"
                .yellow()
                .bold()
        );
        for v in &old_violations {
            println!(
                "  {} {}: {} — {}",
                "!".yellow(),
                v.file,
                v.relation,
                v.statement
            );
        }
    }
    if !new_violations.is_empty() {
        for v in &new_violations {
            eprintln!(
                "  {} chain-order violation in the generated migration: {} — {}",
                "✗".red(),
                v.relation,
                v.statement
            );
        }
        anyhow::bail!(
            "the generated migration references relations that do not exist at its \
             position in the chain; refusing to write it"
        );
    }

    if preview {
        println!();
        println!("{}", "UP Migration (preview):".green().bold());
        println!("{}", "─".repeat(60));
        println!("{}", up_sql);
        println!("{}", "─".repeat(60));
        if !down_sql.trim().is_empty() {
            println!();
            println!("{}", "DOWN Migration (preview):".yellow().bold());
            println!("{}", "─".repeat(60));
            println!("{}", down_sql);
            println!("{}", "─".repeat(60));
        }
        return Ok(());
    }

    if let Some(output_path) = output {
        let combined = generate_migration(&diff, &new_schema, destructive);
        fs::write(&output_path, &combined)?;
        println!();
        println!(
            "{} {}",
            "Migration written to:".green(),
            output_path.display()
        );
    } else {
        let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
        let migrations_dir = schema_path
            .parent()
            .unwrap_or(&schema_path)
            .join("migrations");

        fs::create_dir_all(&migrations_dir)?;

        let up_file = migrations_dir.join(format!("{}_{}_migration.up.sql", timestamp, module));
        let down_file = migrations_dir.join(format!("{}_{}_migration.down.sql", timestamp, module));

        let up_content = format!(
            "-- Migration generated by metaphor-schema\n-- WARNING: Review carefully before applying!\n\n{}",
            up_sql
        );
        fs::write(&up_file, &up_content)?;

        let down_content = format!(
            "-- Rollback migration generated by metaphor-schema\n\n{}",
            down_sql
        );
        fs::write(&down_file, &down_content)?;

        println!();
        println!(
            "{} {}",
            "UP migration written to:".green(),
            up_file.display()
        );
        println!(
            "{} {}",
            "DOWN migration written to:".green(),
            down_file.display()
        );
    }

    let snapshot_path = schema_path
        .parent()
        .unwrap_or(&schema_path)
        .join(".schema_snapshot.json");
    let snapshot_json = serde_json::to_string_pretty(&new_schema)?;
    fs::write(&snapshot_path, &snapshot_json)?;
    println!(
        "{} {}",
        "Schema snapshot saved to:".blue(),
        snapshot_path.display()
    );

    Ok(())
}
