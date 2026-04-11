//! Navigation layer generator — Phase 5
//!
//! 5A: Module-level `@Serializable` sealed NavConfig class (one List + one Detail per entity)
//! 5B: Deep link extension `fromDeepLink(uri)` for the NavConfig
//! 5C: Role-visibility stub inside the NavConfig companion object (TODO for developer)
//!
//! Per-entity destination classes are kept as-is from the existing generator.

use crate::kotlin::error::{MobileGenError, Result};
use crate::kotlin::generators::GenerationResult;
use crate::kotlin::generators::MobileGenerator;
use crate::kotlin::generators::write_generated_file;
use crate::ast::ModuleSchema;
use serde::Serialize;
use std::path::Path;

// ─────────────────────────────────────────────────────────────────────────────
// Public entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Generate Decompose navigation components for a module.
///
/// Produces:
/// - `presentation/navigation/{Module}NavConfig.kt`     (5A — sealed class)
/// - `presentation/navigation/{Module}DeepLinks.kt`     (5B — fromDeepLink extension)
/// - `presentation/navigation/{module}/XxxDestination.kt` (per entity — unchanged)
pub fn generate_navigation(
    generator: &MobileGenerator,
    schema: &ModuleSchema,
    output_dir: &Path,
) -> Result<GenerationResult> {
    let mut result = GenerationResult::default();

    // 5A — module NavConfig sealed class
    if let Some(path) = generate_nav_config(generator, schema, output_dir)? {
        result.generated_files.push(path);
    }

    // 5B — deep link extension
    if let Some(path) = generate_deep_links(generator, schema, output_dir)? {
        result.generated_files.push(path);
    }

    // Per-entity destination classes (existing pattern — kept as-is)
    for model in &schema.models {
        if generator.is_disabled_for_model(model, crate::kotlin::config::GenerationTarget::Navigation) {
            continue;
        }
        match generate_entity_destination(generator, model, &schema.name, output_dir) {
            Ok(Some(path)) => result.generated_files.push(path),
            Ok(None) => {}
            Err(e) => return Err(e),
        }
    }

    Ok(result)
}

// ─────────────────────────────────────────────────────────────────────────────
// 5A — NavConfig sealed class
// ─────────────────────────────────────────────────────────────────────────────

fn generate_nav_config(
    generator: &MobileGenerator,
    schema: &ModuleSchema,
    output_dir: &Path,
) -> Result<Option<std::path::PathBuf>> {
    let base_package = &generator.package_name;
    let module_pascal = &schema.name;
    let module_lower = schema.name.to_lowercase();
    let package = format!("{}.presentation.navigation", base_package);

    let entities: Vec<NavEntityData> = schema
        .models
        .iter()
        .filter(|m| !generator.is_disabled_for_model(m, crate::kotlin::config::GenerationTarget::Navigation))
        .map(|m| NavEntityData {
            entity_name: m.name.clone(),
            collection: m.collection_name(),
        })
        .collect();

    let first_entity = entities
        .first()
        .map(|e| e.entity_name.clone())
        .unwrap_or_else(|| "Entity".to_string());

    let data = NavConfigData {
        package: package.clone(),
        module_pascal: module_pascal.clone(),
        module_lower: module_lower.clone(),
        first_entity,
        entities,
    };

    let content = generator
        .handlebars
        .render("nav_config", &data)
        .map_err(|e| MobileGenError::template(format!("NavConfig template error: {}", e)))?;

    let relative_path = format!("presentation/navigation/{}NavConfig.kt", module_pascal);

    match write_generated_file(output_dir, base_package, &relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(p) => Ok(Some(p)),
        crate::kotlin::generators::WriteOutcome::Skipped(_) => Ok(None),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 5B — Deep link extension
// ─────────────────────────────────────────────────────────────────────────────

fn generate_deep_links(
    generator: &MobileGenerator,
    schema: &ModuleSchema,
    output_dir: &Path,
) -> Result<Option<std::path::PathBuf>> {
    let base_package = &generator.package_name;
    let module_pascal = &schema.name;
    let module_lower = schema.name.to_lowercase();
    let package = format!("{}.presentation.navigation", base_package);

    let entities: Vec<NavEntityData> = schema
        .models
        .iter()
        .filter(|m| !generator.is_disabled_for_model(m, crate::kotlin::config::GenerationTarget::Navigation))
        .map(|m| NavEntityData {
            entity_name: m.name.clone(),
            collection: m.collection_name(),
        })
        .collect();

    let data = NavDeepLinkData {
        package,
        module_pascal: module_pascal.clone(),
        module_lower,
        entities,
    };

    let content = generator
        .handlebars
        .render("nav_deep_link", &data)
        .map_err(|e| MobileGenError::template(format!("NavDeepLink template error: {}", e)))?;

    let relative_path = format!("presentation/navigation/{}DeepLinks.kt", module_pascal);

    match write_generated_file(output_dir, base_package, &relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(p) => Ok(Some(p)),
        crate::kotlin::generators::WriteOutcome::Skipped(_) => Ok(None),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Per-entity destination class (existing pattern — kept functional)
// ─────────────────────────────────────────────────────────────────────────────

fn generate_entity_destination(
    generator: &MobileGenerator,
    model: &crate::ast::Model,
    module_name: &str,
    output_dir: &Path,
) -> Result<Option<std::path::PathBuf>> {
    let base_package = &generator.package_name;
    let module_lower = module_name.to_lowercase();
    let entity_name = model.name.clone();

    let package_name = format!("{}.presentation.navigation.{}", base_package, module_lower);

    let content = format!(
        r#"package {package}

import {base}.domain.{module}.entity.{entity}
import {base}.presentation.state.{module}.{entity}ListViewModel
import {base}.core.usecase.CrudUseCases

/**
 * Navigation destination for {entity}.
 *
 * Holds the ViewModel for the {entity} list/detail flow.
 * Pass [useCases] from your DI container.
 *
 * Generated from Backbone schema.
 */
class {entity}Destination(
    private val useCases: CrudUseCases<{entity}>,
) {{
    val viewModel = {entity}ListViewModel(useCases)

    fun onBack() {{
        // Handle back navigation (call parent component's pop())
    }}
}}
"#,
        package = package_name,
        base = base_package,
        module = module_lower,
        entity = entity_name,
    );

    let relative_path = format!(
        "presentation/navigation/{module_name}/{entity}Destination.kt",
        entity = entity_name,
    );

    match write_generated_file(output_dir, base_package, &relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(path) => Ok(Some(path)),
        crate::kotlin::generators::WriteOutcome::Skipped(_) => Ok(None),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Template data structs
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
struct NavConfigData {
    package: String,
    module_pascal: String,
    module_lower: String,
    first_entity: String,
    entities: Vec<NavEntityData>,
}

#[derive(Debug, Clone, Serialize)]
struct NavDeepLinkData {
    package: String,
    module_pascal: String,
    module_lower: String,
    entities: Vec<NavEntityData>,
}

#[derive(Debug, Clone, Serialize)]
struct NavEntityData {
    entity_name: String,
    collection: String,
}
