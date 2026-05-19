//! `metaphor schema parse` — parse one or more schema files and dump their
//! AST as JSON or a human-readable summary.
//!
//! Read-only inspection command. Useful for debugging parser behaviour and
//! for tooling that wants a stable JSON view of a schema directory.

use anyhow::{Context, Result};
use colored::Colorize;
use std::fs;
use std::path::PathBuf;

use crate::ast::{HookFile, ModelFile, WorkflowFile};
use crate::parser::{
    parse_hook, parse_model, parse_yaml_hook_flexible, parse_yaml_model, parse_yaml_workflow,
    HookParseResult, YamlHookIndexSchema,
};

use super::discovery::find_schema_files;
use super::OutputFormat;

pub(super) fn execute_parse(path: &PathBuf, format: OutputFormat) -> Result<()> {
    println!("{} {}", "Parsing:".green().bold(), path.display());

    let schema_files = find_schema_files(path)?;

    if schema_files.is_empty() {
        println!("{}", "No schema files found".yellow());
        return Ok(());
    }

    println!(
        "Found {} schema file(s)",
        schema_files.len().to_string().cyan()
    );

    for file in &schema_files {
        println!("  {} {}", "•".blue(), file.display());
    }

    println!();

    for file in &schema_files {
        let content = fs::read_to_string(file)
            .with_context(|| format!("Failed to read {}", file.display()))?;

        let filename = file.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if filename.ends_with(".model.schema") {
            println!("{} {}", "Parsing model:".cyan().bold(), file.display());
            match parse_model(&content) {
                Ok(model_file) => print_model_file(&model_file, &format),
                Err(e) => println!("{}", e.format_with_source(&content, Some(filename)).red()),
            }
        } else if filename.ends_with(".hook.schema") || filename.ends_with(".workflow.schema") {
            println!("{} {}", "Parsing hook:".cyan().bold(), file.display());
            match parse_hook(&content) {
                Ok(hook_file) => print_hook_file(&hook_file, &format),
                Err(e) => println!("{}", e.format_with_source(&content, Some(filename)).red()),
            }
        } else if filename.ends_with(".model.yaml") {
            println!("{} {}", "Parsing YAML model:".cyan().bold(), file.display());
            match parse_yaml_model(&content) {
                Ok(model_file) => print_model_file(&model_file, &format),
                Err(e) => println!("{}", e.format_with_source(&content, Some(filename)).red()),
            }
        } else if filename.ends_with(".hook.yaml") {
            println!("{} {}", "Parsing YAML hook:".cyan().bold(), file.display());
            match parse_yaml_hook_flexible(&content) {
                Ok(HookParseResult::Hook(hook_file)) => print_hook_file(&hook_file, &format),
                Ok(HookParseResult::Index(index_schema)) => print_hook_index(&index_schema, &format),
                Err(e) => println!("{}", e.format_with_source(&content, Some(filename)).red()),
            }
        } else if filename.ends_with(".workflow.yaml") {
            println!("{} {}", "Parsing YAML workflow:".cyan().bold(), file.display());
            match parse_yaml_workflow(&content) {
                Ok(workflow_file) => print_workflow_file(&workflow_file, &format),
                Err(e) => println!("{}", e.format_with_source(&content, Some(filename)).red()),
            }
        }

        println!();
    }

    Ok(())
}

