//! Configuration for webapp code generation

use std::path::PathBuf;
use crate::webgen::{Error, Result};

/// Code generation target
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Generate all webapp code
    All,
    /// Generate React Query hooks
    Hooks,
    /// Generate Zod validation schemas
    Schemas,
    /// Generate form components
    Forms,
    /// Generate CRUD pages
    Pages,
    /// Generate types from proto (already done by buf)
    Types,
    /// Generate workflow UI components
    Workflows,
    /// Generate state machine UI components
    StateMachines,
    /// Generate routing and navigation
    Routing,
    /// Generate enhanced CRUD (uses YAML schemas)
    EnhancedCrud,
    /// Generate DDD domain layer (entity types, Zod schemas, services, commands, queries, events, specifications)
    Domain,
    /// Generate presentation layer (forms, tables, pages, detail views)
    Presentation,
    /// Generate application layer (use cases, app services)
    Application,
    /// Generate infrastructure layer (API clients, repository implementations)
    Infrastructure,
}

impl Target {
    /// Parse target string to Target
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "all" => Some(Self::All),
            "hooks" => Some(Self::Hooks),
            "schemas" | "zod" => Some(Self::Schemas),
            "forms" => Some(Self::Forms),
            "pages" | "crud" => Some(Self::Pages),
            "types" => Some(Self::Types),
            "workflows" | "workflow" => Some(Self::Workflows),
            "state-machines" | "state-machine" | "states" => Some(Self::StateMachines),
            "routing" | "routes" | "nav" => Some(Self::Routing),
            "enhanced" | "enhanced-crud" => Some(Self::EnhancedCrud),
            "domain" | "ddd" => Some(Self::Domain),
            "presentation" | "ui" => Some(Self::Presentation),
            "application" | "app" | "usecases" => Some(Self::Application),
            "infrastructure" | "infra" | "api" => Some(Self::Infrastructure),
            _ => None,
        }
    }

    /// Parse comma-separated targets
    pub fn from_targets(targets: &str) -> Vec<Self> {
        let mut result = Vec::new();
        for target in targets.split(',') {
            let target = target.trim();
            if let Some(t) = Self::parse(target) {
                if !result.contains(&t) {
                    result.push(t);
                }
            }
        }
        // If "all" is specified, return all targets including new Clean Architecture layers
        if result.contains(&Self::All) {
            vec![
                // Legacy basic targets
                Self::Hooks,
                Self::Schemas,
                Self::Forms,
                Self::Pages,
                Self::Types,
                Self::Workflows,
                Self::StateMachines,
                Self::Routing,
                // Clean Architecture layers (schema-driven)
                Self::Domain,
                Self::Application,
                Self::Presentation,
                Self::Infrastructure,
            ]
        } else {
            result
        }
    }

    /// Get target directory name
    pub fn dir_name(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Hooks => "hooks",
            Self::Schemas => "validators",
            Self::Forms => "components",
            Self::Pages => "pages",
            Self::Types => "types",
            Self::Workflows => "workflows",
            Self::StateMachines => "state-machines",
            Self::Routing => "routing",
            Self::EnhancedCrud => "enhanced",
            Self::Domain => "domain",
            Self::Presentation => "presentation",
            Self::Application => "application",
            Self::Infrastructure => "infrastructure",
        }
    }

    /// Check if target is an enhanced generation target (uses YAML schemas)
    pub fn is_enhanced(&self) -> bool {
        matches!(
            self,
            Self::Workflows
            | Self::StateMachines
            | Self::Routing
            | Self::EnhancedCrud
            | Self::Domain
            | Self::Presentation
            | Self::Application
            | Self::Infrastructure
        )
    }
}

/// Configuration for webapp code generation
#[derive(Debug, Clone)]
pub struct Config {
    /// Module name (e.g., "sapiens", "katalog")
    pub module: String,

    /// Generation targets
    pub targets: Vec<Target>,

    /// Entity filter (only generate for specific entity)
    pub entity_filter: Option<String>,

    /// Output directory (default: apps/webapp/src)
    pub output_dir: PathBuf,

    /// Proto modules directory (default: libs/modules)
    pub modules_dir: PathBuf,

    /// Domain import path pattern (default: @webapp/domain/{module})
    pub domain_import_pattern: String,

    /// Dry run - show what would be generated without writing files
    pub dry_run: bool,

    /// Force overwrite existing files
    pub force: bool,
}

impl Config {
    /// Create a new config for a module
    pub fn new(module: impl Into<String>) -> Self {
        let module = module.into();
        Self {
            module,
            targets: vec![Target::All],
            entity_filter: None,
            output_dir: PathBuf::from("apps/webapp/src"),
            modules_dir: PathBuf::from("libs/modules"),
            domain_import_pattern: "@webapp/domain/{module}".to_string(),
            dry_run: false,
            force: false,
        }
    }

    /// Set generation targets
    pub fn with_targets(mut self, targets: Vec<Target>) -> Self {
        self.targets = targets;
        self
    }

    /// Set generation targets from string
    pub fn with_targets_str(mut self, targets: &str) -> Self {
        self.targets = Target::from_targets(targets);
        self
    }

    /// Set entity filter
    pub fn with_entity(mut self, entity: Option<String>) -> Self {
        self.entity_filter = entity;
        self
    }

    /// Set output directory
    pub fn with_output_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.output_dir = dir.into();
        self
    }

    /// Set modules directory
    pub fn with_modules_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.modules_dir = dir.into();
        self
    }

    /// Set domain import pattern
    pub fn with_domain_import_pattern(mut self, pattern: String) -> Self {
        self.domain_import_pattern = pattern;
        self
    }

    /// Set dry run mode
    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    /// Set force mode
    pub fn with_force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        if self.module.is_empty() {
            return Err(Error::InvalidModule("Module name cannot be empty".to_string()));
        }

        // Check if module name is valid (alphanumeric and underscores)
        if !self.module.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(Error::InvalidModule(format!(
                "Invalid module name '{}': only alphanumeric characters and underscores allowed",
                self.module
            )));
        }

        Ok(())
    }

    /// Get the proto directory for this module
    pub fn proto_dir(&self) -> PathBuf {
        self.modules_dir.join(&self.module).join("proto")
    }

    /// Get the schema directory for this module
    pub fn schema_dir(&self) -> PathBuf {
        self.modules_dir.join(&self.module).join("schema")
    }

    /// Get the domain import path for this module
    pub fn domain_import_path(&self) -> String {
        self.domain_import_pattern.replace("{module}", &self.module)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new("sapiens")
    }
}
