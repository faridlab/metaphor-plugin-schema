//! Application layer generators (use cases, services, validators, mappers)

use crate::kotlin::error::{MobileGenError, Result};
use crate::kotlin::generators::GenerationResult;
use crate::kotlin::generators::MobileGenerator;
use crate::kotlin::generators::write_generated_file;
use crate::ast::{Model, ModuleSchema, TypeRef};
use std::path::Path;

/// Generate use cases for all models in a schema
pub fn generate_usecases(
    generator: &MobileGenerator,
    schema: &ModuleSchema,
    output_dir: &Path,
) -> Result<GenerationResult> {
    let mut result = GenerationResult::default();

    for model in &schema.models {
        if generator.is_disabled_for_model(model, crate::kotlin::config::GenerationTarget::UseCases) {
            continue;
        }
        match generate_usecase(generator, model, &schema.name, output_dir) {
            Ok(Some(path)) => {
                result.generated_files.push(path);
                result.usecases_count += 1;
            }
            Ok(None) => {}
            Err(e) => return Err(e),
        }
    }

    Ok(result)
}

/// Generate application services for all models in a schema
pub fn generate_app_services(
    generator: &MobileGenerator,
    schema: &ModuleSchema,
    output_dir: &Path,
) -> Result<GenerationResult> {
    let mut result = GenerationResult::default();

    for model in &schema.models {
        if generator.is_disabled_for_model(model, crate::kotlin::config::GenerationTarget::AppServices) {
            continue;
        }
        match generate_app_service(generator, model, &schema.name, output_dir) {
            Ok(Some(path)) => {
                result.generated_files.push(path);
                result.services_count += 1;
            }
            Ok(None) => {}
            Err(e) => return Err(e),
        }
    }

    Ok(result)
}

/// Generate mappers for all models in a schema
pub fn generate_mappers(
    generator: &MobileGenerator,
    schema: &ModuleSchema,
    output_dir: &Path,
) -> Result<GenerationResult> {
    let mut result = GenerationResult::default();

    for model in &schema.models {
        if generator.is_disabled_for_model(model, crate::kotlin::config::GenerationTarget::Mappers) {
            continue;
        }
        match generate_mapper(generator, model, &schema.name, output_dir) {
            Ok(Some(path)) => {
                result.generated_files.push(path);
                result.mappers_count += 1;
            }
            Ok(None) => {}
            Err(e) => return Err(e),
        }
    }

    Ok(result)
}

/// Generate validators for all models in a schema
pub fn generate_validators(
    generator: &MobileGenerator,
    schema: &ModuleSchema,
    output_dir: &Path,
) -> Result<GenerationResult> {
    let mut result = GenerationResult::default();

    for model in &schema.models {
        if generator.is_disabled_for_model(model, crate::kotlin::config::GenerationTarget::Validators) {
            continue;
        }
        match generate_validator(generator, model, &schema.name, output_dir) {
            Ok(Some(path)) => {
                result.generated_files.push(path);
                result.validators_count += 1;
            }
            Ok(None) => {}
            Err(e) => return Err(e),
        }
    }

    Ok(result)
}

