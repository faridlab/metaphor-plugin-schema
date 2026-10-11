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
}
