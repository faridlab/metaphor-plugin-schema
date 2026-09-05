//! Domain layer generators for DDD frontend architecture
//!
//! This module contains generators that produce a complete TypeScript/React
//! domain layer from YAML schema definitions, mirroring the backend's
//! Schema-First architecture.
//!
//! ## Generated Structure
//!
//! ```text
//! domain/{module}/
//! ├── entity/                   # Entity types + Zod schemas
//! │   ├── {Entity}.ts          # Interface + factory + type guard
//! │   └── {Entity}.schema.ts   # Zod validation schemas
//! ├── value_object/            # Immutable value wrappers
//! ├── repository/              # Repository interface types
//! ├── usecase/                 # CQRS commands and queries
//! │   ├── commands/
//! │   └── queries/
//! ├── service/                 # React Query hooks
//! ├── event/                   # Domain event types
//! └── specification/           # Business rule predicates
//! ```

pub mod command;
pub mod domain_event;
pub mod domain_service;
pub mod entity;
pub mod entity_schema;
pub mod query;
pub mod repository;
pub mod specification;
pub mod type_mapping;
pub mod value_object;

// Re-exports
pub use command::CommandGenerator;
pub use domain_event::DomainEventGenerator;
pub use domain_service::DomainServiceGenerator;
pub use entity::EntityGenerator;
pub use entity_schema::EntitySchemaGenerator;
pub use query::QueryGenerator;
pub use repository::RepositoryGenerator;
pub use specification::SpecificationGenerator;
pub use type_mapping::TypeMapper;
pub use value_object::ValueObjectGenerator;

use crate::webgen::ast::entity::{EntityDefinition, ModelSchema};
use crate::webgen::ast::HookSchema;
use crate::webgen::config::Config;
use crate::webgen::error::{Error, Result};
use std::path::PathBuf;

/// Result of domain layer generation
#[derive(Debug, Default, Clone)]
pub struct DomainGenerationResult {
    /// Files that were generated
    pub files_generated: Vec<PathBuf>,
    /// Files that would be generated in dry run
    pub dry_run_files: Vec<PathBuf>,
    /// Entity count
    pub entity_count: usize,
    /// Enum count
    pub enum_count: usize,
    /// Value object count
    pub value_object_count: usize,
}

impl DomainGenerationResult {
    /// Create a new result
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a generated file
    pub fn add_file(&mut self, path: PathBuf, dry_run: bool) {
        if dry_run {
            self.dry_run_files.push(path);
        } else {
            self.files_generated.push(path);
        }
    }

    /// Get summary message
    pub fn summary(&self) -> String {
        if self.dry_run_files.is_empty() {
            format!(
                "Generated {} domain files ({} entities, {} enums, {} value objects)",
                self.files_generated.len(),
                self.entity_count,
                self.enum_count,
                self.value_object_count
            )
        } else {
            format!(
                "Would generate {} domain files ({} entities, {} enums, {} value objects) - dry run",
                self.dry_run_files.len(),
                self.entity_count,
                self.enum_count,
                self.value_object_count
            )
        }
    }
}

/// Refuse to emit a module barrel that does not compile.
///
/// The entity index re-exports every entity file with `export *`, so two
/// entities whose decorated export names meet on one symbol produce a
/// TypeScript barrel that only fails downstream at `tsc` — in a generated
/// tree the consumer may not edit. Generation fails here instead, naming
/// both entities and the symbol.
fn check_entity_barrel_collisions(
    entities: &[EntityDefinition],
    hooks: &[HookSchema],
) -> Result<()> {
    use std::collections::HashMap;

    let mut owner: HashMap<String, &str> = HashMap::new();
    let mut collisions: Vec<String> = Vec::new();
    for entity in entities {
        let has_hook = hooks
            .iter()
            .any(|h| h.model.eq_ignore_ascii_case(&entity.name));
        let symbols = entity::entity_file_symbols(entity)
            .into_iter()
            .chain(entity_schema::schema_file_symbols(entity, has_hook));
        for symbol in symbols {
            use std::collections::hash_map::Entry;
            match owner.entry(symbol) {
                Entry::Occupied(first) => collisions.push(format!(
                    "`{}` is exported by both `{}` and `{}`",
                    first.key(),
                    first.get(),
                    entity.name
                )),
                Entry::Vacant(slot) => {
                    slot.insert(entity.name.as_str());
                }
            }
        }
    }
    if collisions.is_empty() {
        return Ok(());
    }
    Err(Error::Generation(format!(
        "module barrel collision: {} — the entity index re-exports every entity \
         file with `export *`, so these symbols cannot coexist; rename one of \
         the entities (no decorated export shape can dodge the clash)",
        collisions.join("; ")
    )))
}

