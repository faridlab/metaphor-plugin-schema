//! Presentation layer generators for React components
//!
//! Generates:
//! - Form components with field-level generation
//! - Table components with column definitions
//! - CRUD page components
//! - Detail view components

mod form_fields;
mod table_columns;
mod crud_pages;
mod detail_view;
mod page_config;

pub use form_fields::FormFieldsGenerator;
pub use table_columns::TableColumnsGenerator;
pub use crud_pages::CrudPagesGenerator;
pub use detail_view::DetailViewGenerator;
pub use page_config::PageConfigGenerator;

use std::fs;

use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition};
use crate::webgen::ast::HookSchema;
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::generators::domain::DomainGenerationResult;

/// Presentation layer generator
pub struct PresentationGenerator {
    form_fields_gen: FormFieldsGenerator,
    table_columns_gen: TableColumnsGenerator,
    crud_pages_gen: CrudPagesGenerator,
    detail_view_gen: DetailViewGenerator,
    page_config_gen: PageConfigGenerator,
    config: Config,
}

impl PresentationGenerator {
    /// Create a new presentation generator
    pub fn new(config: Config) -> Self {
        Self {
            form_fields_gen: FormFieldsGenerator::new(config.clone()),
            table_columns_gen: TableColumnsGenerator::new(config.clone()),
            crud_pages_gen: CrudPagesGenerator::new(config.clone()),
            detail_view_gen: DetailViewGenerator::new(config.clone()),
            page_config_gen: PageConfigGenerator::new(config.clone()),
            config,
        }
    }

    /// Generate all presentation layer components
    pub fn generate_all(
        &self,
        entities: &[EntityDefinition],
        enums: &[EnumDefinition],
        hooks: &[HookSchema],
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        // Find hook for entity by name matching
        let find_hook = |entity_name: &str| -> Option<&HookSchema> {
            hooks.iter().find(|h| h.model.eq_ignore_ascii_case(entity_name))
        };

        // Generate for each entity
        for entity in entities {
            let hook_schema = find_hook(&entity.name);

            // Form fields component
            let form_result = self.form_fields_gen.generate(entity, enums, hook_schema)?;
            self.merge_result(&mut result, form_result);

            // Table columns component
            let table_result = self.table_columns_gen.generate(entity, enums)?;
            self.merge_result(&mut result, table_result);

            // CRUD pages
            let crud_result = self.crud_pages_gen.generate(entity, enums)?;
            self.merge_result(&mut result, crud_result);

            // Detail view
            let detail_result = self.detail_view_gen.generate(entity, enums)?;
            self.merge_result(&mut result, detail_result);

            // Page config for generic templates
            let config_result = self.page_config_gen.generate(entity, enums)?;
            self.merge_result(&mut result, config_result);

            // Wrapper pages using generic templates
            let wrapper_result = self.page_config_gen.generate_wrapper_pages(entity, enums)?;
            self.merge_result(&mut result, wrapper_result);

            // Barrel export (index.ts) for entity pages directory
            let barrel_result = self.page_config_gen.generate_barrel_export(entity, enums)?;
            self.merge_result(&mut result, barrel_result);
        }

        // Generate index files
        self.generate_index_files(entities, &mut result)?;

        Ok(result)
    }

    /// Merge a sub-result into the main result
    fn merge_result(&self, main: &mut DomainGenerationResult, sub: DomainGenerationResult) {
        main.files_generated.extend(sub.files_generated);
        main.dry_run_files.extend(sub.dry_run_files);
    }

