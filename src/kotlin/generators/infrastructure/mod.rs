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

/// Generate `Offline<Entity>Repository.kt` for all models in a schema.
///
/// One file per entity (in `infrastructure/repository/offline/`) that wraps
/// the matching `<Entity>ApiClient` with cache-first reads, cache-aware
/// writes, and offline fallback by extending `OfflineFirstRepository<T>`.
///
/// Models with `generators.disabled: [offlinerepositories]` (or whose
/// `generators.enabled` whitelist doesn't list it) are skipped. Files
/// containing the `// <<< CUSTOM` marker on disk are also preserved
/// untouched by [`write_generated_file`].
pub fn generate_offline_repositories(
    generator: &MobileGenerator,
    schema: &ModuleSchema,
    output_dir: &Path,
) -> Result<GenerationResult> {
    let mut result = GenerationResult::default();

    for model in &schema.models {
        if generator.is_disabled_for_model(model, crate::kotlin::config::GenerationTarget::OfflineRepositories) {
            continue;
        }
        match generate_offline_repository(generator, model, &schema.name, output_dir) {
            Ok(Some(path)) => {
                result.generated_files.push(path);
                result.offline_repositories_count += 1;
            }
            Ok(None) => {
                result.skipped_files.push(format!("Offline{}Repository.kt", model.name).into());
            }
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

/// Generate an offline-first repository for a single model.
fn generate_offline_repository(
    generator: &MobileGenerator,
    model: &Model,
    module_name: &str,
    output_dir: &Path,
) -> Result<Option<std::path::PathBuf>> {
    let base_package = &generator.package_name;
    let module_lower = module_name.to_lowercase();
    let entity_name = model.name.clone();
    let collection = model.collection_name();

    // Offline repos live at infrastructure/repository/offline/, NOT under a
    // per-module folder — this matches the existing consumer-codebase
    // convention so all `Offline*Repository.kt` files share one DI lookup
    // location.
    let package = format!("{}.infrastructure.repository.offline", base_package);
    let entity_package = format!("{}.domain.{}.entity", base_package, module_lower);
    let api_package = format!("{}.infrastructure.{}.api", base_package, module_lower);

    let data = OfflineRepositoryData {
        base_package: base_package.clone(),
        package,
        entity_package,
        api_package,
        entity_name: entity_name.clone(),
        collection,
    };

    let content = generator
        .handlebars
        .render("offline_repository", &data)
        .map_err(|e| MobileGenError::template(format!("OfflineRepository template error: {}", e)))?;

    let relative_path = format!(
        "infrastructure/repository/offline/Offline{}Repository.kt",
        entity_name
    );

    match write_generated_file(output_dir, base_package, &relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(path) => Ok(Some(path)),
        crate::kotlin::generators::WriteOutcome::Skipped(_) => Ok(None),
    }
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
struct OfflineRepositoryData {
    base_package: String,
    package: String,
    entity_package: String,
    api_package: String,
    entity_name: String,
    collection: String,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ModuleSchema;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    fn make_schema(module: &str, models: Vec<Model>) -> ModuleSchema {
        let mut schema = ModuleSchema::new(module);
        schema.models = models;
        schema
    }

    fn read_generated(dir: &Path, package_name: &str, relative: &str) -> String {
        let pkg_path = package_name.replace('.', "/");
        let path = dir.join("kotlin").join(pkg_path).join(relative);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("expected file {}: {}", path.display(), e))
    }

    #[test]
    fn generates_offline_repository_for_each_model() {
        let generator = MobileGenerator::new("com.test").unwrap();
        let schema = make_schema("widgets", vec![
            Model::new("Widget"),
            Model::new("Gadget"),
        ]);
        let dir = tempdir().unwrap();

        let result = generate_offline_repositories(&generator, &schema, dir.path()).unwrap();

        assert_eq!(result.offline_repositories_count, 2);
        assert_eq!(result.generated_files.len(), 2);
    }

    #[test]
    fn generated_offline_repository_extends_offline_first_repository_with_correct_wiring() {
        let generator = MobileGenerator::new("com.test").unwrap();
        let schema = make_schema("widgets", vec![Model::new("Widget")]);
        let dir = tempdir().unwrap();

        generate_offline_repositories(&generator, &schema, dir.path()).unwrap();
        let content = read_generated(
            dir.path(),
            "com.test",
            "infrastructure/repository/offline/OfflineWidgetRepository.kt",
        );

        // Class declaration
        assert!(
            content.contains("class OfflineWidgetRepository("),
            "expected class declaration; got:\n{}", content
        );
        // Extends the framework base class
        assert!(
            content.contains(": OfflineFirstRepository<Widget>("),
            "expected to extend OfflineFirstRepository; got:\n{}", content
        );
        // Cache-aware delete is wired (this is the whole point of generation)
        assert!(
            content.contains("override suspend fun deleteFromApi(id: String): Result<Unit> = api.delete(id)"),
            "expected deleteFromApi override; got:\n{}", content
        );
        // entityType comes from collection_name (snake_case_plural)
        assert!(
            content.contains("entityType = \"widgets\""),
            "expected entityType matches collection name; got:\n{}", content
        );
        // We do NOT emit a fetchListSinceFromApi *override* — delta-sync is opt-in
        // via a *RepositoryCustom.kt file. (The KDoc may *mention* the name to
        // tell consumers how to opt in; that's expected.)
        assert!(
            !content.contains("override suspend fun fetchListSinceFromApi"),
            "should not emit delta-sync override (opt-in via *RepositoryCustom.kt); got:\n{}", content
        );
        // Imports line up with the consumer convention
        assert!(content.contains("import com.test.infrastructure.widgets.api.WidgetApiClient"));
        assert!(content.contains("import com.test.infrastructure.repository.OfflineFirstRepository"));
        assert!(content.contains("import com.test.infrastructure.cache.CacheTTL"));
        // Default TTL keeps the generator decoupled from per-entity tuning
        assert!(content.contains("ttl = CacheTTL.DEFAULT"));
    }

    #[test]
    fn skips_models_with_offlinerepositories_disabled() {
        let generator = MobileGenerator::new("com.test").unwrap();
        let mut model = Model::new("Widget");
        model.disabled_generators.push("offlinerepositories".to_string());
        let schema = make_schema("widgets", vec![model]);
        let dir = tempdir().unwrap();

        let result = generate_offline_repositories(&generator, &schema, dir.path()).unwrap();

        assert_eq!(result.offline_repositories_count, 0);
        assert!(result.generated_files.is_empty());
    }

    #[test]
    fn enabled_whitelist_can_restrict_to_offlinerepositories_only() {
        let generator = MobileGenerator::new("com.test").unwrap();
        let mut model = Model::new("Widget");
        // Whitelist: only offlinerepositories runs for this model
        model.enabled_generators.push("offlinerepositories".to_string());
        let schema = make_schema("widgets", vec![model]);
        let dir = tempdir().unwrap();

        let result = generate_offline_repositories(&generator, &schema, dir.path()).unwrap();
        assert_eq!(result.offline_repositories_count, 1);
    }

    #[test]
    fn writes_to_infrastructure_repository_offline_path() {
        let generator = MobileGenerator::new("com.test").unwrap();
        let schema = make_schema("widgets", vec![Model::new("Widget")]);
        let dir = tempdir().unwrap();

        let result = generate_offline_repositories(&generator, &schema, dir.path()).unwrap();
        let path = &result.generated_files[0];
        let path_str = path.to_string_lossy();

        // Path is shared across modules: infrastructure/repository/offline/<file>.kt
        // (NOT infrastructure/{module}/repository/offline/...)
        assert!(
            path_str.contains("infrastructure/repository/offline/OfflineWidgetRepository.kt"),
            "expected shared offline path; got: {}", path_str
        );
    }
}
