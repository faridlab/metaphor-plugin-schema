//! Shared helpers for the migration and status subcommands.
//!
//! Both commands need to compare the *current* resolved schema against an
//! *old* one — either a previously-saved `.schema_snapshot.json` file or a
//! live database introspection. This module owns:
//!
//! - [`get_old_schema`] — resolve the "old" side of the diff.
//! - [`build_schema_snapshot`] — convert a [`ResolvedSchema`] into the
//!   snapshot format that the diff engine consumes.
//! - [`type_to_sql`] — map [`TypeRef`] values to their PostgreSQL spelling
//!   (used when building column snapshots).

#[cfg(feature = "database")]
use anyhow::Context;
use anyhow::Result;
use colored::Colorize;
use std::fs;
use std::path::Path;

use crate::ast::{IndexType, PrimitiveType, TypeRef};

/// Get the "old" schema for diffing — from a live database (if URL provided
/// and the `database` feature is enabled) or from a `.schema_snapshot.json`
/// file alongside the schema directory.
pub(super) fn get_old_schema(
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
pub(super) fn build_schema_snapshot(
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

        // Company-fenced (ADR-0008) iff a `company_id` field is present and not `@global` — the same
        // structural, opt-*out* rule the full-regen path and the Rust generator use.
        let company_scoped = model
            .fields
            .iter()
            .any(|f| f.name == "company_id" && !f.has_attribute("global"));

        snapshot.tables.insert(
            table_name.clone(),
            TableSnapshot {
                name: table_name,
                columns,
                indexes,
                primary_key,
                company_scoped,
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
