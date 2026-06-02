//! Entity type generator for TypeScript domain layer
//!
//! Generates TypeScript interfaces, factory functions, and type guards
//! from schema entity definitions.

use std::fs;

use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition};
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::{to_pascal_case, to_camel_case};
use super::type_mapping::TypeMapper;
use super::DomainGenerationResult;

/// Generator for TypeScript entity types
pub struct EntityGenerator {
    config: Config,
    type_mapper: TypeMapper,
}

impl EntityGenerator {
    /// Create a new entity generator
    pub fn new(config: Config, type_mapper: TypeMapper) -> Self {
        Self { config, type_mapper }
    }

    /// Generate entity type file for a single entity
    pub fn generate(
        &self,
        entity: &EntityDefinition,
        enums: &[EnumDefinition],
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let entity_dir = self.config.output_dir
            .join(&self.config.module).join("domain")
            .join("entity");

        if !self.config.dry_run {
            fs::create_dir_all(&entity_dir).ok();
        }

        // Generate entity type file
        let entity_content = self.generate_entity_content(entity, enums);
        let entity_path = entity_dir.join(format!("{}.ts", entity_pascal));

        result.add_file(entity_path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&entity_path, entity_content).ok();
        }

        // Generate enum types if any are used by this entity
        for enum_def in enums {
            if self.entity_uses_enum(entity, &enum_def.name) {
                let enum_content = self.generate_enum_content(enum_def);
                let enum_path = entity_dir.join(format!("{}.ts", enum_def.name));

                result.add_file(enum_path.clone(), self.config.dry_run);

                if !self.config.dry_run {
                    fs::write(&enum_path, enum_content).ok();
                }
            }
        }

