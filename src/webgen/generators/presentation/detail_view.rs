//! Detail view generator
//!
//! Generates detail view components for displaying entity data.
//! Uses Joy UI components from @/components/ui

use std::fs;

use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition, FieldDefinition, FieldType};
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::{to_pascal_case, to_camel_case};
use crate::webgen::generators::domain::DomainGenerationResult;

/// Generator for detail view components
pub struct DetailViewGenerator {
    config: Config,
}

impl DetailViewGenerator {
    /// Create a new detail view generator
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Generate detail view component for an entity
    pub fn generate(
        &self,
        entity: &EntityDefinition,
        enums: &[EnumDefinition],
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let details_dir = self.config.output_dir
            .join("presentation")
            .join("components")
            .join("details")
            .join(&self.config.module);

        if !self.config.dry_run {
            fs::create_dir_all(&details_dir).ok();
        }

        // Generate detail view component
        let content = self.generate_detail_view_content(entity, enums);
        let file_path = details_dir.join(format!("{}DetailView.tsx", entity_pascal));

        result.add_file(file_path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&file_path, content).ok();
        }

        Ok(result)
    }

    /// Generate detail view content
    fn generate_detail_view_content(
        &self,
        entity: &EntityDefinition,
        enums: &[EnumDefinition],
    ) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_camel = to_camel_case(&entity.name);

        // Generate field displays
        let field_displays = self.generate_field_displays(entity, enums);

        format!(
r#"/**
 * {entity_pascal} Detail View
 *
 * Displays detailed information for a {entity_pascal} entity.
 * Generated from schema definition.
 *
 * @module presentation/details/{module}/{entity_pascal}DetailView
 */

import React from 'react';
import type {{ {entity_pascal} }} from '@webapp/domain/{module}/entity/{entity_pascal}.schema';
import {{
  Box,
  Stack,
  Typography,
  Sheet,
  Button,
  Divider,
  Chip,
}} from '@/components/ui';

// ============================================================================
// Types
// ============================================================================

export interface {entity_pascal}DetailViewProps {{
  {entity_camel}: {entity_pascal};
  onEdit?: () => void;
  onDelete?: () => void;
  isDeleting?: boolean;
}}

// ============================================================================
// Component
// ============================================================================

/**
 * Detail view for {entity_pascal}
 */
export function {entity_pascal}DetailView({{
  {entity_camel},
  onEdit,
  onDelete,
  isDeleting = false,
}}: {entity_pascal}DetailViewProps) {{
  return (
    <Sheet variant="outlined" sx={{{{ overflow: 'hidden' }}}}>
      {{/* Header */}}
      <Stack
        direction="row"
        justifyContent="space-between"
        alignItems="center"
        sx={{{{ p: 3, borderBottom: '1px solid', borderColor: 'divider' }}}}
      >
        <Box>
          <Typography level="h3">{{entity_pascal}} Details</Typography>
          <Typography level="body-sm" textColor="text.secondary">
            ID: {{{entity_camel}.id}}
          </Typography>
        </Box>
        <Stack direction="row" spacing={{1}}>
          {{onEdit && (
            <Button
              onClick={{onEdit}}
              variant="outlined"
            >
              Edit
            </Button>
          )}}
          {{onDelete && (
            <Button
              onClick={{onDelete}}
              disabled={{isDeleting}}
              variant="solid"
              color="danger"
              loading={{isDeleting}}
            >
              Delete
            </Button>
          )}}
        </Stack>
      </Stack>

      {{/* Content */}}
      <Box sx={{{{ p: 3 }}}}>
        <Box
          sx={{{{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fill, minmax(250px, 1fr))',
            gap: 2,
          }}}}
        >
{field_displays}
        </Box>
      </Box>

      {{/* Footer with timestamps */}}
      <Divider />
      <Stack
        direction="row"
        spacing={{4}}
        sx={{{{
          p: 2,
          bgcolor: 'background.level1',
        }}}}
      >
        {{'createdAt' in {entity_camel} && (
          <Typography level="body-xs" textColor="text.secondary">
            Created: {{new Date(({entity_camel} as Record<string, unknown>).createdAt as string).toLocaleString()}}
          </Typography>
        )}}
        {{'updatedAt' in {entity_camel} && (
          <Typography level="body-xs" textColor="text.secondary">
            Updated: {{new Date(({entity_camel} as Record<string, unknown>).updatedAt as string).toLocaleString()}}
          </Typography>
        )}}
      </Stack>
    </Sheet>
  );
}}

// ============================================================================
// Field Display Components
// ============================================================================

interface FieldDisplayProps {{
  label: string;
  value: React.ReactNode;
}}

function FieldDisplay({{ label, value }}: FieldDisplayProps) {{
  return (
    <Box>
      <Typography level="body-xs" textColor="text.secondary" sx={{{{ mb: 0.5 }}}}>
        {{label}}
      </Typography>
      <Typography level="body-sm">
        {{value ?? '-'}}
      </Typography>
    </Box>
  );
}}