/// Generate use case classes for a single model
fn generate_usecase(
    generator: &MobileGenerator,
    model: &Model,
    module_name: &str,
    output_dir: &Path,
) -> Result<Option<std::path::PathBuf>> {
    // Use package from generator
    let base_package = &generator.package_name;
    let module_lower = module_name.to_lowercase();
    let entity_name = model.name.clone();
    let entity_name_lowercase = entity_name.to_lowercase();

    // Package for application/{module}/usecases
    let package_name = format!("{}.application.{}.usecases", base_package, module_lower);
    let entity_package = format!("{}.domain.{}.entity", base_package, module_lower);
    let repository_package = format!("{}.domain.{}.repository", base_package, module_lower);

    // Prepare template data
    let usecase_data = UseCaseData {
        base_package: base_package.clone(),
        package: package_name.clone(),
        entity_name: entity_name.clone(),
        entity_name_lowercase: entity_name_lowercase.clone(),
        entity_package,
        repository_package,
    };

    // Render the template
    let content = generator
        .handlebars
        .render("usecase", &usecase_data)
        .map_err(|e| MobileGenError::template(format!("UseCase template error: {}", e)))?;

    // Create output path: application/{module}/usecases/{Entity}UseCases.kt
    let relative_path = format!(
        "application/{}/usecases/{}UseCases.kt",
        module_name,
        entity_name
    );

    match write_generated_file(output_dir, base_package, &relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(path) => Ok(Some(path)),
        crate::kotlin::generators::WriteOutcome::Skipped(_) => Ok(None),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct UseCaseData {
    base_package: String,
    package: String,
    entity_name: String,
    entity_name_lowercase: String,
    entity_package: String,
    repository_package: String,
}

/// Generate application service for a single model
fn generate_app_service(
    generator: &MobileGenerator,
    model: &Model,
    module_name: &str,
    output_dir: &Path,
) -> Result<Option<std::path::PathBuf>> {
    // Use package from generator
    let base_package = &generator.package_name;
    let module_lower = module_name.to_lowercase();
    let entity_name = model.name.clone();
    let entity_name_lowercase = entity_name.to_lowercase();

    // Package for application/{module}/services
    let package_name = format!("{}.application.{}.services", base_package, module_lower);
    let application_base_package = format!("{}.application.{}", base_package, module_lower);
    let entity_package = format!("{}.domain.{}.entity", base_package, module_lower);
    let repository_package = format!("{}.domain.{}.repository", base_package, module_lower);

    // Prepare template data
    let service_data = AppServiceData {
        base_package: base_package.clone(),
        package: package_name.clone(),
        application_base_package,
        entity_name: entity_name.clone(),
        entity_name_lowercase: entity_name_lowercase.clone(),
        entity_package,
        repository_package,
    };

    // Render the template
    let content = generator
        .handlebars
        .render("app_service", &service_data)
        .map_err(|e| MobileGenError::template(format!("AppService template error: {}", e)))?;

    // Create output path: application/{module}/services/{Entity}Service.kt
    let relative_path = format!(
        "application/{}/services/{}Service.kt",
        module_name,
        entity_name
    );

    match write_generated_file(output_dir, base_package, &relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(path) => Ok(Some(path)),
        crate::kotlin::generators::WriteOutcome::Skipped(_) => Ok(None),
    }
}

/// Generate mapper for a single model
fn generate_mapper(
    generator: &MobileGenerator,
    model: &Model,
    module_name: &str,
    output_dir: &Path,
) -> Result<Option<std::path::PathBuf>> {
    // Use package from generator
    let base_package = &generator.package_name;
    let module_lower = module_name.to_lowercase();
    let entity_name = model.name.clone();
    let entity_name_lowercase = entity_name.to_lowercase();

    // Package for application/{module}/mappers
    let package_name = format!("{}.application.{}.mappers", base_package, module_lower);
    let entity_package = format!("{}.domain.{}.entity", base_package, module_lower);
    let enums_package = format!("{}.domain.{}.enums", base_package, module_lower);

    // Get fields for mapper and collect custom enum types + special type flags
    let mut enum_types = std::collections::BTreeSet::new();
    let mut needs_instant = false;
    let mut needs_local_date = false;
    let mut needs_json_element = false;
    let mut needs_metadata = false;

    let fields: Vec<FieldMappingData> = model.fields.iter()
        .map(|f| {
            // Collect custom enum types (TypeRef::Custom that aren't primitives)
            if let TypeRef::Custom(type_name) = &f.type_ref {
                let is_special = matches!(type_name.as_str(),
                    "String" | "Int" | "Long" | "Double" | "Boolean" | "Instant"
                    | "LocalDate" | "LocalTime" | "Duration" | "ByteArray" | "Unit"
                    | "Metadata" | "JsonElement" | "JsonObject" | "JsonArray"
                );
                if !is_special && !type_name.contains('.') {
                    enum_types.insert(type_name.clone());
                }
            }
            // Also check unwrapped optional types
            if let TypeRef::Optional(inner) = &f.type_ref {
                if let TypeRef::Custom(type_name) = inner.as_ref() {
                    let is_special = matches!(type_name.as_str(),
                        "String" | "Int" | "Long" | "Double" | "Boolean" | "Instant"
                        | "LocalDate" | "LocalTime" | "Duration" | "ByteArray" | "Unit"
                        | "Metadata" | "JsonElement" | "JsonObject" | "JsonArray"
                    );
                    if !is_special && !type_name.contains('.') {
                        enum_types.insert(type_name.clone());
                    }
                }
            }

            // Detect special types that need explicit imports.
            // Use the field-aware variants so `@audit_metadata` fields are seen as `Metadata`,
            // not `JsonElement` (the bare type mapper produces JsonElement for PrimitiveType::Json).
            let type_str = generator.type_mapper.to_kotlin_field_type(f);
            if type_str.contains("Instant") { needs_instant = true; }
            if type_str.contains("LocalDate") { needs_local_date = true; }
            if type_str.contains("JsonElement") || type_str.contains("JsonObject") || type_str.contains("JsonArray") {
                needs_json_element = true;
            }
            if type_str.contains("Metadata") { needs_metadata = true; }

            {
                let kt_type_non_nullable = generator.type_mapper.to_kotlin_field_type_non_nullable(f);
                let is_nullable = f.type_ref.is_optional();
                let default_val = form_default_value(&kt_type_non_nullable, is_nullable);
                let form_is_nullable = is_nullable || default_val == "null";
                FieldMappingData {
                    name: generator.type_mapper.to_kotlin_property_name(&f.name),
                    original_name: f.name.clone(),
                    kotlin_type: generator.type_mapper.to_kotlin_field_type(f),
                    kotlin_type_non_nullable: kt_type_non_nullable,
                    is_nullable,
                    form_is_nullable,
                    is_primary_key: f.is_primary_key(),
                    form_default_value: default_val,
                }
            }
        })
        .collect();

    // Enum imports — exclude Metadata (handled by needs_metadata flag)
    let enum_imports: Vec<String> = enum_types.into_iter()
        .filter(|t| t != "Metadata")
        .collect();

    // Prepare template data
    let mapper_data = MapperData {
        base_package: base_package.clone(),
        package: package_name.clone(),
        entity_name: entity_name.clone(),
        entity_name_lowercase: entity_name_lowercase.clone(),
        entity_package,
        enums_package,
        enum_imports,
        needs_instant,
        needs_local_date,
        needs_json_element,
        needs_metadata,
        fields,
    };

    // Render the template
    let content = generator
        .handlebars
        .render("mapper", &mapper_data)
        .map_err(|e| MobileGenError::template(format!("Mapper template error: {}", e)))?;

    // Create output path: application/{module}/mappers/{Entity}Mapper.kt
    let relative_path = format!(
        "application/{}/mappers/{}Mapper.kt",
        module_name,
        entity_name
    );

    match write_generated_file(output_dir, base_package, &relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(path) => Ok(Some(path)),
        crate::kotlin::generators::WriteOutcome::Skipped(_) => Ok(None),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct AppServiceData {
    base_package: String,
    package: String,
    application_base_package: String,
    entity_name: String,
    entity_name_lowercase: String,
    entity_package: String,
    repository_package: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct MapperData {
    base_package: String,
    package: String,
    entity_name: String,
    entity_name_lowercase: String,
    entity_package: String,
    enums_package: String,
    enum_imports: Vec<String>,
    #[serde(skip_serializing_if = "is_not")]
    needs_instant: bool,
    #[serde(skip_serializing_if = "is_not")]
    needs_local_date: bool,
    #[serde(skip_serializing_if = "is_not")]
    needs_json_element: bool,
    #[serde(skip_serializing_if = "is_not")]
    needs_metadata: bool,
    fields: Vec<FieldMappingData>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct FieldMappingData {
    name: String,
    original_name: String,
    kotlin_type: String,
    kotlin_type_non_nullable: String,
    is_nullable: bool,
    /// True when the FormData field must be nullable (schema optional OR default is null)
    form_is_nullable: bool,
    is_primary_key: bool,
    /// Default value expression used in FormData data class parameter
    form_default_value: String,
}

/// Generate validator for a single model
fn generate_validator(
    generator: &MobileGenerator,
    model: &Model,
    module_name: &str,
    output_dir: &Path,
) -> Result<Option<std::path::PathBuf>> {
    // Use package from generator
    let base_package = &generator.package_name;
    let module_lower = module_name.to_lowercase();
    let entity_name = model.name.clone();
    let entity_name_lowercase = entity_name.to_lowercase();

    // Package for application/{module}/validators
    let package_name = format!("{}.application.{}.validators", base_package, module_lower);
    let mapper_package = format!("{}.application.{}.mappers", base_package, module_lower);
    let enums_package = format!("{}.domain.{}.enums", base_package, module_lower);

    // Collect custom enum types and check for special types that need imports
    let mut enum_types = std::collections::BTreeSet::new();
    let mut needs_instant = false;
    let mut needs_local_date = false;
    let mut needs_json_element = false;

    let fields: Vec<FieldMappingData> = model.fields.iter()
        .map(|f| {
            // Collect custom enum types (TypeRef::Custom that aren't primitives)
            if let TypeRef::Custom(type_name) = &f.type_ref {
                let is_special = matches!(type_name.as_str(),
                    "String" | "Int" | "Long" | "Double" | "Boolean" | "Instant"
                    | "LocalDate" | "LocalTime" | "Duration" | "ByteArray" | "Unit"
                    | "Metadata" | "JsonElement" | "JsonObject" | "JsonArray"
                );
                if !is_special && !type_name.contains('.') {
                    enum_types.insert(type_name.clone());
                }
            }
            // Also check unwrapped optional types
            if let TypeRef::Optional(inner) = &f.type_ref {
                if let TypeRef::Custom(type_name) = inner.as_ref() {
                    let is_special = matches!(type_name.as_str(),
                        "String" | "Int" | "Long" | "Double" | "Boolean" | "Instant"
                        | "LocalDate" | "LocalTime" | "Duration" | "ByteArray" | "Unit"
                        | "Metadata"
                    );
                    if !is_special && !type_name.contains('.') {
                        enum_types.insert(type_name.clone());
                    }
                }
            }

            // Check for special types needing imports.
            // Use the field-aware variant so `@audit_metadata` fields are recognized as `Metadata`.
            let type_str = generator.type_mapper.to_kotlin_field_type(f);
            if type_str.contains("Instant") {
                needs_instant = true;
            }
            if type_str.contains("LocalDate") {
                needs_local_date = true;
            }
            if type_str.contains("JsonElement") {
                needs_json_element = true;
            }

            // Check if Metadata is used (it conflicts with kotlin.Metadata)
            if type_str.contains("Metadata") {
                // Add Metadata to enum_types which will be imported specially
                enum_types.insert("Metadata".to_string());
            }

            {
                let kt_type_non_nullable = generator.type_mapper.to_kotlin_field_type_non_nullable(f);
                let is_nullable = f.type_ref.is_optional();
                let default_val = form_default_value(&kt_type_non_nullable, is_nullable);
                let form_is_nullable = is_nullable || default_val == "null";
                FieldMappingData {
                    name: generator.type_mapper.to_kotlin_property_name(&f.name),
                    original_name: f.name.clone(),
                    kotlin_type: generator.type_mapper.to_kotlin_field_type(f),
                    kotlin_type_non_nullable: kt_type_non_nullable,
                    is_nullable,
                    form_is_nullable,
                    is_primary_key: f.is_primary_key(),
                    form_default_value: default_val,
                }
            }
        })
        .collect();

    // Check if Metadata import is needed
    let needs_metadata = enum_types.contains("Metadata");

    // Convert to sorted Vec for consistent imports (excluding Metadata which is imported separately)
    let enum_imports: Vec<String> = enum_types.clone().into_iter()
        .filter(|t| t != "Metadata")
        .collect();

    // Prepare template data
    let validator_data = ValidatorData {
        base_package: base_package.clone(),
        package: package_name.clone(),
        entity_name: entity_name.clone(),
        entity_name_lowercase: entity_name_lowercase.clone(),
        mapper_package,
        enums_package,
        enum_imports,
        needs_instant,
        needs_local_date,
        needs_metadata,
        needs_json_element,
        fields,
    };

    // Render the template
    let content = generator
        .handlebars
        .render("validator", &validator_data)
        .map_err(|e| MobileGenError::template(format!("Validator template error: {}", e)))?;

    // Create output path: application/{module}/validators/{Entity}Validator.kt
    let relative_path = format!(
        "application/{}/validators/{}Validator.kt",
        module_name,
        entity_name
    );

    match write_generated_file(output_dir, base_package, &relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(path) => Ok(Some(path)),
        crate::kotlin::generators::WriteOutcome::Skipped(_) => Ok(None),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct ValidatorData {
    base_package: String,
    package: String,
    entity_name: String,
    entity_name_lowercase: String,
    mapper_package: String,
    enums_package: String,
    enum_imports: Vec<String>,
    #[serde(skip_serializing_if = "is_not")]
    needs_instant: bool,
    #[serde(skip_serializing_if = "is_not")]
    needs_local_date: bool,
    #[serde(skip_serializing_if = "is_not")]
    needs_metadata: bool,
    #[serde(skip_serializing_if = "is_not")]
    needs_json_element: bool,
    fields: Vec<FieldMappingData>,
}

/// Helper for serde skip_serializing_if
fn is_not(b: &bool) -> bool {
    !b
}

/// Compute the default value expression for a FormData field based on Kotlin type
fn form_default_value(kotlin_type_non_nullable: &str, is_nullable: bool) -> String {
    if is_nullable {
        return "null".to_string();
    }
    match kotlin_type_non_nullable {
        "String" => r#""""#.to_string(),
        "Int" | "Long" => "0".to_string(),
        "Double" | "Float" => "0.0".to_string(),
        "Boolean" => "false".to_string(),
        t if t.starts_with("List<") => "emptyList()".to_string(),
        t if t.starts_with("Map<") => "emptyMap()".to_string(),
        _ => "null".to_string(),
    }
}
