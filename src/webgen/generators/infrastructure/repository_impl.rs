//! Repository implementation generator
//!
//! Generates repository implementations that use API clients for data access.

use std::fs;

use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition};
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::{to_pascal_case, to_camel_case};
use crate::webgen::generators::domain::DomainGenerationResult;

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
        let repo_dir = self.config.output_dir
            .join("infrastructure")
            .join(&self.config.module)
            .join("repository");

        if !self.config.dry_run {
            fs::create_dir_all(&repo_dir).ok();
        }

        // Generate repository implementation file
        let content = self.generate_repository_impl_content(entity);
        let file_path = repo_dir.join(format!("{}RepositoryImpl.ts", entity_pascal));

        result.add_file(file_path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&file_path, content).ok();
        }

        Ok(result)
    }

    /// Generate repository implementation content
    fn generate_repository_impl_content(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_camel = to_camel_case(&entity.name);

        format!(
r#"/**
 * {entity_pascal} Repository Implementation
 *
 * Implements the {entity_pascal}Repository interface using the API client.
 * Bridges the domain layer with infrastructure layer.
 *
 * @module infrastructure/{module}/repository/{entity_pascal}RepositoryImpl
 */

import type {{
  {entity_pascal},
  Create{entity_pascal}Input,
  Update{entity_pascal}Input,
  {entity_pascal}QueryParams,
  {entity_pascal}FilterParams,
}} from '@webapp/domain/{module}/entity/{entity_pascal}.schema';
import type {{
  {entity_pascal}Repository,
  Paginated{entity_pascal}Response,
}} from '@webapp/domain/{module}/repository/{entity_pascal}Repository';
import {{
  get{entity_pascal}ApiClient,
  {entity_pascal}ApiError,
}} from '../api/{entity_pascal}ApiClient';

// ============================================================================
// Repository Implementation
// ============================================================================

/**
 * {entity_pascal} Repository Implementation
 *
 * Uses the {entity_pascal}ApiClient for data operations.
 */
export class {entity_pascal}RepositoryImpl implements {entity_pascal}Repository {{
  private static instance: {entity_pascal}RepositoryImpl;

  private constructor() {{}}

  /**
   * Get singleton instance
   */
  static getInstance(): {entity_pascal}RepositoryImpl {{
    if (!{entity_pascal}RepositoryImpl.instance) {{
      {entity_pascal}RepositoryImpl.instance = new {entity_pascal}RepositoryImpl();
    }}
    return {entity_pascal}RepositoryImpl.instance;
  }}

  /**
   * Find {entity_pascal} by ID
   */
  async findById(id: string): Promise<{entity_pascal} | null> {{
    try {{
      const client = get{entity_pascal}ApiClient();
      return await client.getById(id);
    }} catch (error) {{
      if (error instanceof {entity_pascal}ApiError && error.status === 404) {{
        return null;
      }}
      throw error;
    }}
  }}

  /**
   * Find all {entity_pascal} entities with optional filtering and pagination
   */
  async findAll(
    params?: {entity_pascal}QueryParams,
    filters?: {entity_pascal}FilterParams
  ): Promise<Paginated{entity_pascal}Response> {{
    const client = get{entity_pascal}ApiClient();
    return client.getAll(params, filters);
  }}

  /**
   * Find {entity_pascal} entities matching filter criteria
   */
  async findMany(
    filters: {entity_pascal}FilterParams,
    params?: {entity_pascal}QueryParams
  ): Promise<{entity_pascal}[]> {{
    const client = get{entity_pascal}ApiClient();
    const response = await client.getAll(params, filters);
    return response.items;
  }}

  /**
   * Find a single {entity_pascal} matching filter criteria
   */
  async findOne(filters: {entity_pascal}FilterParams): Promise<{entity_pascal} | null> {{
    const client = get{entity_pascal}ApiClient();
    const response = await client.getAll({{ limit: 1 }}, filters);
    return response.items[0] ?? null;
  }}

  /**
   * Create a new {entity_pascal}
   */
  async create(input: Create{entity_pascal}Input): Promise<{entity_pascal}> {{
    const client = get{entity_pascal}ApiClient();
    return client.create(input);
  }}

  /**
   * Create multiple {entity_pascal} entities
   */
  async createMany(inputs: Create{entity_pascal}Input[]): Promise<{entity_pascal}[]> {{
    const client = get{entity_pascal}ApiClient();
    const results = await Promise.all(
      inputs.map((input) => client.create(input))
    );
    return results;
  }}

  /**
   * Update an existing {entity_pascal}
   */
  async update(id: string, input: Update{entity_pascal}Input): Promise<{entity_pascal}> {{
    const client = get{entity_pascal}ApiClient();
    return client.update(id, input);
  }}

  /**
   * Partially update an existing {entity_pascal}
   */
  async patch(id: string, input: Partial<Update{entity_pascal}Input>): Promise<{entity_pascal}> {{
    const client = get{entity_pascal}ApiClient();
    return client.patch(id, input);
  }}

  /**
   * Delete a {entity_pascal}
   */
  async delete(id: string): Promise<void> {{
    const client = get{entity_pascal}ApiClient();
    return client.delete(id);
  }}

  /**
   * Delete multiple {entity_pascal} entities
   */
  async deleteMany(ids: string[]): Promise<void> {{
    const client = get{entity_pascal}ApiClient();
    await Promise.all(ids.map((id) => client.delete(id)));
  }}

  /**
   * Check if a {entity_pascal} exists
   */
  async exists(id: string): Promise<boolean> {{
    const client = get{entity_pascal}ApiClient();
    return client.exists(id);
  }}

  /**
   * Count {entity_pascal} entities
   */
  async count(filters?: {entity_pascal}FilterParams): Promise<number> {{
    const client = get{entity_pascal}ApiClient();
    return client.count(filters);
  }}

  /**
   * Save a {entity_pascal} (create if new, update if existing)
   */
  async save({entity_camel}: {entity_pascal}): Promise<{entity_pascal}> {{
    const exists = await this.exists({entity_camel}.id);
    if (exists) {{
      return this.update({entity_camel}.id, {entity_camel});
    }}
    return this.create({entity_camel} as Create{entity_pascal}Input);
  }}
}}

// ============================================================================
// Factory Function
// ============================================================================

/**
 * Get {entity_pascal} repository instance
 */
export function get{entity_pascal}Repository(): {entity_pascal}Repository {{
  return {entity_pascal}RepositoryImpl.getInstance();
}}

/**
 * Create {entity_pascal} repository instance
 */
export function create{entity_pascal}Repository(): {entity_pascal}Repository {{
  return {entity_pascal}RepositoryImpl.getInstance();
}}

// <<< CUSTOM: Add custom repository methods here
// END CUSTOM
"#,
            entity_pascal = entity_pascal,
            entity_camel = entity_camel,
            module = self.config.module,
        )
    }
}
