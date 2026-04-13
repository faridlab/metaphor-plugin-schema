//! Type mapping utilities for Schema → TypeScript/Zod conversion
//!
//! This module provides mappings from Backbone schema types to:
//! - TypeScript types
//! - Zod validation schemas
//! - Default values

use crate::webgen::ast::entity::{FieldType, FieldAttribute, FieldDefinition, EnumDefinition};

/// Type mapper for Schema → TypeScript/Zod conversion
#[derive(Debug, Clone)]
pub struct TypeMapper {
    // Configuration could be added here in the future
}

impl Default for TypeMapper {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeMapper {
    /// Create a new type mapper
    pub fn new() -> Self {
        Self {}
    }

    /// Convert a schema field type to TypeScript type
    pub fn to_typescript_type(&self, field_type: &FieldType, optional: bool) -> String {
        let base_type = self.base_typescript_type(field_type);
        if optional {
            format!("{} | null", base_type)
        } else {
            base_type
        }
    }

    /// Get base TypeScript type without nullability
    #[allow(clippy::only_used_in_recursion)]
    fn base_typescript_type(&self, field_type: &FieldType) -> String {
        match field_type {
            FieldType::String | FieldType::Text => "string".to_string(),
            FieldType::Int => "number".to_string(),
            FieldType::Float | FieldType::Decimal => "number".to_string(),
            FieldType::Bool => "boolean".to_string(),
            FieldType::Uuid => "string".to_string(),
            FieldType::DateTime => "Date".to_string(),
            FieldType::Date => "string".to_string(), // ISO date string
            FieldType::Time => "string".to_string(), // ISO time string
            FieldType::Email => "string".to_string(),
            FieldType::Phone => "string".to_string(),
            FieldType::Url => "string".to_string(),
            FieldType::Json => "Record<string, unknown>".to_string(),
            FieldType::Ip => "string".to_string(), // IP address as string
            FieldType::Enum(name) => name.clone(),
            FieldType::Custom(name) => name.clone(),
            FieldType::Array(inner) => format!("{}[]", self.base_typescript_type(inner)),
            FieldType::Optional(inner) => format!("{} | null", self.base_typescript_type(inner)),
        }
    }

    /// Convert a schema field type to Zod schema
    pub fn to_zod_schema(&self, field: &FieldDefinition, enums: &[EnumDefinition]) -> String {
        let base_schema = Self::base_zod_schema(&field.type_name, enums);
        let validations = self.zod_validations(field);

        let mut schema = format!("{}{}", base_schema, validations);

        // Handle optionality
        if field.optional {
            schema = format!("{}.nullable().optional()", schema);
        }

        // Handle default value
        if let Some(default) = &field.default_value {
            schema = format!("{}.default({})", schema, Self::format_default_value(default, &field.type_name));
        }

        schema
    }

    /// Get base Zod schema for a field type
    fn base_zod_schema(field_type: &FieldType, enums: &[EnumDefinition]) -> String {
        match field_type {
            FieldType::String | FieldType::Text => "z.string()".to_string(),
            FieldType::Int => "z.number().int()".to_string(),
            FieldType::Float | FieldType::Decimal => "z.number()".to_string(),
            FieldType::Bool => "z.boolean()".to_string(),
            FieldType::Uuid => "z.string().uuid()".to_string(),
            FieldType::DateTime => "z.string().datetime()".to_string(),
            FieldType::Date => "z.string().date()".to_string(),
            FieldType::Time => "z.string().time()".to_string(),
            FieldType::Email => "z.string().email()".to_string(),
            FieldType::Phone => "z.string()".to_string(),
            FieldType::Url => "z.string().url()".to_string(),
            FieldType::Json => "z.record(z.unknown())".to_string(),
            FieldType::Ip => "ipSchema".to_string(), // IP address validation
            FieldType::Enum(name) => {
                if let Some(enum_def) = enums.iter().find(|e| &e.name == name) {
                    let variants: Vec<String> = enum_def.variants.iter()
                        .map(|v| format!("'{}'", v.name))
                        .collect();
                    format!("z.enum([{}])", variants.join(", "))
                } else {
                    format!("{}Schema", name)
                }
            }
            FieldType::Custom(name) => {
                // Check if it's an enum
                if let Some(enum_def) = enums.iter().find(|e| &e.name == name) {
                    let variants: Vec<String> = enum_def.variants.iter()
                        .map(|v| format!("'{}'", v.name))
                        .collect();
                    format!("z.enum([{}])", variants.join(", "))
                } else {
                    // Assume it's another entity reference
                    format!("{}Schema", name)
                }
            }
            FieldType::Array(inner) => {
                format!("z.array({})", Self::base_zod_schema(inner, enums))
            }
            FieldType::Optional(inner) => {
                Self::base_zod_schema(inner, enums)
            }
        }
    }

