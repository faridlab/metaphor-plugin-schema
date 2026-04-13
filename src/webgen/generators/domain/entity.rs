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
            .join("domain")
            .join(&self.config.module)
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

    /// Check if entity uses a specific enum
    fn entity_uses_enum(&self, entity: &EntityDefinition, enum_name: &str) -> bool {
        entity.fields.iter().any(|f| {
            matches!(&f.type_name, crate::webgen::ast::entity::FieldType::Enum(name) if name == enum_name) ||
            matches!(&f.type_name, crate::webgen::ast::entity::FieldType::Custom(name) if name == enum_name)
        })
    }

    /// Generate the entity TypeScript content
    fn generate_entity_content(&self, entity: &EntityDefinition, enums: &[EnumDefinition]) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_camel = to_camel_case(&entity.name);

        // Collect imports
        let imports = self.generate_imports(entity, enums);

        // Generate interface fields
        let interface_fields = self.generate_interface_fields(entity);

        // Generate factory defaults
        let factory_defaults = self.generate_factory_defaults(entity);

        // Generate relation types if any
        let relation_types = self.generate_relation_types(entity);

        format!(
r#"/**
 * {entity_pascal} Entity Type
 *
 * Generated from schema definition.
 * @module {module}/entity/{entity_pascal}
 */

{imports}

/**
 * {entity_pascal} entity interface
 */
export interface {entity_pascal} {{
{interface_fields}
}}
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
    typeof obj.id === 'string'
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
  return a.id === b.id;
}}

// <<< CUSTOM: Add custom entity utilities here
// END CUSTOM
"#,
            entity_pascal = entity_pascal,
            entity_camel = entity_camel,
            module = self.config.module,
            imports = imports,
            interface_fields = interface_fields,
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

        // Check for relation imports
        for relation in &entity.relations {
            let target_pascal = to_pascal_case(&relation.target_entity);
            imports.push(format!(
                "import type {{ {} }} from './{}';",
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

    /// Get enum name if field type is an enum
    fn get_enum_name(field_type: &crate::webgen::ast::entity::FieldType, enums: &[EnumDefinition]) -> Option<String> {
        match field_type {
            crate::webgen::ast::entity::FieldType::Enum(name) => Some(name.clone()),
            crate::webgen::ast::entity::FieldType::Custom(name) => {
                if enums.iter().any(|e| &e.name == name) {
                    Some(name.clone())
                } else {
                    None
                }
            }
            crate::webgen::ast::entity::FieldType::Array(inner) => Self::get_enum_name(inner, enums),
            crate::webgen::ast::entity::FieldType::Optional(inner) => Self::get_enum_name(inner, enums),
            _ => None,
        }
    }

    /// Generate interface field definitions
    fn generate_interface_fields(&self, entity: &EntityDefinition) -> String {
        let mut fields = Vec::new();

        for field in &entity.fields {
            let ts_type = self.type_mapper.to_typescript_type(&field.type_name, field.optional);
            let optional_mark = if field.optional { "?" } else { "" };

            // Add JSDoc comment if description exists
            if let Some(desc) = &field.description {
                fields.push(format!("  /** {} */", desc));
            }

            // Add readonly for ID field
            if field.name == "id" {
                fields.push(format!("  readonly {}{}: {};", field.name, optional_mark, ts_type));
            } else {
                fields.push(format!("  {}{}: {};", field.name, optional_mark, ts_type));
            }
        }

        fields.join("\n")
    }

    /// Generate factory default values
    fn generate_factory_defaults(&self, entity: &EntityDefinition) -> String {
        let mut defaults = Vec::new();

        for field in &entity.fields {
            let default_val = if let Some(ref default) = field.default_value {
                self.format_default_value(default, &field.type_name)
            } else if field.optional {
                "null".to_string()
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

    /// Format a default value for TypeScript
    fn format_default_value(&self, value: &str, field_type: &crate::webgen::ast::entity::FieldType) -> String {
        match field_type {
            crate::webgen::ast::entity::FieldType::String |
            crate::webgen::ast::entity::FieldType::Text |
            crate::webgen::ast::entity::FieldType::Email |
            crate::webgen::ast::entity::FieldType::Phone |
            crate::webgen::ast::entity::FieldType::Url => {
                if value.starts_with('"') || value.starts_with('\'') {
                    value.to_string()
                } else {
                    format!("'{}'", value)
                }
            }
            crate::webgen::ast::entity::FieldType::Bool => {
                if value == "true" || value == "false" {
                    value.to_string()
                } else {
                    "false".to_string()
                }
            }
            _ => value.to_string(),
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
            let target_pascal = to_pascal_case(&relation.target_entity);
            let relation_name = to_camel_case(&relation.name);

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
    use crate::webgen::ast::entity::FieldType;

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
        assert!(content.contains("export interface User"));
        assert!(content.contains("readonly id: string"));
        assert!(content.contains("email: string"));
    }
}
