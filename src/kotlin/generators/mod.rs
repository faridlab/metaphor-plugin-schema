//! Code generators for Kotlin mobile apps

pub mod domain;
pub mod application;
pub mod infrastructure;
pub mod presentation;
pub mod tests;

use crate::kotlin::config::GenerationTarget;
use crate::kotlin::error::Result;
use crate::kotlin::lang::KotlinTypeMapper;
use crate::kotlin::templates;
use crate::ast::ModuleSchema;
use handlebars::Handlebars;
use std::path::Path;

/// Main mobile code generator
pub struct MobileGenerator {
    /// Type mapper for converting schema types to Kotlin
    pub type_mapper: KotlinTypeMapper,
    /// Handlebars template engine
    pub handlebars: Handlebars<'static>,
    /// Package name for generated code
    pub package_name: String,
    /// Skip existing files (even without // <<< CUSTOM marker)
    pub skip_existing: bool,
}

impl MobileGenerator {
    /// Create a new mobile generator
    pub fn new(package_name: impl Into<String>) -> Result<Self> {
        let mut handlebars = Handlebars::new();

        // Register all templates
        Self::register_templates(&mut handlebars)?;

        Ok(Self {
            type_mapper: KotlinTypeMapper::new(),
            handlebars,
            package_name: package_name.into(),
            skip_existing: false,
        })
    }

    /// Register all Handlebars templates
    fn register_templates(handlebars: &mut Handlebars) -> Result<()> {
        // Domain templates
        handlebars.register_template_string("entity", templates::ENTITY_TEMPLATE)?;
        handlebars.register_template_string("enum", templates::ENUM_TEMPLATE)?;
        handlebars.register_template_string("repository", templates::REPOSITORY_TEMPLATE)?;

        // Common templates (generated once per module)
        handlebars.register_template_string("pagination", templates::PAGINATION_TEMPLATE)?;

        // Application templates
        handlebars.register_template_string("usecase", templates::USECASE_TEMPLATE)?;
        handlebars.register_template_string("app_service", templates::APP_SERVICE_TEMPLATE)?;
        handlebars.register_template_string("mapper", templates::MAPPER_TEMPLATE)?;
        handlebars.register_template_string("validator", templates::VALIDATOR_TEMPLATE)?;

        // Infrastructure templates
        handlebars.register_template_string("api_client", templates::API_CLIENT_TEMPLATE)?;
        handlebars.register_template_string("sqldelight_schema", templates::SQLDELIGHT_SCHEMA_TEMPLATE)?;
        handlebars.register_template_string("sqldelight_queries", templates::SQLDELIGHT_QUERIES_TEMPLATE)?;

        // Presentation templates
        handlebars.register_template_string("viewmodel", templates::VIEWMODEL_TEMPLATE)?;
        handlebars.register_template_string("component_card", templates::COMPONENT_CARD_TEMPLATE)?;

        // Test templates (3B + 3C)
        handlebars.register_template_string("validator_test", templates::VALIDATOR_TEST_TEMPLATE)?;
        handlebars.register_template_string("viewmodel_test", templates::VIEWMODEL_TEST_TEMPLATE)?;
        handlebars.register_template_string("api_client_test", templates::API_CLIENT_TEST_TEMPLATE)?;

        // Sync templates (Phase 4)
        handlebars.register_template_string("sync_handler", templates::SYNC_HANDLER_TEMPLATE)?;

        // Navigation templates (Phase 5)
        handlebars.register_template_string("nav_config", templates::NAV_CONFIG_TEMPLATE)?;
        handlebars.register_template_string("nav_deep_link", templates::NAV_DEEP_LINK_TEMPLATE)?;

        Ok(())
    }

    /// Return true if `target` should be skipped for this specific model.
    ///
    /// Two checks (in priority order):
    /// 1. `enabled_generators` whitelist — if non-empty, only listed targets run
    /// 2. `disabled_generators` blacklist — if target appears here, skip it
    pub fn is_disabled_for_model(&self, model: &crate::ast::Model, target: crate::kotlin::config::GenerationTarget) -> bool {
        let target_name = target.as_str();
        // Whitelist: if enabled is set, skip any target NOT in the list
        if !model.enabled_generators.is_empty() {
            return !model.enabled_generators.iter().any(|e| e.eq_ignore_ascii_case(target_name));
        }
        // Blacklist: skip if explicitly disabled
        model.disabled_generators.iter().any(|d| d.eq_ignore_ascii_case(target_name))
    }

