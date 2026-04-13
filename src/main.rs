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

        // Shortcut: generate:rust → same as `schema generate`
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
            module, module_path, output, package, target, skip_existing, verbose,
        } => {
            let action = kotlin::KotlinAction::Generate {
                module,
                module_path,
                output,
                package,
                target,
                skip_existing,
                verbose,
            };
            kotlin::run(action)?;
        }

        // Shortcut: generate:webapp → delegates to metaphor-webgen binary
        Commands::GenerateWebapp {
            module, target, entity, output, dry_run, force,
        } => {
            webapp::run(&module, &target, entity.as_deref(), output.as_ref(), dry_run, force)?;
        }
    }

    Ok(())
}
