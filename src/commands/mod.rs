//! CLI command definitions and handlers

pub mod kotlin;
pub mod schema;

use clap::{Parser, Subcommand};

/// metaphor-plugin-schema - Schema-driven code generator (Rust + Kotlin)
#[derive(Parser, Debug)]
#[command(name = "metaphor-plugin-schema")]
#[command(author)]
#[command(version)]
#[command(about = "Schema-driven code generator (Rust server-side targets and Kotlin Multiplatform mobile)", long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Schema operations: parse, validate, generate (server-side: Rust, SQL, etc.)
    Schema {
        #[command(subcommand)]
        action: schema::SchemaAction,
    },

    /// Kotlin Multiplatform Mobile code generation
    Kotlin {
        #[command(subcommand)]
        action: kotlin::KotlinAction,
    },
}
