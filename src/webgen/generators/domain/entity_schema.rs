//! Entity Schema generator for Zod validation schemas
//!
//! Generates Zod schemas with validation rules from both
//! model.yaml definitions and hook.yaml business rules.

use std::fs;

use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition, FieldDefinition, FieldType};
use crate::webgen::ast::HookSchema;
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::{to_pascal_case, to_camel_case, to_snake_case};
use super::type_mapping::TypeMapper;
use super::DomainGenerationResult;

/// Generator for Zod entity schemas
pub struct EntitySchemaGenerator {
    config: Config,
    type_mapper: TypeMapper,
}

impl EntitySchemaGenerator {
    /// Create a new entity schema generator
    pub fn new(config: Config, type_mapper: TypeMapper) -> Self {
        Self { config, type_mapper }
    }

    /// Generate schema file for a single entity
    pub fn generate(
        &self,
        entity: &EntityDefinition,
        enums: &[EnumDefinition],
        hooks: Option<&HookSchema>,
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let entity_dir = self.config.output_dir
            .join("domain")
            .join(&self.config.module)
            .join("entity");

        if !self.config.dry_run {
            fs::create_dir_all(&entity_dir).ok();
        }

        // Generate schema file
        let schema_content = self.generate_schema_content(entity, enums, hooks);
        let schema_path = entity_dir.join(format!("{}.schema.ts", entity_pascal));

        result.add_file(schema_path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&schema_path, schema_content).ok();
        }

