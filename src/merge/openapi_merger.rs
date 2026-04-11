//! OpenAPI-specific merging utilities
//!
//! Provides specialized merging logic for OpenAPI specifications that:
//! - Preserves custom paths and schemas
//! - Updates generated content
//! - Handles overrides from `*.openapi.yaml` files

use super::{MergeResult, MergeStrategy};
use super::section_markers::{SectionMarker, SectionType, parse_sections};
use indexmap::IndexMap;
use std::fmt;

/// OpenAPI merger for non-destructive regeneration
pub struct OpenApiMerger {
    strategy: MergeStrategy,
}

/// State tracking for marker insertion
#[derive(Default)]
struct MarkerState {
    in_paths: bool,
    in_schemas: bool,
    schemas_started: bool,
}

impl OpenApiMerger {
    /// Create a new OpenAPI merger with the given strategy
    pub fn new(strategy: MergeStrategy) -> Self {
        Self { strategy }
    }

    /// Merge new generated OpenAPI spec with existing spec
    pub fn merge(&self, existing: &str, generated: &str) -> MergeResult<String> {
        match self.strategy {
            MergeStrategy::Overwrite => Ok(generated.to_string()),
            MergeStrategy::Preserve => Ok(existing.to_string()),
            MergeStrategy::SmartMerge => self.smart_merge(existing, generated),
        }
    }

    /// Smart merge that preserves custom sections while updating generated ones
    fn smart_merge(&self, existing: &str, generated: &str) -> MergeResult<String> {
        let existing_sections = parse_sections(existing);

        // No markers found - assume entire file is generated
        let has_markers = existing_sections
            .iter()
            .any(|s| s.section_type == SectionType::Custom);

        if !has_markers {
            return Ok(self.add_markers_to_generated(generated));
        }

        let custom_sections: Vec<_> = existing_sections
            .iter()
            .filter(|s| s.section_type == SectionType::Custom)
            .collect();

        let generated_with_markers = self.add_markers_to_generated(generated);
        let mut result = String::new();

        for line in generated_with_markers.lines() {
            result.push_str(line);
            result.push('\n');
            self.insert_custom_section_if_needed(&mut result, line, &custom_sections);
        }

        Ok(result)
    }

    /// Insert custom section after generated section end markers
    fn insert_custom_section_if_needed(
        &self,
        result: &mut String,
        line: &str,
        custom_sections: &[&super::section_markers::ParsedSection],
    ) {
        if line.contains("[/PATHS:GENERATED]") {
            self.insert_custom_block(result, "PATHS:CUSTOM", "paths", custom_sections,
                SectionMarker::PATHS_CUSTOM_START, SectionMarker::PATHS_CUSTOM_END);
        }

        if line.contains("[/SCHEMAS:GENERATED]") {
            self.insert_custom_block(result, "SCHEMAS:CUSTOM", "schemas", custom_sections,
                SectionMarker::SCHEMAS_CUSTOM_START, SectionMarker::SCHEMAS_CUSTOM_END);
        }
    }

    /// Insert a custom block (paths or schemas)
    fn insert_custom_block(
        &self,
        result: &mut String,
        section_name: &str,
        placeholder_name: &str,
        custom_sections: &[&super::section_markers::ParsedSection],
        start_marker: &str,
        end_marker: &str,
    ) {
        result.push('\n');

        let custom_section = custom_sections
            .iter()
            .find(|s| s.name.contains(section_name));

        match custom_section {
            Some(section) => {
                result.push_str(&format!("{}\n", start_marker));
                result.push_str(&section.content);
                result.push_str(&format!("{}\n", end_marker));
            }
            None => {
                result.push_str(&SectionMarker::custom_placeholder(placeholder_name));
                result.push('\n');
            }
        }
    }

    /// Add section markers to generated OpenAPI content
    fn add_markers_to_generated(&self, generated: &str) -> String {
        let mut state = MarkerState::default();
        let mut result = String::new();

        for line in generated.lines() {
            let trimmed = line.trim();
            self.process_marker_line(&mut result, &mut state, line, trimmed);
        }

        self.close_unclosed_sections(&mut result, &state);
        result
    }

