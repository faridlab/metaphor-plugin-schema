//! Value Object generator
//!
//! Generates Rust value objects from shared_types defined in index.model.yaml.
//! Value objects are immutable types without identity, defined by their attributes.
//!
//! ## DDD Value Objects
//!
//! Value Objects:
//! - Have no unique identifier
//! - Are immutable (replace, don't modify)
//! - Are defined by their attributes
//! - Have equality based on all attributes
//!
//! ## Generated Output
//!
//! For each shared_type, generates:
//! - `{type_name}.rs` - Value object struct with validation
//!
//! ## Validation Support
//!
//! Supports validation attributes:
//! - `@min_length(n)` - Minimum string length
//! - `@max_length(n)` - Maximum string length
//! - `@min(n)` - Minimum numeric value
//! - `@max(n)` - Maximum numeric value
//! - `@pattern(regex)` - Regex pattern matching
//! - `@email` - Email format validation
//! - `@url` - URL format validation

use super::{GenerateError, GeneratedOutput, Generator};
use crate::ast::{ValueObject, ValueObjectMethod, TypeRef};
use crate::parser::YamlField;
use crate::resolver::ResolvedSchema;
use crate::utils::to_snake_case;
use indexmap::IndexMap;
use std::fmt::Write;
use std::path::PathBuf;

/// Validation rule parsed from field attributes
#[derive(Debug, Clone, Default)]
struct ValidationRule {
    min_length: Option<usize>,
    max_length: Option<usize>,
    min_value: Option<String>,
    max_value: Option<String>,
    pattern: Option<String>,
    is_email: bool,
    is_url: bool,
}

/// Generates value objects from shared_types in schema
pub struct ValueObjectGenerator {
    /// Group generated files by model/domain
    /// Note: For value objects (shared types), this defaults to false
    /// as VOs are typically shared across models
    group_by_domain: bool,
}

impl ValueObjectGenerator {
    pub fn new() -> Self {
        Self {
            group_by_domain: false, // VOs are typically flat
        }
    }

    /// Set whether to group files by domain
    pub fn with_group_by_domain(mut self, group: bool) -> Self {
        self.group_by_domain = group;
        self
    }

    /// Parse validation rules from field attributes
    fn parse_validation_rules(&self, field: &YamlField) -> ValidationRule {
        let mut rule = ValidationRule::default();

        let attributes = match field {
            YamlField::Simple(_) => return rule,
            YamlField::Full { attributes, .. } => attributes,
        };

        for attr in attributes {
            if let Some(val) = attr.strip_prefix("@min_length(").and_then(|s| s.strip_suffix(')')) {
                rule.min_length = val.parse().ok();
            } else if let Some(val) = attr.strip_prefix("@max_length(").and_then(|s| s.strip_suffix(')')) {
                rule.max_length = val.parse().ok();
            } else if let Some(val) = attr.strip_prefix("@min(").and_then(|s| s.strip_suffix(')')) {
                rule.min_value = Some(val.to_string());
            } else if let Some(val) = attr.strip_prefix("@max(").and_then(|s| s.strip_suffix(')')) {
                rule.max_value = Some(val.to_string());
            } else if let Some(val) = attr.strip_prefix("@pattern(").and_then(|s| s.strip_suffix(')')) {
                rule.pattern = Some(val.trim_matches('"').to_string());
            } else if attr == "@email" {
                rule.is_email = true;
            } else if attr == "@url" {
                rule.is_url = true;
            }
        }

        rule
    }

    /// Check if field has any validation rules
    fn has_validation_rules(&self, field: &YamlField) -> bool {
        let rule = self.parse_validation_rules(field);
        rule.min_length.is_some()
            || rule.max_length.is_some()
            || rule.min_value.is_some()
            || rule.max_value.is_some()
            || rule.pattern.is_some()
            || rule.is_email
            || rule.is_url
    }

    /// Check if any field in the map has validation rules
    fn has_any_validation(&self, fields: &IndexMap<String, YamlField>) -> bool {
        fields.values().any(|f| self.has_validation_rules(f))
    }

    /// Generate validation code for a field
    fn generate_field_validation(
        &self,
        output: &mut String,
        field_name: &str,
        field: &YamlField,
        is_optional: bool,
    ) {
        let rule = self.parse_validation_rules(field);

        // Check if field is a string type
        let is_string_type = self.is_field_string_type(field);

        // Wrap in Option check if optional
        let accessor = if is_optional {
            format!("if let Some(ref val) = self.{}", field_name)
        } else {
            format!("let val = &self.{};", field_name)
        };

        let mut validations = Vec::new();

        if let Some(min) = rule.min_length {
            validations.push(format!(
                "if val.len() < {} {{ errors.push(format!(\"{} must be at least {} characters\")); }}",
                min, field_name, min
            ));
        }

        if let Some(max) = rule.max_length {
            validations.push(format!(
                "if val.len() > {} {{ errors.push(format!(\"{} must be at most {} characters\")); }}",
                max, field_name, max
            ));
        }

        // For @min/@max on string types, treat as length constraints
        if let Some(ref min) = rule.min_value {
            if is_string_type {
                validations.push(format!(
                    "if val.len() < {} {{ errors.push(format!(\"{} must be at least {} characters\")); }}",
                    min, field_name, min
                ));
            } else {
                validations.push(format!(
                    "if *val < {} {{ errors.push(format!(\"{} must be at least {}\")); }}",
                    min, field_name, min
                ));
            }
        }

        if let Some(ref max) = rule.max_value {
            if is_string_type {
                validations.push(format!(
                    "if val.len() > {} {{ errors.push(format!(\"{} must be at most {} characters\")); }}",
                    max, field_name, max
                ));
            } else {
                validations.push(format!(
                    "if *val > {} {{ errors.push(format!(\"{} must be at most {}\")); }}",
                    max, field_name, max
                ));
            }
        }

        if rule.is_email {
            validations.push(format!(
                "if !val.contains('@') || !val.contains('.') {{ errors.push(format!(\"{} must be a valid email address\")); }}",
                field_name
            ));
        }

        if rule.is_url {
            validations.push(format!(
                "if !val.starts_with(\"http://\") && !val.starts_with(\"https://\") {{ errors.push(format!(\"{} must be a valid URL\")); }}",
                field_name
            ));
        }

        if let Some(ref pattern) = rule.pattern {
            validations.push(format!(
                "if !regex::Regex::new(r\"{}\").unwrap().is_match(val) {{ errors.push(format!(\"{} does not match required pattern\")); }}",
                pattern, field_name
            ));
        }

        if !validations.is_empty() {
            if is_optional {
                writeln!(output, "        {} {{", accessor).unwrap();
                for v in validations {
                    writeln!(output, "            {}", v).unwrap();
                }
                writeln!(output, "        }}").unwrap();
            } else {
                writeln!(output, "        {}", accessor).unwrap();
                for v in validations {
                    writeln!(output, "        {}", v).unwrap();
                }
            }
        }
    }

