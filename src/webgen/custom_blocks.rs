//! Preserve `// <<< CUSTOM … // END CUSTOM` block bodies across regeneration of
//! generated TypeScript files.
//!
//! The webapp generators emit the markers but write each file fresh, so any
//! hand-authored content inside a block (e.g. a `listSchema`) would be lost on
//! the next `schema generate:webapp`. Unlike the Rust `mod.rs` merge — which
//! re-anchors single-line markers and misfires on nested (brace) structures —
//! this keeps the generator's marker PLACEMENT and only substitutes the body,
//! matched by the marker's header line. That's correct for TS files where the
//! block sits at a fixed, generator-controlled spot.

use std::collections::{HashMap, VecDeque};
use std::path::Path;

fn is_open(line: &str) -> bool {
    line.trim_start().starts_with("// <<< CUSTOM")
}

fn is_end(line: &str) -> bool {
    line.trim() == "// END CUSTOM"
}

/// Map each existing block's (trimmed) open-marker line → queue of bodies (the
/// lines strictly between its open and `// END CUSTOM`). A queue handles the
/// rare case of two blocks sharing an identical header.
fn existing_bodies(existing: &str) -> HashMap<String, VecDeque<Vec<String>>> {
    let mut map: HashMap<String, VecDeque<Vec<String>>> = HashMap::new();
    let lines: Vec<&str> = existing.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if is_open(lines[i]) {
            let header = lines[i].trim().to_string();
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() && !is_end(lines[i]) {
                body.push(lines[i].to_string());
                i += 1;
            }
            map.entry(header).or_default().push_back(body);
        }
        i += 1;
    }
    map
}

/// Substitute existing CUSTOM block bodies into freshly generated content,
/// matched by header line, preserving the generator's placement. If the file
/// doesn't exist yet or has no CUSTOM blocks, the generated content is returned
/// unchanged.
pub(crate) fn preserve_custom_blocks(generated: &str, existing_path: &Path) -> String {
    let existing = match std::fs::read_to_string(existing_path) {
        Ok(s) => s,
        Err(_) => return generated.to_string(),
    };
    let mut bodies = existing_bodies(&existing);
    if bodies.is_empty() {
        return generated.to_string();
    }

    let lines: Vec<&str> = generated.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        out.push(line.to_string());
        if is_open(line) {
            let header = line.trim().to_string();
            // Skip the generated (placeholder) body up to END CUSTOM.
            let mut gen_body: Vec<String> = Vec::new();
            i += 1;
            while i < lines.len() && !is_end(lines[i]) {
                gen_body.push(lines[i].to_string());
                i += 1;
            }
            // Prefer the existing body; fall back to the generated one.
            let body = bodies
                .get_mut(&header)
                .and_then(|q| q.pop_front())
                .unwrap_or(gen_body);
            out.extend(body);
            // `i` is now at END CUSTOM (or EOF); emit it on the next iteration.
            continue;
        }
        i += 1;
    }

    let mut result = out.join("\n");
    if generated.ends_with('\n') {
        result.push('\n');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(content: &str) -> (tempfile::NamedTempFile, std::path::PathBuf) {
        let mut f = tempfile::NamedTempFile::with_suffix(".ts").unwrap();
        f.write_all(content.as_bytes()).unwrap();
        let p = f.path().to_path_buf();
        (f, p)
    }

    #[test]
    fn fills_empty_placeholder_from_existing_keeping_placement() {
        let existing = "export const x = 1;\n\n// <<< CUSTOM: schemas\nexport const listSchema = x;\n// END CUSTOM\n";
        let generated = "export const x = 2;\n\n// <<< CUSTOM: schemas\n// END CUSTOM\n";
        let (_t, path) = tmp(existing);
        let out = preserve_custom_blocks(generated, &path);
        assert!(out.contains("export const x = 2;"), "regenerated body must win outside the block");
        assert!(out.contains("export const listSchema = x;"), "custom body preserved");
        assert_eq!(out.matches("// <<< CUSTOM: schemas").count(), 1, "no duplicate marker");
        assert_eq!(out.matches("export const listSchema").count(), 1, "no duplicate body");
        // placement: the block stays where the generator put it (end), not inside x.
        assert!(out.find("export const x = 2;").unwrap() < out.find("listSchema").unwrap());
    }

    #[test]
    fn no_existing_block_returns_generated() {
        let existing = "export const x = 1;\n";
        let generated = "export const x = 2;\n// <<< CUSTOM: schemas\n// END CUSTOM\n";
        let (_t, path) = tmp(existing);
        assert_eq!(preserve_custom_blocks(generated, &path), generated);
    }

    #[test]
    fn missing_file_returns_generated() {
        let out = preserve_custom_blocks("a\n// <<< CUSTOM\n// END CUSTOM\n", Path::new("/no/such/file.ts"));
        assert_eq!(out, "a\n// <<< CUSTOM\n// END CUSTOM\n");
    }
}
