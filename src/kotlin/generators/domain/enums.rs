//! Enum (sealed class) generator

use crate::kotlin::error::{MobileGenError, Result};
use crate::kotlin::generators::GenerationResult;
use crate::kotlin::generators::MobileGenerator;
use crate::kotlin::generators::write_generated_file;
use crate::ast::{EnumDef, ModuleSchema};
use crate::kotlin::lang::KotlinNaming;

/// Generate sealed class enums for all enum definitions in a schema
pub fn generate_enums(
    generator: &MobileGenerator,
    schema: &ModuleSchema,
    output_dir: &std::path::Path,
) -> Result<GenerationResult> {
    let mut result = GenerationResult::default();

    for enum_def in &schema.enums {
        match generate_enum(generator, enum_def, &schema.name, output_dir) {
            Ok(Some(path)) => {
                result.generated_files.push(path);
                result.enums_count += 1;
            }
            Ok(None) => {
                result.skipped_files.push(enum_def.name.clone().into());
            }
            Err(e) => return Err(e),
        }
    }

    // Emit the `Metadata` typealias helper into the module's enums package.
    // Entities and mappers reference a `metadata: Metadata` field and import it
    // from `<base>.<module>.domain.enums.Metadata`, so the generated tree must
    // be self-contained and provide it (it is not a schema enum).
    if let Some(path) = generate_metadata_typealias(generator, &schema.name, output_dir)? {
        result.generated_files.push(path);
    }

    Ok(result)
}

/// Generate the `Metadata` typealias (`Map<String, JsonElement?>`) used by every
/// entity's audit `metadata` field. One per module, in the enums package.
fn generate_metadata_typealias(
    generator: &MobileGenerator,
    module_name: &str,
    output_dir: &std::path::Path,
) -> Result<Option<std::path::PathBuf>> {
    let module_lower = module_name.to_lowercase();
    let package_name = format!("{}.{}.domain.enums", generator.package_name, module_lower);
    let content = format!(
        "package {package}\n\nimport kotlinx.serialization.json.JsonElement\n\n\
/**\n * Metadata type for entity audit fields (created_at, updated_at, deleted_at, ...).\n \
* Typealias over Map to support bracket-access notation.\n *\n * Generated from Backbone schema\n */\n\
typealias Metadata = Map<String, JsonElement?>\n",
        package = package_name
    );
    let relative_path = format!("{}/domain/enums/Metadata.kt", module_name);
    match write_generated_file(output_dir, &generator.package_name, &relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(path) => Ok(Some(path)),
        crate::kotlin::generators::WriteOutcome::Skipped(_) => Ok(None),
    }
}

/// Generate a single sealed class enum
fn generate_enum(
    generator: &MobileGenerator,
    enum_def: &EnumDef,
    module_name: &str,
    output_dir: &std::path::Path,
) -> Result<Option<std::path::PathBuf>> {
    // Get package from generator and format for enum layer
    // Format: {base_package}.domain.{module}.enums
    let module_lower = module_name.to_lowercase();
    let package_name = format!("{}.{}.domain.enums", generator.package_name, module_lower);

    // Prepare template data - convert snake_case to Title Case for display name
    let variants: Vec<EnumVariantData> = enum_def
        .variants
        .iter()
        .map(|v| {
            // Convert snake_case to Title Case for display name
            let display_name = v
                .name
                .split('_')
                .map(|s| {
                    let mut chars = s.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(first) => {
                            first.to_uppercase().collect::<String>() + chars.as_str()
                        }
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");

            // Convert i32 value to String for the template
            let value_str = v.value.map(|i| i.to_string());

            EnumVariantData {
                name: KotlinNaming::to_pascal_case(&v.name),
                original_name: v.name.clone(),
                display_name,
                value: value_str,
            }
        })
        .collect();

    let enum_data = EnumData {
        base_package: generator.package_name.clone(),
        package: package_name,
        name: enum_def.name.clone(),
        variants,
    };

    // Render the template
    let content = generator
        .handlebars
        .render("enum", &enum_data)
        .map_err(|e| MobileGenError::template(format!("Enum template error: {}", e)))?;

    // Create output path: domain/{module}/enums/{EnumName}.kt
    let relative_path = format!(
        "{}/domain/enums/{}.kt",
        module_name,
        enum_def.name
    );

    match write_generated_file(output_dir, &generator.package_name, &relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(path) => Ok(Some(path)),
        crate::kotlin::generators::WriteOutcome::Skipped(_) => Ok(None),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct EnumData {
    base_package: String,
    package: String,
    name: String,
    variants: Vec<EnumVariantData>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct EnumVariantData {
    name: String,
    original_name: String,
    display_name: String,
    value: Option<String>,
}