/// Domain layer generator that orchestrates all domain generators
pub struct DomainGenerator {
    config: Config,
    entity_gen: EntityGenerator,
    schema_gen: EntitySchemaGenerator,
    value_object_gen: ValueObjectGenerator,
    repository_gen: RepositoryGenerator,
    command_gen: CommandGenerator,
    query_gen: QueryGenerator,
    service_gen: DomainServiceGenerator,
    event_gen: DomainEventGenerator,
    specification_gen: SpecificationGenerator,
}

impl DomainGenerator {
    /// Create a new domain generator
    pub fn new(config: Config) -> Self {
        let type_mapper = TypeMapper::new();

        Self {
            entity_gen: EntityGenerator::new(config.clone(), type_mapper.clone()),
            schema_gen: EntitySchemaGenerator::new(config.clone(), type_mapper.clone()),
            value_object_gen: ValueObjectGenerator::new(config.clone(), type_mapper.clone()),
            repository_gen: RepositoryGenerator::new(config.clone()),
            command_gen: CommandGenerator::new(config.clone()),
            query_gen: QueryGenerator::new(config.clone()),
            service_gen: DomainServiceGenerator::new(config.clone()),
            event_gen: DomainEventGenerator::new(config.clone()),
            specification_gen: SpecificationGenerator::new(config.clone()),
            config,
        }
    }

    /// Generate the complete domain layer from a ModelSchema
    pub fn generate(
        &self,
        schema: &ModelSchema,
        hooks: Option<&HookSchema>,
    ) -> Result<DomainGenerationResult> {
        let hooks_slice: &[HookSchema] = match hooks {
            Some(h) => std::slice::from_ref(h),
            None => &[],
        };
        self.generate_all(&schema.models, &schema.enums, hooks_slice)
    }

    /// Generate the complete domain layer from separate entity and hook lists
    pub fn generate_all(
        &self,
        entities: &[EntityDefinition],
        enums: &[crate::webgen::ast::entity::EnumDefinition],
        hooks: &[HookSchema],
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();
        result.entity_count = entities.len();
        result.enum_count = enums.len();

        // Fail before writing anything if the entity barrel cannot compile.
        check_entity_barrel_collisions(entities, hooks)?;

        // Find hook for entity by name matching (model field in HookSchema)
        let find_hook = |entity_name: &str| -> Option<&HookSchema> {
            hooks
                .iter()
                .find(|h| h.model.eq_ignore_ascii_case(entity_name))
        };

        // Generate for each entity
        for entity in entities {
            let hook_schema = find_hook(&entity.name);

            // Entity types
            let entity_result = self.entity_gen.generate(entity, enums)?;
            self.merge_result(&mut result, entity_result);

            // Entity schemas (Zod)
            let schema_result = self.schema_gen.generate(entity, enums, hook_schema)?;
            self.merge_result(&mut result, schema_result);

            // Repository interfaces
            let repo_result = self.repository_gen.generate(entity)?;
            self.merge_result(&mut result, repo_result);

            // CQRS Commands
            let cmd_result = self.command_gen.generate(entity)?;
            self.merge_result(&mut result, cmd_result);

            // CQRS Queries
            let query_result = self.query_gen.generate(entity)?;
            self.merge_result(&mut result, query_result);

            // Domain services (React Query hooks)
            let service_result = self.service_gen.generate(entity)?;
            self.merge_result(&mut result, service_result);

            // Domain events
            let event_result = self.event_gen.generate(entity)?;
            self.merge_result(&mut result, event_result);

            // Specifications (business rules)
            let spec_result = self.specification_gen.generate(entity)?;
            self.merge_result(&mut result, spec_result);
        }

        // Generate value objects from detected value object fields
        let vo_result = self.value_object_gen.generate_from_entities(entities)?;
        result.value_object_count = vo_result.files_generated.len() + vo_result.dry_run_files.len();
        self.merge_result(&mut result, vo_result);

        // Generate module index files
        self.generate_index_files(entities, &mut result)?;

        Ok(result)
    }

    /// Merge a sub-result into the main result
    fn merge_result(&self, main: &mut DomainGenerationResult, sub: DomainGenerationResult) {
        main.files_generated.extend(sub.files_generated);
        main.dry_run_files.extend(sub.dry_run_files);
    }

