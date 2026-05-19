//! Phase 1 (post short-circuit): print the run banner and the list of
//! generation targets, plus the active flags (`--dry-run`, `--force`,
//! `--changed`).

use colored::Colorize;

use crate::generators::GenerationTarget;

pub(super) fn announce_run(
    module: &str,
    targets: &[GenerationTarget],
    dry_run: bool,
    force: bool,
    changed: bool,
) {
    println!(
        "{} code for module: {}",
        "Generating".green().bold(),
        module.cyan()
    );

    let target_names: Vec<&str> = targets.iter().map(target_label).collect();
    println!("  Targets: {}", target_names.join(", ").yellow());

    if dry_run {
        println!("  {}", "(dry run - no files will be written)".yellow());
    }
    if force {
        println!("  {}", "(force - will overwrite existing files)".yellow());
    }
    if changed {
        println!("  {}", "(changed only - using git to detect changes)".cyan());
    }
}

/// Human-readable label for each [`GenerationTarget`] used in the run banner.
fn target_label(t: &GenerationTarget) -> &'static str {
    match t {
        GenerationTarget::Proto => "proto",
        GenerationTarget::Rust => "rust",
        GenerationTarget::Sql => "sql",
        GenerationTarget::Repository => "repository",
        GenerationTarget::RepositoryTrait => "repository-trait",
        GenerationTarget::Service => "service",
        GenerationTarget::DomainService => "domain-service",
        GenerationTarget::UseCase => "usecase",
        GenerationTarget::Auth => "auth",
        GenerationTarget::Events => "events",
        GenerationTarget::StateMachine => "state-machine",
        GenerationTarget::Validator => "validator",
        // TODO: GenerationTarget::Permission => "permission",
        GenerationTarget::Handler => "handler",
        GenerationTarget::Grpc => "grpc",
        GenerationTarget::Graphql => "graphql",
        GenerationTarget::OpenApi => "openapi",
        GenerationTarget::Trigger => "trigger",
        GenerationTarget::Flow => "flow",
        GenerationTarget::Module => "module",
        GenerationTarget::Config => "config",
        GenerationTarget::ValueObject => "value-object",
        GenerationTarget::Specification => "specification",
        GenerationTarget::Cqrs => "cqrs",
        GenerationTarget::Computed => "computed",
        GenerationTarget::Projection => "projection",
        GenerationTarget::EventStore => "event-store",
        GenerationTarget::Export => "export",
        GenerationTarget::Integration => "integration",
        GenerationTarget::EventSubscription => "event-subscription",
        GenerationTarget::Dto => "dto",
        GenerationTarget::Versioning => "versioning",
        GenerationTarget::BulkOperations => "bulk-operations",
        GenerationTarget::Seeder => "seeder",
        GenerationTarget::IntegrationTest => "integration-test",
        GenerationTarget::AuditTriggers => "audit-triggers",
        GenerationTarget::AppState => "app-state",
        GenerationTarget::RoutesComposer => "routes-composer",
        GenerationTarget::HandlersModule => "handlers-module",
    }
}
