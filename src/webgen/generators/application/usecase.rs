//! Use case generator.
//!
//! Emits thin per-entity use cases built from the generic `makeCrudUseCases`
//! (+ `makeSoftDeleteUseCases`) factories in `shared/crud`.

use std::fs;

use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition};
use crate::webgen::ast::HookSchema;
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::generators::domain::DomainGenerationResult;
use crate::webgen::parser::{to_camel_case, to_pascal_case, to_snake_case};

/// Generator for use case implementations
pub struct UseCaseGenerator {
    config: Config,
}

impl UseCaseGenerator {
    /// Create a new use case generator
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Generate use cases for an entity
    pub fn generate(
        &self,
        entity: &EntityDefinition,
        _enums: &[EnumDefinition],
        _hooks: Option<&HookSchema>,
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let usecases_dir = self
            .config
            .output_dir
            .join(&self.config.module)
            .join("application")
            .join("usecases");

        if !self.config.dry_run {
            fs::create_dir_all(&usecases_dir).ok();
        }

        let content = self.generate_usecases_content(entity);
        let file_path = usecases_dir.join(format!("{}UseCases.ts", entity_pascal));

        result.add_file(file_path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&file_path, content).ok();
        }

        Ok(result)
    }

    /// Generate the thin use-cases content.
    fn generate_usecases_content(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_camel = to_camel_case(&entity.name);
        let entity_upper = to_snake_case(&entity.name).to_uppercase();

        let import_soft = if entity.has_soft_delete() {
            ", makeSoftDeleteUseCases"
        } else {
            ""
        };

        let soft_block = if entity.has_soft_delete() {
            format!(
                r#"

const soft = makeSoftDeleteUseCases('{entity_upper}', get{entity_pascal}Service);
export const softDelete{entity_pascal}UseCase = soft.softDelete;
export const restore{entity_pascal}UseCase = soft.restore;
export const permanentDelete{entity_pascal}UseCase = soft.permanentDelete;
export const list{entity_pascal}DeletedUseCase = soft.listDeleted;"#,
                entity_pascal = entity_pascal,
                entity_upper = entity_upper,
            )
        } else {
            String::new()
        };

        format!(
            r#"/**
 * {entity_pascal} use cases
 *
 * Generic CRUD orchestration over the service port (cross-cutting concerns like
 * authorization / audit / events are server-side, not the frontend's job).
 *
 * @module application/{module}/usecases/{entity_pascal}UseCases
 */

import {{ makeCrudUseCases{import_soft} }} from '{root}/shared/crud/crudUseCases';
import {{ get{entity_pascal}Service }} from '{root}/{module}/domain/service/{entity_pascal}Service';

const crud = makeCrudUseCases('{entity_upper}', get{entity_pascal}Service);

export const create{entity_pascal}UseCase = crud.create;
export const update{entity_pascal}UseCase = crud.update;
export const patch{entity_pascal}UseCase = crud.patch;
export const delete{entity_pascal}UseCase = crud.remove;
export const get{entity_pascal}ByIdUseCase = crud.getById;
export const list{entity_pascal}UseCase = crud.list;

/** All {entity_pascal} use cases as one object. */
export const {entity_camel}UseCases = crud;{soft_block}
"#,
            entity_pascal = entity_pascal,
            entity_camel = entity_camel,
            entity_upper = entity_upper,
            module = self.config.module,
            root = self.config.import_root,
            import_soft = import_soft,
            soft_block = soft_block,
        )
    }
}