    /// Generate code for a module schema
    pub fn generate(
        &self,
        schema: &ModuleSchema,
        targets: &[GenerationTarget],
        output_dir: &Path,
    ) -> Result<GenerationResult> {
        let mut result = GenerationResult::default();

        // Load previous manifest before generating so we can detect stale files.
        let mfest_path = manifest_path(output_dir, &self.package_name, &schema.name);
        let old_manifest = load_manifest(&mfest_path);

        // Expand All → every concrete target
        let mut targets_to_generate: Vec<GenerationTarget> = if targets.contains(&GenerationTarget::All) {
            GenerationTarget::all_targets().to_vec()
        } else {
            targets.to_vec()
        };

        // Apply module-level generators.disabled from schema index config
        if let Some(ref gen_cfg) = schema.generators_config {
            if let Some(ref disabled) = gen_cfg.disabled {
                targets_to_generate.retain(|t| {
                    let name = t.as_str();
                    !disabled.iter().any(|d| d.eq_ignore_ascii_case(name))
                });
            }
            // Whitelist mode: if enabled is set, only keep those targets
            if let Some(ref enabled) = gen_cfg.enabled {
                targets_to_generate.retain(|t| {
                    let name = t.as_str();
                    enabled.iter().any(|e| e.eq_ignore_ascii_case(name))
                });
            }
        }

        // Generate domain layer
        if targets_to_generate.contains(&GenerationTarget::Entities) {
            result.merge(domain::generate_entities(self, schema, output_dir)?);
        }
        if targets_to_generate.contains(&GenerationTarget::Enums) {
            result.merge(domain::generate_enums(self, schema, output_dir)?);
        }
        if targets_to_generate.contains(&GenerationTarget::Repositories) {
            result.merge(domain::generate_repositories(self, schema, output_dir)?);
        }

        // Generate application layer
        if targets_to_generate.contains(&GenerationTarget::UseCases) {
            result.merge(application::generate_usecases(self, schema, output_dir)?);
        }
        if targets_to_generate.contains(&GenerationTarget::AppServices) {
            result.merge(application::generate_app_services(self, schema, output_dir)?);
        }
        if targets_to_generate.contains(&GenerationTarget::Mappers) {
            result.merge(application::generate_mappers(self, schema, output_dir)?);
        }
        if targets_to_generate.contains(&GenerationTarget::Validators) {
            result.merge(application::generate_validators(self, schema, output_dir)?);
        }

        // Generate infrastructure layer
        if targets_to_generate.contains(&GenerationTarget::ApiClients) {
            result.merge(infrastructure::generate_api_clients(self, schema, output_dir)?);
        }
        if targets_to_generate.contains(&GenerationTarget::Database) {
            result.merge(infrastructure::generate_database(self, schema, output_dir)?);
        }
        if targets_to_generate.contains(&GenerationTarget::Sync) {
            result.merge(infrastructure::generate_sync(self, schema, output_dir)?);
        }

        // Generate presentation layer
        if targets_to_generate.contains(&GenerationTarget::ViewModels) {
            result.merge(presentation::generate_viewmodels(self, schema, output_dir)?);
        }
        if targets_to_generate.contains(&GenerationTarget::Components) {
            result.merge(presentation::generate_components(self, schema, output_dir)?);
        }

        // Generate navigation and theme (module-level, not per-model)
        if targets_to_generate.contains(&GenerationTarget::Navigation) {
            result.merge(presentation::navigation::generate_navigation(self, schema, output_dir)?);
        }
        if targets_to_generate.contains(&GenerationTarget::Theme) {
            result.merge(presentation::theme::generate_theme(self, schema, output_dir)?);
        }

        // Generate test stubs (commonTest) — 3B ValidatorTest + ViewModelTest, 3C ApiClientTest
        if targets_to_generate.contains(&GenerationTarget::Tests) {
            result.merge(tests::generate_tests(self, schema, output_dir)?);
        }

        // Cleanup stale files and persist updated manifest.
        let (updated_manifest, stale_deleted) = cleanup_stale(&old_manifest, &result.generated_files);
        result.stale_deleted_files = stale_deleted;
        if let Err(e) = save_manifest(&mfest_path, &updated_manifest) {
            eprintln!("Warning: could not save mobilegen manifest: {}", e);
        }

        Ok(result)
    }
}

