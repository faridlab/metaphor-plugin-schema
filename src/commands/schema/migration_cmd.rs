//! `metaphor schema migration` and `metaphor schema status` — generate
//! database migration SQL from schema drift, or report drift without
//! generating anything.
//!
//! Drift is computed by diffing the current resolved schema against an
//! "old" snapshot, where the old snapshot comes from either a live
//! database introspection (when `database_url` is provided and the
//! `database` feature is on) or a previously-saved `.schema_snapshot.json`
//! file alongside the schema directory.
//!
//! Output:
//!
//! - `migration` writes timestamped paired `*.up.sql` / `*.down.sql` files
//!   into the module's `migrations/` directory, plus a refreshed
//!   `.schema_snapshot.json`. With `--output`, a combined single file is
//!   written instead. With `--preview`, the SQL is printed and no files
//!   are touched.
//! - `status` is read-only and exits with an error (non-zero) when drift
//!   is detected — useful as a CI gate.

use anyhow::{Context, Result};
use colored::Colorize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::{IndexType, PrimitiveType, TypeRef};
use crate::resolver::resolve_schema;

use super::discovery::{find_module_schema_path, find_schema_files};
use super::module_loader::build_module_schema;

/// Generate database migration SQL by diffing the current schema against a
/// snapshot (or a live database when `database_url` is provided).
pub(super) fn execute_migration(
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
    let old_schema = get_old_schema(&schema_path, database_url.as_deref())?;

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

/// Show schema drift between YAML definitions and database/snapshot (read-only).
pub(super) fn execute_status(module: &str, database_url: Option<String>) -> Result<()> {
    use crate::migration::{diff_schemas, SafetyAnalysis};

    println!(
        "{} for module: {}",
        "Checking schema status".green().bold(),
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
    let old_schema = get_old_schema(&schema_path, database_url.as_deref())?;

    let diff = diff_schemas(&old_schema, &new_schema);

    if !diff.has_changes() {
        println!();
        println!("  {} Schema is up to date — no drift detected", "✓".green());
        return Ok(());
    }

    let safety = SafetyAnalysis::from_diff(&diff);

    println!();
    println!("{}", "Schema drift detected:".yellow().bold());
    println!("{}", diff.summary());
    println!();
    println!("{}", "Safety analysis:".blue().bold());
    println!("{}", safety.summary());

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

    if diff.has_destructive_changes() {
        println!();
        println!("{}", "WARNING: Destructive changes detected!".red().bold());
    }

    println!();
    println!(
        "Run {} to generate migration files.",
        format!("metaphor-schema schema migration {}", module).cyan()
    );

    // Signal drift via error so callers (CLI, CI) can handle appropriately.
    anyhow::bail!("Schema drift detected — {} change(s) pending", {
        let mut count = diff.tables_added.len() + diff.tables_removed.len();
        for change in diff.table_changes.values() {
            count += change.columns_added.len()
                + change.columns_removed.len()
                + change.columns_modified.len();
        }
        count += diff.enums_added.len() + diff.enums_removed.len();
        count
    });
}

/// Get the "old" schema for diffing — from a live database (if URL provided
/// and the `database` feature is enabled) or from a `.schema_snapshot.json`
/// file alongside the schema directory.
fn get_old_schema(
    schema_path: &Path,
    database_url: Option<&str>,
) -> Result<crate::migration::SchemaSnapshot> {
    #[cfg(feature = "database")]
    if let Some(url) = database_url {
        println!(
            "  {} {}",
            "Introspecting database:".blue(),
            url.split('@').last().unwrap_or("***")
        );

        let introspector = crate::migration::DatabaseIntrospector::new(url);
        let rt = tokio::runtime::Runtime::new().context("Failed to create tokio runtime")?;
        let schema = rt.block_on(introspector.introspect("public"))?;

        println!(
            "  {} Found {} tables, {} enums",
            "✓".green(),
            schema.tables.len(),
            schema.enums.len()
        );

        return Ok(schema);
    }

    #[cfg(not(feature = "database"))]
    if database_url.is_some() {
        anyhow::bail!(
            "Database introspection requires the 'database' feature. \
             Rebuild with: cargo build -p metaphor-schema --features database"
        );
    }

    let snapshot_path = schema_path
        .parent()
        .unwrap_or(schema_path)
        .join(".schema_snapshot.json");

    if snapshot_path.exists() {
        let content = fs::read_to_string(&snapshot_path)?;
        Ok(serde_json::from_str(&content).unwrap_or_default())
    } else {
        Ok(crate::migration::SchemaSnapshot::default())
    }
}

/// Build a [`SchemaSnapshot`] from a resolved schema — convert every model
/// and enum into the snapshot shape so it can be diffed against another
/// snapshot or a live introspection.
fn build_schema_snapshot(
    resolved: &crate::resolver::ResolvedSchema,
) -> crate::migration::SchemaSnapshot {
    use crate::migration::{
        ColumnSnapshot, EnumSnapshot, IndexSnapshot, SchemaSnapshot, TableSnapshot,
    };

    let mut snapshot = SchemaSnapshot::default();

    for model in &resolved.schema.models {
        let table_name = model.collection_name();
        let mut columns = indexmap::IndexMap::new();
        let mut primary_key = None;

        for field in &model.fields {
            let sql_type = type_to_sql(&field.type_ref);
            let nullable = field.type_ref.is_optional();

            if field.is_primary_key() {
                primary_key = Some(field.name.clone());
            }

            columns.insert(
                field.name.clone(),
                ColumnSnapshot {
                    name: field.name.clone(),
                    data_type: sql_type,
                    nullable,
                    default: None,
                    is_unique: field.is_unique(),
                },
            );
        }

        let mut indexes = indexmap::IndexMap::new();
        for index in &model.indexes {
            let idx_name = format!("idx_{}_{}", table_name, index.fields.join("_"));
            indexes.insert(
                idx_name.clone(),
                IndexSnapshot {
                    name: idx_name,
                    columns: index.fields.clone(),
                    unique: matches!(index.index_type, IndexType::Unique),
                    index_type: match index.index_type {
                        IndexType::Index => "btree".to_string(),
                        IndexType::Unique => "unique".to_string(),
                        IndexType::Fulltext => "gin".to_string(),
                        IndexType::Gin => "gin".to_string(),
                    },
                },
            );
        }

        snapshot.tables.insert(
            table_name.clone(),
            TableSnapshot {
                name: table_name,
                columns,
                indexes,
                primary_key,
            },
        );
    }

    for enum_def in &resolved.schema.enums {
        let enum_name = enum_def.name.to_lowercase();
        let variants: Vec<String> = enum_def.variants.iter().map(|v| v.name.clone()).collect();
        snapshot.enums.insert(
            enum_name.clone(),
            EnumSnapshot {
                name: enum_name,
                variants,
            },
        );
    }

    snapshot
}

/// Convert a [`TypeRef`] to its PostgreSQL SQL-type spelling.
fn type_to_sql(type_ref: &TypeRef) -> String {
    match type_ref {
        TypeRef::Primitive(p) => match p {
            PrimitiveType::String => "VARCHAR(255)".to_string(),
            PrimitiveType::Int => "INTEGER".to_string(),
            PrimitiveType::Int32 => "INTEGER".to_string(),
            PrimitiveType::Int64 => "BIGINT".to_string(),
            PrimitiveType::Float => "REAL".to_string(),
            PrimitiveType::Float32 => "REAL".to_string(),
            PrimitiveType::Float64 => "DOUBLE PRECISION".to_string(),
            PrimitiveType::Bool => "BOOLEAN".to_string(),
            PrimitiveType::Uuid => "UUID".to_string(),
            PrimitiveType::Email => "VARCHAR(255)".to_string(),
            PrimitiveType::Url => "TEXT".to_string(),
            PrimitiveType::Phone => "VARCHAR(50)".to_string(),
            PrimitiveType::Slug => "VARCHAR(255)".to_string(),
            PrimitiveType::Ip => "INET".to_string(),
            PrimitiveType::IpV4 => "INET".to_string(),
            PrimitiveType::IpV6 => "INET".to_string(),
            PrimitiveType::Mac => "MACADDR".to_string(),
            PrimitiveType::DateTime => "TIMESTAMPTZ".to_string(),
            PrimitiveType::Date => "DATE".to_string(),
            PrimitiveType::Time => "TIME".to_string(),
            PrimitiveType::Duration => "INTERVAL".to_string(),
            PrimitiveType::Timestamp => "TIMESTAMPTZ".to_string(),
            PrimitiveType::Json => "JSONB".to_string(),
            PrimitiveType::Markdown => "TEXT".to_string(),
            PrimitiveType::Html => "TEXT".to_string(),
            PrimitiveType::Bytes => "BYTEA".to_string(),
            PrimitiveType::Binary => "BYTEA".to_string(),
            PrimitiveType::Base64 => "TEXT".to_string(),
            PrimitiveType::Money => "DECIMAL(19, 4)".to_string(),
            PrimitiveType::Decimal => "DECIMAL".to_string(),
            PrimitiveType::Percentage => "DECIMAL(5, 2)".to_string(),
        },
        TypeRef::Custom(name) => name.to_uppercase(),
        TypeRef::Array(inner) => format!("{}[]", type_to_sql(inner)),
        TypeRef::Optional(inner) => type_to_sql(inner),
        TypeRef::Map { .. } => "JSONB".to_string(),
        TypeRef::ModuleRef { name, .. } => name.to_uppercase(),
    }
}
