//! Use case generator
//!
//! Generates use case implementations that orchestrate domain services.

use std::fs;

use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition};
use crate::webgen::ast::HookSchema;
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::{to_pascal_case, to_camel_case, to_snake_case};
use crate::webgen::generators::domain::DomainGenerationResult;

/// Generator for use case implementations
pub struct UseCaseGenerator {
    config: Config,
}

impl UseCaseGenerator {
    /// Create a new use case generator
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Generate use cases for an entity
    pub fn generate(
        &self,
        entity: &EntityDefinition,
        _enums: &[EnumDefinition],
        _hooks: Option<&HookSchema>,
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let usecases_dir = self.config.output_dir
            .join("application")
            .join(&self.config.module)
            .join("usecases");

        if !self.config.dry_run {
            fs::create_dir_all(&usecases_dir).ok();
        }

        // Generate use cases file
        let content = self.generate_usecases_content(entity);
        let file_path = usecases_dir.join(format!("{}UseCases.ts", entity_pascal));

        result.add_file(file_path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&file_path, content).ok();
        }

        Ok(result)
    }

    /// Generate use cases content
    fn generate_usecases_content(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_camel = to_camel_case(&entity.name);
        let entity_snake = to_snake_case(&entity.name);
        let entity_upper = entity_snake.to_uppercase();
        let has_soft_delete = entity.has_soft_delete();

        let mut content = format!(
r#"/**
 * {entity_pascal} Use Cases
 *
 * Application use cases for {entity_pascal} entity operations.
 * Orchestrates domain services and handles cross-cutting concerns.
 *
 * Follows Clean Architecture principles:
 * - Application layer orchestrates Domain layer
 * - Uses Infrastructure layer for external communication
 * - Returns standardized UseCaseResult for error handling
 *
 * @module application/{module}/usecases/{entity_pascal}UseCases
 */

import type {{
  {entity_pascal},
  Create{entity_pascal}Input,
  Update{entity_pascal}Input,
  Patch{entity_pascal}Input,
  {entity_pascal}QueryParams,
  {entity_pascal}FilterParams,
}} from '@webapp/domain/{module}/entity/{entity_pascal}.schema';
import {{
  get{entity_pascal}Service,
  type {entity_pascal}Service,
}} from '@webapp/domain/{module}/service/{entity_pascal}Service';
import type {{ PaginatedResponse }} from '@webapp/shared/types/pagination';

// ============================================================================
// Types
// ============================================================================

export interface {entity_pascal}UseCaseContext {{
  userId?: string;
  correlationId?: string;
}}

export interface {entity_pascal}UseCaseResult<T> {{
  success: boolean;
  data?: T;
  error?: {{
    code: string;
    message: string;
  }};
}}

// ============================================================================
// Create Use Case
// ============================================================================

/**
 * Create {entity_pascal} use case
 *
 * Application layer orchestrator that:
 * 1. Validates input (via domain schema)
 * 2. Calls domain service
 * 3. Handles business rules
 * 4. Returns standardized result
 */
export async function create{entity_pascal}UseCase(
  input: Create{entity_pascal}Input,
  context?: {entity_pascal}UseCaseContext
): Promise<{entity_pascal}UseCaseResult<{entity_pascal}>> {{
  try {{
    const service = get{entity_pascal}Service();

    // TODO: Add authorization check
    // TODO: Add validation beyond schema validation
    // TODO: Add audit logging

    const {entity_camel} = await service.create(input);

    // TODO: Emit domain event

    return {{
      success: true,
      data: {entity_camel},
    }};
  }} catch (error) {{
    return {{
      success: false,
      error: {{
        code: 'CREATE_{entity_upper}_FAILED',
        message: error instanceof Error ? error.message : 'Unknown error',
      }},
    }};
  }}
}}

// ============================================================================
// Update Use Case
// ============================================================================

/**
 * Update {entity_pascal} use case
 */
export async function update{entity_pascal}UseCase(
  id: string,
  input: Update{entity_pascal}Input,
  context?: {entity_pascal}UseCaseContext
): Promise<{entity_pascal}UseCaseResult<{entity_pascal}>> {{
  try {{
    const service = get{entity_pascal}Service();

    // Verify entity exists
    const existing = await service.getById(id);
    if (!existing) {{
      return {{
        success: false,
        error: {{
          code: '{entity_upper}_NOT_FOUND',
          message: `{entity_pascal} with ID ${{id}} not found`,
        }},
      }};
    }}

    // TODO: Add authorization check
    // TODO: Add validation beyond schema validation
    // TODO: Add audit logging

    const {entity_camel} = await service.update(id, input);

    // TODO: Emit domain event

    return {{
      success: true,
      data: {entity_camel},
    }};
  }} catch (error) {{
    return {{
      success: false,
      error: {{
        code: 'UPDATE_{entity_upper}_FAILED',
        message: error instanceof Error ? error.message : 'Unknown error',
      }},
    }};
  }}
}}

// ============================================================================
// Patch Use Case (Partial Update)
// ============================================================================

/**
 * Patch {entity_pascal} use case (partial update)
 */
export async function patch{entity_pascal}UseCase(
  id: string,
  input: Patch{entity_pascal}Input,
  context?: {entity_pascal}UseCaseContext
): Promise<{entity_pascal}UseCaseResult<{entity_pascal}>> {{
  try {{
    const service = get{entity_pascal}Service();

    // Verify entity exists
    const existing = await service.getById(id);
    if (!existing) {{
      return {{
        success: false,
        error: {{
          code: '{entity_upper}_NOT_FOUND',
          message: `{entity_pascal} with ID ${{id}} not found`,
        }},
      }};
    }}

    // TODO: Add authorization check
    // TODO: Add audit logging

    const {entity_camel} = await service.patch(id, input);

    // TODO: Emit domain event

    return {{
      success: true,
      data: {entity_camel},
    }};
  }} catch (error) {{
    return {{
      success: false,
      error: {{
        code: 'PATCH_{entity_upper}_FAILED',
        message: error instanceof Error ? error.message : 'Unknown error',
      }},
    }};
  }}
}}

// ============================================================================
// Delete Use Case
// ============================================================================

/**
 * Delete {entity_pascal} use case
 */
export async function delete{entity_pascal}UseCase(
  id: string,
  context?: {entity_pascal}UseCaseContext
): Promise<{entity_pascal}UseCaseResult<void>> {{
  try {{
    const service = get{entity_pascal}Service();

    // Verify entity exists
    const existing = await service.getById(id);
    if (!existing) {{
      return {{
        success: false,
        error: {{
          code: '{entity_upper}_NOT_FOUND',
          message: `{entity_pascal} with ID ${{id}} not found`,
        }},
      }};
    }}

    // TODO: Add authorization check
    // TODO: Check for dependencies before deletion
    // TODO: Add audit logging

    await service.delete(id);

    // TODO: Emit domain event

    return {{
      success: true,
    }};
  }} catch (error) {{
    return {{
      success: false,
      error: {{
        code: 'DELETE_{entity_upper}_FAILED',
        message: error instanceof Error ? error.message : 'Unknown error',
      }},
    }};
  }}
}}

// ============================================================================
// Get By ID Use Case
// ============================================================================

/**
 * Get {entity_pascal} by ID use case
 */
export async function get{entity_pascal}ByIdUseCase(
  id: string,
  context?: {entity_pascal}UseCaseContext
): Promise<{entity_pascal}UseCaseResult<{entity_pascal}>> {{
  try {{
    const service = get{entity_pascal}Service();

    const {entity_camel} = await service.getById(id);

    if (!{entity_camel}) {{
      return {{
        success: false,
        error: {{
          code: '{entity_upper}_NOT_FOUND',
          message: `{entity_pascal} with ID ${{id}} not found`,
        }},
      }};
    }}

    // TODO: Add authorization check

    return {{
      success: true,
      data: {entity_camel},
    }};
  }} catch (error) {{
    return {{
      success: false,
      error: {{
        code: 'GET_{entity_upper}_FAILED',
        message: error instanceof Error ? error.message : 'Unknown error',
      }},
    }};
  }}
}}

// ============================================================================
// List Use Case
// ============================================================================

/**
 * List {entity_pascal} use case
 */
export async function list{entity_pascal}UseCase(
  params?: {entity_pascal}QueryParams,
  filters?: {entity_pascal}FilterParams,
  context?: {entity_pascal}UseCaseContext
): Promise<{entity_pascal}UseCaseResult<PaginatedResponse<{entity_pascal}>>> {{
  try {{
    const service = get{entity_pascal}Service();

    // TODO: Add authorization check
    // TODO: Filter based on user permissions

    const result = await service.getAll(params, filters);

    return {{
      success: true,
      data: result as PaginatedResponse<{entity_pascal}>,
    }};
  }} catch (error) {{
    return {{
      success: false,
      error: {{
        code: 'LIST_{entity_upper}_FAILED',
        message: error instanceof Error ? error.message : 'Unknown error',
      }},
    }};
  }}
}}

// <<< CUSTOM: Add custom use cases here
// END CUSTOM
"#,
            entity_pascal = entity_pascal,
            entity_camel = entity_camel,
            entity_upper = entity_upper,
            module = self.config.module,
        );

        // Append soft-delete use cases if entity has soft_delete
        if has_soft_delete {
            content.push_str(&Self::generate_soft_delete_usecases(
                &entity_pascal, &entity_camel, &entity_upper,
            ));
        }

        content
    }

    /// Generate soft-delete use cases content (appended after main content)
    fn generate_soft_delete_usecases(
        entity_pascal: &str,
        entity_camel: &str,
        entity_upper: &str,
    ) -> String {
        format!(
r#"
// ============================================================================
// Soft Delete Use Case
// ============================================================================

/**
 * Soft delete {ep} use case (move to trash)
 */
export async function softDelete{ep}UseCase(
  id: string,
  context?: {ep}UseCaseContext
): Promise<{ep}UseCaseResult<{ep}>> {{
  try {{
    const service = get{ep}Service();

    // Verify entity exists
    const existing = await service.getById(id);
    if (!existing) {{
      return {{
        success: false,
        error: {{
          code: '{eu}_NOT_FOUND',
          message: `{ep} with ID ${{id}} not found`,
        }},
      }};
    }}

    const {ec} = await service.softDelete(id);

    return {{
      success: true,
      data: {ec},
    }};
  }} catch (error) {{
    return {{
      success: false,
      error: {{
        code: 'SOFT_DELETE_{eu}_FAILED',
        message: error instanceof Error ? error.message : 'Unknown error',
      }},
    }};
  }}
}}

// ============================================================================
// Restore Use Case
// ============================================================================

/**
 * Restore {ep} use case (restore from trash)
 */
export async function restore{ep}UseCase(
  id: string,
  context?: {ep}UseCaseContext
): Promise<{ep}UseCaseResult<{ep}>> {{
  try {{
    const service = get{ep}Service();

    const {ec} = await service.restore(id);

    return {{
      success: true,
      data: {ec},
    }};
  }} catch (error) {{
    return {{
      success: false,
      error: {{
        code: 'RESTORE_{eu}_FAILED',
        message: error instanceof Error ? error.message : 'Unknown error',
      }},
    }};
  }}
}}

// ============================================================================
// Permanent Delete Use Case
// ============================================================================

/**
 * Permanently delete {ep} use case (cannot be undone)
 */
export async function permanentDelete{ep}UseCase(
  id: string,
  context?: {ep}UseCaseContext
): Promise<{ep}UseCaseResult<void>> {{
  try {{
    const service = get{ep}Service();

    await service.permanentDelete(id);

    return {{
      success: true,
    }};
  }} catch (error) {{
    return {{
      success: false,
      error: {{
        code: 'PERMANENT_DELETE_{eu}_FAILED',
        message: error instanceof Error ? error.message : 'Unknown error',
      }},
    }};
  }}
}}

// ============================================================================
// List Deleted Use Case
// ============================================================================

/**
 * List deleted {ep} use case (fetch trash items)
 */
export async function listDeleted{ep}UseCase(
  params?: {ep}QueryParams,
  filters?: {ep}FilterParams,
  context?: {ep}UseCaseContext
): Promise<{ep}UseCaseResult<PaginatedResponse<{ep}>>> {{
  try {{
    const service = get{ep}Service();

    const result = await service.getDeleted(params, filters);

    return {{
      success: true,
      data: result as PaginatedResponse<{ep}>,
    }};
  }} catch (error) {{
    return {{
      success: false,
      error: {{
        code: 'LIST_DELETED_{eu}_FAILED',
        message: error instanceof Error ? error.message : 'Unknown error',
      }},
    }};
  }}
}}
"#,
            ep = entity_pascal,
            ec = entity_camel,
            eu = entity_upper,
        )
    }
}
