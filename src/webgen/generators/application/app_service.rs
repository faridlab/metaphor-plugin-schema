//! Application service generator
//!
//! Generates application services that coordinate between presentation and domain layers.

use std::fs;

use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition};
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::{to_pascal_case, to_camel_case};
use crate::webgen::generators::domain::DomainGenerationResult;

/// Generator for application services
pub struct AppServiceGenerator {
    config: Config,
}

impl AppServiceGenerator {
    /// Create a new application service generator
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Generate application service for an entity
    pub fn generate(
        &self,
        entity: &EntityDefinition,
        _enums: &[EnumDefinition],
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let services_dir = self.config.output_dir
            .join("application")
            .join(&self.config.module)
            .join("services");

        if !self.config.dry_run {
            fs::create_dir_all(&services_dir).ok();
        }

        // Generate application service file
        let content = self.generate_app_service_content(entity);
        let file_path = services_dir.join(format!("{}AppService.ts", entity_pascal));

        result.add_file(file_path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&file_path, content).ok();
        }

        Ok(result)
    }

    /// Generate application service content
    fn generate_app_service_content(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_camel = to_camel_case(&entity.name);

        format!(
r#"/**
 * {entity_pascal} Application Service
 *
 * Coordinates between presentation layer and domain layer.
 * Handles DTOs, validation, and cross-cutting concerns.
 *
 * @module application/{module}/services/{entity_pascal}AppService
 */

import type {{
  {entity_pascal},
  Create{entity_pascal}Input,
  Update{entity_pascal}Input,
  {entity_pascal}QueryParams,
  {entity_pascal}FilterParams,
}} from '@webapp/domain/{module}/entity/{entity_pascal}.schema';
import {{
  create{entity_pascal}Schema,
  update{entity_pascal}Schema,
}} from '@webapp/domain/{module}/entity/{entity_pascal}.schema';
import {{
  get{entity_pascal}Service,
}} from '@webapp/domain/{module}/service/{entity_pascal}Service';

// ============================================================================
// Types
// ============================================================================

/**
 * DTO for {entity_pascal} list response
 */
export interface {entity_pascal}ListDTO {{
  items: {entity_pascal}DTO[];
  total: number;
  page: number;
  limit: number;
  hasNext: boolean;
  hasPrev: boolean;
}}

/**
 * DTO for single {entity_pascal}
 */
export interface {entity_pascal}DTO extends {entity_pascal} {{
  // Add computed fields here
}}

/**
 * Service error
 */
export class {entity_pascal}AppServiceError extends Error {{
  constructor(
    message: string,
    public code: string,
    public details?: Record<string, unknown>
  ) {{
    super(message);
    this.name = '{entity_pascal}AppServiceError';
  }}
}}

// ============================================================================
// Application Service Class
// ============================================================================

/**
 * {entity_pascal} Application Service
 */
export class {entity_pascal}AppService {{
  private static instance: {entity_pascal}AppService;

  private constructor() {{}}

  /**
   * Get singleton instance
   */
  static getInstance(): {entity_pascal}AppService {{
    if (!{entity_pascal}AppService.instance) {{
      {entity_pascal}AppService.instance = new {entity_pascal}AppService();
    }}
    return {entity_pascal}AppService.instance;
  }}

  /**
   * Create a new {entity_pascal}
   */
  async create(input: Create{entity_pascal}Input): Promise<{entity_pascal}DTO> {{
    // Validate input
    const validationResult = create{entity_pascal}Schema.safeParse(input);
    if (!validationResult.success) {{
      throw new {entity_pascal}AppServiceError(
        'Validation failed',
        'VALIDATION_ERROR',
        {{ errors: validationResult.error.errors }}
      );
    }}

    const service = get{entity_pascal}Service();
    const {entity_camel} = await service.create(validationResult.data);

    return this.toDTO({entity_camel});
  }}

  /**
   * Update an existing {entity_pascal}
   */
  async update(id: string, input: Update{entity_pascal}Input): Promise<{entity_pascal}DTO> {{
    // Validate input
    const validationResult = update{entity_pascal}Schema.safeParse({{ ...input, id }});
    if (!validationResult.success) {{
      throw new {entity_pascal}AppServiceError(
        'Validation failed',
        'VALIDATION_ERROR',
        {{ errors: validationResult.error.errors }}
      );
    }}

    const service = get{entity_pascal}Service();

    // Check if exists
    const exists = await service.exists(id);
    if (!exists) {{
      throw new {entity_pascal}AppServiceError(
        `{entity_pascal} with ID ${{id}} not found`,
        'NOT_FOUND'
      );
    }}

    const {entity_camel} = await service.update(id, validationResult.data);
    return this.toDTO({entity_camel});
  }}

  /**
   * Delete a {entity_pascal}
   */
  async delete(id: string): Promise<void> {{
    const service = get{entity_pascal}Service();

    // Check if exists
    const exists = await service.exists(id);
    if (!exists) {{
      throw new {entity_pascal}AppServiceError(
        `{entity_pascal} with ID ${{id}} not found`,
        'NOT_FOUND'
      );
    }}

    await service.delete(id);
  }}

  /**
   * Get a {entity_pascal} by ID
   */
  async getById(id: string): Promise<{entity_pascal}DTO | null> {{
    const service = get{entity_pascal}Service();
    const {entity_camel} = await service.getById(id);

    if (!{entity_camel}) {{
      return null;
    }}

    return this.toDTO({entity_camel});
  }}

  /**
   * Get paginated list of {entity_pascal}
   */
  async list(
    params?: {entity_pascal}QueryParams,
    filters?: {entity_pascal}FilterParams
  ): Promise<{entity_pascal}ListDTO> {{
    const service = get{entity_pascal}Service();
    const result = await service.getAll(params, filters);

    return {{
      items: result.items.map((item) => this.toDTO(item)),
      total: result.total,
      page: result.page,
      limit: result.limit,
      hasNext: result.hasNext,
      hasPrev: result.hasPrev,
    }};
  }}

  /**
   * Check if {entity_pascal} exists
   */
  async exists(id: string): Promise<boolean> {{
    const service = get{entity_pascal}Service();
    return service.exists(id);
  }}

  /**
   * Count {entity_pascal} entities
   */
  async count(filters?: {entity_pascal}FilterParams): Promise<number> {{
    const service = get{entity_pascal}Service();
    return service.count(filters);
  }}

  /**
   * Convert entity to DTO
   */
  private toDTO({entity_camel}: {entity_pascal}): {entity_pascal}DTO {{
    return {{
      ...{entity_camel},
      // Add computed fields here
    }};
  }}
}}

// ============================================================================
// Singleton Export
// ============================================================================

/**
 * Get {entity_pascal} application service instance
 */
export function get{entity_pascal}AppService(): {entity_pascal}AppService {{
  return {entity_pascal}AppService.getInstance();
}}

// <<< CUSTOM: Add custom application service methods here
// END CUSTOM
"#,
            entity_pascal = entity_pascal,
            entity_camel = entity_camel,
            module = self.config.module,
        )
    }
}
