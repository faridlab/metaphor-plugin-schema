//! Form fields generator
//!
//! Generates React form components with proper field types based on schema definitions.
//! Uses MUI Material v6 components from @/components/ui

use std::fs;

use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition, FieldDefinition, FieldType};
use crate::webgen::ast::HookSchema;
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::{to_pascal_case, to_camel_case};
use crate::webgen::generators::domain::DomainGenerationResult;

/// Generator for form field components
pub struct FormFieldsGenerator {
    config: Config,
}

impl FormFieldsGenerator {
    /// Create a new form fields generator
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Generate form fields component for an entity
    pub fn generate(
        &self,
        entity: &EntityDefinition,
        enums: &[EnumDefinition],
        _hooks: Option<&HookSchema>,
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let forms_dir = self.config.output_dir
            .join("presentation")
            .join("components")
            .join("forms")
            .join(&self.config.module);

        if !self.config.dry_run {
            fs::create_dir_all(&forms_dir).ok();
        }

        // Generate form fields component
        let content = self.generate_form_fields_content(entity, enums);
        let file_path = forms_dir.join(format!("{}FormFields.tsx", entity_pascal));

        result.add_file(file_path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&file_path, content).ok();
        }

        Ok(result)
    }

    /// Generate form fields component content
    fn generate_form_fields_content(
        &self,
        entity: &EntityDefinition,
        enums: &[EnumDefinition],
    ) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_camel = to_camel_case(&entity.name);

        // Generate imports
        let enum_imports = self.generate_enum_imports(entity, enums);

        // Generate form fields
        let form_fields = self.generate_field_components(entity, enums);

        // Generate default values
        let default_values = self.generate_default_values(entity, enums);

        format!(
r#"/**
 * {entity_pascal} Form Fields
 *
 * Reusable form field components for {entity_pascal} entity.
 * Generated from schema definition.
 *
 * @module presentation/forms/{module}/{entity_pascal}FormFields
 */

import React from 'react';
import {{ useForm, Controller }} from 'react-hook-form';
import {{ zodResolver }} from '@hookform/resolvers/zod';
import {{
  create{entity_pascal}Schema,
  update{entity_pascal}Schema,
  type Create{entity_pascal}Input,
  type Update{entity_pascal}Input,
}} from '@webapp/domain/{module}/entity/{entity_pascal}.schema';
{enum_imports}
import {{
  Box,
  Stack,
  FormControl,
  FormLabel,
  FormHelperText,
  TextField,
  Button,
  Checkbox,
  Select,
  MenuItem,
  Typography,
}} from '@/components/ui';

// ============================================================================
// Types
// ============================================================================

export interface {entity_pascal}FormFieldsProps {{
  control: ReturnType<typeof useForm>['control'];
  errors: ReturnType<typeof useForm>['formState']['errors'];
  disabled?: boolean;
}}

export interface {entity_pascal}CreateFormProps {{
  onSubmit: (data: Create{entity_pascal}Input) => void | Promise<void>;
  onCancel?: () => void;
  defaultValues?: Partial<Create{entity_pascal}Input>;
  isLoading?: boolean;
}}

export interface {entity_pascal}EditFormProps {{
  onSubmit: (data: Update{entity_pascal}Input) => void | Promise<void>;
  onCancel?: () => void;
  defaultValues: Update{entity_pascal}Input;
  isLoading?: boolean;
}}

// ============================================================================
// Default Values
// ============================================================================

export const {entity_camel}DefaultValues: Partial<Create{entity_pascal}Input> = {{
{default_values}
}};

// ============================================================================
// Form Fields Component
// ============================================================================

/**
 * Reusable form fields for {entity_pascal}
 */
export function {entity_pascal}FormFields({{
  control,
  errors,
  disabled = false,
}}: {entity_pascal}FormFieldsProps) {{
  return (
    <Stack spacing={{2}}>
{form_fields}
    </Stack>
  );
}}

// ============================================================================
// Create Form
// ============================================================================

/**
 * Form for creating a new {entity_pascal}
 */
export function {entity_pascal}CreateForm({{
  onSubmit,
  onCancel,
  defaultValues,
  isLoading = false,
}}: {entity_pascal}CreateFormProps) {{
  const {{
    control,
    handleSubmit,
    formState: {{ errors }},
  }} = useForm<Create{entity_pascal}Input>({{
    resolver: zodResolver(create{entity_pascal}Schema),
    defaultValues: {{ ...{entity_camel}DefaultValues, ...defaultValues }},
  }});

  return (
    <Box component="form" onSubmit={{handleSubmit(onSubmit)}}>
      <Stack spacing={{3}}>
        <{entity_pascal}FormFields
          control={{control}}
          errors={{errors}}
          disabled={{isLoading}}
        />

        <Stack direction="row" spacing={{2}} justifyContent="flex-end">
          {{onCancel && (
            <Button
              type="button"
              onClick={{onCancel}}
              disabled={{isLoading}}
              variant="outlined"
            >
              Cancel
            </Button>
          )}}
          <Button
            type="submit"
            disabled={{isLoading}}
            variant="solid"
            loading={{isLoading}}
          >
            Create {entity_pascal}
          </Button>
        </Stack>
      </Stack>
    </Box>
  );
}}

// ============================================================================
// Edit Form
// ============================================================================

/**
 * Form for editing an existing {entity_pascal}
 */
export function {entity_pascal}EditForm({{
  onSubmit,
  onCancel,
  defaultValues,
  isLoading = false,
}}: {entity_pascal}EditFormProps) {{
  const {{
    control,
    handleSubmit,
    formState: {{ errors }},
  }} = useForm<Update{entity_pascal}Input>({{
    resolver: zodResolver(update{entity_pascal}Schema),
    defaultValues,
  }});

  return (
    <Box component="form" onSubmit={{handleSubmit(onSubmit)}}>
      <Stack spacing={{3}}>
        <{entity_pascal}FormFields
          control={{control}}
          errors={{errors}}
          disabled={{isLoading}}
        />

        <Stack direction="row" spacing={{2}} justifyContent="flex-end">
          {{onCancel && (
            <Button
              type="button"
              onClick={{onCancel}}
              disabled={{isLoading}}
              variant="outlined"
            >
              Cancel
            </Button>
          )}}
          <Button
            type="submit"
            disabled={{isLoading}}
            variant="solid"
            loading={{isLoading}}
          >
            Save Changes
          </Button>
        </Stack>
      </Stack>
    </Box>
  );
}}

// <<< CUSTOM: Add custom form components here
// END CUSTOM
"#,
            entity_pascal = entity_pascal,
            entity_camel = entity_camel,
            module = self.config.module,
            enum_imports = enum_imports,
            form_fields = form_fields,
            default_values = default_values,
        )
    }

    /// Generate enum imports
    fn generate_enum_imports(&self, entity: &EntityDefinition, enums: &[EnumDefinition]) -> String {
        let used_enums: Vec<&EnumDefinition> = enums.iter()
            .filter(|e| self.entity_uses_enum(entity, &e.name))
            .collect();

        if used_enums.is_empty() {
            return String::new();
        }

        let imports: Vec<String> = used_enums.iter()
            .map(|e| format!("  {}Values,", e.name))
            .collect();

        format!(
            "import {{\n{}\n}} from '@webapp/domain/{}/entity/{}.schema';\n",
            imports.join("\n"),
            self.config.module,
            to_pascal_case(&entity.name)
        )
    }

    /// Check if entity uses a specific enum
    fn entity_uses_enum(&self, entity: &EntityDefinition, enum_name: &str) -> bool {
        entity.fields.iter().any(|f| {
            matches!(&f.type_name, FieldType::Enum(name) if name == enum_name) ||
            matches!(&f.type_name, FieldType::Custom(name) if name == enum_name)
        })
    }

    /// Generate form field components
    fn generate_field_components(&self, entity: &EntityDefinition, enums: &[EnumDefinition]) -> String {
        let fields: Vec<String> = entity.fields.iter()
            .filter(|f| !self.is_auto_generated_field(f))
            .map(|f| self.generate_field_component(f, enums))
            .collect();

        fields.join("\n\n")
    }

    /// Generate a single field component
    fn generate_field_component(&self, field: &FieldDefinition, enums: &[EnumDefinition]) -> String {
        let field_name = &field.name;
        let label = self.field_to_label(field_name);
        let is_required = !field.optional;

        match &field.type_name {
            FieldType::String | FieldType::Text => {
                self.generate_text_field(field_name, &label, is_required, field.type_name == FieldType::Text)
            }
            FieldType::Email => {
                self.generate_email_field(field_name, &label, is_required)
            }
            FieldType::Url => {
                self.generate_url_field(field_name, &label, is_required)
            }
            FieldType::Phone => {
                self.generate_phone_field(field_name, &label, is_required)
            }
            FieldType::Ip => {
                // IP address is rendered as a text field
                self.generate_text_field(field_name, &label, is_required, false)
            }
            FieldType::Int | FieldType::Float | FieldType::Decimal => {
                self.generate_number_field(field_name, &label, is_required)
            }
            FieldType::Bool => {
                self.generate_checkbox_field(field_name, &label)
            }
            FieldType::DateTime | FieldType::Date => {
                self.generate_date_field(field_name, &label, is_required)
            }
            FieldType::Time => {
                self.generate_time_field(field_name, &label, is_required)
            }
            FieldType::Uuid => {
                self.generate_text_field(field_name, &label, is_required, false)
            }
            FieldType::Enum(name) | FieldType::Custom(name) => {
                if let Some(enum_def) = enums.iter().find(|e| &e.name == name) {
                    self.generate_select_field(field_name, &label, is_required, enum_def)
                } else {
                    self.generate_text_field(field_name, &label, is_required, false)
                }
            }
            FieldType::Json => {
                self.generate_json_field(field_name, &label, is_required)
            }
            FieldType::Array(_) => {
                self.generate_array_field(field_name, &label)
            }
            FieldType::Optional(inner) => {
                // Recursively handle optional fields
                let inner_field = FieldDefinition {
                    name: field.name.clone(),
                    type_name: inner.as_ref().clone(),
                    optional: true,
                    ..field.clone()
                };
                self.generate_field_component(&inner_field, enums)
            }
        }
    }

    /// Generate text input field
    fn generate_text_field(&self, name: &str, label: &str, required: bool, multiline: bool) -> String {
        let required_asterisk = if required { " *" } else { "" };
        let multiline_prop = if multiline { r#"multiline rows={4}"# } else { "" };
        let label_value = format!(r#"{}{}"#, label, required_asterisk);

        format!(
r#"      {{/* Field */}}
      <Controller
        name="{name}"
        control={{control}}
        render={{({{ field }}) => (
          <TextField
            {{...field}}
            label="{label_value}"
            error={{!!errors.{name}}}
            helperText={{errors.{name}?.message as string}}
            disabled={{disabled}}
            variant="outlined"
            size="small"
            {multiline_prop}
            fullWidth
          />
        )}}
      />"#,
            name = name,
            label_value = label_value,
            multiline_prop = multiline_prop,
        )
    }

    /// Generate email input field
    fn generate_email_field(&self, name: &str, label: &str, required: bool) -> String {
        let required_asterisk = if required { " *" } else { "" };
        let label_value = format!(r#"{}{}"#, label, required_asterisk);

        format!(
r#"      {{/* Field */}}
      <Controller
        name="{name}"
        control={{control}}
        render={{({{ field }}) => (
          <TextField
            {{...field}}
            label="{label_value}"
            type="email"
            error={{!!errors.{name}}}
            helperText={{errors.{name}?.message as string}}
            disabled={{disabled}}
            variant="outlined"
            size="small"
            fullWidth
          />
        )}}
      />"#,
            name = name,
            label_value = label_value,
        )
    }

    /// Generate URL input field
    fn generate_url_field(&self, name: &str, label: &str, required: bool) -> String {
        let required_asterisk = if required { " *" } else { "" };
        let label_value = format!(r#"{}{}"#, label, required_asterisk);

        format!(
r#"      {{/* Field */}}
      <Controller
        name="{name}"
        control={{control}}
        render={{({{ field }}) => (
          <TextField
            {{...field}}
            label="{label_value}"
            type="url"
            error={{!!errors.{name}}}
            helperText={{errors.{name}?.message as string}}
            disabled={{disabled}}
            placeholder="https://"
            variant="outlined"
            size="small"
            fullWidth
          />
        )}}
      />"#,
            name = name,
            label_value = label_value,
        )
    }

    /// Generate phone input field
    fn generate_phone_field(&self, name: &str, label: &str, required: bool) -> String {
        let required_asterisk = if required { " *" } else { "" };
        let label_value = format!(r#"{}{}"#, label, required_asterisk);

        format!(
r#"      {{/* Field */}}
      <Controller
        name="{name}"
        control={{control}}
        render={{({{ field }}) => (
          <TextField
            {{...field}}
            label="{label_value}"
            type="tel"
            error={{!!errors.{name}}}
            helperText={{errors.{name}?.message as string}}
            disabled={{disabled}}
            variant="outlined"
            size="small"
            fullWidth
          />
        )}}
      />"#,
            name = name,
            label_value = label_value,
        )
    }

    /// Generate number input field
    fn generate_number_field(&self, name: &str, label: &str, required: bool) -> String {
        let required_asterisk = if required { " *" } else { "" };
        let label_value = format!(r#"{}{}"#, label, required_asterisk);

        format!(
r#"      {{/* Field */}}
      <Controller
        name="{name}"
        control={{control}}
        render={{({{ field }}) => (
          <TextField
            {{...field}}
            label="{label_value}"
            type="number"
            error={{!!errors.{name}}}
            helperText={{errors.{name}?.message as string}}
            disabled={{disabled}}
            onChange={{(e) => field.onChange(e.target.valueAsNumber)}}
            variant="outlined"
            size="small"
            fullWidth
          />
        )}}
      />"#,
            name = name,
            label_value = label_value,
        )
    }

    /// Generate checkbox field
    fn generate_checkbox_field(&self, name: &str, label: &str) -> String {
        format!(
r#"      {{/* Field */}}
      <Controller
        name="{name}"
        control={{control}}
        render={{({{ field }}) => (
          <Checkbox
            {{...field}}
            checked={{field.value}}
            disabled={{disabled}}
            label="{label}"
          />
        )}}
      />"#,
            name = name,
            label = label,
        )
    }

    /// Generate date input field
    fn generate_date_field(&self, name: &str, label: &str, required: bool) -> String {
        let required_asterisk = if required { " *" } else { "" };
        let label_value = format!(r#"{}{}"#, label, required_asterisk);

        format!(
r#"      {{/* Field */}}
      <Controller
        name="{name}"
        control={{control}}
        render={{({{ field }}) => (
          <TextField
            {{...field}}
            label="{label_value}"
            type="datetime-local"
            error={{!!errors.{name}}}
            helperText={{errors.{name}?.message as string}}
            disabled={{disabled}}
            variant="outlined"
            size="small"
            fullWidth
            InputLabelProps={{{{
              shrink: true,
            }}}}
          />
        )}}
      />"#,
            name = name,
            label_value = label_value,
        )
    }

    /// Generate time input field
    fn generate_time_field(&self, name: &str, label: &str, required: bool) -> String {
        let required_asterisk = if required { " *" } else { "" };
        let label_value = format!(r#"{}{}"#, label, required_asterisk);

        format!(
r#"      {{/* Field */}}
      <Controller
        name="{name}"
        control={{control}}
        render={{({{ field }}) => (
          <TextField
            {{...field}}
            label="{label_value}"
            type="time"
            error={{!!errors.{name}}}
            helperText={{errors.{name}?.message as string}}
            disabled={{disabled}}
            variant="outlined"
            size="small"
            fullWidth
            InputLabelProps={{{{
              shrink: true,
            }}}}
          />
        )}}
      />"#,
            name = name,
            label_value = label_value,
        )
    }

    /// Generate select field for enums
    fn generate_select_field(&self, name: &str, label: &str, required: bool, enum_def: &EnumDefinition) -> String {
        let required_asterisk = if required { " *" } else { "" };
        let label_value = format!(r#"{}{}"#, label, required_asterisk);

        let options: Vec<String> = enum_def.variants.iter()
            .map(|v| format!(
                "              <MenuItem value=\"{}\">{}</MenuItem>",
                v.name,
                self.field_to_label(&v.name)
            ))
            .collect();

        format!(
r#"      {{/* Field */}}
      <Controller
        name="{name}"
        control={{control}}
        render={{({{ field }}) => (
          <TextField
            {{...field}}
            label="{label_value}"
            select
            error={{!!errors.{name}}}
            helperText={{errors.{name}?.message as string}}
            disabled={{disabled}}
            variant="outlined"
            size="small"
            fullWidth
          >
            <MenuItem value="">Select {label}</MenuItem>
            {options}
          </TextField>
        )}}
      />"#,
            name = name,
            label = label,
            label_value = label_value,
            options = options.join("\n"),
        )
    }

    /// Generate JSON field
    fn generate_json_field(&self, name: &str, label: &str, required: bool) -> String {
        let required_asterisk = if required { " *" } else { "" };
        let label_value = format!(r#"{}{}"#, label, required_asterisk);

        format!(
r#"      {{/* JSON field */}}
      <Controller
        name="{name}"
        control={{control}}
        render={{({{ field }}) => (
          <TextField
            value={{JSON.stringify(field.value, null, 2)}}
            onChange={{(e) => {{
              try {{
                field.onChange(JSON.parse(e.target.value));
              }} catch {{
                // Invalid JSON, keep current value
              }}
            }}}}
            label="{label_value}"
            error={{!!errors.{name}}}
            helperText={{errors.{name}?.message as string}}
            disabled={{disabled}}
            multiline
            rows={{4}}
            variant="outlined"
            size="small"
            fullWidth
            sx={{{{ fontFamily: 'monospace', fontSize: '0.75rem' }}}}
          />
        )}}
      />"#,
            name = name,
            label_value = label_value,
        )
    }

    /// Generate array field placeholder
    fn generate_array_field(&self, name: &str, label: &str) -> String {
        format!(
r#"      {{/* {label} (Array) */}}
      <Controller
        name="{name}"
        control={{control}}
        render={{({{ field }}) => (
          <Box>
            <Typography variant="body2" sx={{{{ mb: 0.5 }}}}>

              {label}
            </Typography>
            <Box
              sx={{{{
                p: 2,
                border: '1px solid',
                borderColor: 'divider',
                borderRadius: 1,
                bgcolor: 'action.hover',
              }}}}
            >
              <Typography variant="body2" color="text.secondary">
                Array field: {{Array.isArray(field.value) ? field.value.length : 0}} items
              </Typography>
              {{/* TODO: Implement array field editor */}}
            </Box>
            {{errors.{name} && (
              <FormHelperText error>{{errors.{name}?.message as string}}</FormHelperText>
            )}}
          </Box>
        )}}
      />"#,
            name = name,
            label = label,
        )
    }

    /// Generate default values for form
    fn generate_default_values(&self, entity: &EntityDefinition, enums: &[EnumDefinition]) -> String {
        let defaults: Vec<String> = entity.fields.iter()
            .filter(|f| !self.is_auto_generated_field(f))
            .map(|f| {
                let default = self.get_default_value(f, enums);
                format!("  {}: {},", f.name, default)
            })
            .collect();

        defaults.join("\n")
    }

    /// Get default value for a field
    fn get_default_value(&self, field: &FieldDefinition, enums: &[EnumDefinition]) -> String {
        if let Some(default) = &field.default_value {
            // Sanitize the default value to ensure it's valid TypeScript
            return self.sanitize_default_value(default, &field.type_name);
        }

        match &field.type_name {
            FieldType::String | FieldType::Text | FieldType::Email |
            FieldType::Url | FieldType::Phone | FieldType::Uuid | FieldType::Ip => "''".to_string(),
            FieldType::Int | FieldType::Float | FieldType::Decimal => "0".to_string(),
            FieldType::Bool => "false".to_string(),
            FieldType::DateTime | FieldType::Date | FieldType::Time => "''".to_string(),
            FieldType::Json => "'{}'".to_string(),
            FieldType::Array(_) => "[]".to_string(),
            FieldType::Enum(name) | FieldType::Custom(name) => {
                if let Some(enum_def) = enums.iter().find(|e| &e.name == name) {
                    if let Some(first) = enum_def.variants.first() {
                        format!("'{}'", first.name)
                    } else {
                        "''".to_string()
                    }
                } else {
                    "''".to_string()
                }
            }
            FieldType::Optional(_) => "null".to_string(),
        }
    }

    /// Sanitize default value from schema to be valid TypeScript
    fn sanitize_default_value(&self, value: &str, field_type: &FieldType) -> String {
        let trimmed = value.trim();

        // Handle special function-like values
        if trimmed == "now()" || trimmed.to_lowercase() == "current_timestamp" {
            return "new Date()".to_string();
        }

        // Handle boolean values
        if trimmed == "true" || trimmed == "false" {
            return trimmed.to_string();
        }

        // Handle numeric values
        if matches!(field_type, FieldType::Int | FieldType::Float | FieldType::Decimal)
            && trimmed.parse::<f64>().is_ok() {
                return trimmed.to_string();
            }

        // Handle null
        if trimmed == "null" {
            return "null".to_string();
        }

        // Handle JSON objects/arrays (basic check)
        if trimmed.starts_with('{') && trimmed.ends_with('}') {
            // Quote the JSON string
            return format!("'{}'", trimmed.replace('\\', "\\\\").replace('\'', "\\'"));
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            return trimmed.to_string();
        }

        // Handle URN/URL-like values (e.g., "urn:oasis:names:tc:SAML:1.1:nameid-format:unspecified")
        if trimmed.contains(':') && trimmed.contains('/') {
            return format!("'{}'", trimmed.replace('\\', "\\\\").replace('\'', "\\'"));
        }

        // Handle version-like values (e.g., "v1.0", "v2.1")
        if trimmed.starts_with('v') || trimmed.starts_with('V') {
            return format!("'{}'", trimmed.replace('\\', "\\\\").replace('\'', "\\'"));
        }

        // Default: quote as string, escaping special characters
        format!("'{}'", trimmed.replace('\\', "\\\\").replace('\'', "\\'"))
    }

    /// Check if field is auto-generated
    fn is_auto_generated_field(&self, field: &FieldDefinition) -> bool {
        let name = field.name.to_lowercase();
        name == "id" ||
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