    /// Generate index files for presentation layer
    fn generate_index_files(
        &self,
        entities: &[EntityDefinition],
        result: &mut DomainGenerationResult,
    ) -> Result<()> {
        let base_dir = self.config.output_dir
            .join("presentation")
            .join("components")
            .join("forms")
            .join(&self.config.module);

        if !self.config.dry_run {
            fs::create_dir_all(&base_dir).ok();
        }

        // Generate forms index
        let forms_index = self.generate_forms_index(entities);
        let forms_index_path = base_dir.join("index.ts");

        result.add_file(forms_index_path.clone(), self.config.dry_run);
        if !self.config.dry_run {
            fs::write(&forms_index_path, forms_index).ok();
        }

        // Generate tables index
        let tables_dir = self.config.output_dir
            .join("presentation")
            .join("components")
            .join("tables")
            .join(&self.config.module);

        if !self.config.dry_run {
            fs::create_dir_all(&tables_dir).ok();
        }

        let tables_index = self.generate_tables_index(entities);
        let tables_index_path = tables_dir.join("index.ts");

        result.add_file(tables_index_path.clone(), self.config.dry_run);
        if !self.config.dry_run {
            fs::write(&tables_index_path, tables_index).ok();
        }

        // Generate pages index
        let pages_dir = self.config.output_dir
            .join("presentation")
            .join("pages")
            .join(&self.config.module);

        if !self.config.dry_run {
            fs::create_dir_all(&pages_dir).ok();
        }

        let pages_index = self.generate_pages_index(entities);
        let pages_index_path = pages_dir.join("index.ts");

        result.add_file(pages_index_path.clone(), self.config.dry_run);
        if !self.config.dry_run {
            fs::write(&pages_index_path, pages_index).ok();
        }

        // Generate configs index for generic templates
        let configs_dir = self.config.output_dir
            .join("presentation")
            .join("pages")
            .join("templates")
            .join("configs");

        if !self.config.dry_run {
            fs::create_dir_all(&configs_dir).ok();
        }

        let configs_index = self.generate_configs_index(entities);
        let configs_index_path = configs_dir.join("index.ts");

        result.add_file(configs_index_path.clone(), self.config.dry_run);
        if !self.config.dry_run {
            fs::write(&configs_index_path, configs_index).ok();
        }

        Ok(())
    }

    fn generate_forms_index(&self, entities: &[EntityDefinition]) -> String {
        use crate::webgen::parser::to_pascal_case;

        let exports: Vec<String> = entities.iter()
            .map(|e| {
                let pascal = to_pascal_case(&e.name);
                format!(
                    "export {{ {pascal}FormFields, {pascal}CreateForm, {pascal}EditForm }} from './{pascal}FormFields';",
                    pascal = pascal
                )
            })
            .collect();

        format!(
            "// Form components for {} module\n// Generated by metaphor-webgen\n\n{}\n",
            self.config.module,
            exports.join("\n")
        )
    }

    fn generate_tables_index(&self, entities: &[EntityDefinition]) -> String {
        use crate::webgen::parser::to_pascal_case;

        let exports: Vec<String> = entities.iter()
            .map(|e| {
                let pascal = to_pascal_case(&e.name);
                format!(
                    "export {{ {pascal}TableColumns, use{pascal}TableColumns }} from './{pascal}TableColumns';",
                    pascal = pascal
                )
            })
            .collect();

        format!(
            "// Table column definitions for {} module\n// Generated by metaphor-webgen\n\n{}\n",
            self.config.module,
            exports.join("\n")
        )
    }

    fn generate_pages_index(&self, entities: &[EntityDefinition]) -> String {
        use crate::webgen::parser::to_pascal_case;

        let exports: Vec<String> = entities.iter()
            .map(|e| {
                let pascal = to_pascal_case(&e.name);
                format!(
                    "export {{ {pascal}ListPage, {pascal}DetailPage, {pascal}CreatePage, {pascal}EditPage }} from './{pascal}Pages';",
                    pascal = pascal
                )
            })
            .collect();

        format!(
            "// CRUD pages for {} module\n// Generated by metaphor-webgen\n\n{}\n",
            self.config.module,
            exports.join("\n")
        )
    }

    fn generate_configs_index(&self, entities: &[EntityDefinition]) -> String {
        use crate::webgen::parser::{to_camel_case, to_snake_case};

        let exports: Vec<String> = entities.iter()
            .map(|e| {
                let camel = to_camel_case(&e.name);
                let snake = to_snake_case(&e.name);
                format!(
                    "export {{ {camel}Config }} from './{snake}.config';",
                    camel = camel,
                    snake = snake
                )
            })
            .collect();

        format!(
            r#"/**
 * Resource Page Configurations
 *
 * Export all entity configurations for use with generic page templates.
 * Generated by metaphor-webgen.
 *
 * @module presentation/pages/templates/configs
 */

{}

// <<< CUSTOM: Add custom exports here
// END CUSTOM
"#,
            exports.join("\n")
        )
    }
}
