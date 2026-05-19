//! Phase 2: single-line `// <<< CUSTOM` markers — collect from the existing
//! file with each block's anchor, then re-insert into the regenerated
//! content using fuzzy anchor matching.
//!
//! This is the harder of the two CUSTOM strategies because anchors are
//! ambiguous — `pub b: i32,` (struct field) and `B,` (last item in
//! `pub use foo::{ A, B, };`) both end with `,` but the block belongs in
//! different places. [`insert_custom_blocks`] resolves this by looking at
//! the BLOCK CONTENT rather than just the anchor — see its body for the
//! inside-vs-after-container heuristic.

use super::markers::{
    find_anchor_line, is_custom_end_marker, is_custom_start_marker, is_whole_line_custom_marker,
    normalize_line,
};

/// Scan `existing_content` for `// <<< CUSTOM` markers and collect each
/// block together with its anchor line. Paired `END CUSTOM` blocks are
/// preserved verbatim; inline markers filter out lines already present in
/// `generated_content`.
pub(super) fn collect_custom_blocks(
    existing_content: &str,
    generated_content: &str,
) -> Vec<(Option<String>, Vec<String>)> {
    let mut custom_blocks: Vec<(Option<String>, Vec<String>)> = Vec::new();
    let existing_lines: Vec<&str> = existing_content.lines().collect();
    let generated_lines: Vec<&str> = generated_content.lines().collect();
    let mut i = 0;

    while i < existing_lines.len() {
        let line = existing_lines[i];
        // Skip lines that are part of CUSTOM METHODS START/END blocks
        // (handled in the paired-methods phase).
        if line.contains("CUSTOM METHODS START") || line.contains("CUSTOM METHODS END") {
            i += 1;
            continue;
        }
        if is_custom_start_marker(line) {
            let anchor = find_anchor_line(&existing_lines, i);

            let has_end_marker = existing_lines[i + 1..]
                .iter()
                .take_while(|l| !is_custom_start_marker(l))
                .any(|l| is_custom_end_marker(l));

            let mut block_lines = vec![line.to_string()];
            i += 1;

            if has_end_marker {
                // Paired block: preserve ALL lines verbatim until END CUSTOM.
                // The has_end_marker scan above confirmed a closing marker
                // exists before the next `// <<< CUSTOM`, so we loop without
                // an arbitrary cap — capping here truncates legitimate
                // multi-line blocks (e.g. large `matches!` macros).
                while i < existing_lines.len() {
                    let next = existing_lines[i];
                    if is_custom_end_marker(next) {
                        block_lines.push(next.to_string());
                        i += 1;
                        break;
                    }
                    block_lines.push(next.to_string());
                    i += 1;
                }
            } else {
                // Inline marker: filter lines that already exist in generated content
                while i < existing_lines.len() {
                    let next = existing_lines[i];
                    if next.trim().is_empty() || is_custom_start_marker(next) {
                        break;
                    }
                    let is_generated = generated_lines.iter().any(|gl| gl.trim() == next.trim());
                    if !is_generated {
                        block_lines.push(next.to_string());
                    }
                    i += 1;
                }
            }

            // Skip empty paired blocks (just markers, no content) to prevent
            // accumulation. Inline markers (code with a trailing
            // `// <<< CUSTOM` tag) are NOT whole-line markers, so the code
            // part still counts as preservable content.
            let has_content = block_lines
                .iter()
                .any(|l| !is_whole_line_custom_marker(l) && !l.trim().is_empty());
            if has_content {
                custom_blocks.push((anchor, block_lines));
            }
        } else {
            i += 1;
        }
    }

    custom_blocks
}

