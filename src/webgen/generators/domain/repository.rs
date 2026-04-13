//! Repository interface generator for TypeScript domain layer
//!
//! Generates repository interface types that mirror backend repositories.

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
        let repo_dir = self.config.output_dir
            .join("domain")
            .join(&self.config.module)
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

    /// Generate repository interface content
    fn generate_repository_content(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);

        format!(
r#"/**
 * {entity_pascal} Repository Interface
 *
 * Defines the contract for {entity_pascal} data access.
 * Implementations can use REST API, GraphQL, or other backends.
 *
 * @module {module}/repository/{entity_pascal}Repository
 */

import type {{
  {entity_pascal},
  Create{entity_pascal}Input,
  Update{entity_pascal}Input,
  {entity_pascal}QueryParams,
  {entity_pascal}FilterParams,
}} from '../entity/{entity_pascal}.schema';

// ============================================================================
// Response Types
// ============================================================================

/**
 * Paginated response for list queries
 */
export interface Paginated{entity_pascal}Response {{
  data: {entity_pascal}[];
  total: number;
  page: number;
  limit: number;
  totalPages: number;
  hasNext: boolean;
  hasPrev: boolean;
}}

/**
 * Single entity response
 */
export interface {entity_pascal}Response {{
  data: {entity_pascal};
}}

/**
 * Delete response
 */
export interface Delete{entity_pascal}Response {{
  success: boolean;
  id: string;
}}

/**
 * Batch operation response
 */
export interface Batch{entity_pascal}Response {{
  success: boolean;
  count: number;
  ids: string[];
  errors?: Array<{{ id: string; error: string }}>;
}}

// ============================================================================
// Repository Interface
// ============================================================================

/**
 * {entity_pascal} Repository Interface
 *
 * This interface defines all data access operations for {entity_pascal}.
 * Implement this interface to create a concrete repository.
 */
export interface {entity_pascal}Repository {{
  /**
   * Find a {entity_pascal} by ID
   */
  findById(id: string): Promise<{entity_pascal} | null>;

  /**
   * Find a {entity_pascal} by ID or throw if not found
   */
  findByIdOrThrow(id: string): Promise<{entity_pascal}>;

  /**
   * Get all {entity_pascal} entities with pagination
   */
  findAll(
    params?: {entity_pascal}QueryParams,
    filters?: {entity_pascal}FilterParams
  ): Promise<Paginated{entity_pascal}Response>;

  /**
   * Create a new {entity_pascal}
   */
  create(input: Create{entity_pascal}Input): Promise<{entity_pascal}>;

  /**
   * Update an existing {entity_pascal}
   */
  update(id: string, input: Update{entity_pascal}Input): Promise<{entity_pascal}>;

  /**
   * Delete a {entity_pascal} by ID
   */
  delete(id: string): Promise<Delete{entity_pascal}Response>;

  /**
   * Check if a {entity_pascal} exists by ID
   */
  exists(id: string): Promise<boolean>;

  /**
   * Count {entity_pascal} entities
   */
  count(filters?: {entity_pascal}FilterParams): Promise<number>;

  /**
   * Batch create multiple {entity_pascal} entities
   */
  createMany(inputs: Create{entity_pascal}Input[]): Promise<Batch{entity_pascal}Response>;

  /**
   * Batch delete multiple {entity_pascal} entities
   */
  deleteMany(ids: string[]): Promise<Batch{entity_pascal}Response>;
}}

// ============================================================================
// Repository Factory
// ============================================================================

/**
 * Repository creation options
 */
export interface {entity_pascal}RepositoryOptions {{
  baseUrl: string;
  headers?: Record<string, string>;
  timeout?: number;
}}

/**
 * Create a {entity_pascal} repository instance
 *
 * This factory function creates a repository with the specified options.
 * The actual implementation depends on the injected fetcher.
 */
export function create{entity_pascal}Repository(
  options: {entity_pascal}RepositoryOptions
): {entity_pascal}Repository {{
  // This would typically be implemented by an API client
  throw new Error('Repository implementation required. Use create{entity_pascal}ApiRepository instead.');
}}

// <<< CUSTOM: Add custom repository methods here
// END CUSTOM
"#,
            entity_pascal = entity_pascal,
            module = self.config.module,
        )
    }
}
