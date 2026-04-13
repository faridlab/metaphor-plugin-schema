//! API client generator
//!
//! Generates API client implementations for REST/gRPC communication.

use std::fs;

use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition};
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::{to_pascal_case, to_snake_case, pluralize};
use crate::webgen::generators::domain::DomainGenerationResult;

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
        let api_dir = self.config.output_dir
            .join("infrastructure")
            .join(&self.config.module)
            .join("api");

        if !self.config.dry_run {
            fs::create_dir_all(&api_dir).ok();
        }

        // Generate API client file
        let content = self.generate_api_client_content(entity);
        let file_path = api_dir.join(format!("{}ApiClient.ts", entity_pascal));

        result.add_file(file_path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&file_path, content).ok();
        }

        Ok(result)
    }

    /// Generate API client content
    fn generate_api_client_content(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_snake = to_snake_case(&entity.name);
        let entity_route = pluralize(&entity_snake);
        let has_soft_delete = entity.has_soft_delete();

        let mut content = format!(
r#"/**
 * {entity_pascal} API Client
 *
 * HTTP client for {entity_pascal} entity CRUD operations.
 * Implements the {entity_pascal}Service interface.
 *
 * @module infrastructure/{module}/api/{entity_pascal}ApiClient
 */

import type {{
  {entity_pascal},
  Create{entity_pascal}Input,
  Update{entity_pascal}Input,
  {entity_pascal}QueryParams,
  {entity_pascal}FilterParams,
}} from '@webapp/domain/{module}/entity/{entity_pascal}.schema';
import type {{ {entity_pascal}Service }} from '@webapp/domain/{module}/service/{entity_pascal}Service';
import type {{ Paginated{entity_pascal}Response }} from '@webapp/domain/{module}/repository/{entity_pascal}Repository';
import {{ PaginatedApiResponse }} from './utils';

// ============================================================================
// Configuration
// ============================================================================

const API_BASE_URL = import.meta.env.VITE_API_BASE_URL || '/api';
const API_VERSION = 'v1';

/**
 * Build API URL
 */
function buildUrl(path: string): string {{
  return `${{API_BASE_URL}}/${{API_VERSION}}/{module}/{entity_route}${{path}}`;
}}

/**
 * Build query string from params
 */
function buildQueryString(params?: Record<string, unknown>): string {{
  if (!params) return '';

  const searchParams = new URLSearchParams();
  Object.entries(params).forEach(([key, value]) => {{
    if (value !== undefined && value !== null) {{
      searchParams.append(key, String(value));
    }}
  }});

  const query = searchParams.toString();
  return query ? `?${{query}}` : '';
}}

// ============================================================================
// Error Handling
// ============================================================================

export class {entity_pascal}ApiError extends Error {{
  constructor(
    message: string,
    public status: number,
    public code?: string,
    public details?: Record<string, unknown>
  ) {{
    super(message);
    this.name = '{entity_pascal}ApiError';
  }}
}}

async function handleResponse<T>(response: Response): Promise<T> {{
  if (!response.ok) {{
    const errorData = await response.json().catch(() => ({{}}));
    throw new {entity_pascal}ApiError(
      errorData.message || `HTTP error ${{response.status}}`,
      response.status,
      errorData.code,
      errorData.details
    );
  }}
  return response.json();
}}

// ============================================================================
// API Client Implementation
// ============================================================================

/**
 * {entity_pascal} API Client
 *
 * Implements {entity_pascal}Service interface using REST API
 */
export class {entity_pascal}ApiClient implements {entity_pascal}Service {{
  private static instance: {entity_pascal}ApiClient;
  private authToken?: string;

  private constructor() {{}}

  /**
   * Get singleton instance
   */
  static getInstance(): {entity_pascal}ApiClient {{
    if (!{entity_pascal}ApiClient.instance) {{
      {entity_pascal}ApiClient.instance = new {entity_pascal}ApiClient();
    }}
    return {entity_pascal}ApiClient.instance;
  }}

  /**
   * Set authentication token
   */
  setAuthToken(token: string): void {{
    this.authToken = token;
  }}

  /**
   * Get default headers
   */
  private getHeaders(): HeadersInit {{
    const headers: HeadersInit = {{
      'Content-Type': 'application/json',
    }};

    if (this.authToken) {{
      headers['Authorization'] = `Bearer ${{this.authToken}}`;
    }}

    return headers;
  }}

  /**
   * Get {entity_pascal} by ID
   */
  async getById(id: string): Promise<{entity_pascal}> {{
    const response = await fetch(buildUrl(`/${{id}}`), {{
      method: 'GET',
      headers: this.getHeaders(),
    }});

    return handleResponse<{entity_pascal}>(response);
  }}

  /**
   * Get all {entity_pascal} entities with pagination
   */
  async getAll(
    params?: {entity_pascal}QueryParams,
    filters?: {entity_pascal}FilterParams
  ): Promise<Paginated{entity_pascal}Response> {{
    const queryParams = {{ ...params, ...filters }};
    const url = buildUrl(buildQueryString(queryParams as Record<string, unknown>));

    const response = await fetch(url, {{
      method: 'GET',
      headers: this.getHeaders(),
    }});

    // Handle the new flat API response: {{ success: true, data: [...], meta: {{{{...}}}} }}
    const apiResponse = await handleResponse<PaginatedApiResponse<{entity_pascal}>>(response);

    if (!apiResponse.success) {{
      throw new {entity_pascal}ApiError(
        apiResponse.error || 'Failed to fetch {entity_snake} entities',
        500,
        'FETCH_FAILED'
      );
    }}

    const {{ data: entities, meta }} = apiResponse;

    // Transform to Paginated{entity_pascal}Response format
    return {{
      data: entities,
      total: meta.total,
      page: meta.page,
      limit: meta.limit,
      totalPages: meta.total_pages,
      hasNext: meta.page < meta.total_pages,
      hasPrev: meta.page > 1,
    }};
  }}

  /**
   * Create a new {entity_pascal}
   */
  async create(input: Create{entity_pascal}Input): Promise<{entity_pascal}> {{
    const response = await fetch(buildUrl(''), {{
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify(input),
    }});

    return handleResponse<{entity_pascal}>(response);
  }}

  /**
   * Update an existing {entity_pascal}
   */
  async update(id: string, input: Update{entity_pascal}Input): Promise<{entity_pascal}> {{
    const response = await fetch(buildUrl(`/${{id}}`), {{
      method: 'PUT',
      headers: this.getHeaders(),
      body: JSON.stringify(input),
    }});

    return handleResponse<{entity_pascal}>(response);
  }}

  /**
   * Partially update an existing {entity_pascal}
   */
  async patch(id: string, input: Partial<Update{entity_pascal}Input>): Promise<{entity_pascal}> {{
    const response = await fetch(buildUrl(`/${{id}}`), {{
      method: 'PATCH',
      headers: this.getHeaders(),
      body: JSON.stringify(input),
    }});

    return handleResponse<{entity_pascal}>(response);
  }}

  /**
   * Delete a {entity_pascal}{delete_note}
   */
  async delete(id: string): Promise<void> {{
    const response = await fetch(buildUrl(`/${{id}}`), {{
      method: 'DELETE',
      headers: this.getHeaders(),
    }});

    if (!response.ok) {{
      const errorData = await response.json().catch(() => ({{}}));
      throw new {entity_pascal}ApiError(
        errorData.message || `HTTP error ${{response.status}}`,
        response.status,
        errorData.code
      );
    }}
  }}

  /**
   * Check if {entity_pascal} exists
   */
  async exists(id: string): Promise<boolean> {{
    try {{
      const response = await fetch(buildUrl(`/${{id}}`), {{
        method: 'HEAD',
        headers: this.getHeaders(),
      }});
      return response.ok;
    }} catch {{
      return false;
    }}
  }}

  /**
   * Count {entity_pascal} entities
   */
  async count(filters?: {entity_pascal}FilterParams): Promise<number> {{
    const url = buildUrl('/count' + buildQueryString(filters as Record<string, unknown>));

    const response = await fetch(url, {{
      method: 'GET',
      headers: this.getHeaders(),
    }});

    const result = await handleResponse<{{ count: number }}>(response);
    return result.count;
  }}
"#,
            entity_pascal = entity_pascal,
            entity_snake = entity_snake,
            entity_route = entity_route,
            module = self.config.module,
            delete_note = if has_soft_delete { " (soft delete - moves to trash)" } else { "" },
        );

        // Add soft delete methods if enabled
        if has_soft_delete {
            content.push_str(&format!(
r#"
  // ============================================================================
  // Soft Delete / Trash Operations
  // ============================================================================

  /**
   * Get all soft-deleted {entity_pascal} entities from trash
   */
  async getDeleted(
    params?: {entity_pascal}QueryParams,
    filters?: {entity_pascal}FilterParams
  ): Promise<Paginated{entity_pascal}Response> {{
    const queryParams = {{ ...params, ...filters }};
    const url = buildUrl('/trash' + buildQueryString(queryParams as Record<string, unknown>));

    const response = await fetch(url, {{
      method: 'GET',
      headers: this.getHeaders(),
    }});

    const apiResponse = await handleResponse<PaginatedApiResponse<{entity_pascal}>>(response);

    if (!apiResponse.success) {{
      throw new {entity_pascal}ApiError(
        apiResponse.error || 'Failed to fetch deleted {entity_snake} entities',
        500,
        'FETCH_FAILED'
      );
    }}

    const {{ data: entities, meta }} = apiResponse;

    return {{
      data: entities,
      total: meta.total,
      page: meta.page,
      limit: meta.limit,
      totalPages: meta.total_pages,
      hasNext: meta.page < meta.total_pages,
      hasPrev: meta.page > 1,
    }};
  }}

  /**
   * Get a soft-deleted {entity_pascal} by ID from trash
   */
  async getDeletedById(id: string): Promise<{entity_pascal}> {{
    const response = await fetch(buildUrl(`/trash/${{id}}`), {{
      method: 'GET',
      headers: this.getHeaders(),
    }});

    return handleResponse<{entity_pascal}>(response);
  }}

  /**
   * Restore a soft-deleted {entity_pascal} from trash
   */
  async restore(id: string): Promise<{entity_pascal}> {{
    const response = await fetch(buildUrl(`/${{id}}/restore`), {{
      method: 'POST',
      headers: this.getHeaders(),
    }});

    return handleResponse<{entity_pascal}>(response);
  }}

  /**
   * Permanently delete a {entity_pascal} from trash (cannot be restored)
   */
  async permanentDelete(id: string): Promise<void> {{
    const response = await fetch(buildUrl(`/trash/${{id}}`), {{
      method: 'DELETE',
      headers: this.getHeaders(),
    }});

    if (!response.ok) {{
      const errorData = await response.json().catch(() => ({{}}));
      throw new {entity_pascal}ApiError(
        errorData.message || `HTTP error ${{response.status}}`,
        response.status,
        errorData.code
      );
    }}
  }}

  /**
   * Empty trash - permanently delete all soft-deleted {entity_pascal} entities
   */
  async emptyTrash(): Promise<{{ deleted: number }}> {{
    const response = await fetch(buildUrl('/trash'), {{
      method: 'DELETE',
      headers: this.getHeaders(),
    }});

    return handleResponse<{{ deleted: number }}>(response);
  }}

  /**
   * Count soft-deleted {entity_pascal} entities in trash
   */
  async countDeleted(): Promise<number> {{
    const url = buildUrl('/trash/count');

    const response = await fetch(url, {{
      method: 'GET',
      headers: this.getHeaders(),
    }});

    const result = await handleResponse<{{ count: number }}>(response);
    return result.count;
  }}
"#,
                entity_pascal = entity_pascal,
                entity_snake = entity_snake,
            ));
        }

        // Close the class and add factory functions
        content.push_str(&format!(
r#"}}

// ============================================================================
// Factory Function
// ============================================================================

/**
 * Get {entity_pascal} API client instance
 */
export function get{entity_pascal}ApiClient(): {entity_pascal}ApiClient {{
  return {entity_pascal}ApiClient.getInstance();
}}

/**
 * Create and configure {entity_pascal} API client
 */
export function create{entity_pascal}ApiClient(authToken?: string): {entity_pascal}ApiClient {{
  const client = {entity_pascal}ApiClient.getInstance();
  if (authToken) {{
    client.setAuthToken(authToken);
  }}
  return client;
}}

// <<< CUSTOM: Add custom API methods here
// END CUSTOM
"#,
            entity_pascal = entity_pascal,
        ));

        content
    }
}
