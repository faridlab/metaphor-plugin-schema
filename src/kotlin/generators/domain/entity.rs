//! Entity (data class) generator

use crate::kotlin::error::{MobileGenError, Result};
use crate::kotlin::generators::GenerationResult;
use crate::kotlin::generators::MobileGenerator;
use crate::kotlin::generators::write_generated_file;
use crate::ast::{Field, Model, ModuleSchema};
use crate::ast::model::AttributeValue;
use std::path::{Path, PathBuf};

/// Context data for entity template rendering
#[derive(Debug, Clone, serde::Serialize)]
pub struct EntityData {
    /// Base package name (e.g., "com.bersihir")
    pub base_package: String,
    /// Package name
    pub package: String,
    /// Entity name (PascalCase)
    pub name: String,
    /// Collection name (snake_case plural)
    pub collection: String,
    /// Entity fields
    pub fields: Vec<FieldData>,
    /// Entity imports
    pub imports: Vec<String>,
    /// Whether this entity has soft delete
    pub has_soft_delete: bool,
    /// Whether this entity has soft delete AND has a metadata field
    pub has_soft_delete_with_metadata: bool,
    /// Whether this entity has any Map<String, Any?> fields
    pub has_map_any: bool,
    /// Primary key field name
    pub primary_key: Option<String>,
    /// Entity description
    pub description: Option<String>,
}

/// Field data for template rendering
#[derive(Debug, Clone, serde::Serialize)]
pub struct FieldData {
    /// Field name (camelCase)
    pub name: String,
    /// Original field name (snake_case)
    pub original_name: String,
    /// Kotlin type
    pub kotlin_type: String,
    /// Whether field is nullable
    pub is_nullable: bool,
    /// Whether field is the primary key
    pub is_primary_key: bool,
    /// Whether field has default value
    pub has_default: bool,
    /// Default value as string
    pub default_value: Option<String>,
    /// Field description
    pub description: Option<String>,
    /// Whether field contains sensitive data (passwords, tokens, secrets)
    pub is_sensitive: bool,
    /// Whether the Kotlin type contains a Map (for @Contextual annotation)
    pub kotlin_type_contains_map: bool,
    /// Whether @SerialName annotation is needed (name differs from original_name)
    pub name_needs_serial_name: bool,
}

