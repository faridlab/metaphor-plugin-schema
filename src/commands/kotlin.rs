//! Kotlin code generation CLI subcommand.
//!
//! Wraps the merged-in `crate::kotlin` module (originally `metaphor-plugin-mobilegen`).
//! The actual generation logic lives in [`crate::kotlin`]; this file is the
//! CLI parsing layer plus workspace-aware discovery so a single invocation
//! generates the requested module *and* its transitive schema-module deps.

use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::commands::workspace::Workspace;
use crate::kotlin::{
    config::{GenerationTarget, GeneratorConfig},
    generators::MobileGenerator,
    package_detector::PackageSource,
    parse_module_schema,
};

#[derive(Subcommand, Debug)]
pub enum KotlinAction {
    /// Generate Kotlin Multiplatform Mobile code from a module's schema.
    Generate {
        /// Module identifier — either a `metaphor.yaml` project name (e.g.
        /// `bersihir-service`) or the `module:` value declared inside an
        /// `index.model.yaml` (e.g. `bersihir`). Auto-detected from CWD when
        /// invoked from inside a workspace project directory.
        module: Option<String>,

        /// Module base path. Legacy `--module-path` for non-workspace layouts;
        /// when running inside a workspace this is only consulted as a fallback.
        #[arg(long, default_value = "libs/modules")]
        module_path: PathBuf,

        /// Workspace project name to write generated code to (resolves to
        /// `<project>/shared/src/commonMain/kotlin`). Mutually exclusive with
        /// `--output-path`.
        #[arg(short, long, conflicts_with = "output_path")]
        output: Option<String>,

        /// Raw filesystem path to write generated code to. Mutually exclusive
        /// with `--output`.
        #[arg(long, conflicts_with = "output")]
        output_path: Option<PathBuf>,

        /// Kotlin package name (auto-detects from project if not provided).
        #[arg(short, long)]
        package: Option<String>,

        /// Generation targets (comma-separated). Pass `all` for everything.
        #[arg(short, long, default_value = "all", value_delimiter = ',')]
        target: Vec<GenerationTarget>,

        /// Skip files that already exist on disk.
        #[arg(long)]
        skip_existing: bool,

        /// Do not recursively generate code for transitive schema-module
        /// dependencies (default: deps are generated alongside the primary).
        #[arg(long)]
        no_deps: bool,

        /// Verbose output.
        #[arg(short, long)]
        verbose: bool,
    },
}

