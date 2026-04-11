//! Git-aware merge utilities
//!
//! This module provides utilities for non-destructive code generation that
//! preserves custom sections while updating generated sections.

mod openapi_merger;
mod section_markers;

pub use openapi_merger::OpenApiMerger;
pub use section_markers::{MergeStrategy, SectionMarker, SectionType};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MergeError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parse error: {0}")]
    YamlParse(String),

    #[error("Invalid section marker: {0}")]
    InvalidMarker(String),

    #[error("Conflict detected: {0}")]
    Conflict(String),

    #[error("Missing required section: {0}")]
    MissingSection(String),
}

/// Result type for merge operations
pub type MergeResult<T> = Result<T, MergeError>;

/// Trait for mergeable content
pub trait Mergeable {
    /// Merge with another instance of the same type
    fn merge_with(&self, other: &Self, strategy: MergeStrategy) -> MergeResult<Self>
    where
        Self: Sized;

    /// Check if content has custom sections
    fn has_custom_sections(&self) -> bool;

    /// Extract custom sections from the content
    fn extract_custom_sections(&self) -> Vec<String>;
}

/// Diff result between two versions of a file
#[derive(Debug, Clone)]
pub struct DiffResult {
    /// Sections that were added in the new version
    pub added: Vec<String>,
    /// Sections that were removed from the old version
    pub removed: Vec<String>,
    /// Sections that were modified
    pub modified: Vec<ModifiedSection>,
    /// Whether there are conflicts
    pub has_conflicts: bool,
}

/// A section that was modified
#[derive(Debug, Clone)]
pub struct ModifiedSection {
    pub path: String,
    pub old_value: String,
    pub new_value: String,
}

/// Perform a three-way merge
pub fn three_way_merge(
    base: &str,
    current: &str,
    new: &str,
    strategy: MergeStrategy,
) -> MergeResult<String> {
    // Simple implementation - can be enhanced with proper diff algorithms
    match strategy {
        MergeStrategy::Overwrite => Ok(new.to_string()),
        MergeStrategy::Preserve => Ok(current.to_string()),
        MergeStrategy::SmartMerge => smart_merge(base, current, new),
    }
}

fn smart_merge(_base: &str, current: &str, new: &str) -> MergeResult<String> {
    // For now, implement a simple section-based merge
    // This can be enhanced with proper diff algorithms (e.g., diffy crate)

    let current_sections = parse_yaml_sections(current)?;
    let new_sections = parse_yaml_sections(new)?;

    let mut result = String::new();
    let mut used_sections = std::collections::HashSet::new();

    // First, process sections from the new content
    for (key, value) in &new_sections {
        if let Some(custom_value) = current_sections.get(key) {
            // Check if current has custom marker
            if custom_value.contains("[CUSTOM]") {
                result.push_str(custom_value);
            } else {
                result.push_str(value);
            }
        } else {
            result.push_str(value);
        }
        used_sections.insert(key.clone());
    }

    // Then, add any custom sections from current that aren't in new
    for (key, value) in &current_sections {
        if !used_sections.contains(key) && value.contains("[CUSTOM]") {
            result.push_str(value);
        }
    }

    Ok(result)
}

fn parse_yaml_sections(content: &str) -> MergeResult<indexmap::IndexMap<String, String>> {
    let mut sections = indexmap::IndexMap::new();
    let mut current_key = String::new();
    let mut current_content = String::new();

    for line in content.lines() {
        let trimmed = line.trim_start();
        let current_indent = line.len() - trimmed.len();

        // Detect top-level keys (no indentation or same as root level)
        if current_indent == 0 && !trimmed.is_empty() && !trimmed.starts_with('#') {
            // Save previous section
            if !current_key.is_empty() {
                sections.insert(current_key.clone(), current_content.clone());
            }

            // Start new section
            if let Some(colon_pos) = trimmed.find(':') {
                current_key = trimmed[..colon_pos].to_string();
                current_content = format!("{}\n", line);
            }
        } else if !current_key.is_empty() {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    // Don't forget the last section
    if !current_key.is_empty() {
        sections.insert(current_key, current_content);
    }

    Ok(sections)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_yaml_sections() {
        let yaml = r#"
openapi: '3.0.3'
info:
  title: Test API
  version: '1.0.0'
paths:
  /api/v1/users:
    get:
      summary: List users
"#;
        let sections = parse_yaml_sections(yaml).unwrap();

        assert!(sections.contains_key("openapi"));
        assert!(sections.contains_key("info"));
        assert!(sections.contains_key("paths"));
    }

    #[test]
    fn test_three_way_merge_overwrite() {
        let base = "old content";
        let current = "current content";
        let new = "new content";

        let result = three_way_merge(base, current, new, MergeStrategy::Overwrite).unwrap();
        assert_eq!(result, "new content");
    }

    #[test]
    fn test_three_way_merge_preserve() {
        let base = "old content";
        let current = "current content";
        let new = "new content";

        let result = three_way_merge(base, current, new, MergeStrategy::Preserve).unwrap();
        assert_eq!(result, "current content");
    }
}