impl EntityData {
    /// Create entity data from a model
    pub fn from_model(
        generator: &MobileGenerator,
        model: &Model,
        package: &str,
        base_package: &str,
    ) -> Result<Self> {
        let fields = model
            .fields
            .iter()
            .map(|f| FieldData::from_field(generator, f))
            .collect::<Result<Vec<_>>>()?;

        // Get primary key field name (Kotlin property name)
        let primary_key = model.primary_key().map(|f| generator.type_mapper.to_kotlin_property_name(&f.name));

        // Collect imports needed
        let mut imports: Vec<String> = vec![];
        let needs_contextual = fields.iter().any(|f| f.kotlin_type_contains_map);

        if needs_contextual {
            imports.push("kotlinx.serialization.Contextual".to_string());
        }

        for field in &fields {
            // Check for datetime types - strip nullable suffix for comparison
            let base_type = field.kotlin_type.trim_end_matches('?');

            if base_type == "Instant" || base_type == "Instant?" {
                if !imports.iter().any(|i| i.contains("kotlinx.datetime.Instant")) {
                    imports.push("kotlinx.datetime.Instant".to_string());
                }
            } else if base_type == "LocalDate" || base_type == "LocalDate?" {
                if !imports.iter().any(|i| i.contains("kotlinx.datetime.LocalDate")) {
                    imports.push("kotlinx.datetime.LocalDate".to_string());
                }
            } else if base_type == "LocalTime" || base_type == "LocalTime?" {
                #[allow(clippy::collapsible_if)]
                if !imports.iter().any(|i| i.contains("kotlinx.datetime.LocalTime")) {
                    imports.push("kotlinx.datetime.LocalTime".to_string());
                }
            }

            // Check for custom enum types - types that start with uppercase and aren't built-in
            // Custom enums are generated in the enums subdirectory of the same module
            if Self::is_custom_enum_type(&field.kotlin_type) {
                // Get the base type name (strip nullable and generic parameters)
                let type_name = field.kotlin_type
                    .trim_end_matches('?')
                    .split('<')
                    .next()
                    .unwrap_or(&field.kotlin_type);

                // Skip built-in Kotlin types and kotlinx types
                if !Self::is_builtin_type(type_name) {
                    // Import from the enums subdirectory
                    // Package format: {base_package}.domain.{module}.enums
                    // We need to extract the module name (second to last element)
                    let parts: Vec<&str> = package.split('.').collect();
                    let module_name = if parts.len() >= 4 {
                        // Get the second-to-last element (module name before 'entity')
                        parts.get(parts.len() - 2).copied().unwrap_or("common")
                    } else {
                        "common"
                    };
                    let enum_import = format!("{}.domain.{}.enums.{}", base_package, module_name, type_name);
                    if !imports.iter().any(|i| i.ends_with(type_name)) {
                        imports.push(enum_import);
                    }
                }
            }
        }

        // Check if entity has a metadata field
        let has_metadata_field = fields.iter().any(|f| f.original_name == "metadata");
        // Check if any field uses Map<String, Any?>
        let has_map_any = fields.iter().any(|f| f.kotlin_type.contains("Map<String, Any?>"));

        Ok(Self {
            base_package: base_package.to_string(),
            package: package.to_string(),
            name: model.name.clone(),
            collection: model.collection_name(),
            fields,
            imports: {
                use indexmap::IndexSet;
                let set: IndexSet<_> = imports.into_iter().collect();
                set.into_iter().collect()
            },
            has_soft_delete: model.has_soft_delete(),
            has_soft_delete_with_metadata: model.has_soft_delete() && has_metadata_field,
            has_map_any,
            primary_key,
            description: None,
        })
    }

    /// Create entity data from a model with base package
    pub fn from_model_with_base(
        generator: &MobileGenerator,
        model: &Model,
        package: &str,
        base_package: &str,
    ) -> Result<Self> {
        Self::from_model(generator, model, package, base_package)
    }

    /// Check if a type name is a custom enum type
    /// Custom enums start with uppercase and aren't built-in types
    fn is_custom_enum_type(kotlin_type: &str) -> bool {
        let base_type = kotlin_type.trim_end_matches('?').split('<').next().unwrap_or(kotlin_type);

        // Must start with uppercase letter
        let first_char = base_type.chars().next();
        match first_char {
            Some(c) if c.is_uppercase() => !Self::is_builtin_type(base_type),
            _ => false,
        }
    }

    /// Check if a type name is a built-in Kotlin/stdlib type
    fn is_builtin_type(type_name: &str) -> bool {
        matches!(
            type_name,
            "String" | "Int" | "Long" | "Double" | "Float" |
            "Boolean" | "ByteArray" | "Unit" | "Any" |
            "List" | "Map" | "Set" | "Collection" |
            "Instant" | "LocalDate" | "LocalTime" | "Duration" |
            "UUID" | "Pair" | "Triple" | "Array" |
            "CharSequence" | "Number" | "Comparable" |
            "Enum" | "Throwable" | "Nothing" |
            "JsonElement" | "JsonObject" | "JsonArray" | "JsonPrimitive"
        )
    }

    /// Get the relative file path for this entity
    /// Outputs to: domain/{module}/entity/{Entity}.kt
    pub fn relative_path(&self, module_name: &str) -> String {
        format!(
            "domain/{}/entity/{}.kt",
            module_name,
            self.name
        )
    }

    /// Get the file path for this entity
    /// Outputs to: domain/{module}/entity/{Entity}.kt
    #[deprecated(note = "Use relative_path() with write_generated_file() instead")]
    pub fn file_path(&self, base_dir: &Path, module_name: &str) -> PathBuf {
        base_dir.join(self.relative_path(module_name))
    }
}