function BooleanDisplay({{ label, value }}: {{ label: string; value: boolean }}) {{
  return (
    <Box>
      <Typography level="body-xs" textColor="text.secondary" sx={{{{ mb: 0.5 }}}}>
        {{label}}
      </Typography>
      <Chip
        size="sm"
        color={{value ? 'success' : 'neutral'}}
        variant={{value ? 'soft' : 'outlined'}}
      >
        {{value ? 'Yes' : 'No'}}
      </Chip>
    </Box>
  );
}}

function BadgeDisplay({{ label, value }}: {{ label: string; value: string }}) {{
  return (
    <Box>
      <Typography level="body-xs" textColor="text.secondary" sx={{{{ mb: 0.5 }}}}>
        {{label}}
      </Typography>
      <Chip size="sm" variant="soft" color="primary">
        {{value}}
      </Chip>
    </Box>
  );
}}

function DateDisplay({{ label, value }}: {{ label: string; value: string | null }}) {{
  return (
    <FieldDisplay
      label={{label}}
      value={{value ? new Date(value).toLocaleDateString() : '-'}}
    />
  );
}}

function JsonDisplay({{ label, value }}: {{ label: string; value: Record<string, unknown> }}) {{
  return (
    <Box sx={{{{ gridColumn: '1 / -1' }}}}>
      <Typography level="body-xs" textColor="text.secondary" sx={{{{ mb: 0.5 }}}}>
        {{label}}
      </Typography>
      <Sheet
        variant="soft"
        sx={{{{
          p: 2,
          borderRadius: 'sm',
          bgcolor: 'background.level1',
        }}}}
      >
        <Typography
          level="body-xs"
          sx={{{{
            fontFamily: 'monospace',
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-all',
          }}}}
        >
          {{JSON.stringify(value, null, 2)}}
        </Typography>
      </Sheet>
    </Box>
  );
}}

// <<< CUSTOM: Add custom display components here
// END CUSTOM
"#,
            entity_pascal = entity_pascal,
            entity_camel = entity_camel,
            module = self.config.module,
            field_displays = field_displays,
        )
    }

    /// Generate field displays
    fn generate_field_displays(&self, entity: &EntityDefinition, _enums: &[EnumDefinition]) -> String {
        let displays: Vec<String> = entity.fields.iter()
            .filter(|f| !self.is_sensitive_field(f) && !self.is_timestamp_field(f))
            .map(|f| self.generate_field_display(f))
            .collect();

        displays.join("\n")
    }

    /// Generate a single field display
    fn generate_field_display(&self, field: &FieldDefinition) -> String {
        let field_name = &field.name;
        let label = self.field_to_label(field_name);

        match &field.type_name {
            FieldType::Bool => {
                format!(
                    "          <BooleanDisplay label=\"{label}\" value={{{entity_camel}.{field_name}}} />",
                    label = label,
                    field_name = field_name,
                    entity_camel = "{entity_camel}",
                ).replace("{entity_camel}", &format!("{{{}}}", "entity_camel"))
            }
            FieldType::DateTime | FieldType::Date => {
                format!(
                    "          <DateDisplay label=\"{label}\" value={{{entity_camel}.{field_name} as string}} />",
                    label = label,
                    field_name = field_name,
                    entity_camel = "{entity_camel}",
                ).replace("{entity_camel}", &format!("{{{}}}", "entity_camel"))
            }
            FieldType::Enum(_) | FieldType::Custom(_) => {
                format!(
                    "          <BadgeDisplay label=\"{label}\" value={{{entity_camel}.{field_name}}} />",
                    label = label,
                    field_name = field_name,
                    entity_camel = "{entity_camel}",
                ).replace("{entity_camel}", &format!("{{{}}}", "entity_camel"))
            }
            FieldType::Json => {
                format!(
                    "          <JsonDisplay label=\"{label}\" value={{{entity_camel}.{field_name}}} />",
                    label = label,
                    field_name = field_name,
                    entity_camel = "{entity_camel}",
                ).replace("{entity_camel}", &format!("{{{}}}", "entity_camel"))
            }
            _ => {
                format!(
                    "          <FieldDisplay label=\"{label}\" value={{{entity_camel}.{field_name}}} />",
                    label = label,
                    field_name = field_name,
                    entity_camel = "{entity_camel}",
                ).replace("{entity_camel}", &format!("{{{}}}", "entity_camel"))
            }
        }
    }

    /// Check if field is sensitive
    fn is_sensitive_field(&self, field: &FieldDefinition) -> bool {
        let name = field.name.to_lowercase();
        name.contains("password") ||
        name.contains("secret") ||
        name.contains("token") ||
        name.contains("hash")
    }

    /// Check if field is a timestamp field
    fn is_timestamp_field(&self, field: &FieldDefinition) -> bool {
        let name = field.name.to_lowercase();
        name == "created_at" ||
        name == "createdat" ||
        name == "updated_at" ||
        name == "updatedat" ||
        name == "deleted_at" ||
        name == "deletedat"
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
