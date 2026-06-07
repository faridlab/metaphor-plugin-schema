//! Repository implementation generator.
//!
//! Emits a thin per-entity repo that extends the generic `BaseRepositoryImpl`,
//! delegating to the entity's API client.

use std::fs;

use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition};
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::generators::domain::{DomainGenerationResult, EntityGenerator};
use crate::webgen::parser::to_pascal_case;

/// Generator for repository implementations
pub struct RepositoryImplGenerator {
    config: Config,
}

impl RepositoryImplGenerator {
    /// Create a new repository implementation generator
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Generate repository implementation for an entity
    pub fn generate(
        &self,
        entity: &EntityDefinition,
        _enums: &[EnumDefinition],
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let repo_dir = self
            .config
            .output_dir
            .join(&self.config.module)
            .join("infrastructure")
            .join("repository");

        if !self.config.dry_run {
            fs::create_dir_all(&repo_dir).ok();
        }

        let content = self.generate_repository_impl_content(entity);
        let file_path = repo_dir.join(format!("{}RepositoryImpl.ts", entity_pascal));

        result.add_file(file_path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            crate::webgen::custom_blocks::preserve_and_write(&file_path, content).ok();
        }

        Ok(result)
    }

    /// Generate the thin repository implementation content.
    fn generate_repository_impl_content(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let pk = EntityGenerator::primary_key(entity);

        format!(
            r#"/**
 * {entity_pascal} Repository implementation
 *
 * Extends the generic `BaseRepositoryImpl`, delegating to the API client.
 *
 * @module infrastructure/{module}/repository/{entity_pascal}RepositoryImpl
 */

import {{ BaseRepositoryImpl }} from '{root}/shared/crud/BaseRepositoryImpl';
import {{ get{entity_pascal}ApiClient }} from '../api/{entity_pascal}ApiClient';
import type {{
  {entity_pascal},
  Create{entity_pascal}Input,
  Update{entity_pascal}Input,
  {entity_pascal}QueryParams,
  {entity_pascal}FilterParams,
}} from '{root}/{module}/domain/entity/{entity_pascal}.schema';
import type {{ {entity_pascal}Repository }} from '{root}/{module}/domain/repository/{entity_pascal}Repository';

export class {entity_pascal}RepositoryImpl
  extends BaseRepositoryImpl<
    {entity_pascal},
    Create{entity_pascal}Input,
    Update{entity_pascal}Input,
    {entity_pascal}QueryParams,
    {entity_pascal}FilterParams
  >
  implements {entity_pascal}Repository
{{
  constructor() {{
    super(get{entity_pascal}ApiClient(), '{pk}');
  }}
}}

let _repo: {entity_pascal}RepositoryImpl | null = null;

/** Get the shared {entity_pascal} repository instance. */
export function get{entity_pascal}Repository(): {entity_pascal}Repository {{
  return (_repo ??= new {entity_pascal}RepositoryImpl());
}}
"#,
            entity_pascal = entity_pascal,
            module = self.config.module,
            root = self.config.import_root,
            pk = pk,
        )
    }
}
