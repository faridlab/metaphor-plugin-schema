//! Repository interface generator for TypeScript domain layer.
//!
//! Emits a thin per-entity port that extends the generic `CrudRepository`.

use std::fs;

use crate::webgen::ast::entity::EntityDefinition;
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::to_pascal_case;
use super::DomainGenerationResult;

/// Generator for repository interfaces
pub struct RepositoryGenerator {
    config: Config,
}

impl RepositoryGenerator {
    /// Create a new repository generator
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Generate repository interface for an entity
    pub fn generate(&self, entity: &EntityDefinition) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let repo_dir = self
            .config
            .output_dir
            .join(&self.config.module)
            .join("domain")
            .join("repository");

        if !self.config.dry_run {
            fs::create_dir_all(&repo_dir).ok();
        }

        let content = self.generate_repository_content(entity);
        let path = repo_dir.join(format!("{}Repository.ts", entity_pascal));

        result.add_file(path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&path, content).ok();
        }

        Ok(result)
    }

    /// Generate the thin repository port content.
    fn generate_repository_content(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);

        format!(
            r#"/**
 * {entity_pascal} Repository port
 *
 * Extends the generic `CrudRepository`. Implementations live in the
 * infrastructure layer.
 *
 * @module {module}/repository/{entity_pascal}Repository
 */

import type {{ CrudRepository }} from '{root}/shared/crud/CrudRepository';
import type {{ PaginatedResponse }} from '{root}/shared/types/pagination';
import type {{
  {entity_pascal},
  Create{entity_pascal}Input,
  Update{entity_pascal}Input,
  {entity_pascal}QueryParams,
  {entity_pascal}FilterParams,
}} from '../entity/{entity_pascal}.schema';

export interface {entity_pascal}Repository
  extends CrudRepository<
    {entity_pascal},
    Create{entity_pascal}Input,
    Update{entity_pascal}Input,
    {entity_pascal}QueryParams,
    {entity_pascal}FilterParams
  > {{}}

/** Back-compat alias for the paginated list response. */
export type Paginated{entity_pascal}Response = PaginatedResponse<{entity_pascal}>;
"#,
            entity_pascal = entity_pascal,
            module = self.config.module,
            root = self.config.import_root,
        )
    }
}
