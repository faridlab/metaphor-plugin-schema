//! Phase 1: paired `// <<< CUSTOM METHODS START >>>` /
//! `// <<< CUSTOM METHODS END >>>` blocks.
//!
//! Extracts ALL content between the markers from the existing file and
//! substitutes it into the generated content — every line preserved
//! verbatim, no filtering against generated lines (unlike the
//! [single-marker phase](super::single_marker)).
//!
//! **DDD migration support**: if the existing file has an empty paired
//! block but has DDD method implementations in the old generated section
//! (outside custom markers — pre-marker format), those are migrated into
//! the custom block automatically. This handles the one-time transition
//! from "DDD methods generated, custom override below" to "DDD methods
//! live inside the CUSTOM METHODS block".

/// Replace the generated `CUSTOM METHODS` block with the one from the
/// existing file. Returns the regenerated content unchanged when either
/// side lacks the markers.
pub(super) fn merge_custom_methods_block(generated_content: &str, existing_content: &str) -> String {
    let start_marker = "// <<< CUSTOM METHODS START >>>";
    let end_marker = "// <<< CUSTOM METHODS END >>>";

    let existing_lines: Vec<&str> = existing_content.lines().collect();
    let start_idx = existing_lines.iter().position(|l| l.contains(start_marker));
    let end_idx = existing_lines.iter().position(|l| l.contains(end_marker));

    let existing_block = match (start_idx, end_idx) {
        (Some(s), Some(e)) if e > s => {
            let inner_lines: Vec<&str> = existing_lines[s + 1..e]
                .iter()
                .copied()
                .filter(|l| !l.trim().is_empty())
                .collect();
            let has_real_content = inner_lines.iter().any(|l| {
                let trimmed = l.trim();
                // Real content = not a comment, or a comment with actual
                // guidance. Exclude pure placeholder comments like
                // "// Add custom entity methods here".
                !trimmed.starts_with("//")
                    || trimmed.starts_with("/// ")
                    || trimmed.contains("TODO")
            });
            if has_real_content {
                Some(existing_lines[s..=e].to_vec())
            } else {
                migrate_old_ddd_section(&existing_lines, s, e)
            }
        }
        _ => None,
    };

    let existing_block = match existing_block {
        Some(b) => b,
        None => return generated_content.to_string(),
    };

    let gen_lines: Vec<&str> = generated_content.lines().collect();
    let gen_start = gen_lines.iter().position(|l| l.contains(start_marker));
    let gen_end = gen_lines.iter().position(|l| l.contains(end_marker));

    match (gen_start, gen_end) {
        (Some(gs), Some(ge)) if ge > gs => {
            let mut result_lines: Vec<String> = Vec::new();
            for line in &gen_lines[..gs] {
                result_lines.push(line.to_string());
            }
            for line in &existing_block {
                result_lines.push(line.to_string());
            }
            for line in &gen_lines[ge + 1..] {
                result_lines.push(line.to_string());
            }
            result_lines.join("\n")
        }
        _ => generated_content.to_string(),
    }
}

/// Migrate DDD method implementations from the old generated section into
/// the custom methods block. Handles the one-time transition from the old
/// format (DDD methods in generated section) to the new format (DDD
/// methods in custom block).
///
/// Looks for a `// DDD Entity Methods` section before the custom markers
/// that has real implementations (not `todo!()` stubs).
fn migrate_old_ddd_section<'a>(
    existing_lines: &[&'a str],
    custom_start: usize,
    custom_end: usize,
) -> Option<Vec<&'a str>> {
    let ddd_header_idx = existing_lines[..custom_start]
        .iter()
        .position(|l| l.contains("DDD Entity Methods"));

    let ddd_start = match ddd_header_idx {
        Some(idx) => {
            let mut start = idx;
            if start > 0 && existing_lines[start - 1].contains("// ===") {
                start -= 1;
            }
            start
        }
        None => return None,
    };

    let ddd_section = &existing_lines[ddd_start..custom_start];
    let has_implementations = ddd_section.iter().any(|l| {
        let trimmed = l.trim();
        (trimmed.starts_with("if ")
            || trimmed.starts_with("match ")
            || trimmed.starts_with("self.")
            || trimmed.starts_with("let ")
            || trimmed.starts_with("return ")
            || trimmed.starts_with("errors.push")
            || trimmed.contains("!self.")
            || trimmed.contains(".is_")
            || trimmed.contains(".max(")
            || trimmed.contains(".min("))
            && !trimmed.contains("todo!(")
    });

    if !has_implementations {
        return None;
    }

    let _invariants_idx = existing_lines[ddd_start..custom_start]
        .iter()
        .position(|l| l.contains("fn check_invariants"))
        .map(|i| i + ddd_start);

    let mut block: Vec<&str> = Vec::new();
    block.push(existing_lines[custom_start]);
    block.push("");
    for line in &existing_lines[ddd_start..custom_start] {
        block.push(line);
    }
    block.push(existing_lines[custom_end]);

    Some(block)
}