impl FieldData {
    /// Create field data from a schema field
    pub fn from_field(generator: &MobileGenerator, field: &Field) -> Result<Self> {
        let kotlin_type = generator.type_mapper.to_kotlin_field_type(field);
        let is_nullable = field.type_ref.is_optional();
        let kotlin_type_contains_map = kotlin_type.contains("Map<");

        let default_value = match field.default_value() {
            Some(AttributeValue::String(s)) => Some(format!("\"{}\"", s)),
            Some(AttributeValue::Int(i)) => Some(i.to_string()),
            Some(AttributeValue::Float(f)) => Some(f.to_string()),
            Some(AttributeValue::Bool(b)) => Some(b.to_string()),
            Some(AttributeValue::Ident(s)) => Some(s.clone()),
            Some(AttributeValue::Array(_)) => Some("emptyList()".to_string()),
            Some(AttributeValue::Object(_)) => Some("emptyMap()".to_string()),
            None => None,
        };

        let has_default = default_value.is_some();

        // Detect sensitive field names (passwords, tokens, secrets)
        let is_sensitive = is_sensitive_field(&field.name);

        // Convert to camelCase and check if @SerialName is needed
        let camel_case_name = generator.type_mapper.to_kotlin_property_name(&field.name);
        let name_needs_serial_name = camel_case_name != field.name;

        Ok(Self {
            name: camel_case_name,
            original_name: field.name.clone(),
            kotlin_type,
            is_nullable,
            is_primary_key: field.is_primary_key(),
            has_default,
            default_value,
            description: None,
            is_sensitive,
            kotlin_type_contains_map,
            name_needs_serial_name,
        })
    }
}

/// Check if a field name suggests sensitive data
/// Detects passwords, tokens, secrets, and similar sensitive fields
fn is_sensitive_field(name: &str) -> bool {
    let name_lower = name.to_lowercase();
    name_lower.contains("password")
        || name_lower.contains("token")
        || name_lower.contains("secret")
        || name_lower.contains("hash")
        || name_lower.contains("salt")
        || name_lower.contains("pin")
        || name_lower.contains("credential")
        || name_lower.contains("key")
        || name_lower.ends_with("_hash")
}

/// Generate entity data classes for all models in a schema
pub fn generate_entities(
    generator: &MobileGenerator,
    schema: &ModuleSchema,
    output_dir: &Path,
) -> Result<GenerationResult> {
    let mut result = GenerationResult::default();

    for model in &schema.models {
        if generator.is_disabled_for_model(model, crate::kotlin::config::GenerationTarget::Entities) {
            continue;
        }
        match generate_entity(generator, model, &schema.name, output_dir) {
            Ok(Some(path)) => {
                result.generated_files.push(path);
                result.entities_count += 1;
            }
            Ok(None) => {
                result.skipped_files.push(model.name.clone().into());
            }
            Err(e) => return Err(e),
        }
    }

    Ok(result)
}

/// Generate a single entity data class
fn generate_entity(
    generator: &MobileGenerator,
    model: &Model,
    module_name: &str,
    output_dir: &Path,
) -> Result<Option<PathBuf>> {
    // Get package from generator and format for entity layer
    // Format: {base_package}.domain.{module}.entity
    let module_lower = module_name.to_lowercase();
    let base_package = &generator.package_name;
    let package_name = format!("{}.domain.{}.entity", base_package, module_lower);
    let entity_data = EntityData::from_model_with_base(generator, model, &package_name, base_package)?;

    // Render the template
    let content = generator
        .handlebars
        .render("entity", &entity_data)
        .map_err(|e| MobileGenError::template(format!("Entity template error: {}", e)))?;

    // Write file using helper - use base package from generator
    let relative_path = entity_data.relative_path(module_name);
    match write_generated_file(output_dir, base_package, &relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(path) => Ok(Some(path)),
        crate::kotlin::generators::WriteOutcome::Skipped(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    include!("entity_test.rs");
}
