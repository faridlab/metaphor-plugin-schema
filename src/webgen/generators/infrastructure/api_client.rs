//! API client generator.
//!
//! Emits a thin per-entity client that extends the generic
//! `BaseCrudApiClient` / `SoftDeleteCrudApiClient` from `shared/crud`. The
//! concrete client only declares its `module` + `collection`.

use std::fs;

use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition};
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::generators::domain::DomainGenerationResult;
use crate::webgen::parser::{pluralize, to_pascal_case, to_snake_case};

/// Generator for API client implementations
pub struct ApiClientGenerator {
    config: Config,
}

impl ApiClientGenerator {
    /// Create a new API client generator
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Generate API client for an entity
    pub fn generate(
        &self,
        entity: &EntityDefinition,
        _enums: &[EnumDefinition],
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let api_dir = self
            .config
            .output_dir
            .join(&self.config.module)
            .join("infrastructure")
            .join("api");

        if !self.config.dry_run {
            fs::create_dir_all(&api_dir).ok();
        }

        let content = self.generate_api_client_content(entity);
        let file_path = api_dir.join(format!("{}ApiClient.ts", entity_pascal));

        result.add_file(file_path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&file_path, content).ok();
        }

        Ok(result)
    }

    /// Generate the thin API client content.
    fn generate_api_client_content(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_route = pluralize(&to_snake_case(&entity.name));
        let base = if entity.has_soft_delete() {
            "SoftDeleteCrudApiClient"
        } else {
            "BaseCrudApiClient"
        };

        format!(
            r#"/**
 * {entity_pascal} API Client
 *
 * Thin REST client — extends the generic {base} from `shared/crud`,
 * which provides all CRUD over the injectable HTTP transport.
 *
 * @module infrastructure/{module}/api/{entity_pascal}ApiClient
 */

import {{ {base} }} from '{root}/shared/crud/BaseCrudApiClient';
import type {{
  {entity_pascal},
  Create{entity_pascal}Input,
  Update{entity_pascal}Input,
  {entity_pascal}QueryParams,
  {entity_pascal}FilterParams,
}} from '{root}/{module}/domain/entity/{entity_pascal}.schema';

export class {entity_pascal}ApiClient extends {base}<
  {entity_pascal},
  Create{entity_pascal}Input,
  Update{entity_pascal}Input,
  {entity_pascal}QueryParams,
  {entity_pascal}FilterParams
> {{
  protected readonly module = '{module}';
  protected readonly collection = '{entity_route}';
}}

let _client: {entity_pascal}ApiClient | null = null;

/** Get the shared {entity_pascal} API client instance. */
export function get{entity_pascal}ApiClient(): {entity_pascal}ApiClient {{
  return (_client ??= new {entity_pascal}ApiClient());
}}
"#,
            entity_pascal = entity_pascal,
            entity_route = entity_route,
            module = self.config.module,
            root = self.config.import_root,
            base = base,
        )
    }
}