    /// Generate Zod validation chains from field attributes
    fn zod_validations(&self, field: &FieldDefinition) -> String {
        let mut validations = Vec::new();

        for attr in &field.attributes {
            if let Some(validation) = self.attribute_to_zod_validation(attr, &field.type_name) {
                validations.push(validation);
            }
        }

        validations.join("")
    }

    /// Convert a single attribute to Zod validation
    fn attribute_to_zod_validation(&self, attr: &FieldAttribute, field_type: &FieldType) -> Option<String> {
        match attr.name.as_str() {
            "min" => {
                let arg = attr.first_arg()?;
                let is_string = matches!(
                    field_type,
                    FieldType::String | FieldType::Text | FieldType::Email | FieldType::Phone | FieldType::Url | FieldType::Ip
                );
                if is_string {
                    Some(format!(".min({}, {{ message: 'Must be at least {} characters' }})", arg, arg))
                } else {
                    Some(format!(".min({}, {{ message: 'Must be at least {}' }})", arg, arg))
                }
            }
            "max" => {
                let arg = attr.first_arg()?;
                let is_string = matches!(
                    field_type,
                    FieldType::String | FieldType::Text | FieldType::Email | FieldType::Phone | FieldType::Url | FieldType::Ip
                );
                if is_string {
                    Some(format!(".max({}, {{ message: 'Must be at most {} characters' }})", arg, arg))
                } else {
                    Some(format!(".max({}, {{ message: 'Must be at most {}' }})", arg, arg))
                }
            }
            "length" => {
                let arg = attr.first_arg()?;
                Some(format!(".length({}, {{ message: 'Must be exactly {} characters' }})", arg, arg))
            }
            "pattern" | "regex" => {
                let pattern = attr.first_arg()?;
                Some(format!(".regex(/{}/)", pattern))
            }
            "alpha_dash" => {
                Some(".regex(/^[a-zA-Z0-9_-]+$/, { message: 'Only alphanumeric, underscore, and hyphen allowed' })".to_string())
            }
            "alpha_num" => {
                Some(".regex(/^[a-zA-Z0-9]+$/, { message: 'Only alphanumeric characters allowed' })".to_string())
            }
            "positive" => {
                Some(".positive({ message: 'Must be a positive number' })".to_string())
            }
            "negative" => {
                Some(".negative({ message: 'Must be a negative number' })".to_string())
            }
            "non_negative" | "nonnegative" => {
                Some(".nonnegative({ message: 'Must be zero or positive' })".to_string())
            }
            "non_positive" | "nonpositive" => {
                Some(".nonpositive({ message: 'Must be zero or negative' })".to_string())
            }
            "integer" | "int" => {
                Some(".int({ message: 'Must be an integer' })".to_string())
            }
            "finite" => {
                Some(".finite({ message: 'Must be a finite number' })".to_string())
            }
            "safe" => {
                Some(".safe({ message: 'Must be a safe integer' })".to_string())
            }
            "trim" => {
                Some(".trim()".to_string())
            }
            "lowercase" => {
                Some(".toLowerCase()".to_string())
            }
            "uppercase" => {
                Some(".toUpperCase()".to_string())
            }
            "includes" => {
                let arg = attr.first_arg()?;
                Some(format!(".includes('{}', {{ message: \"Must include '{}'\" }})", arg, arg))
            }
            "starts_with" | "startsWith" => {
                let arg = attr.first_arg()?;
                Some(format!(".startsWith('{}', {{ message: \"Must start with '{}'\" }})", arg, arg))
            }
            "ends_with" | "endsWith" => {
                let arg = attr.first_arg()?;
                Some(format!(".endsWith('{}', {{ message: \"Must end with '{}'\" }})", arg, arg))
            }
            _ => None,
        }
    }

