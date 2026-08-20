//! Phase 2: single-line `// <<< CUSTOM` markers.
//!
//! Regions from the existing file are paired with slots in the regenerated
//! content by SLOT IDENTITY — (normalised name, marker indent, occurrence
//! ordinal) — and each paired region REPLACES its slot's interior verbatim.
//! Only regions whose slot the template no longer emits fall back to
//! anchor-based insertion; a region that can be placed nowhere is appended
//! at EOF with a loud warning. A region is never relocated into a slot it
//! did not come from.
//!
//! Why identity pairing instead of anchor-first placement: anchors are
//! arbitrary neighbouring lines, and pure closers (`));`, `}`, …) make
//! worthless anchors that vanish or match at the wrong scope between
//! regens. On the backbone-accounting v0.6.1 regen (2026-08-20) an
//! unanchored build-wiring block was relocated into the FIRST
//! `// <<< CUSTOM FIELDS` slot in the file — clobbering the struct-field
//! block already placed there and splicing `let` statements into the struct
//! body (91 compile errors) — while a METHODS block whose anchor was `))`
//! landed at EOF outside its `impl`. Pairing by identity makes those
//! crossings impossible: the same-named, same-indent, same-ordinal region
//! fills the same-named, same-indent, same-ordinal slot.

use super::markers::{
    find_anchor_line, is_custom_end_marker, is_custom_start_marker, is_whole_line_custom_marker,
    normalize_line,
};

/// One CUSTOM region parsed out of a file: the START marker line through its
/// END marker (paired form), or the code line carrying an inline tag
/// (`let x = foo(); // <<< CUSTOM - note`).
#[derive(Debug, Clone)]
pub(super) struct CustomRegion {
    /// Normalised slot name: text after `<<< CUSTOM` with decorations
    /// stripped and lowercased — `"FIELDS"` → `"fields"`,
    /// `"- custom builder methods"` → `"custom builder methods"`,
    /// `""` for a bare marker.
    pub(super) name: String,
    /// Leading whitespace of the START marker line. Same-named slots at
    /// different scopes (a struct field list vs an impl body) sit at
    /// different indents, so indent is part of the identity.
    pub(super) indent: usize,
    /// True when the region is an inline tag on a code line (no END marker).
    inline: bool,
    /// Nearest preceding substantive line in the ORIGINAL file — the anchor
    /// used only when the region's slot no longer exists in the template.
    anchor: Option<String>,
    /// The whole region verbatim, markers included.
    pub(super) lines: Vec<String>,
}

impl CustomRegion {
    /// First real CODE line of the region (markers, blanks, and comments
    /// skipped). `None` for a placeholder region (comment-only).
    fn first_code_line(&self) -> Option<&String> {
        self.lines.iter().find(|l| is_real_code(l))
    }

    /// Whether the region carries hand-written code worth preserving.
    fn has_real_code(&self) -> bool {
        self.first_code_line().is_some()
    }

    /// Identity key for a pairing pass: `(name, indent)` for pass 1, indent
    /// only for pass 2 (bridging template marker renames such as
    /// `// <<< CUSTOM INIT` → bare `// <<< CUSTOM` at the same indent).
    fn key(&self, pass: Pass) -> (String, usize) {
        match pass {
            Pass::NameAndIndent => (self.name.clone(), self.indent),
            Pass::IndentOnly => (String::new(), self.indent),
        }
    }
}

/// Pairing passes, in order.
#[derive(Clone, Copy, PartialEq)]
enum Pass {
    NameAndIndent,
    IndentOnly,
}

/// A line is real code when it is not a CUSTOM marker, not blank, and not a
/// comment.
fn is_real_code(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty() && !trimmed.starts_with("//") && !is_whole_line_custom_marker(line)
}

/// Normalise a START marker line into a slot name.
fn marker_name(line: &str) -> String {
    let trimmed = line.trim();
    let after = trimmed.strip_prefix("//").unwrap_or(trimmed).trim_start();
    let after = after
        .strip_prefix("<<< CUSTOM")
        .unwrap_or(after)
        .trim_start();
    let after = after.trim_end_matches("START >>>").trim_end();
    let after = after.trim_start_matches('-').trim();
    after.to_lowercase()
}

