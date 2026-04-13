//! Enhanced webapp code generator using YAML schemas

use std::fs;
use std::path::{Path, PathBuf};
use crate::webgen::config::{Config, Target};
use crate::webgen::error::{Error, Result};
use crate::webgen::parser::{
    ModelParser,
    HookParser,
    to_snake_case, to_pascal_case,
};
use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition};
use crate::webgen::ast::HookSchema;
use crate::webgen::templates::enhanced::{FormTemplates, TableTemplates};
use crate::webgen::templates::base::TemplateReplacer;
use crate::webgen::generators::{
    DomainGenerator,
    PresentationGenerator,
    ApplicationGenerator,
    InfrastructureGenerator,
};

/// Enhanced generator that uses YAML schemas for field-aware code generation
pub struct EnhancedGenerator {
    config: Config,
}

impl EnhancedGenerator {
    /// Create a new enhanced generator
    pub fn new(config: Config) -> Result<Self> {
        config.validate()?;
        Ok(Self { config })
    }

    /// Run the enhanced code generation
    pub fn generate(&self) -> Result<GenerationResult> {
        let mut result = GenerationResult::default();

        // Validate output directory exists
        if !self.config.output_dir.exists() {
            return Err(Error::WebappNotFound(self.config.output_dir.clone()));
        }

        // Parse YAML schemas from the schema directory
        let schema_dir = self.config.schema_dir();

        // Find all model.yaml files
        let model_files = Self::find_model_files(&schema_dir)?;
        if model_files.is_empty() {
            return Err(Error::Parse("No .model.yaml files found in schema directory".to_string()));
        }

        // Parse all model schemas
        let mut all_entities = Vec::new();
        let mut all_enums = Vec::new();
        let mut seen_enum_names = std::collections::HashSet::new();

        for model_file in &model_files {
            match ModelParser::parse_file(model_file) {
                Ok(schema) => {
                    all_entities.extend(schema.models);
                    // Deduplicate enums by name to avoid duplicate exports in generated files
                    for enum_def in schema.enums {
                        if !seen_enum_names.contains(&enum_def.name) {
                            seen_enum_names.insert(enum_def.name.clone());
                            all_enums.push(enum_def);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Failed to parse {}: {}", model_file.display(), e);
                }
            }
        }

        if all_entities.is_empty() {
            return Err(Error::Parse("No entities found in model schemas".to_string()));
        }

        result.entities_found = all_entities.len();

        // Check if domain generation is requested
        let generate_domain = self.config.targets.contains(&Target::Domain) ||
                             self.config.targets.contains(&Target::All);

        // Check if presentation generation is requested
        let generate_presentation = self.config.targets.contains(&Target::Presentation) ||
                                    self.config.targets.contains(&Target::All);

        // Check if application generation is requested
        let generate_application = self.config.targets.contains(&Target::Application) ||
                                   self.config.targets.contains(&Target::All);

        // Parse hook schemas if needed for domain, presentation, or application generation
        let hook_schemas = if generate_domain || generate_presentation || generate_application {
            self.find_and_parse_hooks(&schema_dir)?
        } else {
            Vec::new()
        };

        // Generate for each entity
        for entity in &all_entities {
            // Generate enhanced form components
            self.generate_forms(entity, &all_enums, &mut result)?;

            // Generate enhanced pages
            self.generate_pages(entity, &all_enums, &mut result)?;

            // Generate enhanced schemas
            self.generate_schemas(entity, &all_enums, &mut result)?;

            // Generate hooks (with soft-delete support)
            self.generate_hooks(entity, &mut result)?;
        }

        // Generate domain layer if requested
        if generate_domain {
            self.generate_domain(&all_entities, &all_enums, &hook_schemas, &mut result)?;
        }

        // Generate presentation layer if requested
        if generate_presentation {
            self.generate_presentation(&all_entities, &all_enums, &hook_schemas, &mut result)?;
        }

        // Generate application layer if requested
        if generate_application {
            self.generate_application(&all_entities, &all_enums, &hook_schemas, &mut result)?;
        }

        // Check if infrastructure generation is requested
        let generate_infrastructure = self.config.targets.contains(&Target::Infrastructure) ||
                                      self.config.targets.contains(&Target::All);

        if generate_infrastructure {
            self.generate_infrastructure(&all_entities, &all_enums, &mut result)?;
        }

        // Check if routing generation is requested
        let generate_routing = self.config.targets.contains(&Target::Routing) ||
                               self.config.targets.contains(&Target::All);

        if generate_routing {
            self.generate_routing(&all_entities, &mut result)?;
        }

        Ok(result)
    }

    /// Find all .model.yaml files in the schema directory
    fn find_model_files(schema_dir: &Path) -> Result<Vec<PathBuf>> {
        let mut model_files = Vec::new();

        let models_dir = schema_dir.join("models");
        if models_dir.exists() {
            let entries = fs::read_dir(&models_dir)
                .map_err(|e| Error::Parse(format!("Failed to read models directory: {}", e)))?;

            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("yaml") ||
                   path.extension().and_then(|s| s.to_str()) == Some("yml") {
                    model_files.push(path);
                }
            }
        }

        model_files.sort();
        Ok(model_files)
    }

    /// Find and parse hook schemas from the hooks directory
    fn find_and_parse_hooks(&self, schema_dir: &Path) -> Result<Vec<HookSchema>> {
        let mut hooks = Vec::new();

        let hooks_dir = schema_dir.join("hooks");
        if hooks_dir.exists() {
            let entries = fs::read_dir(&hooks_dir)
                .map_err(|e| Error::Parse(format!("Failed to read hooks directory: {}", e)))?;

            for entry in entries.flatten() {
                let path = entry.path();
                let filename = path.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");

                // Match *.hook.yaml or *.hook.yml files
                if filename.ends_with(".hook.yaml") || filename.ends_with(".hook.yml") {
                    match HookParser::parse_file(&path) {
                        Ok(hook) => hooks.push(hook),
                        Err(e) => {
                            eprintln!("Warning: Failed to parse {}: {}", path.display(), e);
                        }
                    }
                }
            }
        }

        Ok(hooks)
    }

    /// Generate DDD domain layer (entity types, schemas, services, events, etc.)
    fn generate_domain(
        &self,
        entities: &[EntityDefinition],
        enums: &[EnumDefinition],
        hooks: &[HookSchema],
        result: &mut GenerationResult,
    ) -> Result<()> {
        // Clone and adjust config for domain generation
        let domain_config = self.config.clone();

        let domain_generator = DomainGenerator::new(domain_config);

        // Generate domain layer for all entities
        let domain_result = domain_generator.generate_all(entities, enums, hooks)?;

        // Merge results
        if self.config.dry_run {
            result.dry_run_files.extend(domain_result.dry_run_files);
        } else {
            result.files_generated.extend(domain_result.files_generated);
        }

        Ok(())
    }

    /// Generate presentation layer (forms, tables, pages, detail views)
    fn generate_presentation(
        &self,
        entities: &[EntityDefinition],
        enums: &[EnumDefinition],
        hooks: &[HookSchema],
        result: &mut GenerationResult,
    ) -> Result<()> {
        let presentation_config = self.config.clone();
        let presentation_generator = PresentationGenerator::new(presentation_config);

        let presentation_result = presentation_generator.generate_all(entities, enums, hooks)?;

        // Merge results
        if self.config.dry_run {
            result.dry_run_files.extend(presentation_result.dry_run_files);
        } else {
            result.files_generated.extend(presentation_result.files_generated);
        }

        Ok(())
    }

    /// Generate application layer (use cases, app services)
    fn generate_application(
        &self,
        entities: &[EntityDefinition],
        enums: &[EnumDefinition],
        hooks: &[HookSchema],
        result: &mut GenerationResult,
    ) -> Result<()> {
        let application_config = self.config.clone();
        let application_generator = ApplicationGenerator::new(application_config);

        let application_result = application_generator.generate_all(entities, enums, hooks)?;

        // Merge results
        if self.config.dry_run {
            result.dry_run_files.extend(application_result.dry_run_files);
        } else {
            result.files_generated.extend(application_result.files_generated);
        }

        Ok(())
    }

    /// Generate infrastructure layer (API clients, repository implementations)
    fn generate_infrastructure(
        &self,
        entities: &[EntityDefinition],
        enums: &[EnumDefinition],
        result: &mut GenerationResult,
    ) -> Result<()> {
        let infrastructure_config = self.config.clone();
        let infrastructure_generator = InfrastructureGenerator::new(infrastructure_config);

        let infrastructure_result = infrastructure_generator.generate_all(entities, enums)?;

        // Merge results
        if self.config.dry_run {
            result.dry_run_files.extend(infrastructure_result.dry_run_files);
        } else {
            result.files_generated.extend(infrastructure_result.files_generated);
        }

        Ok(())
    }

    /// Generate routing configuration from entity definitions
    fn generate_routing(
        &self,
        entities: &[EntityDefinition],
        result: &mut GenerationResult,
    ) -> Result<()> {
        use crate::webgen::templates::routing::RoutingTemplates;

        let output_dir = self.config.output_dir
            .join("shared/routing")
            .join(&self.config.module);

        if !self.config.dry_run {
            fs::create_dir_all(&output_dir)
                .map_err(|e| Error::Parse(format!("Failed to create routing dir: {}", e)))?;
        }

        // Generate route definitions
        let routes_content = RoutingTemplates::generate_route_definitions(entities, &self.config.module);
        let routes_path = output_dir.join("routes.ts");

        if self.config.dry_run {
            result.dry_run_files.push(routes_path.clone());
        } else {
            self.write_file(&routes_path, &routes_content)?;
            result.files_generated.push(routes_path);
        }

        // Generate route components
        let components_content = RoutingTemplates::generate_route_components(entities, &self.config.module);
        let components_path = output_dir.join("route-components.ts");

        if self.config.dry_run {
            result.dry_run_files.push(components_path.clone());
        } else {
            self.write_file(&components_path, &components_content)?;
            result.files_generated.push(components_path);
        }

        // Generate route configuration
        let config_content = RoutingTemplates::generate_route_config(entities, &self.config.module);
        let config_path = output_dir.join("route-config.tsx");

        if self.config.dry_run {
            result.dry_run_files.push(config_path.clone());
        } else {
            self.write_file(&config_path, &config_content)?;
            result.files_generated.push(config_path);
        }

        // Generate navigation menu
        let nav_content = RoutingTemplates::generate_navigation_menu(entities, &self.config.module);
        let nav_path = output_dir.join("navigation.tsx");

        if self.config.dry_run {
            result.dry_run_files.push(nav_path.clone());
        } else {
            self.write_file(&nav_path, &nav_content)?;
            result.files_generated.push(nav_path);
        }

        Ok(())
    }

    /// Generate enhanced form components
    fn generate_forms(
        &self,
        entity: &EntityDefinition,
        enums: &[EnumDefinition],
        result: &mut GenerationResult,
    ) -> Result<()> {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_snake = to_snake_case(&entity.name);
        let forms_dir = self.config.output_dir
            .join("presentation/components/forms")
            .join(&self.config.module);

        if !self.config.dry_run {
            fs::create_dir_all(&forms_dir)
                .map_err(|e| Error::Parse(format!("Failed to create forms dir: {}", e)))?;
        }

        let domain_import = self.config.domain_import_path();
        let replacer = TemplateReplacer::new(
            entity_pascal.clone(),
            entity_snake.clone(),
            self.config.module.clone(),
            domain_import,
        );

        // Generate create form with field-aware inputs
        let form_fields = FormTemplates::generate_form_fields(entity, enums, true);
        let create_template = Self::enhanced_create_form_template(&entity_pascal, &entity_snake);
        let create_content = replacer.replace(&create_template);
        let create_content = create_content.replace("{{FORM_FIELDS}}", &form_fields);

        let create_path = forms_dir.join(format!("{}CreateForm.tsx", entity_pascal));

        if self.config.dry_run {
            result.dry_run_files.push(create_path);
        } else {
            self.write_file(&create_path, &create_content)?;
            result.files_generated.push(create_path);
        }

        // Generate edit form with field-aware inputs
        let form_fields = FormTemplates::generate_form_fields(entity, enums, false);
        let edit_template = Self::enhanced_edit_form_template(&entity_pascal, &entity_snake);
        let edit_content = replacer.replace(&edit_template);
        let edit_content = edit_content.replace("{{FORM_FIELDS}}", &form_fields);

        let edit_path = forms_dir.join(format!("{}EditForm.tsx", entity_pascal));

        if self.config.dry_run {
            result.dry_run_files.push(edit_path);
        } else {
            self.write_file(&edit_path, &edit_content)?;
            result.files_generated.push(edit_path);
        }

        Ok(())
    }

    /// Generate enhanced page components
    fn generate_pages(
        &self,
        entity: &EntityDefinition,
        _enums: &[EnumDefinition],
        result: &mut GenerationResult,
    ) -> Result<()> {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_snake = to_snake_case(&entity.name);
        let has_soft_delete = entity.has_soft_delete();
        let pages_dir = self.config.output_dir
            .join("presentation/pages")
            .join(&self.config.module)
            .join(&entity_snake);

        if !self.config.dry_run {
            fs::create_dir_all(&pages_dir)
                .map_err(|e| Error::Parse(format!("Failed to create pages dir: {}", e)))?;
        }

        let domain_import = self.config.domain_import_path();
        let replacer = TemplateReplacer::new(
            entity_pascal.clone(),
            entity_snake.clone(),
            self.config.module.clone(),
            domain_import,
        );

        // Generate list page with data table
        let table_columns = TableTemplates::generate_table_columns(entity, &self.config.module, &entity_snake);
        let table_rows = TableTemplates::generate_table_rows(entity);
        let list_template = Self::enhanced_list_page_template(&entity_pascal, &entity_snake, has_soft_delete);
        let list_content = replacer.replace(&list_template);
        let list_content = list_content.replace("{{TABLE_COLUMNS}}", &table_columns);
        let list_content = list_content.replace("{{TABLE_ROWS}}", &table_rows);

        let list_path = pages_dir.join(format!("{}ListPage.tsx", entity_pascal));

        if self.config.dry_run {
            result.dry_run_files.push(list_path.clone());
        } else {
            self.write_file(&list_path, &list_content)?;
            result.files_generated.push(list_path);
        }

        // Generate detail page with field display
        let detail_fields = Self::generate_detail_fields(entity);
        let detail_template = Self::enhanced_detail_page_template(&entity_pascal, &entity_snake, has_soft_delete);
        let detail_content = replacer.replace(&detail_template);
        let detail_content = detail_content.replace("{{DETAIL_FIELDS}}", &detail_fields);

        let detail_path = pages_dir.join(format!("{}DetailPage.tsx", entity_pascal));

        if self.config.dry_run {
            result.dry_run_files.push(detail_path.clone());
        } else {
            self.write_file(&detail_path, &detail_content)?;
            result.files_generated.push(detail_path);
        }

        // Generate create page
        let create_template = Self::enhanced_create_page_template(&entity_pascal, &entity_snake);
        let create_content = replacer.replace(&create_template);
        let create_path = pages_dir.join(format!("{}CreatePage.tsx", entity_pascal));

        if self.config.dry_run {
            result.dry_run_files.push(create_path.clone());
        } else {
            self.write_file(&create_path, &create_content)?;
            result.files_generated.push(create_path);
        }

        // Generate edit page
        let edit_template = Self::enhanced_edit_page_template(&entity_pascal, &entity_snake);
        let edit_content = replacer.replace(&edit_template);
        let edit_path = pages_dir.join(format!("{}EditPage.tsx", entity_pascal));

        if self.config.dry_run {
            result.dry_run_files.push(edit_path.clone());
        } else {
            self.write_file(&edit_path, &edit_content)?;
            result.files_generated.push(edit_path);
        }

        // Generate trash pages if soft delete is enabled
        if has_soft_delete {
            // Generate trash list page
            let trash_template = Self::enhanced_trash_page_template(&entity_pascal, &entity_snake);
            let trash_content = replacer.replace(&trash_template);
            let trash_path = pages_dir.join(format!("{}TrashPage.tsx", entity_pascal));

            if self.config.dry_run {
                result.dry_run_files.push(trash_path.clone());
            } else {
                self.write_file(&trash_path, &trash_content)?;
                result.files_generated.push(trash_path);
            }

            // Generate trash detail page
            let trash_detail_template = Self::enhanced_trash_detail_page_template(&entity_pascal, &entity_snake);
            let trash_detail_content = replacer.replace(&trash_detail_template);
            let trash_detail_content = trash_detail_content.replace("{{DETAIL_FIELDS}}", &detail_fields);
            let trash_detail_path = pages_dir.join(format!("{}TrashDetailPage.tsx", entity_pascal));

            if self.config.dry_run {
                result.dry_run_files.push(trash_detail_path.clone());
            } else {
                self.write_file(&trash_detail_path, &trash_detail_content)?;
                result.files_generated.push(trash_detail_path);
            }
        }

        // Generate index.ts for importing pages from directory
        let trash_exports = if has_soft_delete {
            format!(
r#"export {{ {entity_pascal}TrashPage }} from './{entity_pascal}TrashPage';
export {{ {entity_pascal}TrashDetailPage }} from './{entity_pascal}TrashDetailPage';
"#,
                entity_pascal = entity_pascal
            )
        } else {
            String::new()
        };

        let index_content = format!(
r#"// Re-exports all page components for {entity_pascal}
export {{ {entity_pascal}ListPage }} from './{entity_pascal}ListPage';
export {{ {entity_pascal}DetailPage }} from './{entity_pascal}DetailPage';
export {{ {entity_pascal}CreatePage }} from './{entity_pascal}CreatePage';
export {{ {entity_pascal}EditPage }} from './{entity_pascal}EditPage';
{trash_exports}"#,
            entity_pascal = entity_pascal,
            trash_exports = trash_exports
        );
        let index_path = pages_dir.join("index.ts");

        if self.config.dry_run {
            result.dry_run_files.push(index_path.clone());
        } else {
            self.write_file(&index_path, &index_content)?;
            result.files_generated.push(index_path);
        }

        Ok(())
    }

    /// Generate enhanced Zod schemas
    fn generate_schemas(
        &self,
        entity: &EntityDefinition,
        enums: &[EnumDefinition],
        result: &mut GenerationResult,
    ) -> Result<()> {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_snake = to_snake_case(&entity.name);
        let validators_dir = self.config.output_dir
            .join("application/validators")
            .join(&self.config.module);

        if !self.config.dry_run {
            fs::create_dir_all(&validators_dir)
                .map_err(|e| Error::Parse(format!("Failed to create validators dir: {}", e)))?;
        }

        // Generate enum definitions
        let mut enum_defs = String::new();
        for enum_def in enums {
            if entity.fields.iter().any(|f| {
                matches!(&f.type_name, crate::webgen::ast::entity::FieldType::Enum(t) if t == &enum_def.name)
            }) {
                enum_defs.push_str(&Self::generate_enum_def(enum_def));
            }
        }

        // Generate Zod schema
        let zod_fields = FormTemplates::generate_zod_schema(entity, enums, false);
        let create_zod_fields = FormTemplates::generate_zod_schema(entity, enums, true);
        let schema_content = Self::enhanced_schema_template(
            &entity_pascal,
            &entity_snake,
            &enum_defs,
            &zod_fields,
            &create_zod_fields,
        );

        let schema_path = validators_dir.join(format!("{}.schema.ts", entity_snake));

        if self.config.dry_run {
            result.dry_run_files.push(schema_path);
        } else {
            self.write_file(&schema_path, &schema_content)?;
            result.files_generated.push(schema_path);
        }

        Ok(())
    }

    /// Generate React Query hooks (with soft-delete support for entities that have it)
    fn generate_hooks(
        &self,
        entity: &EntityDefinition,
        result: &mut GenerationResult,
    ) -> Result<()> {
        use crate::webgen::templates::base::{HookTemplate, TemplateReplacer};

        let entity_pascal = to_pascal_case(&entity.name);
        let entity_snake = to_snake_case(&entity.name);
        let has_soft_delete = entity.has_soft_delete();

        let hooks_dir = self.config.output_dir
            .join("application/hooks")
            .join(&self.config.module);

        if !self.config.dry_run {
            fs::create_dir_all(&hooks_dir)
                .map_err(|e| Error::Parse(format!("Failed to create hooks dir: {}", e)))?;
        }

        let domain_import = self.config.domain_import_path();
        let replacer = TemplateReplacer::new(
            entity_pascal.clone(),
            entity_snake.clone(),
            self.config.module.clone(),
            domain_import,
        );

        // Generate query hook file
        let mut query_content = replacer.replace(HookTemplate::query());
        if has_soft_delete {
            // Append soft-delete query hooks (trash list)
            query_content.push_str(&replacer.replace(HookTemplate::soft_delete_queries()));
            // Re-export soft-delete mutation hooks from query file for convenience
            query_content.push_str(&format!(
                "\n// Re-export soft-delete mutations for convenience\nexport {{\n  use{}SoftDelete,\n  use{}Restore,\n  use{}PermanentDelete,\n}} from './use{}Mutation';\n",
                entity_pascal, entity_pascal, entity_pascal, entity_pascal
            ));
        }
        let query_path = hooks_dir.join(format!("use{}.ts", entity_pascal));

        if self.config.dry_run {
            result.dry_run_files.push(query_path);
        } else {
            self.write_file(&query_path, &query_content)?;
            result.files_generated.push(query_path);
        }

        // Generate mutation hook file
        let mut mutation_content = replacer.replace(HookTemplate::mutation());
        if has_soft_delete {
            // Append soft-delete mutation hooks
            mutation_content.push_str(&replacer.replace(HookTemplate::soft_delete_mutations()));
        }
        let mutation_path = hooks_dir.join(format!("use{}Mutation.ts", entity_pascal));

        if self.config.dry_run {
            result.dry_run_files.push(mutation_path);
        } else {
            self.write_file(&mutation_path, &mutation_content)?;
            result.files_generated.push(mutation_path);
        }

        Ok(())
    }

    /// Generate enum definition for Zod
    fn generate_enum_def(enum_def: &EnumDefinition) -> String {
        let variants: Vec<String> = enum_def.variants.iter()
            .map(|v| format!("'{}'", v.name))
            .collect();

        format!(r#"/** {} enum values */
export const {}Enum = [{}] as const;
export type {}EnumValue = typeof {}Enum[number];
"#,
            enum_def.name,
            enum_def.name,
            variants.join(", "),
            enum_def.name,
            enum_def.name
        )
    }

    /// Generate detail field display components
    fn generate_detail_fields(entity: &EntityDefinition) -> String {
        let mut fields = String::new();

        for field in &entity.fields {
            // Skip sensitive fields
            if field.name.contains("password") || field.name.contains("hash") || field.name.contains("token") {
                continue;
            }

            let label = FormTemplates::field_label(field);
            let field_display = Self::detail_field_display(field);

            // Use string concatenation to avoid format! escaping issues
            fields.push_str(r#"        <DetailField label=""#);
            fields.push_str(&label);
            fields.push_str(r#"" value={"#);
            fields.push_str(&field_display);
            fields.push_str(r#"} />
"#);
        }

        fields
    }

    /// Generate field display for detail page
    fn detail_field_display(field: &FieldDefinition) -> String {
        match &field.type_name {
            crate::webgen::ast::entity::FieldType::Bool => {
                format!(r#"{}.{} ? 'Yes' : 'No'"#, "entity", field.name)
            }
            crate::webgen::ast::entity::FieldType::DateTime => {
                format!(r#"{}.{} ? new Date({}.{}).toLocaleString() : '-'"#, "entity", field.name, "entity", field.name)
            }
            crate::webgen::ast::entity::FieldType::Date => {
                format!(r#"{}.{} ? new Date({}.{}).toLocaleDateString() : '-'"#, "entity", field.name, "entity", field.name)
            }
            _ => {
                format!("entity.{}", field.name)
            }
        }
    }

    /// Write file to disk
    fn write_file(&self, path: &PathBuf, content: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| Error::write_error(path.clone(), e))?;
        }

        if path.exists() && !self.config.force {
            // TODO: Implement custom section preservation
        }

        fs::write(path, content)
            .map_err(|e| Error::write_error(path.clone(), e))?;

        Ok(())
    }

    // Template methods

    fn enhanced_create_form_template(entity_pascal: &str, entity_snake: &str) -> String {
        format!(r#"/**
 * {entity_pascal} Create Form Component
 *
 * Generated form component for creating a {entity_pascal}.
 */

import {{ useForm, Controller }} from 'react-hook-form';
import {{ zodResolver }} from '@hookform/resolvers/zod';
import {{
  Box, Button, Stack, TextField, FormControlLabel, Switch,
  FormControl, InputLabel, Select, MenuItem, FormHelperText,
}} from '@/components/ui';
import {{
  create{entity_pascal}Schema,
  type Create{entity_pascal}Input,
}} from '@webapp/application/validators/{{MODULE_NAME}}/{entity_snake}.schema';
import {{ useCreate{entity_pascal} }} from '@webapp/application/hooks/{{MODULE_NAME}}/use{entity_pascal}Mutation';

export interface {entity_pascal}CreateFormProps {{
  onSuccess?: () => void;
  onCancel?: () => void;
}}

export function {entity_pascal}CreateForm({{ onSuccess, onCancel }}: {entity_pascal}CreateFormProps) {{
  const {{ mutate: create{entity_pascal}, isPending }} = useCreate{entity_pascal}();

  const {{
    register,
    control,
    handleSubmit,
    formState: {{ errors }},
  }} = useForm<Create{entity_pascal}Input>({{
    resolver: zodResolver(create{entity_pascal}Schema),
    defaultValues: {{}},
  }});

  const onSubmit = (data: Create{entity_pascal}Input) => {{
    create{entity_pascal}(data);
    if (onSuccess) onSuccess();
  }};

  return (
    <Box component="form" onSubmit={{handleSubmit(onSubmit)}}>
      <Stack spacing={{3}}>
{{FORM_FIELDS}}
        <Stack direction="row" spacing={{2}} justifyContent="flex-end">
          {{onCancel && (
            <Button onClick={{onCancel}} disabled={{isPending}}>
              Cancel
            </Button>
          )}}
          <Button
            type="submit"
            variant="contained"
            disabled={{isPending}}
          >
            {{isPending ? 'Creating...' : 'Create {entity_pascal}'}}
          </Button>
        </Stack>
      </Stack>
    </Box>
  );
}}
"#)
    }

    fn enhanced_edit_form_template(entity_pascal: &str, entity_snake: &str) -> String {
        format!(r#"/**
 * {entity_pascal} Edit Form Component
 *
 * Generated form component for editing a {entity_pascal}.
 */

import {{ useForm, Controller }} from 'react-hook-form';
import {{ zodResolver }} from '@hookform/resolvers/zod';
import {{
  Box, Button, Stack, TextField, FormControlLabel, Switch,
  FormControl, InputLabel, Select, MenuItem, FormHelperText,
}} from '@/components/ui';
import {{
  update{entity_pascal}Schema,
  type Update{entity_pascal}Input,
}} from '@webapp/application/validators/{{MODULE_NAME}}/{entity_snake}.schema';
import {{ useUpdate{entity_pascal} }} from '@webapp/application/hooks/{{MODULE_NAME}}/use{entity_pascal}Mutation';
import type {{ {entity_pascal} }} from '{{DOMAIN_IMPORT}}';
import {{ useEffect }} from 'react';

export interface {entity_pascal}EditFormProps {{
  {entity_snake}: {entity_pascal};
  onSuccess?: () => void;
  onCancel?: () => void;
}}

export function {entity_pascal}EditForm({{ {entity_snake}, onSuccess, onCancel }}: {entity_pascal}EditFormProps) {{
  const {{ mutate: update{entity_pascal}, isPending }} = useUpdate{entity_pascal}();

  const {{
    register,
    control,
    handleSubmit,
    reset,
    formState: {{ errors }},
  }} = useForm<Update{entity_pascal}Input>({{
    resolver: zodResolver(update{entity_pascal}Schema),
    defaultValues: {{
      id: {entity_snake}.id,
    }},
  }});

  useEffect(() => {{
    reset({{ id: {entity_snake}.id }});
  }}, [{entity_snake}, reset]);

  const onSubmit = (data: Update{entity_pascal}Input) => {{
    update{entity_pascal}({{ id: {entity_snake}.id, input: data }});
    if (onSuccess) onSuccess();
  }};

  return (
    <Box component="form" onSubmit={{handleSubmit(onSubmit)}}>
      <Stack spacing={{3}}>
{{FORM_FIELDS}}
        <Stack direction="row" spacing={{2}} justifyContent="flex-end">
          {{onCancel && (
            <Button onClick={{onCancel}} disabled={{isPending}}>
              Cancel
            </Button>
          )}}
          <Button
            type="submit"
            variant="contained"
            disabled={{isPending}}
          >
            {{isPending ? 'Saving...' : 'Save Changes'}}
          </Button>
        </Stack>
      </Stack>
    </Box>
  );
}}
"#)
    }

    fn enhanced_list_page_template(entity_pascal: &str, entity_snake: &str, has_soft_delete: bool) -> String {
        let trash_menu_item = if has_soft_delete {
            format!(r#"
            moreMenuItems={{[
              {{
                label: 'Trash',
                icon: <DeleteSweep />,
                onClick: () => navigate('/{{{{MODULE_NAME}}}}/{entity_snake}/trash'),
              }},
            ]}}"#)
        } else {
            String::new()
        };

        let is_soft_delete_prop = if has_soft_delete {
            "\n        isSoftDelete"
        } else {
            ""
        };

        let soft_delete_icon_import = if has_soft_delete {
            ", DeleteSweep"
        } else {
            ""
        };

        format!(r#"/**
 * {entity_pascal} List Page
 *
 * Generated list page for {entity_pascal} entities.
 * Uses reusable useListOperations hook and ListPageActions component.
 */

import {{ useState, useCallback }} from 'react';
import {{
  Box,
  Container,
  Stack,
  Typography,
  Chip,
  Alert,
  CircularProgress,
}} from '@/components/ui';
import {{ Plus{soft_delete_icon_import} }} from '@/components/ui';
import {{ DataGrid }} from '@/components/ui';
import {{ use{entity_pascal}List }} from '@webapp/application/hooks/{{{{MODULE_NAME}}}}/use{entity_pascal}';
import {{ useNavigate }} from 'react-router-dom';
import {{ useDelete{entity_pascal} }} from '@webapp/application/hooks/{{{{MODULE_NAME}}}}/use{entity_pascal}Mutation';
import {{ useListOperations }} from '@webapp/application/hooks/common';
import {{ ListPageActions, DeleteDialogs }} from '@webapp/presentation/components/list';
import type {{ {entity_pascal} }} from '@webapp/domain/{{{{MODULE_NAME}}}}/entity/{entity_pascal}.schema';

export function {entity_pascal}ListPage() {{
  const navigate = useNavigate();
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState(20);

  const {{ data, isLoading, error, refetch }} = use{entity_pascal}List();
  const deleteMutation = useDelete{entity_pascal}();

  // Use reusable list operations hook
  const listOps = useListOperations<{entity_pascal}>({{
    deleteMutation,
    onRefetch: refetch,
  }});

  const handleRefresh = useCallback(async () => {{
    await refetch();
  }}, [refetch]);

  if (error) {{
    return (
      <Container maxWidth="xl">
        <Alert severity="error">Error: {{error.message}}</Alert>
      </Container>
    );
  }}

  const columns = [
{{{{TABLE_COLUMNS}}}}
  ];

  return (
    <Container maxWidth="xl">
      <Stack spacing={{3}}>
        <Stack direction="row" justifyContent="space-between" alignItems="center">
          <Typography variant="h4">{entity_pascal} List</Typography>
          <ListPageActions
            selectedCount={{listOps.selectedCount}}
            hasSelectedRows={{listOps.hasSelectedRows}}
            onBulkDelete={{listOps.handleBulkDelete}}
            onRefresh={{handleRefresh}}
            onCreate={{() => navigate('/{{{{MODULE_NAME}}}}/{entity_snake}/create')}}
            isRefreshing={{isLoading}}
            isDeleting={{deleteMutation.isPending}}
            addButtonLabel="Add {entity_pascal}"{trash_menu_item}
          />
        </Stack>

        {{isLoading ? (
          <Box display="flex" justifyContent="center" py={{12}}>
            <CircularProgress />
          </Box>
        ) : (
          <DataGrid
            rows={{data?.data || []}}
            columns={{columns}}
            pageSizeOptions={{[5, 10, 20, 50]}}
            pagination={{{{ page, pageSize }}}}
            paginationMode="server"
            rowCount={{data?.total || 0}}
            onPaginationModelChange={{(model) => {{
              setPage(model.page);
              setPageSize(model.pageSize);
            }}}}
            getRowId={{(row) => row.id}}
            loading={{isLoading}}
            disableRowSelectionOnClick
          />
        )}}
      </Stack>

      {{/* Delete Dialogs */}}
      <DeleteDialogs<{entity_pascal}>
        entityName="{entity_snake}"
        deleteConfirmOpen={{listOps.deleteConfirmOpen}}
        onCloseDeleteDialog={{listOps.handleCloseDeleteDialog}}
        onConfirmDelete={{listOps.handleConfirmDelete}}
        deleteError={{listOps.deleteError}}
        isBulkOperation={{listOps.isBulkOperation}}
        itemToDelete={{listOps.itemToDelete}}
        itemsToDelete={{listOps.itemsToDelete}}
        isDeleting={{deleteMutation.isPending}}
        getItemDisplayName={{(item) => item.id}}
        bulkDeleteProgress={{listOps.bulkDeleteProgress}}{is_soft_delete_prop}
      />
    </Container>
  );
}}
"#)
    }

    fn enhanced_detail_page_template(entity_pascal: &str, entity_snake: &str, has_soft_delete: bool) -> String {
        let is_soft_delete_prop = if has_soft_delete {
            "\n          isSoftDelete"
        } else {
            ""
        };

        format!(r#"/**
 * {entity_pascal} Detail Page
 *
 * Generated detail page for viewing a single {entity_pascal}.
 * Uses reusable useDetailOperations hook, DangerZone and DeleteDialog components.
 */

import {{
  Box,
  Container,
  Stack,
  Typography,
  Paper,
  Button,
  Alert,
  CircularProgress,
}} from '@/components/ui';
import {{ useParams, useNavigate }} from 'react-router-dom';
import {{ use{entity_pascal} }} from '@webapp/application/hooks/{{{{MODULE_NAME}}}}/use{entity_pascal}';
import {{ useDelete{entity_pascal} }} from '@webapp/application/hooks/{{{{MODULE_NAME}}}}/use{entity_pascal}Mutation';
import {{ ArrowBack }} from '@/components/ui';
import {{ IconButton }} from '@/components/ui';
import {{ useDetailOperations }} from '@webapp/application/hooks/common';
import {{ DangerZone, DeleteDialog }} from '@webapp/presentation/components/detail';

interface DetailFieldProps {{
  label: string;
  value: React.ReactNode;
}}

function DetailField({{ label, value }}: DetailFieldProps) {{
  return (
    <Stack spacing={{1}}>
      <Typography variant="caption" color="text.secondary">
        {{label}}
      </Typography>
      <Typography variant="body1">
        {{value ?? '-'}}
      </Typography>
    </Stack>
  );
}}

export function {entity_pascal}DetailPage() {{
  const {{ id }} = useParams<{{ id: string }}>();
  const navigate = useNavigate();

  const {{ data: entity, isLoading, error, refetch }} = use{entity_pascal}(id || '');
  const deleteMutation = useDelete{entity_pascal}();

  // Use reusable detail operations hook
  const detailOps = useDetailOperations({{
    entityId: id ?? '',
    deleteMutation,
    onDeleteSuccess: () => navigate('/{{{{MODULE_NAME}}}}/{entity_snake}'),
    onRefetch: refetch,
  }});

  if (isLoading) {{
    return (
      <Container maxWidth="xl">
        <Box display="flex" justifyContent="center" py={{12}}>
          <CircularProgress />
        </Box>
      </Container>
    );
  }}

  if (error) {{
    return (
      <Container maxWidth="xl">
        <Alert severity="error">Error: {{error.message}}</Alert>
      </Container>
    );
  }}

  if (!entity) {{
    return (
      <Container maxWidth="xl">
        <Alert severity="warning">{entity_pascal} not found</Alert>
      </Container>
    );
  }}

  return (
    <Container maxWidth="xl">
      <Stack spacing={{3}}>
        <Stack direction="row" justifyContent="space-between" alignItems="center">
          <Stack direction="row" alignItems="center" spacing={{2}}>
            <IconButton onClick={{() => navigate('/{{{{MODULE_NAME}}}}/{entity_snake}')}}>
              <ArrowBack />
            </IconButton>
            <Typography variant="h4">{entity_pascal} Details</Typography>
          </Stack>
          <Stack direction="row" spacing={{1}}>
            <Button
              variant="outlined"
              onClick={{() => navigate('/{{{{MODULE_NAME}}}}/{entity_snake}')}}
            >
              Back to List
            </Button>
            <Button
              variant="contained"
              onClick={{() => navigate(`/{{{{MODULE_NAME}}}}/{entity_snake}/${{entity.id}}/edit`)}}
            >
              Edit
            </Button>
          </Stack>
        </Stack>

        <Paper>
          <Stack spacing={{3}} sx={{{{ p: 3 }}}}>
{{{{DETAIL_FIELDS}}}}
          </Stack>
        </Paper>

        {{/* Danger Zone */}}
        <DangerZone
          entityName="{entity_snake}"
          onDelete={{detailOps.handleDelete}}{is_soft_delete_prop}
        />
      </Stack>

      {{/* Delete Dialog */}}
      <DeleteDialog
        open={{detailOps.deleteConfirmOpen}}
        onClose={{detailOps.handleCloseDeleteDialog}}
        onConfirm={{detailOps.handleConfirmDelete}}
        entityName="{entity_snake}"
        itemDisplayName={{entity.id}}
        error={{detailOps.deleteError}}
        isDeleting={{detailOps.isDeleting}}{is_soft_delete_prop}
      />
    </Container>
  );
}}
"#)
    }

    fn enhanced_create_page_template(entity_pascal: &str, entity_snake: &str) -> String {
        format!(r#"/**
 * {entity_pascal} Create Page
 *
 * Generated create page for creating a new {entity_pascal}.
 */

import {{ Container, Stack, Typography }} from '@/components/ui';
import {{ {entity_pascal}CreateForm }} from '@webapp/presentation/components/forms/{{MODULE_NAME}}/{entity_pascal}CreateForm';
import {{ useNavigate }} from 'react-router-dom';

export function {entity_pascal}CreatePage() {{
  const navigate = useNavigate();

  const handleSuccess = () => {{
    navigate(`/{{MODULE_NAME}}/{entity_snake}`);
  }};

  const handleCancel = () => {{
    navigate(`/{{MODULE_NAME}}/{entity_snake}`);
  }};

  return (
    <Container maxWidth="md">
      <Stack spacing={{3 }}>
        <Typography variant="h4">Create {entity_pascal}</Typography>

        <{entity_pascal}CreateForm
          onSuccess={{handleSuccess}}
          onCancel={{handleCancel}}
        />

      </Stack>
    </Container>
  );
}}
"#)
    }

    fn enhanced_edit_page_template(entity_pascal: &str, entity_snake: &str) -> String {
        format!(r#"/**
 * {entity_pascal} Edit Page
 *
 * Generated edit page for editing an existing {entity_pascal}.
 */

import {{ Container, Stack, Typography }} from '@/components/ui';
import {{ {entity_pascal}EditForm }} from '@webapp/presentation/components/forms/{{MODULE_NAME}}/{entity_pascal}EditForm';
import {{ useNavigate, useParams }} from 'react-router-dom';
import {{ use{entity_pascal} }} from '@webapp/application/hooks/{{MODULE_NAME}}/use{entity_pascal}';

export function {entity_pascal}EditPage() {{
  const {{ id }} = useParams<{{ id: string }}>();
  const navigate = useNavigate();

  const {{ data: {entity_snake}, isLoading }} = use{entity_pascal}(id || '');

  const handleSuccess = () => {{
    navigate(`/{{MODULE_NAME}}/{entity_snake}`);
  }};

  const handleCancel = () => {{
    navigate(`/{{MODULE_NAME}}/{entity_snake}`);
  }};

  if (isLoading) {{
    return <div>Loading...</div>;
  }}

  if (!{entity_snake}) {{
    return <div>{entity_pascal} not found</div>;
  }}

  return (
    <Container maxWidth="md">
      <Stack spacing={{3 }}>
        <Typography variant="h4">Edit {entity_pascal}</Typography>

        <{entity_pascal}EditForm
          {entity_snake}={{{entity_snake}}}
          onSuccess={{handleSuccess}}
          onCancel={{handleCancel}}
        />

      </Stack>
    </Container>
  );
}}
"#)
    }

    /// Generate trash page template for soft-delete entities
    fn enhanced_trash_page_template(entity_pascal: &str, entity_snake: &str) -> String {
        format!(r#"/**
 * {entity_pascal} Trash Page
 *
 * Generated trash page for soft-deleted {entity_pascal} entities.
 * Uses reusable useTrashOperations hook, TrashPageActions and TrashDialogs components.
 */

import {{ useState, useCallback }} from 'react';
import {{
  Box,
  Container,
  Stack,
  Typography,
  Alert,
  CircularProgress,
}} from '@/components/ui';
import {{ DataGrid }} from '@/components/ui';
import {{ use{entity_pascal}TrashList }} from '@webapp/application/hooks/{{{{MODULE_NAME}}}}/use{entity_pascal}';
import {{ useNavigate }} from 'react-router-dom';
import {{
  use{entity_pascal}Restore,
  use{entity_pascal}PermanentDelete,
}} from '@webapp/application/hooks/{{{{MODULE_NAME}}}}/use{entity_pascal}Mutation';
import {{ useTrashOperations }} from '@webapp/application/hooks/common';
import {{ TrashPageActions, TrashDialogs }} from '@webapp/presentation/components/trash';
import type {{ {entity_pascal} }} from '@webapp/domain/{{{{MODULE_NAME}}}}/entity/{entity_pascal}.schema';

export function {entity_pascal}TrashPage() {{
  const navigate = useNavigate();
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState(20);
  const [restoreSuccess, setRestoreSuccess] = useState<string | null>(null);

  const {{ data, isLoading, error, refetch }} = use{entity_pascal}TrashList();
  const restoreMutation = use{entity_pascal}Restore();
  const permanentDeleteMutation = use{entity_pascal}PermanentDelete();

  const {entity_snake}s = data?.data ?? [];
  const total = data?.total ?? 0;

  // Use reusable trash operations hook
  const trashOps = useTrashOperations<{entity_pascal}>({{
    restoreMutation,
    permanentDeleteMutation,
    allItems: {entity_snake}s,
    onRestoreSuccess: (count) => {{
      setRestoreSuccess(`${{count}} {entity_snake}(s) restored successfully`);
      setTimeout(() => setRestoreSuccess(null), 3000);
    }},
  }});

  const handleRefresh = useCallback(async () => {{
    await refetch();
  }}, [refetch]);

  if (error) {{
    return (
      <Container maxWidth="xl">
        <Alert severity="error">Error loading trash: {{error.message}}</Alert>
      </Container>
    );
  }}

  const columns = [
    {{ field: 'id', headerName: 'ID', flex: 1 }},
    // Add more columns as needed
  ];

  return (
    <Container maxWidth="xl">
      <Stack spacing={{3}}>
        <Stack direction="row" justifyContent="space-between" alignItems="center">
          <Typography variant="h4">{entity_pascal} Trash</Typography>
          <TrashPageActions
            selectedCount={{trashOps.selectedCount}}
            hasSelectedRows={{trashOps.hasSelectedRows}}
            totalItems={{total}}
            onBulkRestore={{trashOps.handleBulkRestore}}
            onBulkPermanentDelete={{trashOps.handleBulkPermanentDelete}}
            onEmptyTrash={{trashOps.handleEmptyTrash}}
            onRefresh={{handleRefresh}}
            isRestorePending={{restoreMutation.isPending}}
            isDeletePending={{permanentDeleteMutation.isPending}}
            isRefreshing={{isLoading}}
            isEmptyingTrash={{trashOps.isEmptyingTrash}}
          />
        </Stack>

        {{restoreSuccess && (
          <Alert severity="success">{{restoreSuccess}}</Alert>
        )}}

        <Alert severity="info">
          Items in trash can be restored or permanently deleted. Permanent deletion cannot be undone.
        </Alert>

        {{isLoading ? (
          <Box display="flex" justifyContent="center" py={{12}}>
            <CircularProgress />
          </Box>
        ) : (
          <DataGrid
            rows={{{entity_snake}s}}
            columns={{columns}}
            pageSizeOptions={{[5, 10, 20, 50]}}
            pagination={{{{ page, pageSize }}}}
            paginationMode="server"
            rowCount={{total}}
            onPaginationModelChange={{(model) => {{
              setPage(model.page);
              setPageSize(model.pageSize);
            }}}}
            getRowId={{(row) => row.id}}
            loading={{isLoading}}
            checkboxSelection
            rowSelectionModel={{trashOps.selectedRowIds}}
            onRowSelectionModelChange={{(newSelection) => {{
              trashOps.setSelectedRowIds(newSelection as string[]);
            }}}}
          />
        )}}
      </Stack>

      {{/* Trash Dialogs */}}
      <TrashDialogs<{entity_pascal}>
        entityName="{entity_snake}"
        permanentDeleteConfirmOpen={{trashOps.permanentDeleteConfirmOpen}}
        onClosePermanentDeleteDialog={{trashOps.handleClosePermanentDeleteDialog}}
        onConfirmPermanentDelete={{trashOps.handleConfirmPermanentDelete}}
        permanentDeleteError={{trashOps.permanentDeleteError}}
        isBulkOperation={{trashOps.isBulkOperation}}
        itemToDelete={{trashOps.itemToDelete}}
        itemsToDelete={{trashOps.itemsToDelete}}
        isDeletePending={{permanentDeleteMutation.isPending}}
        getItemDisplayName={{(item) => item.id}}
        emptyTrashConfirmOpen={{trashOps.emptyTrashConfirmOpen}}
        onCloseEmptyTrashDialog={{trashOps.handleCloseEmptyTrashDialog}}
        onConfirmEmptyTrash={{trashOps.handleConfirmEmptyTrash}}
        emptyTrashError={{trashOps.emptyTrashError}}
        isEmptyingTrash={{trashOps.isEmptyingTrash}}
        totalItems={{total}}
      />
    </Container>
  );
}}
"#)
    }

    /// Generate trash detail page template for soft-delete entities
    fn enhanced_trash_detail_page_template(entity_pascal: &str, entity_snake: &str) -> String {
        format!(r#"/**
 * {entity_pascal} Trash Detail Page
 *
 * Generated trash detail page for viewing a soft-deleted {entity_pascal}.
 * Uses DangerZone and DeleteDialog components.
 */

import {{ useState }} from 'react';
import {{
  Box,
  Container,
  Stack,
  Typography,
  Paper,
  Button,
  Alert,
  CircularProgress,
}} from '@/components/ui';
import {{ useParams, useNavigate }} from 'react-router-dom';
import {{ use{entity_pascal} }} from '@webapp/application/hooks/{{{{MODULE_NAME}}}}/use{entity_pascal}';
import {{
  use{entity_pascal}Restore,
  use{entity_pascal}PermanentDelete,
}} from '@webapp/application/hooks/{{{{MODULE_NAME}}}}/use{entity_pascal}Mutation';
import {{ ArrowBack, RestoreFromTrash }} from '@/components/ui';
import {{ IconButton }} from '@/components/ui';
import {{ DangerZone, DeleteDialog }} from '@webapp/presentation/components/detail';

interface DetailFieldProps {{
  label: string;
  value: React.ReactNode;
}}

function DetailField({{ label, value }}: DetailFieldProps) {{
  return (
    <Stack spacing={{1}}>
      <Typography variant="caption" color="text.secondary">
        {{label}}
      </Typography>
      <Typography variant="body1">
        {{value ?? '-'}}
      </Typography>
    </Stack>
  );
}}

export function {entity_pascal}TrashDetailPage() {{
  const {{ id }} = useParams<{{ id: string }}>();
  const navigate = useNavigate();

  const {{ data: entity, isLoading, error }} = use{entity_pascal}(id || '');
  const restoreMutation = use{entity_pascal}Restore();
  const permanentDeleteMutation = use{entity_pascal}PermanentDelete();
  const [restoreDialogOpen, setRestoreDialogOpen] = useState(false);
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  const handleRestoreConfirm = async () => {{
    if (!entity) return;
    setActionError(null);
    try {{
      await restoreMutation.mutateAsync(entity.id);
      setRestoreDialogOpen(false);
      navigate('/{{{{MODULE_NAME}}}}/{entity_snake}');
    }} catch (err) {{
      setActionError(err instanceof Error ? err.message : 'Failed to restore');
    }}
  }};

  const handlePermanentDeleteConfirm = async () => {{
    if (!entity) return;
    setActionError(null);
    try {{
      await permanentDeleteMutation.mutateAsync(entity.id);
      setDeleteDialogOpen(false);
      navigate('/{{{{MODULE_NAME}}}}/{entity_snake}/trash');
    }} catch (err) {{
      setActionError(err instanceof Error ? err.message : 'Failed to permanently delete');
    }}
  }};

  if (isLoading) {{
    return (
      <Container maxWidth="xl">
        <Box display="flex" justifyContent="center" py={{12}}>
          <CircularProgress />
        </Box>
      </Container>
    );
  }}

  if (error) {{
    return (
      <Container maxWidth="xl">
        <Alert severity="error">Error: {{error.message}}</Alert>
      </Container>
    );
  }}

  if (!entity) {{
    return (
      <Container maxWidth="xl">
        <Alert severity="warning">{entity_pascal} not found in trash</Alert>
      </Container>
    );
  }}

  return (
    <Container maxWidth="xl">
      <Stack spacing={{3}}>
        <Stack direction="row" justifyContent="space-between" alignItems="center">
          <Stack direction="row" alignItems="center" spacing={{2}}>
            <IconButton onClick={{() => navigate('/{{{{MODULE_NAME}}}}/{entity_snake}/trash')}}>
              <ArrowBack />
            </IconButton>
            <Typography variant="h4">{entity_pascal} (Deleted)</Typography>
          </Stack>
          <Stack direction="row" spacing={{1}}>
            <Button
              variant="outlined"
              onClick={{() => navigate('/{{{{MODULE_NAME}}}}/{entity_snake}/trash')}}
            >
              Back to Trash
            </Button>
            <Button
              variant="contained"
              color="success"
              startIcon={{<RestoreFromTrash />}}
              onClick={{() => setRestoreDialogOpen(true)}}
              disabled={{restoreMutation.isPending}}
            >
              {{restoreMutation.isPending ? 'Restoring...' : 'Restore'}}
            </Button>
            <Button
              variant="contained"
              color="error"
              onClick={{() => setDeleteDialogOpen(true)}}
              disabled={{permanentDeleteMutation.isPending}}
            >
              Delete Permanently
            </Button>
          </Stack>
        </Stack>

        <Alert severity="warning">
          This item is in the trash. Restore it to make it active again, or delete it permanently.
        </Alert>

        <Paper>
          <Stack spacing={{3}} sx={{{{ p: 3 }}}}>
{{{{DETAIL_FIELDS}}}}
          </Stack>
        </Paper>

        {{/* Danger Zone for permanent delete */}}
        <DangerZone
          entityName="{entity_snake}"
          onDelete={{() => setDeleteDialogOpen(true)}}
          isSoftDelete={{false}}
          description="This action is irreversible. The data will be permanently removed."
          buttonLabel="Delete Permanently"
        />
      </Stack>

      {{/* Restore Dialog */}}
      <DeleteDialog
        open={{restoreDialogOpen}}
        onClose={{() => !restoreMutation.isPending && setRestoreDialogOpen(false)}}
        onConfirm={{handleRestoreConfirm}}
        entityName="{entity_snake}"
        itemDisplayName={{entity.id}}
        error={{actionError}}
        isDeleting={{restoreMutation.isPending}}
        isSoftDelete={{false}}
      />

      {{/* Permanent Delete Dialog */}}
      <DeleteDialog
        open={{deleteDialogOpen}}
        onClose={{() => !permanentDeleteMutation.isPending && setDeleteDialogOpen(false)}}
        onConfirm={{handlePermanentDeleteConfirm}}
        entityName="{entity_snake}"
        itemDisplayName={{entity.id}}
        error={{actionError}}
        isDeleting={{permanentDeleteMutation.isPending}}
        isSoftDelete={{false}}
      />
    </Container>
  );
}}
"#)
    }

    fn enhanced_schema_template(
        entity_pascal: &str,
        entity_snake: &str,
        enum_defs: &str,
        zod_fields: &str,
        create_zod_fields: &str,
    ) -> String {
        format!(r#"/**
 * {entity_pascal} Validation Schema
 *
 * Generated Zod schema for {entity_pascal} validation.
 */

import {{ z }} from 'zod';

{enum_defs}
/**
 * {entity_pascal} schema for validation
 */
export const {entity_snake}Schema = z.object({{
  id: z.string().uuid().optional(),
{zod_fields}
}});

/**
 * Schema for creating a {entity_pascal}
 */
export const create{entity_pascal}Schema = z.object({{
{create_zod_fields}
}});

/**
 * Schema for updating a {entity_pascal}
 */
export const update{entity_pascal}Schema = {entity_snake}Schema.partial().required({{ id: true }});

/**
 * Type inference from schema
 */
export type {entity_pascal}Input = z.infer<typeof {entity_snake}Schema>;
export type Create{entity_pascal}Input = z.infer<typeof create{entity_pascal}Schema>;
export type Update{entity_pascal}Input = z.infer<typeof update{entity_pascal}Schema>;

// <<< CUSTOM: Add custom schemas here
// END CUSTOM
"#)
    }
}

/// Result of enhanced code generation
#[derive(Debug, Default, Clone)]
pub struct GenerationResult {
    /// Number of entities found
    pub entities_found: usize,
    /// Files that were generated
    pub files_generated: Vec<PathBuf>,
    /// Files that would be generated in dry run
    pub dry_run_files: Vec<PathBuf>,
    /// Number of files skipped
    pub skipped: usize,
}

impl GenerationResult {
    /// Get a summary of the generation result
    pub fn summary(&self) -> String {
        if self.dry_run_files.is_empty() {
            format!(
                "Generated {} files for {} entities",
                self.files_generated.len(),
                self.entities_found
            )
        } else {
            format!(
                "Would generate {} files for {} entities (dry run)",
                self.dry_run_files.len(),
                self.entities_found
            )
        }
    }
}

// Import FieldDefinition for use in methods
use crate::webgen::ast::entity::FieldDefinition;

impl FieldDefinition {
    /// Get a user-friendly label for a field
    #[allow(dead_code)]
    fn field_label(&self) -> String {
        if let Some(desc) = &self.description {
            return desc.clone();
        }

        // Convert snake_case to Title Case
        self.name
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
}