    /// Generate a value object from a shared type
    fn generate_value_object(
        &self,
        name: &str,
        fields: &IndexMap<String, YamlField>,
    ) -> Result<String, GenerateError> {
        let mut output = String::new();
        let has_validation = self.has_any_validation(fields);
        let has_pattern_validation = fields.values().any(|f| {
            self.parse_validation_rules(f).pattern.is_some()
        });

        // Header
        writeln!(output, "//! {} value object", name).unwrap();
        writeln!(output, "//!").unwrap();
        writeln!(output, "//! Generated by metaphor-schema. Do not edit manually.").unwrap();
        writeln!(output, "//!").unwrap();
        writeln!(output, "//! This is a DDD Value Object - immutable, no identity, equality by attributes.").unwrap();
        writeln!(output).unwrap();

        // Conditional imports based on actual field types
        let needs_datetime = fields.values().any(|f| {
            let t = self.get_type_str(f).to_lowercase();
            let base = t.trim().trim_end_matches('?').trim_end_matches("[]");
            matches!(base, "datetime" | "timestamp")
        });
        let needs_date = fields.values().any(|f| {
            let t = self.get_type_str(f).to_lowercase();
            let base = t.trim().trim_end_matches('?').trim_end_matches("[]");
            base == "date"
        });
        let needs_time = fields.values().any(|f| {
            let t = self.get_type_str(f).to_lowercase();
            let base = t.trim().trim_end_matches('?').trim_end_matches("[]");
            base == "time"
        });
        let needs_decimal = fields.values().any(|f| {
            let t = self.get_type_str(f).to_lowercase();
            let base = t.trim().trim_end_matches('?').trim_end_matches("[]");
            matches!(base, "decimal" | "money")
        });
        let needs_uuid = fields.values().any(|f| {
            let t = self.get_type_str(f).to_lowercase();
            let base = t.trim().trim_end_matches('?').trim_end_matches("[]");
            matches!(base, "uuid" | "guid")
        });

        // Imports
        {
            let mut chrono_imports = Vec::new();
            if needs_datetime {
                chrono_imports.push("DateTime");
                chrono_imports.push("Utc");
            }
            if needs_date {
                chrono_imports.push("NaiveDate");
            }
            if needs_time {
                chrono_imports.push("NaiveTime");
            }
            if !chrono_imports.is_empty() {
                writeln!(output, "use chrono::{{{}}};", chrono_imports.join(", ")).unwrap();
            }
        }
        if needs_decimal {
            writeln!(output, "use rust_decimal::Decimal;").unwrap();
        }
        writeln!(output, "use serde::{{Deserialize, Serialize}};").unwrap();
        if needs_uuid {
            writeln!(output, "use uuid::Uuid;").unwrap();
        }
        if has_pattern_validation {
            writeln!(output, "use regex::Regex;").unwrap();
        }
        writeln!(output).unwrap();

        // Check if any field is a float type (f32/f64) - can't derive Eq/Hash for floats
        let has_float_fields = fields.values().any(|f| {
            let type_str = self.get_type_str(f).trim().trim_end_matches('?').trim_end_matches("[]");
            matches!(type_str.to_lowercase().as_str(), "float" | "f32" | "f64" | "double")
        });

        // Value Object struct derives
        if has_float_fields {
            writeln!(output, "#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]").unwrap();
        } else {
            writeln!(output, "#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]").unwrap();
        }

        // Struct definition
        writeln!(output, "pub struct {} {{", name).unwrap();

        for (field_name, field) in fields {
            let rust_type = self.yaml_field_to_rust_type(field);
            let is_optional = self.is_field_optional(field);

            // Add serde skip for optional fields
            if is_optional {
                writeln!(output, "    #[serde(skip_serializing_if = \"Option::is_none\")]").unwrap();
            }

            writeln!(output, "    pub {}: {},", field_name, rust_type).unwrap();
        }

        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Implement Default if all fields are optional or have defaults
        let all_optional_or_default = fields.iter().all(|(_, f)| {
            self.is_field_optional(f) || self.has_field_default(f)
        });

        if all_optional_or_default {
            writeln!(output, "impl Default for {} {{", name).unwrap();
            writeln!(output, "    fn default() -> Self {{").unwrap();
            writeln!(output, "        Self {{").unwrap();

            for (field_name, field) in fields {
                if self.is_field_optional(field) {
                    writeln!(output, "            {}: None,", field_name).unwrap();
                } else if let Some(default) = self.get_field_default(field) {
                    writeln!(output, "            {}: {},", field_name, default).unwrap();
                } else {
                    writeln!(output, "            {}: Default::default(),", field_name).unwrap();
                }
            }

            writeln!(output, "        }}").unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output, "}}").unwrap();
            writeln!(output).unwrap();
        }

        // Implementation block with builder and validation
        writeln!(output, "impl {} {{", name).unwrap();

        // Constructor
        writeln!(output, "    /// Create a new {} value object", name).unwrap();
        write!(output, "    pub fn new(").unwrap();

        let mut first = true;
        for (field_name, field) in fields {
            if !first {
                write!(output, ", ").unwrap();
            }
            first = false;

            let rust_type = self.yaml_field_to_rust_type(field);
            write!(output, "{}: {}", field_name, rust_type).unwrap();
        }

        writeln!(output, ") -> Self {{").unwrap();
        writeln!(output, "        Self {{").unwrap();

        for (field_name, _) in fields {
            writeln!(output, "            {},", field_name).unwrap();
        }

        writeln!(output, "        }}").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Builder pattern
        writeln!(output, "    /// Create a builder for {}", name).unwrap();
        writeln!(output, "    pub fn builder() -> {}Builder {{", name).unwrap();
        writeln!(output, "        {}Builder::default()", name).unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Generate validation methods if there are validation rules
        if has_validation {
            // validate() method
            writeln!(output, "    /// Validate all fields according to defined rules").unwrap();
            writeln!(output, "    ///").unwrap();
            writeln!(output, "    /// Returns Ok(()) if valid, Err with list of validation errors otherwise.").unwrap();
            writeln!(output, "    pub fn validate(&self) -> Result<(), Vec<String>> {{").unwrap();
            writeln!(output, "        let mut errors = Vec::new();").unwrap();
            writeln!(output).unwrap();

            for (field_name, field) in fields {
                if self.has_validation_rules(field) {
                    let is_optional = self.is_field_optional(field);
                    self.generate_field_validation(&mut output, field_name, field, is_optional);
                }
            }

            writeln!(output).unwrap();
            writeln!(output, "        if errors.is_empty() {{").unwrap();
            writeln!(output, "            Ok(())").unwrap();
            writeln!(output, "        }} else {{").unwrap();
            writeln!(output, "            Err(errors)").unwrap();
            writeln!(output, "        }}").unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();

            // is_valid() method
            writeln!(output, "    /// Check if the value object is valid").unwrap();
            writeln!(output, "    ///").unwrap();
            writeln!(output, "    /// Returns true if all validation rules pass.").unwrap();
            writeln!(output, "    pub fn is_valid(&self) -> bool {{").unwrap();
            writeln!(output, "        self.validate().is_ok()").unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();

            // validation_errors() method
            writeln!(output, "    /// Get validation errors if any").unwrap();
            writeln!(output, "    ///").unwrap();
            writeln!(output, "    /// Returns None if valid, Some with errors otherwise.").unwrap();
            writeln!(output, "    pub fn validation_errors(&self) -> Option<Vec<String>> {{").unwrap();
            writeln!(output, "        self.validate().err()").unwrap();
            writeln!(output, "    }}").unwrap();
        }

        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Generate builder
        self.generate_builder(&mut output, name, fields, has_validation)?;

