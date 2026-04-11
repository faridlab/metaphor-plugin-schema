//! Test generators (3B ValidatorTest, 3B ViewModelTest, 3C ApiClientTest)

use crate::kotlin::config::GenerationTarget;
use crate::kotlin::error::{MobileGenError, Result};
use crate::kotlin::generators::{write_generated_file, GenerationResult, MobileGenerator};
use crate::ast::{Model, ModuleSchema};
use std::path::Path;

// ─────────────────────────────────────────────────────────────────────────────
// Public entry points
// ─────────────────────────────────────────────────────────────────────────────

/// Generate all three test kinds for every model in the schema.
pub fn generate_tests(
    generator: &MobileGenerator,
    schema: &ModuleSchema,
    output_dir: &Path,
) -> Result<GenerationResult> {
    let mut result = GenerationResult::default();

    for model in &schema.models {
        if generator.is_disabled_for_model(model, GenerationTarget::Tests) {
            continue;
        }

        // 3B — validator test
        if let Some(path) = generate_validator_test(generator, model, &schema.name, output_dir)? {
            result.generated_files.push(path);
        }

        // 3B — ViewModel test
        if let Some(path) = generate_viewmodel_test(generator, model, &schema.name, output_dir)? {
            result.generated_files.push(path);
        }

        // 3C — API client mock test
        if let Some(path) = generate_api_client_test(generator, model, &schema.name, output_dir)? {
            result.generated_files.push(path);
        }
    }

    Ok(result)
}

// ─────────────────────────────────────────────────────────────────────────────
// Validator test (3B)
// ─────────────────────────────────────────────────────────────────────────────

fn generate_validator_test(
    generator: &MobileGenerator,
    model: &Model,
    module_name: &str,
    output_dir: &Path,
) -> Result<Option<std::path::PathBuf>> {
    let base_package = &generator.package_name;
    let module_lower = module_name.to_lowercase();
    let entity_name = model.name.clone();

    let package = format!(
        "{}.application.{}.validators",
        base_package, module_lower
    );

    let fields = build_field_data(generator, model);

    let data = ValidatorTestData {
        base_package: base_package.clone(),
        package,
        module_lower: module_lower.clone(),
        entity_name: entity_name.clone(),
        fields,
    };

    let content = generator
        .handlebars
        .render("validator_test", &data)
        .map_err(|e| MobileGenError::template(format!("ValidatorTest template error: {}", e)))?;

    let relative_path = format!(
        "application/{}/validators/{}ValidatorTest.kt",
        module_name, entity_name
    );

    write_test_file(generator, output_dir, base_package, &relative_path, &content)
}

// ─────────────────────────────────────────────────────────────────────────────
// ViewModel test (3B)
// ─────────────────────────────────────────────────────────────────────────────

fn generate_viewmodel_test(
    generator: &MobileGenerator,
    model: &Model,
    module_name: &str,
    output_dir: &Path,
) -> Result<Option<std::path::PathBuf>> {
    let base_package = &generator.package_name;
    let module_lower = module_name.to_lowercase();
    let entity_name = model.name.clone();

    let package = format!(
        "{}.presentation.state.{}",
        base_package, module_lower
    );

    let primary_key_field = model
        .fields
        .iter()
        .find(|f| f.is_primary_key())
        .map(|f| generator.type_mapper.to_kotlin_property_name(&f.name))
        .unwrap_or_else(|| "id".to_string());

    let data = ViewModelTestData {
        base_package: base_package.clone(),
        package,
        module_lower: module_lower.clone(),
        entity_name: entity_name.clone(),
        primary_key_field,
    };

    let content = generator
        .handlebars
        .render("viewmodel_test", &data)
        .map_err(|e| MobileGenError::template(format!("ViewModelTest template error: {}", e)))?;

    let relative_path = format!(
        "presentation/state/{}/{}ListViewModelTest.kt",
        module_name, entity_name
    );

    write_test_file(generator, output_dir, base_package, &relative_path, &content)
}

