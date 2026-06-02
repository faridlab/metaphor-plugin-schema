//! Webapp code generation — uses the embedded `webgen` module.
//!
//! `metaphor schema generate:webapp` generates TypeScript + React code from
//! schema definitions, using the webgen engine merged into this crate.
//!
//! Two modes, both consistent with the kotlin/mobile generator:
//!
//! - **Workspace "app" mode** (recommended): `--output <app-name>` resolves the
//!   app's `src/generated/` dir from `metaphor.yaml`, auto-detects the primary
//!   module (explicit arg or from CWD), and fans out across transitive module
//!   deps — one command regenerates everything for an app, no per-app script.
//!   e.g. (from `apps/bersihir-service/`): `metaphor schema generate:webapp --output bersihir-webapp-admin`
//!
//! - **Single-module mode** (low-level): explicit `<module> --schema-dir <dir>
//!   --output <path>` for one module into a raw path.

use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::commands::kotlin::{discover_external_modules, read_index_module_name, resolve_schema_dir};
use crate::commands::workspace::Workspace;
use crate::webgen::{Config, Generator};

/// Default targets for a webapp: the framework-free Clean Architecture stack
/// (domain contracts + application + infrastructure). The legacy MUI/hooks
/// targets are opt-in via an explicit `--target all`/`forms`/`pages`/etc.
const DEFAULT_TARGETS: &str = "contracts,application,infrastructure";

/// Default import alias for generated app/infrastructure imports — matches the
/// in-app `@/generated` convention webapps expose for `src/generated/`.
const DEFAULT_IMPORT_ALIAS: &str = "@/generated";

/// CLI entry. Dispatches to workspace "app" mode or single-module mode.
#[allow(clippy::too_many_arguments)]
pub fn run(
    module: Option<&str>,
    target: &str,
    entity: Option<&str>,
    output: Option<&PathBuf>,
    schema_dir: Option<&PathBuf>,
    import_alias: Option<&str>,
    with_grpc: bool,
    dry_run: bool,
    force: bool,
) -> Result<()> {
    let cwd = std::env::current_dir().context("getting current dir")?;
    let workspace = Workspace::from_cwd(&cwd);
    let alias = import_alias.unwrap_or(DEFAULT_IMPORT_ALIAS);

    // Workspace "app" mode: `--output <app-name>` (a single-segment name that
    // resolves to a workspace app) → multi-module fan-out into the app.
    if let (Some(ws), Some(out)) = (workspace.as_ref(), output) {
        if let Some(app_out) = out.to_str().and_then(|s| ws.webapp_output_for_app(s)) {
            return run_for_app(
                ws, &cwd, module, &app_out, target, entity, alias, with_grpc, dry_run, force,
            );
        }
    }

    // Single-module mode.
    let module = module.context(
        "missing <MODULE>: pass a module name, or use `--output <app-name>` inside a \
         Metaphor workspace to auto-resolve the app's modules.",
    )?;
    generate_module(module, target, entity, output, schema_dir, alias, with_grpc, dry_run, force)
}

