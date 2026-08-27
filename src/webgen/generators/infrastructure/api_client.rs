//! API client generator.
//!
//! Emits a thin per-entity client that extends the generic
//! `BaseCrudApiClient` / `SoftDeleteCrudApiClient` from `shared/crud`. The
//! concrete client only declares its `module` + `collection`. The `module`
//! value is the module's schema name (see [`Config::url_segment`]), so client
//! base paths are namespaced by what the module owns (`/api/v1/sapiens/...`)
//! rather than by the crate key the generation was invoked with.

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
            crate::webgen::custom_blocks::preserve_and_write(&file_path, content).ok();
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
  protected readonly module = '{url_module}';
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
            url_module = self.config.url_segment(),
            root = self.config.import_root,
            base = base,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_entity() -> EntityDefinition {
        EntityDefinition {
            name: "Customer".to_string(),
            collection: "customers".to_string(),
            fields: vec![],
            relations: vec![],
            indexes: vec![],
            soft_delete: false,
        }
    }

    /// Create a unique temp schema dir with the given `models/index.model.yaml`
    /// body (`None` = no index). Returns the schema dir.
    fn temp_schema_dir(index_body: Option<&str>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "webgen-api-client-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let models = dir.join("models");
        std::fs::create_dir_all(&models).expect("create temp schema models dir");
        if let Some(body) = index_body {
            std::fs::write(models.join("index.model.yaml"), body).expect("write index");
        }
        dir
    }

    #[test]
    fn emitted_client_uses_schema_name_segment_not_crate_key() {
        // A backbone module addressed by its crate key: the schema index
        // declares `sapiens`, so the client must target /api/v1/sapiens/...
        let dir = temp_schema_dir(Some("module: sapiens\nschema: sapiens\n"));
        let config = Config::new("backbone_sapiens").with_schema_dir(Some(dir.clone()));
        let generator = ApiClientGenerator::new(config);
        let content = generator.generate_api_client_content(&test_entity());
        assert!(
            content.contains("protected readonly module = 'sapiens';"),
            "expected schema-name segment in emitted client:\n{content}"
        );
        assert!(
            !content.contains("module = 'backbone_sapiens';"),
            "client must not target the crate name:\n{content}"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn emitted_client_falls_back_to_module_name_without_index() {
        // No schema index: the configured module key is the segment.
        let dir = temp_schema_dir(None);
        let config = Config::new("backbone_catalog").with_schema_dir(Some(dir.clone()));
        let generator = ApiClientGenerator::new(config);
        let content = generator.generate_api_client_content(&test_entity());
        assert!(content.contains("protected readonly module = 'backbone_catalog';"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn emitted_client_has_empty_segment_for_api_root_module() {
        // The product module mounts at /api/v1/{collection} (no segment).
        let dir = temp_schema_dir(Some("module: sapiens\nschema: sapiens\n"));
        let config = Config::new("backbone_sapiens")
            .with_schema_dir(Some(dir.clone()))
            .with_api_root(true);
        let generator = ApiClientGenerator::new(config);
        let content = generator.generate_api_client_content(&test_entity());
        assert!(content.contains("protected readonly module = '';"));
        std::fs::remove_dir_all(dir).ok();
    }
}