fn print_model_file(model_file: &ModelFile, format: &OutputFormat) {
    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(model_file)
                    .unwrap_or_else(|_| "Failed to serialize".to_string())
            );
        }
        OutputFormat::Pretty => {
            for model in &model_file.models {
                println!("  {} {}", "Model:".green(), model.name.yellow());
                if let Some(ref collection) = model.collection {
                    println!("    Collection: {}", collection);
                }
                println!("    Fields: {}", model.fields.len());
                for field in &model.fields {
                    println!("      {} {}: {:?}", "•".blue(), field.name, field.type_ref);
                }
                if !model.relations.is_empty() {
                    println!("    Relations: {}", model.relations.len());
                    for rel in &model.relations {
                        println!(
                            "      {} {} -> {:?} ({:?})",
                            "•".blue(),
                            rel.name,
                            rel.target,
                            rel.relation_type
                        );
                    }
                }
            }

            if !model_file.enums.is_empty() {
                println!("  {} {}", "Enums:".green(), model_file.enums.len());
                for enum_def in &model_file.enums {
                    println!(
                        "    {} {} ({} variants)",
                        "•".blue(),
                        enum_def.name,
                        enum_def.variants.len()
                    );
                }
            }
        }
    }
}

fn print_hook_file(hook_file: &HookFile, format: &OutputFormat) {
    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(hook_file)
                    .unwrap_or_else(|_| "Failed to serialize".to_string())
            );
        }
        OutputFormat::Pretty => {
            for hook in &hook_file.hooks {
                println!("  {} {}", "Hook:".green(), hook.name.yellow());
                println!("    Model ref: {}", hook.model_ref);

                if let Some(ref sm) = hook.state_machine {
                    println!("    State Machine:");
                    println!("      Field: {}", sm.field);
                    println!("      States: {}", sm.states.len());
                    for state in &sm.states {
                        println!("        {} {}", "•".blue(), state.name);
                    }
                    println!("      Transitions: {}", sm.transitions.len());
                    for trans in &sm.transitions {
                        println!(
                            "        {} {:?} -> {}",
                            "•".blue(),
                            trans.from,
                            trans.to
                        );
                    }
                }

                println!("    Rules: {}", hook.rules.len());
                for rule in &hook.rules {
                    println!("      {} {}", "•".blue(), rule.name);
                }

                println!("    Triggers: {}", hook.triggers.len());
                for trigger in &hook.triggers {
                    println!("      {} {:?}", "•".blue(), trigger.event);
                }
            }
        }
    }
}

fn print_workflow_file(workflow_file: &WorkflowFile, format: &OutputFormat) {
    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(workflow_file)
                    .unwrap_or_else(|_| "Failed to serialize".to_string())
            );
        }
        OutputFormat::Pretty => {
            for workflow in &workflow_file.workflows {
                println!("  {} {}", "Workflow:".green(), workflow.name.yellow());
                if let Some(ref desc) = workflow.description {
                    println!("    Description: {}", desc);
                }
                println!("    Version: {}", workflow.version);
                println!("    Steps: {}", workflow.steps.len());
                for step in &workflow.steps {
                    println!("      {} {}", "•".blue(), step.name);
                }
            }
        }
    }
}

fn print_hook_index(index: &YamlHookIndexSchema, format: &OutputFormat) {
    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(index)
                    .unwrap_or_else(|_| "Failed to serialize".to_string())
            );
        }
        OutputFormat::Pretty => {
            println!("  {} (module configuration file)", "Hook Index:".green());

            if let Some(ref module) = index.module {
                println!("    Module: {}", module.yellow());
            }

            if let Some(version) = index.version {
                println!("    Version: {}", version);
            }

            if !index.imports.is_empty() {
                println!("    Imports: {}", index.imports.len());
                for import in &index.imports {
                    println!("      {} {}", "•".blue(), import);
                }
            }

            if !index.events.is_empty() {
                println!("    Domain Events: {}", index.events.len());
                for (name, event) in &index.events {
                    println!(
                        "      {} {} ({} fields)",
                        "•".blue(),
                        name,
                        event.fields.len()
                    );
                }
            }

            if !index.scheduled_jobs.is_empty() {
                println!("    Scheduled Jobs: {}", index.scheduled_jobs.len());
                for (name, job) in &index.scheduled_jobs {
                    println!("      {} {} - {}", "•".blue(), name, job.schedule);
                }
            }
        }
    }
}