        Ok(output)
    }

    // =========================================================================
    // COMPOSITION TYPE EXPANSION
    // =========================================================================

    /// Generate an expanded Metadata value object with actual audit fields.
    ///
    /// The Metadata shared type is defined as `Metadata: [Timestamps, Actors]` in the schema.
    /// When resolved for entity models, it becomes a single JSONB field for storage.
    /// However, for the value object, we expand it to show the actual fields for better type safety.
    ///
    /// Expanded fields from Timestamps + Actors:
    /// - created_at: DateTime<Utc>
    /// - updated_at: DateTime<Utc>
    /// - deleted_at: Option<DateTime<Utc>>
    /// - created_by: Option<Uuid>
    /// - updated_by: Option<Uuid>
    /// - deleted_by: Option<Uuid>
    fn generate_expanded_metadata_value_object(&self) -> Result<String, GenerateError> {
        let mut output = String::new();

        // Header
        writeln!(output, "//! Metadata value object").unwrap();
        writeln!(output, "//!").unwrap();
        writeln!(output, "//! Generated by metaphor-schema. Do not edit manually.").unwrap();
        writeln!(output, "//!").unwrap();
        writeln!(output, "//! This is a DDD Value Object - immutable, no identity, equality by attributes.").unwrap();
        writeln!(output, "//!").unwrap();
        writeln!(output, "//! Composition of: Timestamps + Actors").unwrap();
        writeln!(output, "//!").unwrap();
        writeln!(output, "//! Fields:").unwrap();
        writeln!(output, "//!   - created_at: DateTime<Utc> - When the record was created").unwrap();
        writeln!(output, "//!   - updated_at: DateTime<Utc> - When the record was last updated").unwrap();
        writeln!(output, "//!   - deleted_at: Option<DateTime<Utc>> - When the record was soft deleted").unwrap();
        writeln!(output, "//!   - created_by: Option<Uuid> - ID of user who created the record").unwrap();
        writeln!(output, "//!   - updated_by: Option<Uuid> - ID of user who last updated the record").unwrap();
        writeln!(output, "//!   - deleted_by: Option<Uuid> - ID of user who soft deleted the record").unwrap();
        writeln!(output).unwrap();

        // Imports
        writeln!(output, "use chrono::{{DateTime, Utc}};").unwrap();
        writeln!(output, "use serde::{{Deserialize, Serialize}};").unwrap();
        writeln!(output, "use uuid::Uuid;").unwrap();
        writeln!(output).unwrap();

        // Struct derives - DateTime<Utc> doesn't impl Eq/Hash, so exclude those
        writeln!(output, "#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]").unwrap();

        // Struct definition
        writeln!(output, "pub struct Metadata {{").unwrap();
        writeln!(output, "    /// When the record was created").unwrap();
        writeln!(output, "    pub created_at: DateTime<Utc>,").unwrap();
        writeln!(output, "    /// When the record was last updated").unwrap();
        writeln!(output, "    pub updated_at: DateTime<Utc>,").unwrap();
        writeln!(output, "    /// When the record was soft deleted (if applicable)").unwrap();
        writeln!(output, "    #[serde(skip_serializing_if = \"Option::is_none\")]").unwrap();
        writeln!(output, "    pub deleted_at: Option<DateTime<Utc>>,").unwrap();
        writeln!(output, "    /// ID of user who created the record").unwrap();
        writeln!(output, "    #[serde(skip_serializing_if = \"Option::is_none\")]").unwrap();
        writeln!(output, "    pub created_by: Option<Uuid>,").unwrap();
        writeln!(output, "    /// ID of user who last updated the record").unwrap();
        writeln!(output, "    #[serde(skip_serializing_if = \"Option::is_none\")]").unwrap();
        writeln!(output, "    pub updated_by: Option<Uuid>,").unwrap();
        writeln!(output, "    /// ID of user who soft deleted the record").unwrap();
        writeln!(output, "    #[serde(skip_serializing_if = \"Option::is_none\")]").unwrap();
        writeln!(output, "    pub deleted_by: Option<Uuid>,").unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Implementation
        writeln!(output, "impl Metadata {{").unwrap();

        // Constructor
        writeln!(output, "    /// Create a new Metadata value object").unwrap();
        writeln!(output, "    pub fn new(").unwrap();
        writeln!(output, "        created_at: DateTime<Utc>,").unwrap();
        writeln!(output, "        updated_at: DateTime<Utc>,").unwrap();
        writeln!(output, "        deleted_at: Option<DateTime<Utc>>,").unwrap();
        writeln!(output, "        created_by: Option<Uuid>,").unwrap();
        writeln!(output, "        updated_by: Option<Uuid>,").unwrap();
        writeln!(output, "        deleted_by: Option<Uuid>,").unwrap();
        writeln!(output, "    ) -> Self {{").unwrap();
        writeln!(output, "        Self {{").unwrap();
        writeln!(output, "            created_at,").unwrap();
        writeln!(output, "            updated_at,").unwrap();
        writeln!(output, "            deleted_at,").unwrap();
        writeln!(output, "            created_by,").unwrap();
        writeln!(output, "            updated_by,").unwrap();
        writeln!(output, "            deleted_by,").unwrap();
        writeln!(output, "        }}").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Convenience constructor for new records
        writeln!(output, "    /// Create metadata for a new record (created now)").unwrap();
        writeln!(output, "    pub fn new_record(created_by: Option<Uuid>) -> Self {{").unwrap();
        writeln!(output, "        let now = Utc::now();").unwrap();
        writeln!(output, "        Self {{").unwrap();
        writeln!(output, "            created_at: now,").unwrap();
        writeln!(output, "            updated_at: now,").unwrap();
        writeln!(output, "            deleted_at: None,").unwrap();
        writeln!(output, "            created_by,").unwrap();
        writeln!(output, "            updated_by: None,").unwrap();
        writeln!(output, "            deleted_by: None,").unwrap();
        writeln!(output, "        }}").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Domain state transition methods
        writeln!(output, "    // =================================================================").unwrap();
        writeln!(output, "    // Domain State Transition Methods").unwrap();
        writeln!(output, "    // =================================================================").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "    /// NOTE: These methods use &mut self for domain state transitions.").unwrap();
        writeln!(output, "    /// While value objects are typically immutable, audit metadata represents").unwrap();
        writeln!(output, "    /// a special case where the metadata tracks the lifecycle state of an entity.").unwrap();
        writeln!(output, "    ///").unwrap();
        writeln!(output, "    /// Mark the record as updated").unwrap();
        writeln!(output, "    ///").unwrap();
        writeln!(output, "    /// Updates the updated_at timestamp and sets the updated_by user.").unwrap();
        writeln!(output, "    /// This is a domain state transition that occurs when an entity is modified.").unwrap();
        writeln!(output, "    pub fn mark_updated(&mut self, updated_by: Option<Uuid>) {{").unwrap();
        writeln!(output, "        self.updated_at = Utc::now();").unwrap();
        writeln!(output, "        self.updated_by = updated_by;").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Mark the record as soft deleted").unwrap();
        writeln!(output, "    ///").unwrap();
        writeln!(output, "    /// Sets the deleted_at timestamp and deleted_by user.").unwrap();
        writeln!(output, "    /// This is a domain state transition that occurs when an entity is soft deleted.").unwrap();
        writeln!(output, "    pub fn mark_deleted(&mut self, deleted_by: Option<Uuid>) {{").unwrap();
        writeln!(output, "        self.deleted_at = Some(Utc::now());").unwrap();
        writeln!(output, "        self.deleted_by = deleted_by;").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Check if the record is soft deleted").unwrap();
        writeln!(output, "    pub fn is_deleted(&self) -> bool {{").unwrap();
        writeln!(output, "        self.deleted_at.is_some()").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Builder pattern
        writeln!(output, "    /// Create a builder for Metadata").unwrap();
        writeln!(output, "    pub fn builder() -> MetadataBuilder {{").unwrap();
        writeln!(output, "        MetadataBuilder::default()").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Convert to/from JSON value (for storage)
        writeln!(output, "    // =================================================================").unwrap();
        writeln!(output, "    // JSON conversion methods (for JSONB storage)").unwrap();
        writeln!(output, "    // =================================================================").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "    /// Convert to serde_json::Value for storage").unwrap();
        writeln!(output, "    pub fn to_json_value(&self) -> Result<serde_json::Value, serde_json::Error> {{").unwrap();
        writeln!(output, "        serde_json::to_value(self)").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "    /// Convert from serde_json::Value from storage").unwrap();
        writeln!(output, "    pub fn from_json_value(value: serde_json::Value) -> Result<Self, serde_json::Error> {{").unwrap();
        writeln!(output, "        serde_json::from_value(value)").unwrap();
        writeln!(output, "    }}").unwrap();

        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Generate builder
        self.generate_metadata_builder(&mut output);

        Ok(output)
    }

    /// Generate builder for Metadata value object
    fn generate_metadata_builder(&self, output: &mut String) {
        // Builder struct
        writeln!(output, "/// Builder for Metadata value object").unwrap();
        writeln!(output, "///").unwrap();
        writeln!(output, "/// # Example").unwrap();
        writeln!(output, "/// ```").unwrap();
        writeln!(output, "/// use chrono::Utc;").unwrap();
        writeln!(output, "/// use uuid::Uuid;").unwrap();
        writeln!(output, "///").unwrap();
        writeln!(output, "/// let metadata = Metadata::builder()").unwrap();
        writeln!(output, "///     .created_at(Utc::now())").unwrap();
        writeln!(output, "///     .updated_at(Utc::now())").unwrap();
        writeln!(output, "///     .created_by(Some(user_id))").unwrap();
        writeln!(output, "///     .build()").unwrap();
        writeln!(output, "/// ```").unwrap();
        writeln!(output, "#[derive(Debug, Clone, Default)]").unwrap();
        writeln!(output, "pub struct MetadataBuilder {{").unwrap();
        writeln!(output, "    created_at: Option<DateTime<Utc>>,").unwrap();
        writeln!(output, "    updated_at: Option<DateTime<Utc>>,").unwrap();
        writeln!(output, "    deleted_at: Option<DateTime<Utc>>,").unwrap();
        writeln!(output, "    created_by: Option<Uuid>,").unwrap();
        writeln!(output, "    updated_by: Option<Uuid>,").unwrap();
        writeln!(output, "    deleted_by: Option<Uuid>,").unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Builder implementation
        writeln!(output, "impl MetadataBuilder {{").unwrap();

        // Setter methods
        writeln!(output, "    /// Set the created_at timestamp").unwrap();
        writeln!(output, "    pub fn created_at(mut self, value: DateTime<Utc>) -> Self {{").unwrap();
        writeln!(output, "        self.created_at = Some(value);").unwrap();
        writeln!(output, "        self").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Set the updated_at timestamp").unwrap();
        writeln!(output, "    pub fn updated_at(mut self, value: DateTime<Utc>) -> Self {{").unwrap();
        writeln!(output, "        self.updated_at = Some(value);").unwrap();
        writeln!(output, "        self").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Set the deleted_at timestamp").unwrap();
        writeln!(output, "    pub fn deleted_at(mut self, value: DateTime<Utc>) -> Self {{").unwrap();
        writeln!(output, "        self.deleted_at = Some(value);").unwrap();
        writeln!(output, "        self").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Set the created_by user ID").unwrap();
        writeln!(output, "    pub fn created_by(mut self, value: Uuid) -> Self {{").unwrap();
        writeln!(output, "        self.created_by = Some(value);").unwrap();
        writeln!(output, "        self").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Set the updated_by user ID").unwrap();
        writeln!(output, "    pub fn updated_by(mut self, value: Uuid) -> Self {{").unwrap();
        writeln!(output, "        self.updated_by = Some(value);").unwrap();
        writeln!(output, "        self").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Set the deleted_by user ID").unwrap();
        writeln!(output, "    pub fn deleted_by(mut self, value: Uuid) -> Self {{").unwrap();
        writeln!(output, "        self.deleted_by = Some(value);").unwrap();
        writeln!(output, "        self").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Optional setters for None values
        writeln!(output, "    // =================================================================").unwrap();
        writeln!(output, "    // Optional field setters (explicit None)").unwrap();
        writeln!(output, "    // =================================================================").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Set created_by to None explicitly").unwrap();
        writeln!(output, "    pub fn no_created_by(mut self) -> Self {{").unwrap();
        writeln!(output, "        self.created_by = None;").unwrap();
        writeln!(output, "        self").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Set updated_by to None explicitly").unwrap();
        writeln!(output, "    pub fn no_updated_by(mut self) -> Self {{").unwrap();
        writeln!(output, "        self.updated_by = None;").unwrap();
        writeln!(output, "        self").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "    /// Set deleted_by to None explicitly").unwrap();
        writeln!(output, "    pub fn no_deleted_by(mut self) -> Self {{").unwrap();
        writeln!(output, "        self.deleted_by = None;").unwrap();
        writeln!(output, "        self").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Build method
        writeln!(output, "    /// Build the Metadata value object").unwrap();
        writeln!(output, "    ///").unwrap();
        writeln!(output, "    /// # Errors").unwrap();
        writeln!(output, "    /// Returns error if required fields (created_at, updated_at) are not set.").unwrap();
        writeln!(output, "    pub fn build(self) -> Result<Metadata, String> {{").unwrap();
        writeln!(output, "        let created_at = self.created_at.ok_or_else(|| \"created_at is required\".to_string())?;").unwrap();
        writeln!(output, "        let updated_at = self.updated_at.ok_or_else(|| \"updated_at is required\".to_string())?;").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "        Ok(Metadata {{").unwrap();
        writeln!(output, "            created_at,").unwrap();
        writeln!(output, "            updated_at,").unwrap();
        writeln!(output, "            deleted_at: self.deleted_at,").unwrap();
        writeln!(output, "            created_by: self.created_by,").unwrap();
        writeln!(output, "            updated_by: self.updated_by,").unwrap();
        writeln!(output, "            deleted_by: self.deleted_by,").unwrap();
        writeln!(output, "        }})").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        writeln!(output, "}}").unwrap();
    }

    // =========================================================================
    // DDD AST-BASED GENERATION
    // =========================================================================

    /// Generate a value object from DDD AST ValueObject definition
    fn generate_value_object_from_ast(
        &self,
        vo: &ValueObject,
    ) -> Result<String, GenerateError> {
        let mut output = String::new();
        let name = &vo.name;

        // Header
        writeln!(output, "//! {} value object", name).unwrap();
        writeln!(output, "//!").unwrap();
        writeln!(output, "//! Generated by metaphor-schema. Do not edit manually.").unwrap();
        writeln!(output, "//!").unwrap();
        writeln!(output, "//! This is a DDD Value Object - immutable, no identity, equality by attributes.").unwrap();
        if let Some(ref desc) = vo.description {
            writeln!(output, "//!").unwrap();
            writeln!(output, "//! {}", desc).unwrap();
        }
        writeln!(output).unwrap();

        // Conditional imports based on actual field types
        let needs_datetime = vo.fields.iter().any(|f| self.type_ref_needs_datetime(&f.type_ref));
        let needs_date = vo.fields.iter().any(|f| self.type_ref_needs_date(&f.type_ref));
        let needs_time = vo.fields.iter().any(|f| self.type_ref_needs_time(&f.type_ref));
        let needs_decimal = vo.fields.iter().any(|f| self.type_ref_needs_decimal(&f.type_ref));
        let needs_uuid = vo.fields.iter().any(|f| self.type_ref_needs_uuid(&f.type_ref));

        // Imports
        {
            let mut chrono_imports = Vec::new();
            if needs_datetime {
                chrono_imports.push("DateTime");
                chrono_imports.push("Utc");
            }
            if needs_date {
                chrono_imports.push("NaiveDate");
            }
            if needs_time {
                chrono_imports.push("NaiveTime");
            }
            if !chrono_imports.is_empty() {
                writeln!(output, "use chrono::{{{}}};", chrono_imports.join(", ")).unwrap();
            }
        }
        if needs_decimal {
            writeln!(output, "use rust_decimal::Decimal;").unwrap();
        }
        writeln!(output, "use serde::{{Deserialize, Serialize}};").unwrap();
        if needs_uuid {
            writeln!(output, "use uuid::Uuid;").unwrap();
        }
        writeln!(output).unwrap();

        // Determine derives - default plus custom
        let mut derives = vec![
            "Debug", "Clone", "PartialEq", "Eq", "Hash", "Serialize", "Deserialize"
        ];
        // Add custom derives (deduplicated)
        for custom in &vo.derives {
            if !derives.contains(&custom.as_str()) {
                derives.push(custom.as_str());
            }
        }

        if vo.is_simple() {
            // Simple value object (wrapper type)
            self.generate_simple_vo_from_ast(&mut output, vo, &derives)?;
        } else {
            // Composite value object (multiple fields)
            self.generate_composite_vo_from_ast(&mut output, vo, &derives)?;
        }

        Ok(output)
    }

    /// Generate a simple (wrapper) value object from AST
    fn generate_simple_vo_from_ast(
        &self,
        output: &mut String,
        vo: &ValueObject,
        derives: &[&str],
    ) -> Result<(), GenerateError> {
        let name = &vo.name;
        let inner_type = vo.inner_type.as_ref()
            .map(|t| self.type_ref_to_rust(t))
            .unwrap_or_else(|| "String".to_string());

        // Struct derives
        writeln!(output, "#[derive({})]", derives.join(", ")).unwrap();

        // Tuple struct for simple value objects
        writeln!(output, "pub struct {}(pub {});", name, inner_type).unwrap();
        writeln!(output).unwrap();

        // Implementation
        writeln!(output, "impl {} {{", name).unwrap();

        // Constructor
        writeln!(output, "    /// Create a new {}", name).unwrap();
        writeln!(output, "    pub fn new(value: {}) -> Self {{", inner_type).unwrap();
        writeln!(output, "        Self(value)").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Value accessor
        writeln!(output, "    /// Get the inner value").unwrap();
        writeln!(output, "    pub fn value(&self) -> &{} {{", inner_type).unwrap();
        writeln!(output, "        &self.0").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Into inner
        writeln!(output, "    /// Consume and return the inner value").unwrap();
        writeln!(output, "    pub fn into_inner(self) -> {} {{", inner_type).unwrap();
        writeln!(output, "        self.0").unwrap();
        writeln!(output, "    }}").unwrap();

        // Generate validation if present
        if let Some(ref validation) = vo.validation {
            writeln!(output).unwrap();
            writeln!(output, "    /// Validate the value").unwrap();
            writeln!(output, "    ///").unwrap();
            writeln!(output, "    /// Validation rule: {}", validation).unwrap();
            writeln!(output, "    pub fn validate(&self) -> Result<(), String> {{").unwrap();
            writeln!(output, "        // TODO: Implement validation: {}", validation).unwrap();
            writeln!(output, "        Ok(())").unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();

            writeln!(output, "    /// Create and validate in one step").unwrap();
            writeln!(output, "    pub fn try_new(value: {}) -> Result<Self, String> {{", inner_type).unwrap();
            writeln!(output, "        let vo = Self::new(value);").unwrap();
            writeln!(output, "        vo.validate()?;").unwrap();
            writeln!(output, "        Ok(vo)").unwrap();
            writeln!(output, "    }}").unwrap();
        }

        // Generate methods from AST
        if !vo.methods.is_empty() {
            writeln!(output).unwrap();
            writeln!(output, "    // ==========================================================").unwrap();
            writeln!(output, "    // Value Object Methods").unwrap();
            writeln!(output, "    // ==========================================================").unwrap();

            for method in &vo.methods {
                self.generate_vo_method(output, method)?;
            }
        }

        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Implement Display
        writeln!(output, "impl std::fmt::Display for {} {{", name).unwrap();
        writeln!(output, "    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{").unwrap();
        writeln!(output, "        write!(f, \"{{}}\", self.0)").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Implement From<inner_type>
        writeln!(output, "impl From<{}> for {} {{", inner_type, name).unwrap();
        writeln!(output, "    fn from(value: {}) -> Self {{", inner_type).unwrap();
        writeln!(output, "        Self(value)").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output, "}}").unwrap();

        // For String-based value objects, also generate From<&str> and From<&String>
        if inner_type == "String" {
            writeln!(output).unwrap();
            writeln!(output, "impl From<&str> for {} {{", name).unwrap();
            writeln!(output, "    fn from(value: &str) -> Self {{").unwrap();
            writeln!(output, "        Self(value.to_string())").unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output, "}}").unwrap();
            writeln!(output).unwrap();
            writeln!(output, "impl From<&String> for {} {{", name).unwrap();
            writeln!(output, "    fn from(value: &String) -> Self {{").unwrap();
            writeln!(output, "        Self(value.clone())").unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output, "}}").unwrap();
        }

        // AsRef<InnerType> for all simple value objects — enables generic access to inner value
        writeln!(output).unwrap();
        writeln!(output, "impl AsRef<{}> for {} {{", inner_type, name).unwrap();
        writeln!(output, "    fn as_ref(&self) -> &{} {{", inner_type).unwrap();
        writeln!(output, "        &self.0").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output, "}}").unwrap();

        Ok(())
    }

    /// Generate a composite (multi-field) value object from AST
    fn generate_composite_vo_from_ast(
        &self,
        output: &mut String,
        vo: &ValueObject,
        derives: &[&str],
    ) -> Result<(), GenerateError> {
        let name = &vo.name;

        // Struct derives
        writeln!(output, "#[derive({})]", derives.join(", ")).unwrap();

        // Struct definition
        writeln!(output, "pub struct {} {{", name).unwrap();

        for field in &vo.fields {
            let rust_type = self.type_ref_to_rust(&field.type_ref);

            if field.type_ref.is_optional() {
                writeln!(output, "    #[serde(skip_serializing_if = \"Option::is_none\")]").unwrap();
            }

            writeln!(output, "    pub {}: {},", field.name, rust_type).unwrap();
        }

        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Implementation
        writeln!(output, "impl {} {{", name).unwrap();

        // Constructor
        writeln!(output, "    /// Create a new {} value object", name).unwrap();
        write!(output, "    pub fn new(").unwrap();

        let mut first = true;
        for field in &vo.fields {
            if !first {
                write!(output, ", ").unwrap();
            }
            first = false;
            let rust_type = self.type_ref_to_rust(&field.type_ref);
            write!(output, "{}: {}", field.name, rust_type).unwrap();
        }

        writeln!(output, ") -> Self {{").unwrap();
        writeln!(output, "        Self {{").unwrap();

        for field in &vo.fields {
            writeln!(output, "            {},", field.name).unwrap();
        }

        writeln!(output, "        }}").unwrap();
        writeln!(output, "    }}").unwrap();

        // Generate validation if present
        if let Some(ref validation) = vo.validation {
            writeln!(output).unwrap();
            writeln!(output, "    /// Validate the value object").unwrap();
            writeln!(output, "    ///").unwrap();
            writeln!(output, "    /// Validation rule: {}", validation).unwrap();
            writeln!(output, "    pub fn validate(&self) -> Result<(), Vec<String>> {{").unwrap();
            writeln!(output, "        let mut errors = Vec::new();").unwrap();
            writeln!(output, "        // TODO: Implement validation: {}", validation).unwrap();
            writeln!(output, "        if errors.is_empty() {{ Ok(()) }} else {{ Err(errors) }}").unwrap();
            writeln!(output, "    }}").unwrap();
        }

        // Generate methods from AST
        if !vo.methods.is_empty() {
            writeln!(output).unwrap();
            writeln!(output, "    // ==========================================================").unwrap();
            writeln!(output, "    // Value Object Methods").unwrap();
            writeln!(output, "    // ==========================================================").unwrap();

            for method in &vo.methods {
                self.generate_vo_method(output, method)?;
            }
        }

        writeln!(output, "}}").unwrap();

        Ok(())
    }

    /// Generate a single value object method
    fn generate_vo_method(
        &self,
        output: &mut String,
        method: &ValueObjectMethod,
    ) -> Result<(), GenerateError> {
        writeln!(output).unwrap();

        // Add doc comment if description is provided
        if let Some(ref desc) = method.description {
            writeln!(output, "    /// {}", desc).unwrap();
        }

        // Build method signature - value objects are immutable so always &self
        let self_ref = "&self";

        // Build parameter list
        let params: Vec<String> = method.params.iter()
            .map(|(name, type_ref)| format!("{}: {}", name, self.type_ref_to_rust(type_ref)))
            .collect();

        let param_str = if params.is_empty() {
            String::new()
        } else {
            format!(", {}", params.join(", "))
        };

        // Return type
        let return_type = format!(" -> {}", self.type_ref_to_rust(&method.returns));

        // Write method signature
        writeln!(output, "    pub fn {}({}{}){}{{", method.name, self_ref, param_str, return_type).unwrap();
        writeln!(output, "        // TODO: Implement {} method", method.name).unwrap();
        writeln!(output, "        todo!(\"Implement {}\");", method.name).unwrap();
        writeln!(output, "    }}").unwrap();

        Ok(())
    }

    /// Convert TypeRef to Rust type string
    fn type_ref_to_rust(&self, type_ref: &TypeRef) -> String {
        match type_ref {
            TypeRef::Primitive(p) => p.rust_type().to_string(),
            TypeRef::Custom(name) => name.clone(),
            TypeRef::Array(inner) => format!("Vec<{}>", self.type_ref_to_rust(inner)),
            TypeRef::Optional(inner) => format!("Option<{}>", self.type_ref_to_rust(inner)),
            TypeRef::Map { key, value } => {
                format!(
                    "std::collections::HashMap<{}, {}>",
                    self.type_ref_to_rust(key),
                    self.type_ref_to_rust(value)
                )
            }
            TypeRef::ModuleRef { module, name } => format!("{}::{}", module, name),
        }
    }

    /// Generate builder struct for value object
    fn generate_builder(
        &self,
        output: &mut String,
        name: &str,
        fields: &IndexMap<String, YamlField>,
        has_validation: bool,
    ) -> Result<(), GenerateError> {
        // Builder struct
        writeln!(output, "/// Builder for {} value object", name).unwrap();
        writeln!(output, "#[derive(Debug, Clone, Default)]").unwrap();
        writeln!(output, "pub struct {}Builder {{", name).unwrap();

        for (field_name, field) in fields {
            let rust_type = self.yaml_field_to_rust_type(field);
            // All builder fields are Option
            if rust_type.starts_with("Option<") {
                writeln!(output, "    {}: {},", field_name, rust_type).unwrap();
            } else {
                writeln!(output, "    {}: Option<{}>,", field_name, rust_type).unwrap();
            }
        }

        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Builder implementation
        writeln!(output, "impl {}Builder {{", name).unwrap();

        // Setter methods
        for (field_name, field) in fields {
            let rust_type = self.yaml_field_to_rust_type(field);
            let inner_type = if rust_type.starts_with("Option<") {
                rust_type.strip_prefix("Option<").unwrap().strip_suffix('>').unwrap().to_string()
            } else {
                rust_type.clone()
            };

            writeln!(output, "    /// Set the {} field", field_name).unwrap();
            writeln!(output, "    pub fn {}(mut self, value: {}) -> Self {{", field_name, inner_type).unwrap();
            writeln!(output, "        self.{} = Some(value);", field_name).unwrap();
            writeln!(output, "        self").unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();
        }

        // Build method
        writeln!(output, "    /// Build the {} value object", name).unwrap();
        if has_validation {
            writeln!(output, "    ///").unwrap();
            writeln!(output, "    /// Validates the value object after construction.").unwrap();
        }
        writeln!(output, "    pub fn build(self) -> Result<{}, String> {{", name).unwrap();

        // Validate required fields
        for (field_name, field) in fields {
            if !self.is_field_optional(field) && !self.has_field_default(field) {
                writeln!(output, "        let {} = self.{}.ok_or_else(|| \"{} is required\".to_string())?;",
                    field_name, field_name, field_name).unwrap();
            }
        }

        writeln!(output).unwrap();
        writeln!(output, "        let value = {} {{", name).unwrap();

        for (field_name, field) in fields {
            if self.is_field_optional(field) {
                writeln!(output, "            {}: self.{},", field_name, field_name).unwrap();
            } else if self.has_field_default(field) {
                let default = self.get_field_default(field).unwrap_or_else(|| "Default::default()".to_string());
                writeln!(output, "            {}: self.{}.unwrap_or({}),", field_name, field_name, default).unwrap();
            } else {
                writeln!(output, "            {},", field_name).unwrap();
            }
        }

        writeln!(output, "        }};").unwrap();
        writeln!(output).unwrap();

        // Add validation if there are validation rules
        if has_validation {
            writeln!(output, "        // Validate the constructed value").unwrap();
            writeln!(output, "        value.validate().map_err(|errs| errs.join(\"; \"))?;").unwrap();
            writeln!(output).unwrap();
        }

        writeln!(output, "        Ok(value)").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Build unchecked method (skip validation)
        if has_validation {
            writeln!(output, "    /// Build the {} value object without validation", name).unwrap();
            writeln!(output, "    ///").unwrap();
            writeln!(output, "    /// Use with caution - skips all validation rules.").unwrap();
            writeln!(output, "    pub fn build_unchecked(self) -> Result<{}, String> {{", name).unwrap();

            // Validate required fields only
            for (field_name, field) in fields {
                if !self.is_field_optional(field) && !self.has_field_default(field) {
                    writeln!(output, "        let {} = self.{}.ok_or_else(|| \"{} is required\".to_string())?;",
                        field_name, field_name, field_name).unwrap();
                }
            }

            writeln!(output).unwrap();
            writeln!(output, "        Ok({} {{", name).unwrap();

            for (field_name, field) in fields {
                if self.is_field_optional(field) {
                    writeln!(output, "            {}: self.{},", field_name, field_name).unwrap();
                } else if self.has_field_default(field) {
                    let default = self.get_field_default(field).unwrap_or_else(|| "Default::default()".to_string());
                    writeln!(output, "            {}: self.{}.unwrap_or({}),", field_name, field_name, default).unwrap();
                } else {
                    writeln!(output, "            {},", field_name).unwrap();
                }
            }

            writeln!(output, "        }})").unwrap();
            writeln!(output, "    }}").unwrap();
        }

        writeln!(output, "}}").unwrap();

        Ok(())
    }

    /// Check if field is optional (type ends with ?)
    fn is_field_optional(&self, field: &YamlField) -> bool {
        match field {
            YamlField::Simple(s) => s.ends_with('?'),
            YamlField::Full { field_type, .. } => field_type.ends_with('?'),
        }
    }

    /// Check if field is a string type (for validation purposes)
    fn is_field_string_type(&self, field: &YamlField) -> bool {
        let type_str = self.get_type_str(field);
        let type_str = type_str.trim().trim_end_matches('?');

        matches!(type_str.to_lowercase().as_str(), "string" | "str")
    }

    /// Check if field has a default value
    fn has_field_default(&self, field: &YamlField) -> bool {
        match field {
            YamlField::Simple(_) => false,
            YamlField::Full { attributes, .. } => {
                attributes.iter().any(|a| a.starts_with("@default"))
            }
        }
    }

    /// Get field default value
    fn get_field_default(&self, field: &YamlField) -> Option<String> {
        match field {
            YamlField::Simple(_) => None,
            YamlField::Full { attributes, .. } => {
                attributes.iter()
                    .find(|a| a.starts_with("@default"))
                    .and_then(|a| {
                        // Parse @default(value) format
                        let content = a.strip_prefix("@default(")?;
                        let content = content.strip_suffix(')')?;
                        Some(self.convert_default_value(content))
                    })
            }
        }
    }

    /// Convert YAML default value to Rust expression
    fn convert_default_value(&self, value: &str) -> String {
        match value.trim() {
            "now" | "now()" => "chrono::Utc::now()".to_string(),
            "true" => "true".to_string(),
            "false" => "false".to_string(),
            "{}" => "serde_json::json!({})".to_string(),
            s if s.parse::<i64>().is_ok() => s.to_string(),
            s if s.parse::<f64>().is_ok() => s.to_string(),
            s => format!("\"{}\".to_string()", s),
        }
    }

    /// Get type string from YamlField
    fn get_type_str<'a>(&self, field: &'a YamlField) -> &'a str {
        match field {
            YamlField::Simple(s) => s.as_str(),
            YamlField::Full { field_type, .. } => field_type.as_str(),
        }
    }

    /// Convert YamlField to Rust type
    fn yaml_field_to_rust_type(&self, field: &YamlField) -> String {
        let type_str = self.get_type_str(field);
        let type_str = type_str.trim();

        // Check for optional and array
        let is_optional = type_str.ends_with('?');
        let is_array = type_str.ends_with("[]");

        // Strip modifiers to get base type
        let without_optional = type_str.strip_suffix('?').unwrap_or(type_str);
        let base_str = without_optional.strip_suffix("[]").unwrap_or(without_optional);

        let base_type = match base_str.to_lowercase().as_str() {
            "string" | "str" => "String".to_string(),
            "email" | "phone" | "url" | "slug" | "ip" => "String".to_string(),
            "int" | "integer" | "int32" | "i32" => "i32".to_string(),
            "int64" | "i64" | "long" => "i64".to_string(),
            "float" | "f64" | "double" => "f64".to_string(),
            "float32" | "f32" => "f32".to_string(),
            "bool" | "boolean" => "bool".to_string(),
            "uuid" | "guid" => "Uuid".to_string(),
            "datetime" | "timestamp" => "DateTime<Utc>".to_string(),
            "date" => "NaiveDate".to_string(),
            "time" => "NaiveTime".to_string(),
            "decimal" | "money" => "Decimal".to_string(),
            "bytes" | "binary" => "Vec<u8>".to_string(),
            "json" => "serde_json::Value".to_string(),
            other => other.to_string(), // Custom type reference
        };

        // Apply modifiers
        if is_array {
            format!("Vec<{}>", base_type)
        } else if is_optional {
            format!("Option<{}>", base_type)
        } else {
            base_type
        }
    }

    /// Check if a TypeRef needs DateTime/Utc imports
    fn type_ref_needs_datetime(&self, type_ref: &crate::ast::TypeRef) -> bool {
        use crate::ast::{TypeRef, PrimitiveType};
        match type_ref {
            TypeRef::Primitive(p) => matches!(p, PrimitiveType::DateTime | PrimitiveType::Timestamp),
            TypeRef::Optional(inner) | TypeRef::Array(inner) => self.type_ref_needs_datetime(inner),
            _ => false,
        }
    }

    /// Check if a TypeRef needs NaiveDate import
    fn type_ref_needs_date(&self, type_ref: &crate::ast::TypeRef) -> bool {
        use crate::ast::{TypeRef, PrimitiveType};
        match type_ref {
            TypeRef::Primitive(p) => matches!(p, PrimitiveType::Date),
            TypeRef::Optional(inner) | TypeRef::Array(inner) => self.type_ref_needs_date(inner),
            _ => false,
        }
    }

    /// Check if a TypeRef needs NaiveTime import
    fn type_ref_needs_time(&self, type_ref: &crate::ast::TypeRef) -> bool {
        use crate::ast::{TypeRef, PrimitiveType};
        match type_ref {
            TypeRef::Primitive(p) => matches!(p, PrimitiveType::Time),
            TypeRef::Optional(inner) | TypeRef::Array(inner) => self.type_ref_needs_time(inner),
            _ => false,
        }
    }

    /// Check if a TypeRef needs rust_decimal import
    fn type_ref_needs_decimal(&self, type_ref: &crate::ast::TypeRef) -> bool {
        use crate::ast::{TypeRef, PrimitiveType};
        match type_ref {
            TypeRef::Primitive(p) => matches!(p, PrimitiveType::Decimal | PrimitiveType::Money | PrimitiveType::Percentage),
            TypeRef::Optional(inner) | TypeRef::Array(inner) => self.type_ref_needs_decimal(inner),
            _ => false,
        }
    }

    /// Check if a TypeRef needs uuid import
    fn type_ref_needs_uuid(&self, type_ref: &crate::ast::TypeRef) -> bool {
        use crate::ast::{TypeRef, PrimitiveType};
        match type_ref {
            TypeRef::Primitive(p) => matches!(p, PrimitiveType::Uuid),
            TypeRef::Optional(inner) | TypeRef::Array(inner) => self.type_ref_needs_uuid(inner),
            _ => false,
        }
    }
}