    /// Format a default value for Zod
    fn format_default_value(value: &str, field_type: &FieldType) -> String {
        match field_type {
            FieldType::String | FieldType::Text | FieldType::Email |
            FieldType::Phone | FieldType::Url | FieldType::Uuid |
            FieldType::Date | FieldType::Time | FieldType::DateTime | FieldType::Ip => {
                // Quote strings unless already quoted
                if value.starts_with('"') || value.starts_with('\'') {
                    value.to_string()
                } else {
                    format!("'{}'", value)
                }
            }
            FieldType::Bool => {
                if value == "true" || value == "false" {
                    value.to_string()
                } else {
                    "false".to_string()
                }
            }
            FieldType::Int | FieldType::Float | FieldType::Decimal => {
                value.to_string()
            }
            FieldType::Array(_) => {
                if value == "[]" || value.starts_with('[') {
                    value.to_string()
                } else {
                    "[]".to_string()
                }
            }
            FieldType::Json => {
                if value.starts_with('{') {
                    value.to_string()
                } else {
                    "{}".to_string()
                }
            }
            FieldType::Enum(_) | FieldType::Custom(_) => {
                if value.starts_with('"') || value.starts_with('\'') {
                    value.to_string()
                } else {
                    format!("'{}'", value)
                }
            }
            FieldType::Optional(inner) => {
                Self::format_default_value(value, inner)
            }
        }
    }

    /// Get default value for a field type
    pub fn default_value_for_type(&self, field_type: &FieldType) -> String {
        match field_type {
            FieldType::String | FieldType::Text | FieldType::Email |
            FieldType::Phone | FieldType::Url | FieldType::Ip => "''".to_string(),
            FieldType::Uuid => "crypto.randomUUID()".to_string(),
            FieldType::Int => "0".to_string(),
            FieldType::Float | FieldType::Decimal => "0.0".to_string(),
            FieldType::Bool => "false".to_string(),
            FieldType::DateTime => "new Date()".to_string(),
            FieldType::Date => "new Date().toISOString().split('T')[0]".to_string(),
            FieldType::Time => "new Date().toISOString().split('T')[1].split('.')[0]".to_string(),
            FieldType::Json => "{}".to_string(),
            FieldType::Array(_) => "[]".to_string(),
            FieldType::Enum(name) | FieldType::Custom(name) => {
                // Return first variant or empty string
                format!("'' as {}", name)
            }
            FieldType::Optional(inner) => {
                format!("null as {} | null", self.base_typescript_type(inner))
            }
        }
    }

    /// Check if a field is considered a value object
    pub fn is_value_object_field(&self, field: &FieldDefinition) -> bool {
        // Check by name patterns
        let name = field.name.to_lowercase();
        let is_vo_name = name.ends_with("_address") ||
            name.ends_with("_email") ||
            name.ends_with("_phone") ||
            name.ends_with("_money") ||
            name.ends_with("_amount") ||
            name.ends_with("_price") ||
            name == "email" ||
            name == "phone" ||
            name == "address";

        // Check by type
        let is_vo_type = matches!(
            &field.type_name,
            FieldType::Email | FieldType::Phone | FieldType::Url
        );

        // Check by attribute
        let has_vo_attr = field.attributes.iter().any(|a| a.name == "value_object");

        is_vo_name || is_vo_type || has_vo_attr
    }

