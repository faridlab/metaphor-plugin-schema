//! Contracts generator — PURE, framework-free domain genotype.
//!
//! This generator is a deliberately slim subset of [`super::domain::DomainGenerator`].
//! It emits ONLY the framework-free "genotype" that every target (Rust / Kotlin / TS)
//! shares, leaving the runtime "phenotype" (React hooks, Mantine UI, TanStack Query)
//! to be hand-written by the consuming webapp.
//!
//! Emitted per entity (all pure TypeScript, importing at most `zod`):
//! - `entity/{Entity}.ts`         — interface + factory + type guards (+ enums)
//! - `entity/{Entity}.schema.ts`  — Zod schemas + inferred DTOs (Create/Update/Patch)
//! - `repository/{Entity}Repository.ts` — repository PORT interface (DIP boundary)
//!
//! Explicitly EXCLUDED (vs the full `domain` target): domain services (TanStack
//! hooks), CQRS commands/queries, domain events, specifications, value objects —
//! i.e. anything framework-coupled or ceremonial. A consuming webapp composes the
//! generated port with its own data-access adapter (e.g. TanStack Query).
//!
//! ## Generated structure (rooted at `output_dir`)
//!
//! ```text
//! domain/{module}/
//! ├── entity/
//! │   ├── {Entity}.ts
//! │   ├── {Entity}.schema.ts
//! │   └── index.ts
//! ├── repository/
//! │   ├── {Entity}Repository.ts
//! │   └── index.ts
//! └── index.ts
//! ```
//!
//! Point `--output` at the webapp's generated folder (e.g.
//! `apps/<app>/src/generated`) so the whole tree is clearly marked as generated.

use std::fs;
use std::path::PathBuf;

use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition};
use crate::webgen::ast::HookSchema;
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::to_pascal_case;

use super::domain::{
    DomainGenerationResult, DomainServiceGenerator, EntityGenerator, EntitySchemaGenerator,
    RepositoryGenerator, TypeMapper,
};

/// Generator for pure, framework-free domain contracts.
pub struct ContractsGenerator {
    config: Config,
    entity_gen: EntityGenerator,
    schema_gen: EntitySchemaGenerator,
    repository_gen: RepositoryGenerator,
    service_gen: DomainServiceGenerator,
}

impl ContractsGenerator {
    /// Create a new contracts generator.
    pub fn new(config: Config) -> Self {
        let type_mapper = TypeMapper::new();
        Self {
            entity_gen: EntityGenerator::new(config.clone(), type_mapper.clone()),
            schema_gen: EntitySchemaGenerator::new(config.clone(), type_mapper),
            repository_gen: RepositoryGenerator::new(config.clone()),
            service_gen: DomainServiceGenerator::new(config.clone()),
            config,
        }
    }

    /// Generate pure contracts for the given entities.
    ///
    /// `hooks` are used only to enrich Zod validation rules (business constraints
    /// declared in `*.hook.yaml`); they never introduce framework coupling.
    pub fn generate_all(
        &self,
        entities: &[EntityDefinition],
        enums: &[EnumDefinition],
        hooks: &[HookSchema],
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();
        result.entity_count = entities.len();
        result.enum_count = enums.len();

        let find_hook = |entity_name: &str| -> Option<&HookSchema> {
            hooks.iter().find(|h| h.model.eq_ignore_ascii_case(entity_name))
        };

        for entity in entities {
            let hook_schema = find_hook(&entity.name);

            // Entity types (+ enums used by the entity) — pure.
            self.merge(&mut result, self.entity_gen.generate(entity, enums)?);
            // Zod schemas + inferred DTOs — imports only `zod`.
            self.merge(&mut result, self.schema_gen.generate(entity, enums, hook_schema)?);
            // Repository PORT interface — imports only the entity's own schema types.
            self.merge(&mut result, self.repository_gen.generate(entity)?);
            // Service PORT — pure interface + injectable accessor (no @tanstack).
            self.merge(&mut result, self.service_gen.generate(entity)?);
        }

        self.generate_shared_types(&mut result)?;
        self.generate_index_files(entities, &mut result)?;
        self.generate_manifest(&mut result)?;

        Ok(result)
    }

    /// Emit the framework-free shared runtime (generic CRUD bases, ports,
    /// repository impl, use-case factories, entity helpers, HTTP transport,
    /// pagination types) into `shared/`. Generated entity files extend/call
    /// these instead of repeating the boilerplate.
    fn generate_shared_types(&self, result: &mut DomainGenerationResult) -> Result<()> {
        for (rel_path, content) in crate::webgen::generators::shared_runtime::shared_files() {
            let path = self.config.output_dir.join(rel_path);
            self.write_index(path, content.to_string(), result);
        }
        Ok(())
    }

