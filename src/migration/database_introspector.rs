//! Database introspection for PostgreSQL
//!
//! Queries `information_schema` and `pg_catalog` to build a [`SchemaSnapshot`]
//! from a live database, enabling accurate diff-based migration generation.

use anyhow::{Context, Result};
use indexmap::IndexMap;
use sqlx::postgres::PgPool;
use sqlx::Row;

use super::{ColumnSnapshot, EnumSnapshot, IndexSnapshot, SchemaSnapshot, TableSnapshot};

/// Introspects a live PostgreSQL database to build a [`SchemaSnapshot`].
pub struct DatabaseIntrospector {
    connection_string: String,
}

impl DatabaseIntrospector {
    pub fn new(connection_string: &str) -> Self {
        Self {
            connection_string: connection_string.to_string(),
        }
    }

    /// Connect to the database and introspect the given schema.
    pub async fn introspect(&self, schema_name: &str) -> Result<SchemaSnapshot> {
        let pool = PgPool::connect(&self.connection_string)
            .await
            .context("Failed to connect to database")?;

        let tables = self.introspect_tables(&pool, schema_name).await?;
        let enums = self.introspect_enums(&pool, schema_name).await?;

        pool.close().await;

        Ok(SchemaSnapshot { tables, enums })
    }

    async fn introspect_tables(
        &self,
        pool: &PgPool,
        schema: &str,
    ) -> Result<IndexMap<String, TableSnapshot>> {
        let mut tables = IndexMap::new();

        // 1. Get all base tables
        let table_rows = sqlx::query(
            "SELECT table_name FROM information_schema.tables \
             WHERE table_schema = $1 AND table_type = 'BASE TABLE' \
             ORDER BY table_name",
        )
        .bind(schema)
        .fetch_all(pool)
        .await
        .context("Failed to query tables")?;

        for row in &table_rows {
            let name: String = row.get("table_name");
            tables.insert(
                name.clone(),
                TableSnapshot {
                    name,
                    columns: IndexMap::new(),
                    indexes: IndexMap::new(),
                    primary_key: None,
                },
            );
        }

        // 2. Get columns for all tables
        let column_rows = sqlx::query(
            "SELECT table_name, column_name, data_type, is_nullable, column_default, \
                    character_maximum_length, numeric_precision, numeric_scale, udt_name \
             FROM information_schema.columns \
             WHERE table_schema = $1 \
             ORDER BY table_name, ordinal_position",
        )
        .bind(schema)
        .fetch_all(pool)
        .await
        .context("Failed to query columns")?;

        for row in &column_rows {
            let table_name: String = row.get("table_name");
            let col_name: String = row.get("column_name");
            let data_type: String = row.get("data_type");
            let is_nullable: String = row.get("is_nullable");
            let column_default: Option<String> = row.get("column_default");
            let char_max_len: Option<i32> = row.get("character_maximum_length");
            let num_precision: Option<i32> = row.get("numeric_precision");
            let num_scale: Option<i32> = row.get("numeric_scale");
            let udt_name: String = row.get("udt_name");

            let sql_type =
                normalize_pg_type(&data_type, &udt_name, char_max_len, num_precision, num_scale);

            if let Some(table) = tables.get_mut(&table_name) {
                table.columns.insert(
                    col_name.clone(),
                    ColumnSnapshot {
                        name: col_name,
                        data_type: sql_type,
                        nullable: is_nullable == "YES",
                        default: column_default,
                        is_unique: false, // populated below from constraints
                    },
                );
            }
        }

        // 3. Get primary keys
        let pk_rows = sqlx::query(
            "SELECT tc.table_name, kcu.column_name \
             FROM information_schema.table_constraints tc \
             JOIN information_schema.key_column_usage kcu \
               ON tc.constraint_name = kcu.constraint_name \
               AND tc.table_schema = kcu.table_schema \
             WHERE tc.constraint_type = 'PRIMARY KEY' AND tc.table_schema = $1",
        )
        .bind(schema)
        .fetch_all(pool)
        .await
        .context("Failed to query primary keys")?;

        for row in &pk_rows {
            let table_name: String = row.get("table_name");
            let col_name: String = row.get("column_name");
            if let Some(table) = tables.get_mut(&table_name) {
                table.primary_key = Some(col_name);
            }
        }

        // 4. Get unique constraints (mark columns)
        let unique_rows = sqlx::query(
            "SELECT tc.table_name, kcu.column_name \
             FROM information_schema.table_constraints tc \
             JOIN information_schema.key_column_usage kcu \
               ON tc.constraint_name = kcu.constraint_name \
               AND tc.table_schema = kcu.table_schema \
             WHERE tc.constraint_type = 'UNIQUE' AND tc.table_schema = $1",
        )
        .bind(schema)
        .fetch_all(pool)
        .await
        .context("Failed to query unique constraints")?;

        for row in &unique_rows {
            let table_name: String = row.get("table_name");
            let col_name: String = row.get("column_name");
            if let Some(table) = tables.get_mut(&table_name) {
                if let Some(col) = table.columns.get_mut(&col_name) {
                    col.is_unique = true;
                }
            }
        }

        // 5. Get indexes
        let index_rows = sqlx::query(
            "SELECT tablename, indexname, indexdef \
             FROM pg_indexes \
             WHERE schemaname = $1 \
             ORDER BY tablename, indexname",
        )
        .bind(schema)
        .fetch_all(pool)
        .await
        .context("Failed to query indexes")?;

        for row in &index_rows {
            let table_name: String = row.get("tablename");
            let index_name: String = row.get("indexname");
            let index_def: String = row.get("indexdef");

            // Skip primary key indexes (already tracked)
            if index_name.ends_with("_pkey") {
                continue;
            }

            let unique = index_def.contains("UNIQUE INDEX");
            let columns = parse_index_columns(&index_def);
            let index_type = if index_def.contains("USING gin") {
                "gin".to_string()
            } else if index_def.contains("USING gist") {
                "gist".to_string()
            } else {
                "btree".to_string()
            };

            if let Some(table) = tables.get_mut(&table_name) {
                table.indexes.insert(
                    index_name.clone(),
                    IndexSnapshot {
                        name: index_name,
                        columns,
                        unique,
                        index_type,
                    },
                );
            }
        }

        Ok(tables)
    }

