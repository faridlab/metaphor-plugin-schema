//! Low-level marker predicates and line-comparison helpers.
//!
//! Both the [single-marker](super::single_marker) and
//! [paired-methods](super::paired_methods) mergers, plus the
//! [unprotected-code](super::unprotected) scanner, share these primitives:
//!
//! - `is_custom_start_marker` / `is_custom_end_marker` — recognise
//!   `// <<< CUSTOM` and `// END CUSTOM` while ignoring doc comments that
//!   *mention* the marker string.
//! - `is_whole_line_custom_marker` — distinguishes a bare structural
//!   marker line from an inline marker (code with a trailing tag).
//! - `find_anchor_line` — walks backwards to find the nearest stable line
//!   we can pin a CUSTOM block to, skipping unhelpful closing braces.
//! - `normalize_line` — fuzzy-match key used by anchor matching and dedup.

/// Normalise a line for fuzzy comparison: trim whitespace, strip `.clone()`
/// calls, and collapse double spaces. Centralised so future normalisation
/// improvements apply everywhere anchors and dedup checks are run.
pub(super) fn normalize_line(line: &str) -> String {
    line.trim().replace(".clone()", "").replace("  ", " ")
}

/// Whether a line contains a real `// <<< CUSTOM` start marker.
///
/// Doc comments (`///`) and module doc comments (`//!`) that mention the
/// marker string inside prose or backticks are NOT markers — treating them
/// as markers creates ghost blocks that leak stale content into regen output.
pub(super) fn is_custom_start_marker(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("///") || trimmed.starts_with("//!") {
        return false;
    }
    line.contains("// <<< CUSTOM")
}

/// Whether a line contains a real `// END CUSTOM` end marker. Doc comments
/// are excluded for the same reason as [`is_custom_start_marker`].
pub(super) fn is_custom_end_marker(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("///") || trimmed.starts_with("//!") {
        return false;
    }
    line.contains("END CUSTOM")
}

/// Whether the line is PURELY a CUSTOM marker with no other content (e.g.
/// `    // <<< CUSTOM` or `    // END CUSTOM`). These are structural markers
/// that bracket content rather than content themselves.
///
/// Inline markers like `mod foo; // <<< CUSTOM - Extension` return false —
/// the Rust code preceding the marker IS preservable content.
pub(super) fn is_whole_line_custom_marker(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
        return false;
    }
    let after = trimmed[2..].trim_start();
    after.starts_with("<<< CUSTOM") || after.starts_with("END CUSTOM")
}

/// Walk backwards from `start_index` to find the nearest non-empty,
/// non-CUSTOM line. Pure closing braces are skipped — they aren't unique
/// enough as anchors and tend to match at wrong positions in regen content.
pub(super) fn find_anchor_line(existing_lines: &[&str], start_index: usize) -> Option<String> {
    if start_index == 0 {
        return None;
    }
    let mut j = start_index - 1;
    loop {
        let prev = existing_lines[j].trim();
        if !prev.is_empty() && !is_custom_start_marker(prev) && !is_custom_end_marker(prev) {
            if matches!(prev, "}" | "})" | "});" | "}," | "};") {
                if j == 0 {
                    return None;
                }
                j -= 1;
                continue;
            }
            return Some(existing_lines[j].to_string());
        }
        if j == 0 {
            return None;
        }
        j -= 1;
    }
}
