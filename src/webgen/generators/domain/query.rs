//! CQRS Query generator for TypeScript domain layer
//!
//! Generates query types for Get and List operations.

use std::fs;

use crate::webgen::ast::entity::EntityDefinition;
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::to_pascal_case;
use super::DomainGenerationResult;

/// Generator for CQRS query types
pub struct QueryGenerator {
    config: Config,
}

impl QueryGenerator {
    /// Create a new query generator
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Generate query types for an entity
    pub fn generate(&self, entity: &EntityDefinition) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let queries_dir = self.config.output_dir
            .join("domain")
            .join(&self.config.module)
            .join("usecase")
            .join("queries");

        if !self.config.dry_run {
            fs::create_dir_all(&queries_dir).ok();
        }

        // Generate individual query files
        let get_query = self.generate_get_query(entity);
        let get_path = queries_dir.join(format!("Get{}Query.ts", entity_pascal));
        result.add_file(get_path.clone(), self.config.dry_run);
        if !self.config.dry_run {
            fs::write(&get_path, get_query).ok();
        }

        let list_query = self.generate_list_query(entity);
        let list_path = queries_dir.join(format!("List{}Query.ts", entity_pascal));
        result.add_file(list_path.clone(), self.config.dry_run);
        if !self.config.dry_run {
            fs::write(&list_path, list_query).ok();
        }

        // Generate queries index
        let index_content = self.generate_queries_index(entity);
        let index_path = queries_dir.join("index.ts");
        result.add_file(index_path.clone(), self.config.dry_run);
        if !self.config.dry_run {
            fs::write(&index_path, index_content).ok();
        }

