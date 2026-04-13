//! Specification generator for TypeScript domain layer
//!
//! Generates business rule predicates and validation specifications.

use std::fs;

use crate::webgen::ast::entity::EntityDefinition;
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::{to_pascal_case, to_camel_case};
use super::DomainGenerationResult;

/// Generator for specification patterns (business rules as predicates)
pub struct SpecificationGenerator {
    config: Config,
}

impl SpecificationGenerator {
    /// Create a new specification generator
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Generate specifications for an entity
    pub fn generate(&self, entity: &EntityDefinition) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let specs_dir = self.config.output_dir
            .join("domain")
            .join(&self.config.module)
            .join("specification");

        if !self.config.dry_run {
            fs::create_dir_all(&specs_dir).ok();
        }

        let content = self.generate_specification_content(entity);
        let path = specs_dir.join(format!("{}Specifications.ts", entity_pascal));

        result.add_file(path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&path, content).ok();
        }

        Ok(result)
    }

    /// Generate specification content
    fn generate_specification_content(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_camel = to_camel_case(&entity.name);

        // Detect if entity has common fields
        let has_status = entity.fields.iter().any(|f| f.name == "status");
        let has_deleted_at = entity.fields.iter().any(|f| f.name == "deleted_at" || f.name == "deletedAt");
        let has_created_at = entity.fields.iter().any(|f| f.name == "created_at" || f.name == "createdAt");

        let mut status_specs = String::new();
        if has_status {
            status_specs = format!(r#"
/**
 * Specification: {entity_pascal} is active
 */
export function isActive({entity_camel}: {entity_pascal}): boolean {{
  return {entity_camel}.status === 'active';
}}

/**
 * Specification: {entity_pascal} is inactive
 */
export function isInactive({entity_camel}: {entity_pascal}): boolean {{
  return {entity_camel}.status === 'inactive';
}}

/**
 * Specification: {entity_pascal} is pending
 */
export function isPending({entity_camel}: {entity_pascal}): boolean {{
  return {entity_camel}.status === 'pending';
}}
"#,
                entity_pascal = entity_pascal,
                entity_camel = entity_camel,
            );
        }

        let mut soft_delete_specs = String::new();
        if has_deleted_at {
            soft_delete_specs = format!(r#"
/**
 * Specification: {entity_pascal} is soft deleted
 */
export function isDeleted({entity_camel}: {entity_pascal}): boolean {{
  return {entity_camel}.deletedAt !== null && {entity_camel}.deletedAt !== undefined;
}}

/**
 * Specification: {entity_pascal} is not deleted
 */
export function isNotDeleted({entity_camel}: {entity_pascal}): boolean {{
  return {entity_camel}.deletedAt === null || {entity_camel}.deletedAt === undefined;
}}
"#,
                entity_pascal = entity_pascal,
                entity_camel = entity_camel,
            );
        }

        let mut date_specs = String::new();
        if has_created_at {
            date_specs = format!(r#"
/**
 * Specification: {entity_pascal} was created within the last N days
 */
export function wasCreatedWithinDays({entity_camel}: {entity_pascal}, days: number): boolean {{
  const cutoff = new Date();
  cutoff.setDate(cutoff.getDate() - days);
  const createdAt = typeof {entity_camel}.createdAt === 'string'
    ? new Date({entity_camel}.createdAt)
    : {entity_camel}.createdAt;
  return createdAt >= cutoff;
}}

/**
 * Specification: {entity_pascal} was created today
 */
export function wasCreatedToday({entity_camel}: {entity_pascal}): boolean {{
  return wasCreatedWithinDays({entity_camel}, 1);
}}

/**
 * Specification: {entity_pascal} was created this week
 */
export function wasCreatedThisWeek({entity_camel}: {entity_pascal}): boolean {{
  return wasCreatedWithinDays({entity_camel}, 7);
}}

/**
 * Specification: {entity_pascal} was created this month
 */
export function wasCreatedThisMonth({entity_camel}: {entity_pascal}): boolean {{
  return wasCreatedWithinDays({entity_camel}, 30);
}}
"#,
                entity_pascal = entity_pascal,
                entity_camel = entity_camel,
            );
        }

        format!(
r#"/**
 * {entity_pascal} Specifications
 *
 * Business rules expressed as composable predicates.
 * Specifications can be combined using and(), or(), not() for complex rules.
 *
 * @module {module}/specification/{entity_pascal}Specifications
 */

import type {{ {entity_pascal} }} from '../entity/{entity_pascal}.schema';

// ============================================================================
// Specification Types
// ============================================================================

/**
 * Specification function type
 */
export type Specification<T> = (entity: T) => boolean;

/**
 * Async specification function type
 */
export type AsyncSpecification<T> = (entity: T) => Promise<boolean>;

/**
 * Specification result with explanation
 */
export interface SpecificationResult {{
  satisfied: boolean;
  reason?: string;
  violations?: string[];
}}

/**
 * Detailed specification function type
 */
export type DetailedSpecification<T> = (entity: T) => SpecificationResult;

// ============================================================================
// Specification Combinators
// ============================================================================

/**
 * Combine specifications with AND logic
 */
export function and<T>(
  ...specs: Specification<T>[]
): Specification<T> {{
  return (entity: T) => specs.every((spec) => spec(entity));
}}

/**
 * Combine specifications with OR logic
 */
export function or<T>(
  ...specs: Specification<T>[]
): Specification<T> {{
  return (entity: T) => specs.some((spec) => spec(entity));
}}

/**
 * Negate a specification
 */
export function not<T>(spec: Specification<T>): Specification<T> {{
  return (entity: T) => !spec(entity);
}}

/**
 * Create a specification that always passes
 */
export function always<T>(): Specification<T> {{
  return () => true;
}}

/**
 * Create a specification that always fails
 */
export function never<T>(): Specification<T> {{
  return () => false;
}}

// ============================================================================
// {entity_pascal} Specifications
// ============================================================================

/**
 * Specification: {entity_pascal} has valid ID
 */
export function hasValidId({entity_camel}: {entity_pascal}): boolean {{
  return (
    typeof {entity_camel}.id === 'string' &&
    {entity_camel}.id.length > 0 &&
    /^[0-9a-f]{{8}}-[0-9a-f]{{4}}-[0-9a-f]{{4}}-[0-9a-f]{{4}}-[0-9a-f]{{12}}$/i.test({entity_camel}.id)
  );
}}

/**
 * Specification: {entity_pascal} is new (not persisted)
 */
export function isNew({entity_camel}: {entity_pascal}): boolean {{
  return !hasValidId({entity_camel});
}}

/**
 * Specification: {entity_pascal} is persisted
 */
export function isPersisted({entity_camel}: {entity_pascal}): boolean {{
  return hasValidId({entity_camel});
}}
{status_specs}{soft_delete_specs}{date_specs}
// ============================================================================
// Composite Specifications
// ============================================================================

/**
 * Specification: {entity_pascal} is editable
 */
export function isEditable({entity_camel}: {entity_pascal}): boolean {{
  const specs: Specification<{entity_pascal}>[] = [isPersisted];
  {status_editable}
  {delete_editable}
  return and(...specs)({entity_camel});
}}

/**
 * Specification: {entity_pascal} is deletable
 */
export function isDeletable({entity_camel}: {entity_pascal}): boolean {{
  const specs: Specification<{entity_pascal}>[] = [isPersisted];
  {delete_deletable}
  return and(...specs)({entity_camel});
}}

// ============================================================================
// Detailed Specifications (with explanations)
// ============================================================================

/**
 * Detailed validation with explanations
 */
export function validate{entity_pascal}(
  {entity_camel}: {entity_pascal}
): SpecificationResult {{
  const violations: string[] = [];

  if (!hasValidId({entity_camel})) {{
    violations.push('Invalid or missing ID');
  }}

  // <<< CUSTOM: Add custom validation rules here
  // Example:
  // if (!someCondition) {{
  //   violations.push('Custom validation message');
  // }}
  // END CUSTOM

  return {{
    satisfied: violations.length === 0,
    reason: violations.length > 0 ? 'Validation failed' : undefined,
    violations: violations.length > 0 ? violations : undefined,
  }};
}}

/**
 * Check if {entity_pascal} can transition to a new status
 */
export function canTransitionTo(
  {entity_camel}: {entity_pascal},
  _newStatus: string
): SpecificationResult {{
  // <<< CUSTOM: Define allowed status transitions
  // const allowedTransitions: Record<string, string[]> = {{
  //   'draft': ['pending', 'cancelled'],
  //   'pending': ['active', 'rejected'],
  //   'active': ['inactive', 'suspended'],
  //   'inactive': ['active', 'archived'],
  // }};
  //
  // const currentStatus = {entity_camel}.status;
  // const allowed = allowedTransitions[currentStatus] ?? [];
  //
  // if (!allowed.includes(newStatus)) {{
  //   return {{
  //     satisfied: false,
  //     reason: `Cannot transition from ${{currentStatus}} to ${{newStatus}}`,
  //     violations: [`Invalid status transition`],
  //   }};
  // }}
  // END CUSTOM

  return {{
    satisfied: true,
  }};
}}

// ============================================================================
// Filter Helpers
// ============================================================================

/**
 * Filter array by specification
 */
export function filterBy<T>(
  items: T[],
  spec: Specification<T>
): T[] {{
  return items.filter(spec);
}}

/**
 * Find first item matching specification
 */
export function findBy<T>(
  items: T[],
  spec: Specification<T>
): T | undefined {{
  return items.find(spec);
}}

/**
 * Check if any item matches specification
 */
export function anyMatch<T>(
  items: T[],
  spec: Specification<T>
): boolean {{
  return items.some(spec);
}}

/**
 * Check if all items match specification
 */
export function allMatch<T>(
  items: T[],
  spec: Specification<T>
): boolean {{
  return items.every(spec);
}}

/**
 * Count items matching specification
 */
export function countMatching<T>(
  items: T[],
  spec: Specification<T>
): number {{
  return items.filter(spec).length;
}}

// ============================================================================
// Specification Builders
// ============================================================================

/**
 * Create a field equality specification
 */
export function hasFieldValue<T, K extends keyof T>(
  field: K,
  value: T[K]
): Specification<T> {{
  return (entity: T) => entity[field] === value;
}}

/**
 * Create a field inclusion specification
 */
export function hasFieldIn<T, K extends keyof T>(
  field: K,
  values: T[K][]
): Specification<T> {{
  return (entity: T) => values.includes(entity[field]);
}}

/**
 * Create a field range specification (for numbers/dates)
 */
export function hasFieldInRange<T, K extends keyof T>(
  field: K,
  min: T[K],
  max: T[K]
): Specification<T> {{
  return (entity: T) => entity[field] >= min && entity[field] <= max;
}}

// <<< CUSTOM: Add custom specifications here
// END CUSTOM
"#,
            entity_pascal = entity_pascal,
            entity_camel = entity_camel,
            module = self.config.module,
            status_specs = status_specs,
            soft_delete_specs = soft_delete_specs,
            date_specs = date_specs,
            status_editable = if has_status { "specs.push(isActive);" } else { "" },
            delete_editable = if has_deleted_at { "specs.push(isNotDeleted);" } else { "" },
            delete_deletable = if has_deleted_at { "specs.push(isNotDeleted);" } else { "" },
        )
    }
}
