//! GraphQL resolver generator
//!
//! Generates async-graphql service implementations for the standard Backbone CRUD operations.
//! Produces per-entity resolver files, a module index, and a server composition file.

use super::{GenerateError, GeneratedOutput, Generator};
use crate::ast::{Field, Model, PrimitiveType, TypeRef};
use crate::resolver::ResolvedSchema;
use crate::utils::{escape_rust_keyword, pluralize, to_pascal_case, to_snake_case};
use std::collections::HashMap;
use std::fmt::Write;
use std::path::PathBuf;

/// Generates GraphQL resolvers using async-graphql
pub struct GraphqlGenerator;

impl GraphqlGenerator {
    pub fn new() -> Self {
        Self
    }

    /// Check if a field is a system-managed field (excluded from inputs)
    fn is_system_field(name: &str) -> bool {
        matches!(
            name,
            "id" | "created_at" | "updated_at" | "deleted_at" | "metadata"
        )
    }

    /// Check if a field is sensitive and should be excluded from GraphQL output.
    /// Fields with `@sensitive`, `@hashed`, or `@encrypted` attributes contain secrets
    /// (password hashes, API keys, tokens) that must never be exposed via the API.
    /// Also catches common sensitive field names as a safety net.
    fn is_sensitive_field(field: &Field) -> bool {
        // Attribute-based detection
        field.has_attribute("sensitive")
            || field.has_attribute("hashed")
            || field.has_attribute("encrypted")
            // Name-based safety net for fields that should always be hidden
            || field.name.ends_with("_hash")
            || field.name == "password"
            || field.name == "secret"
            || field.name.ends_with("_secret")
            || field.name == "access_token"
            || field.name == "refresh_token"
            || field.name == "api_key"
            || field.name == "secret_key"
            || field.name.ends_with("_password")
    }

