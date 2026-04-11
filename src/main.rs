use anyhow::Result;
use metaphor_schema::commands::{kotlin, Cli, Commands};
use clap::Parser;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Schema { action } => {
            metaphor_schema::commands::schema::execute(action)?;
        }
        Commands::Kotlin { action } => {
            kotlin::run(action)?;
        }
    }

    Ok(())
}
