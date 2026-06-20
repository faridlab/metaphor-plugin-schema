//! Codegen manifest (`metaphor.codegen.yaml`) loading.
//!
//! Declares which files the generator must NEVER touch (`user_owned`). The
//! manifest lives at the codegen output root; consumers adopt it by dropping
//! a `metaphor.codegen.yaml` next to their generated tree. Repos that haven't
//! adopted it get an empty [`GlobSet`] and the legacy behavior — generated
//! files are written unconditionally.

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;
use std::fs;
use std::path::Path;

/// Codegen manifest loaded from `metaphor.codegen.yaml` at the output root.
///
/// Files matching any `user_owned` glob are skipped entirely — the generator
/// does not read, merge, or write them. This is the contract that lets
/// application code own files inside the generator's output tree without
/// losing them on regen.
#[derive(Debug, Default, Deserialize)]
struct CodegenManifest {
    #[serde(default)]
    user_owned: Vec<String>,
    #[serde(default)]
    openapi_vendor: Option<OpenapiVendor>,
}

/// `openapi_vendor` section of `metaphor.codegen.yaml`. Declares where a consumer
/// app wants its composed modules' generated OpenAPI specs copied to, so they can
/// be embedded and served via Swagger UI. Consumed by `schema openapi-collect`.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct OpenapiVendor {
    /// Destination directory for the vendored specs, relative to the app root.
    pub dest: String,
    /// Module names to vendor (e.g. `backbone-sapiens`). Empty = the app's
    /// `depends_on` from `metaphor.yaml`.
    #[serde(default)]
    pub modules: Vec<String>,
}

/// Load the `openapi_vendor` section from `metaphor.codegen.yaml` in `output_dir`,
/// or `None` when the manifest or the section is absent.
pub(super) fn load_openapi_vendor(output_dir: &Path) -> Result<Option<OpenapiVendor>> {
    let manifest_path = output_dir.join("metaphor.codegen.yaml");
    if !manifest_path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
    let manifest: CodegenManifest = serde_yaml::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
    Ok(manifest.openapi_vendor)
}

/// Load `metaphor.codegen.yaml` from `output_dir` and compile its
/// `user_owned` patterns into a [`GlobSet`]. Returns an empty `GlobSet` when
/// the manifest is missing — preserving today's behavior for repos that
/// haven't adopted the manifest yet.
pub(super) fn load_user_owned_globs(output_dir: &Path) -> Result<GlobSet> {
    let manifest_path = output_dir.join("metaphor.codegen.yaml");
    if !manifest_path.exists() {
        return Ok(GlobSet::empty());
    }
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
    let manifest: CodegenManifest = serde_yaml::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
    let mut builder = GlobSetBuilder::new();
    for pattern in &manifest.user_owned {
        let glob = Glob::new(pattern).with_context(|| {
            format!("Invalid user_owned glob in metaphor.codegen.yaml: {}", pattern)
        })?;
        builder.add(glob);
    }
    builder
        .build()
        .with_context(|| "Failed to compile user_owned glob set")
}
