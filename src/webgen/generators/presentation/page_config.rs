//! Page configuration generator
//!
//! Generates entity-specific configuration files for use with generic page templates.
//! This follows a convention-over-configuration approach similar to Quasar framework.

use std::fs;
use std::path::PathBuf;

use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition, FieldDefinition};
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::{to_pascal_case, to_camel_case, to_snake_case, to_kebab_case, pluralize};
use crate::webgen::generators::domain::DomainGenerationResult;

/// Generator for page configuration files
pub struct PageConfigGenerator {
    config: Config,
}

impl PageConfigGenerator {
    /// Create a new page config generator
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Generate page config for an entity
    pub fn generate(
        &self,
        entity: &EntityDefinition,
        _enums: &[EnumDefinition],
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let configs_dir = self.config.output_dir
            .join("presentation")
            .join("pages")
            .join("templates")
            .join("configs");

        if !self.config.dry_run {
            fs::create_dir_all(&configs_dir).ok();
        }

        // Generate config file
        let file_path = configs_dir.join(format!("{}.config.tsx", to_snake_case(&entity.name)));

        // Check if file exists and preserve custom code
        let existing_custom = self.extract_custom_code(&file_path);
        let content = self.generate_config_content(entity, existing_custom);

        result.add_file(file_path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&file_path, content).ok();
        }