pub fn run(action: KotlinAction) -> Result<()> {
    match action {
        KotlinAction::Generate {
            module,
            module_path,
            output,
            output_path,
            package,
            target,
            skip_existing,
            no_deps,
            verbose,
        } => run_generate(
            module,
            module_path,
            output,
            output_path,
            package,
            target,
            skip_existing,
            no_deps,
            verbose,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_generate(
    module: Option<String>,
    module_path: PathBuf,
    output: Option<String>,
    output_path: Option<PathBuf>,
    package: Option<String>,
    target: Vec<GenerationTarget>,
    skip_existing: bool,
    no_deps: bool,
    verbose: bool,
) -> Result<()> {
    let cwd = std::env::current_dir().context("getting current dir")?;
    let workspace = Workspace::from_cwd(&cwd);

    println!(
        "{}",
        format!("📱 Backbone Mobilegen v{}\n", env!("CARGO_PKG_VERSION")).bright_blue()
    );

    // Resolve MODULE: explicit arg, or auto-detect from CWD via workspace.
    let module: String = if let Some(m) = module.filter(|s| !s.is_empty()) {
        m
    } else {
        let ws = workspace.as_ref().context(
            "no <MODULE> arg given and not inside a Metaphor workspace. \
             Run from a project dir with a metaphor.yaml in its tree, or pass MODULE explicitly.",
        )?;
        let project = ws.project_for_cwd(&cwd).with_context(|| {
            let names: Vec<String> = ws.projects().iter().map(|p| p.name.clone()).collect();
            format!(
                "no <MODULE> arg given and CWD does not match any workspace project. \
                 Run from inside a project dir, or pass MODULE explicitly. \
                 Available projects: {}",
                if names.is_empty() {
                    "(metaphor.yaml has no projects)".to_string()
                } else {
                    names.join(", ")
                }
            )
        })?;
        if verbose {
            eprintln!(
                "{} (auto-detected from CWD) {}",
                "Module:".dimmed(),
                project.name.cyan()
            );
        }
        project.name.clone()
    };

    if let Some(ws) = &workspace {
        if verbose {
            eprintln!(
                "{} {}",
                "Workspace:".dimmed(),
                ws.root.display().to_string().cyan()
            );
        }
    }

    // --- Resolve primary schema ---
    let primary_schema = resolve_schema_dir(&cwd, &module_path, &module, workspace.as_ref())
        .with_context(|| {
            format!(
                "could not locate schema for module '{}': tried workspace lookup, --module-path={}, and ./schema",
                module,
                module_path.display()
            )
        })?;

    // --- Translate MODULE arg from project name to schema `module:` value ---
    // The Kotlin generator uses this string in package paths, so we want the
    // schema's declared name (`bersihir`) rather than a project name with
    // hyphens (`bersihir-service`) which would be an invalid Kotlin package.
    let module_for_codegen = read_index_module_name(&primary_schema).unwrap_or_else(|| module.clone());
    if verbose && module_for_codegen != module {
        eprintln!(
            "{} '{}' → '{}' (from index.model.yaml)",
            "Module name:".dimmed(),
            module.cyan(),
            module_for_codegen.cyan()
        );
    }

    // --- Resolve output destination ---
    // --output PROJECT_NAME → workspace lookup (must succeed)
    // --output-path PATH    → raw filesystem path (no lookup)
    // neither                → falls through to GeneratorConfig's default
    //                          (apps/mobileapp/shared/src/commonMain)
    let resolved_output: Option<PathBuf> = match (output.as_deref(), output_path.as_deref()) {
        (Some(name), _) => {
            let ws = workspace.as_ref().context(
                "--output requires being inside a Metaphor workspace (directory with metaphor.yaml). \
                 Use --output-path for a raw filesystem path instead.",
            )?;
            Some(ws.resolve_output(Path::new(name)).with_context(|| {
                format!(
                    "--output '{}' did not match any workspace project. \
                     Available projects: {}. Use --output-path for a raw filesystem path.",
                    name,
                    project_names_for_error(ws)
                )
            })?)
        }
        (None, Some(path)) => Some(path.to_path_buf()),
        (None, None) => None,
    };

    if verbose {
        eprintln!(
            "{} {}",
            "Primary schema:".dimmed(),
            primary_schema.display().to_string().cyan()
        );
        if let Some(out) = &resolved_output {
            eprintln!(
                "{} {}",
                "Output:".dimmed(),
                out.display().to_string().cyan()
            );
        }
    }

    // --- Generate primary ---
    generate_one(
        &cwd,
        &module_for_codegen,
        &primary_schema,
        resolved_output.as_deref(),
        package.as_deref(),
        &target,
        skip_existing,
        verbose,
    )?;

    if no_deps {
        return Ok(());
    }

    // --- Discover & generate transitive deps ---
    let mut deps = discover_external_modules(&primary_schema)?;
    if let Some(ws) = &workspace {
        // Augment with metaphor.yaml depends_on entries — translated from
        // project names to schema-module names by reading each dep project's
        // index file. This catches deps that are declared in metaphor.yaml but
        // not referenced via `external_imports` in the schema YAML.
        if let Some(primary_project) = ws.project_by_name(&module).cloned().or_else(|| {
            ws.projects()
                .iter()
                .find(|p| ws.schema_dir_for(&module).map(|d| d.starts_with(ws.project_path(p))).unwrap_or(false))
                .cloned()
        }) {
            for dep_name in &primary_project.depends_on {
                if let Some(dep_schema) = ws.schema_dir_for(dep_name) {
                    if let Some(module_name) = read_index_module_name(&dep_schema) {
                        deps.insert(module_name);
                    }
                }
            }
        }
    }

    // Drop self-references — both the raw arg and the resolved schema-module name.
    deps.remove(&module);
    deps.remove(&module_for_codegen);
    if let Some(ws) = &workspace {
        // Also drop entries that resolve to the same primary schema dir.
        deps.retain(|d| {
            ws.schema_dir_for(d)
                .map(|s| s != primary_schema)
                .unwrap_or(true)
        });
    }

    if deps.is_empty() {
        return Ok(());
    }

    println!(
        "\n{} {} transitive module dep(s): {}",
        "→".bright_yellow(),
        deps.len(),
        deps.iter().cloned().collect::<Vec<_>>().join(", ").cyan()
    );

    for dep in deps {
        match resolve_schema_dir(&cwd, &module_path, &dep, workspace.as_ref()) {
            Ok(dep_schema) => {
                println!(
                    "\n{} {}",
                    "▸ Generating module:".bright_yellow(),
                    dep.cyan()
                );
                if let Err(e) = generate_one(
                    &cwd,
                    &dep,
                    &dep_schema,
                    resolved_output.as_deref(),
                    package.as_deref(),
                    &target,
                    skip_existing,
                    verbose,
                ) {
                    eprintln!(
                        "{} module '{}' failed: {}",
                        "⚠".yellow(),
                        dep,
                        e
                    );
                }
            }
            Err(e) => {
                eprintln!(
                    "{} skipping '{}' — {}",
                    "⚠".yellow(),
                    dep,
                    e
                );
            }
        }
    }

    Ok(())
}

/// Run a single module's Kotlin generation and print its summary. Mirrors the
/// output formatting of the original single-module command so users see the
/// same per-module breakdown they're used to.
#[allow(clippy::too_many_arguments)]
fn generate_one(
    cwd: &Path,
    module: &str,
    schema_dir: &Path,
    output: Option<&Path>,
    package: Option<&str>,
    target: &[GenerationTarget],
    skip_existing: bool,
    verbose: bool,
) -> Result<()> {
    let config = GeneratorConfig {
        module: module.to_string(),
        app: "mobileapp".to_string(),
        output: output.map(|p| p.to_path_buf()),
        package: package.map(|s| s.to_string()),
        target: target.to_vec(),
        // Kept for the parts of GeneratorConfig that still read it; not used
        // for schema location now that we resolve it ourselves.
        module_path: PathBuf::from("libs/modules"),
        skip_existing,
        verbose,
    };

    if verbose {
        eprintln!("{}: {:?}", "Config".cyan(), config);
    }

    println!(
        "{} {}",
        "Reading schema from".dimmed(),
        schema_dir.display().to_string().cyan()
    );

    let schema = parse_module_schema(schema_dir, module)
        .map_err(|e| anyhow::anyhow!("parsing module schema: {e}"))?;

    println!(
        "{} {} models, {} enums",
        "Found".green(),
        schema.models.len(),
        schema.enums.len()
    );

    if schema.models.is_empty() && schema.enums.is_empty() {
        println!(
            "{} no models — nothing to generate",
            "·".dimmed()
        );
        return Ok(());
    }

    let package_info = config.package_info();
    let source_display = match &package_info.source {
        PackageSource::GradleNamespace(path) => format!("Gradle namespace ({})", path.display()),
        PackageSource::SqlDelightPackage(path) => format!("SQLDelight package ({})", path.display()),
        PackageSource::ExistingKotlinFiles => "existing Kotlin files".to_string(),
        PackageSource::Default => "default".to_string(),
    };

    println!(
        "{} {} ({})",
        "Using package".dimmed(),
        package_info.base_package.cyan(),
        source_display.dimmed()
    );

    let mut generator = MobileGenerator::new(&package_info.base_package)
        .map_err(|e| anyhow::anyhow!("creating generator: {e}"))?;
    generator.skip_existing = config.skip_existing;

    let output_dir_raw = config.output_path();
    let output_dir = if output_dir_raw.is_absolute() {
        output_dir_raw
    } else {
        cwd.join(output_dir_raw)
    };

    println!(
        "{} {}",
        "Generating code to".dimmed(),
        output_dir.display().to_string().cyan()
    );

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} [{elapsed_precise}] {msg}")
            .unwrap(),
    );
    pb.enable_steady_tick(Duration::from_millis(100));
    pb.set_message("generating Kotlin code...");

    let targets: Vec<GenerationTarget> = config.targets();
    let result = generator
        .generate(&schema, &targets, &output_dir)
        .map_err(|e| {
            pb.finish_and_clear();
            anyhow::anyhow!("generation failed: {e}")
        })?;

    pb.finish_with_message("Done!");

    println!("\n{}", "=== Generation Summary ===".bright_cyan());
    println!(
        "{} {} files generated",
        "✓".green(),
        result.total_generated()
    );

    if !result.skipped_files.is_empty() {
        let reason = if config.skip_existing {
            "skipped (--skip-existing or // <<< CUSTOM marker)"
        } else {
            "skipped (contain // <<< CUSTOM marker)"
        };
        println!("{} {} files {}", "⚠".yellow(), result.skipped_files.len(), reason);
        if config.verbose {
            for path in &result.skipped_files {
                println!("  → {}", path.display());
            }
        }
    }

    if !result.stale_deleted_files.is_empty() {
        println!(
            "{} {} stale files removed (no longer generated by current schema)",
            "🗑".yellow(),
            result.stale_deleted_files.len()
        );
        if config.verbose {
            for path in &result.stale_deleted_files {
                println!("  ✗ {}", path.display());
            }
        }
    }

    if result.entities_count > 0 {
        println!("  • {} entities", result.entities_count);
    }
    if result.enums_count > 0 {
        println!("  • {} enums", result.enums_count);
    }
    if result.repositories_count > 0 {
        println!("  • {} repositories", result.repositories_count);
    }
    if result.usecases_count > 0 {
        println!("  • {} use cases", result.usecases_count);
    }
    if result.services_count > 0 {
        println!("  • {} services", result.services_count);
    }
    if result.mappers_count > 0 {
        println!("  • {} mappers", result.mappers_count);
    }
    if result.validators_count > 0 {
        println!("  • {} validators", result.validators_count);
    }
    if result.api_clients_count > 0 {
        println!("  • {} API clients", result.api_clients_count);
    }
    if result.viewmodels_count > 0 {
        println!("  • {} ViewModels", result.viewmodels_count);
    }
    if result.components_count > 0 {
        println!("  • {} components", result.components_count);
    }

    println!(
        "\n{} {}",
        "Output:".dimmed(),
        output_dir.display().to_string().cyan()
    );
    println!(
        "{} {}",
        "Package:".dimmed(),
        package_info.base_package.cyan()
    );

    Ok(())
}

/// Try multiple strategies to find a schema directory for `identifier`.
///
/// Order:
/// 1. Workspace lookup (matches `metaphor.yaml` project name OR a project's
///    schema `module:` field).
/// 2. `<module-path>/<identifier>/schema` (legacy, backwards-compatible).
/// 3. `./schema` when CWD basename matches `identifier`.
/// 4. `./<identifier>/schema` direct.
fn resolve_schema_dir(
    cwd: &Path,
    module_path_hint: &Path,
    identifier: &str,
    workspace: Option<&Workspace>,
) -> Result<PathBuf> {
    if let Some(ws) = workspace {
        if let Some(dir) = ws.schema_dir_for(identifier) {
            return Ok(dir);
        }
    }

    let absolute_hint = if module_path_hint.is_absolute() {
        module_path_hint.to_path_buf()
    } else {
        cwd.join(module_path_hint)
    };
    let legacy = absolute_hint.join(identifier).join("schema");
    if legacy.is_dir() {
        return Ok(legacy);
    }

    if cwd.file_name().and_then(|s| s.to_str()) == Some(identifier) {
        let cand = cwd.join("schema");
        if cand.is_dir() {
            return Ok(cand);
        }
    }

    let cand = cwd.join(identifier).join("schema");
    if cand.is_dir() {
        return Ok(cand);
    }

    anyhow::bail!(
        "no schema dir for '{}' (tried workspace, {}, ./schema, ./{}/schema)",
        identifier,
        legacy.display(),
        identifier
    )
}

/// Walk every `.yaml`/`.yml` file under `schema_dir` and collect every
/// `external_imports[*].module` value. The schema parser silently drops
/// `external_imports` today, so we re-read the raw YAML here.
fn discover_external_modules(schema_dir: &Path) -> Result<BTreeSet<String>> {
    #[derive(serde::Deserialize)]
    struct ExternalImport {
        module: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct WithImports {
        #[serde(default)]
        external_imports: Vec<ExternalImport>,
    }

    let mut deps = BTreeSet::new();
    for entry in walkdir::WalkDir::new(schema_dir).into_iter().flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if ext != "yaml" && ext != "yml" {
            continue;
        }
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !content.contains("external_imports") {
            continue;
        }
        let parsed: WithImports = match serde_yaml::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };
        for imp in parsed.external_imports {
            if let Some(m) = imp.module {
                deps.insert(m);
            }
        }
    }
    Ok(deps)
}

/// Comma-separated list of project names from a workspace, for error hints.
/// Filtered to projects that actually have a schema dir (i.e. plausible
/// targets for code generation).
fn project_names_for_error(ws: &Workspace) -> String {
    let names: Vec<String> = ws
        .projects()
        .iter()
        .map(|p| p.name.clone())
        .collect();
    if names.is_empty() {
        "(none in metaphor.yaml)".to_string()
    } else {
        names.join(", ")
    }
}

/// Read the `module:` field from `<schema_dir>/models/index.model.yaml`.
fn read_index_module_name(schema_dir: &Path) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Header {
        module: Option<String>,
    }
    let path = schema_dir.join("models/index.model.yaml");
    let content = std::fs::read_to_string(&path).ok()?;
    let header: Header = serde_yaml::from_str(&content).ok()?;
    header.module
}