/// Parse every CUSTOM region out of `content`. Paired regions run to their
/// END marker and are kept verbatim; inline tags keep their code line and
/// any continuation lines up to a blank, dropping lines that already exist
/// verbatim in `generated_lines` (an inline tag followed by generated code
/// must not capture that code).
///
/// Named `// <<< CUSTOM … START >>>` blocks are skipped — phase 1
/// ([`super::paired_methods`]) owns them.
pub(super) fn parse_custom_regions(content: &str, generated_lines: &[String]) -> Vec<CustomRegion> {
    let lines: Vec<&str> = content.lines().collect();
    let mut regions = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        // Phase-1 territory: named paired METHODS blocks.
        if line.contains("CUSTOM METHODS START") || line.contains("CUSTOM METHODS END") {
            i += 1;
            continue;
        }
        if !is_custom_start_marker(line) {
            i += 1;
            continue;
        }

        let name = marker_name(line);
        let indent = line.len() - line.trim_start().len();
        let inline_tag_only = !is_whole_line_custom_marker(line);

        let has_end_marker = lines[i + 1..]
            .iter()
            .take_while(|l| !is_custom_start_marker(l))
            .any(|l| is_custom_end_marker(l));

        let anchor = find_anchor_line(&lines, i);
        let mut region_lines = vec![line.to_string()];
        i += 1;

        if inline_tag_only && !has_end_marker {
            // Inline tag on a code line: the code IS the content.
            while i < lines.len() {
                let next = lines[i];
                if next.trim().is_empty() || is_custom_start_marker(next) {
                    break;
                }
                if !generated_lines.iter().any(|gl| gl.trim() == next.trim()) {
                    region_lines.push(next.to_string());
                }
                i += 1;
            }
            regions.push(CustomRegion {
                name,
                indent,
                inline: true,
                anchor,
                lines: region_lines,
            });
        } else if has_end_marker {
            // Paired region: verbatim through the END marker.
            while i < lines.len() {
                let next = lines[i];
                region_lines.push(next.to_string());
                i += 1;
                if is_custom_end_marker(next) {
                    break;
                }
            }
            regions.push(CustomRegion {
                name,
                indent,
                inline: false,
                anchor,
                lines: region_lines,
            });
        }
        // A whole-line marker with no END marker before the next start is a
        // degenerate/empty region: skip it entirely.
    }

    regions
}

/// Insert the existing file's CUSTOM regions into the regenerated lines.
///
/// 1. **Identity pairing** — pass 1 pairs by (name, indent, ordinal); pass 2
///    pairs leftovers by (indent, ordinal) against slots that still hold
///    their placeholder. The k-th region of a key fills the k-th slot of
///    that key; ordinals are structural (position in the original file), so
///    placement is deterministic and re-running is idempotent.
/// 2. **Anchor fallback** — inline tags, and paired regions whose slot the
///    template no longer emits, re-insert after their anchor line. If the
///    insertion point lands on a slot START marker, the region fills that
///    slot ONLY when the slot still holds its placeholder — a slot already
///    carrying real code is another region's placed content and is never
///    clobbered; the region inserts after the slot's END marker instead.
/// 3. **EOF append** — a region with neither slot nor anchor is appended at
///    EOF with a warning.
pub(super) fn insert_custom_blocks(
    result_lines: &mut Vec<String>,
    existing_regions: &[CustomRegion],
) {
    use std::collections::HashSet;

    let mut placed = vec![false; existing_regions.len()];
    // Slots already filled by an earlier pairing in THIS merge. Keyed by the
    // slot's canonical identity (name, indent, ordinal) so a later pass can
    // never redirect another region into a filled slot — while a generated
    // slot that merely SHIPS example code (no claim yet) stays replaceable:
    // a CUSTOM region is user-owned and the user's content wins.
    let mut claimed: HashSet<(String, usize, usize)> = HashSet::new();

    for pass in [Pass::NameAndIndent, Pass::IndentOnly] {
        for (ri, region) in existing_regions.iter().enumerate() {
            if placed[ri] || region.inline || !region.has_real_code() {
                continue;
            }
            if let Some(slot_start) =
                find_slot(result_lines, existing_regions, ri, pass, &mut claimed)
            {
                replace_slot_span(result_lines, slot_start, region);
                placed[ri] = true;
            }
        }
    }

    for (ri, region) in existing_regions.iter().enumerate() {
        if placed[ri] || !region.has_real_code() {
            continue; // placeholder regions stand down
        }
        insert_anchored(result_lines, region);
    }
}