        Ok(result)
    }

    /// Extract custom code from existing file if it exists
    fn extract_custom_code(&self, file_path: &PathBuf) -> Option<String> {
        if let Ok(content) = fs::read_to_string(file_path) {
            // Find custom code section
            if let Some(start) = content.find("// <<< CUSTOM") {
                if let Some(end) = content.find("// END CUSTOM") {
                    let custom_start = content[start..].find('\n').map(|i| start + i + 1)?;
                    let custom_content = &content[custom_start..end];
                    if !custom_content.trim().is_empty() {
                        return Some(custom_content.to_string());
                    }
                }
            }
        }
        None
    }

    /// Generate config file content
    fn generate_config_content(&self, entity: &EntityDefinition, existing_custom: Option<String>) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_snake = to_snake_case(&entity.name);
        let entity_camel = to_camel_case(&entity.name);
        let entity_kebab = to_kebab_case(&entity.name);
        let entity_kebab_plural = pluralize(&entity_kebab);
        let has_soft_delete = entity.has_soft_delete();

        // Generate column definitions from entity fields
        let columns = self.generate_columns(entity);

        // Generate filter definitions if entity has status-like fields
        let filters = self.generate_filters(entity);

        // Find display name field (prefer 'name', 'title', 'username', then fall back to 'id')
        let display_field = self.find_display_field(entity);

        // Hook imports based on soft delete support
        let trash_hooks = if has_soft_delete {
            format!(
r#"    useTrashList: use{entity_pascal}TrashList,
    useRestore: use{entity_pascal}Restore,
    usePermanentDelete: use{entity_pascal}PermanentDelete,"#,
                entity_pascal = entity_pascal,
            )
        } else {
            String::new()
        };

        let trash_hook_imports = if has_soft_delete {
            format!(
                ",\n  use{entity_pascal}TrashList,\n  use{entity_pascal}Restore,\n  use{entity_pascal}PermanentDelete",
                entity_pascal = entity_pascal,
            )
        } else {
            String::new()
        };

        let trash_config = if has_soft_delete {
            r#"
  trash: {
    defaultSortField: 'deleted_at',
    defaultSortDirection: 'desc',
    infoMessage: 'Items in trash will be permanently deleted after 30 days. You can restore items before they are permanently deleted.',
  },"#.to_string()
        } else {
            String::new()
        };

        let custom_section = existing_custom.unwrap_or_default();

        format!(
r#"/**
 * {entity_pascal} Resource Page Configuration
 *
 * Configuration for {entity_pascal} entity pages using generic templates.
 * Generated from schema definition. Custom code preserved between markers.
 *
 * @module presentation/pages/templates/configs/{entity_snake}.config
 */

import {{ Box }} from '@/components/ui/joy';
import {{
  use{entity_pascal}List,
  use{entity_pascal},
  use{entity_pascal}Delete{trash_hook_imports},
}} from '@webapp/application/hooks/{module}/use{entity_pascal}';
import type {{ {entity_pascal} }} from '@webapp/domain/{module}';
import type {{ ResourcePageConfig, ColumnConfig, SelectFilterConfig }} from '../types';

// ============================================================================
// Column Definitions
// ============================================================================

const listColumns: ColumnConfig<{entity_pascal}>[] = [
{columns}
];

// ============================================================================
// Filter Definitions
// ============================================================================
{filters}
// ============================================================================
// {entity_pascal} Configuration Export
// ============================================================================

/**
 * {entity_pascal} resource page configuration
 *
 * Use with ResourceListPage and ResourceTrashPage:
 * ```tsx
 * <ResourceListPage config={{{entity_camel}Config}} />
 * <ResourceTrashPage config={{{entity_camel}Config}} />
 * ```
 */
export const {entity_camel}Config: ResourcePageConfig<{entity_pascal}> = {{
  hooks: {{
    useList: use{entity_pascal}List,
    useDetail: use{entity_pascal},
    useDelete: use{entity_pascal}Delete,
{trash_hooks}
  }},
  routes: {{
    basePath: '/{module}/{entity_kebab_plural}',
  }},
  display: {{
    singular: '{entity_pascal}',
    plural: '{entity_pascal}s',
    getDisplayName: ({entity_camel}) => {entity_camel}.{display_field} ?? {entity_camel}.id,
    searchPlaceholder: 'Search {entity_snake}s...',
    emptyMessage: 'Get started by creating your first {entity_snake}',
  }},
  list: {{
    columns: listColumns,{filters_ref}
    defaultSortField: 'id',
    defaultSortDirection: 'asc',
    defaultPageSize: 20,
    hasSoftDelete: {has_soft_delete},
  }},{trash_config}
}};

// <<< CUSTOM: Add custom column renderers, filters, or configuration overrides here
{custom_section}// END CUSTOM

export default {entity_camel}Config;
"#,
            entity_pascal = entity_pascal,
            entity_snake = entity_snake,
            entity_camel = entity_camel,
            entity_kebab_plural = entity_kebab_plural,
            module = self.config.module,
            columns = columns,
            filters = filters,
            filters_ref = if !filters.is_empty() { "\n    selectFilters," } else { "" },
            display_field = display_field,
            trash_hooks = trash_hooks,
            trash_hook_imports = trash_hook_imports,
            has_soft_delete = has_soft_delete,
            trash_config = trash_config,
            custom_section = custom_section,
        )
    }

    /// Generate column definitions from entity fields
    fn generate_columns(&self, entity: &EntityDefinition) -> String {
        let mut columns = Vec::new();

        for field in &entity.fields {
            // Skip system fields and sensitive fields
            if self.should_skip_field(field) {
                continue;
            }

            let column = self.generate_column(field);
            columns.push(column);
        }

        columns.join("\n")
    }

    /// Check if field should be skipped in column generation
    fn should_skip_field(&self, field: &FieldDefinition) -> bool {
        let skip_fields = [
            "id", "created_at", "updated_at", "deleted_at",
            "password", "password_hash", "secret", "token",
            "metadata", "created_by", "updated_by",
        ];

        let field_lower = field.name.to_lowercase();
        skip_fields.iter().any(|&skip| field_lower == skip || field_lower.contains("password") || field_lower.contains("secret"))
    }

    /// Generate a single column definition
    fn generate_column(&self, field: &FieldDefinition) -> String {
        let field_snake = to_snake_case(&field.name);
        let label = self.generate_label(&field.name);
        let width = self.estimate_column_width(field);
        let sortable = self.is_sortable(field);
        let render = self.generate_render_function(field);

        let mut column = format!(
r#"  {{
    id: '{field_snake}',
    label: '{label}',
    field: '{field_snake}',"#,
            field_snake = field_snake,
            label = label,
        );

        if let Some(w) = width {
            column.push_str(&format!("\n    width: {},", w));
        }

        column.push_str(&format!("\n    sortable: {},", sortable));

        if let Some(render_fn) = render {
            column.push_str(&format!("\n    render: {},", render_fn));
        }

        column.push_str("\n  },");
        column
    }

    /// Generate human-readable label from field name
    fn generate_label(&self, name: &str) -> String {
        to_snake_case(name)
            .split('_')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Estimate column width based on field type
    fn estimate_column_width(&self, field: &FieldDefinition) -> Option<u32> {
        use crate::webgen::ast::entity::FieldType;

        let base_type = field.type_name.base_type();
        match base_type {
            FieldType::Bool => Some(100),
            FieldType::Date | FieldType::DateTime | FieldType::Time => Some(140),
            FieldType::Uuid => Some(150),
            FieldType::Int => Some(100),
            _ => None, // Auto width for strings and other types
        }
    }

    /// Check if field should be sortable
    fn is_sortable(&self, field: &FieldDefinition) -> bool {
        use crate::webgen::ast::entity::FieldType;

        let base_type = field.type_name.base_type();
        !matches!(base_type, FieldType::Json | FieldType::Array(_))
    }

    /// Generate render function for field type
    fn generate_render_function(&self, field: &FieldDefinition) -> Option<String> {
        use crate::webgen::ast::entity::FieldType;

        let base_type = field.type_name.base_type();
        match base_type {
            FieldType::Bool => Some("(value: unknown) => (\n      <Box sx={{ fontSize: '0.875rem' }}>\n        {value ? '✓ Yes' : '—'}\n      </Box>\n    )".to_string()),
            FieldType::Date | FieldType::DateTime | FieldType::Time => Some("(value: unknown) => {\n      if (!value) return 'Never';\n      return new Date(value as string).toLocaleDateString('en-US', {\n        month: 'short',\n        day: 'numeric',\n        year: 'numeric',\n      });\n    }".to_string()),
            _ => None,
        }
    }

    /// Generate filter definitions if entity has status-like fields
    fn generate_filters(&self, entity: &EntityDefinition) -> String {
        let mut filters = Vec::new();

        for field in &entity.fields {
            let field_lower = field.name.to_lowercase();

            // Generate filter for status/type/category fields
            if field_lower == "status" || field_lower.ends_with("_status") ||
               field_lower == "type" || field_lower.ends_with("_type") ||
               field_lower == "category" {
                let filter = self.generate_filter(field);
                filters.push(filter);
            }
        }

        if filters.is_empty() {
            return String::new();
        }

        format!(
r#"
const selectFilters: SelectFilterConfig[] = [
{}
];
"#,
            filters.join("\n")
        )
    }

    /// Generate a single filter definition
    fn generate_filter(&self, field: &FieldDefinition) -> String {
        let field_snake = to_snake_case(&field.name);
        let label = format!("Filter by {}", self.generate_label(&field.name).to_lowercase());

        format!(
r#"  {{
    name: '{field_snake}',
    label: '{label}',
    options: [
      {{ value: 'all', label: 'All' }},
      // TODO: Add status options from enum or schema
    ],
    defaultValue: 'all',
  }},"#,
            field_snake = field_snake,
            label = label,
        )
    }

    /// Find the best field to use for display name
    fn find_display_field(&self, entity: &EntityDefinition) -> String {
        // Priority order for display field
        let preferred = ["name", "title", "username", "email", "label", "display_name"];

        for pref in &preferred {
            if entity.fields.iter().any(|f| f.name.to_lowercase() == *pref) {
                return pref.to_string();
            }
        }

        // Fall back to first string field that's not an ID or system field
        for field in &entity.fields {
            use crate::webgen::ast::entity::FieldType;

            let field_lower = field.name.to_lowercase();
            let base_type = field.type_name.base_type();

            let is_string_type = matches!(base_type, FieldType::String | FieldType::Text | FieldType::Email);

            if is_string_type &&
               !field_lower.contains("id") &&
               !field_lower.contains("password") &&
               !field_lower.contains("secret") {
                return to_snake_case(&field.name);
            }
        }

        "id".to_string()
    }

    /// Generate wrapper pages that use generic templates
    pub fn generate_wrapper_pages(
        &self,
        entity: &EntityDefinition,
        _enums: &[EnumDefinition],
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let pages_dir = self.config.output_dir
            .join("presentation")
            .join("pages")
            .join(&self.config.module);

        if !self.config.dry_run {
            fs::create_dir_all(&pages_dir).ok();
        }

        // Generate wrapper pages file
        let file_path = pages_dir.join(format!("{}WrapperPages.tsx", entity_pascal));

        // Check if file exists and preserve custom code
        let existing_custom = self.extract_custom_code(&file_path);
        let content = self.generate_wrapper_content(entity, existing_custom);

        result.add_file(file_path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&file_path, content).ok();
        }

        Ok(result)
    }

    /// Generate wrapper pages content
    fn generate_wrapper_content(&self, entity: &EntityDefinition, existing_custom: Option<String>) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_camel = to_camel_case(&entity.name);
        let has_soft_delete = entity.has_soft_delete();

        let trash_wrapper = if has_soft_delete {
            format!(
r#"
/**
 * {entity_pascal} Trash Page
 * Wrapper component using ResourceTrashPage with {entity_camel}Config
 */
export function {entity_pascal}TrashPage() {{
  return <ResourceTrashPage config={{{entity_camel}Config}} />;
}}
"#,
                entity_pascal = entity_pascal,
                entity_camel = entity_camel,
            )
        } else {
            String::new()
        };

        let trash_import = if has_soft_delete {
            ", ResourceTrashPage"
        } else {
            ""
        };

        let custom_section = existing_custom.unwrap_or_default();

        format!(
r#"/**
 * {entity_pascal} Wrapper Pages
 *
 * Thin wrapper components using generic ResourceListPage and ResourceTrashPage.
 * These use the {entity_camel}Config for all entity-specific configuration.
 *
 * Generated by metaphor-webgen. Custom code preserved between markers.
 *
 * @module presentation/pages/{module}/{entity_pascal}WrapperPages
 */

import {{ ResourceListPage{trash_import} }} from '@webapp/presentation/pages/templates';
import {{ {entity_camel}Config }} from '@webapp/presentation/pages/templates/configs';

/**
 * {entity_pascal} List Page
 * Wrapper component using ResourceListPage with {entity_camel}Config
 */
export function {entity_pascal}ListPage() {{
  return <ResourceListPage config={{{entity_camel}Config}} />;
}}
{trash_wrapper}
// <<< CUSTOM: Add custom page components or overrides here
{custom_section}// END CUSTOM
"#,
            entity_pascal = entity_pascal,
            entity_camel = entity_camel,
            module = self.config.module,
            trash_import = trash_import,
            trash_wrapper = trash_wrapper,
            custom_section = custom_section,
        )
    }

    /// Generate barrel export (index.ts) that re-exports from wrapper pages
    pub fn generate_barrel_export(
        &self,
        entity: &EntityDefinition,
        _enums: &[EnumDefinition],
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_snake = to_snake_case(&entity.name);
        let has_soft_delete = entity.has_soft_delete();

        // Create entity-specific pages directory (e.g., pages/sapiens/roles/)
        let entity_pages_dir = self.config.output_dir
            .join("presentation")
            .join("pages")
            .join(&self.config.module)
            .join(format!("{}s", entity_snake)); // plural form for directory

        if !self.config.dry_run {
            fs::create_dir_all(&entity_pages_dir).ok();
        }

        let file_path = entity_pages_dir.join("index.ts");

        // Check if file exists and preserve custom exports
        let existing_custom = self.extract_custom_code(&file_path);
        let content = self.generate_barrel_content(entity, has_soft_delete, existing_custom);

        result.add_file(file_path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&file_path, content).ok();
        }

        Ok(result)
    }

    /// Generate barrel export content
    fn generate_barrel_content(&self, entity: &EntityDefinition, has_soft_delete: bool, existing_custom: Option<String>) -> String {
        let entity_pascal = to_pascal_case(&entity.name);

        let trash_export = if has_soft_delete {
            format!(", {entity_pascal}TrashPage", entity_pascal = entity_pascal)
        } else {
            String::new()
        };

        let trash_detail_export = if has_soft_delete {
            format!("\nexport {{ {entity_pascal}TrashDetailPage }} from './{entity_pascal}TrashDetailPage';", entity_pascal = entity_pascal)
        } else {
            String::new()
        };

        let custom_section = existing_custom.unwrap_or_default();

        format!(
r#"/**
 * {entity_pascal} Pages Export
 *
 * Barrel export for {entity_pascal} pages.
 * List and Trash pages use generic ResourcePage templates via wrapper pages.
 * Custom pages (Create, Detail) use specific implementations.
 *
 * Generated by metaphor-webgen. Custom code preserved between markers.
 */

// Generic template pages (using ResourceListPage and ResourceTrashPage)
export {{ {entity_pascal}ListPage{trash_export} }} from '../{entity_pascal}WrapperPages';

// Custom pages (specific implementations)
export {{ {entity_pascal}CreatePage }} from './{entity_pascal}CreatePage';
export {{ {entity_pascal}DetailPage }} from './{entity_pascal}DetailPage';{trash_detail_export}

// Default export for lazy loading
export {{ {entity_pascal}ListPage as default }} from '../{entity_pascal}WrapperPages';

// <<< CUSTOM: Add custom page exports here
{custom_section}// END CUSTOM
"#,
            entity_pascal = entity_pascal,
            trash_export = trash_export,
            trash_detail_export = trash_detail_export,
            custom_section = custom_section,
        )
    }
}