    /// Map a PrimitiveType to its GraphQL-compatible Rust type representation
    fn primitive_to_graphql(p: &PrimitiveType) -> &'static str {
        match p {
            // String-like types
            PrimitiveType::String
            | PrimitiveType::Email
            | PrimitiveType::Url
            | PrimitiveType::Phone
            | PrimitiveType::Slug
            | PrimitiveType::Ip
            | PrimitiveType::IpV4
            | PrimitiveType::IpV6
            | PrimitiveType::Mac
            | PrimitiveType::Markdown
            | PrimitiveType::Html => "String",

            // UUID/DateTime/Decimal serialized as String for GraphQL v1
            PrimitiveType::Uuid => "String",
            PrimitiveType::DateTime | PrimitiveType::Timestamp => "String",
            PrimitiveType::Date => "String",
            PrimitiveType::Time => "String",
            PrimitiveType::Duration => "String",
            PrimitiveType::Decimal | PrimitiveType::Money | PrimitiveType::Percentage => "String",

            // Numeric types
            PrimitiveType::Int | PrimitiveType::Int32 => "i32",
            PrimitiveType::Int64 => "i64",
            PrimitiveType::Float | PrimitiveType::Float64 => "f64",
            PrimitiveType::Float32 => "f32",

            // Boolean
            PrimitiveType::Bool => "bool",

            // JSON
            PrimitiveType::Json => "async_graphql::Json<serde_json::Value>",

            // Binary types as base64 string
            PrimitiveType::Bytes | PrimitiveType::Binary | PrimitiveType::Base64 => "String",
        }
    }

    /// Map a TypeRef to its GraphQL output type string
    fn type_ref_to_graphql(type_ref: &TypeRef) -> String {
        match type_ref {
            TypeRef::Primitive(p) => Self::primitive_to_graphql(p).to_string(),
            TypeRef::Custom(_) => "String".to_string(), // Enums as string in v1
            TypeRef::Array(inner) => format!("Vec<{}>", Self::type_ref_to_graphql(inner)),
            TypeRef::Optional(inner) => format!("Option<{}>", Self::type_ref_to_graphql(inner)),
            TypeRef::Map { .. } => "async_graphql::Json<serde_json::Value>".to_string(),
            TypeRef::ModuleRef { .. } => "String".to_string(), // Cross-module refs as ID string
        }
    }

    /// Map a TypeRef to its GraphQL input type (same as output for v1)
    fn type_ref_to_graphql_input(type_ref: &TypeRef) -> String {
        Self::type_ref_to_graphql(type_ref)
    }

    /// Generate conversion expression from entity field to GraphQL field
    fn field_to_graphql_expr(field: &Field) -> String {
        let name = escape_rust_keyword(&field.name);

        // Special handling for @audit_metadata fields: entity type is AuditMetadata, not serde_json::Value
        if field.has_attribute("audit_metadata") {
            if field.type_ref.is_optional() {
                return format!(
                    "{}: e.{}.as_ref().map(|v| async_graphql::Json(serde_json::to_value(v).unwrap_or_else(|e| {{ eprintln!(\"WARN: JSON serialization failed: {{}}\", e); serde_json::Value::Null }})))",
                    name, name
                );
            } else {
                return format!(
                    "{}: async_graphql::Json(serde_json::to_value(&e.{}).unwrap_or_else(|e| {{ eprintln!(\"WARN: JSON serialization failed: {{}}\", e); serde_json::Value::Null }}))",
                    name, name
                );
            }
        }

        // Handle Array types: need to .iter().map() each element
        if let TypeRef::Array(inner) = &field.type_ref {
            let base_prim = Self::get_base_primitive(inner);
            let needs_conversion = matches!(
                base_prim,
                Some(PrimitiveType::Uuid)
                    | Some(PrimitiveType::DateTime)
                    | Some(PrimitiveType::Timestamp)
                    | Some(PrimitiveType::Date)
                    | Some(PrimitiveType::Time)
                    | Some(PrimitiveType::Duration)
                    | Some(PrimitiveType::Decimal)
                    | Some(PrimitiveType::Money)
                    | Some(PrimitiveType::Percentage)
            ) || matches!(inner.as_ref(), TypeRef::Custom(_));
            if needs_conversion {
                return format!(
                    "{}: e.{}.iter().map(|v| v.to_string()).collect()",
                    name, name
                );
            }
            // For primitive-compatible arrays (String, int, bool), direct copy
            return format!("{}: e.{}.clone()", name, name);
        }
        // Handle Optional(Array(...))
        if let TypeRef::Optional(inner) = &field.type_ref {
            if let TypeRef::Array(arr_inner) = inner.as_ref() {
                let base_prim = Self::get_base_primitive(arr_inner);
                let needs_conversion = matches!(
                    base_prim,
                    Some(PrimitiveType::Uuid)
                        | Some(PrimitiveType::DateTime)
                        | Some(PrimitiveType::Timestamp)
                        | Some(PrimitiveType::Date)
                        | Some(PrimitiveType::Time)
                        | Some(PrimitiveType::Duration)
                        | Some(PrimitiveType::Decimal)
                        | Some(PrimitiveType::Money)
                        | Some(PrimitiveType::Percentage)
                ) || matches!(arr_inner.as_ref(), TypeRef::Custom(_));
                if needs_conversion {
                    return format!(
                        "{}: e.{}.as_ref().map(|arr| arr.iter().map(|v| v.to_string()).collect())",
                        name, name
                    );
                }
                return format!("{}: e.{}.clone()", name, name);
            }
        }

        // Check the base type (unwrap Optional/Array)
        let base_type = Self::get_base_primitive(&field.type_ref);

        match base_type {
            // Types that need .to_string() conversion
            Some(PrimitiveType::Uuid) => {
                if field.type_ref.is_optional() {
                    format!("{}: e.{}.map(|v| v.to_string())", name, name)
                } else {
                    format!("{}: e.{}.to_string()", name, name)
                }
            }
            Some(PrimitiveType::DateTime) | Some(PrimitiveType::Timestamp) => {
                if field.type_ref.is_optional() {
                    format!(
                        "{}: e.{}.map(|v| v.to_rfc3339())",
                        name, name
                    )
                } else {
                    format!("{}: e.{}.to_rfc3339()", name, name)
                }
            }
            Some(PrimitiveType::Date) => {
                if field.type_ref.is_optional() {
                    format!("{}: e.{}.map(|v| v.to_string())", name, name)
                } else {
                    format!("{}: e.{}.to_string()", name, name)
                }
            }
            Some(PrimitiveType::Time) | Some(PrimitiveType::Duration) => {
                if field.type_ref.is_optional() {
                    format!("{}: e.{}.map(|v| v.to_string())", name, name)
                } else {
                    format!("{}: e.{}.to_string()", name, name)
                }
            }
            Some(PrimitiveType::Decimal) | Some(PrimitiveType::Money) | Some(PrimitiveType::Percentage) => {
                if field.type_ref.is_optional() {
                    format!("{}: e.{}.map(|v| v.to_string())", name, name)
                } else {
                    format!("{}: e.{}.to_string()", name, name)
                }
            }
            Some(PrimitiveType::Json) => {
                if field.type_ref.is_optional() {
                    format!(
                        "{}: e.{}.map(|v| async_graphql::Json(v))",
                        name, name
                    )
                } else {
                    format!("{}: async_graphql::Json(e.{})", name, name)
                }
            }
            // Custom types (enums) → .to_string()
            None if matches!(field.type_ref, TypeRef::Custom(_)) => {
                format!("{}: e.{}.to_string()", name, name)
            }
            None if matches!(field.type_ref, TypeRef::Optional(ref inner) if matches!(**inner, TypeRef::Custom(_))) => {
                format!("{}: e.{}.map(|v| v.to_string())", name, name)
            }
            // Map types → Json wrapper
            None if matches!(field.type_ref, TypeRef::Map { .. }) => {
                format!(
                    "{}: async_graphql::Json(serde_json::to_value(&e.{}).unwrap_or_else(|e| {{ eprintln!(\"WARN: JSON serialization failed: {{}}\", e); serde_json::Value::Null }}))",
                    name, name
                )
            }
            None if matches!(field.type_ref, TypeRef::Optional(ref inner) if matches!(**inner, TypeRef::Map { .. })) => {
                format!(
                    "{}: e.{}.as_ref().map(|v| async_graphql::Json(serde_json::to_value(v).unwrap_or_else(|e| {{ eprintln!(\"WARN: JSON serialization failed: {{}}\", e); serde_json::Value::Null }})))",
                    name, name
                )
            }
            // Direct copy for String, int, float, bool
            _ => format!("{}: e.{}", name, name),
        }
    }

    /// Get the base primitive type, unwrapping Optional and Array
    fn get_base_primitive(type_ref: &TypeRef) -> Option<PrimitiveType> {
        match type_ref {
            TypeRef::Primitive(p) => Some(*p),
            TypeRef::Optional(inner) | TypeRef::Array(inner) => Self::get_base_primitive(inner),
            _ => None,
        }
    }

    /// Get the Rust parse target type for a primitive that needs string→type conversion.
    /// Returns None for types where GraphQL input matches entity type (String, int, bool, etc.)
    fn parse_target_type(p: &PrimitiveType) -> Option<&'static str> {
        match p {
            PrimitiveType::Uuid => Some("uuid::Uuid"),
            PrimitiveType::DateTime | PrimitiveType::Timestamp => Some("chrono::DateTime<chrono::Utc>"),
            PrimitiveType::Date => Some("chrono::NaiveDate"),
            PrimitiveType::Time => Some("chrono::NaiveTime"),
            PrimitiveType::Decimal | PrimitiveType::Money | PrimitiveType::Percentage => Some("rust_decimal::Decimal"),
            _ => None,
        }
    }

    /// Generate conversion expression from a variable holding the GraphQL input type
    /// to the entity's Rust type. Returns (expression, is_fallible).
    fn input_to_entity_expr(var: &str, type_ref: &TypeRef, enum_map: &HashMap<String, String>) -> (String, bool) {
        match type_ref {
            TypeRef::Primitive(p) => {
                if let Some(target) = Self::parse_target_type(p) {
                    (format!("{}.parse::<{}>()", var, target), true)
                } else if matches!(p, PrimitiveType::Json) {
                    (format!("{}.0", var), false)
                } else if matches!(p, PrimitiveType::Bytes | PrimitiveType::Binary | PrimitiveType::Base64) {
                    (format!("{}.into_bytes()", var), false)
                } else if matches!(p, PrimitiveType::Duration) {
                    (format!("chrono::Duration::seconds({}.parse::<i64>().unwrap_or(0))", var), false)
                } else {
                    (var.to_string(), false)
                }
            }
            TypeRef::Custom(name) => {
                let resolved = Self::resolve_custom_type(name, enum_map);
                (format!("{}.parse::<{}>()", var, resolved), true)
            }
            TypeRef::Optional(inner) => {
                let (inner_expr, fallible) = Self::input_to_entity_expr("v", inner, enum_map);
                // If inner conversion is identity (var == "v"), skip the .map()
                if inner_expr == "v" {
                    (var.to_string(), false)
                } else if fallible {
                    (format!("{}.map(|v| {}).transpose()", var, inner_expr), true)
                } else {
                    (format!("{}.map(|v| {})", var, inner_expr), false)
                }
            }
            TypeRef::Array(inner) => {
                let (inner_expr, fallible) = Self::input_to_entity_expr("v", inner, enum_map);
                if fallible {
                    (format!("{}.into_iter().map(|v| {}).collect::<Result<Vec<_>, _>>()", var, inner_expr), true)
                } else {
                    (format!("{}.into_iter().map(|v| {}).collect()", var, inner_expr), false)
                }
            }
            TypeRef::Map { .. } => {
                (format!("serde_json::from_value({}.0).unwrap_or_else(|e| {{ eprintln!(\"WARN: JSON deserialization failed: {{}}\", e); Default::default() }})", var), false)
            }
            TypeRef::ModuleRef { .. } => {
                (var.to_string(), false)
            }
        }
    }

    /// Check if the service's find_by_id method accepts a typed ID (not &str).
    /// This handles the case where some modules' services were generated before typed IDs were added.
    fn service_uses_typed_id(module_name: &str, model_name: &str) -> bool {
        let snake = to_snake_case(model_name);
        let service_path = format!(
            "libs/modules/{}/src/application/service/{}_service.rs",
            module_name, snake
        );
        std::fs::read_to_string(&service_path)
            .map(|content| content.contains(&format!("id: {}Id", model_name)))
            .unwrap_or(false)
    }

    /// Build a mapping from schema Custom type names to actual Rust type names
    /// by reading the entity file on disk. This handles cases where the schema
    /// was renamed but entities weren't regenerated.
    fn build_enum_type_map(module_name: &str, model: &Model) -> HashMap<String, String> {
        let mut map = HashMap::new();
        let snake = to_snake_case(&model.name);
        let entity_path = format!(
            "libs/modules/{}/src/domain/entity/{}.rs",
            module_name, snake
        );
        let content = match std::fs::read_to_string(&entity_path) {
            Ok(c) => c,
            Err(_) => return map,
        };

        // For each field with a Custom type, check what the entity file actually uses
        for field in &model.fields {
            let custom_name = match &field.type_ref {
                TypeRef::Custom(name) => Some(name.clone()),
                TypeRef::Optional(inner) => match inner.as_ref() {
                    TypeRef::Custom(name) => Some(name.clone()),
                    _ => None,
                },
                TypeRef::Array(inner) => match inner.as_ref() {
                    TypeRef::Custom(name) => Some(name.clone()),
                    _ => None,
                },
                _ => None,
            };
            if let Some(schema_type) = custom_name {
                // Look for "pub {field_name}: {ActualType}" in the entity file
                let field_name = &field.name;
                for line in content.lines() {
                    let trimmed = line.trim();
                    if let Some(rest) = trimmed.strip_prefix(&format!("pub {}: ", field_name)) {
                        // Extract the type (strip trailing comma and whitespace)
                        let type_str = rest.trim_end_matches(',').trim();
                        // Extract the base type from Optional/Array wrappers
                        let base = type_str
                            .strip_prefix("Option<").and_then(|s| s.strip_suffix('>'))
                            .or_else(|| type_str.strip_prefix("Vec<").and_then(|s| s.strip_suffix('>')))
                            .unwrap_or(type_str);
                        let pascal_schema = to_pascal_case(&schema_type);
                        if base != pascal_schema {
                            map.insert(schema_type, base.to_string());
                        }
                        break;
                    }
                }
            }
        }
        map
    }

    /// Resolve a Custom type name to the actual Rust type, using the enum map for mismatches.
    fn resolve_custom_type(name: &str, enum_map: &HashMap<String, String>) -> String {
        if let Some(actual) = enum_map.get(name) {
            actual.clone()
        } else {
            to_pascal_case(name)
        }
    }

    /// Generate per-entity GraphQL file
    fn generate_entity_graphql(
        &self,
        model: &Model,
        schema: &ResolvedSchema,
    ) -> Result<String, GenerateError> {
        let mut output = String::new();
        let model_name = &model.name;
        let model_snake = to_snake_case(model_name);
        let model_plural = pluralize(&model_snake);
        let module_name = &schema.schema.name;
        let pascal_module = to_pascal_case(module_name);
        let module_snake = to_snake_case(module_name);
        let use_typed_id = Self::service_uses_typed_id(module_name, model_name);
        let enum_map = Self::build_enum_type_map(module_name, model);

        // Header
        writeln!(output, "//! {} GraphQL resolvers", model_name).unwrap();
        writeln!(output, "//!").unwrap();
        writeln!(output, "//! Auto-generated by metaphor-schema. Do not edit manually.").unwrap();
        writeln!(output).unwrap();

        // Imports
        writeln!(output, "use std::sync::Arc;").unwrap();
        writeln!(output, "use async_graphql::*;").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "use crate::application::service::{}Service;", model_name).unwrap();
        writeln!(output, "use crate::domain::entity::*;").unwrap();
        writeln!(output).unwrap();

        // ============================================================
        // GraphQL Output Type
        // ============================================================
        writeln!(output, "// =================================================================").unwrap();
        writeln!(output, "// GraphQL Output Type").unwrap();
        writeln!(output, "// =================================================================").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "/// {} GraphQL output type", model_name).unwrap();
        writeln!(output, "#[derive(Debug, Clone, SimpleObject)]").unwrap();
        writeln!(output, "#[graphql(name = \"{}{}\")]", pascal_module, model_name).unwrap();
        writeln!(output, "pub struct {}Gql {{", model_name).unwrap();

        for field in &model.fields {
            // Exclude sensitive fields (password hashes, secrets, tokens) from output
            if Self::is_sensitive_field(field) {
                continue;
            }
            let gql_type = Self::type_ref_to_graphql(&field.type_ref);
            let field_name = escape_rust_keyword(&field.name);
            writeln!(output, "    pub {}: {},", field_name, gql_type).unwrap();
        }

        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // From<Entity> for EntityGql
        writeln!(output, "impl From<{}> for {}Gql {{", model_name, model_name).unwrap();
        writeln!(output, "    fn from(e: {}) -> Self {{", model_name).unwrap();
        writeln!(output, "        Self {{").unwrap();

        for field in &model.fields {
            if Self::is_sensitive_field(field) {
                continue;
            }
            let expr = Self::field_to_graphql_expr(field);
            writeln!(output, "            {},", expr).unwrap();
        }

        writeln!(output, "        }}").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // ============================================================
        // Paginated Response Type
        // ============================================================
        writeln!(output, "// =================================================================").unwrap();
        writeln!(output, "// Paginated Response").unwrap();
        writeln!(output, "// =================================================================").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "#[derive(Debug, SimpleObject)]").unwrap();
        writeln!(output, "#[graphql(name = \"{}{}PaginatedResponse\")]", pascal_module, model_name).unwrap();
        writeln!(output, "pub struct {}PaginatedResponse {{", model_name).unwrap();
        writeln!(output, "    pub data: Vec<{}Gql>,", model_name).unwrap();
        writeln!(output, "    pub total: i64,").unwrap();
        writeln!(output, "    pub page: i32,").unwrap();
        writeln!(output, "    pub limit: i32,").unwrap();
        writeln!(output, "    pub total_pages: i32,").unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // ============================================================
        // Input Types
        // ============================================================
        writeln!(output, "// =================================================================").unwrap();
        writeln!(output, "// Input Types").unwrap();
        writeln!(output, "// =================================================================").unwrap();
        writeln!(output).unwrap();

        // Create input (non-system fields, respect required/optional)
        writeln!(output, "/// Input for creating a {}", model_name).unwrap();
        writeln!(output, "#[derive(Debug, InputObject)]").unwrap();
        writeln!(output, "#[graphql(name = \"{}Create{}Input\")]", pascal_module, model_name).unwrap();
        writeln!(output, "pub struct Create{}Input {{", model_name).unwrap();

        for field in &model.fields {
            if Self::is_system_field(&field.name) {
                continue;
            }
            let gql_type = Self::type_ref_to_graphql_input(&field.type_ref);
            let field_name = escape_rust_keyword(&field.name);
            // If the field is not optional in the schema, keep it required in create input
            writeln!(output, "    pub {}: {},", field_name, gql_type).unwrap();
        }

        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Update input (all fields wrapped in Option for partial update)
        writeln!(output, "/// Input for updating a {}", model_name).unwrap();
        writeln!(output, "///").unwrap();
        writeln!(output, "/// All fields are optional. Only provided fields will be updated.").unwrap();
        writeln!(output, "#[derive(Debug, InputObject)]").unwrap();
        writeln!(output, "#[graphql(name = \"{}Update{}Input\")]", pascal_module, model_name).unwrap();
        writeln!(output, "pub struct Update{}Input {{", model_name).unwrap();

        for field in &model.fields {
            if Self::is_system_field(&field.name) {
                continue;
            }
            let gql_type = Self::type_ref_to_graphql_input(&field.type_ref);
            let field_name = escape_rust_keyword(&field.name);
            // Always wrap in Option so we can distinguish "not provided" from "set to value"
            writeln!(output, "    pub {}: Option<{}>,", field_name, gql_type).unwrap();
        }

        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // ============================================================
        // GraphQL Resolver — Phase 1: type alias over GenericGraphQLResolver
        // ============================================================
        writeln!(output, "// =================================================================").unwrap();
        writeln!(output, "// GraphQL Resolver (Phase 1 — type alias)").unwrap();
        writeln!(output, "// =================================================================").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "use backbone_core::GenericGraphQLResolver;").unwrap();
        writeln!(output, "use crate::presentation::dto::{{Create{n}Dto, Update{n}Dto}};",
            n = model_name).unwrap();
        writeln!(output, "use crate::application::service::{n}Service;", n = model_name).unwrap();
        writeln!(output).unwrap();

        writeln!(output, "/// GraphQL resolver for {} entities.", model_name).unwrap();
        writeln!(output, "///").unwrap();
        writeln!(output, "/// All standard CRUD queries and mutations are provided by `GenericGraphQLResolver`.").unwrap();
        writeln!(output, "/// Entity-specific computed fields and custom mutations go in the `// <<< CUSTOM` zone.").unwrap();
        writeln!(output, "pub type {n}GraphQLResolver = GenericGraphQLResolver<{n}, Create{n}Dto, Update{n}Dto, {n}Service>;",
            n = model_name).unwrap();
        writeln!(output).unwrap();

        // Legacy Query/Mutation stubs for server.rs composition — still needed for MergedObject.
        writeln!(output, "/// Query root for {} — delegates to GenericGraphQLResolver.", model_name).unwrap();
        writeln!(output, "#[derive(Default)]").unwrap();
        writeln!(output, "pub struct {}Query;", model_name).unwrap();
        writeln!(output).unwrap();

        writeln!(output, "#[Object(name = \"{}{}Query\")]", pascal_module, model_name).unwrap();
        writeln!(output, "impl {}Query {{", model_name).unwrap();

        // List query
        writeln!(output, "    /// List {} with pagination", model_plural).unwrap();
        writeln!(
            output,
            "    async fn {}_{}(",
            module_snake, model_plural
        ).unwrap();
        writeln!(output, "        &self,").unwrap();
        writeln!(output, "        ctx: &Context<'_>,").unwrap();
        writeln!(output, "        #[graphql(default = 1)] page: i32,").unwrap();
        writeln!(output, "        #[graphql(default = 20)] limit: i32,").unwrap();
        writeln!(output, "    ) -> Result<{}PaginatedResponse> {{", model_name).unwrap();
        writeln!(output, "        let service = ctx.data::<Arc<{}Service>>()?;", model_name).unwrap();
        writeln!(output, "        let p = page.max(1) as u32;").unwrap();
        writeln!(output, "        let l = limit.clamp(1, 100) as u32;").unwrap();
        writeln!(output, "        let (items, total) = service.list(p, l, Default::default()).await").unwrap();
        writeln!(output, "            .map_err(|e| Error::new(e.to_string()))?;").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "        let total_pages = if l == 0 {{ 0 }} else {{ ((total as f64) / (l as f64)).ceil() as i32 }};").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "        Ok({}PaginatedResponse {{", model_name).unwrap();
        writeln!(output, "            data: items.into_iter().map(Into::into).collect(),").unwrap();
        writeln!(output, "            total: total as i64,").unwrap();
        writeln!(output, "            page: p as i32,").unwrap();
        writeln!(output, "            limit: l as i32,").unwrap();
        writeln!(output, "            total_pages,").unwrap();
        writeln!(output, "        }})").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Get by ID query
        writeln!(output, "    /// Get {} by ID", model_snake).unwrap();
        writeln!(
            output,
            "    async fn {}_{}(&self, ctx: &Context<'_>, id: String) -> Result<Option<{}Gql>> {{",
            module_snake, model_snake, model_name
        ).unwrap();
        writeln!(output, "        let service = ctx.data::<Arc<{}Service>>()?;", model_name).unwrap();
        if use_typed_id {
            writeln!(output, "        let typed_id: {}Id = id.parse()", model_name).unwrap();
            writeln!(output, "            .map_err(|_| Error::new(format!(\"Invalid {} ID: {{}}\", id)))?;", model_name).unwrap();
            writeln!(output, "        let entity = service.find_by_id(typed_id).await").unwrap();
        } else {
            writeln!(output, "        let entity = service.find_by_id(&id).await").unwrap();
        }
        writeln!(output, "            .map_err(|e| Error::new(e.to_string()))?;").unwrap();
        writeln!(output, "        Ok(entity.map(Into::into))").unwrap();
        writeln!(output, "    }}").unwrap();

        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // ============================================================
        // Mutation Resolvers
        // ============================================================
        writeln!(output, "// =================================================================").unwrap();
        writeln!(output, "// Mutation Resolvers").unwrap();
        writeln!(output, "// =================================================================").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "#[derive(Default)]").unwrap();
        writeln!(output, "pub struct {}Mutation;", model_name).unwrap();
        writeln!(output).unwrap();

        writeln!(output, "#[Object(name = \"{}{}Mutation\")]", pascal_module, model_name).unwrap();
        writeln!(output, "impl {}Mutation {{", model_name).unwrap();

        // Create mutation
        writeln!(output, "    /// Create a new {}", model_snake).unwrap();
        writeln!(
            output,
            "    async fn {}_create_{}(",
            module_snake, model_snake
        ).unwrap();
        writeln!(output, "        &self,").unwrap();
        writeln!(output, "        ctx: &Context<'_>,").unwrap();
        writeln!(output, "        input: Create{}Input,", model_name).unwrap();
        writeln!(output, "    ) -> Result<{}Gql> {{", model_name).unwrap();
        writeln!(output, "        let service = ctx.data::<Arc<{}Service>>()?;", model_name).unwrap();
        writeln!(output).unwrap();
        writeln!(output, "        let entity = {} {{", model_name).unwrap();

        // Generate field assignments for entity construction
        for field in &model.fields {
            let field_name = escape_rust_keyword(&field.name);
            match field.name.as_str() {
                "id" => {
                    writeln!(output, "            id: uuid::Uuid::new_v4(),").unwrap();
                }
                "created_at" => {
                    if field.type_ref.is_optional() {
                        writeln!(output, "            created_at: Some(chrono::Utc::now()),").unwrap();
                    } else {
                        writeln!(output, "            created_at: chrono::Utc::now(),").unwrap();
                    }
                }
                "updated_at" => {
                    if field.type_ref.is_optional() {
                        writeln!(output, "            updated_at: Some(chrono::Utc::now()),").unwrap();
                    } else {
                        writeln!(output, "            updated_at: chrono::Utc::now(),").unwrap();
                    }
                }
                "deleted_at" => {
                    if field.type_ref.is_optional() {
                        writeln!(output, "            deleted_at: None,").unwrap();
                    } else {
                        writeln!(output, "            deleted_at: chrono::Utc::now(),").unwrap();
                    }
                }
                "metadata" => {
                    if field.type_ref.is_optional() {
                        writeln!(output, "            metadata: None,").unwrap();
                    } else {
                        writeln!(output, "            metadata: Default::default(),").unwrap();
                    }
                }
                _ => {
                    let (expr, fallible) = Self::input_to_entity_expr(
                        &format!("input.{}", field_name),
                        &field.type_ref,
                        &enum_map,
                    );
                    if fallible {
                        writeln!(
                            output,
                            "            {}: {}\n                .map_err(|e| Error::new(format!(\"Invalid {}: {{}}\", e)))?,",
                            field_name, expr, field.name
                        ).unwrap();
                    } else {
                        writeln!(output, "            {}: {},", field_name, expr).unwrap();
                    }
                }
            }
        }

        writeln!(output, "        }};").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "        let created = service.create(entity).await").unwrap();
        writeln!(output, "            .map_err(|e| Error::new(e.to_string()))?;").unwrap();
        writeln!(output, "        Ok(created.into())").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Update mutation
        writeln!(output, "    /// Update {} by ID", model_snake).unwrap();
        writeln!(
            output,
            "    async fn {}_update_{}(",
            module_snake, model_snake
        ).unwrap();
        writeln!(output, "        &self,").unwrap();
        writeln!(output, "        ctx: &Context<'_>,").unwrap();
        writeln!(output, "        id: String,").unwrap();
        writeln!(output, "        input: Update{}Input,", model_name).unwrap();
        writeln!(output, "    ) -> Result<{}Gql> {{", model_name).unwrap();
        writeln!(output, "        let service = ctx.data::<Arc<{}Service>>()?;", model_name).unwrap();
        if use_typed_id {
            writeln!(output, "        let typed_id: {}Id = id.parse()", model_name).unwrap();
            writeln!(output, "            .map_err(|_| Error::new(format!(\"Invalid {} ID: {{}}\", id)))?;", model_name).unwrap();
            writeln!(output, "        let mut entity = service.find_by_id(typed_id).await").unwrap();
        } else {
            writeln!(output, "        let mut entity = service.find_by_id(&id).await").unwrap();
        }
        writeln!(output, "            .map_err(|e| Error::new(e.to_string()))?").unwrap();
        writeln!(output, "            .ok_or_else(|| Error::new(format!(\"{} not found: {{}}\", id)))?;", model_name).unwrap();
        writeln!(output).unwrap();

        // Generate field update assignments
        for field in &model.fields {
            if Self::is_system_field(&field.name) {
                continue;
            }
            let field_name = escape_rust_keyword(&field.name);
            let (expr, fallible) = Self::input_to_entity_expr("val", &field.type_ref, &enum_map);

            if fallible {
                writeln!(
                    output,
                    "        if let Some(val) = input.{} {{ entity.{} = {}\n            .map_err(|e| Error::new(format!(\"Invalid {}: {{}}\", e)))?; }}",
                    field_name, field_name, expr, field.name
                ).unwrap();
            } else {
                writeln!(
                    output,
                    "        if let Some(val) = input.{} {{ entity.{} = {}; }}",
                    field_name, field_name, expr
                ).unwrap();
            }
        }

        writeln!(output).unwrap();
        if use_typed_id {
            writeln!(output, "        let updated = service.update(typed_id, entity).await").unwrap();
        } else {
            writeln!(output, "        let updated = service.update(&id, entity).await").unwrap();
        }
        writeln!(output, "            .map_err(|e| Error::new(e.to_string()))?").unwrap();
        writeln!(output, "            .ok_or_else(|| Error::new(format!(\"{} not found: {{}}\", id)))?;", model_name).unwrap();
        writeln!(output, "        Ok(updated.into())").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Delete mutation
        writeln!(output, "    /// Delete {} (soft delete)", model_snake).unwrap();
        writeln!(
            output,
            "    async fn {}_delete_{}(&self, ctx: &Context<'_>, id: String) -> Result<bool> {{",
            module_snake, model_snake
        ).unwrap();
        writeln!(output, "        let service = ctx.data::<Arc<{}Service>>()?;", model_name).unwrap();
        if use_typed_id {
            writeln!(output, "        let typed_id: {}Id = id.parse()", model_name).unwrap();
            writeln!(output, "            .map_err(|_| Error::new(format!(\"Invalid {} ID: {{}}\", id)))?;", model_name).unwrap();
            writeln!(output, "        service.delete(typed_id).await").unwrap();
        } else {
            writeln!(output, "        service.delete(&id).await").unwrap();
        }
        writeln!(output, "            .map_err(|e| Error::new(e.to_string()))").unwrap();
        writeln!(output, "    }}").unwrap();

        writeln!(output, "}}").unwrap();

        Ok(output)
    }

    /// Generate mod.rs for the graphql presentation layer
    fn generate_mod_rs(&self, schema: &ResolvedSchema) -> String {
        let mut output = String::new();

        writeln!(output, "//! GraphQL resolvers").unwrap();
        writeln!(output, "//!").unwrap();
        writeln!(output, "//! Auto-generated by metaphor-schema. Do not edit manually.").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "pub mod server;").unwrap();
        writeln!(output).unwrap();

        // Module declarations
        for model in &schema.schema.models {
            let model_snake = to_snake_case(&model.name);
            writeln!(output, "pub mod {}_graphql;", model_snake).unwrap();
        }
        writeln!(output).unwrap();

        // Re-exports
        for model in &schema.schema.models {
            let model_snake = to_snake_case(&model.name);
            writeln!(
                output,
                "pub use {}_graphql::{{{}Query, {}Mutation, {}Gql}};",
                model_snake, model.name, model.name, model.name
            )
            .unwrap();
        }
        writeln!(output).unwrap();

        // Re-export module-level composed types from server
        let pascal_name = to_pascal_case(&schema.schema.name);
        let has_name_conflict = schema.schema.models.iter().any(|m| m.name == pascal_name);
        let root_suffix = if has_name_conflict { "Schema" } else { "" };
        writeln!(
            output,
            "pub use server::{{{}{}Query, {}{}Mutation}};",
            pascal_name, root_suffix, pascal_name, root_suffix
        )
        .unwrap();

        output
    }

    /// Maximum entities per MergedObject group before splitting.
    /// async-graphql's MergedObject creates nested pairs which overflows the compiler
    /// recursion limit when there are too many members.
    const GROUP_SIZE: usize = 16;

    /// Generate server.rs for module-level schema composition
    fn generate_server_rs(&self, schema: &ResolvedSchema) -> Result<String, GenerateError> {
        let mut output = String::new();
        let module_name = &schema.schema.name;
        let pascal_name = to_pascal_case(module_name);
        let models = &schema.schema.models;
        let needs_groups = models.len() > Self::GROUP_SIZE;

        // Detect name collision: if any model has same PascalCase name as the module,
        // we suffix the module-level type names with "Schema" to avoid conflict.
        let has_name_conflict = models.iter().any(|m| m.name == pascal_name);
        let root_suffix = if has_name_conflict { "Schema" } else { "" };
        let query_root = format!("{}{}Query", pascal_name, root_suffix);
        let mutation_root = format!("{}{}Mutation", pascal_name, root_suffix);

        // Header
        writeln!(output, "//! GraphQL schema composition for {} module", pascal_name).unwrap();
        writeln!(output, "//!").unwrap();
        writeln!(output, "//! Generated by metaphor-schema. Do not edit manually.").unwrap();
        writeln!(output, "//!").unwrap();
        writeln!(output, "//! Provides both standalone `build_schema()` and composable `inject_services()`").unwrap();
        writeln!(output, "//! methods. Use `inject_services()` to compose module GraphQL services into").unwrap();
        writeln!(output, "//! a shared app-level schema.").unwrap();
        writeln!(output).unwrap();

        // Imports
        writeln!(output, "#[allow(unused_imports)]").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "use std::sync::Arc;").unwrap();
        writeln!(output, "use async_graphql::*;").unwrap();
        writeln!(output).unwrap();

        // Import query/mutation types
        for model in models {
            writeln!(
                output,
                "use super::{{{}Query, {}Mutation}};",
                model.name, model.name
            )
            .unwrap();
        }
        writeln!(output).unwrap();

        // Import service types
        for model in models {
            writeln!(
                output,
                "use crate::application::service::{}Service;",
                model.name
            )
            .unwrap();
        }
        writeln!(output).unwrap();

        if needs_groups {
            // Split into sub-groups to avoid compiler recursion overflow
            let chunks: Vec<&[Model]> = models.chunks(Self::GROUP_SIZE).collect();

            // Generate Query sub-groups
            for (gi, chunk) in chunks.iter().enumerate() {
                writeln!(output, "#[derive(MergedObject, Default)]").unwrap();
                write!(output, "pub struct {}QueryGroup{}(", pascal_name, gi + 1).unwrap();
                for (i, model) in chunk.iter().enumerate() {
                    if i > 0 { write!(output, ", ").unwrap(); }
                    if i > 0 && i % 5 == 0 {
                        writeln!(output).unwrap();
                        write!(output, "    ").unwrap();
                    }
                    write!(output, "{}Query", model.name).unwrap();
                }
                writeln!(output, ");").unwrap();
                writeln!(output).unwrap();
            }

            // Top-level Query composing sub-groups
            writeln!(output, "/// Merged Query root for {} module", pascal_name).unwrap();
            writeln!(output, "#[derive(MergedObject, Default)]").unwrap();
            write!(output, "pub struct {}(", query_root).unwrap();
            for gi in 0..chunks.len() {
                if gi > 0 { write!(output, ", ").unwrap(); }
                write!(output, "{}QueryGroup{}", pascal_name, gi + 1).unwrap();
            }
            writeln!(output, ");").unwrap();
            writeln!(output).unwrap();

            // Generate Mutation sub-groups
            for (gi, chunk) in chunks.iter().enumerate() {
                writeln!(output, "#[derive(MergedObject, Default)]").unwrap();
                write!(output, "pub struct {}MutationGroup{}(", pascal_name, gi + 1).unwrap();
                for (i, model) in chunk.iter().enumerate() {
                    if i > 0 { write!(output, ", ").unwrap(); }
                    if i > 0 && i % 5 == 0 {
                        writeln!(output).unwrap();
                        write!(output, "    ").unwrap();
                    }
                    write!(output, "{}Mutation", model.name).unwrap();
                }
                writeln!(output, ");").unwrap();
                writeln!(output).unwrap();
            }

            // Top-level Mutation composing sub-groups
            writeln!(output, "/// Merged Mutation root for {} module", pascal_name).unwrap();
            writeln!(output, "#[derive(MergedObject, Default)]").unwrap();
            write!(output, "pub struct {}(", mutation_root).unwrap();
            for gi in 0..chunks.len() {
                if gi > 0 { write!(output, ", ").unwrap(); }
                write!(output, "{}MutationGroup{}", pascal_name, gi + 1).unwrap();
            }
            writeln!(output, ");").unwrap();
            writeln!(output).unwrap();
        } else {
            // Small module — single MergedObject is fine
            writeln!(output, "/// Merged Query root for {} module", pascal_name).unwrap();
            writeln!(output, "#[derive(MergedObject, Default)]").unwrap();
            write!(output, "pub struct {}(", query_root).unwrap();
            for (i, model) in models.iter().enumerate() {
                if i > 0 { write!(output, ", ").unwrap(); }
                write!(output, "{}Query", model.name).unwrap();
            }
            writeln!(output, ");").unwrap();
            writeln!(output).unwrap();

            writeln!(output, "/// Merged Mutation root for {} module", pascal_name).unwrap();
            writeln!(output, "#[derive(MergedObject, Default)]").unwrap();
            write!(output, "pub struct {}(", mutation_root).unwrap();
            for (i, model) in models.iter().enumerate() {
                if i > 0 { write!(output, ", ").unwrap(); }
                write!(output, "{}Mutation", model.name).unwrap();
            }
            writeln!(output, ");").unwrap();
            writeln!(output).unwrap();
        }

        // inject_services function
        writeln!(output, "/// Inject all {} services into a schema builder", pascal_name).unwrap();
        writeln!(output, "///").unwrap();
        writeln!(output, "/// Use this for app-level composition where multiple modules share one schema.").unwrap();
        writeln!(output, "pub fn inject_services<Q: ObjectType + 'static, M: ObjectType + 'static, S: SubscriptionType + 'static>(").unwrap();
        writeln!(output, "    builder: SchemaBuilder<Q, M, S>,").unwrap();
        writeln!(output, "    module: &crate::{}Module,", pascal_name).unwrap();
        writeln!(output, ") -> SchemaBuilder<Q, M, S> {{").unwrap();
        writeln!(output, "    builder").unwrap();

        for model in models {
            let model_snake = to_snake_case(&model.name);
            writeln!(
                output,
                "        .data(module.{}_service.clone())",
                model_snake
            )
            .unwrap();
        }

        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // build_schema function (standalone)
        writeln!(output, "/// Build the GraphQL schema for {} module (standalone)", pascal_name).unwrap();
        writeln!(output, "pub fn build_schema(module: &crate::{}Module) -> Schema<{}, {}, EmptySubscription> {{",
            pascal_name, query_root, mutation_root).unwrap();
        writeln!(output, "    let builder = Schema::build({}::default(), {}::default(), EmptySubscription);",
            query_root, mutation_root).unwrap();
        writeln!(output, "    inject_services(builder, module).finish()").unwrap();
        writeln!(output, "}}").unwrap();

        Ok(output)
    }
}

impl Default for GraphqlGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl Generator for GraphqlGenerator {
    fn generate(&self, schema: &ResolvedSchema) -> Result<GeneratedOutput, GenerateError> {
        let mut output = GeneratedOutput::new();

        // Generate per-entity files
        for model in &schema.schema.models {
            let content = self.generate_entity_graphql(model, schema)?;
            let path = PathBuf::from(format!(
                "src/presentation/graphql/{}_graphql.rs",
                to_snake_case(&model.name)
            ));
            output.add_file(path, content);
        }

        // Generate mod.rs
        output.add_file(
            PathBuf::from("src/presentation/graphql/mod.rs"),
            self.generate_mod_rs(schema),
        );

        // Generate server.rs
        let server = self.generate_server_rs(schema)?;
        output.add_file(
            PathBuf::from("src/presentation/graphql/server.rs"),
            server,
        );

        Ok(output)
    }

    fn name(&self) -> &'static str {
        "graphql"
    }
}
