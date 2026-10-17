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

use crate::ast::{ModuleSchema, ScheduledJob};
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
/// Take the module name an index file declares. Both index files name the
/// module; when they disagree the name used to depend on which file was read
/// last, so a disagreement is an error instead.
fn name_module(
    current: &mut String,
    named_by: &mut Option<(String, String)>,
    name: &str,
    file: &str,
    errors: &mut Vec<String>,
) {
    match named_by {
        Some((first_file, first_name)) if first_name != name => errors.push(format!(
            "{first_file} names the module `{first_name}` but {file} names it `{name}`; \
             the index files must agree on the module name"
        )),
        Some(_) => {}
        None => {
            *named_by = Some((file.to_string(), name.to_string()));
            *current = name.to_string();
        }
    }
}

pub(super) fn build_module_schema(
    module_name: &str,
    schema_files: &[PathBuf],
) -> Result<(ModuleSchema, Vec<String>)> {
    let mut module_schema = ModuleSchema::new(module_name);
    let mut errors = Vec::new();
    // The index file that first named the module, and the name it gave.
    let mut module_named_by: Option<(String, String)> = None;

    // First pass: collect shared_types from index files
    let mut resolved_shared_types: IndexMap<String, IndexMap<String, YamlField>> = IndexMap::new();
    // Module-level Postgres schema default (from `index.model.yaml` → `schema:`),
    // applied to any model that doesn't set its own (per-model or file-level).
    let mut module_schema_default: Option<String> = None;

    for file in schema_files {
        let content = fs::read_to_string(file)
            .with_context(|| format!("Failed to read {}", file.display()))?;

        let filename = file.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if filename == "index.model.yaml" {
            match parse_yaml_model_flexible(&content) {
                Ok(ModelParseResult::Index(index_schema)) => {
                    if let Some(name) = &index_schema.module {
                        name_module(&mut module_schema.name, &mut module_named_by, name, filename, &mut errors);
                    }
                    if let Some(config) = &index_schema.config {
                        module_schema.generators_config = config.generators.clone();
                    }
                    module_schema_default = index_schema.schema.clone();
                    module_schema.company_fence = index_schema.company_fence;
                    module_schema.org_fence = index_schema.org_fence;
                    // The permission vocabulary and system roles live in the module index.
                    if let Some(auth) = &index_schema.authorization {
                        module_schema.authorization = Some(auth.clone().into_authorization());
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
            for event in declared_event_names(&content) {
                module_schema.declared_events.push((filename.to_string(), event));
            }
            module_schema.rule_register.read_events_file(filename, &content);
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
                        if let Some(existing) =
                            module_schema.enums.iter().find(|e| e.name == enum_def.name)
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

                    let value_objects: Vec<_> = match yaml_schema
                        .value_objects
                        .iter()
                        .map(|(name, vo)| vo.clone().into_value_object(name.clone()))
                        .collect::<Result<Vec<_>, _>>()
                    {
                        Ok(vos) => vos,
                        Err(e) => {
                            errors.push(format!("{}: {e}", filename));
                            continue;
                        }
                    };

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
                    let mut models =
                        match yaml_schema.into_models_with_context(&resolved_shared_types) {
                            Ok(models) => models,
                            Err(e) => {
                                errors.push(format!("{}: {e}", filename));
                                continue;
                            }
                        };
                    // Apply the module-level schema default last: per-model and
                    // file-level `schema:` have already been resolved, so only
                    // models still without a schema inherit the module default.
                    for model in &mut models {
                        model.apply_schema_default(&module_schema_default);
                    }
                    for model in models {
                        if module_schema.models.iter().any(|m| m.name == model.name) {
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
                // `{:#}` walks the anyhow chain so serde's "unknown field
                // `foo`, expected one of ..." surfaces instead of being hidden
                // behind the generic top-level context.
                Err(e) => errors.push(format!("{}: {:#}", filename, e)),
            }
        } else if filename.ends_with(".hook.yaml") {
            module_schema.rule_register.read_file(filename, &content);
            for event in declared_event_names(&content) {
                module_schema.declared_events.push((filename.to_string(), event));
            }
            match parse_yaml_hook_flexible(&content) {
                Ok(HookParseResult::Hook(hook_file)) => module_schema.merge_hook_file(hook_file),
                Ok(HookParseResult::Index(index_schema)) => {
                    if let Some(module_name) = &index_schema.module {
                        name_module(&mut module_schema.name, &mut module_named_by, module_name, filename, &mut errors);
                    }
                    // Scheduled jobs are module-level declarations — carry them
                    // into the schema instead of dropping them at load time.
                    for (name, job) in &index_schema.scheduled_jobs {
                        module_schema.scheduled_jobs.push(ScheduledJob {
                            name: name.clone(),
                            schedule: job.schedule.clone(),
                            handler: job.handler.clone(),
                            posture: job.posture,
                            triggers: job.triggers.clone(),
                            commit_policy: job.commit_policy,
                            pickup_lock: job.pickup_lock,
                        });
                    }
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

    // A module config file (`_module.config.yaml`) is no schema file the passes above read, but
    // it declares events too: read them so the module's whole event contract is known.
    if let Some(schema_dir) = schema_files
        .first()
        .and_then(|f| f.ancestors().find(|a| a.file_name().is_some_and(|n| n == "schema")))
    {
        let mut configs: Vec<PathBuf> = walkdir::WalkDir::new(schema_dir)
            .into_iter()
            .filter_map(Result::ok)
            .map(|e| e.into_path())
            .filter(|p| p.to_string_lossy().ends_with(".config.yaml"))
            .collect();
        configs.sort();
        for path in configs {
            if let Ok(content) = fs::read_to_string(&path) {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                for event in declared_event_names(&content) {
                    module_schema.declared_events.push((name.to_string(), event));
                }
                module_schema.rule_register.read_events_file(name, &content);
            }
        }
    }

    // Bind once every model is known: hook files may load before model files.
    let models = module_schema.models.clone();
    module_schema.rule_register.bind(&models);
    module_schema.rule_register.check(&models);
    let enums = module_schema.enums.iter().map(|e| e.name.clone()).collect();
    module_schema.rule_register.mark_compiled(&models, &enums);
    // `enforced_by:` names a file under the module root, the parent of its `schema/` directory.
    if let Some(root) = schema_files
        .first()
        .and_then(|f| f.ancestors().find(|a| a.file_name().is_some_and(|n| n == "schema")))
        .and_then(|schema_dir| schema_dir.parent())
    {
        module_schema.rule_register.verify_enforced_by(root);
    }

    Ok((module_schema, errors))
}

/// The event names a hook file or the model index declares under a top-level `events:`, as a map keyed by name or
/// a list of `{ name: … }` entries.
fn declared_event_names(content: &str) -> Vec<String> {
    let Ok(doc) = serde_yaml::from_str::<serde_yaml::Value>(content) else { return Vec::new() };
    match doc.get("events") {
        Some(serde_yaml::Value::Mapping(m)) => m.keys().filter_map(|k| k.as_str().map(str::to_string)).collect(),
        Some(serde_yaml::Value::Sequence(items)) => items
            .iter()
            .filter_map(|i| i.get("name").and_then(|n| n.as_str()).or_else(|| i.as_str()).map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: `index.hook.yaml` scheduled jobs used to be parsed and then
    /// silently dropped at load time ("Index-level events may be wired in
    /// later"). They must reach [`ModuleSchema::scheduled_jobs`].
    #[test]
    fn hook_index_scheduled_jobs_reach_module_schema() {
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("index.hook.yaml");
        std::fs::write(
            &index,
            "module: demo\nscheduled_jobs:\n  nightly_gc:\n    schedule: \"0 3 * * *\"\n    handler: gc::run\n",
        )
        .unwrap();

        let (schema, errors) = build_module_schema("demo", &[index.clone()]).unwrap();
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert_eq!(schema.name, "demo");
        assert_eq!(schema.scheduled_jobs.len(), 1);
        let job = &schema.scheduled_jobs[0];
        assert_eq!(job.name, "nightly_gc");
        assert_eq!(job.schedule, "0 3 * * *");
        assert_eq!(job.handler, "gc::run");
        // ADR-0020 vocabulary is undeclared on legacy entries.
        assert_eq!(job.posture, None);
        assert!(job.triggers.is_empty());
        assert_eq!(job.commit_policy, None);
        assert!(!job.pickup_lock);
    }

    /// The two index files each name the module. When they disagree, which
    /// name the generated module type took depended on the order the files
    /// were read: a `module: __MODULE__` left from the scaffold, or a hook
    /// index naming another module, renamed the crate's public module type.
    #[test]
    fn index_files_that_name_different_modules_are_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let models = dir.path().join("index.model.yaml");
        let hooks = dir.path().join("index.hook.yaml");
        std::fs::write(&models, "module: expenses\n").unwrap();
        std::fs::write(&hooks, "module: __MODULE__\nscheduled_jobs: {}\n").unwrap();

        let (_, errors) = build_module_schema("expenses", &[hooks.clone(), models.clone()]).unwrap();
        assert_eq!(
            errors,
            vec!["index.model.yaml names the module `expenses` but index.hook.yaml names it `__MODULE__`; \
                  the index files must agree on the module name"
                .to_string()],
        );

        std::fs::write(&hooks, "module: expenses\nscheduled_jobs: {}\n").unwrap();
        let (schema, errors) = build_module_schema("expenses", &[hooks, models]).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(schema.name, "expenses");
    }

    /// ADR-0020 vocabulary on `index.hook.yaml` jobs reaches the module
    /// schema verbatim — posture, triggers, commit policy, pickup lock.
    #[test]
    fn hook_index_scheduled_jobs_carry_posture_vocabulary() {
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("index.hook.yaml");
        std::fs::write(
            &index,
            "module: demo\nscheduled_jobs:\n  mail_queue:\n    schedule: \"every 5m\"\n    handler: mail::drain\n    posture: self_arming\n    triggers:\n      - message.created\n      - message.retried\n    commit_policy: commit_per_batch\n    pickup_lock: true\n",
        )
        .unwrap();

        let (schema, errors) = build_module_schema("demo", &[index]).unwrap();
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        let job = &schema.scheduled_jobs[0];
        assert_eq!(job.name, "mail_queue");
        assert_eq!(job.posture, Some(crate::ast::JobPosture::SelfArming));
        assert_eq!(job.triggers, vec!["message.created", "message.retried"]);
        assert_eq!(
            job.commit_policy,
            Some(crate::ast::CommitPolicy::CommitPerBatch)
        );
        assert!(job.pickup_lock);
    }

    /// A bad posture name must be a clean parse error, not a silent default.
    #[test]
    fn hook_index_rejects_unknown_posture() {
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("index.hook.yaml");
        std::fs::write(
            &index,
            "module: demo\nscheduled_jobs:\n  weird:\n    schedule: \"0 3 * * *\"\n    handler: gc::run\n    posture: whenever\n",
        )
        .unwrap();

        let (_schema, errors) = build_module_schema("demo", &[index]).unwrap();
        assert!(
            errors.iter().any(|e| e.contains("whenever")),
            "unknown posture must surface as a named error, got: {errors:?}"
        );
    }

    /// A module with no scheduled jobs (the state of all 49 backbone
    /// `index.hook.yaml` files) must serialize and behave exactly as before.
    #[test]
    fn hook_index_without_jobs_stays_empty() {
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("index.hook.yaml");
        std::fs::write(&index, "module: demo\nscheduled_jobs: {}\n").unwrap();

        let (schema, errors) = build_module_schema("demo", &[index]).unwrap();
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert!(schema.scheduled_jobs.is_empty());
    }

    /// A model's machine in its own hook file and its rules in another are one hook: the
    /// generated verbs are emitted and mounted whichever file loads first.
    #[test]
    fn a_machine_and_rules_in_separate_hook_files_join_into_one_hook() {
        let dir = tempfile::tempdir().unwrap();
        let models = dir.path().join("ticket.model.yaml");
        std::fs::write(
            &models,
            "enums:\n  - name: TicketStatus\n    variants: [open, done]\nmodels:\n  - name: Ticket\n    collection: tickets\n    fields:\n      id:\n        type: uuid\n        attributes: [\"@id\", \"@default(uuid)\"]\n      title:\n        type: string\n      status:\n        type: TicketStatus\n        attributes: [\"@default(open)\"]\n",
        )
        .unwrap();
        let rules = dir.path().join("ticket.hook.yaml");
        std::fs::write(
            &rules,
            "name: Ticket\nmodel: Ticket\nrules:\n  title_set:\n    when: [create]\n    condition: \"title != ''\"\n    code: title_required\n    message: \"a title is required\"\n",
        )
        .unwrap();
        let machine = dir.path().join("ticket_state.hook.yaml");
        std::fs::write(
            &machine,
            "name: ticket_state\nmodel: Ticket\nstates:\n  field: status\n  verbs: generated\n  values:\n    open:\n      initial: true\n    done:\n      final: true\n  transitions:\n    finish:\n      from: open\n      to: done\n",
        )
        .unwrap();
        for order in [vec![models.clone(), rules.clone(), machine.clone()], vec![models.clone(), machine.clone(), rules.clone()]] {
            let (schema, errors) = build_module_schema("helpdesk", &order).unwrap();
            assert!(errors.is_empty(), "unexpected errors: {errors:?}");
            assert_eq!(schema.hooks.len(), 1, "one hook for the model");
            let hook = schema.hook_for("Ticket").expect("Ticket has a hook");
            assert!(hook.state_machine.is_some(), "the machine is joined in");
            assert_eq!(hook.rules.len(), 1, "the rules are joined in");
            assert_eq!(hook.name, "ticket_state", "the hook keeps the machine file's name");
            let model = schema.models.iter().find(|m| m.name == "Ticket").unwrap();
            assert!(crate::rules::compile::mounts_generated_verbs(&schema, model), "the generated verbs are mounted");
        }
    }


    /// Events declared in the model index and in a module config file are read, not only those of
    /// hook files, and each is bound to the aggregate it names.
    #[test]
    fn events_of_the_index_and_the_config_file_are_declared_too() {
        let dir = tempfile::tempdir().unwrap();
        let schema_dir = dir.path().join("schema");
        std::fs::create_dir_all(schema_dir.join("hooks")).unwrap();
        std::fs::create_dir_all(schema_dir.join("models")).unwrap();
        let index = schema_dir.join("models/index.model.yaml");
        std::fs::write(&index, "module: m\nevents:\n  FromIndex:\n    aggregate: Ticket\n    on: create\n    fields: [id]\n").unwrap();
        let model = schema_dir.join("models/ticket.model.yaml");
        std::fs::write(&model, "models:\n  - name: Ticket\n    fields:\n      id: { type: uuid, attributes: [\"@id\"] }\n").unwrap();
        std::fs::write(schema_dir.join("hooks/_module.config.yaml"), "events:\n  FromConfig:\n    fields: { user_id: uuid }\n").unwrap();
        let (schema, errors) = build_module_schema("m", &[index, model]).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        let events: Vec<_> = schema.rule_register.events.iter().map(|e| (e.name.as_str(), e.bound_model.as_deref())).collect();
        assert_eq!(events, vec![("FromIndex", Some("Ticket")), ("FromConfig", None)]);
        assert!(schema.rule_register.events[0].problems.is_empty(), "{:?}", schema.rule_register.events[0].problems);
    }

    /// A declared event with no `on:` is staged by hand and a workflow runs nothing; generation
    /// says so instead of dropping them without a word.
    #[test]
    fn hand_staged_events_and_workflows_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let hook = dir.path().join("ticket.hook.yaml");
        std::fs::write(&hook, "name: Ticket\nmodel: Ticket\nevents:\n  - name: TicketOpened\n    fields: [id]\n").unwrap();
        let (mut schema, errors) = build_module_schema("helpdesk", &[hook]).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(schema.declared_events, vec![("ticket.hook.yaml".to_string(), "TicketOpened".to_string())]);
        schema.workflows.push(crate::ast::Workflow { name: "escalation".into(), ..Default::default() });
        let warnings = crate::resolver::declaration_warnings(&schema);
        assert!(warnings.iter().any(|w| w.contains("TicketOpened") && w.contains("no generated code stages")), "{warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("workflow 'escalation'") && w.contains("declaration-only")), "{warnings:?}");
    }

}