/// Outcome of a file write attempt
#[derive(Debug)]
pub enum WriteOutcome {
    /// File was written successfully
    Written(std::path::PathBuf),
    /// File was skipped (contains custom code marker)
    Skipped(std::path::PathBuf),
}

// ─── Manifest helpers ────────────────────────────────────────────────────────

/// Compute the resolved on-disk path for a generated file (mirrors write_generated_file logic).
pub fn resolve_output_path(output_dir: &Path, package_name: &str, relative_path: &str) -> std::path::PathBuf {
    let package_path = package_name.replace('.', "/");
    let output_str = output_dir.to_string_lossy();
    let kotlin_path = if output_str.ends_with("kotlin") || output_str.contains("/kotlin/") {
        let expected_package_suffix = format!("/kotlin/{}/", package_path);
        let expected_package_suffix_no_slash = format!("/kotlin/{}", package_path);
        if output_str.contains(&expected_package_suffix)
            || output_str.ends_with(&expected_package_suffix_no_slash)
        {
            relative_path.to_string()
        } else {
            format!("{}/{}", package_path, relative_path)
        }
    } else {
        format!("kotlin/{}/{}", package_path, relative_path)
    };
    output_dir.join(&kotlin_path)
}

/// Return the path to the per-module manifest file.
fn manifest_path(output_dir: &Path, package_name: &str, module_name: &str) -> std::path::PathBuf {
    let package_path = package_name.replace('.', "/");
    let output_str = output_dir.to_string_lossy();
    let base = if output_str.ends_with("kotlin") || output_str.contains("/kotlin/") {
        let expected_package_suffix = format!("/kotlin/{}/", package_path);
        let expected_package_suffix_no_slash = format!("/kotlin/{}", package_path);
        if output_str.contains(&expected_package_suffix)
            || output_str.ends_with(&expected_package_suffix_no_slash)
        {
            output_dir.to_path_buf()
        } else {
            output_dir.join(&package_path)
        }
    } else {
        output_dir.join(format!("kotlin/{}", package_path))
    };
    base.join(format!(".mobilegen-{}.manifest", module_name))
}

/// Load a manifest file; returns an empty vec if absent.
fn load_manifest(path: &std::path::Path) -> Vec<std::path::PathBuf> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(std::path::PathBuf::from)
        .collect()
}

/// Persist the manifest, creating parent dirs as needed.
fn save_manifest(path: &std::path::Path, files: &[std::path::PathBuf]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(path, content + "\n")
}

/// Compare the previous manifest with newly generated files, delete stale
/// entries, and return the updated manifest.
///
/// A file is **stale** when it was managed by mobilegen in a previous run
/// (`old_manifest` entry), was **not** regenerated this run, and does **not**
/// contain the `// <<< CUSTOM` marker on disk.  Files with the custom marker
/// are kept — the user has extended them and they remain in the manifest.
fn cleanup_stale(
    old_manifest: &[std::path::PathBuf],
    new_generated: &[std::path::PathBuf],
) -> (Vec<std::path::PathBuf>, Vec<std::path::PathBuf>) {
    use std::collections::HashSet;
    let new_set: HashSet<&std::path::PathBuf> = new_generated.iter().collect();

    let mut stale_deleted: Vec<std::path::PathBuf> = Vec::new();
    let mut updated_manifest: Vec<std::path::PathBuf> = new_generated.to_vec();

    for path in old_manifest {
        if new_set.contains(path) {
            // Already in the new manifest (was regenerated).
            continue;
        }
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(path) {
                if content.contains("// <<< CUSTOM") {
                    // User has custom code — preserve in manifest, don't delete.
                    updated_manifest.push(path.clone());
                    continue;
                }
            }
            // No custom code — this file is stale; remove it.
            if std::fs::remove_file(path).is_ok() {
                stale_deleted.push(path.clone());
            }
        }
        // If the file no longer exists on disk, simply omit it from the manifest.
    }

    updated_manifest.sort();
    updated_manifest.dedup();
    (updated_manifest, stale_deleted)
}

// ─────────────────────────────────────────────────────────────────────────────

