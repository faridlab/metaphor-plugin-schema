//! Domain Service generator for TypeScript domain layer
//!
//! Generates a PURE service port: a framework-free interface plus an injectable
//! singleton accessor. No `@tanstack/react-query` (data-access hooks are the
//! app's hand-written phenotype). The infrastructure API client implements this
//! interface; the application layer consumes it via `get{Entity}Service()`.

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
        let service_dir = self.config.output_dir
            .join(&self.config.module).join("domain")
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

    /// Generate the pure service port content (interface + injectable accessor).
    fn generate_service_content(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let has_soft_delete = entity.has_soft_delete();

        // Soft-delete methods, matching exactly what the REST API client implements.
        let soft_delete_methods = if has_soft_delete {
            format!(
                r#"
  getDeleted(params?: {ep}QueryParams, filters?: {ep}FilterParams): Promise<Paginated{ep}Response>;
  getDeletedById(id: string): Promise<{ep}>;
  restore(id: string): Promise<{ep}>;
  permanentDelete(id: string): Promise<void>;
  emptyTrash(): Promise<{{ deleted: number }}>;
  countDeleted(): Promise<number>;"#,
                ep = entity_pascal
            )
        } else {
            String::new()
        };

        format!(
r#"/**
 * {entity_pascal} Domain Service (port)
 *
 * Pure, framework-free service interface plus an injectable singleton accessor.
 * The infrastructure API client implements this interface; the application
 * layer consumes it via `get{entity_pascal}Service()`. Data-access hooks
 * (TanStack Query) are the app's hand-written phenotype and live elsewhere.
 *
 * @module {module}/service/{entity_pascal}Service
 */

import type {{
  {entity_pascal},
  Create{entity_pascal}Input,
  Update{entity_pascal}Input,
  {entity_pascal}QueryParams,
  {entity_pascal}FilterParams,
}} from '../entity/{entity_pascal}.schema';
import type {{ Paginated{entity_pascal}Response }} from '../repository/{entity_pascal}Repository';

// ============================================================================
// Service Interface (port)
// ============================================================================

/**
 * {entity_pascal} Service Interface
 *
 * Implemented by the infrastructure API client.
 */
export interface {entity_pascal}Service {{
  getById(id: string): Promise<{entity_pascal}>;
  getAll(params?: {entity_pascal}QueryParams, filters?: {entity_pascal}FilterParams): Promise<Paginated{entity_pascal}Response>;
  create(input: Create{entity_pascal}Input): Promise<{entity_pascal}>;
  update(id: string, input: Update{entity_pascal}Input): Promise<{entity_pascal}>;
  patch(id: string, input: Partial<Update{entity_pascal}Input>): Promise<{entity_pascal}>;
  delete(id: string): Promise<void>;
  exists(id: string): Promise<boolean>;
  count(filters?: {entity_pascal}FilterParams): Promise<number>;{soft_delete_methods}
}}

// ============================================================================
// Injectable singleton accessor
// ============================================================================

let _service: {entity_pascal}Service | null = null;

/** Inject the {entity_pascal} service implementation (call once at startup). */
export function set{entity_pascal}Service(service: {entity_pascal}Service): void {{
  _service = service;
}}

/**
 * Get the {entity_pascal} service instance.
 * @throws Error if the service has not been initialized.
 */
export function get{entity_pascal}Service(): {entity_pascal}Service {{
  if (!_service) {{
    throw new Error(
      '{entity_pascal}Service not initialized. Call set{entity_pascal}Service() first.'
    );
  }}
  return _service;
}}

// <<< CUSTOM: Add custom service methods here
// END CUSTOM
"#,
            entity_pascal = entity_pascal,
            module = self.config.module,
            soft_delete_methods = soft_delete_methods,
        )
    }
}
