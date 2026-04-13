//! CQRS Command generator for TypeScript domain layer
//!
//! Generates command types for Create, Update, and Delete operations.

use std::fs;

use crate::webgen::ast::entity::EntityDefinition;
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::to_pascal_case;
use super::DomainGenerationResult;

/// Generator for CQRS command types
pub struct CommandGenerator {
    config: Config,
}

impl CommandGenerator {
    /// Create a new command generator
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Generate command types for an entity
    pub fn generate(&self, entity: &EntityDefinition) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let commands_dir = self.config.output_dir
            .join("domain")
            .join(&self.config.module)
            .join("usecase")
            .join("commands");

        if !self.config.dry_run {
            fs::create_dir_all(&commands_dir).ok();
        }

        // Generate individual command files
        let create_cmd = self.generate_create_command(entity);
        let create_path = commands_dir.join(format!("Create{}Command.ts", entity_pascal));
        result.add_file(create_path.clone(), self.config.dry_run);
        if !self.config.dry_run {
            fs::write(&create_path, create_cmd).ok();
        }

        let update_cmd = self.generate_update_command(entity);
        let update_path = commands_dir.join(format!("Update{}Command.ts", entity_pascal));
        result.add_file(update_path.clone(), self.config.dry_run);
        if !self.config.dry_run {
            fs::write(&update_path, update_cmd).ok();
        }

        let delete_cmd = self.generate_delete_command(entity);
        let delete_path = commands_dir.join(format!("Delete{}Command.ts", entity_pascal));
        result.add_file(delete_path.clone(), self.config.dry_run);
        if !self.config.dry_run {
            fs::write(&delete_path, delete_cmd).ok();
        }

        // Generate commands index
        let index_content = self.generate_commands_index(entity);
        let index_path = commands_dir.join("index.ts");
        result.add_file(index_path.clone(), self.config.dry_run);
        if !self.config.dry_run {
            fs::write(&index_path, index_content).ok();
        }

