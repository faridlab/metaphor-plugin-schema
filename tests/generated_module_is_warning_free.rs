//! The warning gate: a module generated from the skeleton and the warning-gate
//! fixture must compile with every warning denied.
//!
//! It copies the module skeleton (`../modules/backbone-module`, or the path in
//! `METAPHOR_MODULE_SKELETON`), adds `tests/fixtures/warning-gate/schema`,
//! regenerates it with this crate's generator and runs
//! `cargo rustc --lib -- -D warnings`. Dependencies come from crates.io, so it
//! needs the network and a few minutes, and runs only when asked:
//!
//!     cargo test --test generated_module_is_warning_free -- --ignored
//!
//! `METAPHOR_GATE_TARGET` names a cargo target directory to reuse between runs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if matches!(name.to_str(), Some("target" | ".git" | "graphify-out")) {
            continue;
        }
        let (src, dst) = (entry.path(), to.join(&name));
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&src, &dst);
        } else {
            fs::copy(&src, &dst).unwrap();
        }
    }
}

fn fill_module_name(dir: &Path, name: &str) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            fill_module_name(&path, name);
        } else if let Ok(text) = fs::read_to_string(&path) {
            if text.contains("__MODULE__") {
                fs::write(&path, text.replace("__MODULE__", name)).unwrap();
            }
        }
    }
}

#[test]
#[ignore = "builds a generated module against crates.io; run with --ignored"]
fn a_module_generated_from_the_skeleton_compiles_without_warnings() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let skeleton = std::env::var_os("METAPHOR_MODULE_SKELETON")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate_dir.join("../modules/backbone-module"));
    assert!(
        skeleton.join("Cargo.toml").exists(),
        "no module skeleton at {}; set METAPHOR_MODULE_SKELETON",
        skeleton.display()
    );

    let work = tempfile::tempdir().unwrap();
    let module = work.path().join("module");
    copy_tree(&skeleton, &module);
    fill_module_name(&module.join("schema"), "gate");
    copy_tree(&crate_dir.join("tests/fixtures/warning-gate/schema"), &module.join("schema"));

    let generate = Command::new(env!("CARGO_BIN_EXE_metaphor-schema"))
        .args(["schema", "generate", ".", "--force"])
        .current_dir(&module)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        generate.status.success(),
        "generation failed:\n{}",
        String::from_utf8_lossy(&generate.stderr)
    );

    let target = std::env::var_os("METAPHOR_GATE_TARGET")
        .map(PathBuf::from)
        .unwrap_or_else(|| work.path().join("target"));
    let check = Command::new(env!("CARGO"))
        .args(["rustc", "--lib", "--", "-D", "warnings"])
        .current_dir(&module)
        .env("CARGO_TARGET_DIR", target)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&check.stderr);
    let errors: Vec<&str> = stderr.lines().filter(|l| l.starts_with("error")).collect();
    assert!(
        check.status.success(),
        "the generated module does not compile warning-free:\n{}\n\n{}",
        errors.join("\n"),
        stderr.lines().rev().take(60).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")
    );
}