impl Default for ValueObjectGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl Generator for ValueObjectGenerator {
    fn generate(&self, schema: &ResolvedSchema) -> Result<GeneratedOutput, GenerateError> {
        let mut output = GeneratedOutput::new();
        let has_shared_types = !schema.schema.shared_types.is_empty();
        let has_ddd_vos = !schema.schema.value_objects.is_empty();

        // Only generate if we have value objects from either source
        if !has_shared_types && !has_ddd_vos {
            return Ok(output);
        }

        let mut mod_content = String::new();
        writeln!(mod_content, "//! Value Objects").unwrap();
        writeln!(mod_content, "//!").unwrap();
        writeln!(mod_content, "//! Generated by metaphor-schema. Do not edit manually.").unwrap();
        writeln!(mod_content, "//!").unwrap();
        writeln!(mod_content, "//! DDD Value Objects - immutable types defined by their attributes.").unwrap();
        writeln!(mod_content).unwrap();

        // Track generated types for mod.rs
        let mut generated_types: Vec<(String, String, bool)> = Vec::new(); // (snake_name, type_name, has_builder)

        // =====================================================================
        // 1. Generate value objects from shared_types (legacy/simple types)
        // =====================================================================
        for (type_name, fields) in &schema.schema.shared_types {
            // Skip composition types (arrays like [Timestamps, Actors])
            if fields.is_empty() {
                continue;
            }

            let snake_name = to_snake_case(type_name);

            // Special handling for Metadata: expand into actual audit fields
            // The resolved shared_types has Metadata as {metadata: json} for entity use,
            // but for the value object we want the actual expanded fields.
            let vo_content = if type_name == "Metadata" && fields.len() == 1 && fields.contains_key("metadata") {
                self.generate_expanded_metadata_value_object()?
            } else {
                self.generate_value_object(type_name, fields)?
            };

            let vo_path = PathBuf::from(format!("src/domain/value_objects/{}.rs", snake_name));
            output.add_file(vo_path, vo_content);

            generated_types.push((snake_name, type_name.clone(), true)); // has builder
        }

        // =====================================================================
        // 2. Generate value objects from DDD AST (enhanced value objects)
        // =====================================================================
        for vo in &schema.schema.value_objects {
            let snake_name = to_snake_case(&vo.name);

            // Skip if already generated from shared_types (avoid duplicates)
            if generated_types.iter().any(|(_, name, _)| name == &vo.name) {
                continue;
            }

            // Generate value object file from AST
            let vo_content = self.generate_value_object_from_ast(vo)?;
            let vo_path = PathBuf::from(format!("src/domain/value_objects/{}.rs", snake_name));
            output.add_file(vo_path, vo_content);

            // Simple VOs don't have builders (they use From trait)
            let has_builder = vo.is_composite();
            generated_types.push((snake_name, vo.name.clone(), has_builder));
        }

        // =====================================================================
        // 3. Generate mod.rs with all modules and re-exports
        // =====================================================================
        // Module declarations
        for (snake_name, _, _) in &generated_types {
            writeln!(mod_content, "mod {};", snake_name).unwrap();
        }

        writeln!(mod_content).unwrap();
        writeln!(mod_content, "// Re-exports").unwrap();

        for (snake_name, type_name, has_builder) in &generated_types {
            writeln!(mod_content, "pub use {}::{};", snake_name, type_name).unwrap();
            if *has_builder {
                writeln!(mod_content, "pub use {}::{}Builder;", snake_name, type_name).unwrap();
            }
        }

        output.add_file(PathBuf::from("src/domain/value_objects/mod.rs"), mod_content);

        Ok(output)
    }