        Ok(result)
    }

    /// Check if entity uses a specific enum (unwrapping Optional/Array wrappers
    /// so `Status?` and `Tag[]` still trigger enum-file generation).
    fn entity_uses_enum(&self, entity: &EntityDefinition, enum_name: &str) -> bool {
        fn references(ft: &crate::webgen::ast::entity::FieldType, target: &str) -> bool {
            use crate::webgen::ast::entity::FieldType;
            match ft {
                FieldType::Enum(name) | FieldType::Custom(name) => name == target,
                FieldType::Array(inner) | FieldType::Optional(inner) => references(inner, target),
                _ => false,
            }
        }
        entity.fields.iter().any(|f| references(&f.type_name, enum_name))
    }

    /// Generate the entity TypeScript content
    fn generate_entity_content(&self, entity: &EntityDefinition, enums: &[EnumDefinition]) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_camel = to_camel_case(&entity.name);

        // Collect imports
        let imports = self.generate_imports(entity, enums);

        // Generate factory defaults
        let factory_defaults = self.generate_factory_defaults(entity, enums);

        // Generate relation types if any
        let relation_types = self.generate_relation_types(entity);

        // The canonical entity type is the Zod-inferred type from the schema
        // file (single source of truth). This file imports it and adds runtime
        // helpers (factory, type guard) plus the "with relations" view.
        format!(
r#"/**
 * {entity_pascal} Entity Helpers
 *
 * Generated from schema definition. The canonical `{entity_pascal}` type is
 * defined in `./{entity_pascal}.schema` (inferred from its Zod schema).
 * @module {module}/entity/{entity_pascal}
 */

import type {{ {entity_pascal} }} from './{entity_pascal}.schema';
{imports}
{relation_types}
/**
 * Create a new {entity_pascal} with default values
 */
export function create{entity_pascal}(data: Partial<{entity_pascal}> = {{}}): {entity_pascal} {{
  return {{
{factory_defaults}
  }};
}}

/**
 * Type guard for {entity_pascal}
 */
export function is{entity_pascal}(value: unknown): value is {entity_pascal} {{
  if (typeof value !== 'object' || value === null) {{
    return false;
  }}

  const obj = value as Record<string, unknown>;
  return (
    typeof obj.{pk} === 'string'
  );
}}

/**
 * Clone a {entity_pascal} with optional overrides
 */
export function clone{entity_pascal}(
  {entity_camel}: {entity_pascal},
  overrides: Partial<{entity_pascal}> = {{}}
): {entity_pascal} {{
  return {{
    ...{entity_camel},
    ...overrides,
  }};
}}

/**
 * Compare two {entity_pascal} entities by ID
 */
export function equals{entity_pascal}(a: {entity_pascal}, b: {entity_pascal}): boolean {{
  return a.{pk} === b.{pk};
}}

// <<< CUSTOM: Add custom entity utilities here
// END CUSTOM
"#,
            entity_pascal = entity_pascal,
            entity_camel = entity_camel,
            pk = Self::primary_key(entity),
            module = self.config.module,
            imports = imports,
            factory_defaults = factory_defaults,
            relation_types = relation_types,
        )
    }

    /// Generate import statements
    fn generate_imports(&self, entity: &EntityDefinition, enums: &[EnumDefinition]) -> String {
        let mut imports = Vec::new();

        // Check for enum imports
        for field in &entity.fields {
            if let Some(enum_name) = Self::get_enum_name(&field.type_name, enums) {
                imports.push(format!("import {{ {} }} from './{}';", enum_name, enum_name));
            }
        }

        // Check for relation imports. Skip cross-module/qualified targets
        // (e.g. `Sapiens.User`) — they aren't generated as local files in this
        // module and are rendered as `unknown` in the relations interface.
        let self_pascal = to_pascal_case(&entity.name);
        for relation in &entity.relations {
            if !Self::is_local_relation(&relation.target_entity) {
                continue;
            }
            let target_pascal = to_pascal_case(&relation.target_entity);
            if target_pascal == self_pascal {
                continue; // avoid self-import
            }
            // The canonical entity type lives in the sibling `.schema` file.
            imports.push(format!(
                "import type {{ {} }} from './{}.schema';",
                target_pascal, target_pascal
            ));
        }

        if imports.is_empty() {
            String::new()
        } else {
            imports.sort();
            imports.dedup();
            imports.join("\n")
        }
    }

    /// The entity's primary-key field name: the field marked `@id`, else a
    /// field literally named `id`, else the first field (fallback).
    pub fn primary_key(entity: &EntityDefinition) -> String {
        entity
            .fields
            .iter()
            .find(|f| f.attributes.iter().any(|a| a.name == "id"))
            .or_else(|| entity.fields.iter().find(|f| f.name == "id"))
            .or_else(|| entity.fields.first())
            .map(|f| f.name.clone())
            .unwrap_or_else(|| "id".to_string())
    }

    /// Whether a relation target is a local (same-module) entity that is
    /// generated as a sibling file. Qualified names (`Module.Entity`) refer to
    /// other modules and are not imported here.
    fn is_local_relation(target: &str) -> bool {
        !target.contains('.')
    }

    /// Get enum name if field type is an enum
    fn get_enum_name(field_type: &crate::webgen::ast::entity::FieldType, enums: &[EnumDefinition]) -> Option<String> {
        use crate::webgen::ast::entity::FieldType;
        match field_type {
            // Only treat as a (locally generated) enum if it is in our enum set,
            // so we never emit an import for a file we don't generate.
            FieldType::Enum(name) | FieldType::Custom(name) => {
                if enums.iter().any(|e| &e.name == name) {
                    Some(name.clone())
                } else {
                    None
                }
            }
            FieldType::Array(inner) => Self::get_enum_name(inner, enums),
            FieldType::Optional(inner) => Self::get_enum_name(inner, enums),
            _ => None,
        }
    }

    /// Generate factory default values
    fn generate_factory_defaults(&self, entity: &EntityDefinition, enums: &[EnumDefinition]) -> String {
        let mut defaults = Vec::new();

        for field in &entity.fields {
            let default_val = if let Some(ref default) = field.default_value {
                self.format_default_value(default, &field.type_name, enums)
            } else if field.optional {
                "null".to_string()
            } else if let Some(enum_name) = Self::enum_type_name(&field.type_name, enums) {
                // Required enum without a schema default → use the first variant so
                // the factory yields a valid enum member (not `'' as Enum`).
                Self::first_enum_member(&enum_name, enums)
            } else {
                self.type_mapper.default_value_for_type(&field.type_name)
            };

            defaults.push(format!(
                "    {}: data.{} ?? {},",
                field.name, field.name, default_val
            ));
        }

        defaults.join("\n")
    }

    /// Resolve the enum type name for a field type, if it refers to a known enum.
    fn enum_type_name(
        field_type: &crate::webgen::ast::entity::FieldType,
        enums: &[EnumDefinition],
    ) -> Option<String> {
        use crate::webgen::ast::entity::FieldType;
        match field_type {
            FieldType::Enum(name) => Some(name.clone()),
            FieldType::Custom(name) if enums.iter().any(|e| &e.name == name) => Some(name.clone()),
            FieldType::Optional(inner) => Self::enum_type_name(inner, enums),
            _ => None,
        }
    }

    /// `EnumName.FirstVariant`, or a safe cast if the enum has no variants.
    fn first_enum_member(enum_name: &str, enums: &[EnumDefinition]) -> String {
        match enums
            .iter()
            .find(|e| e.name == enum_name)
            .and_then(|e| e.variants.first())
        {
            Some(v) => format!("{}.{}", enum_name, v.name),
            None => format!("'' as unknown as {}", enum_name),
        }
    }

    /// Format a schema `@default(...)` value as a TypeScript expression.
    fn format_default_value(
        &self,
        value: &str,
        field_type: &crate::webgen::ast::entity::FieldType,
        enums: &[EnumDefinition],
    ) -> String {
        use crate::webgen::ast::entity::FieldType;

        // Runtime-function defaults (uuid/now/…) → idiomatic runtime expression.
        if TypeMapper::is_function_default(value) {
            return self.type_mapper.default_value_for_type(field_type);
        }

        match field_type {
            FieldType::String | FieldType::Text | FieldType::Email | FieldType::Phone
            | FieldType::Url | FieldType::Uuid | FieldType::Ip | FieldType::Date
            | FieldType::Time | FieldType::DateTime => {
                if value.starts_with('"') || value.starts_with('\'') {
                    value.to_string()
                } else {
                    format!("'{}'", value)
                }
            }
            FieldType::Bool => {
                if value == "true" || value == "false" {
                    value.to_string()
                } else {
                    "false".to_string()
                }
            }
            FieldType::Int | FieldType::Float | FieldType::Decimal => {
                if value.parse::<f64>().is_ok() { value.to_string() } else { "0".to_string() }
            }
            FieldType::Json => {
                // The TS type for Json is `Record<string, unknown>`; an array
                // default would not be assignable, so coerce to an object.
                if value.starts_with('{') { value.to_string() } else { "{}".to_string() }
            }
            FieldType::Array(_) => {
                if value.starts_with('[') { value.to_string() } else { "[]".to_string() }
            }
            FieldType::Enum(name) => format!("{}.{}", name, value),
            FieldType::Custom(name) if enums.iter().any(|e| &e.name == name) => {
                format!("{}.{}", name, value)
            }
            FieldType::Custom(name) if TypeMapper::is_numeric_scalar(name) => {
                if value.parse::<f64>().is_ok() { value.to_string() } else { "0".to_string() }
            }
            FieldType::Custom(_) => "{}".to_string(),
            FieldType::Optional(inner) => self.format_default_value(value, inner, enums),
        }
    }

    /// Generate relation type definitions
    fn generate_relation_types(&self, entity: &EntityDefinition) -> String {
        if entity.relations.is_empty() {
            return String::new();
        }

        let entity_pascal = to_pascal_case(&entity.name);
        let mut relation_fields = Vec::new();

        for relation in &entity.relations {
            let relation_name = to_camel_case(&relation.name);

            // Cross-module/qualified targets aren't generated locally → `unknown`.
            let target_pascal = if Self::is_local_relation(&relation.target_entity) {
                to_pascal_case(&relation.target_entity)
            } else {
                "unknown".to_string()
            };

            let relation_type = if relation.relation_type.is_many() {
                format!("{}[]", target_pascal)
            } else {
                format!("{} | null", target_pascal)
            };

            relation_fields.push(format!("  {}?: {};", relation_name, relation_type));
        }

        format!(
r#"

/**
 * {entity_pascal} with loaded relations
 */
export interface {entity_pascal}WithRelations extends {entity_pascal} {{
{relations}
}}
"#,
            entity_pascal = entity_pascal,
            relations = relation_fields.join("\n"),
        )
    }

    /// Generate enum TypeScript content
    fn generate_enum_content(&self, enum_def: &EnumDefinition) -> String {
        let variants: Vec<String> = enum_def.variants.iter()
            .map(|v| format!("  {} = '{}',", v.name, v.name))
            .collect();

        let variant_union: Vec<String> = enum_def.variants.iter()
            .map(|v| format!("'{}'", v.name))
            .collect();

        let variant_array: Vec<String> = enum_def.variants.iter()
            .map(|v| format!("  '{}',", v.name))
            .collect();

        format!(
r#"/**
 * {name} Enum
 *
 * Generated from schema definition.
 */

/**
 * {name} enum values
 */
export enum {name} {{
{variants}
}}

/**
 * {name} as union type
 */
export type {name}Type = {union};

/**
 * Array of all {name} values
 */
export const {name}Values = [
{array}
] as const;

/**
 * Check if a value is a valid {name}
 */
export function is{name}(value: unknown): value is {name} {{
  return typeof value === 'string' && {name}Values.includes(value as {name});
}}

/**
 * Get display label for {name} value
 */
export function get{name}Label(value: {name}): string {{
  const labels: Record<{name}, string> = {{
{labels}
  }};
  return labels[value] ?? value;
}}
"#,
            name = enum_def.name,
            variants = variants.join("\n"),
            union = variant_union.join(" | "),
            array = variant_array.join("\n"),
            labels = enum_def.variants.iter()
                .map(|v| {
                    let label = v.description.as_ref().unwrap_or(&v.name);
                    format!("    [{}.{}]: '{}',", enum_def.name, v.name, label)
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webgen::ast::entity::{FieldDefinition, FieldType};

    fn test_config() -> Config {
        Config::new("test_module")
            .with_output_dir("/tmp/test")
            .with_dry_run(true)
    }

    #[test]
    fn test_entity_generation() {
        let generator = EntityGenerator::new(test_config(), TypeMapper::new());

        let entity = EntityDefinition {
            name: "User".to_string(),
            collection: "users".to_string(),
            fields: vec![
                FieldDefinition {
                    name: "id".to_string(),
                    type_name: FieldType::Uuid,
                    attributes: vec![],
                    description: Some("Unique identifier".to_string()),
                    optional: false,
                    default_value: None,
                },
                FieldDefinition {
                    name: "email".to_string(),
                    type_name: FieldType::Email,
                    attributes: vec![],
                    description: Some("Email address".to_string()),
                    optional: false,
                    default_value: None,
                },
            ],
            relations: vec![],
            indexes: vec![],
            soft_delete: false,
        };

        let content = generator.generate_entity_content(&entity, &[]);
        // The canonical type now lives in the schema file; this helper file
        // imports it and provides runtime helpers (factory, guards).
        assert!(content.contains("import type { User } from './User.schema';"));
        assert!(content.contains("export function createUser"));
        assert!(content.contains("export function isUser"));
        assert!(!content.contains("export interface User {"));
    }
}