    /// Determine value object type from field
    pub fn detect_value_object_type(&self, field: &FieldDefinition) -> Option<ValueObjectType> {
        let name = field.name.to_lowercase();

        // Check field type first
        match &field.type_name {
            FieldType::Email => return Some(ValueObjectType::Email),
            FieldType::Phone => return Some(ValueObjectType::Phone),
            FieldType::Url => return Some(ValueObjectType::Url),
            _ => {}
        }

        // Check by name pattern
        if name.contains("email") {
            return Some(ValueObjectType::Email);
        }
        if name.contains("phone") || name.contains("mobile") || name.contains("tel") {
            return Some(ValueObjectType::Phone);
        }
        if name.contains("address") && !name.contains("email") {
            return Some(ValueObjectType::Address);
        }
        if name.contains("money") || name.contains("amount") || name.contains("price") || name.contains("cost") {
            return Some(ValueObjectType::Money);
        }
        if name.contains("url") || name.contains("website") || name.contains("link") {
            return Some(ValueObjectType::Url);
        }

        None
    }
}

/// Common value object types
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ValueObjectType {
    Email,
    Phone,
    Address,
    Money,
    Url,
    PersonName,
    Identifier,
    Custom(String),
}

impl ValueObjectType {
    /// Get the TypeScript interface name
    pub fn type_name(&self) -> &str {
        match self {
            ValueObjectType::Email => "Email",
            ValueObjectType::Phone => "PhoneNumber",
            ValueObjectType::Address => "Address",
            ValueObjectType::Money => "Money",
            ValueObjectType::Url => "Url",
            ValueObjectType::PersonName => "PersonName",
            ValueObjectType::Identifier => "Identifier",
            ValueObjectType::Custom(name) => name,
        }
    }

    /// Get the file name (without extension)
    pub fn file_name(&self) -> &str {
        match self {
            ValueObjectType::Email => "Email",
            ValueObjectType::Phone => "PhoneNumber",
            ValueObjectType::Address => "Address",
            ValueObjectType::Money => "Money",
            ValueObjectType::Url => "Url",
            ValueObjectType::PersonName => "PersonName",
            ValueObjectType::Identifier => "Identifier",
            ValueObjectType::Custom(name) => name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_typescript_type_mapping() {
        let mapper = TypeMapper::new();

        assert_eq!(mapper.to_typescript_type(&FieldType::String, false), "string");
        assert_eq!(mapper.to_typescript_type(&FieldType::String, true), "string | null");
        assert_eq!(mapper.to_typescript_type(&FieldType::Int, false), "number");
        assert_eq!(mapper.to_typescript_type(&FieldType::Bool, false), "boolean");
        assert_eq!(mapper.to_typescript_type(&FieldType::DateTime, false), "Date");
        assert_eq!(mapper.to_typescript_type(&FieldType::Uuid, false), "string");
    }

    #[test]
    fn test_zod_schema_mapping() {
        let mapper = TypeMapper::new();
        let enums: Vec<EnumDefinition> = vec![];

        let field = FieldDefinition {
            name: "email".to_string(),
            type_name: FieldType::Email,
            attributes: vec![],
            description: None,
            optional: false,
            default_value: None,
        };

        let schema = mapper.to_zod_schema(&field, &enums);
        assert!(schema.contains("z.string().email()"));
    }

    #[test]
    fn test_value_object_detection() {
        let mapper = TypeMapper::new();

        let email_field = FieldDefinition {
            name: "user_email".to_string(),
            type_name: FieldType::String,
            attributes: vec![],
            description: None,
            optional: false,
            default_value: None,
        };

        assert!(mapper.is_value_object_field(&email_field));
        assert_eq!(mapper.detect_value_object_type(&email_field), Some(ValueObjectType::Email));
    }
}