    fn name(&self) -> &'static str {
        "value_object"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ModuleSchema;

    fn create_test_schema() -> ResolvedSchema {
        let mut schema = ModuleSchema::new("test");

        // Add shared_types
        let mut timestamps = IndexMap::new();
        timestamps.insert("created_at".to_string(), YamlField::Simple("datetime".to_string()));
        timestamps.insert("updated_at".to_string(), YamlField::Simple("datetime".to_string()));
        timestamps.insert("deleted_at".to_string(), YamlField::Simple("datetime?".to_string()));

        schema.shared_types.insert("Timestamps".to_string(), timestamps);

        ResolvedSchema { schema }
    }

    #[test]
    fn test_value_object_generator_creates_files() {
        let schema = create_test_schema();
        let generator = ValueObjectGenerator::new();
        let output = generator.generate(&schema).unwrap();

        assert!(output.files.contains_key(&PathBuf::from("src/domain/value_objects/timestamps.rs")));
        assert!(output.files.contains_key(&PathBuf::from("src/domain/value_objects/mod.rs")));
    }

    #[test]
    fn test_value_object_has_builder() {
        let schema = create_test_schema();
        let generator = ValueObjectGenerator::new();
        let output = generator.generate(&schema).unwrap();

        let vo_content = output.files.get(&PathBuf::from("src/domain/value_objects/timestamps.rs")).unwrap();

        assert!(vo_content.contains("pub struct TimestampsBuilder"));
        assert!(vo_content.contains("pub fn builder()"));
        assert!(vo_content.contains("pub fn build(self)"));
    }

