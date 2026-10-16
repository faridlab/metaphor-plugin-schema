//! Generated source files nothing compiles.
//!
//! A generated `src/**.rs` file is part of the crate only when a chain of `mod`
//! declarations reaches it from `src/lib.rs` (or `src/main.rs`). A layer whose
//! index is hand-owned, or a generator that emits a file its own index never
//! names, leaves a file on disk that no build ever reads: it rots unchecked and
//! comes back on every regeneration after someone deletes it. Such a file is not
//! written; a copy an earlier run left behind is then a retired generated file,
//! which the write phase removes when git can restore it.
//!
//! A declaring file is read as it will be on disk after the run: the generated
//! text, plus the existing copy when CUSTOM zones are merged back into it, plus
//! the directory's hand-written `mod.ext.rs` (or `lib.ext.rs` for `src/`).

use globset::GlobSet;
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::generators::GeneratedOutput;

/// The generated `src/**.rs` files no declaration chain reaches from the crate root,
/// sorted. Empty when the module has no crate root at all, so a first generation into
/// an empty directory writes everything.
pub(super) fn unreachable_generated(
    generated: &GeneratedOutput,
    output_dir: &Path,
    user_owned: &GlobSet,
    merge_custom: bool,
) -> Vec<PathBuf> {
    let reach = Reach { generated, output_dir, user_owned, merge_custom };
    if !["src/lib.rs", "src/main.rs"].iter().any(|r| reach.text(Path::new(r)).is_some()) {
        return Vec::new();
    }
    let mut memo = HashMap::new();
    let mut out: Vec<PathBuf> = generated
        .files
        .keys()
        .filter(|p| is_module_source(p) && !user_owned.is_match(p))
        .filter(|p| !reach.reachable(p, &mut memo))
        .cloned()
        .collect();
    out.sort();
    out
}

/// A crate source file that needs a declaration: under `src/`, a `.rs` file, not an
/// extension file (its own generated neighbour pulls it in), not a crate root and not
/// a binary target Cargo finds by itself.
fn is_module_source(path: &Path) -> bool {
    let rel = path.to_string_lossy().replace('\\', "/");
    rel.starts_with("src/")
        && rel.ends_with(".rs")
        && !rel.ends_with(".ext.rs")
        && !rel.starts_with("src/bin/")
        && !matches!(rel.as_str(), "src/lib.rs" | "src/main.rs")
}

struct Reach<'a> {
    generated: &'a GeneratedOutput,
    output_dir: &'a Path,
    user_owned: &'a GlobSet,
    merge_custom: bool,
}

impl Reach<'_> {
    /// The file's text after this run, or `None` when it will not exist.
    fn text(&self, rel: &Path) -> Option<String> {
        let disk = || fs::read_to_string(self.output_dir.join(rel)).ok();
        match self.generated.files.get(rel) {
            // A user-owned file is never written; what is on disk stays.
            Some(_) if self.user_owned.is_match(rel) => disk(),
            // Merged CUSTOM zones keep declarations the generator does not emit.
            Some(text) if self.merge_custom => Some(match disk() {
                Some(old) => format!("{text}\n{old}"),
                None => text.clone(),
            }),
            Some(text) => Some(text.clone()),
            None => disk(),
        }
    }

    fn reachable(&self, rel: &Path, memo: &mut HashMap<PathBuf, bool>) -> bool {
        if let Some(&known) = memo.get(rel) {
            return known;
        }
        // Cut cycles: a file is unreached until a declaration proves otherwise.
        memo.insert(rel.to_path_buf(), false);
        let found = self.find_reach(rel, memo);
        memo.insert(rel.to_path_buf(), found);
        found
    }

    fn find_reach(&self, rel: &Path, memo: &mut HashMap<PathBuf, bool>) -> bool {
        let s = rel.to_string_lossy().replace('\\', "/");
        if matches!(s.as_str(), "src/lib.rs" | "src/main.rs") || s.starts_with("src/bin/") {
            return self.text(rel).is_some();
        }
        let Some(dir) = rel.parent() else { return false };
        let stem = rel.file_stem().and_then(|x| x.to_str()).unwrap_or_default();
        // `a/b/mod.rs` and `a/b.rs` are both the module `b`, declared from `a`.
        let (name, decl_dir) = if stem == "mod" {
            let Some(name) = dir.file_name().and_then(|x| x.to_str()) else { return false };
            let Some(up) = dir.parent() else { return false };
            (name.to_string(), up.to_path_buf())
        } else {
            (stem.to_string(), dir.to_path_buf())
        };
        let file_tail = rel.strip_prefix(&decl_dir).unwrap_or(rel).to_string_lossy().replace('\\', "/");
        for (declarer, ext) in declarers(&decl_dir) {
            let Some(mut text) = self.text(&declarer) else { continue };
            if let Some(more) = ext.and_then(|e| fs::read_to_string(self.output_dir.join(e)).ok()) {
                text.push('\n');
                text.push_str(&more);
            }
            if declares(&text, &name, &file_tail) && self.reachable(&declarer, memo) {
                return true;
            }
        }
        false
    }
}

