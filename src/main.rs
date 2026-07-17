use anyhow::Result;
use metaphor_schema::commands::{kotlin, schema, webapp, Cli, Commands};
use clap::Parser;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Schema { action } => {
            schema::execute(action)?;
        }
        Commands::Kotlin { action } => {
            kotlin::run(action)?;
        }

        // Shortcut: openapi-collect → same as `schema openapi-collect`. Exists at
        // top level so the `metaphor` orchestrator (which forwards
        // `metaphor schema <X>` as `metaphor-schema <X>`) can reach it.
        Commands::OpenapiCollect { module } => {
            schema::execute(schema::SchemaAction::OpenapiCollect { module })?;
        }

        // Shortcut: doctor → same as `schema doctor`. Needed at top level for the same
        // reason as openapi-collect above: `metaphor schema doctor` arrives here as
        // `metaphor-schema doctor`. Without this arm the command parsed as an unknown
        // subcommand and `execute_doctor` was unreachable at every version.
        Commands::Doctor { module } => {
            schema::execute(schema::SchemaAction::Doctor { module })?;
        }

        // Shortcut: generate:rust → same as `schema generate`. The MODULE
        // arg is optional here too; the inner SchemaAction::Generate dispatch
        // applies the same auto-detect-or-error logic.
        Commands::GenerateRust {
            module, target, output, dry_run, force, changed, base,
            models, hooks, workflows, lenient,
        } => {
            let action = schema::SchemaAction::Generate {
                module,
                target,
                output,
                dry_run,
                force,
                split: false,
                changed,
                base,
                validate: false,
                models,
                hooks,
                workflows,
                lenient,
            };
            schema::execute(action)?;
        }

        // Shortcut: generate:kotlin → same as `kotlin generate`
        Commands::GenerateKotlin {
            module, module_path, output, output_path, package, target, skip_existing, no_deps, verbose,
        } => {
            let action = kotlin::KotlinAction::Generate {
                module,
                module_path,
                output,
                output_path,
                package,
                target,
                skip_existing,
                no_deps,
                verbose,
            };
            kotlin::run(action)?;
        }

        // Shortcut: generate:webapp → delegates to metaphor-webgen binary
        Commands::GenerateWebapp {
            module, target, entity, output, schema_dir, import_alias, with_grpc, dry_run, force,
        } => {
            webapp::run(
                module.as_deref(), &target, entity.as_deref(), output.as_ref(), schema_dir.as_ref(),
                import_alias.as_deref(), with_grpc, dry_run, force,
            )?;
        }
    }

    Ok(())
}