    #[test]
    fn test_value_object_is_immutable() {
        let schema = create_test_schema();
        let generator = ValueObjectGenerator::new();
        let output = generator.generate(&schema).unwrap();

        let vo_content = output.files.get(&PathBuf::from("src/domain/value_objects/timestamps.rs")).unwrap();

        // Should derive Clone, PartialEq, Eq for value object semantics
        assert!(vo_content.contains("Clone"));
        assert!(vo_content.contains("PartialEq"));
        assert!(vo_content.contains("Eq"));
    }

    fn create_validated_schema() -> ResolvedSchema {
        let mut schema = ModuleSchema::new("test");

        // Add shared_types with validation rules
        let mut contact = IndexMap::new();
        contact.insert("email".to_string(), YamlField::Full {
            field_type: "string".to_string(),
            attributes: vec!["@email".to_string(), "@max_length(255)".to_string()],
            description: None,
        });
        contact.insert("phone".to_string(), YamlField::Full {
            field_type: "string".to_string(),
            attributes: vec!["@min_length(10)".to_string(), "@max_length(20)".to_string()],
            description: None,
        });
        contact.insert("website".to_string(), YamlField::Full {
            field_type: "string?".to_string(),
            attributes: vec!["@url".to_string()],
            description: None,
        });

        schema.shared_types.insert("Contact".to_string(), contact);

        ResolvedSchema { schema }
    }