        Ok(result)
    }

    /// Generate Get query
    fn generate_get_query(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);

        format!(
r#"/**
 * Get{entity_pascal}Query
 *
 * CQRS query for retrieving a single {entity_pascal} by ID.
 *
 * @module {module}/usecase/queries/Get{entity_pascal}Query
 */

import {{ z }} from 'zod';
import type {{ {entity_pascal} }} from '../../entity/{entity_pascal}.schema';

// ============================================================================
// Query Types
// ============================================================================

/**
 * Query type identifier
 */
export const GET_{entity_upper}_QUERY = 'Get{entity_pascal}' as const;

/**
 * Get{entity_pascal}Query schema
 */
export const get{entity_pascal}QuerySchema = z.object({{
  type: z.literal(GET_{entity_upper}_QUERY),
  params: z.object({{
    id: z.string().uuid(),
    include: z.array(z.string()).optional(), // Related entities to include
  }}),
}});

/**
 * Get{entity_pascal}Query type
 */
export interface Get{entity_pascal}Query {{
  type: typeof GET_{entity_upper}_QUERY;
  params: {{
    id: string;
    include?: string[];
  }};
}}

/**
 * Query result type
 */
export interface Get{entity_pascal}QueryResult {{
  data: {entity_pascal} | null;
  found: boolean;
}}

// ============================================================================
// Query Factory
// ============================================================================

/**
 * Create a Get{entity_pascal}Query
 */
export function get{entity_pascal}Query(
  id: string,
  options?: {{ include?: string[] }}
): Get{entity_pascal}Query {{
  return {{
    type: GET_{entity_upper}_QUERY,
    params: {{
      id,
      include: options?.include,
    }},
  }};
}}

/**
 * Validate a Get{entity_pascal}Query
 */
export function validateGet{entity_pascal}Query(query: unknown): Get{entity_pascal}Query {{
  return get{entity_pascal}QuerySchema.parse(query);
}}

/**
 * Check if a value is a Get{entity_pascal}Query
 */
export function isGet{entity_pascal}Query(value: unknown): value is Get{entity_pascal}Query {{
  return (
    typeof value === 'object' &&
    value !== null &&
    (value as Get{entity_pascal}Query).type === GET_{entity_upper}_QUERY
  );
}}
"#,
            entity_pascal = entity_pascal,
            entity_upper = entity.name.to_uppercase(),
            module = self.config.module,
        )
    }

    /// Generate List query
    fn generate_list_query(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);

        format!(
r#"/**
 * List{entity_pascal}Query
 *
 * CQRS query for listing {entity_pascal} entities with pagination and filtering.
 *
 * @module {module}/usecase/queries/List{entity_pascal}Query
 */

import {{ z }} from 'zod';
import type {{
  {entity_pascal},
  {entity_pascal}QueryParams,
  {entity_pascal}FilterParams,
}} from '../../entity/{entity_pascal}.schema';

// ============================================================================
// Query Types
// ============================================================================

/**
 * Query type identifier
 */
export const LIST_{entity_upper}_QUERY = 'List{entity_pascal}' as const;

/**
 * Sort order options
 */
export type SortOrder = 'asc' | 'desc';

/**
 * List{entity_pascal}Query schema
 */
export const list{entity_pascal}QuerySchema = z.object({{
  type: z.literal(LIST_{entity_upper}_QUERY),
  params: z.object({{
    page: z.number().int().positive().default(1),
    limit: z.number().int().positive().max(100).default(20),
    sortBy: z.string().optional(),
    sortOrder: z.enum(['asc', 'desc']).default('desc'),
    search: z.string().optional(),
    filters: z.record(z.unknown()).optional(),
    include: z.array(z.string()).optional(),
  }}).default({{}}),
}});

/**
 * List{entity_pascal}Query type
 */
export interface List{entity_pascal}Query {{
  type: typeof LIST_{entity_upper}_QUERY;
  params: {{
    page?: number;
    limit?: number;
    sortBy?: string;
    sortOrder?: SortOrder;
    search?: string;
    filters?: {entity_pascal}FilterParams;
    include?: string[];
  }};
}}

/**
 * Query result type
 */
export interface List{entity_pascal}QueryResult {{
  data: {entity_pascal}[];
  total: number;
  page: number;
  limit: number;
  totalPages: number;
  hasNext: boolean;
  hasPrev: boolean;
}}

// ============================================================================
// Query Factory
// ============================================================================

/**
 * Create a List{entity_pascal}Query
 */
export function list{entity_pascal}Query(
  params?: List{entity_pascal}Query['params']
): List{entity_pascal}Query {{
  return {{
    type: LIST_{entity_upper}_QUERY,
    params: {{
      page: 1,
      limit: 20,
      sortOrder: 'desc',
      ...params,
    }},
  }};
}}

/**
 * Validate a List{entity_pascal}Query
 */
export function validateList{entity_pascal}Query(query: unknown): List{entity_pascal}Query {{
  return list{entity_pascal}QuerySchema.parse(query);
}}

/**
 * Check if a value is a List{entity_pascal}Query
 */
export function isList{entity_pascal}Query(value: unknown): value is List{entity_pascal}Query {{
  return (
    typeof value === 'object' &&
    value !== null &&
    (value as List{entity_pascal}Query).type === LIST_{entity_upper}_QUERY
  );
}}

/**
 * Create an empty paginated result
 */
export function emptyList{entity_pascal}Result(): List{entity_pascal}QueryResult {{
  return {{
    data: [],
    total: 0,
    page: 1,
    limit: 20,
    totalPages: 0,
    hasNext: false,
    hasPrev: false,
  }};
}}
"#,
            entity_pascal = entity_pascal,
            entity_upper = entity.name.to_uppercase(),
            module = self.config.module,
        )
    }

    /// Generate queries index file
    fn generate_queries_index(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);

        format!(
r#"// Queries exports for {entity_pascal}
// Generated by metaphor-webgen - Do not edit manually

export * from './Get{entity_pascal}Query';
export * from './List{entity_pascal}Query';

// <<< CUSTOM: Add custom query exports here
// END CUSTOM
"#,
            entity_pascal = entity_pascal,
        )
    }
}
