//! Infrastructure layer generators (API clients, database, sync)

use crate::kotlin::error::{MobileGenError, Result};
use serde::Serialize;
use crate::kotlin::generators::GenerationResult;
use crate::kotlin::generators::MobileGenerator;
use crate::kotlin::generators::write_generated_file;
use crate::ast::{Model, ModuleSchema};
use std::path::Path;

/// Generate Ktor API clients for all models in a schema
pub fn generate_api_clients(
    generator: &MobileGenerator,
    schema: &ModuleSchema,
    output_dir: &Path,
) -> Result<GenerationResult> {
    let mut result = GenerationResult::default();

    for model in &schema.models {
        if generator.is_disabled_for_model(model, crate::kotlin::config::GenerationTarget::ApiClients) {
            continue;
        }
        match generate_api_client(generator, model, &schema.name, output_dir) {
            Ok(Some(path)) => {
                result.generated_files.push(path);
                result.api_clients_count += 1;
            }
            Ok(None) => {
                result.skipped_files.push(format!("{}ApiClient.kt", model.name).into());
            }
            Err(e) => return Err(e),
        }
    }

    Ok(result)
}

/// Generate SQLDelight database schemas for all models in a schema
pub fn generate_database(
    generator: &MobileGenerator,
    schema: &ModuleSchema,
    output_dir: &Path,
) -> Result<GenerationResult> {
    let mut result = GenerationResult::default();

    for model in &schema.models {
        if generator.is_disabled_for_model(model, crate::kotlin::config::GenerationTarget::Database) {
            continue;
        }
        match generate_sqldelight_schema(generator, model, &schema.name, output_dir) {
            Ok(Some(path)) => {
                result.generated_files.push(path);
            }
            Ok(None) => {}
            Err(e) => return Err(e),
        }
    }

    Ok(result)
}

/// Generate offline sync handlers for all models in a schema.
///
/// Produces one `XxxSyncHandler.kt` per entity (in `infrastructure/{module}/sync/`).
/// Models with `generators.disabled: [sync]` (or not in `generators.enabled`) are skipped.
pub fn generate_sync(
    generator: &MobileGenerator,
    schema: &ModuleSchema,
    output_dir: &Path,
) -> Result<GenerationResult> {
    let mut result = GenerationResult::default();

    for model in &schema.models {
        if generator.is_disabled_for_model(model, crate::kotlin::config::GenerationTarget::Sync) {
            continue;
        }
        match generate_sync_handler(generator, model, &schema.name, output_dir) {
            Ok(Some(path)) => {
                result.generated_files.push(path);
            }
            Ok(None) => {
                result.skipped_files.push(
                    format!("{}SyncHandler.kt", model.name).into(),
                );
            }
            Err(e) => return Err(e),
        }
    }

    Ok(result)
}

/// Generate a sync handler for a single model.
fn generate_sync_handler(
    generator: &MobileGenerator,
    model: &Model,
    module_name: &str,
    output_dir: &Path,
) -> Result<Option<std::path::PathBuf>> {
    let base_package = &generator.package_name;
    let module_lower = module_name.to_lowercase();
    let entity_name = model.name.clone();
    let entity_name_lowercase = entity_name.to_lowercase();
    let collection = model.collection_name();

    let package = format!("{}.infrastructure.{}.sync", base_package, module_lower);
    let entity_package = format!("{}.domain.{}.entity", base_package, module_lower);
    let mapper_package = format!("{}.application.{}.mappers", base_package, module_lower);
    let api_package = format!("{}.infrastructure.{}.api", base_package, module_lower);

    let data = SyncHandlerData {
        base_package: base_package.clone(),
        package,
        entity_package,
        mapper_package,
        api_package,
        entity_name: entity_name.clone(),
        entity_name_lowercase,
        collection,
        module_lower,
    };

    let content = generator
        .handlebars
        .render("sync_handler", &data)
        .map_err(|e| MobileGenError::template(format!("SyncHandler template error: {}", e)))?;

    let relative_path = format!(
        "infrastructure/{}/sync/{}SyncHandler.kt",
        module_name, entity_name
    );

    match write_generated_file(output_dir, base_package, &relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(path) => Ok(Some(path)),
        crate::kotlin::generators::WriteOutcome::Skipped(_) => Ok(None),
    }
}

