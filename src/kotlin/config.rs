//! Generator configuration

use crate::kotlin::package_detector::{detect_package, PackageInfo};
use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Mobile code generator configuration
#[derive(Debug, Clone, Parser, Serialize, Deserialize)]
#[command(name = "backbone-mobilegen")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Kotlin Multiplatform Mobile code generator for Backbone Framework", long_about = None)]
pub struct GeneratorConfig {
    /// Module name to generate code for
    #[arg(short, long)]
    pub module: String,

    /// Target app name (e.g., mobileapp, webapp) for output directory
    #[arg(long, default_value = "mobileapp")]
    pub app: String,

    /// Output directory for generated code (overrides --app)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Package name for generated Kotlin code (auto-detects from project if not provided)
    #[arg(short, long)]
    pub package: Option<String>,

    /// Generation targets (default: all)
    #[arg(short, long, value_delimiter = ',')]
    pub target: Vec<GenerationTarget>,

    /// Module base path (where libs/modules/ is located)
    #[arg(long, default_value = "libs/modules")]
    pub module_path: PathBuf,

    /// Whether to skip existing files
    #[arg(long, default_value = "false")]
    pub skip_existing: bool,

    /// Verbose output
    #[arg(short, long, default_value = "false")]
    pub verbose: bool,
}

impl GeneratorConfig {
    /// Detect the base package from the project directory
    ///
    /// This is called when --package is not explicitly provided.
    /// It scans the output directory for build.gradle.kts and existing Kotlin files.
    fn detect_base_package(&self) -> PackageInfo {
        let output_dir = self.output_path();
        detect_package(&output_dir)
    }

    /// Get the base package name
    ///
    /// Returns the package from --package flag if provided,
    /// otherwise auto-detects from the project directory.
    pub fn base_package_name(&self) -> String {
        if let Some(pkg) = &self.package {
            // User explicitly provided a package
            pkg.replace("{module}", &self.module.to_lowercase())
        } else {
            // Auto-detect from project
            let info = self.detect_base_package();
            info.base_package
        }
    }

    /// Get the actual package name for a specific layer
    ///
    /// Formats the package as: {base_package}.{layer}.{module}
    /// Example: com.bersihir.domain.sapiens
    pub fn package_name_for(&self, layer: &str) -> String {
        let base = self.base_package_name();
        format!("{}.{}.{}", base, layer, self.module.to_lowercase())
    }

    /// Get the actual package name
    /// For mobile app, use detected base package, then layer/module path
    ///
    /// @deprecated Use package_name_for() instead for clarity
    pub fn package_name(&self) -> String {
        self.base_package_name()
    }

    /// Get the package info (including detection source)
    pub fn package_info(&self) -> PackageInfo {
        if self.package.is_some() {
            PackageInfo {
                base_package: self.base_package_name(),
                source: crate::kotlin::package_detector::PackageSource::Default,
            }
        } else {
            self.detect_base_package()
        }
    }

    /// Get the actual output path
    /// If --output is not specified, uses apps/{app}/shared/src/commonMain/
    /// Note: The generator adds kotlin/id/startapp/ automatically
    pub fn output_path(&self) -> PathBuf {
        self.output
            .as_ref()
            .map(|p| {
                p.to_str()
                    .map(|s| s.replace("{module}", &self.module.to_lowercase()))
                    .map(PathBuf::from)
                    .unwrap_or_else(|| p.clone())
            })
            .unwrap_or_else(|| {
                // Default path based on --app option (without kotlin/id/startapp prefix)
                PathBuf::from(format!(
                    "apps/{}/shared/src/commonMain",
                    self.app
                ))
            })
    }

    /// Get the actual targets (defaults to All if empty)
    pub fn targets(&self) -> Vec<GenerationTarget> {
        if self.target.is_empty() {
            vec![GenerationTarget::All]
        } else {
            self.target.clone()
        }
    }
}

