//! `metaphor schema doctor` — find drift between hand-written aggregator
//! files and the current generator output.
//!
//! Why this exists: consumers often keep hand-written composition files
//! (e.g. `bersihir_core.rs`, `bersihir_module.rs`) that explicitly list
//! `create_<model>_routes(...)` for every model with a handler. When a
//! model later toggles `config.generators.disabled: [handler, ...]` (or a
//! new model is added without handler), the generated handler vanishes
//! but those hand-written imports stay — and `cargo check` only surfaces
//! the breakage at the very end of a regen cycle.
//!
//! The doctor runs the same gating logic the generator does, then scans
//! each `user_owned` `.rs` file in `metaphor.codegen.yaml` for symbols
//! that reference handlers the schema says aren't being emitted. Output
//! is actionable: file + line + suggested remediation.
//!
//! Pure read-only — never writes.
//!
//! Exit codes:
//!   0  — no drift
//!   1  — drift found (the report is printed to stdout)
//!   2  — schema load/parse failure

use anyhow::{Context, Result};
use colored::Colorize;
use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::generators::{model_skips_target, GenerationTarget};
use crate::resolver::resolve_schema;

use super::discovery::{find_module_schema_path, find_schema_files};
use super::module_loader::build_module_schema;

/// The subset of `metaphor.codegen.yaml` both `doctor` and `undeclared` care
/// about: the hand-written files the generator promises never to touch.
#[derive(Debug, Default, Deserialize)]
pub(super) struct CodegenManifest {
    #[serde(default)]
    pub(super) user_owned: Vec<String>,
}