/// Insert previously collected custom blocks into `result_lines`, using
/// fuzzy anchor matching and dedup checks to avoid duplicating content.
///
/// The merger sees `(anchor, block)` but has no context for whether the
/// CUSTOM block was originally INSIDE a container (struct/enum fields,
/// use-list) or a SIBLING at module/function scope. The anchor alone is
/// ambiguous — `pub b: i32,` (struct field) and `B,` (last item in
/// `pub use foo::{ A, B, };`) both end with `,` but the block belongs in
/// different places.
///
/// The reliable signal is the BLOCK CONTENT, not the anchor:
/// - If the block's first real content line is a complete statement (ends
///   with `;`) or opens a module-scope item (`pub mod`, `pub use`,
///   `pub fn`, `impl`, `mod`, `use`, `fn`, `struct`, `enum`, `trait`,
///   `type`, `const`, `static`, `#[…]`), the block is module-scope → walk
///   past close braces so it lands AFTER any containing `};`.
/// - Otherwise the block looks field-like (identifier comma, variant
///   comma) → keep position so it stays INSIDE the enclosing container.
pub(super) fn insert_custom_blocks(
    result_lines: &mut Vec<String>,
    custom_blocks: &[(Option<String>, Vec<String>)],
) {
    for (anchor, block_lines) in custom_blocks {
        // Dedup: skip if the first real content line already exists in result
        let first_content_line = block_lines
            .iter()
            .find(|l| !is_whole_line_custom_marker(l) && !l.trim().is_empty());
        if let Some(content_line) = first_content_line {
            let content_normalized = normalize_line(content_line);
            let already_in_result = result_lines
                .iter()
                .any(|rl| normalize_line(rl) == content_normalized);
            if already_in_result {
                eprintln!("  Custom block already present (dedup), skipping");
                continue;
            }
        }

        let insert_pos = if let Some(anchor_line) = anchor {
            let anchor_trimmed = anchor_line.trim();
            let anchor_normalized = normalize_line(anchor_line);

            // 1. Exact, 2. trimmed, 3. normalized fallback.
            let pos = result_lines
                .iter()
                .rposition(|l| l == anchor_line)
                .or_else(|| {
                    result_lines
                        .iter()
                        .rposition(|l| l.trim() == anchor_trimmed)
                })
                .or_else(|| {
                    result_lines
                        .iter()
                        .rposition(|l| normalize_line(l) == anchor_normalized)
                });

            pos.map(|p| p + 1)
        } else {
            None
        };

        if let Some(pos) = insert_pos {
            let pos = adjust_for_placement(result_lines, pos, block_lines);
            insert_at(result_lines, pos, block_lines);
        } else {
            eprintln!("  Warning: No anchor found for custom block, appending at end of file");
            for custom_line in block_lines {
                result_lines.push(custom_line.clone());
            }
        }
    }
}

/// Apply the inside-vs-after-container heuristic: when the block looks
/// module-scope, walk past trailing closing braces so it lands AFTER the
/// enclosing container, not inside it.
fn adjust_for_placement(
    result_lines: &[String],
    initial_pos: usize,
    block_lines: &[String],
) -> usize {
    let first_content = block_lines.iter().find(|l| {
        !is_whole_line_custom_marker(l)
            && !l.trim().is_empty()
            && !l.trim_start().starts_with("//")
    });
    let block_is_module_scope = first_content
        .map(|l| {
            let t = l.trim();
            let trimmed_no_semi = t.trim_end_matches(';');
            t.ends_with(';')
                || t.ends_with('}')
                || trimmed_no_semi.starts_with("pub mod ")
                || trimmed_no_semi.starts_with("pub use ")
                || trimmed_no_semi.starts_with("pub fn ")
                || trimmed_no_semi.starts_with("pub struct ")
                || trimmed_no_semi.starts_with("pub enum ")
                || trimmed_no_semi.starts_with("pub trait ")
                || trimmed_no_semi.starts_with("pub type ")
                || trimmed_no_semi.starts_with("pub const ")
                || trimmed_no_semi.starts_with("pub static ")
                || trimmed_no_semi.starts_with("mod ")
                || trimmed_no_semi.starts_with("use ")
                || trimmed_no_semi.starts_with("fn ")
                || trimmed_no_semi.starts_with("impl ")
                || trimmed_no_semi.starts_with("struct ")
                || trimmed_no_semi.starts_with("enum ")
                || trimmed_no_semi.starts_with("trait ")
                || trimmed_no_semi.starts_with("type ")
                || trimmed_no_semi.starts_with("const ")
                || trimmed_no_semi.starts_with("static ")
                || trimmed_no_semi.starts_with("#[")
        })
        .unwrap_or(false);

    if !block_is_module_scope {
        return initial_pos;
    }

    let mut pos = initial_pos;
    while pos < result_lines.len() {
        let line = result_lines[pos].trim();
        let is_close_brace_only =
            line == "}" || line == "};" || line == "})" || line == "});";
        if is_close_brace_only {
            pos += 1;
        } else {
            break;
        }
    }
    if pos < result_lines.len() && result_lines[pos].trim().is_empty() {
        pos += 1;
    }
    pos
}

/// Insert `block_lines` at `pos`, replacing an empty placeholder
/// (`// <<< CUSTOM` immediately followed by `// END CUSTOM`) when one is
/// already present at that position. When a non-empty CUSTOM placeholder
/// exists there, skip insertion (content already present).
fn insert_at(result_lines: &mut Vec<String>, pos: usize, block_lines: &[String]) {
    let has_custom_at_pos =
        pos < result_lines.len() && is_custom_start_marker(&result_lines[pos]);
    if has_custom_at_pos {
        let is_empty_placeholder =
            pos + 1 < result_lines.len() && is_custom_end_marker(&result_lines[pos + 1]);
        if is_empty_placeholder {
            result_lines.remove(pos + 1);
            result_lines.remove(pos);
            for (j, custom_line) in block_lines.iter().enumerate() {
                result_lines.insert(pos + j, custom_line.clone());
            }
        }
        // Otherwise content already present → skip.
        return;
    }

    for (j, custom_line) in block_lines.iter().enumerate() {
        result_lines.insert(pos + j, custom_line.clone());
    }
}