/// Generation targets for mobile code
#[derive(Debug, Clone, Copy, ValueEnum, Serialize, Deserialize, PartialEq, Eq)]
pub enum GenerationTarget {
    /// Generate all targets
    All,
    /// Domain entities (data classes)
    Entities,
    /// Enums (sealed classes)
    Enums,
    /// Repository interfaces (domain layer)
    Repositories,
    /// Offline-first repository implementations (infrastructure layer)
    ///
    /// Generates one `Offline<Entity>Repository.kt` per model that wraps the
    /// matching `<Entity>ApiClient` with cache-first reads, cache-aware writes
    /// (delete invalidates lists), and offline fallback. Subclasses opt into
    /// delta-sync by overriding `fetchListSinceFromApi` in a companion
    /// `*RepositoryCustom.kt` file marked with `// <<< CUSTOM`.
    OfflineRepositories,
    /// Use cases (application layer)
    UseCases,
    /// Application services (application layer)
    AppServices,
    /// API clients (Ktor)
    ApiClients,
    /// Database schemas (SQLDelight)
    Database,
    /// Offline sync managers
    Sync,
    /// MVI ViewModels
    ViewModels,
    /// Reusable UI components
    Components,
    /// Navigation (Decompose)
    Navigation,
    /// Material 3 theme
    Theme,
    /// Input validators
    Validators,
    /// Data mappers (application layer)
    Mappers,
    /// Generated test stubs (validator tests, ViewModel tests, API client mock tests)
    Tests,
}

impl GenerationTarget {
    /// Return the snake_case YAML name for this target (matches schema generators.disabled values)
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Entities => "entities",
            Self::Enums => "enums",
            Self::Repositories => "repositories",
            Self::OfflineRepositories => "offlinerepositories",
            Self::UseCases => "usecases",
            Self::AppServices => "appservices",
            Self::ApiClients => "apiclients",
            Self::Database => "database",
            Self::Sync => "sync",
            Self::ViewModels => "viewmodels",
            Self::Components => "components",
            Self::Navigation => "navigation",
            Self::Theme => "theme",
            Self::Validators => "validators",
            Self::Mappers => "mappers",
            Self::Tests => "tests",
        }
    }

    /// Check if this target should be generated when using "all"
    pub fn is_included_in_all(&self) -> bool {
        matches!(self, Self::All)
    }

    /// Get all non-All targets
    pub fn all_targets() -> &'static [GenerationTarget] {
        &[
            GenerationTarget::Entities,
            GenerationTarget::Enums,
            GenerationTarget::Repositories,
            GenerationTarget::OfflineRepositories,
            GenerationTarget::UseCases,
            GenerationTarget::AppServices,
            GenerationTarget::ApiClients,
            GenerationTarget::Database,
            GenerationTarget::Sync,
            GenerationTarget::ViewModels,
            GenerationTarget::Components,
            GenerationTarget::Navigation,
            GenerationTarget::Theme,
            GenerationTarget::Validators,
            GenerationTarget::Mappers,
            GenerationTarget::Tests,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_name_replacement() {
        let config = GeneratorConfig {
            module: "Bersihir".to_string(),
            app: "mobileapp".to_string(),
            package: Some("com.backbone.{module}".to_string()),
            ..Default::default()
        };
        assert_eq!(config.package_name(), "com.backbone.bersihir");
    }

    #[test]
    fn test_output_path_replacement() {
        let config = GeneratorConfig {
            module: "Bersihir".to_string(),
            app: "mobileapp".to_string(),
            output: Some(PathBuf::from("../apps/{module}-mobile")),
            ..Default::default()
        };
        assert_eq!(
            config.output_path(),
            PathBuf::from("../apps/bersihir-mobile")
        );
    }

    #[test]
    fn test_output_path_with_app() {
        let config = GeneratorConfig {
            module: "sapiens".to_string(),
            app: "mobileapp".to_string(),
            output: None,
            ..Default::default()
        };
        assert_eq!(
            config.output_path(),
            PathBuf::from("apps/mobileapp/shared/src/commonMain")
        );
    }
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self {
            module: String::new(),
            app: "mobileapp".to_string(),
            output: None,
            package: None,
            target: vec![GenerationTarget::All],
            module_path: PathBuf::from("../../libs/modules"),
            skip_existing: false,
            verbose: false,
        }
    }
}
