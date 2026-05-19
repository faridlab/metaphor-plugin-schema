//! Detect lines in an existing file that would be silently dropped on
//! regeneration because they sit outside `// <<< CUSTOM` markers.
//!
//! Used by the write loop to warn the user *before* a merge overwrites
//! their hand-edits — every unprotected line gets reported so the user
//! can wrap it in a CUSTOM block before the next regen.
//!
//! A line is **unprotected custom code** when ALL of these hold:
//!
//! - It is NOT inside a `// <<< CUSTOM` … `// END CUSTOM` block.
//! - It does NOT appear (trimmed) in the regenerated content.
//! - It does NOT match a boilerplate prefix (`use`, `mod`, `pub fn`, …)
//!   that varies cosmetically between regens.
//! - It is at least 3 chars long (filters out lone braces, semis, etc.).

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use super::markers::{is_custom_end_marker, is_custom_start_marker};

pub(in crate::commands::schema) fn detect_unprotected_custom_code(
    generated_content: &str,
    existing_path: &Path,
) -> Vec<String> {
    if !existing_path.exists() {
        return Vec::new();
    }

    let existing_content = match fs::read_to_string(existing_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let generated_lines: HashSet<&str> = generated_content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    let boilerplate_prefixes = [
        "use ", "pub use ", "mod ", "pub mod ", "#[", "//", "}", "{",
        "pub struct ", "pub enum ", "pub trait ", "pub fn ", "pub async fn ",
        "impl ", "fn ", "async fn ", "struct ", "enum ", "trait ",
        "pub type ", "type ", "pub const ", "const ", "pub static ", "static ",
        "super::", "self::", "crate::", "extern ", "where ",
    ];

    let mut in_custom_block = false;
    let mut unprotected: Vec<String> = Vec::new();

    for line in existing_content.lines() {
        let trimmed = line.trim();

        if is_custom_start_marker(line) {
            in_custom_block = true;
            continue;
        }
        if is_custom_end_marker(line) || (in_custom_block && trimmed.is_empty()) {
            in_custom_block = false;
            continue;
        }

        if in_custom_block {
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        if generated_lines.contains(trimmed) {
            continue;
        }
        if boilerplate_prefixes.iter().any(|p| trimmed.starts_with(p)) {
            continue;
        }
        if trimmed.len() <= 2 {
            continue;
        }

        unprotected.push(line.to_string());
    }

    unprotected
}