    fn merge(&self, main: &mut DomainGenerationResult, sub: DomainGenerationResult) {
        main.files_generated.extend(sub.files_generated);
        main.dry_run_files.extend(sub.dry_run_files);
    }

    /// Emit barrel `index.ts` files for `entity/`, `repository/`, and the module root.
    fn generate_index_files(
        &self,
        entities: &[EntityDefinition],
        result: &mut DomainGenerationResult,
    ) -> Result<()> {
        let domain_dir = self.module_dir();

        // entity/index.ts — re-export each entity type + its schema.
        let mut entity_exports = Vec::new();
        for entity in entities {
            let pascal = to_pascal_case(&entity.name);
            entity_exports.push(format!(
                "export * from './{pascal}';\nexport * from './{pascal}.schema';"
            ));
        }
        let entity_index = format!(
            "// Entity contracts — generated by metaphor-webgen (contracts target)\n// Do not edit manually\n\n{}\n",
            entity_exports.join("\n")
        );
        self.write_index(domain_dir.join("entity/index.ts"), entity_index, result);

        // repository/index.ts — re-export each repository port.
        let mut repo_exports = Vec::new();
        for entity in entities {
            let pascal = to_pascal_case(&entity.name);
            repo_exports.push(format!("export * from './{pascal}Repository';"));
        }
        let repo_index = format!(
            "// Repository ports — generated by metaphor-webgen (contracts target)\n// Do not edit manually\n\n{}\n",
            repo_exports.join("\n")
        );
        self.write_index(domain_dir.join("repository/index.ts"), repo_index, result);

        // service/index.ts — re-export each service port.
        let mut service_exports = Vec::new();
        for entity in entities {
            let pascal = to_pascal_case(&entity.name);
            service_exports.push(format!("export * from './{pascal}Service';"));
        }
        let service_index = format!(
            "// Service ports — generated by metaphor-webgen (contracts target)\n// Do not edit manually\n\n{}\n",
            service_exports.join("\n")
        );
        self.write_index(domain_dir.join("service/index.ts"), service_index, result);

        // {module}/index.ts — module barrel. Exports the consumed contracts
        // (entity + repository). Service ports are imported via explicit paths
        // (e.g. by generated DI wiring): a `{X}` entity and a `{X}` service port
        // for entity `{X-minus-Service}` would otherwise collide here.
        let module_index = format!(
            "// Pure domain contracts for the `{}` module\n// Generated by metaphor-webgen (contracts target) — Do not edit manually\n\nexport * from './entity';\nexport * from './repository';\n",
            self.config.module
        );
        self.write_index(domain_dir.join("index.ts"), module_index, result);

        Ok(())
    }

    /// Write a `metaphor.codegen.yaml` manifest at the output root, marking the
    /// generated tree as generator-owned and reserving hand-written globs.
    fn generate_manifest(&self, result: &mut DomainGenerationResult) -> Result<()> {
        let manifest_path = self.config.output_dir.join("metaphor.codegen.yaml");
        // Module-agnostic globs: one manifest covers every generated module
        // regardless of which module ran last (each per-module run rewrites it).
        let manifest = r#"# metaphor.codegen.yaml — generated by metaphor-webgen
# Records what the schema generator owns. Regenerate every module with:
#   pnpm schema:gen   (loops: metaphor schema generate:webapp <module> ...)
#
# Everything under the generated tree below is OWNED BY THE GENERATOR and is
# overwritten on every regen — never hand-edit it. Change the schema instead.
generated:
  - "*/domain/**"
  - "*/application/**"
  - "*/infrastructure/**"
  - shared/**

# Hand-written files the generator must NEVER touch. Add your composition layer
# globs here (feature folders, UI, hooks) relative to the consuming app's src/.
user_owned:
  - features/**
  - components/**
  - auth/**
  - lib/**
  - hooks/**
"#
        .to_string();

        if self.config.dry_run {
            result.dry_run_files.push(manifest_path);
        } else {
            if let Some(parent) = manifest_path.parent() {
                fs::create_dir_all(parent).ok();
            }
            crate::webgen::custom_blocks::preserve_and_write(&manifest_path, manifest).ok();
            result.files_generated.push(manifest_path);
        }
        Ok(())
    }

    fn module_dir(&self) -> PathBuf {
        self.config.output_dir.join(&self.config.module).join("domain")
    }

    fn write_index(&self, path: PathBuf, content: String, result: &mut DomainGenerationResult) {
        if self.config.dry_run {
            result.dry_run_files.push(path);
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).ok();
            }
            crate::webgen::custom_blocks::preserve_and_write(&path, content).ok();
            result.files_generated.push(path);
        }
    }
}
