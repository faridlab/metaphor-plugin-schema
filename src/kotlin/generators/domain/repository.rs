//! Repository interface generator

use crate::kotlin::error::{MobileGenError, Result};
use crate::kotlin::generators::GenerationResult;
use crate::kotlin::generators::MobileGenerator;
use crate::kotlin::generators::write_generated_file;
use crate::ast::{Model, ModuleSchema};
use std::path::Path;

/// Generate repository interfaces for all models in a schema.
///
/// Note: pagination types (`PaginatedResult`, `PaginatedApiResponse`,
/// `BackendPaginatedResponse`, `PaginationMeta`) are NOT generated — they are
/// hand-written framework contracts used by the base `BaseCrudApiClient` /
/// `OfflineFirstRepository`, so generated code imports them from the framework
/// package (see the `{{framework base_package}}` imports in the templates).
pub fn generate_repositories(
    generator: &MobileGenerator,
    schema: &ModuleSchema,
    output_dir: &Path,
) -> Result<GenerationResult> {
    let mut result = GenerationResult::default();

    for model in &schema.models {
        if generator.is_disabled_for_model(model, crate::kotlin::config::GenerationTarget::Repositories) {
            continue;
        }
        match generate_repository(generator, model, &schema.name, output_dir) {
            Ok(Some(path)) => {
                result.generated_files.push(path);
                result.repositories_count += 1;
            }
            Ok(None) => {
                result.skipped_files.push(model.name.clone().into());
            }
            Err(e) => return Err(e),
        }
    }

    Ok(result)
}


/// Generate a single repository interface
fn generate_repository(
    generator: &MobileGenerator,
    model: &Model,
    module_name: &str,
    output_dir: &Path,
) -> Result<Option<std::path::PathBuf>> {
    // Use package from generator
    let base_package = &generator.package_name;
    let module_lower = module_name.to_lowercase();
    let package_name = format!("{}.{}.domain.repository", base_package, module_lower);
    let entity_package = format!("{}.{}.domain.entity", base_package, module_lower);
    let entity_name = model.name.clone();

    // Prepare template data
    let repo_data = RepositoryData {
        base_package: base_package.clone(),
        package: package_name,
        entity_package,
        entity_name: entity_name.clone(),
        entity_name_lowercase: entity_name.to_lowercase(),
        collection: model.collection_name(),
        has_soft_delete: model.has_soft_delete(),
    };

    // Render the template
    let content = generator
        .handlebars
        .render("repository", &repo_data)
        .map_err(|e| MobileGenError::template(format!("Repository template error: {}", e)))?;

    // Create output path: domain/{module}/repository/{Entity}Repository.kt
    let relative_path = format!(
        "{}/domain/repository/{}Repository.kt",
        module_name,
        entity_name
    );

    match write_generated_file(output_dir, base_package, &relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(path) => Ok(Some(path)),
        crate::kotlin::generators::WriteOutcome::Skipped(_) => Ok(None),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct RepositoryData {
    base_package: String,
    package: String,
    entity_package: String,
    entity_name: String,
    entity_name_lowercase: String,
    collection: String,
    has_soft_delete: bool,
}
