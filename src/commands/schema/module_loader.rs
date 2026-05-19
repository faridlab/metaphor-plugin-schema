//! Build a [`ModuleSchema`] from a set of schema files.
//!
//! Walks the provided files in two passes: the first picks up
//! `index.model.yaml` (module name, generators config, shared types); the
//! second parses every other `.model.yaml` / `.hook.yaml` / `.workflow.yaml`
//! (plus the legacy `.schema` DSL) and merges entities, value objects,
//! domain services, event-sourcing configs, projections, application
//! services, handlers, subscriptions, integrations, DTOs, etc. into the
//! schema.
//!
//! Shared by `validate`, `diff`, and `generate` — anywhere the command needs
//! the fully-resolved view of a module.

use anyhow::{Context, Result};
use indexmap::IndexMap;
use std::fs;
use std::path::PathBuf;

use crate::ast::ModuleSchema;
use crate::parser::{
    parse_hook, parse_model, parse_yaml_hook_flexible, parse_yaml_model_flexible,
    parse_yaml_workflow, resolve_shared_types, HookParseResult, ModelParseResult, YamlField,
};

/// Build a [`ModuleSchema`] from a list of schema files.
///
/// Supports both the legacy `.schema` DSL and the modern YAML format.
/// Returns the merged schema plus a list of soft errors (duplicate models,
/// parse failures on individual files) so callers can decide whether to
/// surface them, fail, or ignore them under `--lenient`.
pub(super) fn build_module_schema(
    module_name: &str,
    schema_files: &[PathBuf],
) -> Result<(ModuleSchema, Vec<String>)> {
    let mut module_schema = ModuleSchema::new(module_name);
    let mut errors = Vec::new();

    // First pass: collect shared_types from index files
    let mut resolved_shared_types: IndexMap<String, IndexMap<String, YamlField>> = IndexMap::new();

    for file in schema_files {
        let content = fs::read_to_string(file)
            .with_context(|| format!("Failed to read {}", file.display()))?;

        let filename = file.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if filename == "index.model.yaml" {
            match parse_yaml_model_flexible(&content) {
                Ok(ModelParseResult::Index(index_schema)) => {
                    if let Some(name) = &index_schema.module {
                        module_schema.name = name.clone();
                    }
                    if let Some(config) = &index_schema.config {
                        module_schema.generators_config = config.generators.clone();
                    }
                    resolved_shared_types = resolve_shared_types(&index_schema.shared_types);
                }
                Ok(ModelParseResult::Model(_)) => {
                    // index.model.yaml parsed as a regular model — unusual but ok.
                }
                Err(e) => errors.push(e.format_with_source(&content, Some(filename))),
            }
        }
    }

    module_schema.shared_types = resolved_shared_types.clone();

    // Second pass: parse everything else
    for file in schema_files {
        let content = fs::read_to_string(file)
            .with_context(|| format!("Failed to read {}", file.display()))?;

        let filename = file.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if filename == "index.model.yaml" {
            continue;
        }

        // Legacy .schema format
        if filename.ends_with(".model.schema") {
            match parse_model(&content) {
                Ok(model_file) => module_schema.merge_model_file(model_file),
                Err(e) => errors.push(e.format_with_source(&content, Some(filename))),
            }
        } else if filename.ends_with(".hook.schema") || filename.ends_with(".workflow.schema") {
            match parse_hook(&content) {
                Ok(hook_file) => module_schema.merge_hook_file(hook_file),
                Err(e) => errors.push(e.format_with_source(&content, Some(filename))),
            }
        }
        // Modern YAML format
        else if filename.ends_with(".model.yaml") {
            match crate::parser::parse_model_yaml_str(&content) {
                Ok(yaml_schema) => {
                    let enums: Vec<_> = yaml_schema.enums.to_vec();
                    for yaml_enum in enums {
                        let enum_def = yaml_enum.into_enum();
                        if let Some(existing) = module_schema
                            .enums
                            .iter()
                            .find(|e| e.name == enum_def.name)
                        {
                            errors.push(format!(
                                "Duplicate enum '{}' defined in '{}' — already defined with {} variant(s). \
                                 Rename one to avoid conflicts (e.g., '{}' → 'Template{}').",
                                enum_def.name, filename,
                                existing.variants.len(),
                                enum_def.name, enum_def.name,
                            ));
                        } else {
                            module_schema.enums.push(enum_def);
                        }
                    }

                    // ------------------------------------------------------------
                    // DDD EXTENSIONS: entities, value objects, domain services,
                    // event sourcing configs, and authorization.
                    // ------------------------------------------------------------
                    let entities: Vec<_> = yaml_schema
                        .entities
                        .iter()
                        .map(|(name, entity)| entity.clone().into_entity(name.clone()))
                        .collect();

                    let value_objects: Vec<_> = yaml_schema
                        .value_objects
                        .iter()
                        .map(|(name, vo)| vo.clone().into_value_object(name.clone()))
                        .collect();

                    let domain_services: Vec<_> = yaml_schema
                        .domain_services
                        .iter()
                        .map(|(name, ds)| ds.clone().into_domain_service(name.clone()))
                        .collect();

                    let event_sourced: Vec<_> = yaml_schema
                        .event_sourced
                        .iter()
                        .map(|(name, es)| es.clone().into_event_sourced(name.clone()))
                        .collect();

                    let authorization = yaml_schema
                        .authorization
                        .as_ref()
                        .map(|auth| auth.clone().into_authorization());

                    let usecases: Vec<_> = yaml_schema
                        .usecases
                        .iter()
                        .map(|(name, uc)| uc.clone().into_usecase(name.clone()))
                        .collect();

                    let events: Vec<_> = yaml_schema
                        .events
                        .iter()
                        .map(|(name, ev)| ev.clone().into_domain_event(name.clone()))
                        .collect();

                    module_schema.merge_ddd_extensions(
                        entities,
                        value_objects,
                        domain_services,
                        event_sourced,
                        authorization,
                        usecases,
                        events,
                    );

                    // ------------------------------------------------------------
                    // CQRS & PRESENTATION EXTENSIONS: projections, services,
                    // handlers, subscriptions, integrations, presentation, DTOs,
                    // versioning, repository traits.
                    // ------------------------------------------------------------
                    let projections: Vec<_> = yaml_schema
                        .projections
                        .iter()
                        .map(|(name, proj)| proj.clone().into_projection(name.clone()))
                        .collect();

                    let services: Vec<_> = yaml_schema
                        .services
                        .iter()
                        .map(|(name, svc)| svc.clone().into_app_service(name.clone()))
                        .collect();

                    let handlers: Vec<_> = yaml_schema
                        .handlers
                        .iter()
                        .map(|(name, h)| h.clone().into_handler(name.clone()))
                        .collect();

                    let subscriptions: Vec<_> = yaml_schema
                        .subscribes_to
                        .iter()
                        .flat_map(|(module, events_map)| {
                            events_map.iter().map(move |(event, sub)| {
                                sub.clone().into_subscription(module.clone(), event.clone())
                            })
                        })
                        .collect();

                    let integrations: Vec<_> = yaml_schema
                        .integration
                        .iter()
                        .map(|(name, intg)| intg.clone().into_integration(name.clone()))
                        .collect();

                    let presentation = yaml_schema
                        .presentation
                        .as_ref()
                        .map(|p| p.clone().into_presentation());

                    let dtos: Vec<_> = yaml_schema
                        .dtos
                        .iter()
                        .map(|(name, dto)| dto.clone().into_dto(name.clone()))
                        .collect();

                    let versioning = yaml_schema
                        .versioning
                        .as_ref()
                        .map(|v| v.clone().into_versioning());

                    let traits: Vec<_> = yaml_schema
                        .traits
                        .iter()
                        .map(|(name, tr)| tr.clone().into_repository_trait(name.clone()))
                        .collect();

                    module_schema.merge_cqrs_extensions(
                        projections,
                        services,
                        handlers,
                        subscriptions,
                        integrations,
                        presentation,
                        dtos,
                        versioning,
                        traits,
                    );

                    // Convert models with shared-types context (for `extends` and JSONB support)
                    let models = yaml_schema.into_models_with_context(&resolved_shared_types);
                    for model in models {
                        if module_schema
                            .models
                            .iter()
                            .any(|m| m.name == model.name)
                        {
                            errors.push(format!(
                                "Duplicate model '{}' defined in '{}' — already defined in another schema file. \
                                 Each model name must be unique within a module.",
                                model.name, filename,
                            ));
                        } else {
                            module_schema.models.push(model);
                        }
                    }
                }
                Err(e) => errors.push(format!("{}: {}", filename, e)),
            }
        } else if filename.ends_with(".hook.yaml") {
            match parse_yaml_hook_flexible(&content) {
                Ok(HookParseResult::Hook(hook_file)) => module_schema.merge_hook_file(hook_file),
                Ok(HookParseResult::Index(index_schema)) => {
                    if let Some(module_name) = &index_schema.module {
                        module_schema.name = module_name.clone();
                    }
                    // Index-level events may be wired in later.
                }
                Err(e) => errors.push(e.format_with_source(&content, Some(filename))),
            }
        } else if filename.ends_with(".workflow.yaml") {
            match parse_yaml_workflow(&content) {
                Ok(workflow_file) => module_schema.merge_workflow_file(workflow_file),
                Err(e) => errors.push(e.format_with_source(&content, Some(filename))),
            }
        }
    }

    Ok((module_schema, errors))
}
