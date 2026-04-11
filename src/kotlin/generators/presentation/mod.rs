//! Presentation layer generators (ViewModels, UI Components, Navigation, Theme)

pub mod navigation;
pub mod theme;

use crate::kotlin::error::{MobileGenError, Result};
use crate::kotlin::generators::GenerationResult;
use crate::kotlin::generators::MobileGenerator;
use crate::kotlin::generators::write_generated_file;
use crate::ast::{Model, ModuleSchema};
use std::path::Path;

/// Generate MVI ViewModels for all models in a schema
pub fn generate_viewmodels(
    generator: &MobileGenerator,
    schema: &ModuleSchema,
    output_dir: &Path,
) -> Result<GenerationResult> {
    let mut result = GenerationResult::default();

    for model in &schema.models {
        if generator.is_disabled_for_model(model, crate::kotlin::config::GenerationTarget::ViewModels) {
            continue;
        }
        match generate_viewmodel(generator, model, &schema.name, output_dir) {
            Ok(Some(path)) => {
                result.generated_files.push(path);
                result.viewmodels_count += 1;
            }
            Ok(None) => {}
            Err(e) => return Err(e),
        }
    }

    Ok(result)
}

/// Generate reusable UI components
pub fn generate_components(
    generator: &MobileGenerator,
    schema: &ModuleSchema,
    output_dir: &Path,
) -> Result<GenerationResult> {
    let mut result = GenerationResult::default();

    for model in &schema.models {
        if generator.is_disabled_for_model(model, crate::kotlin::config::GenerationTarget::Components) {
            continue;
        }
        match generate_component(generator, model, &schema.name, output_dir) {
            Ok(Some(path)) => {
                result.generated_files.push(path);
                result.components_count += 1;
            }
            Ok(None) => {}
            Err(e) => return Err(e),
        }
    }

    Ok(result)
}

/// Generate MVI ViewModel for a single model
fn generate_viewmodel(
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

    // Package for presentation/state/{module}
    let package_name = format!("{}.presentation.state.{}", base_package, module_lower);
    let entity_package = format!("{}.domain.{}.entity", base_package, module_lower);
    let repository_package = format!("{}.domain.{}.repository", base_package, module_lower);
    let mapper_package = format!("{}.application.{}.mappers", base_package, module_lower);

    // Find the primary key field name (first primary key field)
    let primary_key_field = model.fields.iter()
        .find(|f| f.is_primary_key())
        .map(|f| generator.type_mapper.to_kotlin_property_name(&f.name))
        .unwrap_or_else(|| "id".to_string());

    // Prepare template data
    let vm_data = ViewModelData {
        base_package: base_package.clone(),
        package: package_name.clone(),
        entity_name: entity_name.clone(),
        entity_name_lowercase: entity_name_lowercase.clone(),
        entity_package,
        repository_package,
        mapper_package,
        primary_key_field,
    };

    // Render the template
    let content = generator
        .handlebars
        .render("viewmodel", &vm_data)
        .map_err(|e| MobileGenError::template(format!("ViewModel template error: {}", e)))?;

    // Create output path: presentation/state/{module}/{Entity}ListViewModel.kt
    let relative_path = format!(
        "presentation/state/{}/{}ListViewModel.kt",
        module_name,
        entity_name
    );

    match write_generated_file(output_dir, base_package, &relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(path) => Ok(Some(path)),
        crate::kotlin::generators::WriteOutcome::Skipped(_) => Ok(None),
    }
}

/// Generate UI component for a single model
fn generate_component(
    generator: &MobileGenerator,
    model: &Model,
    module_name: &str,
    output_dir: &Path,
) -> Result<Option<std::path::PathBuf>> {
    // Use package from generator
    let base_package = &generator.package_name;
    let module_lower = module_name.to_lowercase();
    let entity_name = model.name.clone();

    // Package for presentation/components/{module}
    let package_name = format!("{}.presentation.components.{}", base_package, module_lower);
    let entity_package = format!("{}.domain.{}.entity", base_package, module_lower);

    // Find the primary key field name (first primary key field)
    let primary_key_field = model.fields.iter()
        .find(|f| f.is_primary_key())
        .map(|f| generator.type_mapper.to_kotlin_property_name(&f.name))
        .unwrap_or_else(|| "id".to_string());

    let mapper_package = format!("{}.application.{}.mappers", base_package, module_lower);

    // Prepare template data
    let component_data = ComponentData {
        base_package: base_package.clone(),
        package: package_name.clone(),
        entity_name: entity_name.clone(),
        entity_package,
        mapper_package,
        primary_key_field,
    };

    // Render the template
    let content = generator
        .handlebars
        .render("component_card", &component_data)
        .map_err(|e| MobileGenError::template(format!("Component template error: {}", e)))?;

    // Create output path: presentation/components/{module}/{Entity}Card.kt
    let relative_path = format!(
        "presentation/components/{}/{}Card.kt",
        module_name,
        entity_name
    );

    match write_generated_file(output_dir, base_package, &relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(path) => Ok(Some(path)),
        crate::kotlin::generators::WriteOutcome::Skipped(_) => Ok(None),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct ViewModelData {
    base_package: String,
    package: String,
    entity_name: String,
    entity_name_lowercase: String,
    entity_package: String,
    repository_package: String,
    mapper_package: String,
    primary_key_field: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct ComponentData {
    base_package: String,
    package: String,
    entity_name: String,
    entity_package: String,
    mapper_package: String,
    primary_key_field: String,
}