/// Locate the slot in `result_lines` that the region at index `ri` owns
/// under `pass`'s key: the k-th slot sharing the region's key, where k is
/// the region's ordinal among same-key PAIRED regions in the original file.
/// The slot must still be a placeholder (no real code) — a filled slot
/// already holds a region placed by an earlier pairing and is not returned.
fn find_slot(
    result_lines: &[String],
    existing_regions: &[CustomRegion],
    ri: usize,
    pass: Pass,
    claimed: &mut std::collections::HashSet<(String, usize, usize)>,
) -> Option<usize> {
    use std::collections::HashMap;

    let region = &existing_regions[ri];
    let region_key = region.key(pass);
    let ordinal = existing_regions
        .iter()
        .take(ri)
        .filter(|r| !r.inline && r.key(pass) == region_key)
        .count();

    let slots = parse_custom_regions(&result_lines.join("\n"), &[]);
    // Canonical (pass-1) identity per slot, used for the claim registry so
    // protection against slot-stealing is uniform across passes.
    let mut counts: HashMap<(String, usize), usize> = HashMap::new();
    let canonical: Vec<Option<(String, usize, usize)>> = slots
        .iter()
        .map(|s| {
            let k = (s.name.clone(), s.indent);
            let c = counts.entry(k).or_insert(0);
            let id = (s.name.clone(), s.indent, *c);
            *c += 1;
            if s.inline {
                None
            } else {
                Some(id)
            }
        })
        .collect();

    let mut seen = 0;
    for (si, slot) in slots.iter().enumerate() {
        if slot.inline {
            continue;
        }
        if slot.key(pass) == region_key {
            if seen == ordinal {
                let id = canonical[si].clone()?;
                if claimed.contains(&id) {
                    return None; // filled by an earlier pairing this merge
                }
                claimed.insert(id);
                return locate_span(result_lines, &slot.lines);
            }
            seen += 1;
        }
    }
    None
}

/// Index where `span` occurs as a consecutive subsequence of `result_lines`.
/// Sequence-matching (not marker-line matching) keeps two slots with
/// byte-identical marker lines from aliasing to the first occurrence.
fn locate_span(result_lines: &[String], span: &[String]) -> Option<usize> {
    if span.is_empty() || span.len() > result_lines.len() {
        return None;
    }
    result_lines.windows(span.len()).position(|w| w == span)
}

/// Replace the slot starting at `start` (its START marker line) with the
/// region's content, KEEPING the slot's marker lines so the template's
/// current marker text (and any marker rename) wins over the existing
/// file's.
fn replace_slot_span(result_lines: &mut Vec<String>, start: usize, region: &CustomRegion) {
    let end_idx = (start + 1..result_lines.len())
        .position(|k| is_custom_end_marker(&result_lines[k]))
        .map(|off| start + 1 + off);
    let remove_until = end_idx
        .map(|e| e + 1)
        .unwrap_or(start + 1)
        .min(result_lines.len());

    let mut replacement: Vec<String> = Vec::new();
    replacement.push(result_lines[start].clone()); // slot's START marker
    let interior = if end_idx.is_some() {
        &region.lines[1..region.lines.len().saturating_sub(1)]
    } else {
        &region.lines[1..]
    };
    for line in interior {
        replacement.push(line.clone());
    }
    if let Some(e) = end_idx {
        replacement.push(result_lines[e].clone()); // slot's END marker
    }

    for _ in start..remove_until {
        result_lines.remove(start);
    }
    for (j, line) in replacement.iter().enumerate() {
        result_lines.insert(start + j, line.clone());
    }
}

