//! Application service generator.
//!
//! Emits a thin per-entity application service built from the generic
//! `makeCrudAppService` factory in `shared/crud`.

use std::fs;

use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition};
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::generators::domain::DomainGenerationResult;
use crate::webgen::parser::{to_camel_case, to_pascal_case};

/// Generator for application services
pub struct AppServiceGenerator {
    config: Config,
}

impl AppServiceGenerator {
    /// Create a new application service generator
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Generate application service for an entity
    pub fn generate(
        &self,
        entity: &EntityDefinition,
        _enums: &[EnumDefinition],
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let services_dir = self
            .config
            .output_dir
            .join(&self.config.module)
            .join("application")
            .join("services");

        if !self.config.dry_run {
            fs::create_dir_all(&services_dir).ok();
        }

        let content = self.generate_app_service_content(entity);
        let file_path = services_dir.join(format!("{}AppService.ts", entity_pascal));

        result.add_file(file_path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&file_path, content).ok();
        }

        Ok(result)
    }

    /// Generate the thin application-service content.
    fn generate_app_service_content(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_camel = to_camel_case(&entity.name);

        format!(
            r#"/**
 * {entity_pascal} application service
 *
 * Thin orchestration over the service port (get / list / create / update /
 * patch / remove). Built from the generic `makeCrudAppService`.
 *
 * @module application/{module}/services/{entity_pascal}AppService
 */

import {{ makeCrudAppService }} from '{root}/shared/crud/crudAppService';
import {{ get{entity_pascal}Service }} from '{root}/{module}/domain/service/{entity_pascal}Service';

export const {entity_camel}AppService = makeCrudAppService(get{entity_pascal}Service);
"#,
            entity_pascal = entity_pascal,
            entity_camel = entity_camel,
            module = self.config.module,
            root = self.config.import_root,
        )
    }
}
