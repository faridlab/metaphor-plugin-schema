//! Domain Service generator.
//!
//! Emits a thin per-entity service port (extends the generic `CrudService` /
//! `SoftDeleteCrudService`) plus an injectable singleton accessor. Pure — no
//! `@tanstack/react-query`; data-access hooks are the app's hand-written
//! phenotype.

use std::fs;

use crate::webgen::ast::entity::EntityDefinition;
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::to_pascal_case;
use super::DomainGenerationResult;

/// Generator for the pure domain service port.
pub struct DomainServiceGenerator {
    config: Config,
}

impl DomainServiceGenerator {
    /// Create a new domain service generator
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Generate domain service for an entity
    pub fn generate(&self, entity: &EntityDefinition) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let service_dir = self
            .config
            .output_dir
            .join(&self.config.module)
            .join("domain")
            .join("service");

        if !self.config.dry_run {
            fs::create_dir_all(&service_dir).ok();
        }

        let content = self.generate_service_content(entity);
        let path = service_dir.join(format!("{}Service.ts", entity_pascal));

        result.add_file(path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&path, content).ok();
        }

        Ok(result)
    }

    /// Generate the thin service port content.
    fn generate_service_content(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let base = if entity.has_soft_delete() {
            "SoftDeleteCrudService"
        } else {
            "CrudService"
        };

        format!(
            r#"/**
 * {entity_pascal} Service port + injectable accessor
 *
 * Extends the generic `{base}`. The infrastructure API client implements it;
 * the application layer consumes it via `get{entity_pascal}Service()`.
 *
 * @module {module}/service/{entity_pascal}Service
 */

import {{ makeServiceAccessor, type {base} }} from '{root}/shared/crud/CrudService';
import type {{
  {entity_pascal},
  Create{entity_pascal}Input,
  Update{entity_pascal}Input,
  {entity_pascal}QueryParams,
  {entity_pascal}FilterParams,
}} from '../entity/{entity_pascal}.schema';

export interface {entity_pascal}Service
  extends {base}<
    {entity_pascal},
    Create{entity_pascal}Input,
    Update{entity_pascal}Input,
    {entity_pascal}QueryParams,
    {entity_pascal}FilterParams
  > {{}}

const accessor = makeServiceAccessor<{entity_pascal}Service>('{entity_pascal}');

/** Inject the {entity_pascal} service implementation (call once at startup). */
export const set{entity_pascal}Service = accessor.set;

/** Get the {entity_pascal} service instance (throws if not initialized). */
export const get{entity_pascal}Service = accessor.get;
"#,
            entity_pascal = entity_pascal,
            module = self.config.module,
            root = self.config.import_root,
            base = base,
        )
    }
}
