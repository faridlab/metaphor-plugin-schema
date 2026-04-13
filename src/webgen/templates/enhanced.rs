//! Enhanced templates for field-aware code generation

use crate::webgen::ast::entity::{EntityDefinition, FieldDefinition, FieldType, EnumDefinition};

/// Enhanced template generator for field-aware forms
pub struct FormTemplates;

impl FormTemplates {
    /// Generate field-aware form fields for an entity
    pub fn generate_form_fields(
        entity: &EntityDefinition,
        enums: &[EnumDefinition],
        is_create: bool,
    ) -> String {
        let mut fields = String::new();

        for field in &entity.fields {
            // Skip auto-generated ID fields in create forms
            if is_create && field.name == "id" {
                continue;
            }

            // Skip sensitive fields like password_hash
            if field.name.contains("password") || field.name.contains("hash") {
                continue;
            }

            let field_component = Self::field_input_component(field, enums);
            fields.push_str(&field_component);
            fields.push('\n');
        }

        fields
    }

    /// Generate the appropriate input component for a field type
    fn field_input_component(field: &FieldDefinition, enums: &[EnumDefinition]) -> String {
        let label = Self::field_label(field);
        let field_path = &field.name;
        let is_optional = field.optional;

        match &field.type_name {
            FieldType::String | FieldType::Text => {
                if Self::is_email_field(field) {
                    Self::text_field(field_path, &label, "email", is_optional)
                } else if Self::is_url_field(field) {
                    Self::text_field(field_path, &label, "url", is_optional)
                } else if Self::is_multiline(field) {
                    Self::textarea_field(field_path, &label, is_optional)
                } else {
                    Self::text_field(field_path, &label, "text", is_optional)
                }
            }
            FieldType::Int => Self::number_field(field_path, &label, "integer", is_optional),
            FieldType::Float => Self::number_field(field_path, &label, "decimal", is_optional),
            FieldType::Bool => Self::switch_field(field_path, &label),
            FieldType::DateTime => Self::date_time_field(field_path, &label, is_optional),
            FieldType::Date => Self::date_field(field_path, &label, is_optional),
            FieldType::Time => Self::time_field(field_path, &label, is_optional),
            FieldType::Email => Self::text_field(field_path, &label, "email", is_optional),
            FieldType::Phone => Self::text_field(field_path, &label, "tel", is_optional),
            FieldType::Url => Self::text_field(field_path, &label, "url", is_optional),
            FieldType::Json => Self::json_field(field_path, &label, is_optional),
            FieldType::Custom(type_name) => {
                // Check if it's an enum
                if let Some(enum_def) = enums.iter().find(|e| &e.name == type_name) {
                    Self::select_field(field_path, &label, enum_def, is_optional)
                } else {
                    // Treat as text input for custom types
                    Self::text_field(field_path, &label, "text", is_optional)
                }
            }
            FieldType::Optional(inner) => {
                // Recursively handle optional types
                Self::optional_field(field, inner, enums)
            }
            FieldType::Array(inner) => {
                Self::array_field(field_path, &label, inner, enums)
            }
            FieldType::Enum(type_name) => {
                if let Some(enum_def) = enums.iter().find(|e| &e.name == type_name) {
                    Self::select_field(field_path, &label, enum_def, is_optional)
                } else {
                    Self::text_field(field_path, &label, "text", is_optional)
                }
            }
            _ => Self::text_field(field_path, &label, "text", is_optional),
        }
    }

    /// Check if field is an email field
    fn is_email_field(field: &FieldDefinition) -> bool {
        field.name.contains("email") ||
        field.attributes.iter().any(|a| a.name == "email")
    }

    /// Check if field is a URL field
    fn is_url_field(field: &FieldDefinition) -> bool {
        field.name.contains("url") ||
        field.name.contains("website") ||
        field.name.contains("link") ||
        field.attributes.iter().any(|a| a.name == "url")
    }

    /// Check if field should be multiline
    fn is_multiline(field: &FieldDefinition) -> bool {
        matches!(field.type_name, FieldType::Text) ||
        field.name.contains("description") ||
        field.name.contains("content") ||
        field.name.contains("body") ||
        field.attributes.iter().any(|a| a.name == "multiline")
    }