    #[test]
    fn test_value_object_has_validation() {
        let schema = create_validated_schema();
        let generator = ValueObjectGenerator::new();
        let output = generator.generate(&schema).unwrap();

        let vo_content = output.files.get(&PathBuf::from("src/domain/value_objects/contact.rs")).unwrap();

        // Should have validation methods
        assert!(vo_content.contains("pub fn validate(&self)"));
        assert!(vo_content.contains("pub fn is_valid(&self)"));
        assert!(vo_content.contains("pub fn validation_errors(&self)"));
    }

    #[test]
    fn test_builder_validates_on_build() {
        let schema = create_validated_schema();
        let generator = ValueObjectGenerator::new();
        let output = generator.generate(&schema).unwrap();

        let vo_content = output.files.get(&PathBuf::from("src/domain/value_objects/contact.rs")).unwrap();

        // Should validate in build method
        assert!(vo_content.contains("value.validate()"));
        // Should have build_unchecked for skipping validation
        assert!(vo_content.contains("pub fn build_unchecked(self)"));
    }

    #[test]
    fn test_validation_rules_parsing() {
        let generator = ValueObjectGenerator::new();

        // Test email validation
        let email_field = YamlField::Full {
            field_type: "string".to_string(),
            attributes: vec!["@email".to_string()],
            description: None,
        };
        let rule = generator.parse_validation_rules(&email_field);
        assert!(rule.is_email);

        // Test length validation
        let length_field = YamlField::Full {
            field_type: "string".to_string(),
            attributes: vec!["@min_length(5)".to_string(), "@max_length(100)".to_string()],
            description: None,
        };
        let rule = generator.parse_validation_rules(&length_field);
        assert_eq!(rule.min_length, Some(5));
        assert_eq!(rule.max_length, Some(100));

        // Test numeric validation
        let numeric_field = YamlField::Full {
            field_type: "int".to_string(),
            attributes: vec!["@min(0)".to_string(), "@max(100)".to_string()],
            description: None,
        };
        let rule = generator.parse_validation_rules(&numeric_field);
        assert_eq!(rule.min_value, Some("0".to_string()));
        assert_eq!(rule.max_value, Some("100".to_string()));
    }
}
