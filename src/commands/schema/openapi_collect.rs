//! `metaphor schema openapi-collect` — vendor composed modules' generated
//! OpenAPI specs into a consumer app.
//!
//! A consumer (e.g. a `backend-service`) composes several modules' routers but
//! only serves a single Swagger UI. Each module generates its own
//! `schema/openapi/openapi.yaml`; this command copies them into the app so they
//! can be embedded (`include_str!`) and offered as additional Swagger specs.
//!
//! It must be a *copy into the app* (not a reference) because the service's build
//! context is typically just the app directory — `modules/` isn't reachable at
//! build time.
//!
//! Driven by an `openapi_vendor` section in the app's `metaphor.codegen.yaml`:
//!
//! ```yaml
//! openapi_vendor:
//!   dest: src/presentation/http/openapi   # relative to the app root
//!   modules: [backbone-sapiens, backbone-bucket]   # optional; default = depends_on
//! ```
//!
//! Each module's spec lands at `<dest>/<short>.openapi.yaml`, where `<short>` is
//! the module name with any `backbone-` prefix stripped (e.g. `sapiens`).

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

use crate::commands::workspace::Workspace;
use super::manifest;

/// Collect composed modules' OpenAPI specs into the consumer app declared by
/// `module` (or auto-detected from the current directory).
pub(super) fn execute_openapi_collect(module: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    let ws = Workspace::from_cwd(&cwd)
        .context("not inside a metaphor workspace (no metaphor.yaml found in any parent)")?;

    // Resolve the consumer project: explicit name, else the project owning cwd.
    let project = match module.as_deref() {
        Some(name) => ws
            .project_by_name(name)
            .with_context(|| format!("no project named '{name}' in metaphor.yaml"))?,
        None => ws
            .project_for_cwd(&cwd)
            .context("could not detect the current project from the working directory; pass the app name explicitly")?,
    };
    let app_path = ws.project_path(project);

    let vendor = manifest::load_openapi_vendor(&app_path)?.context(
        "no `openapi_vendor` section in the app's metaphor.codegen.yaml. Add:\n\n\
         openapi_vendor:\n  \
           dest: src/presentation/http/openapi\n  \
           modules: [backbone-sapiens, backbone-bucket]\n",
    )?;

    // Modules to vendor: explicit list, else the app's declared dependencies.
    let modules: Vec<String> = if vendor.modules.is_empty() {
        project.depends_on.clone()
    } else {
        vendor.modules.clone()
    };
    if modules.is_empty() {
        anyhow::bail!(
            "no modules to collect — set `openapi_vendor.modules` or `depends_on` for '{}'",
            project.name
        );
    }

    let dest_dir = app_path.join(&vendor.dest);
    fs::create_dir_all(&dest_dir)
        .with_context(|| format!("failed to create dest dir {}", dest_dir.display()))?;

    println!(
        "Collecting module OpenAPI specs into {}:",
        vendor.dest
    );

    let mut copied = 0usize;
    let mut skipped = 0usize;
    for m in &modules {
        // Resolve the module's source path: a declared project, else modules/<m>.
        let module_path: PathBuf = match ws.project_by_name(m) {
            Some(p) => ws.project_path(p),
            None => ws.root.join("modules").join(m),
        };
        let src = module_path.join("schema/openapi/openapi.yaml");
        if !src.exists() {
            eprintln!(
                "  \u{26a0} {m}: no schema/openapi/openapi.yaml — generate it first \
                 (enable `openapi` then `metaphor schema generate --target openapi --force`). Skipped."
            );
            skipped += 1;
            continue;
        }
        let short = m.strip_prefix("backbone-").unwrap_or(m);
        let dest = dest_dir.join(format!("{short}.openapi.yaml"));
        fs::copy(&src, &dest)
            .with_context(|| format!("failed to copy {} -> {}", src.display(), dest.display()))?;
        let shown = dest.strip_prefix(&app_path).unwrap_or(&dest);
        let bytes = fs::metadata(&dest).map(|md| md.len()).unwrap_or(0);
        println!("  \u{2713} {m} \u{2192} {} ({bytes} bytes)", shown.display());
        copied += 1;
    }

    println!(
        "Collected {copied} spec(s), {skipped} skipped. Rebuild the app to embed the updated specs."
    );
    Ok(())
}