/// Generate Ktor API client for a single model
fn generate_api_client(
    generator: &MobileGenerator,
    model: &Model,
    module_name: &str,
    output_dir: &Path,
) -> Result<Option<std::path::PathBuf>> {
    // Use package from generator
    let base_package = &generator.package_name;
    let module_lower = module_name.to_lowercase();
    let package_name = format!("{}.infrastructure.{}.api", base_package, module_lower);
    let entity_package = format!("{}.domain.{}.entity", base_package, module_lower);
    let entity_name = model.name.clone();
    let entity_name_lowercase = entity_name.to_lowercase();
    let collection = model.collection_name();

    // Prepare template data
    let api_data = ApiClientData {
        base_package: base_package.clone(),
        package: package_name,
        entity_package,
        entity_name: entity_name.clone(),
        entity_name_lowercase,
        collection,
        module_lower: module_lower.clone(),
    };

    // Render the template
    let content = generator
        .handlebars
        .render("api_client", &api_data)
        .map_err(|e| MobileGenError::template(format!("API client template error: {}", e)))?;

    // Create output path: infrastructure/{module}/api/{Entity}ApiClient.kt
    let relative_path = format!(
        "infrastructure/{}/api/{}ApiClient.kt",
        module_name,
        entity_name
    );

    match write_generated_file(output_dir, base_package, &relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(path) => Ok(Some(path)),
        crate::kotlin::generators::WriteOutcome::Skipped(_) => Ok(None),
    }
}

/// Generate SQLDelight schema for a single model
fn generate_sqldelight_schema(
    generator: &MobileGenerator,
    model: &Model,
    module_name: &str,
    output_dir: &Path,
) -> Result<Option<std::path::PathBuf>> {
    let entity_name = model.name.clone();
    let collection = model.collection_name();

    // Get fields for SQLDelight schema
    let fields: Vec<SqlField> = model.fields.iter().map(|f| SqlField {
        name: f.name.clone(),
        sql_type: to_sqldelight_type(&generator.type_mapper, &f.type_ref),
    }).collect();

    // Prepare template data
    let entity_name_lower = entity_name.to_lowercase();
    let schema_data = SqlDelightData {
        entity_name: entity_name.clone(),
        collection,
        fields,
        has_soft_delete: model.has_soft_delete(),
    };

    // Render the template
    let content = generator
        .handlebars
        .render("sqldelight_schema", &schema_data)
        .map_err(|e| MobileGenError::template(format!("SQLDelight template error: {}", e)))?;

    // Create output path: sqldelight/{module}/{entity_lower}.sq
    let relative_path = format!(
        "sqldelight/{}/{}.sq",
        module_name,
        entity_name_lower
    );

    match write_generated_file(output_dir, &generator.package_name, &relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(path) => Ok(Some(path)),
        crate::kotlin::generators::WriteOutcome::Skipped(_) => Ok(None),
    }
}

/// Map backbone TypeRef to SQLDelight type
/// Extends KotlinTypeMapper::to_sqldelight_type to handle full TypeRef
fn to_sqldelight_type(mapper: &crate::kotlin::lang::KotlinTypeMapper, type_ref: &crate::ast::TypeRef) -> String {
    use crate::ast::TypeRef;

    match type_ref {
        TypeRef::Primitive(primitive) => mapper.to_sqldelight_type(primitive),
        TypeRef::Optional(inner) => to_sqldelight_type(mapper, inner),
        TypeRef::Array(_) => "TEXT".to_string(), // JSON string
        TypeRef::Map { .. } => "TEXT".to_string(), // JSON string
        TypeRef::Custom(_) => "TEXT".to_string(), // Enums
        TypeRef::ModuleRef { .. } => "TEXT".to_string(),
    }
}

#[derive(Debug, Clone, Serialize)]
struct SyncHandlerData {
    base_package: String,
    package: String,
    entity_package: String,
    mapper_package: String,
    api_package: String,
    entity_name: String,
    entity_name_lowercase: String,
    collection: String,
    module_lower: String,
}

#[derive(Debug, Clone, Serialize)]
struct ApiClientData {
    base_package: String,
    package: String,
    entity_package: String,
    entity_name: String,
    entity_name_lowercase: String,
    collection: String,
    module_lower: String,
}

#[derive(Debug, Clone, Serialize)]
struct SqlDelightData {
    entity_name: String,
    collection: String,
    fields: Vec<SqlField>,
    has_soft_delete: bool,
}

#[derive(Debug, Clone, Serialize)]
struct SqlField {
    name: String,
    sql_type: String,
}