        Ok(result)
    }

    /// Generate the Zod schema content
    fn generate_schema_content(
        &self,
        entity: &EntityDefinition,
        enums: &[EnumDefinition],
        hooks: Option<&HookSchema>,
    ) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_camel = to_camel_case(&entity.name);
        let _entity_snake = to_snake_case(&entity.name);

        // Check if entity uses IP address fields
        let uses_ip = self.entity_uses_ip(entity);

        // Generate enum schemas
        let enum_schemas = self.generate_enum_schemas(entity, enums);

        // Generate field schemas
        let base_fields = self.generate_base_field_schemas(entity, enums);

        // Generate create schema fields (without auto-generated fields)
        let create_fields = self.generate_create_field_schemas(entity, enums);

        // Generate update schema fields (all fields partial except id)
        let update_fields = self.generate_update_field_schemas(entity, enums);

        // Get additional validations from hooks
        let hook_validations = if let Some(hook_schema) = hooks {
            self.extract_hook_validations(entity, hook_schema)
        } else {
            String::new()
        };

        // Generate IP schema if needed
        let ip_schema = if uses_ip {
            r#"
// ============================================================================
// Common Validation Schemas
// ============================================================================

/**
 * IP address validation schema (supports both IPv4 and IPv6)
 */
export const ipSchema = z.string().refine(
  (val) => {
    // IPv4 regex pattern
    const ipv4Pattern = /^(\d{1,3}\.){3}\d{1,3}$/;
    // IPv6 regex pattern (simplified)
    const ipv6Pattern = /^([0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}$/;
    return ipv4Pattern.test(val) || ipv6Pattern.test(val);
  },
  { message: "Invalid IP address format" }
);
"#
        } else {
            ""
        };

        format!(
r#"/**
 * {entity_pascal} Validation Schemas
 *
 * Zod schemas for validating {entity_pascal} data.
 * Generated from schema definition with validation rules.
 *
 * @module {module}/entity/{entity_pascal}.schema
 */

import {{ z }} from 'zod';{ip_schema}{enum_schemas}
// ============================================================================
// Base Schema
// ============================================================================

/**
 * Complete {entity_pascal} schema with all fields
 */
export const {entity_camel}Schema = z.object({{
{base_fields}
}});

/**
 * {entity_pascal} type inferred from schema
 */
export type {entity_pascal} = z.infer<typeof {entity_camel}Schema>;

// ============================================================================
// Create/Update Schemas
// ============================================================================

/**
 * Schema for creating a new {entity_pascal}
 * Excludes auto-generated fields (id, createdAt, updatedAt, etc.)
 */
export const create{entity_pascal}Schema = z.object({{
{create_fields}
}});

/**
 * Input type for creating a {entity_pascal}
 */
export type Create{entity_pascal}Input = z.infer<typeof create{entity_pascal}Schema>;

/**
 * Schema for updating an existing {entity_pascal}
 * All fields are optional except id
 */
export const update{entity_pascal}Schema = z.object({{
  id: z.string().uuid(),
{update_fields}
}});

/**
 * Input type for updating a {entity_pascal}
 */
export type Update{entity_pascal}Input = z.infer<typeof update{entity_pascal}Schema>;

/**
 * Schema for partial {entity_pascal} updates (PATCH)
 */
export const patch{entity_pascal}Schema = update{entity_pascal}Schema.partial().required({{ id: true }});

/**
 * Input type for patching a {entity_pascal}
 */
export type Patch{entity_pascal}Input = z.infer<typeof patch{entity_pascal}Schema>;

// ============================================================================
// Query Schemas
// ============================================================================

/**
 * Schema for {entity_pascal} list query parameters
 */
export const {entity_camel}QuerySchema = z.object({{
  page: z.number().int().positive().default(1),
  limit: z.number().int().positive().max(100).default(20),
  sortBy: z.string().optional(),
  sortOrder: z.enum(['asc', 'desc']).default('desc'),
  search: z.string().optional(),
}});

/**
 * Query parameters type
 */
export type {entity_pascal}QueryParams = z.infer<typeof {entity_camel}QuerySchema>;

/**
 * Schema for {entity_pascal} filter parameters
 */
export const {entity_camel}FilterSchema = z.object({{
{filter_fields}
}}).partial();

/**
 * Filter parameters type
 */
export type {entity_pascal}FilterParams = z.infer<typeof {entity_camel}FilterSchema>;
{hook_validations}
// ============================================================================
// Validation Helpers
// ============================================================================

/**
 * Validate create input and return typed result
 */
export function validateCreate{entity_pascal}(data: unknown): Create{entity_pascal}Input {{
  return create{entity_pascal}Schema.parse(data);
}}

/**
 * Validate update input and return typed result
 */
export function validateUpdate{entity_pascal}(data: unknown): Update{entity_pascal}Input {{
  return update{entity_pascal}Schema.parse(data);
}}

/**
 * Safe parse create input (returns result object)
 */
export function safeParseCreate{entity_pascal}(data: unknown) {{
  return create{entity_pascal}Schema.safeParse(data);
}}

/**
 * Safe parse update input (returns result object)
 */
export function safeParseUpdate{entity_pascal}(data: unknown) {{
  return update{entity_pascal}Schema.safeParse(data);
}}

// <<< CUSTOM: Add custom validation schemas here
// END CUSTOM
"#,
            entity_pascal = entity_pascal,
            entity_camel = entity_camel,
            module = self.config.module,
            ip_schema = ip_schema,
            enum_schemas = if enum_schemas.is_empty() { String::new() } else { format!("\n{}", enum_schemas) },
            base_fields = base_fields,
            create_fields = create_fields,
            update_fields = update_fields,
            filter_fields = self.generate_filter_field_schemas(entity, enums),
            hook_validations = hook_validations,
        )
    }

    /// Generate enum schema definitions
    fn generate_enum_schemas(&self, entity: &EntityDefinition, enums: &[EnumDefinition]) -> String {
        let mut schemas = Vec::new();
        let mut seen_enums = std::collections::HashSet::new();

        for enum_def in enums {
            // Skip if we've already added this enum name (avoid duplicates)
            if seen_enums.contains(&enum_def.name) {
                continue;
            }

            if self.entity_uses_enum(entity, &enum_def.name) {
                seen_enums.insert(enum_def.name.clone());

                let variants: Vec<String> = enum_def.variants.iter()
                    .map(|v| format!("'{}'", v.name))
                    .collect();

                schemas.push(format!(
r#"
/**
 * {name} enum values
 */
export const {name}Values = [{variants}] as const;
export type {name} = typeof {name}Values[number];
export const {name_camel}Schema = z.enum({name}Values);
"#,
                    name = enum_def.name,
                    name_camel = to_camel_case(&enum_def.name),
                    variants = variants.join(", "),
                ));
            }
        }

        schemas.join("\n")
    }

    /// Check if entity uses a specific enum
    fn entity_uses_enum(&self, entity: &EntityDefinition, enum_name: &str) -> bool {
        entity.fields.iter().any(|f| {
            matches!(&f.type_name, FieldType::Enum(name) if name == enum_name) ||
            matches!(&f.type_name, FieldType::Custom(name) if name == enum_name) ||
            matches!(&f.type_name, FieldType::Array(inner) if {
                matches!(inner.as_ref(), FieldType::Enum(name) if name == enum_name) ||
                matches!(inner.as_ref(), FieldType::Custom(name) if name == enum_name)
            })
        })
    }

    /// Check if entity has any IP address fields
    fn entity_uses_ip(&self, entity: &EntityDefinition) -> bool {
        entity.fields.iter().any(|f| {
            matches!(&f.type_name, FieldType::Ip) ||
            matches!(&f.type_name, FieldType::Optional(inner) if {
                matches!(inner.as_ref(), FieldType::Ip)
            }) ||
            matches!(&f.type_name, FieldType::Array(inner) if {
                matches!(inner.as_ref(), FieldType::Ip) ||
                matches!(inner.as_ref(), FieldType::Optional(opt_inner) if {
                    matches!(opt_inner.as_ref(), FieldType::Ip)
                })
            })
        })
    }

    /// Generate base field schemas (all fields)
    fn generate_base_field_schemas(&self, entity: &EntityDefinition, enums: &[EnumDefinition]) -> String {
        let mut fields = Vec::new();

        for field in &entity.fields {
            let schema = self.type_mapper.to_zod_schema(field, enums);

            // Add comment if description exists
            if let Some(desc) = &field.description {
                fields.push(format!("  /** {} */", desc));
            }

            fields.push(format!("  {}: {},", field.name, schema));
        }

        fields.join("\n")
    }

    /// Generate create field schemas (exclude auto-generated)
    fn generate_create_field_schemas(&self, entity: &EntityDefinition, enums: &[EnumDefinition]) -> String {
        let mut fields = Vec::new();

        for field in &entity.fields {
            // Skip auto-generated fields
            if self.is_auto_generated_field(field) {
                continue;
            }

            let schema = self.type_mapper.to_zod_schema(field, enums);
            fields.push(format!("  {}: {},", field.name, schema));
        }

        fields.join("\n")
    }

    /// Generate update field schemas (all optional except those that shouldn't be)
    fn generate_update_field_schemas(&self, entity: &EntityDefinition, enums: &[EnumDefinition]) -> String {
        let mut fields = Vec::new();

        for field in &entity.fields {
            // Skip id (handled separately) and auto-generated timestamp fields
            if field.name == "id" || self.is_timestamp_field(field) {
                continue;
            }

            let base_schema = self.type_mapper.to_zod_schema(field, enums);
            // Make all update fields optional
            let optional_schema = format!("{}.optional()", base_schema);
            fields.push(format!("  {}: {},", field.name, optional_schema));
        }

        fields.join("\n")
    }

    /// Generate filter field schemas for query parameters
    fn generate_filter_field_schemas(&self, entity: &EntityDefinition, enums: &[EnumDefinition]) -> String {
        let mut fields = Vec::new();

        for field in &entity.fields {
            // Skip complex types for simple filters
            if matches!(field.type_name, FieldType::Json | FieldType::Text) {
                continue;
            }

            // Skip sensitive fields
            if field.name.contains("password") || field.name.contains("hash") || field.name.contains("token") {
                continue;
            }

            let filter_schema = self.generate_filter_schema_for_field(field, enums);
            fields.push(format!("  {}: {},", field.name, filter_schema));
        }

        fields.join("\n")
    }

    /// Generate filter schema for a specific field
    #[allow(clippy::only_used_in_recursion)]
    fn generate_filter_schema_for_field(&self, field: &FieldDefinition, enums: &[EnumDefinition]) -> String {
        match &field.type_name {
            FieldType::String | FieldType::Email | FieldType::Url | FieldType::Phone | FieldType::Ip => {
                "z.string().optional()".to_string()
            }
            FieldType::Int => {
                "z.union([z.number().int(), z.object({ gte: z.number().optional(), lte: z.number().optional() })]).optional()".to_string()
            }
            FieldType::Float | FieldType::Decimal => {
                "z.union([z.number(), z.object({ gte: z.number().optional(), lte: z.number().optional() })]).optional()".to_string()
            }
            FieldType::Bool => {
                "z.boolean().optional()".to_string()
            }
            FieldType::DateTime | FieldType::Date => {
                "z.union([z.string().datetime(), z.object({ gte: z.string().datetime().optional(), lte: z.string().datetime().optional() })]).optional()".to_string()
            }
            FieldType::Uuid => {
                "z.string().uuid().optional()".to_string()
            }
            FieldType::Enum(name) | FieldType::Custom(name) => {
                if enums.iter().any(|e| &e.name == name) {
                    format!("{}.optional()", to_camel_case(name) + "Schema")
                } else {
                    "z.string().optional()".to_string()
                }
            }
            FieldType::Array(inner) => {
                let inner_schema = self.generate_filter_schema_for_field(
                    &FieldDefinition {
                        name: field.name.clone(),
                        type_name: inner.as_ref().clone(),
                        attributes: vec![],
                        description: None,
                        optional: true,
                        default_value: None,
                    },
                    enums,
                );
                format!("z.array({}).optional()", inner_schema.trim_end_matches(".optional()"))
            }
            _ => "z.any().optional()".to_string(),
        }
    }

    /// Check if field is auto-generated
    fn is_auto_generated_field(&self, field: &FieldDefinition) -> bool {
        let name = field.name.to_lowercase();

        // Check common auto-generated field names
        name == "id" ||
        name == "created_at" ||
        name == "createdat" ||
        name == "updated_at" ||
        name == "updatedat" ||
        name == "deleted_at" ||
        name == "deletedat" ||
        // Check attributes
        field.attributes.iter().any(|a| {
            a.name == "id" ||
            a.name == "auto" ||
            a.name == "generated" ||
            a.name == "default" && a.first_arg().is_some_and(|v| {
                v.contains("now()") || v.contains("uuid") || v.contains("auto")
            })
        })
    }

    /// Check if field is a timestamp field
    fn is_timestamp_field(&self, field: &FieldDefinition) -> bool {
        let name = field.name.to_lowercase();
        name == "created_at" ||
        name == "createdat" ||
        name == "updated_at" ||
        name == "updatedat" ||
        name == "deleted_at" ||
        name == "deletedat"
    }

    /// Extract additional validations from hook schema
    fn extract_hook_validations(&self, entity: &EntityDefinition, _hooks: &HookSchema) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_camel = to_camel_case(&entity.name);

        // For now, generate a placeholder for custom validations
        // This can be enhanced to actually parse hook validations
        format!(
r#"
// ============================================================================
// Business Rule Validations (from hooks)
// ============================================================================

/**
 * Validate {entity_pascal} against business rules
 */
export function validate{entity_pascal}BusinessRules(
  {entity_camel}: {entity_pascal}
): {{ valid: boolean; errors: string[] }} {{
  const errors: string[] = [];

  // Add business rule validations here based on hook definitions
  // These are typically more complex validations that span multiple fields

  return {{
    valid: errors.length === 0,
    errors,
  }};
}}
"#,
            entity_pascal = entity_pascal,
            entity_camel = entity_camel,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        Config::new("test_module")
            .with_output_dir("/tmp/test")
            .with_dry_run(true)
    }

    #[test]
    fn test_auto_generated_field_detection() {
        let generator = EntitySchemaGenerator::new(test_config(), TypeMapper::new());

        let id_field = FieldDefinition {
            name: "id".to_string(),
            type_name: FieldType::Uuid,
            attributes: vec![],
            description: None,
            optional: false,
            default_value: None,
        };

        let created_at_field = FieldDefinition {
            name: "created_at".to_string(),
            type_name: FieldType::DateTime,
            attributes: vec![],
            description: None,
            optional: false,
            default_value: None,
        };

        let email_field = FieldDefinition {
            name: "email".to_string(),
            type_name: FieldType::Email,
            attributes: vec![],
            description: None,
            optional: false,
            default_value: None,
        };

        assert!(generator.is_auto_generated_field(&id_field));
        assert!(generator.is_auto_generated_field(&created_at_field));
        assert!(!generator.is_auto_generated_field(&email_field));
    }
}