    /// Generate index.ts files for each subdirectory
    fn generate_index_files(
        &self,
        entities: &[EntityDefinition],
        result: &mut DomainGenerationResult,
    ) -> Result<()> {
        use std::fs;

        let domain_dir = self
            .config
            .output_dir
            .join("domain")
            .join(&self.config.module);

        // Entity index
        let entity_index = self.generate_entity_index(entities);
        let entity_index_path = domain_dir.join("entity/index.ts");

        if self.config.dry_run {
            result.dry_run_files.push(entity_index_path);
        } else {
            if let Some(parent) = entity_index_path.parent() {
                fs::create_dir_all(parent).ok();
            }
            crate::webgen::custom_blocks::preserve_and_write(&entity_index_path, entity_index).ok();
            result.files_generated.push(entity_index_path);
        }

        // Generate other index files
        let subdirs = ["repository", "usecase", "service", "event", "specification"];
        for subdir in &subdirs {
            let index_path = domain_dir.join(subdir).join("index.ts");
            let index_content = self.generate_subdir_index(entities, subdir);

            if self.config.dry_run {
                result.dry_run_files.push(index_path);
            } else {
                if let Some(parent) = index_path.parent() {
                    fs::create_dir_all(parent).ok();
                }
                crate::webgen::custom_blocks::preserve_and_write(&index_path, index_content).ok();
                result.files_generated.push(index_path);
            }
        }

        // Main module index
        let module_index = self.generate_module_index();
        let module_index_path = domain_dir.join("index.ts");

        if self.config.dry_run {
            result.dry_run_files.push(module_index_path);
        } else {
            crate::webgen::custom_blocks::preserve_and_write(&module_index_path, module_index).ok();
            result.files_generated.push(module_index_path);
        }

        Ok(())
    }

    /// Generate entity index.ts
    fn generate_entity_index(&self, entities: &[EntityDefinition]) -> String {
        use crate::webgen::parser::to_pascal_case;

        let mut exports = Vec::new();

        for entity in entities {
            let pascal = to_pascal_case(&entity.name);
            exports.push(format!(
                "export * from './{}';\nexport * from './{}.schema';",
                pascal, pascal
            ));
        }

        format!(
            "// Entity exports - Generated by metaphor-webgen\n// Do not edit manually\n\n{}\n",
            exports.join("\n")
        )
    }

    /// Generate subdirectory index.ts
    fn generate_subdir_index(&self, entities: &[EntityDefinition], subdir: &str) -> String {
        use crate::webgen::parser::to_pascal_case;

        let mut exports = Vec::new();

        for entity in entities {
            let pascal = to_pascal_case(&entity.name);
            match subdir {
                "repository" => exports.push(format!("export * from './{}Repository';", pascal)),
                "service" => exports.push(format!("export * from './{}Service';", pascal)),
                "event" => exports.push(format!("export * from './{}Events';", pascal)),
                "specification" => {
                    exports.push(format!("export * from './{}Specifications';", pascal))
                }
                "usecase" => {
                    exports.push("export * from './commands';".to_string());
                    exports.push("export * from './queries';".to_string());
                }
                _ => {}
            }
        }

        format!(
            "// {} exports - Generated by metaphor-webgen\n// Do not edit manually\n\n{}\n",
            subdir,
            exports.join("\n")
        )
    }

    /// Generate main module index.ts
    fn generate_module_index(&self) -> String {
        format!(
            r#"// Domain layer exports for {} module
// Generated by metaphor-webgen - Do not edit manually

export * from './entity';
export * from './repository';
export * from './usecase';
export * from './service';
export * from './event';
export * from './specification';
"#,
            self.config.module
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webgen::ast::entity::{FieldDefinition, FieldType};

    fn entity_named(name: &str) -> EntityDefinition {
        EntityDefinition {
            name: name.to_string(),
            collection: format!("{}s", name.to_lowercase()),
            fields: vec![FieldDefinition {
                name: "id".to_string(),
                type_name: FieldType::Uuid,
                attributes: vec![],
                description: None,
                optional: false,
                default_value: None,
            }],
            relations: vec![],
            indexes: vec![],
            soft_delete: false,
        }
    }

    #[test]
    fn a_suffix_form_collision_fails_generation_naming_both_entities() {
        // Entity `Mailing` decorates to `mailingListFilterSchema`; an entity
        // literally named `MailingListFilter` claims the same symbol as its
        // base schema — the collision class the module hit, one segment along.
        let entities = vec![entity_named("Mailing"), entity_named("MailingListFilter")];
        let message = check_entity_barrel_collisions(&entities, &[])
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("`Mailing` and `MailingListFilter`"),
            "both entities must be named, got: {message}"
        );
        assert!(
            message.contains("mailingListFilterSchema"),
            "the colliding symbol must be named, got: {message}"
        );
    }

    #[test]
    fn a_prefix_form_collision_also_fails() {
        // `CreateUser`'s base schema name equals `User`'s create schema name.
        let entities = vec![entity_named("User"), entity_named("CreateUser")];
        assert!(check_entity_barrel_collisions(&entities, &[]).is_err());
    }

    #[test]
    fn the_names_that_collided_before_the_rename_now_pass() {
        // `Mailing` and `MailingFilter` were the live collision; the `List`
        // segment on the query-side names keeps them apart.
        let entities = vec![entity_named("Mailing"), entity_named("MailingFilter")];
        assert!(check_entity_barrel_collisions(&entities, &[]).is_ok());
    }
}