/// Write generated content to a file, creating parent directories as needed.
/// If the existing file contains `// <<< CUSTOM`, it is skipped to preserve custom code.
///
/// # Arguments
/// * `output_dir` - The base output directory (e.g., apps/mobileapp/shared/src/commonMain)
/// * `package_name` - The package name for the generated code (e.g., "com.bersihir")
/// * `relative_path` - The relative path within the package (e.g., "domain/sapiens/entity/User.kt")
/// * `content` - The file content to write
///
/// # Examples
/// ```ignore
/// write_generated_file(
///     &PathBuf::from("apps/mobileapp/shared/src/commonMain"),
///     "com.bersihir",
///     "domain/sapiens/entity/User.kt",
///     "package com.bersihir.domain.sapiens.entity..."
/// )?;
/// ```
pub fn write_generated_file(
    output_dir: &Path,
    package_name: &str,
    relative_path: &str,
    content: &str,
    skip_existing: bool,
) -> Result<WriteOutcome> {
    // Convert package name to path (e.g., "com.bersihir" -> "com/bersihir")
    let package_path = package_name.replace('.', "/");

    // Check if output_dir already ends with 'kotlin' (commonMain/kotlin)
    let output_str = output_dir.to_string_lossy();
    let kotlin_path = if output_str.ends_with("kotlin") || output_str.contains("/kotlin/") {
        // Check if the package path is already part of the output directory
        // e.g., ".../kotlin/com/bersihir" already contains the package
        let expected_package_suffix = format!("/kotlin/{}/", package_path);
        let expected_package_suffix_no_slash = format!("/kotlin/{}", package_path);
        if output_str.contains(&expected_package_suffix) || output_str.ends_with(&expected_package_suffix_no_slash) {
            // Output already includes kotlin and package directory
            relative_path.to_string()
        } else {
            // Output includes kotlin but not package (e.g., ".../kotlin/")
            format!("{}/{}", package_path, relative_path)
        }
    } else {
        // Standard: all source files go under kotlin/
        format!("kotlin/{}/{}", package_path, relative_path)
    };
    let file_path = output_dir.join(&kotlin_path);

    // skip_existing: skip any file that already exists on disk
    if skip_existing && file_path.exists() {
        return Ok(WriteOutcome::Skipped(file_path));
    }

    // Always skip files that contain the custom code marker (even without --skip-existing)
    if file_path.exists() {
        if let Ok(existing) = std::fs::read_to_string(&file_path) {
            if existing.contains("// <<< CUSTOM") {
                return Ok(WriteOutcome::Skipped(file_path));
            }
        }
    }

    // Create parent directories if they don't exist
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Write the file
    std::fs::write(&file_path, content)?;

    Ok(WriteOutcome::Written(file_path))
}

/// Result of code generation
#[derive(Debug, Default)]
pub struct GenerationResult {
    /// Files that were generated
    pub generated_files: Vec<std::path::PathBuf>,
    /// Files that were skipped
    pub skipped_files: Vec<std::path::PathBuf>,
    /// Stale files removed during this run
    pub stale_deleted_files: Vec<std::path::PathBuf>,
    /// Number of entities processed
    pub entities_count: usize,
    /// Number of enums processed
    pub enums_count: usize,
    /// Number of repositories processed
    pub repositories_count: usize,
    /// Number of use cases processed
    pub usecases_count: usize,
    /// Number of services processed
    pub services_count: usize,
    /// Number of mappers processed
    pub mappers_count: usize,
    /// Number of validators processed
    pub validators_count: usize,
    /// Number of API clients processed
    pub api_clients_count: usize,
    /// Number of ViewModels processed
    pub viewmodels_count: usize,
    /// Number of components processed
    pub components_count: usize,
}

impl GenerationResult {
    /// Merge another result into this one
    pub fn merge(&mut self, other: GenerationResult) {
        self.generated_files.extend(other.generated_files);
        self.skipped_files.extend(other.skipped_files);
        self.stale_deleted_files.extend(other.stale_deleted_files);
        self.entities_count += other.entities_count;
        self.enums_count += other.enums_count;
        self.repositories_count += other.repositories_count;
        self.usecases_count += other.usecases_count;
        self.services_count += other.services_count;
        self.mappers_count += other.mappers_count;
        self.validators_count += other.validators_count;
        self.api_clients_count += other.api_clients_count;
        self.viewmodels_count += other.viewmodels_count;
        self.components_count += other.components_count;
    }

    /// Get the total number of files generated
    pub fn total_generated(&self) -> usize {
        self.generated_files.len()
    }

    /// Get the total number of files processed
    pub fn total_processed(&self) -> usize {
        self.generated_files.len() + self.skipped_files.len()
    }
}