/// Workspace "app" mode: resolve the app output dir, the primary module, and
/// transitive module deps, then generate every module into `<app>/src/generated`.
#[allow(clippy::too_many_arguments)]
fn run_for_app(
    ws: &Workspace,
    cwd: &Path,
    module: Option<&str>,
    output_dir: &Path,
    target: &str,
    entity: Option<&str>,
    alias: &str,
    with_grpc: bool,
    dry_run: bool,
    force: bool,
) -> Result<()> {
    if !dry_run {
        // The webgen engine requires its output dir to already exist.
        std::fs::create_dir_all(output_dir)
            .with_context(|| format!("creating output dir {}", output_dir.display()))?;
    }

    // Primary module: explicit arg, or auto-detect from CWD project.
    let primary: String = match module.filter(|s| !s.is_empty()) {
        Some(m) => m.to_string(),
        None => ws
            .project_for_cwd(cwd)
            .map(|p| p.name.clone())
            .context(
                "no <MODULE> given and CWD is not inside a workspace project. Run from a \
                 module dir (e.g. apps/bersihir-service) or pass the module name explicitly.",
            )?,
    };

    let primary_schema = resolve_schema_dir(cwd, Path::new("libs/modules"), &primary, Some(ws))
        .with_context(|| format!("could not locate schema for module '{}'", primary))?;
    let primary_module =
        read_index_module_name(&primary_schema).unwrap_or_else(|| primary.clone());

    // Plan: primary + transitive deps (schema external_imports + metaphor.yaml depends_on).
    let mut planned: Vec<(String, PathBuf)> = vec![(primary_module.clone(), primary_schema.clone())];
    let mut seen: BTreeSet<String> = BTreeSet::new();
    seen.insert(primary_module.clone());
    seen.insert(primary.clone());

    let mut deps = discover_external_modules(&primary_schema).unwrap_or_default();
    // Find the primary project by name, or (when `primary` is a schema-module
    // name like `bersihir`) by the project whose path owns the primary schema —
    // so `depends_on` deps resolve regardless of how the module was named.
    let primary_project = ws.project_by_name(&primary).or_else(|| {
        ws.projects()
            .iter()
            .find(|p| primary_schema.starts_with(ws.project_path(p)))
    });
    if let Some(project) = primary_project {
        for dep in &project.depends_on {
            if let Some(ds) = ws.schema_dir_for(dep) {
                if let Some(mn) = read_index_module_name(&ds) {
                    deps.insert(mn);
                }
            }
        }
    }

    for dep in deps {
        if seen.contains(&dep) {
            continue;
        }
        match resolve_schema_dir(cwd, Path::new("libs/modules"), &dep, Some(ws)) {
            Ok(ds) if ds != primary_schema => {
                planned.push((dep.clone(), ds));
                seen.insert(dep);
            }
            _ => {
                println!(
                    "  {} module '{}' has no schema in this workspace — skipped \
                     (declare it in metaphor.yaml + `metaphor sync` to include it)",
                    "⚠".yellow(),
                    dep
                );
            }
        }
    }

    println!(
        "{}",
        format!(
            "🌐 Webapp generation → {} ({} module(s))",
            output_dir.display(),
            planned.len()
        )
        .bright_cyan()
        .bold()
    );

    let output_buf = output_dir.to_path_buf();
    for (mod_name, schema) in &planned {
        generate_module(
            mod_name,
            target,
            entity,
            Some(&output_buf),
            Some(schema),
            alias,
            with_grpc,
            dry_run,
            force,
        )?;
    }

    println!();
    println!(
        "{}",
        format!("✅ Generated {} module(s) for the app.", planned.len())
            .green()
            .bold()
    );
    Ok(())
}

/// Generate a single module into `output` using the webgen engine.
#[allow(clippy::too_many_arguments)]
fn generate_module(
    module: &str,
    target: &str,
    entity: Option<&str>,
    output: Option<&PathBuf>,
    schema_dir: Option<&PathBuf>,
    import_alias: &str,
    with_grpc: bool,
    dry_run: bool,
    force: bool,
) -> Result<()> {
    println!(
        "{}",
        format!("  • module: {}", module).bright_cyan()
    );

    // Build config. An empty/`"all"` target keeps the engine's own default
    // expansion; otherwise the requested comma-separated targets are used.
    let mut config = Config::new(module);
    let effective_target = if target.trim().is_empty() { DEFAULT_TARGETS } else { target };
    if effective_target != "all" {
        config = config.with_targets_str(effective_target);
    }

    if let Some(entity_name) = entity {
        config = config.with_entity(Some(entity_name.to_string()));
    }
    if let Some(output_dir) = output {
        config = config.with_output_dir(output_dir.clone());
    }
    if let Some(dir) = schema_dir {
        config = config.with_schema_dir(Some(dir.clone()));
    }
    config = config
        .with_import_root(import_alias)
        .with_grpc(with_grpc)
        .with_dry_run(dry_run)
        .with_force(force);

    let generator = Generator::new(config)?;
    if dry_run {
        println!("{}", "    DRY RUN - No files will be written".yellow());
    }
    let result = generator.generate()?;
    println!("    {}", result.summary().dimmed());
    Ok(())
}
