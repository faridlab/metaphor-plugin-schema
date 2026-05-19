//! Seed file & seed-order merging.
//!
//! Two related strategies live here:
//!
//! - **SQL seed files** ([`merge_seed_file`]) carry generator-emitted base
//!   data plus a `-- <<< CUSTOM SEED DATA >>>` marker below which users add
//!   their own rows. The marker section is preserved across regens.
//! - **`seed_order.yml`** ([`merge_seed_order`]) is an append-only list of
//!   seed identifiers. New seeds from the generator are appended under a
//!   "Newly added seeds" comment; the user's manual ordering above is
//!   preserved unchanged.

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Merge generated SQL seed content with the user's custom rows.
///
/// If the existing file has a `-- <<< CUSTOM SEED DATA >>>` marker, the
/// content below it is appended to the regenerated base. Otherwise the
/// generated content is used as-is.
pub(in crate::commands::schema) fn merge_seed_file(generated_content: &str, existing_path: &Path) -> Result<String> {
    if !existing_path.exists() {
        return Ok(generated_content.to_string());
    }

    let existing_content = fs::read_to_string(existing_path)
        .with_context(|| format!("Failed to read existing seed file: {:?}", existing_path))?;

    if let Some(custom_data) = extract_custom_seed_data(&existing_content) {
        Ok(format!("{}\n\n{}", generated_content.trim_end(), custom_data))
    } else {
        Ok(generated_content.to_string())
    }
}

/// Extract content below the `-- <<< CUSTOM SEED DATA >>>` marker, stripping
/// the common boilerplate "Add your custom seed data below" trailing comment
/// if present.
fn extract_custom_seed_data(content: &str) -> Option<String> {
    let marker = "-- <<< CUSTOM SEED DATA >>>";
    content.find(marker).map(|pos| {
        let after_marker = &content[pos + marker.len()..];
        let trimmed = after_marker.trim();
        let trailing_comments = [
            "-- Add your custom seed data below",
            "-- Add your custom seed data",
            "Add your custom seed data below",
        ];
        for comment in trailing_comments {
            if let Some(stripped) = trimmed.strip_prefix(comment) {
                return stripped.trim().to_string();
            }
        }
        trimmed.to_string()
    })
}

/// Merge `seed_order.yml`, appending new seeds below the user's manual list.
///
/// The user's existing ordering is preserved verbatim. Any seed names the
/// generator emits that aren't already in the file are listed below a
/// `# Newly added seeds` comment for the user to slot in.
pub(in crate::commands::schema) fn merge_seed_order(generated_content: &str, existing_path: &Path) -> Result<String> {
    if !existing_path.exists() {
        return Ok(generated_content.to_string());
    }

    let existing_content = fs::read_to_string(existing_path)
        .with_context(|| format!("Failed to read existing seed_order.yml: {:?}", existing_path))?;

    let generated_seeds = extract_seed_names(generated_content);

    let mut result = String::new();
    let mut existing_seeds = HashSet::new();
    let mut in_seed_list = false;

    for line in existing_content.lines() {
        process_seed_order_line(line, &mut in_seed_list, &mut existing_seeds, &mut result);
    }

    append_new_seeds(&generated_seeds, &existing_seeds, &mut result);

    Ok(result)
}

/// Extract seed names from YAML content (lines starting with `-`).
fn extract_seed_names(content: &str) -> HashSet<String> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed.strip_prefix('-')?.trim().to_string().into()
        })
        .collect()
}

/// Process a single line of `seed_order.yml` during merge.
fn process_seed_order_line(
    line: &str,
    in_seed_list: &mut bool,
    existing_seeds: &mut HashSet<String>,
    result: &mut String,
) {
    let trimmed = line.trim();

    if trimmed.starts_with('#') || trimmed.is_empty() {
        result.push_str(line);
        result.push('\n');
        return;
    }

    if trimmed.starts_with('-') {
        *in_seed_list = true;
        if let Some(seed_name) = trimmed.strip_prefix('-').map(|s| s.trim()) {
            existing_seeds.insert(seed_name.to_string());
        }
        result.push_str(line);
        result.push('\n');
        return;
    }

    if !*in_seed_list {
        result.push_str(line);
        result.push('\n');
    }
}

/// Append seeds present in `generated_seeds` but missing from `existing_seeds`,
/// under a "Newly added seeds" comment.
fn append_new_seeds(
    generated_seeds: &HashSet<String>,
    existing_seeds: &HashSet<String>,
    result: &mut String,
) {
    let new_seeds: Vec<_> = generated_seeds
        .iter()
        .filter(|seed| !existing_seeds.contains(*seed))
        .collect();

    if new_seeds.is_empty() {
        return;
    }

    result.push_str("\n# Newly added seeds (preserve manual order above):\n");
    for seed in new_seeds {
        result.push_str(&format!("- {}\n", seed));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_seed_names() {
        let content = "# Header\n- seed1\n- seed2\n# Comment\n- seed3";
        let seeds = extract_seed_names(content);

        assert_eq!(seeds.len(), 3);
        assert!(seeds.contains("seed1"));
        assert!(seeds.contains("seed2"));
        assert!(seeds.contains("seed3"));
    }

    #[test]
    fn test_merge_seed_order_nonexistent_file() {
        let generated = "- first_seed\n- second_seed\n- third_seed\n- new_seed\n";

        let result = merge_seed_order(generated, Path::new("nonexistent.yml")).unwrap();
        assert!(result.contains("new_seed"));
    }

    #[test]
    fn test_append_new_seeds() {
        let generated = "- seed1\n- seed2\n- seed3\n";
        let existing = "- seed1\n- seed2\n";

        let generated_seeds = extract_seed_names(generated);
        let existing_seeds = extract_seed_names(existing);
        let mut result = String::from("- seed1\n- seed2\n");

        append_new_seeds(&generated_seeds, &existing_seeds, &mut result);

        assert!(result.contains("seed3"));
        assert!(result.contains("Newly added seeds"));
    }

    #[test]
    fn test_append_new_seeds_empty_when_all_exist() {
        let generated = "- seed1\n- seed2\n";
        let existing = "- seed1\n- seed2\n";

        let generated_seeds = extract_seed_names(generated);
        let existing_seeds = extract_seed_names(existing);
        let mut result = String::from("- seed1\n- seed2\n");

        append_new_seeds(&generated_seeds, &existing_seeds, &mut result);

        assert!(!result.contains("Newly added seeds"));
    }
}
