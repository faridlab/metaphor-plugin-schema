//! Section markers for generated vs custom code
//!
//! Provides markers to distinguish between generated and custom sections
//! in output files, enabling non-destructive regeneration.

use serde::{Deserialize, Serialize};

/// Merge strategies for combining generated and existing content
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeStrategy {
    /// Always use newly generated content (overwrites custom changes)
    Overwrite,
    /// Keep existing content, only add new sections
    Preserve,
    /// Smart merge: preserve custom sections, update generated sections
    #[default]
    SmartMerge,
}

/// Types of sections in generated files
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionType {
    /// Fully generated section - can be safely regenerated
    Generated,
    /// Custom section - must be preserved during regeneration
    Custom,
    /// Partially customizable - generated base with custom additions
    Mixed,
}

/// Marker comments used to identify section boundaries
pub struct SectionMarker;

impl SectionMarker {
    /// Start marker for generated sections
    pub const GENERATED_START: &'static str = "# [GENERATED] - Do not edit below this line";
    /// End marker for generated sections
    pub const GENERATED_END: &'static str = "# [/GENERATED]";

    /// Start marker for custom sections
    pub const CUSTOM_START: &'static str = "# [CUSTOM] - Safe to edit below this line";
    /// End marker for custom sections
    pub const CUSTOM_END: &'static str = "# [/CUSTOM]";

    /// Start marker for paths section in OpenAPI
    pub const PATHS_GENERATED_START: &'static str = "# [PATHS:GENERATED]";
    /// End marker for paths section
    pub const PATHS_GENERATED_END: &'static str = "# [/PATHS:GENERATED]";

    /// Start marker for custom paths
    pub const PATHS_CUSTOM_START: &'static str = "# [PATHS:CUSTOM] - Add custom endpoints here";
    /// End marker for custom paths
    pub const PATHS_CUSTOM_END: &'static str = "# [/PATHS:CUSTOM]";

    /// Start marker for schemas section
    pub const SCHEMAS_GENERATED_START: &'static str = "# [SCHEMAS:GENERATED]";
    /// End marker for schemas section
    pub const SCHEMAS_GENERATED_END: &'static str = "# [/SCHEMAS:GENERATED]";

    /// Start marker for custom schemas
    pub const SCHEMAS_CUSTOM_START: &'static str = "# [SCHEMAS:CUSTOM] - Add custom schemas here";
    /// End marker for custom schemas
    pub const SCHEMAS_CUSTOM_END: &'static str = "# [/SCHEMAS:CUSTOM]";

    /// Check if a line is a section marker
    pub fn is_marker(line: &str) -> bool {
        let trimmed = line.trim();
        trimmed.starts_with("# [") && trimmed.contains(']')
    }

    /// Parse the section type from a marker line
    pub fn parse_marker(line: &str) -> Option<SectionType> {
        let trimmed = line.trim();
        if trimmed.contains("GENERATED") {
            Some(SectionType::Generated)
        } else if trimmed.contains("CUSTOM") {
            Some(SectionType::Custom)
        } else {
            None
        }
    }

    /// Check if a marker is a start marker
    pub fn is_start_marker(line: &str) -> bool {
        let trimmed = line.trim();
        trimmed.starts_with("# [") && !trimmed.starts_with("# [/")
    }

    /// Check if a marker is an end marker
    pub fn is_end_marker(line: &str) -> bool {
        let trimmed = line.trim();
        trimmed.starts_with("# [/")
    }

    /// Get the section name from a marker
    pub fn get_section_name(line: &str) -> Option<String> {
        let trimmed = line.trim();
        if !trimmed.starts_with("# [") {
            return None;
        }

        let start = 3; // After "# ["
        let end = trimmed.find(']')?;
        let section = &trimmed[start..end];

        // Handle end markers
        if let Some(stripped) = section.strip_prefix('/') {
            Some(stripped.to_string())
        } else {
            Some(section.to_string())
        }
    }

    /// Wrap content in generated markers
    pub fn wrap_generated(content: &str) -> String {
        format!(
            "{}\n{}\n{}",
            Self::GENERATED_START,
            content,
            Self::GENERATED_END
        )
    }

