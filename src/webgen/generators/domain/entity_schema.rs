//! Entity Schema generator for Zod validation schemas
//!
//! Generates Zod schemas with validation rules from both
//! model.yaml definitions and hook.yaml business rules.

use std::fs;

use super::entity::escape_single_quoted;
use super::type_mapping::TypeMapper;
use super::DomainGenerationResult;
use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition, FieldDefinition, FieldType};
use crate::webgen::ast::HookSchema;
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::{to_camel_case, to_pascal_case, to_snake_case};

/// Generator for Zod entity schemas
pub struct EntitySchemaGenerator {
    config: Config,
    type_mapper: TypeMapper,
}

impl EntitySchemaGenerator {
    /// Create a new entity schema generator
    pub fn new(config: Config, type_mapper: TypeMapper) -> Self {
        Self {
            config,
            type_mapper,
        }
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
        let entity_dir = self
            .config
            .output_dir
            .join(&self.config.module)
            .join("domain")
            .join("entity");

        if !self.config.dry_run {
            fs::create_dir_all(&entity_dir).ok();
        }

        // Generate schema file
        let schema_content = self.generate_schema_content(entity, enums, hooks);
        let schema_path = entity_dir.join(format!("{}.schema.ts", entity_pascal));

        result.add_file(schema_path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            // Preserve any `// <<< CUSTOM … // END CUSTOM` block authored in the
            // existing file (e.g. a hand-written `listSchema`) across regen — the
            // generator emits the markers but the content lives only on disk.
            crate::webgen::custom_blocks::preserve_and_write(&schema_path, &schema_content).ok();
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
 * IP address validation schema (supports both IPv4 and IPv6).
 * Module-local to avoid barrel collisions across entity schema files.
 */
const ipSchema = z.string().refine(
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
{relation_targets}{hook_validations}
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
            enum_schemas = if enum_schemas.is_empty() {
                String::new()
            } else {
                format!("\n{}", enum_schemas)
            },
            base_fields = base_fields,
            create_fields = create_fields,
            update_fields = update_fields,
            filter_fields = self.generate_filter_field_schemas(entity, enums),
            relation_targets = self.generate_relation_targets(entity),
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

                let variants: Vec<String> = enum_def
                    .variants
                    .iter()
                    .map(|v| format!("'{}'", v.name))
                    .collect();

                // `{name}` and `{name}Values` are kept module-local to avoid
                // colliding with the standalone enum file (`{name}.ts`) at the
                // barrel. Only the zod schema is exported (used by filters).
                schemas.push(format!(
                    r#"
/**
 * {name} enum values (local — canonical export lives in ./{name})
 */
const {name}Values = [{variants}] as const;
type {name} = typeof {name}Values[number];
const {name_camel}Schema = z.enum({name}Values);
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
            matches!(&f.type_name, FieldType::Enum(name) if name == enum_name)
                || matches!(&f.type_name, FieldType::Custom(name) if name == enum_name)
                || matches!(&f.type_name, FieldType::Array(inner) if {
                    matches!(inner.as_ref(), FieldType::Enum(name) if name == enum_name) ||
                    matches!(inner.as_ref(), FieldType::Custom(name) if name == enum_name)
                })
        })
    }

    /// Check if entity has any IP address fields
    fn entity_uses_ip(&self, entity: &EntityDefinition) -> bool {
        entity.fields.iter().any(|f| {
            matches!(&f.type_name, FieldType::Ip)
                || matches!(&f.type_name, FieldType::Optional(inner) if {
                    matches!(inner.as_ref(), FieldType::Ip)
                })
                || matches!(&f.type_name, FieldType::Array(inner) if {
                    matches!(inner.as_ref(), FieldType::Ip) ||
                    matches!(inner.as_ref(), FieldType::Optional(opt_inner) if {
                        matches!(opt_inner.as_ref(), FieldType::Ip)
                    })
                })
        })
    }

    /// Generate base field schemas (all fields)
    fn generate_base_field_schemas(
        &self,
        entity: &EntityDefinition,
        enums: &[EnumDefinition],
    ) -> String {
        let mut fields = Vec::new();

        for field in &entity.fields {
            let schema = describing(field, self.type_mapper.to_zod_schema(field, enums));

            // Add comment if description exists
            if let Some(desc) = &field.description {
                fields.push(format!("  /** {} */", desc));
            }

            fields.push(format!("  {}: {},", field.name, schema));
        }

        fields.join("\n")
    }

    /// Generate create field schemas (exclude auto-generated)
    fn generate_create_field_schemas(
        &self,
        entity: &EntityDefinition,
        enums: &[EnumDefinition],
    ) -> String {
        let mut fields = Vec::new();

        for field in &entity.fields {
            // Skip auto-generated fields
            if self.is_auto_generated_field(field) {
                continue;
            }

            let schema = describing(field, self.type_mapper.to_zod_schema(field, enums));
            fields.push(format!("  {}: {},", field.name, schema));
        }

        fields.join("\n")
    }

    /// Generate update field schemas (all optional except those that shouldn't be)
    fn generate_update_field_schemas(
        &self,
        entity: &EntityDefinition,
        enums: &[EnumDefinition],
    ) -> String {
        let mut fields = Vec::new();

        for field in &entity.fields {
            // Skip id (handled separately) and auto-generated timestamp fields
            if field.name == "id" || self.is_timestamp_field(field) {
                continue;
            }

            let base_schema = self.type_mapper.to_zod_schema(field, enums);
            // Make all update fields optional
            let optional_schema = describing(field, format!("{}.optional()", base_schema));
            fields.push(format!("  {}: {},", field.name, optional_schema));
        }

        fields.join("\n")
    }

    /// Emit what each reference field points at, where the schema says so.
    ///
    /// A `*_id` names its target only by convention, and the convention breaks on
    /// every alias (`reverses_id`, `matched_source_id`) and every reference into
    /// another module. The schema records the real target in the field's note, in
    /// prose — which no consumer can read. This turns the ones stated confidently
    /// into data, and stays silent about the rest.
    fn generate_relation_targets(&self, entity: &EntityDefinition) -> String {
        let entries: Vec<String> = entity
            .fields
            .iter()
            .filter_map(|f| {
                let target = target_of(f)?;
                Some(format!(
                    "  {}: '{}',",
                    f.name,
                    escape_single_quoted(&target)
                ))
            })
            .collect();
        if entries.is_empty() {
            return String::new();
        }
        format!(
            r#"
// ============================================================================
// Relation Targets
// ============================================================================

/**
 * The entity each reference field points at, as recorded in the schema.
 *
 * Only the references the schema states outright are here — a field whose target
 * is left to its name is absent, and its consumer falls back to reading the name.
 */
export const {entity_camel}RelationTargets = {{
{entries}
}} as const;
"#,
            entity_camel = to_camel_case(&entity.name),
            entries = entries.join(
                "
"
            ),
        )
    }

    /// Generate filter field schemas for query parameters
    fn generate_filter_field_schemas(
        &self,
        entity: &EntityDefinition,
        enums: &[EnumDefinition],
    ) -> String {
        let mut fields = Vec::new();

        for field in &entity.fields {
            // Skip complex types for simple filters
            if matches!(field.type_name, FieldType::Json | FieldType::Text) {
                continue;
            }

            // Skip sensitive fields
            if field.name.contains("password")
                || field.name.contains("hash")
                || field.name.contains("token")
            {
                continue;
            }

            let filter_schema = self.generate_filter_schema_for_field(field, enums);
            fields.push(format!("  {}: {},", field.name, filter_schema));
        }

        fields.join("\n")
    }

    /// Generate filter schema for a specific field
    #[allow(clippy::only_used_in_recursion)]
    fn generate_filter_schema_for_field(
        &self,
        field: &FieldDefinition,
        enums: &[EnumDefinition],
    ) -> String {
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
        name == "created_at"
            || name == "createdat"
            || name == "updated_at"
            || name == "updatedat"
            || name == "deleted_at"
            || name == "deletedat"
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

/// The reader-facing half of a field description.
///
/// A schema description is written for two audiences at once: the sentence that
/// explains the field, and — after a ` # ` — a note to whoever maintains the
/// schema ("logical FK to organization.Company.id", an ADR reference, "in-module").
/// Only the first half means anything on a screen, so only the first half is
/// carried into the emitted schema. The full text stays in the doc comment above
/// the field, where the note was written to be read.
///
/// Returns None for a description that is nothing but a note, so nothing empty
/// is emitted.
fn field_hint(description: &str) -> Option<String> {
    let head = description
        .split(" # ")
        .next()
        .unwrap_or(description)
        .trim();
    // A description that is nothing but the note starts with the marker.
    if head.is_empty() || head.starts_with('#') {
        return None;
    }
    Some(head.to_string())
}

/// What a field points at, from the two places the schema can say so.
///
/// `@foreign_key(JournalLine.id)` is a declaration and is taken first: it is
/// structured, it is checked by the schema tooling, and it cannot drift from the
/// prose the way a note can. The note after a ` # ` is the fallback, for the many
/// logical references that carry no attribute because there is no SQL constraint
/// to declare — cross-module references, and everything the deployment leaves
/// unenforced.
fn target_of(field: &FieldDefinition) -> Option<String> {
    declared_target(field).or_else(|| field.description.as_deref().and_then(relation_target))
}

/// The target named by a `@foreign_key(...)` attribute.
///
/// The same attribute appears on a `relations:` entry naming the FK FIELD rather
/// than the target (`@foreign_key(user_id)`), so only a capitalised name is taken
/// as an entity — a lowercase one is a field name and means nothing here.
fn declared_target(field: &FieldDefinition) -> Option<String> {
    let arg = field
        .attributes
        .iter()
        .find(|a| a.name == "foreign_key")
        .and_then(|a| a.first_arg())?;
    let reference = arg.trim().trim_end_matches(".id");
    let last = reference.rsplit('.').next()?;
    if last.starts_with(|c: char| c.is_ascii_uppercase()) {
        Some(last.to_string())
    } else {
        None
    }
}

/// English plural → singular, enough for a table name (`approval_policies` →
/// `approval_policy`). A word that is already singular is returned unchanged.
fn singular(word: &str) -> String {
    if let Some(stem) = word.strip_suffix("ies") {
        return format!("{stem}y");
    }
    if let Some(stem) = word.strip_suffix("sses") {
        return format!("{stem}ss");
    }
    if let Some(stem) = word.strip_suffix("ses") {
        return format!("{stem}s");
    }
    word.strip_suffix('s').unwrap_or(word).to_string()
}

/// The entity a reference field points at, read out of the note a schema author
/// left after the ` # ` — "logical FK to organization.Company.id", "FK
/// approval_policies.id (in-module)".
///
/// Deliberately strict, because a wrong target sends a reader to the wrong record
/// and a missing one only costs a fallback. A reference has a recognisable shape:
/// it is qualified (`module.Entity`), written as an entity (`PascalCase`), or names
/// a table (a plural). Prose after the marker — "FK to the workflow that triggered
/// it" — has none of those, and returns None.
fn relation_target(description: &str) -> Option<String> {
    let (_, note) = description.split_once('#')?;
    let mut words = note
        .split_whitespace()
        .skip_while(|w| !w.eq_ignore_ascii_case("FK"));
    words.next()?; // the "FK" itself
    let mut token = words.next()?;
    if token.eq_ignore_ascii_case("to") {
        token = words.next()?;
    }
    let reference = token
        .trim_end_matches([',', '.', ';', ':', ')', '(', '"'])
        .trim_end_matches(".id")
        .trim_end_matches(".Id")
        .trim_end_matches('.');
    let last = reference.rsplit('.').next()?;
    if last.is_empty() {
        return None;
    }
    if last.starts_with(|c: char| c.is_ascii_uppercase()) {
        return Some(last.to_string());
    }
    // A lowercase token only counts when its shape says "reference": qualified by
    // a module, or plural like a table name.
    if reference.contains('.') || last.ends_with('s') {
        return Some(to_pascal_case(&singular(last)));
    }
    None
}

/// Attach a field's description to its Zod schema, so the explanation the schema
/// already carries survives compilation and can be shown beside the field.
///
/// A doc comment cannot: it is stripped with the types, leaving every generated
/// consumer to re-invent an explanation the schema author already wrote.
/// `.describe()` makes the same prose data.
fn describing(field: &FieldDefinition, schema: String) -> String {
    match field.description.as_deref().and_then(field_hint) {
        Some(hint) => format!("{}.describe('{}')", schema, escape_single_quoted(&hint)),
        None => schema,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webgen::ast::entity::FieldAttribute;

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

    fn described(name: &str, description: Option<&str>) -> FieldDefinition {
        FieldDefinition {
            name: name.to_string(),
            type_name: FieldType::String,
            attributes: vec![],
            description: description.map(str::to_string),
            optional: false,
            default_value: None,
        }
    }

    fn with_fk(name: &str, arg: &str, description: Option<&str>) -> FieldDefinition {
        let mut f = described(name, description);
        f.attributes = vec![FieldAttribute {
            name: "foreign_key".to_string(),
            args: vec![arg.to_string()],
        }];
        f
    }

    #[test]
    fn a_declared_foreign_key_names_the_target() {
        assert_eq!(
            target_of(&with_fk(
                "debit_move_id",
                "JournalLine.id",
                Some("Debit side of the edge")
            ))
            .as_deref(),
            Some("JournalLine")
        );
        // A qualified declaration resolves to the entity, not the module.
        assert_eq!(
            target_of(&with_fk("user_id", "sapiens.User.id", None)).as_deref(),
            Some("User")
        );
        // On a relations entry the attribute names the FK FIELD; that is not an entity.
        assert_eq!(target_of(&with_fk("parent", "parent_id", None)), None);
    }

    #[test]
    fn a_declaration_is_preferred_over_the_note() {
        // Both present and disagreeing: the declaration is the checked one.
        let f = with_fk(
            "reward_item_id",
            "Item.id",
            Some("Free reward item # logical FK to catalog.Product.id"),
        );
        assert_eq!(target_of(&f).as_deref(), Some("Item"));
        // With no declaration the note still answers.
        assert_eq!(
            target_of(&described(
                "company_id",
                Some("Owner # logical FK to organization.Company.id")
            ))
            .as_deref(),
            Some("Company")
        );
    }

    #[test]
    fn field_hint_keeps_the_sentence_and_drops_the_schema_note() {
        assert_eq!(
            field_hint("Legal entity that owns this Chart of Accounts # logical FK to organization.Company.id"),
            Some("Legal entity that owns this Chart of Accounts".to_string())
        );
        // A description with nothing but a note explains nothing to a reader.
        assert_eq!(field_hint("# FK employees.id"), None);
        assert_eq!(field_hint("   "), None);
        // Prose that merely contains a hash is not a note.
        assert_eq!(
            field_hint("Invoice number (e.g. #INV-001)"),
            Some("Invoice number (e.g. #INV-001)".to_string())
        );
    }

    #[test]
    fn a_described_field_carries_its_explanation_into_the_schema() {
        let field = described(
            "is_header",
            Some("Header account (cannot post transactions directly)"),
        );
        assert_eq!(
            describing(&field, "z.string()".to_string()),
            "z.string().describe('Header account (cannot post transactions directly)')"
        );

        // An undescribed field is emitted exactly as before.
        assert_eq!(
            describing(&described("name", None), "z.string()".to_string()),
            "z.string()"
        );
    }

    #[test]
    fn a_description_with_an_apostrophe_stays_one_string_literal() {
        let field = described("owner_id", Some("The kiosk's own reader"));
        assert_eq!(
            describing(&field, "z.string()".to_string()),
            "z.string().describe('The kiosk\\'s own reader')"
        );
    }

    #[test]
    fn relation_target_reads_the_reference_the_schema_states() {
        // Every shape the schemas actually use.
        assert_eq!(
            relation_target("Legal entity # logical FK to organization.Company.id").as_deref(),
            Some("Company")
        );
        assert_eq!(
            relation_target("# logical FK employee.Employee.id (self-ref)").as_deref(),
            Some("Employee")
        );
        assert_eq!(
            relation_target("# FK approval_policies.id (in-module)").as_deref(),
            Some("ApprovalPolicy")
        );
        assert_eq!(
            relation_target("# FK employees.id").as_deref(),
            Some("Employee")
        );
        assert_eq!(
            relation_target("Project dimension # logical FK to projects.Project").as_deref(),
            Some("Project")
        );
        // A hyphenated module qualifier still resolves to the entity.
        assert_eq!(
            relation_target("# logical FK to backbone-sapiens.User.id").as_deref(),
            Some("User")
        );
    }

    #[test]
    fn relation_target_stays_silent_on_prose_and_on_silence() {
        // Prose after the marker names no reference — sending a reader to an entity
        // called "The" is worse than falling back to reading the field name.
        assert_eq!(
            relation_target("The thing being approved # logical FK to the workflow"),
            None
        );
        assert_eq!(relation_target("Parent account for hierarchy"), None);
        assert_eq!(relation_target("Source transaction ID"), None);
        assert_eq!(relation_target("# see the ADR"), None);
    }

    #[test]
    fn an_entity_with_no_stated_reference_gets_no_map() {
        let generator = EntitySchemaGenerator::new(test_config(), TypeMapper::new());
        let bare = EntityDefinition {
            name: "Country".to_string(),
            collection: "countries".to_string(),
            fields: vec![described("name", Some("Country name"))],
            relations: vec![],
            indexes: vec![],
            soft_delete: false,
        };
        assert_eq!(generator.generate_relation_targets(&bare), "");

        let referring = EntityDefinition {
            name: "Account".to_string(),
            collection: "accounts".to_string(),
            fields: vec![
                described(
                    "company_id",
                    Some("Owner # logical FK to organization.Company.id"),
                ),
                described("parent_id", Some("Parent account for hierarchy")),
            ],
            relations: vec![],
            indexes: vec![],
            soft_delete: false,
        };
        let map = generator.generate_relation_targets(&referring);
        assert!(map.contains("export const accountRelationTargets = {"));
        assert!(map.contains("  company_id: 'Company',"));
        // The field whose target is only implied by its name is left out.
        assert!(!map.contains("parent_id"));
    }

    #[test]
    fn every_schema_a_consumer_introspects_carries_the_description() {
        let generator = EntitySchemaGenerator::new(test_config(), TypeMapper::new());
        let entity = EntityDefinition {
            name: "Province".to_string(),
            collection: "provinces".to_string(),
            fields: vec![described("name", Some("Province name"))],
            relations: vec![],
            indexes: vec![],
            soft_delete: false,
        };

        // A form derives its fields from the create schema and a record screen
        // from the update schema, so a description on the base schema alone would
        // never reach either of them.
        for body in [
            generator.generate_base_field_schemas(&entity, &[]),
            generator.generate_create_field_schemas(&entity, &[]),
            generator.generate_update_field_schemas(&entity, &[]),
        ] {
            assert!(
                body.contains(".describe('Province name')"),
                "missing in: {body}"
            );
        }
    }
}