/// The files that can declare a module living in `dir`, each with the hand-written
/// extension file whose lines are part of it.
fn declarers(dir: &Path) -> Vec<(PathBuf, Option<PathBuf>)> {
    if dir == Path::new("src") {
        return vec![
            (PathBuf::from("src/lib.rs"), Some(PathBuf::from("src/lib.ext.rs"))),
            (PathBuf::from("src/main.rs"), None),
        ];
    }
    let mut out = vec![(dir.join("mod.rs"), Some(dir.join("mod.ext.rs")))];
    if let (Some(up), Some(name)) = (dir.parent(), dir.file_name()) {
        out.push((up.join(format!("{}.rs", name.to_string_lossy())), None));
    }
    out
}

/// Whether `text` declares the module `name` (stored as `file_tail` relative to the
/// declaring directory): a `mod name;` line, a `#[path]` naming the file, or an
/// `include!` of it. Comment lines are ignored.
fn declares(text: &str, name: &str, file_tail: &str) -> bool {
    let code: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let module = Regex::new(&format!(r"\bmod\s+(r#)?{}\s*;", regex::escape(name))).expect("an escaped name is a valid pattern");
    let named = Regex::new(&format!(
        r#"(#\[\s*path\s*=|include!\s*\()\s*"([^"]*/)?{}""#,
        regex::escape(file_tail)
    ))
    .expect("an escaped name is a valid pattern");
    module.is_match(&code) || named.is_match(&code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(files: &[(&str, &str)]) -> GeneratedOutput {
        let mut g = GeneratedOutput::new();
        for (p, t) in files {
            g.add_file(PathBuf::from(p), t.to_string());
        }
        g
    }

    fn globs(patterns: &[&str]) -> GlobSet {
        let mut b = globset::GlobSetBuilder::new();
        for p in patterns {
            b.add(globset::Glob::new(p).unwrap());
        }
        b.build().unwrap()
    }

    #[test]
    fn a_generated_file_nothing_declares_is_reported_and_a_declared_one_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let g = out(&[
            ("src/lib.rs", "pub mod domain;\npub mod config;\n"),
            ("src/domain/mod.rs", "pub mod entity;\n// pub mod value_objects;\n"),
            ("src/domain/entity/mod.rs", "pub mod order;\npub mod r#type;\n"),
            ("src/domain/entity/order.rs", "pub struct Order;\n"),
            ("src/domain/entity/type.rs", "pub enum Type {}\n"),
            ("src/domain/value_objects/mod.rs", "pub mod money;\n"),
            ("src/domain/value_objects/money.rs", "pub struct Money;\n"),
            ("src/config/mod.rs", "pub mod settings;\n"),
            ("src/config/generated.rs", "pub struct C;\n"),
            ("src/handlers/mod.rs", "pub fn h() {}\n"),
        ]);
        let found = unreachable_generated(&g, dir.path(), &globs(&[]), false);
        let names: Vec<String> = found.iter().map(|p| p.display().to_string()).collect();
        assert_eq!(
            names,
            ["src/config/generated.rs", "src/domain/value_objects/mod.rs", "src/domain/value_objects/money.rs", "src/handlers/mod.rs"],
            "a commented-out declaration reaches nothing, and an unreached directory takes its files with it"
        );
    }

    #[test]
    fn a_hand_owned_index_decides_what_its_directory_compiles() {
        let dir = tempfile::tempdir().unwrap();
        let entity = dir.path().join("src/domain/entity");
        fs::create_dir_all(&entity).unwrap();
        fs::write(entity.join("mod.rs"), "pub mod oauth_provider_status;\n").unwrap();
        let g = out(&[
            ("src/lib.rs", "pub mod domain;\n"),
            ("src/domain/mod.rs", "pub mod entity;\n"),
            ("src/domain/entity/mod.rs", "pub mod o_auth_provider_status;\n"),
            ("src/domain/entity/o_auth_provider_status.rs", "pub enum OAuthProviderStatus {}\n"),
        ]);
        let found = unreachable_generated(&g, dir.path(), &globs(&["src/domain/entity/mod.rs"]), false);
        assert_eq!(found, [PathBuf::from("src/domain/entity/o_auth_provider_status.rs")]);
    }

    #[test]
    fn extension_files_merged_zones_and_path_attributes_declare_too() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src/presentation")).unwrap();
        fs::write(root.join("src/lib.ext.rs"), "pub mod presentation;\n").unwrap();
        fs::write(root.join("src/presentation/mod.rs"), "// <<< CUSTOM\npub mod extra;\n// END CUSTOM\n").unwrap();
        let g = out(&[
            ("src/lib.rs", "#[path = \"routes_gen.rs\"]\nmod routes;\n"),
            ("src/routes_gen.rs", "pub fn r() {}\n"),
            ("src/presentation/mod.rs", "pub mod http;\n"),
            ("src/presentation/http.rs", "pub fn h() {}\n"),
            ("src/presentation/extra.rs", "pub fn e() {}\n"),
        ]);
        assert!(unreachable_generated(&g, root, &globs(&[]), true).is_empty());
        let whole = unreachable_generated(&g, root, &globs(&[]), false);
        assert_eq!(whole, [PathBuf::from("src/presentation/extra.rs")], "a file written whole keeps no old declarations");
    }

    #[test]
    fn a_module_without_a_crate_root_writes_everything() {
        let dir = tempfile::tempdir().unwrap();
        let g = out(&[("src/domain/mod.rs", "pub mod entity;\n")]);
        assert!(unreachable_generated(&g, dir.path(), &globs(&[]), false).is_empty());
    }
}
