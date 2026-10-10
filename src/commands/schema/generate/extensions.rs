//! The extension-file layout's write-time pass (ADR-0031).
//!
//! A module that uses the layout (`generators: extensions: true`) keeps no hand code in a
//! generated file. Two things then have to be declared by the generator, because they live on
//! disk rather than in the schema:
//!
//! - every hand-written (`user_owned`) file beside a generated `mod.rs` or `lib.rs` is declared
//!   there (`pub mod <stem>;`), and a `mod.ext.rs` in the directory is pulled in with
//!   `include!`, so its `pub use` lines resolve against the same directory;
//! - an item file `<name>.rs` with a `<name>.ext.rs` beside it declares that extension and
//!   re-exports it (`#[path = "<name>.ext.rs"] mod <name>_ext; pub use <name>_ext::*;`).
//!
//! `lib.ext.rs` is wired by the module generator itself, since it extends the module struct.

use std::path::{Path, PathBuf};

use globset::GlobSet;

use crate::generators::GeneratedOutput;

/// Apply the extension layout to the generated files of a module rooted at `output_dir`.
pub(super) fn weave_extensions(generated: &mut GeneratedOutput, output_dir: &Path, user_owned: &GlobSet) {
    let generated_paths: std::collections::HashSet<PathBuf> = generated.files.keys().cloned().collect();
    for (path, content) in generated.files.iter_mut() {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if !name.ends_with(".rs") || !path.starts_with("src") {
            continue;
        }
        let dir = path.parent().unwrap_or(Path::new(""));
        if name == "mod.rs" || name == "lib.rs" {
            let hand = hand_modules(output_dir, dir, &generated_paths, user_owned);
            let missing: Vec<&String> = hand.iter().filter(|m| !declares(content, m)).collect();
            if !missing.is_empty() {
                content.push_str("\n// Hand-written modules in this directory (user_owned; ADR-0031).\n");
                for m in missing {
                    content.push_str(&format!("pub mod {m};\n"));
                }
            }
            if name == "mod.rs" && output_dir.join(dir).join("mod.ext.rs").is_file() {
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

/// The hand-written modules in `dir`: `.rs` files (other than `mod.rs`, `lib.rs`, `main.rs`
/// and extension files) and subdirectories with a `mod.rs`, that the run does not generate and
/// that `user_owned` covers. Sorted.
fn hand_modules(output_dir: &Path, dir: &Path, generated: &std::collections::HashSet<PathBuf>, user_owned: &GlobSet) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(output_dir.join(dir)) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let fname = entry.file_name().to_string_lossy().to_string();
        let (rel, stem) = if entry.path().is_dir() {
            (dir.join(&fname).join("mod.rs"), fname.clone())
        } else if fname.ends_with(".rs")
            && !fname.ends_with(".ext.rs")
            && !matches!(fname.as_str(), "mod.rs" | "lib.rs" | "main.rs")
        {
            (dir.join(&fname), fname.trim_end_matches(".rs").to_string())
        } else {
            continue;
        };
        if !output_dir.join(&rel).is_file() || generated.contains(&rel) {
            continue;
        }
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if user_owned.is_match(&rel_str) && is_ident(&stem) {
            out.push(stem);
        }
    }
    out.sort();
    out
}

fn is_ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !s.starts_with(|c: char| c.is_ascii_digit())
}

/// Whether `content` already declares module `m` (any visibility).
fn declares(content: &str, m: &str) -> bool {
    content.lines().any(|l| {
        let l = l.trim();
        let l = l.strip_prefix("pub(crate) ").or_else(|| l.strip_prefix("pub ")).unwrap_or(l);
        l == format!("mod {m};")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use globset::{Glob, GlobSetBuilder};

    #[test]
    fn hand_files_are_declared_and_extensions_attached_for_a_module_using_the_layout() {
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
        assert!(m.contains("pub mod period_close_service;\npub mod verbs;\n"), "{m}");
        assert!(!m.contains("pub mod stray;"), "a file user_owned does not cover is not declared");
        assert_eq!(m.matches("pub mod user_service;").count(), 1, "a generated module is not declared twice");
        assert!(m.ends_with("include!(\"mod.ext.rs\");\n"), "{m}");
        let u = &generated.files[&PathBuf::from("src/application/service/user_service.rs")];
        assert!(u.contains("#[path = \"user_service.ext.rs\"]\nmod user_service_ext;\npub use user_service_ext::*;"), "{u}");
    }
}
