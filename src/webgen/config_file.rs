//! Configuration file support for metaphor-webgen
//!
//! This module handles loading and parsing of metaphor-webgen.yaml configuration files.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use crate::webgen::config::Config;
use crate::webgen::error::{Error, Result};

/// Project-level configuration file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Default modules directory
    pub modules_dir: Option<String>,

    /// Default output directory for generated code
    pub output_dir: Option<String>,

    /// Domain import pattern for TypeScript imports
    pub domain_import_pattern: Option<String>,

    /// Default generation targets
    pub default_targets: Option<String>,

    /// Force overwrite existing generated files without prompting
    pub force: Option<bool>,

    /// Dry run mode - show what would be generated without writing files
    pub dry_run: Option<bool>,

    /// Module-specific overrides
    pub modules: Option<std::collections::HashMap<String, ModuleConfig>>,
}

/// Module-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleConfig {
    /// Output directory override for this module
    pub output_dir: Option<String>,

    /// Domain import pattern override for this module
    pub domain_import_pattern: Option<String>,

    /// Generation targets override for this module
    pub targets: Option<String>,
}

impl ProjectConfig {
    /// Load configuration from a file
    pub fn load_from_file(path: &PathBuf) -> Result<Self> {
        let content = fs::read_to_string(path)
            .map_err(|e| Error::Parse(format!("Failed to read config file {}: {}", path.display(), e)))?;

        Self::parse(&content)
    }

    /// Parse configuration from YAML string
    pub fn parse(content: &str) -> Result<Self> {
        serde_yaml::from_str(content)
            .map_err(|e| Error::Parse(format!("Failed to parse config YAML: {}", e)))
    }

    /// Create a Config for a specific module from this project config
    pub fn module_config(&self, module: &str) -> Config {
        let mut config = Config::new(module);

        // Apply module-specific overrides if they exist
        if let Some(modules) = &self.modules {
            if let Some(module_config) = modules.get(module) {
                if let Some(output_dir) = &module_config.output_dir {
                    config = config.with_output_dir(output_dir);
                }
                if let Some(domain_import) = &module_config.domain_import_pattern {
                    config = config.with_domain_import_pattern(domain_import.clone());
                }
                if let Some(targets) = &module_config.targets {
                    config = config.with_targets_str(targets);
                }
                return config;
            }
        }

        // Apply global defaults
        if let Some(modules_dir) = &self.modules_dir {
            config = config.with_modules_dir(modules_dir);
        }
        if let Some(output_dir) = &self.output_dir {
            config = config.with_output_dir(output_dir);
        }
        if let Some(domain_import) = &self.domain_import_pattern {
            config = config.with_domain_import_pattern(domain_import.clone());
        }
        if let Some(targets) = &self.default_targets {
            config = config.with_targets_str(targets);
        }
        if let Some(force) = self.force {
            config = config.with_force(force);
        }
        if let Some(dry_run) = self.dry_run {
            config = config.with_dry_run(dry_run);
        }

        config
    }

    /// Find and load configuration file from current directory
    pub fn discover() -> Option<Self> {
        let paths = [
            "metaphor-webgen.yaml",
            "metaphor-webgen.yml",
            ".metaphor-webgen.yaml",
            ".metaphor-webgen.yml",
        ];

        for path in paths {
            let path_buf = PathBuf::from(path);
            if path_buf.exists() {
                return Self::load_from_file(&path_buf).ok();
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config() {
        let yaml = r#"
modules_dir: libs/modules
output_dir: apps/webapp/src
domain_import_pattern: "@webapp/domain/{module}"
default_targets: all
force: false
dry_run: false

modules:
  sapiens:
    output_dir: apps/webapp/src/modules/sapiens
    domain_import_pattern: "@webapp/domain/sapiens"
"#;

        let config = ProjectConfig::parse(yaml).unwrap();
        assert_eq!(config.modules_dir, Some("libs/modules".to_string()));
        assert_eq!(config.output_dir, Some("apps/webapp/src".to_string()));
        assert!(config.modules.is_some());
    }

    #[test]
    fn test_module_config() {
        let yaml = r#"
modules_dir: libs/modules
output_dir: apps/webapp/src

modules:
  sapiens:
    output_dir: apps/webapp/src/modules/sapiens
    targets: hooks,schemas
"#;

        let project_config = ProjectConfig::parse(yaml).unwrap();
        let config = project_config.module_config("sapiens");

        assert_eq!(config.module, "sapiens");
        assert_eq!(config.output_dir, PathBuf::from("apps/webapp/src/modules/sapiens"));
    }
}
