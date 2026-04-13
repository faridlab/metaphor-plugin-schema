//! Table columns generator
//!
//! Generates table column definitions for data tables.
//! Uses Joy UI components from @/components/ui

use std::fs;

use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition, FieldDefinition, FieldType};
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::to_pascal_case;
use crate::webgen::generators::domain::DomainGenerationResult;

/// Generator for table column definitions
pub struct TableColumnsGenerator {
    config: Config,
}

impl TableColumnsGenerator {
    /// Create a new table columns generator
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Generate table columns component for an entity
    pub fn generate(
        &self,
        entity: &EntityDefinition,
        enums: &[EnumDefinition],
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let tables_dir = self.config.output_dir
            .join("presentation")
            .join("components")
            .join("tables")
            .join(&self.config.module);

        if !self.config.dry_run {
            fs::create_dir_all(&tables_dir).ok();
        }

        // Generate table columns component
        let content = self.generate_table_columns_content(entity, enums);
        let file_path = tables_dir.join(format!("{}TableColumns.tsx", entity_pascal));

        result.add_file(file_path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&file_path, content).ok();
        }

        Ok(result)
    }

    /// Generate table columns content
    fn generate_table_columns_content(
        &self,
        entity: &EntityDefinition,
        enums: &[EnumDefinition],
    ) -> String {
        let entity_pascal = to_pascal_case(&entity.name);

        // Generate column definitions
        let column_defs = self.generate_column_definitions(entity, enums);

        // Generate column accessors
        let column_accessors = self.generate_column_accessors(entity);

        format!(
r#"/**
 * {entity_pascal} Table Columns
 *
 * Column definitions and cell renderers for {entity_pascal} data tables.
 * Generated from schema definition.
 *
 * @module presentation/tables/{module}/{entity_pascal}TableColumns
 */

import React, {{ useMemo }} from 'react';
import type {{ ColumnDef }} from '@tanstack/react-table';
import type {{ {entity_pascal} }} from '@webapp/domain/{module}/entity/{entity_pascal}.schema';
import {{ IconButton, Chip, Link }} from '@/components/ui';
import {{ Visibility, Edit, Delete as DeleteIcon }} from '@/components/ui';

// ============================================================================
// Types
// ============================================================================

export interface {entity_pascal}TableColumnsOptions {{
  onEdit?: (row: {entity_pascal}) => void;
  onDelete?: (row: {entity_pascal}) => void;
  onView?: (row: {entity_pascal}) => void;
}}

// ============================================================================
// Column Definitions
// ============================================================================

/**
 * Get table columns for {entity_pascal}
 */
export function {entity_pascal}TableColumns(
  options?: {entity_pascal}TableColumnsOptions
): ColumnDef<{entity_pascal}>[] {{
  const {{ onEdit, onDelete, onView }} = options ?? {{}};

  return [
{column_defs}
    // Actions column
    {{
      id: 'actions',
      header: 'Actions',
      cell: ({{ row }}) => (
        <div style={{{{ display: 'flex', gap: '0.5rem' }}}}>

          {{onView && (
            <IconButton
              size="sm"
              variant="outlined"
              color="neutral"
              onClick={{() => onView(row.original)}}
              title="View"
            >
              <Visibility />
            </IconButton>
          )}}
          {{onEdit && (
            <IconButton
              size="sm"
              variant="outlined"
              color="primary"
              onClick={{() => onEdit(row.original)}}
              title="Edit"
            >
              <Edit />
            </IconButton>
          )}}
          {{onDelete && (
            <IconButton
              size="sm"
              variant="outlined"
              color="danger"
              onClick={{() => onDelete(row.original)}}
              title="Delete"
            >
              <DeleteIcon />
            </IconButton>
          )}}
        </div>
      ),
    }},
  ];
}}

// ============================================================================
// Hook
// ============================================================================

/**
 * Hook for memoized table columns
 */
export function use{entity_pascal}TableColumns(
  options?: {entity_pascal}TableColumnsOptions
): ColumnDef<{entity_pascal}>[] {{
  return useMemo(
    () => {entity_pascal}TableColumns(options),
    [options?.onEdit, options?.onDelete, options?.onView]
  );
}}

// ============================================================================
// Column Accessors
// ============================================================================

{column_accessors}

// <<< CUSTOM: Add custom column renderers here
// END CUSTOM
"#,
            entity_pascal = entity_pascal,
            module = self.config.module,
            column_defs = column_defs,
            column_accessors = column_accessors,
        )
    }

    /// Generate column definitions
    fn generate_column_definitions(&self, entity: &EntityDefinition, _enums: &[EnumDefinition]) -> String {
        let columns: Vec<String> = entity.fields.iter()
            .filter(|f| !self.is_sensitive_field(f))
            .take(8) // Limit to reasonable number of columns
            .map(|f| self.generate_column_def(f, _enums))
            .collect();

        columns.join("\n")
    }

    /// Generate a single column definition
    fn generate_column_def(&self, field: &FieldDefinition, _enums: &[EnumDefinition]) -> String {
        let field_name = &field.name;
        let header = self.field_to_label(field_name);
        let cell_renderer = self.get_cell_renderer(field);

        format!(
r#"    {{
      accessorKey: '{field_name}',
      header: '{header}',
      cell: {cell_renderer},
    }},"#,
            field_name = field_name,
            header = header,
            cell_renderer = cell_renderer,
        )
    }

    /// Get cell renderer for field type
    fn get_cell_renderer(&self, field: &FieldDefinition) -> String {
        match &field.type_name {
            FieldType::Bool => {
                "({ getValue }) => getValue() ? <Chip color=\"success\" size=\"sm\">✓</Chip> : <Chip color=\"neutral\" size=\"sm\">✗</Chip>".to_string()
            }
            FieldType::DateTime | FieldType::Date => {
                "({ getValue }) => getValue() ? new Date(getValue() as string).toLocaleDateString() : '-'".to_string()
            }
            FieldType::Time => {
                "({ getValue }) => getValue() || '-'".to_string()
            }
            FieldType::Json => {
                "({ getValue }) => JSON.stringify(getValue()).substring(0, 50) + '...'".to_string()
            }
            FieldType::Array(_) => {
                "({ getValue }) => Array.isArray(getValue()) ? <Chip size=\"sm\" variant=\"soft\">${(getValue() as unknown[]).length} items</Chip> : '-'".to_string()
            }
            FieldType::Enum(_) | FieldType::Custom(_) => {
                "({ getValue }) => getValue() ? <Chip size=\"sm\" variant=\"soft\">{getValue() as string}</Chip> : '-'".to_string()
            }
            _ => {
                if field.name.contains("email") {
                    "({ getValue }) => getValue() ? <Link href={`mailto:${getValue()}`}>{getValue() as string}</Link> : '-'".to_string()
                } else if field.name.contains("url") || field.name.contains("website") {
                    "({ getValue }) => getValue() ? <Link href={getValue() as string} target=\"_blank\" rel=\"noopener noreferrer\">Link</Link> : '-'".to_string()
                } else {
                    "({ getValue }) => getValue() ?? '-'".to_string()
                }
            }
        }
    }

    /// Generate column accessors
    fn generate_column_accessors(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);

        let accessors: Vec<String> = entity.fields.iter()
            .filter(|f| !self.is_sensitive_field(f))
            .map(|f| {
                let field_pascal = to_pascal_case(&f.name);
                format!(
                    "export const get{entity_pascal}{field_pascal} = (row: {entity_pascal}) => row.{field_name};",
                    entity_pascal = entity_pascal,
                    field_pascal = field_pascal,
                    field_name = f.name,
                )
            })
            .collect();

        accessors.join("\n")
    }

    /// Check if field is sensitive
    fn is_sensitive_field(&self, field: &FieldDefinition) -> bool {
        let name = field.name.to_lowercase();
        name.contains("password") ||
        name.contains("secret") ||
        name.contains("token") ||
        name.contains("hash") ||
        name.contains("key") && !name.contains("api_key_id")
    }

    /// Convert field name to label
    fn field_to_label(&self, name: &str) -> String {
        name.split('_')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}