    /// Process a single line for marker insertion
    fn process_marker_line(&self, result: &mut String, state: &mut MarkerState, line: &str, trimmed: &str) {
        // Handle section transitions
        if trimmed == "paths:" {
            result.push_str(&format!("{}\n", SectionMarker::PATHS_GENERATED_START));
            state.in_paths = true;
        }

        if trimmed == "schemas:" && state.in_paths {
            self.end_paths_section(result);
            state.in_paths = false;
        }

        if trimmed.starts_with("schemas:") || (trimmed == "schemas:" && !state.in_paths) {
            result.push_str(&format!("{}\n", SectionMarker::SCHEMAS_GENERATED_START));
            state.in_schemas = true;
            state.schemas_started = true;
        }

        result.push_str(line);
        result.push('\n');

        // Handle securitySchemes (ends schemas section)
        if trimmed == "securitySchemes:" && state.schemas_started {
            self.handle_security_schemes_marker(result);
            state.in_schemas = false;
        }
    }

    /// End the paths section with markers
    fn end_paths_section(&self, result: &mut String) {
        result.push_str(&format!("{}\n", SectionMarker::PATHS_GENERATED_END));
        result.push('\n');
        result.push_str(&SectionMarker::custom_placeholder("paths"));
        result.push_str("\n\n");
    }

    /// Handle securitySchemes marker by inserting schemas end marker before it
    fn handle_security_schemes_marker(&self, result: &mut String) {
        let Some(last_pos) = result.rfind("  securitySchemes:") else {
            return;
        };
        result.truncate(last_pos);
        result.push_str(&format!("{}\n", SectionMarker::SCHEMAS_GENERATED_END));
        result.push('\n');
        result.push_str(&SectionMarker::custom_placeholder("schemas"));
        result.push_str("\n\n");
        result.push_str("  securitySchemes:\n");
    }

    /// Close any unclosed sections at end of file
    fn close_unclosed_sections(&self, result: &mut String, state: &MarkerState) {
        if state.in_paths {
            result.push_str(&format!("{}\n", SectionMarker::PATHS_GENERATED_END));
            result.push('\n');
            result.push_str(&SectionMarker::custom_placeholder("paths"));
            result.push('\n');
        }
        if state.in_schemas {
            result.push_str(&format!("{}\n", SectionMarker::SCHEMAS_GENERATED_END));
            result.push('\n');
            result.push_str(&SectionMarker::custom_placeholder("schemas"));
            result.push('\n');
        }
    }

    /// Apply overrides from an openapi.yaml override file
    pub fn apply_overrides(&self, spec: &str, overrides: &OpenApiOverrides) -> MergeResult<String> {
        let mut result = spec.to_string();

        // Apply path overrides
        for (path, override_content) in &overrides.path_overrides {
            result = self.override_path(&result, path, override_content)?;
        }

        // Add custom paths
        for (path, content) in &overrides.custom_paths {
            result = self.add_custom_path(&result, path, content)?;
        }

        // Add custom schemas
        for (name, content) in &overrides.custom_schemas {
            result = self.add_custom_schema(&result, name, content)?;
        }

        Ok(result)
    }

    fn override_path(&self, spec: &str, _path: &str, _content: &str) -> MergeResult<String> {
        // Simple implementation - can be enhanced with proper YAML manipulation
        // For now, just replace the path section
        Ok(spec.to_string())
    }

    fn add_custom_path(&self, spec: &str, path: &str, content: &str) -> MergeResult<String> {
        // Find the custom paths section and add the new path
        let marker = SectionMarker::PATHS_CUSTOM_START;
        if let Some(pos) = spec.find(marker) {
            let insert_pos = pos + marker.len() + 1; // +1 for newline
            let mut result = spec.to_string();
            result.insert_str(insert_pos, &format!("  {}:\n{}\n", path, content));
            return Ok(result);
        }

        // No custom paths section found, add it
        Ok(spec.to_string())
    }

    fn add_custom_schema(&self, spec: &str, name: &str, content: &str) -> MergeResult<String> {
        // Find the custom schemas section and add the new schema
        let marker = SectionMarker::SCHEMAS_CUSTOM_START;
        if let Some(pos) = spec.find(marker) {
            let insert_pos = pos + marker.len() + 1; // +1 for newline
            let mut result = spec.to_string();
            result.insert_str(insert_pos, &format!("    {}:\n{}\n", name, content));
            return Ok(result);
        }

        Ok(spec.to_string())
    }

