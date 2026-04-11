//! Kotlin Multiplatform Mobile code generation.
//!
//! Originally a separate crate (`backbone-mobilegen` / `metaphor-plugin-mobilegen`).
//! Merged into `metaphor-plugin-schema` because mobilegen was already a thin layer
//! on top of schema's parser and AST — separating them was historical inertia.
//!
//! Public surface: [`generate`] is the entry point that reads a module's schema
//! directory and writes Kotlin code to an output directory. The CLI subcommand
//! that exposes this lives in `crate::commands::kotlin`.

pub mod config;
pub mod error;
pub mod generators;
pub mod lang;
pub mod package_detector;
pub mod templates;

pub use config::{GeneratorConfig, GenerationTarget};
pub use error::{MobileGenError, Result};
pub use generators::MobileGenerator;
pub use lang::KotlinTypeMapper;
pub use package_detector::{detect_package, resolve_package, PackageInfo, PackageSource};

use std::path::Path;

use crate::ast::ModuleSchema;
use crate::parser;

/// Walk a `<module>/schema` directory and produce a [`ModuleSchema`] AST.
///
/// Lifted directly from the original mobilegen `main.rs::parse_module_schema`.
/// Lives here so the kotlin module is fully self-contained — `crate::commands::kotlin`
/// only needs to call `kotlin::parse_module_schema` and then `MobileGenerator::generate`.
pub fn parse_module_schema(
    schema_path: &Path,
    module_name: &str,
) -> Result<ModuleSchema> {
    let mut schema = ModuleSchema::new(module_name);

    // Directories to skip (non-model directories)
    const SKIP_DIRS: &[&str] = &["hooks", "workflows", "openapi"];

    fn find_model_files(
        dir: &Path,
        files: &mut Vec<std::path::PathBuf>,
    ) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if SKIP_DIRS.contains(&dir_name) {
                    continue;
                }
                find_model_files(&path, files)?;
            } else if path.extension().and_then(|s| s.to_str()) == Some("yaml")
                || path.extension().and_then(|s| s.to_str()) == Some("yml")
            {
                files.push(path);
            }
        }
        Ok(())
    }

    let mut model_files = Vec::new();
    find_model_files(schema_path, &mut model_files)
        .map_err(|e| error::MobileGenError::SchemaParse(format!("read schema dir: {e}")))?;

    for path in model_files {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| error::MobileGenError::SchemaParse(format!("read {}: {e}", path.display())))?;

        if parser::is_model_index_file(&content) {
            if let Ok(index) = parser::parse_model_index_yaml_str(&content) {
                if let Some(cfg) = index.config.and_then(|c| c.generators) {
                    schema.generators_config = Some(cfg);
                }
            }
        } else {
            let yaml_schema = parser::parse_model_yaml_str(&content)
                .map_err(|e| error::MobileGenError::SchemaParse(format!("parse {}: {e}", path.display())))?;

            let file_disabled: Vec<String> = yaml_schema
                .generators
                .as_ref()
                .and_then(|g| g.disabled.clone())
                .unwrap_or_default();
            let file_enabled: Vec<String> = yaml_schema
                .generators
                .as_ref()
                .and_then(|g| g.enabled.clone())
                .unwrap_or_default();

            for yaml_enum in &yaml_schema.enums {
                schema.enums.push(yaml_enum.clone().into_enum());
            }
            let mut models = yaml_schema.into_models();

            for model in &mut models {
                if model.disabled_generators.is_empty() && !file_disabled.is_empty() {
                    model.disabled_generators = file_disabled.clone();
                }
                if model.enabled_generators.is_empty() && !file_enabled.is_empty() {
                    model.enabled_generators = file_enabled.clone();
                }
            }
            schema.models.extend(models);
        }
    }

    Ok(schema)
}