// ─────────────────────────────────────────────────────────────────────────────
// API client mock test (3C)
// ─────────────────────────────────────────────────────────────────────────────

fn generate_api_client_test(
    generator: &MobileGenerator,
    model: &Model,
    module_name: &str,
    output_dir: &Path,
) -> Result<Option<std::path::PathBuf>> {
    let base_package = &generator.package_name;
    let module_lower = module_name.to_lowercase();
    let entity_name = model.name.clone();

    let package = format!(
        "{}.infrastructure.{}.api",
        base_package, module_lower
    );

    let primary_key_field = model
        .fields
        .iter()
        .find(|f| f.is_primary_key())
        .map(|f| generator.type_mapper.to_kotlin_property_name(&f.name))
        .unwrap_or_else(|| "id".to_string());

    let fields = build_field_data(generator, model);

    let data = ApiClientTestData {
        base_package: base_package.clone(),
        package,
        module_lower: module_lower.clone(),
        entity_name: entity_name.clone(),
        primary_key_field,
        fields,
    };

    let content = generator
        .handlebars
        .render("api_client_test", &data)
        .map_err(|e| MobileGenError::template(format!("ApiClientTest template error: {}", e)))?;

    let relative_path = format!(
        "infrastructure/{}/api/{}ApiClientTest.kt",
        module_name, entity_name
    );

    write_test_file(generator, output_dir, base_package, &relative_path, &content)
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Write a test file into the commonTest source set, rooted at
/// `<output_dir>/../commonTest/kotlin/<package_path>/<relative_path>`.
///
/// The generator's `output_dir` points at `commonMain`; tests go one level up
/// into `commonTest`.
fn write_test_file(
    generator: &MobileGenerator,
    output_dir: &Path,
    package_name: &str,
    relative_path: &str,
    content: &str,
) -> Result<Option<std::path::PathBuf>> {
    // Resolve sibling commonTest directory from commonMain
    let test_output_dir = output_dir
        .parent() // src/
        .map(|p| p.join("commonTest"))
        .unwrap_or_else(|| output_dir.join("../commonTest").to_path_buf());

    match write_generated_file(
        &test_output_dir,
        package_name,
        relative_path,
        content,
        generator.skip_existing,
    )? {
        crate::kotlin::generators::WriteOutcome::Written(p) => Ok(Some(p)),
        crate::kotlin::generators::WriteOutcome::Skipped(_) => Ok(None),
    }
}

/// Build shared field data list (name, original_name, nullable, primary_key, form_default_value).
fn build_field_data(generator: &MobileGenerator, model: &Model) -> Vec<TestFieldData> {
    model
        .fields
        .iter()
        .map(|f| {
            let kt_type_non_nullable =
                generator.type_mapper.to_kotlin_type_non_nullable(&f.type_ref);
            let is_nullable = f.type_ref.is_optional();
            TestFieldData {
                name: generator.type_mapper.to_kotlin_property_name(&f.name),
                original_name: f.name.clone(),
                kotlin_type: generator.type_mapper.to_kotlin_type(&f.type_ref),
                is_nullable,
                is_primary_key: f.is_primary_key(),
                form_default_value: form_default_value(&kt_type_non_nullable, is_nullable),
            }
        })
        .collect()
}

/// Sensible Kotlin default value per type, used in test FormData construction.
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

// ─────────────────────────────────────────────────────────────────────────────
// Template data structs
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
struct ValidatorTestData {
    base_package: String,
    package: String,
    module_lower: String,
    entity_name: String,
    fields: Vec<TestFieldData>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct ViewModelTestData {
    base_package: String,
    package: String,
    module_lower: String,
    entity_name: String,
    primary_key_field: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct ApiClientTestData {
    base_package: String,
    package: String,
    module_lower: String,
    entity_name: String,
    primary_key_field: String,
    fields: Vec<TestFieldData>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct TestFieldData {
    name: String,
    original_name: String,
    kotlin_type: String,
    is_nullable: bool,
    is_primary_key: bool,
    form_default_value: String,
}