    async fn introspect_enums(
        &self,
        pool: &PgPool,
        schema: &str,
    ) -> Result<IndexMap<String, EnumSnapshot>> {
        let mut enums: IndexMap<String, EnumSnapshot> = IndexMap::new();

        let rows = sqlx::query(
            "SELECT t.typname, e.enumlabel \
             FROM pg_type t \
             JOIN pg_enum e ON t.oid = e.enumtypid \
             JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace \
             WHERE n.nspname = $1 \
             ORDER BY t.typname, e.enumsortorder",
        )
        .bind(schema)
        .fetch_all(pool)
        .await
        .context("Failed to query enums")?;

        for row in &rows {
            let type_name: String = row.get("typname");
            let label: String = row.get("enumlabel");

            enums
                .entry(type_name.clone())
                .or_insert_with(|| EnumSnapshot {
                    name: type_name,
                    variants: Vec::new(),
                })
                .variants
                .push(label);
        }

        Ok(enums)
    }
}

/// Normalize a PostgreSQL `information_schema` type into the framework's SQL type format.
///
/// This is critical for accurate diffing — the YAML-derived snapshot uses types like
/// `VARCHAR(255)` and `TIMESTAMPTZ`, while PostgreSQL reports `character varying` and
/// `timestamp with time zone`.
pub fn normalize_pg_type(
    data_type: &str,
    udt_name: &str,
    char_max_len: Option<i32>,
    num_precision: Option<i32>,
    num_scale: Option<i32>,
) -> String {
    match data_type {
        "character varying" => {
            if let Some(len) = char_max_len {
                format!("VARCHAR({})", len)
            } else {
                "VARCHAR(255)".to_string()
            }
        }
        "character" => {
            if let Some(len) = char_max_len {
                format!("CHAR({})", len)
            } else {
                "CHAR(1)".to_string()
            }
        }
        "text" => "TEXT".to_string(),
        "integer" => "INTEGER".to_string(),
        "smallint" => "SMALLINT".to_string(),
        "bigint" => "BIGINT".to_string(),
        "real" => "REAL".to_string(),
        "double precision" => "DOUBLE PRECISION".to_string(),
        "boolean" => "BOOLEAN".to_string(),
        "uuid" => "UUID".to_string(),
        "jsonb" => "JSONB".to_string(),
        "json" => "JSON".to_string(),
        "bytea" => "BYTEA".to_string(),
        "inet" => "INET".to_string(),
        "macaddr" => "MACADDR".to_string(),
        "date" => "DATE".to_string(),
        "interval" => "INTERVAL".to_string(),
        "numeric" => {
            match (num_precision, num_scale) {
                (Some(p), Some(s)) if s > 0 => format!("DECIMAL({}, {})", p, s),
                (Some(p), _) => format!("DECIMAL({})", p),
                _ => "DECIMAL".to_string(),
            }
        }
        "timestamp with time zone" => "TIMESTAMPTZ".to_string(),
        "timestamp without time zone" => "TIMESTAMP".to_string(),
        "time with time zone" => "TIMETZ".to_string(),
        "time without time zone" => "TIME".to_string(),
        "ARRAY" => {
            // Array types: udt_name starts with '_' (e.g., _varchar, _int4)
            let element_type = normalize_array_element(udt_name);
            format!("{}[]", element_type)
        }
        "USER-DEFINED" => {
            // Custom enum types — use the udt_name directly (uppercase)
            udt_name.to_uppercase()
        }
        _ => data_type.to_uppercase(),
    }
}