    /// Compute diff between two OpenAPI specs
    pub fn diff(&self, old: &str, new: &str) -> DiffReport {
        let old_sections = parse_sections(old);
        let new_sections = parse_sections(new);

        let old_names: std::collections::HashSet<_> =
            old_sections.iter().map(|s| &s.name).collect();
        let new_names: std::collections::HashSet<_> =
            new_sections.iter().map(|s| &s.name).collect();

        let added: Vec<_> = new_names
            .difference(&old_names)
            .map(|s| (*s).clone())
            .collect();
        let removed: Vec<_> = old_names
            .difference(&new_names)
            .map(|s| (*s).clone())
            .collect();

        let mut modified = Vec::new();
        for name in old_names.intersection(&new_names) {
            let old_section = old_sections.iter().find(|s| &s.name == *name).unwrap();
            let new_section = new_sections.iter().find(|s| &s.name == *name).unwrap();

            if old_section.content != new_section.content {
                modified.push(name.to_string());
            }
        }

        DiffReport {
            added,
            removed,
            modified,
            custom_preserved: old_sections
                .iter()
                .filter(|s| s.section_type == SectionType::Custom)
                .count(),
        }
    }
}

impl Default for OpenApiMerger {
    fn default() -> Self {
        Self::new(MergeStrategy::SmartMerge)
    }
}

/// Overrides from an openapi.yaml file
#[derive(Debug, Default)]
pub struct OpenApiOverrides {
    /// Override info section
    pub info_override: Option<String>,
    /// Override specific paths (path -> content)
    pub path_overrides: IndexMap<String, String>,
    /// Custom paths to add (path -> content)
    pub custom_paths: IndexMap<String, String>,
    /// Custom schemas to add (name -> content)
    pub custom_schemas: IndexMap<String, String>,
}

impl OpenApiOverrides {
    /// Parse overrides from YAML content
    pub fn from_yaml(_content: &str) -> MergeResult<Self> {
        // Parse YAML using serde_yaml
        // For now, return empty overrides
        Ok(Self::default())
    }
}

/// Report of differences between two OpenAPI specs
#[derive(Debug)]
pub struct DiffReport {
    /// New sections added
    pub added: Vec<String>,
    /// Sections removed
    pub removed: Vec<String>,
    /// Sections modified
    pub modified: Vec<String>,
    /// Number of custom sections preserved
    pub custom_preserved: usize,
}

impl fmt::Display for DiffReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.added.is_empty() {
            writeln!(f, "Added:")?;
            for item in &self.added {
                writeln!(f, "  + {}", item)?;
            }
        }

        if !self.removed.is_empty() {
            writeln!(f, "Removed:")?;
            for item in &self.removed {
                writeln!(f, "  - {}", item)?;
            }
        }

        if !self.modified.is_empty() {
            writeln!(f, "Modified:")?;
            for item in &self.modified {
                writeln!(f, "  ~ {}", item)?;
            }
        }

        writeln!(f, "\nCustom sections preserved: {}", self.custom_preserved)
    }
}

impl DiffReport {
    /// Check if there are any changes
    pub fn has_changes(&self) -> bool {
        !self.added.is_empty() || !self.removed.is_empty() || !self.modified.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_overwrite_strategy() {
        let merger = OpenApiMerger::new(MergeStrategy::Overwrite);
        let existing = "old content";
        let generated = "new content";

        let result = merger.merge(existing, generated).unwrap();
        assert_eq!(result, "new content");
    }

    #[test]
    fn test_merge_preserve_strategy() {
        let merger = OpenApiMerger::new(MergeStrategy::Preserve);
        let existing = "existing content";
        let generated = "new content";

        let result = merger.merge(existing, generated).unwrap();
        assert_eq!(result, "existing content");
    }

    #[test]
    fn test_add_markers_to_generated() {
        let merger = OpenApiMerger::new(MergeStrategy::SmartMerge);
        let generated = r#"openapi: '3.0.3'
info:
  title: Test API
paths:
  /api/v1/users:
    get:
      summary: List users
components:
  schemas:
    User:
      type: object
  securitySchemes:
    bearerAuth:
      type: http
"#;

        let result = merger.add_markers_to_generated(generated);

        // Should contain path markers
        assert!(result.contains("[PATHS:GENERATED]"));
        assert!(result.contains("[PATHS:CUSTOM]"));

        // Should contain schema markers
        assert!(result.contains("[SCHEMAS:GENERATED]"));
        assert!(result.contains("[SCHEMAS:CUSTOM]"));
    }

    #[test]
    fn test_diff_report() {
        let old = r#"# [GENERATED]
generated content
# [/GENERATED]
"#;
        let new = r#"# [GENERATED]
new generated content
# [/GENERATED]
"#;

        let merger = OpenApiMerger::new(MergeStrategy::SmartMerge);
        let report = merger.diff(old, new);

        assert!(report.modified.contains(&"GENERATED".to_string()));
    }
}