/// Anchor-based insertion for regions without a slot (inline tags, and
/// paired regions whose slot the template dropped). If the insertion point
/// lands on a slot START marker, the region fills that slot only when the
/// slot still holds its placeholder; a filled slot is another region's
/// placed content — insert after its END marker instead, never clobber.
fn insert_anchored(result_lines: &mut Vec<String>, region: &CustomRegion) {
    let Some(anchor_line) = region.anchor.clone() else {
        warn_unplaced(region, "no anchor recorded");
        if !first_code_line_already_present(result_lines, region) {
            for line in &region.lines {
                result_lines.push(line.clone());
            }
        }
        return;
    };

    let anchor_trimmed = anchor_line.trim();
    let anchor_normalized = normalize_line(&anchor_line);
    let pos = result_lines
        .iter()
        .rposition(|l| l == &anchor_line)
        .or_else(|| {
            result_lines
                .iter()
                .rposition(|l| l.trim() == anchor_trimmed)
        })
        .or_else(|| {
            result_lines
                .iter()
                .rposition(|l| normalize_line(l) == anchor_normalized)
        })
        .map(|p| p + 1);

    let Some(pos) = pos else {
        warn_unplaced(region, "anchor not found in regenerated file");
        if !first_code_line_already_present(result_lines, region) {
            for line in &region.lines {
                result_lines.push(line.clone());
            }
        }
        return;
    };

    let pos = adjust_for_placement(result_lines, pos, region);

    if pos < result_lines.len() && is_custom_start_marker(&result_lines[pos]) {
        if let Some(end) = (pos + 1..result_lines.len())
            .position(|k| is_custom_end_marker(&result_lines[k]))
            .map(|off| pos + 1 + off)
        {
            let slot_is_placeholder = result_lines[pos + 1..end].iter().all(|l| !is_real_code(l));
            if slot_is_placeholder {
                replace_slot_span(result_lines, pos, region);
                return;
            }
            // Filled slot: insert after its END marker, never clobber.
            let after = (end + 1).min(result_lines.len());
            if !first_code_line_already_present(result_lines, region) {
                for (j, line) in region.lines.iter().enumerate() {
                    result_lines.insert(after + j, line.clone());
                }
            }
            return;
        }
    }

    if !first_code_line_already_present(result_lines, region) {
        for (j, line) in region.lines.iter().enumerate() {
            result_lines.insert(pos + j, line.clone());
        }
    }
}

fn warn_unplaced(region: &CustomRegion, why: &str) {
    eprintln!(
        "  Warning: no slot for CUSTOM block ({:?}, {}), appending at end of file",
        region.name, why
    );
}

/// Whether the region's first real CODE line already exists in
/// `result_lines` — guards against duplicating a block on re-insertion.
fn first_code_line_already_present(result_lines: &[String], region: &CustomRegion) -> bool {
    let Some(content_line) = region.first_code_line() else {
        return false;
    };
    let content_normalized = normalize_line(content_line);
    if result_lines
        .iter()
        .any(|rl| normalize_line(rl) == content_normalized)
    {
        eprintln!("  Custom block already present (dedup), skipping");
        true
    } else {
        false
    }
}

/// A field-like block (identifier comma — struct/enum field or use-list
/// item) belongs INSIDE its container; a module-scope block (statement or
/// item opener) belongs AFTER any enclosing container close.
fn block_is_module_scope(region: &CustomRegion) -> bool {
    region
        .first_code_line()
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
        .unwrap_or(false)
}

/// When a block looks module-scope, walk past trailing closing braces so it
/// lands AFTER the enclosing container, not inside it.
///
/// Brace depth matters as much as closers: when the regenerated file reflows
/// `pub use foo::{A, B}` into one item per line, the anchor line (`    A,`)
/// ends up INSIDE the still-open use group. Inserting after it splices module
/// items into the group body — a guaranteed syntax error. So for module-scope
/// blocks the insertion point first advances until the accumulated brace depth
/// from the file start returns to zero, then past any stray close-brace lines
/// (containers that already closed before the anchor), then past one blank.
fn adjust_for_placement(
    result_lines: &[String],
    initial_pos: usize,
    region: &CustomRegion,
) -> usize {
    if !block_is_module_scope(region) {
        return initial_pos;
    }

    let mut pos = initial_pos;
    let mut depth = brace_depth_before(result_lines, pos);
    while pos < result_lines.len() && depth > 0 {
        depth = depth.saturating_add(net_braces(&result_lines[pos]));
        pos += 1;
    }

    while pos < result_lines.len() {
        let line = result_lines[pos].trim();
        let is_close_brace_only = line == "}" || line == "};" || line == "})" || line == "});";
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

/// Brace depth (`{` count minus `}` count) accumulated over `lines[..pos]`.
fn brace_depth_before(lines: &[String], pos: usize) -> i32 {
    lines[..pos.min(lines.len())]
        .iter()
        .map(|l| net_braces(l))
        .sum()
}

/// Net brace delta of a single line.
fn net_braces(line: &str) -> i32 {
    let opens = line.matches('{').count() as i32;
    let closes = line.matches('}').count() as i32;
    opens - closes
}
