//! The extension-file layout's write-time pass (ADR-0031).
//!
//! A module that uses the layout (`generators: extensions: true`) keeps no hand code in a
//! generated file. Two things then have to be declared by the generator, because they live on
//! disk rather than in the schema:
//!
//! - a directory's `mod.ext.rs` is pulled into its generated `mod.rs` (or `lib.rs` for `src/`)
//!   with `include!`, so the `pub mod` and `pub use` lines it holds for the directory's
//!   hand-written files resolve against that directory. Declarations stay explicit there: a
//!   hand file nothing declares is not compiled, and a declaration keeps its attributes;
//! - an item file `<name>.rs` with a `<name>.ext.rs` beside it declares that extension and
//!   re-exports it (`#[path = "<name>.ext.rs"] mod <name>_ext; pub use <name>_ext::*;`).
//!
//! `lib.ext.rs` is wired by the module generator itself, since it extends the module struct.

use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use globset::GlobSet;

use crate::generators::GeneratedOutput;

/// Apply the extension layout to the generated files of a module rooted at `output_dir`.
pub(super) fn weave_extensions(generated: &mut GeneratedOutput, output_dir: &Path, _user_owned: &GlobSet) {
    for (path, content) in generated.files.iter_mut() {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if !name.ends_with(".rs") || !path.starts_with("src") {
            continue;
        }
        let dir = path.parent().unwrap_or(Path::new(""));
        if name == "mod.rs" || name == "lib.rs" {
            if output_dir.join(dir).join("mod.ext.rs").is_file() {
                content.push_str("\n// This directory's extension (hand-written; ADR-0031).\ninclude!(\"mod.ext.rs\");\n");
            }
        } else {
            let stem = &name[..name.len() - 3];
            let ext = format!("{stem}.ext.rs");
            if output_dir.join(dir).join(&ext).is_file() {
                content.push_str(&format!(
                    "\n// This file's extension (hand-written; ADR-0031).\n#[path = \"{ext}\"]\nmod {stem}_ext;\npub use {stem}_ext::*;\n"
                ));
            }
        }
    }
}

/// The generated Rust files whose copy on disk still holds hand code in a CUSTOM zone, in any
/// of the merge's marker forms (paired, `START >>>`/`END >>>`, a bare marker running to a blank
/// line, an inline tag): those where merging the disk copy into fresh output would keep
/// anything. A layout module's generated files are written whole, so such a file must not be
/// overwritten until its hand code has moved to an extension file. Run before
/// [`strip_custom_markers`], while the fresh output still carries the zones the merge anchors on.
pub(super) fn files_with_hand_code(generated: &GeneratedOutput, output_dir: &Path, user_owned: &GlobSet) -> Vec<String> {
    let mut out = Vec::new();
    for (path, content) in &generated.files {
        let rel = path.to_string_lossy().replace('\\', "/");
        if !rel.ends_with(".rs") || rel.ends_with(".ext.rs") || user_owned.is_match(&rel) {
            continue;
        }
        let disk = output_dir.join(path);
        if !disk.is_file() {
            continue;
        }
        if let Ok(existing) = std::fs::read_to_string(&disk) {
            if !super::super::merge::hand_code_regions(&existing, content).is_empty() {
                out.push(rel);
            }
        }
    }
    out.sort();
    out
}

/// Remove the CUSTOM marker lines from a layout module's generated files: its hand code lives
/// in extension files, so a generated file carries no zone to edit (ADR-0031). A zone the
/// generator itself fills is never emitted between markers, so only empty markers are removed.
pub(super) fn strip_custom_markers(generated: &mut GeneratedOutput) {
    for (path, content) in generated.files.iter_mut() {
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        if !content.contains("CUSTOM") {
            continue;
        }
        let kept: Vec<&str> = content
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                !(t.starts_with("// <<< CUSTOM") || t.starts_with("// END CUSTOM"))
            })
            .collect();
        let mut out = kept.join("\n");
        if content.ends_with('\n') {
            out.push('\n');
        }
        *content = out;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use globset::{Glob, GlobSetBuilder};

    #[test]
    fn a_directorys_extension_is_included_and_an_items_extension_attached() {
        let root = tempfile::tempdir().unwrap();
        let svc = root.path().join("src/application/service");
        std::fs::create_dir_all(svc.join("verbs")).unwrap();
        for f in ["period_close_service.rs", "user_service.ext.rs", "mod.ext.rs", "verbs/mod.rs", "stray.rs"] {
            std::fs::write(svc.join(f), "").unwrap();
        }
        let mut owned = GlobSetBuilder::new();
        for g in ["src/application/service/period_close_service.rs", "src/application/service/verbs/**", "src/**/*.ext.rs"] {
            owned.add(Glob::new(g).unwrap());
        }
        let owned = owned.build().unwrap();
        let mut generated = GeneratedOutput::new();
        generated.add_file(PathBuf::from("src/application/service/mod.rs"), "pub mod user_service;\n");
        generated.add_file(PathBuf::from("src/application/service/user_service.rs"), "pub type UserService = ();\n");

        weave_extensions(&mut generated, root.path(), &owned);
        let m = &generated.files[&PathBuf::from("src/application/service/mod.rs")];
        assert!(!m.contains("pub mod period_close_service;") && !m.contains("pub mod stray;"), "nothing is declared implicitly: {m}");
        assert!(m.ends_with("include!(\"mod.ext.rs\");\n"), "{m}");
        let u = &generated.files[&PathBuf::from("src/application/service/user_service.rs")];
        assert!(u.contains("#[path = \"user_service.ext.rs\"]\nmod user_service_ext;\npub use user_service_ext::*;"), "{u}");
    }

    #[test]
    fn a_generated_file_still_holding_hand_code_in_any_zone_form_is_found() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src/exports")).unwrap();
        let fresh = "pub mod types;\n// <<< CUSTOM SERVICES START >>>\n// <<< CUSTOM SERVICES END >>>\n";
        std::fs::write(root.path().join("src/exports/services.rs"), "pub mod types;\n// <<< CUSTOM SERVICES START >>>\npub trait QueryService {}\n// <<< CUSTOM SERVICES END >>>\n").unwrap();
        std::fs::write(root.path().join("src/exports/events.rs"), fresh).unwrap();
        let mut generated = GeneratedOutput::new();
        generated.add_file(PathBuf::from("src/exports/services.rs"), fresh);
        generated.add_file(PathBuf::from("src/exports/events.rs"), fresh);
        let none = globset::GlobSetBuilder::new().build().unwrap();
        assert_eq!(files_with_hand_code(&generated, root.path(), &none), vec!["src/exports/services.rs".to_string()]);
    }

    #[test]
    fn a_layout_modules_generated_file_carries_no_custom_zone() {
        let mut generated = GeneratedOutput::new();
        generated.add_file(PathBuf::from("src/lib.rs"), "pub mod domain;\n// <<< CUSTOM MODULES\n// END CUSTOM\n    // <<< CUSTOM FIELDS\n    // END CUSTOM\n/// Extend in the `// <<< CUSTOM` zone.\n");
        generated.add_file(PathBuf::from("migrations/x.up.sql"), "-- <<< CUSTOM\n");
        strip_custom_markers(&mut generated);
        assert_eq!(generated.files[&PathBuf::from("src/lib.rs")], "pub mod domain;\n/// Extend in the `// <<< CUSTOM` zone.\n");
        assert_eq!(generated.files[&PathBuf::from("migrations/x.up.sql")], "-- <<< CUSTOM\n", "only Rust files");
    }
}