        Ok(result)
    }

    /// Generate Create command
    fn generate_create_command(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);

        format!(
r#"/**
 * Create{entity_pascal}Command
 *
 * CQRS command for creating a new {entity_pascal}.
 *
 * @module {module}/usecase/commands/Create{entity_pascal}Command
 */

import {{ z }} from 'zod';
import {{
  create{entity_pascal}Schema,
  type Create{entity_pascal}Input,
  type {entity_pascal},
}} from '../../entity/{entity_pascal}.schema';

// ============================================================================
// Command Types
// ============================================================================

/**
 * Command type identifier
 */
export const CREATE_{entity_upper}_COMMAND = 'Create{entity_pascal}' as const;

/**
 * Create{entity_pascal}Command schema
 */
export const create{entity_pascal}CommandSchema = z.object({{
  type: z.literal(CREATE_{entity_upper}_COMMAND),
  payload: create{entity_pascal}Schema,
  metadata: z.object({{
    timestamp: z.date().default(() => new Date()),
    correlationId: z.string().uuid().optional(),
    userId: z.string().uuid().optional(),
  }}).optional(),
}});

/**
 * Create{entity_pascal}Command type
 */
export interface Create{entity_pascal}Command {{
  type: typeof CREATE_{entity_upper}_COMMAND;
  payload: Create{entity_pascal}Input;
  metadata?: {{
    timestamp: Date;
    correlationId?: string;
    userId?: string;
  }};
}}

/**
 * Command result type
 */
export interface Create{entity_pascal}CommandResult {{
  success: boolean;
  data?: {entity_pascal};
  error?: string;
}}

// ============================================================================
// Command Factory
// ============================================================================

/**
 * Create a Create{entity_pascal}Command
 */
export function create{entity_pascal}Command(
  payload: Create{entity_pascal}Input,
  metadata?: Create{entity_pascal}Command['metadata']
): Create{entity_pascal}Command {{
  return {{
    type: CREATE_{entity_upper}_COMMAND,
    payload,
    metadata: {{
      timestamp: new Date(),
      ...metadata,
    }},
  }};
}}

/**
 * Validate a Create{entity_pascal}Command
 */
export function validateCreate{entity_pascal}Command(
  command: unknown
): Create{entity_pascal}Command {{
  return create{entity_pascal}CommandSchema.parse(command);
}}

/**
 * Check if a value is a Create{entity_pascal}Command
 */
export function isCreate{entity_pascal}Command(
  value: unknown
): value is Create{entity_pascal}Command {{
  return (
    typeof value === 'object' &&
    value !== null &&
    (value as Create{entity_pascal}Command).type === CREATE_{entity_upper}_COMMAND
  );
}}
"#,
            entity_pascal = entity_pascal,
            entity_upper = entity.name.to_uppercase(),
            module = self.config.module,
        )
    }

    /// Generate Update command
    fn generate_update_command(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);

        format!(
r#"/**
 * Update{entity_pascal}Command
 *
 * CQRS command for updating an existing {entity_pascal}.
 *
 * @module {module}/usecase/commands/Update{entity_pascal}Command
 */

import {{ z }} from 'zod';
import {{
  update{entity_pascal}Schema,
  type Update{entity_pascal}Input,
  type {entity_pascal},
}} from '../../entity/{entity_pascal}.schema';

// ============================================================================
// Command Types
// ============================================================================

/**
 * Command type identifier
 */
export const UPDATE_{entity_upper}_COMMAND = 'Update{entity_pascal}' as const;

/**
 * Update{entity_pascal}Command schema
 */
export const update{entity_pascal}CommandSchema = z.object({{
  type: z.literal(UPDATE_{entity_upper}_COMMAND),
  payload: update{entity_pascal}Schema,
  metadata: z.object({{
    timestamp: z.date().default(() => new Date()),
    correlationId: z.string().uuid().optional(),
    userId: z.string().uuid().optional(),
  }}).optional(),
}});

/**
 * Update{entity_pascal}Command type
 */
export interface Update{entity_pascal}Command {{
  type: typeof UPDATE_{entity_upper}_COMMAND;
  payload: Update{entity_pascal}Input;
  metadata?: {{
    timestamp: Date;
    correlationId?: string;
    userId?: string;
  }};
}}

/**
 * Command result type
 */
export interface Update{entity_pascal}CommandResult {{
  success: boolean;
  data?: {entity_pascal};
  error?: string;
}}

// ============================================================================
// Command Factory
// ============================================================================

/**
 * Create an Update{entity_pascal}Command
 */
export function update{entity_pascal}Command(
  payload: Update{entity_pascal}Input,
  metadata?: Update{entity_pascal}Command['metadata']
): Update{entity_pascal}Command {{
  return {{
    type: UPDATE_{entity_upper}_COMMAND,
    payload,
    metadata: {{
      timestamp: new Date(),
      ...metadata,
    }},
  }};
}}

/**
 * Validate an Update{entity_pascal}Command
 */
export function validateUpdate{entity_pascal}Command(
  command: unknown
): Update{entity_pascal}Command {{
  return update{entity_pascal}CommandSchema.parse(command);
}}

/**
 * Check if a value is an Update{entity_pascal}Command
 */
export function isUpdate{entity_pascal}Command(
  value: unknown
): value is Update{entity_pascal}Command {{
  return (
    typeof value === 'object' &&
    value !== null &&
    (value as Update{entity_pascal}Command).type === UPDATE_{entity_upper}_COMMAND
  );
}}
"#,
            entity_pascal = entity_pascal,
            entity_upper = entity.name.to_uppercase(),
            module = self.config.module,
        )
    }

    /// Generate Delete command
    fn generate_delete_command(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);

        format!(
r#"/**
 * Delete{entity_pascal}Command
 *
 * CQRS command for deleting a {entity_pascal}.
 *
 * @module {module}/usecase/commands/Delete{entity_pascal}Command
 */

import {{ z }} from 'zod';

// ============================================================================
// Command Types
// ============================================================================

/**
 * Command type identifier
 */
export const DELETE_{entity_upper}_COMMAND = 'Delete{entity_pascal}' as const;

/**
 * Delete{entity_pascal}Command schema
 */
export const delete{entity_pascal}CommandSchema = z.object({{
  type: z.literal(DELETE_{entity_upper}_COMMAND),
  payload: z.object({{
    id: z.string().uuid(),
    soft: z.boolean().default(true), // Soft delete by default
  }}),
  metadata: z.object({{
    timestamp: z.date().default(() => new Date()),
    correlationId: z.string().uuid().optional(),
    userId: z.string().uuid().optional(),
    reason: z.string().optional(),
  }}).optional(),
}});

/**
 * Delete{entity_pascal}Command type
 */
export interface Delete{entity_pascal}Command {{
  type: typeof DELETE_{entity_upper}_COMMAND;
  payload: {{
    id: string;
    soft?: boolean;
  }};
  metadata?: {{
    timestamp: Date;
    correlationId?: string;
    userId?: string;
    reason?: string;
  }};
}}

/**
 * Command result type
 */
export interface Delete{entity_pascal}CommandResult {{
  success: boolean;
  id: string;
  error?: string;
}}

// ============================================================================
// Command Factory
// ============================================================================

/**
 * Create a Delete{entity_pascal}Command
 */
export function delete{entity_pascal}Command(
  id: string,
  options?: {{ soft?: boolean }},
  metadata?: Delete{entity_pascal}Command['metadata']
): Delete{entity_pascal}Command {{
  return {{
    type: DELETE_{entity_upper}_COMMAND,
    payload: {{
      id,
      soft: options?.soft ?? true,
    }},
    metadata: {{
      timestamp: new Date(),
      ...metadata,
    }},
  }};
}}

/**
 * Validate a Delete{entity_pascal}Command
 */
export function validateDelete{entity_pascal}Command(
  command: unknown
): Delete{entity_pascal}Command {{
  return delete{entity_pascal}CommandSchema.parse(command);
}}

/**
 * Check if a value is a Delete{entity_pascal}Command
 */
export function isDelete{entity_pascal}Command(
  value: unknown
): value is Delete{entity_pascal}Command {{
  return (
    typeof value === 'object' &&
    value !== null &&
    (value as Delete{entity_pascal}Command).type === DELETE_{entity_upper}_COMMAND
  );
}}
"#,
            entity_pascal = entity_pascal,
            entity_upper = entity.name.to_uppercase(),
            module = self.config.module,
        )
    }

    /// Generate commands index file
    fn generate_commands_index(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);

        format!(
r#"// Commands exports for {entity_pascal}
// Generated by metaphor-webgen - Do not edit manually

export * from './Create{entity_pascal}Command';
export * from './Update{entity_pascal}Command';
export * from './Delete{entity_pascal}Command';

// <<< CUSTOM: Add custom command exports here
// END CUSTOM
"#,
            entity_pascal = entity_pascal,
        )
    }
}