pub(super) fn execute_doctor(module: &str) -> Result<()> {
    println!("{} module: {}", "Doctor".green().bold(), module.cyan());

    let schema_path = find_module_schema_path(module)?;
    let schema_files = find_schema_files(&schema_path)?;

    if schema_files.is_empty() {
        println!("{}", "  No schema files found".yellow());
        return Ok(());
    }

    let (module_schema, parse_errors) = build_module_schema(module, &schema_files)?;
    if !parse_errors.is_empty() {
        for e in &parse_errors {
            eprintln!("  {} {}", "Parse error:".red().bold(), e);
        }
        anyhow::bail!("schema failed to parse — fix YAML errors before running doctor");
    }
    let resolved = resolve_schema(&module_schema).map_err(|errs| {
        let joined = errs
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n  ");
        anyhow::anyhow!("schema validation failed:\n  {}", joined)
    })?;

    // The consumer root is the parent of `schema/` (the model-yaml dir).
    // `find_module_schema_path` returns either `<root>/schema/models/` or
    // `<root>` depending on layout; walk up until we find `metaphor.codegen.yaml`
    // or hit the workspace root.
    let consumer_root = locate_consumer_root(&schema_path).unwrap_or_else(|| schema_path.clone());

    let manifest_path = consumer_root.join("metaphor.codegen.yaml");
    let manifest: CodegenManifest = if manifest_path.exists() {
        let raw = fs::read_to_string(&manifest_path)
            .with_context(|| format!("read {}", manifest_path.display()))?;
        serde_yaml::from_str(&raw).with_context(|| format!("parse {}", manifest_path.display()))?
    } else {
        CodegenManifest::default()
    };

    // Compile the user_owned globs so each finding can be tagged as "in a
    // hand-written file (your job)" vs "in a generator-emitted file (will
    // self-heal on next regen, but breaks `cargo check` right now)". This
    // makes the report actionable instead of just noisy.
    let user_owned_globs = compile_globs(&manifest.user_owned)?;

    // Build the forbidden-names map: for every model whose Handler is
    // disabled, enumerate the exact symbol names that the handler/auth/
    // state-machine generators would have emitted. Anything matching one of
    // these names in a hand-written or stale generator file is drift.
    //
    // This is the inverse of the "extract-then-classify" approach — there
    // are many `create_<thing>_routes` forms in the codebase (transition,
    // protected, stateless, ...) and only some of them are model-scoped, so
    // searching for known-bad names is strictly less noisy than parsing
    // every reference and trying to filter.
    let mut forbidden: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for model in &resolved.schema.models {
        if !model_skips_target(model, GenerationTarget::Handler) {
            continue;
        }
        let snake = crate::utils::to_snake_case(&model.name);
        for name in [
            format!("create_{}_routes", snake),
            format!("create_{}_read_routes", snake),
            format!("create_{}_write_routes", snake),
            format!("create_protected_{}_routes", snake),
            format!("create_{}_transition_routes", snake),
        ] {
            forbidden.insert(name, snake.clone());
        }
    }

    // Compile a single regex with alternation over the forbidden names.
    // Cheap to search per line; avoids re-scanning every line for every name.
    let route_re = if forbidden.is_empty() {
        None
    } else {
        let alternation = forbidden
            .keys()
            .map(|k| regex::escape(k))
            .collect::<Vec<_>>()
            .join("|");
        Some(
            Regex::new(&format!(r"\b({})\b", alternation))
                .context("compile forbidden-names regex")?,
        )
    };

    let mut findings: Vec<Finding> = Vec::new();
    let mut audited_files = 0usize;

    // Walk every .rs file under src/ and tests/. Catches both files declared
    // in user_owned and ones a developer hand-edited without declaring them
    // (a recurring pain — they look generated, until the user touches them).
    if let Some(re) = route_re.as_ref() {
        let scan_roots = ["src", "tests"];
        for root_name in &scan_roots {
            let root = consumer_root.join(root_name);
            if !root.exists() {
                continue;
            }
            for entry in WalkDir::new(&root).follow_links(false).into_iter().flatten() {
                let path = entry.path();
                if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                audited_files += 1;
                let rel = path.strip_prefix(&consumer_root).unwrap_or(path);
                let content = match fs::read_to_string(path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let is_user_owned = user_owned_globs.is_match(rel);
                for (lineno, line) in content.lines().enumerate() {
                    let trimmed = line.trim_start();
                    if trimmed.starts_with("//") {
                        continue;
                    }
                    for cap in re.captures_iter(line) {
                        let name = &cap[1];
                        let model = forbidden.get(name).cloned().unwrap_or_default();
                        findings.push(Finding {
                            file: rel.display().to_string(),
                            line: lineno + 1,
                            snippet: line.trim_end().to_string(),
                            model,
                            reason: DriftReason::HandlerDisabled,
                            owned_by_user: is_user_owned,
                        });
                    }
                }
            }
        }
    }

    println!("  audited {} .rs file(s)", audited_files);

    if findings.is_empty() {
        println!("  {} no drift detected", "✓".green().bold());
        return Ok(());
    }

    println!();
    println!(
        "{} {} reference(s) to handlers that won't be generated:",
        "Drift:".red().bold(),
        findings.len()
    );
    for f in &findings {
        let owner_tag = if f.owned_by_user {
            "user-owned".bold()
        } else {
            "generator-managed (re-emit will overwrite)".dimmed()
        };
        println!("  {}:{} [{}]", f.file.bold(), f.line, owner_tag);
        println!("    {}", f.snippet.dimmed());
        println!(
            "    → model `{}` opts out of handler generation",
            f.model
        );
    }

    let user_owned_drift = findings.iter().filter(|f| f.owned_by_user).count();
    let unowned_drift = findings.len() - user_owned_drift;
    println!();
    if user_owned_drift > 0 {
        println!(
            "{} {} reference(s) in user-owned files — delete the imports and `.merge(...)` calls above.",
            "Fix (you):".cyan().bold(),
            user_owned_drift,
        );
    }
    if unowned_drift > 0 {
        println!(
            "{} {} reference(s) in generator-managed files — likely stale from a pre-fix regen. \
             Run `metaphor schema generate -f` to refresh.",
            "Fix (regen):".cyan().bold(),
            unowned_drift,
        );
    }

    anyhow::bail!("found {} drift reference(s)", findings.len())
}

#[derive(Debug)]
struct Finding {
    file: String,
    line: usize,
    snippet: String,
    model: String,
    reason: DriftReason,
    owned_by_user: bool,
}

/// Compile the `user_owned:` glob list from a `metaphor.codegen.yaml` into a
/// matchable set. Shared with `schema undeclared`, which tests hand-written
/// files against the very same globs.
pub(super) fn compile_globs(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob =
            Glob::new(pattern).with_context(|| format!("Invalid glob in user_owned: {}", pattern))?;
        builder.add(glob);
    }
    builder.build().context("compile user_owned glob set")
}

#[derive(Debug, Clone, Copy)]
enum DriftReason {
    HandlerDisabled,
}

/// Walk upward from the schema dir until we find a `metaphor.codegen.yaml`
/// (the canonical signal that we've reached the consumer's root) or a
/// `metaphor.yaml` (the workspace root — stop there, manifest is missing).
pub(super) fn locate_consumer_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.canonicalize().ok()?;
    if cur.is_file() {
        cur.pop();
    }
    loop {
        if cur.join("metaphor.codegen.yaml").exists() {
            return Some(cur);
        }
        if cur.join("metaphor.yaml").exists() {
            // Workspace root — no consumer manifest above this.
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}