/// Normalize a PostgreSQL array element type from its udt_name (prefixed with `_`).
fn normalize_array_element(udt_name: &str) -> String {
    let base = udt_name.strip_prefix('_').unwrap_or(udt_name);
    match base {
        "varchar" => "VARCHAR(255)".to_string(),
        "text" => "TEXT".to_string(),
        "int4" | "int" => "INTEGER".to_string(),
        "int8" | "bigint" => "BIGINT".to_string(),
        "int2" | "smallint" => "SMALLINT".to_string(),
        "float4" | "real" => "REAL".to_string(),
        "float8" => "DOUBLE PRECISION".to_string(),
        "bool" => "BOOLEAN".to_string(),
        "uuid" => "UUID".to_string(),
        "jsonb" => "JSONB".to_string(),
        "timestamptz" => "TIMESTAMPTZ".to_string(),
        _ => base.to_uppercase(),
    }
}

/// Parse column names from a `CREATE INDEX` definition string.
///
/// Example: `CREATE INDEX idx_users_email ON public.users USING btree (email)`
/// Returns: `["email"]`
fn parse_index_columns(index_def: &str) -> Vec<String> {
    let Some(start) = index_def.rfind('(') else {
        return Vec::new();
    };
    let Some(end) = index_def.rfind(')') else {
        return Vec::new();
    };
    if start >= end {
        return Vec::new();
    }

    index_def[start + 1..end]
        .split(',')
        .map(|s| s.trim().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_varchar() {
        assert_eq!(
            normalize_pg_type("character varying", "varchar", Some(255), None, None),
            "VARCHAR(255)"
        );
        assert_eq!(
            normalize_pg_type("character varying", "varchar", Some(50), None, None),
            "VARCHAR(50)"
        );
        assert_eq!(
            normalize_pg_type("character varying", "varchar", None, None, None),
            "VARCHAR(255)"
        );
    }

    #[test]
    fn test_normalize_integer_types() {
        assert_eq!(
            normalize_pg_type("integer", "int4", None, None, None),
            "INTEGER"
        );
        assert_eq!(
            normalize_pg_type("bigint", "int8", None, None, None),
            "BIGINT"
        );
        assert_eq!(
            normalize_pg_type("smallint", "int2", None, None, None),
            "SMALLINT"
        );
    }

    #[test]
    fn test_normalize_timestamp() {
        assert_eq!(
            normalize_pg_type("timestamp with time zone", "timestamptz", None, None, None),
            "TIMESTAMPTZ"
        );
        assert_eq!(
            normalize_pg_type("timestamp without time zone", "timestamp", None, None, None),
            "TIMESTAMP"
        );
    }

    #[test]
    fn test_normalize_numeric() {
        assert_eq!(
            normalize_pg_type("numeric", "numeric", None, Some(19), Some(4)),
            "DECIMAL(19, 4)"
        );
        assert_eq!(
            normalize_pg_type("numeric", "numeric", None, Some(5), Some(2)),
            "DECIMAL(5, 2)"
        );
        assert_eq!(
            normalize_pg_type("numeric", "numeric", None, None, None),
            "DECIMAL"
        );
    }

    #[test]
    fn test_normalize_boolean() {
        assert_eq!(
            normalize_pg_type("boolean", "bool", None, None, None),
            "BOOLEAN"
        );
    }

    #[test]
    fn test_normalize_json() {
        assert_eq!(
            normalize_pg_type("jsonb", "jsonb", None, None, None),
            "JSONB"
        );
    }

    #[test]
    fn test_normalize_uuid() {
        assert_eq!(
            normalize_pg_type("uuid", "uuid", None, None, None),
            "UUID"
        );
    }

    #[test]
    fn test_normalize_user_defined_enum() {
        assert_eq!(
            normalize_pg_type("USER-DEFINED", "order_status", None, None, None),
            "ORDER_STATUS"
        );
    }

    #[test]
    fn test_normalize_array() {
        assert_eq!(
            normalize_pg_type("ARRAY", "_varchar", None, None, None),
            "VARCHAR(255)[]"
        );
        assert_eq!(
            normalize_pg_type("ARRAY", "_int4", None, None, None),
            "INTEGER[]"
        );
        assert_eq!(
            normalize_pg_type("ARRAY", "_uuid", None, None, None),
            "UUID[]"
        );
    }

    #[test]
    fn test_parse_index_columns_single() {
        let def = "CREATE INDEX idx_users_email ON public.users USING btree (email)";
        assert_eq!(parse_index_columns(def), vec!["email"]);
    }

    #[test]
    fn test_parse_index_columns_multi() {
        let def =
            "CREATE INDEX idx_orders_user_date ON public.orders USING btree (user_id, created_at)";
        assert_eq!(
            parse_index_columns(def),
            vec!["user_id", "created_at"]
        );
    }

    #[test]
    fn test_parse_index_columns_unique() {
        let def = "CREATE UNIQUE INDEX idx_users_email ON public.users USING btree (email)";
        assert_eq!(parse_index_columns(def), vec!["email"]);
    }
}