    /// Get a user-friendly label for a field
    pub fn field_label(field: &FieldDefinition) -> String {
        if let Some(desc) = &field.description {
            return desc.clone();
        }

        // Convert snake_case to Title Case
        field.name
            .split('_')
            .map(|s| {
                let mut chars = s.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => {
                        first.to_uppercase().collect::<String>() + chars.as_str()
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Generate a TextField component
    fn text_field(name: &str, label: &str, input_type: &str, optional: bool) -> String {
        let optional_mark = if optional { "" } else { " *" };
        let mut result = String::from(r#"        <TextField
          label=""#);
        result.push_str(label);
        result.push_str(optional_mark);
        result.push_str(r#""
          type=""#);
        result.push_str(input_type);
        result.push_str(r#""
          {...register('"#);
        result.push_str(name);
        result.push_str("'})");
        result.push_str(r#"}
          error={!!errors."#);
        result.push_str(name);
        result.push_str(r#"}
          helperText={errors."#);
        result.push_str(name);
        result.push_str(r#"?.message}
          fullWidth
        />"#);
        result
    }

    /// Generate a TextArea field
    fn textarea_field(name: &str, label: &str, optional: bool) -> String {
        let optional_mark = if optional { "" } else { " *" };
        let mut result = String::from(r#"        <TextField
          label=""#);
        result.push_str(label);
        result.push_str(optional_mark);
        result.push_str(r#""
          {...register('"#);
        result.push_str(name);
        result.push_str("'})");
        result.push_str(r#"}
          error={!!errors."#);
        result.push_str(name);
        result.push_str(r#"}
          helperText={errors."#);
        result.push_str(name);
        result.push_str(r#"?.message}
          multiline
          rows={4}
          fullWidth
        />"#);
        result
    }

    /// Generate a number field
    fn number_field(name: &str, label: &str, number_type: &str, optional: bool) -> String {
        let optional_mark = if optional { "" } else { " *" };
        let step = if number_type == "integer" { "1" } else { "0.01" };
        let mut result = String::from(r#"        <TextField
          label=""#);
        result.push_str(label);
        result.push_str(optional_mark);
        result.push_str(r#""
          type="number"
          inputProps={{ step: ""#);
        result.push_str(step);
        result.push_str(r#" }}
          {...register('"#);
        result.push_str(name);
        result.push_str(r#"', { valueAsNumber: true })}
          error={!!errors."#);
        result.push_str(name);
        result.push_str(r#"}
          helperText={errors."#);
        result.push_str(name);
        result.push_str(r#"?.message}
          fullWidth
        />"#);
        result
    }

    /// Generate a switch field for booleans
    fn switch_field(name: &str, label: &str) -> String {
        let mut result = String::from(r#"        <FormControlLabel
          control={
            <Controller
              name=""#);
        result.push_str(name);
        result.push_str(r#""
              control={control}
              render={({ field }) => (
                <Switch
                  checked={field.value}
                  onChange={(_, checked) => field.onChange(checked)}
                />
              )}
            />
          }
          label=""#);
        result.push_str(label);
        result.push_str(r#"
        />"#);
        result
    }

    /// Generate a date-time picker
    fn date_time_field(name: &str, label: &str, optional: bool) -> String {
        let optional_mark = if optional { "" } else { " *" };
        let mut result = String::from(r#"        <Controller
          name=""#);
        result.push_str(name);
        result.push_str(r#""
          control={control}
          render={({ field, fieldState: { error } }) => (
            <DateTimePicker
              label=""#);
        result.push_str(label);
        result.push_str(optional_mark);
        result.push_str(r#""
              value={field.value}
              onChange={(newValue) => field.onChange(newValue)}
              slotProps={{ textField: {{
                error: !!error,
                helperText: error?.message,
                fullWidth: true,
              }} }}
            />
          )}
        />"#);
        result
    }

    /// Generate a date picker
    fn date_field(name: &str, label: &str, optional: bool) -> String {
        let optional_mark = if optional { "" } else { " *" };
        let mut result = String::from(r#"        <Controller
          name=""#);
        result.push_str(name);
        result.push_str(r#""
          control={control}
          render={({ field, fieldState: { error } }) => (
            <DatePicker
              label=""#);
        result.push_str(label);
        result.push_str(optional_mark);
        result.push_str(r#""
              value={field.value}
              onChange={(newValue) => field.onChange(newValue)}
              slotProps={{ textField: {{
                error: !!error,
                helperText: error?.message,
                fullWidth: true,
              }} }}
            />
          )}
        />"#);
        result
    }

    /// Generate a time picker
    fn time_field(name: &str, label: &str, optional: bool) -> String {
        let optional_mark = if optional { "" } else { " *" };
        let mut result = String::from(r#"        <Controller
          name=""#);
        result.push_str(name);
        result.push_str(r#""
          control={control}
          render={({ field, fieldState: { error } }) => (
            <TimePicker
              label=""#);
        result.push_str(label);
        result.push_str(optional_mark);
        result.push_str(r#""
              value={field.value}
              onChange={(newValue) => field.onChange(newValue)}
              slotProps={{ textField: {{
                error: !!error,
                helperText: error?.message,
                fullWidth: true,
              }} }}
            />
          )}
        />"#);
        result
    }

    /// Generate a select field for enums
    fn select_field(name: &str, label: &str, enum_def: &EnumDefinition, optional: bool) -> String {
        let options = enum_def.variants.iter()
            .map(|v| {
                let value = &v.name;
                let display = v.description.as_ref().unwrap_or(value);
                format!(r#"                <MenuItem value="{}">{}</MenuItem>"#, value, display)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let optional_mark = if optional { "" } else { " *" };
        let mut result = String::from(r#"        <FormControl fullWidth error={!!errors."#);
        result.push_str(name);
        result.push_str(r#"}>
          <InputLabel id=""#);
        result.push_str(name);
        result.push_str(r#"-label">"#);
        result.push_str(label);
        result.push_str(optional_mark);
        result.push_str(r#"</InputLabel>
          <Select
            labelId=""#);
        result.push_str(name);
        result.push_str(r#"-label"
            label=""#);
        result.push_str(label);
        result.push_str(optional_mark);
        result.push_str(r#""
            {...register('"#);
        result.push_str(name);
        result.push_str("'})}");
        result.push_str(r#"
          >
"#);
        result.push_str(&options);
        result.push_str(r#"
          </Select>
          {errors."#);
        result.push_str(name);
        result.push_str(r#" && (
            <FormHelperText error>{errors."#);
        result.push_str(name);
        result.push_str(r#"?.message}</FormHelperText>
          )}
        </FormControl>"#);
        result
    }

    /// Generate a JSON editor field
    fn json_field(name: &str, label: &str, optional: bool) -> String {
        let optional_mark = if optional { "" } else { " *" };
        let mut result = String::from(r#"        <TextField
          label=""#);
        result.push_str(label);
        result.push_str(optional_mark);
        result.push_str(r#""
          {...register('"#);
        result.push_str(name);
        result.push_str("'})");
        result.push_str(r#"}
          error={!!errors."#);
        result.push_str(name);
        result.push_str(r#"}
          helperText={errors."#);
        result.push_str(name);
        result.push_str(r#"?.message || "JSON format"}
          multiline
          rows={3}
          placeholder='{"key": "value"}'
          fullWidth
        />"#);
        result
    }

    /// Generate field for optional wrapper types
    fn optional_field(field: &FieldDefinition, inner: &FieldType, enums: &[EnumDefinition]) -> String {
        // Create a temporary field without the optional wrapper
        let temp_field = FieldDefinition {
            name: field.name.clone(),
            type_name: inner.clone(),
            attributes: field.attributes.clone(),
            description: field.description.clone(),
            optional: true,
            default_value: field.default_value.clone(),
        };
        Self::field_input_component(&temp_field, enums)
    }

    /// Generate field for array types
    fn array_field(name: &str, label: &str, inner: &FieldType, enums: &[EnumDefinition]) -> String {
        match inner {
            FieldType::String | FieldType::Text => {
                let mut result = String::from(r#"        <ArrayField
          name=""#);
                result.push_str(name);
                result.push_str(r#""}
          label=""#);
                result.push_str(label);
                result.push_str(r#"
          renderField={(index, fieldName) => (
            <TextField
              {...register(fieldName)}
              size="small"
              fullWidth
            />
          )}
        />"#);
                result
            }
            FieldType::Custom(type_name) | FieldType::Enum(type_name) => {
                if let Some(enum_def) = enums.iter().find(|e| e.name == *type_name) {
                    Self::array_select_field(name, label, enum_def)
                } else {
                    let mut result = String::from(r#"        <TextField
          label=""#);
                    result.push_str(label);
                    result.push_str(r#""
          {...register('"#);
                    result.push_str(name);
                    result.push_str(r#"'"})}"#);
                    result.push_str(r#"
          helperText="Array field (comma-separated values)"
          fullWidth
        />"#);
                    result
                }
            }
            _ => {
                let mut result = String::from(r#"        <TextField
          label=""#);
                result.push_str(label);
                result.push_str(r#""
          {...register('"#);
                result.push_str(name);
                result.push_str(r#"'"})}"#);
                result.push_str(r#"
          helperText="Array field"
          fullWidth
        />"#);
                result
            }
        }
    }

    /// Generate a select field for array of enums
    fn array_select_field(name: &str, label: &str, enum_def: &EnumDefinition) -> String {
        let options = enum_def.variants.iter()
            .map(|v| {
                format!(r#"                <MenuItem value="{}">{}</MenuItem>"#,
                    v.name,
                    v.description.as_ref().unwrap_or(&v.name)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let mut result = String::from(r#"        <FormControl fullWidth>
          <InputLabel id=""#);
        result.push_str(name);
        result.push_str(r#"-label">"#);
        result.push_str(label);
        result.push_str(r#"</InputLabel>
          <Select
            labelId=""#);
        result.push_str(name);
        result.push_str(r#"-label"
            label=""#);
        result.push_str(label);
        result.push_str(r#""}
            multiple
            {...register('"#);
        result.push_str(name);
        result.push_str(r#"'"})}"#);
        result.push_str(r#"
            renderValue={(selected) => (selected as string[]).join(', ')}
          >
"#);
        result.push_str(&options);
        result.push_str(r#"
          </Select>
          <FormHelperText>Multiple selection</FormHelperText>
        </FormControl>"#);
        result
    }

    /// Generate Zod schema for an entity's fields
    pub fn generate_zod_schema(
        entity: &EntityDefinition,
        enums: &[EnumDefinition],
        is_create: bool,
    ) -> String {
        let mut fields = String::new();

        for field in &entity.fields {
            // Skip auto-generated ID fields in create schemas
            if is_create && (field.name == "id" || field.has_id_attribute()) {
                continue;
            }

            let schema = Self::zod_field_schema(field, enums);
            fields.push_str(&schema);
            fields.push('\n');
        }

        fields
    }

    /// Generate Zod schema for a single field
    fn zod_field_schema(field: &FieldDefinition, enums: &[EnumDefinition]) -> String {
        let name = &field.name;
        let (base_type, validations) = Self::zod_type_for_field(field, enums);

        let optional_suffix = if field.optional { ".optional()" } else { "" };
        let default_suffix = if let Some(default_val) = &field.default_value {
            format!(".default({})", default_val)
        } else if field.optional {
            ".nullish()".to_string()
        } else {
            "".to_string()
        };

        let indent = "  ";
        format!("{}{}: {}{}{}{},",
            indent,
            name,
            base_type,
            validations,
            optional_suffix,
            default_suffix
        )
    }

    /// Get the Zod type and validations for a field
    fn zod_type_for_field(field: &FieldDefinition, enums: &[EnumDefinition]) -> (String, String) {
        let validations = Self::zod_validations(field);

        let base_type = match &field.type_name {
            FieldType::String | FieldType::Text => "z.string()".to_string(),
            FieldType::Int => "z.number()".to_string(),
            FieldType::Float => "z.number()".to_string(),
            FieldType::Decimal => "z.number()".to_string(),
            FieldType::Bool => "z.boolean()".to_string(),
            FieldType::DateTime => "z.date()".to_string(),
            FieldType::Date => "z.date()".to_string(),
            FieldType::Time => "z.string()".to_string(),
            FieldType::Email => "z.string().email()".to_string(),
            FieldType::Phone => "z.string()".to_string(),
            FieldType::Url => "z.string().url()".to_string(),
            FieldType::Uuid => "z.string().uuid()".to_string(),
            FieldType::Json => "z.record(z.any())".to_string(),
            FieldType::Ip => "z.string()".to_string(), // IP address as string
            FieldType::Custom(type_name) => {
                if enums.iter().any(|e| e.name == *type_name) {
                    format!("z.enum({}Enum)", type_name)
                } else {
                    "z.any()".to_string()
                }
            }
            FieldType::Enum(type_name) => {
                format!("z.enum({}Enum)", type_name)
            }
            FieldType::Optional(inner) => {
                // Handle optional types by getting inner type
                let temp_field = FieldDefinition {
                    name: field.name.clone(),
                    type_name: inner.as_ref().clone(),
                    attributes: field.attributes.clone(),
                    description: field.description.clone(),
                    optional: false,
                    default_value: None,
                };
                Self::zod_type_for_field(&temp_field, enums).0
            }
            FieldType::Array(inner) => {
                let (inner_type, _) = Self::zod_type_for_inner_array(inner, enums);
                format!("z.array({})", inner_type)
            }
        };

        (base_type, validations)
    }

    /// Get Zod type for array inner element
    fn zod_type_for_inner_array(field_type: &FieldType, enums: &[EnumDefinition]) -> (String, String) {
        match field_type {
            FieldType::String | FieldType::Text | FieldType::Ip => ("z.string()".to_string(), "".to_string()),
            FieldType::Int => ("z.number()".to_string(), "".to_string()),
            FieldType::Bool => ("z.boolean()".to_string(), "".to_string()),
            FieldType::Custom(type_name) | FieldType::Enum(type_name) => {
                if enums.iter().any(|e| e.name == *type_name) {
                    (format!("z.enum({}Enum)", type_name), "".to_string())
                } else {
                    ("z.string()".to_string(), "".to_string())
                }
            }
            _ => ("z.any()".to_string(), "".to_string()),
        }
    }

    /// Generate Zod validation chains from field attributes
    fn zod_validations(field: &FieldDefinition) -> String {
        let mut validations = Vec::new();

        for attr in &field.attributes {
            match attr.name.as_str() {
                "min" => {
                    if let Some(arg) = attr.first_arg() {
                        if matches!(field.type_name, FieldType::String | FieldType::Text | FieldType::Email | FieldType::Phone | FieldType::Url | FieldType::Ip) {
                            validations.push(format!(".min({}, 'Must be at least {} characters')", arg, arg));
                        } else {
                            validations.push(format!(".min({}, 'Must be at least {}')", arg, arg));
                        }
                    }
                }
                "max" => {
                    if let Some(arg) = attr.first_arg() {
                        if matches!(field.type_name, FieldType::String | FieldType::Text | FieldType::Email | FieldType::Phone | FieldType::Url | FieldType::Ip) {
                            validations.push(format!(".max({}, 'Must be at most {} characters')", arg, arg));
                        } else {
                            validations.push(format!(".max({}, 'Must be at most {}')", arg, arg));
                        }
                    }
                }
                "required" => {
                    // Handled by not making the field optional
                }
                "unique" => {
                    // Unique constraint is server-side, can add a note
                }
                "email" => {
                    // Already handled by z.string().email()
                }
                "url" => {
                    // Already handled by z.string().url()
                }
                "alpha_dash" => {
                    validations.push(".regex(/^[a-zA-Z0-9_-]+$/, 'Only alphanumeric characters, underscore, and hyphen allowed')".to_string());
                }
                _ => {}
            }
        }

        validations.join("")
    }
}

/// Data table templates for entity list pages
pub struct TableTemplates;

impl TableTemplates {
    /// Generate table columns for an entity
    pub fn generate_table_columns(entity: &EntityDefinition, module: &str, entity_snake: &str) -> String {
        let mut columns = Vec::new();

        // Add ID column first
        columns.push(Self::column_def("id", "ID", 100));

        // Add other non-sensitive fields
        for field in &entity.fields {
            if field.name == "id" {
                continue;
            }

            // Skip sensitive fields
            if field.name.contains("password") || field.name.contains("hash") || field.name.contains("token") {
                continue;
            }

            // Skip very long text fields in the main table
            if matches!(field.type_name, FieldType::Text | FieldType::Json) {
                continue;
            }

            let label = FormTemplates::field_label(field);
            let width = Self::column_width(&field.type_name);
            columns.push(Self::column_def(&field.name, &label, width));
        }

        // Add actions column last
        columns.push(Self::actions_column_def(module, entity_snake));

        columns.join(",\n")
    }

    /// Generate column definition
    fn column_def(field: &str, header: &str, width: u32) -> String {
        format!(r#"    {{
      field: '{}',
      headerName: '{}',
      width: {},
      sortable: true,
    }}"#,
            field, header, width
        )
    }

    /// Generate actions column definition
    fn actions_column_def(module: &str, entity_snake: &str) -> String {
        format!(r#"    {{
      field: 'actions',
      headerName: 'Actions',
      width: 120,
      sortable: false,
      renderCell: (params) => (
        <TableCell>
          <IconButton
            size="small"
            onClick={{() => navigate(`/{}/{}/${{params.row.id}}`)}}
          >
            <Visibility fontSize="small" />
          </IconButton>
          <IconButton
            size="small"
            onClick={{() => navigate(`/{}/{}/${{params.row.id}}/edit`)}}
          >
            <Edit fontSize="small" />
          </IconButton>
        </TableCell>
      ),
    }}"#, module, entity_snake, module, entity_snake)
    }

    /// Get appropriate column width for field type (returns number for MUI DataGrid)
    fn column_width(field_type: &FieldType) -> u32 {
        match field_type {
            FieldType::Bool => 100,
            FieldType::Int | FieldType::Float => 120,
            FieldType::DateTime | FieldType::Date | FieldType::Time => 180,
            FieldType::Email | FieldType::Phone => 200,
            FieldType::Uuid => 250,
            _ => 150,
        }
    }

    /// Generate table row render function
    pub fn generate_table_rows(entity: &EntityDefinition) -> String {
        let mut row_fields = Vec::new();

        for field in &entity.fields {
            if field.name == "id" {
                continue;
            }

            if field.name.contains("password") || field.name.contains("hash") || field.name.contains("token") {
                continue;
            }

            if matches!(field.type_name, FieldType::Text | FieldType::Json) {
                continue;
            }

            let render = Self::row_cell_renderer(field);
            row_fields.push(render);
        }

        row_fields.join("\n\n")
    }

    /// Generate cell renderer for a field
    fn row_cell_renderer(field: &FieldDefinition) -> String {
        match &field.type_name {
            FieldType::Bool => {
                format!(r#"// Boolean: {}
      row.{} ? <Chip label="Yes" color="success" size="small" /> : <Chip label="No" color="default" size="small" />"#,
                    field.name, field.name
                )
            }
            FieldType::DateTime => {
                format!(r#"// DateTime: {}
      row.{} ? new Date(row.{}).toLocaleString() : '-'"#,
                    field.name, field.name, field.name
                )
            }
            FieldType::Date => {
                format!(r#"// Date: {}
      row.{} ? new Date(row.{}).toLocaleDateString() : '-'"#,
                    field.name, field.name, field.name
                )
            }
            FieldType::Email => {
                // String concatenation to avoid format! escaping with JSX template literals
                let mut result = String::from("// Email: ");
                result.push_str(&field.name);
                result.push_str(r#"
      <Link href={`mailto:${row."#);
                result.push_str(&field.name);
                result.push_str(r#"}`}>{row."#);
                result.push_str(&field.name);
                result.push_str(r#"}</Link>"#);
                result
            }
            FieldType::Url => {
                // String concatenation to avoid format! escaping with JSX
                let mut result = String::from("// URL: ");
                result.push_str(&field.name);
                result.push_str(r#"
      <Link href={row."#);
                result.push_str(&field.name);
                result.push_str(r#"} target="_blank" rel="noopener">
        {row."#);
                result.push_str(&field.name);
                result.push_str(r#"?.length > 30 ? row."#);
                result.push_str(&field.name);
                result.push_str(r#".substring(0, 30) + '...' : row."#);
                result.push_str(&field.name);
                result.push_str(r#"}
      </Link>"#);
                result
            }
            FieldType::Enum(_type_name) | FieldType::Custom(_type_name) => {
                // String concatenation to avoid format! escaping with JSX
                let mut result = String::from("// Enum: ");
                result.push_str(&field.name);
                result.push_str(r#"
      <Chip label={row."#);
                result.push_str(&field.name);
                result.push_str(r#"} size="small" variant="outlined" />"#);
                result
            }
            _ => {
                format!(r#"// Text: {}
      row.{}"#,
                    field.name, field.name
                )
            }
        }
    }
}

impl FieldDefinition {
    /// Check if field has @id attribute
    fn has_id_attribute(&self) -> bool {
        self.attributes.iter().any(|a| a.name == "id")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_field_generation() {
        let _field = FieldDefinition {
            name: "username".to_string(),
            type_name: FieldType::String,
            attributes: vec![],
            description: Some("Username".to_string()),
            optional: false,
            default_value: None,
        };

        let result = FormTemplates::text_field("username", "Username", "text", false);
        assert!(result.contains("Username"));
        assert!(result.contains("*")); // Required
    }

    #[test]
    fn test_zod_schema_generation() {
        let field = FieldDefinition {
            name: "email".to_string(),
            type_name: FieldType::Email,
            attributes: vec![],
            description: Some("Email address".to_string()),
            optional: true,
            default_value: None,
        };

        let result = FormTemplates::zod_field_schema(&field, &[]);
        assert!(result.contains("z.string().email()"));
        assert!(result.contains(".optional()"));
    }
}