    /// Wrap content in custom markers
    pub fn wrap_custom(content: &str) -> String {
        format!(
            "{}\n{}\n{}",
            Self::CUSTOM_START,
            content,
            Self::CUSTOM_END
        )
    }

    /// Create an empty custom section placeholder
    pub fn custom_placeholder(section_name: &str) -> String {
        format!(
            "# [{section_name}:CUSTOM] - Add custom {section_name} here\n# [/{section_name}:CUSTOM]",
            section_name = section_name.to_uppercase()
        )
    }
}

/// Parsed section from a file
#[derive(Debug, Clone)]
pub struct ParsedSection {
    pub name: String,
    pub section_type: SectionType,
    pub content: String,
    pub _start_line: usize,
    pub _end_line: usize,
}

/// Parse sections from file content
pub fn parse_sections(content: &str) -> Vec<ParsedSection> {
    let mut sections = Vec::new();
    let mut current_section: Option<(String, SectionType, String, usize)> = None;
    let mut in_section = false;

    for (line_num, line) in content.lines().enumerate() {
        if SectionMarker::is_marker(line) {
            if SectionMarker::is_start_marker(line) {
                // Start new section
                let name = SectionMarker::get_section_name(line).unwrap_or_default();
                let section_type = SectionMarker::parse_marker(line).unwrap_or(SectionType::Generated);
                current_section = Some((name, section_type, String::new(), line_num));
                in_section = true;
            } else if SectionMarker::is_end_marker(line) && in_section {
                // End current section
                if let Some((name, section_type, content, start_line)) = current_section.take() {
                    sections.push(ParsedSection {
                        name,
                        section_type,
                        content,
                        _start_line: start_line,
                        _end_line: line_num,
                    });
                }
                in_section = false;
            }
        } else if in_section {
            if let Some((_, _, ref mut content, _)) = current_section {
                content.push_str(line);
                content.push('\n');
            }
        }
    }

    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_marker() {
        assert!(SectionMarker::is_marker("# [GENERATED] - Do not edit"));
        assert!(SectionMarker::is_marker("# [/GENERATED]"));
        assert!(SectionMarker::is_marker("# [CUSTOM] - Safe to edit"));
        assert!(!SectionMarker::is_marker("# Regular comment"));
        assert!(!SectionMarker::is_marker("not a comment"));
    }

    #[test]
    fn test_parse_marker() {
        assert_eq!(
            SectionMarker::parse_marker("# [GENERATED]"),
            Some(SectionType::Generated)
        );
        assert_eq!(
            SectionMarker::parse_marker("# [CUSTOM]"),
            Some(SectionType::Custom)
        );
        assert_eq!(SectionMarker::parse_marker("# [UNKNOWN]"), None);
    }

    #[test]
    fn test_get_section_name() {
        assert_eq!(
            SectionMarker::get_section_name("# [PATHS:GENERATED]"),
            Some("PATHS:GENERATED".to_string())
        );
        assert_eq!(
            SectionMarker::get_section_name("# [/PATHS:GENERATED]"),
            Some("PATHS:GENERATED".to_string())
        );
        assert_eq!(
            SectionMarker::get_section_name("not a marker"),
            None
        );
    }

    #[test]
    fn test_wrap_generated() {
        let content = "some content";
        let wrapped = SectionMarker::wrap_generated(content);
        assert!(wrapped.contains(SectionMarker::GENERATED_START));
        assert!(wrapped.contains(SectionMarker::GENERATED_END));
        assert!(wrapped.contains(content));
    }

    #[test]
    fn test_parse_sections() {
        let content = r#"# Header
# [GENERATED] - Do not edit below this line
generated content here
# [/GENERATED]

# [CUSTOM] - Safe to edit below this line
custom content here
# [/CUSTOM]
"#;
        let sections = parse_sections(content);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].section_type, SectionType::Generated);
        assert!(sections[0].content.contains("generated content"));
        assert_eq!(sections[1].section_type, SectionType::Custom);
        assert!(sections[1].content.contains("custom content"));
    }
}
