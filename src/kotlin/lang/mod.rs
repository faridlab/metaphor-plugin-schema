//! Kotlin type mapping and code generation utilities

use crate::ast::{PrimitiveType, TypeRef};
use std::collections::HashMap;

/// Kotlin type mapper for converting schema types to Kotlin types
pub struct KotlinTypeMapper {
    /// Custom type mappings (e.g., for value objects)
    custom_mappings: HashMap<String, String>,
}

impl KotlinTypeMapper {
    /// Create a new Kotlin type mapper
    pub fn new() -> Self {
        let mut custom_mappings = HashMap::new();

        // Add common custom type mappings
        custom_mappings.insert("Email".to_string(), "String".to_string());
        custom_mappings.insert("PhoneNumber".to_string(), "String".to_string());
        custom_mappings.insert("Url".to_string(), "String".to_string());
        custom_mappings.insert("Uuid".to_string(), "String".to_string());
        custom_mappings.insert("Decimal".to_string(), "Double".to_string());
        custom_mappings.insert("Money".to_string(), "Double".to_string());
        custom_mappings.insert("Percentage".to_string(), "Double".to_string());
        custom_mappings.insert("Json".to_string(), "JsonElement".to_string());
        custom_mappings.insert("Timestamp".to_string(), "Instant".to_string());

        Self { custom_mappings }
    }

    /// Convert a TypeRef to Kotlin type string
    pub fn to_kotlin_type(&self, type_ref: &TypeRef) -> String {
        self.to_kotlin_type_with_optional(type_ref, false)
    }

    /// Convert a TypeRef to Kotlin type string WITHOUT optional marker
    /// Used in templates where nullable marker is added separately
    pub fn to_kotlin_type_non_nullable(&self, type_ref: &TypeRef) -> String {
        // Unwrap Optional types to get the base type
        let unwrapped = match type_ref {
            TypeRef::Optional(inner) => inner.as_ref(),
            _ => type_ref,
        };
        self.to_kotlin_type_with_optional(unwrapped, false)
    }

    /// Convert a TypeRef to Kotlin type string with optional handling
    pub fn to_kotlin_type_with_optional(&self, type_ref: &TypeRef, include_optional: bool) -> String {
        match type_ref {
            TypeRef::Primitive(primitive) => {
                let base_type = self.primitive_to_kotlin(primitive);
                if include_optional {
                    format!("{}?", base_type)
                } else {
                    base_type
                }
            }
            TypeRef::Custom(name) => {
                let base_type = self
                    .custom_mappings
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| name.clone());
                if include_optional {
                    format!("{}?", base_type)
                } else {
                    base_type
                }
            }
            TypeRef::Array(inner) => {
                let inner_type = self.to_kotlin_type_with_optional(inner, false);
                format!("List<{}>", inner_type)
            }
            TypeRef::Map { key, value } => {
                let key_type = self.to_kotlin_type_with_optional(key, false);
                let value_type = self.to_kotlin_type_with_optional(value, false);
                format!("Map<{}, {}>", key_type, value_type)
            }
            TypeRef::Optional(inner) => {
                self.to_kotlin_type_with_optional(inner, true)
            }
            TypeRef::ModuleRef { module, name } => {
                format!("{}.{}", module, name)
            }
        }
    }

    /// Convert a primitive type to Kotlin type
    pub fn primitive_to_kotlin(&self, primitive: &PrimitiveType) -> String {
        match primitive {
            // String types
            PrimitiveType::String
            | PrimitiveType::Email
            | PrimitiveType::Url
            | PrimitiveType::Phone
            | PrimitiveType::Slug
            | PrimitiveType::Ip
            | PrimitiveType::IpV4
            | PrimitiveType::IpV6
            | PrimitiveType::Mac
            | PrimitiveType::Markdown
            | PrimitiveType::Html => "String".to_string(),

            // Integer types
            PrimitiveType::Int | PrimitiveType::Int32 => "Int".to_string(),
            PrimitiveType::Int64 => "Long".to_string(),

            // Float types
            PrimitiveType::Float | PrimitiveType::Float32 | PrimitiveType::Float64 => "Double".to_string(),

            // Boolean
            PrimitiveType::Bool => "Boolean".to_string(),

            // Binary types
            PrimitiveType::Bytes | PrimitiveType::Binary | PrimitiveType::Base64 => "ByteArray".to_string(),

            // Special types - use kotlinx.datetime
            PrimitiveType::DateTime | PrimitiveType::Timestamp => "Instant".to_string(),
            PrimitiveType::Date => "LocalDate".to_string(),
            PrimitiveType::Time => "LocalTime".to_string(),
            PrimitiveType::Duration => "Duration".to_string(),

            // UUID as string for KMP compatibility
            PrimitiveType::Uuid => "String".to_string(),

            // JSON as JsonElement (handles both [] and {} from API)
            PrimitiveType::Json => "JsonElement".to_string(),

            // Decimal types as Double for KMP
            PrimitiveType::Decimal | PrimitiveType::Money | PrimitiveType::Percentage => {
                "Double".to_string()
            }
        }
    }

    /// Convert a primitive type to SQLDelight type
    pub fn to_sqldelight_type(&self, primitive: &PrimitiveType) -> String {
        match primitive {
            // String types
            PrimitiveType::String
            | PrimitiveType::Email
            | PrimitiveType::Url
            | PrimitiveType::Phone
            | PrimitiveType::Slug
            | PrimitiveType::Ip
            | PrimitiveType::IpV4
            | PrimitiveType::IpV6
            | PrimitiveType::Mac
            | PrimitiveType::Markdown
            | PrimitiveType::Html
            | PrimitiveType::Uuid => "TEXT".to_string(),

            // Integer types
            PrimitiveType::Int | PrimitiveType::Int32 => "INTEGER".to_string(),
            PrimitiveType::Int64 => "INTEGER".to_string(),

            // Float types
            PrimitiveType::Float | PrimitiveType::Float32 | PrimitiveType::Float64 => "REAL".to_string(),

            // Boolean as INTEGER
            PrimitiveType::Bool => "INTEGER".to_string(),

            // Binary types
            PrimitiveType::Bytes | PrimitiveType::Binary | PrimitiveType::Base64 => "BLOB".to_string(),

            // Date/Time types
            PrimitiveType::DateTime | PrimitiveType::Timestamp => "INTEGER".to_string(), // Unix timestamp
            PrimitiveType::Date => "TEXT".to_string(),
            PrimitiveType::Time => "TEXT".to_string(),
            PrimitiveType::Duration => "INTEGER".to_string(),

            // JSON
            PrimitiveType::Json => "TEXT".to_string(),

            // Decimal types
            PrimitiveType::Decimal | PrimitiveType::Money | PrimitiveType::Percentage => {
                "REAL".to_string()
            }
        }
    }

    /// Get the default value for a type
    pub fn default_value(&self, type_ref: &TypeRef) -> String {
        match type_ref {
            TypeRef::Primitive(primitive) => match primitive {
                PrimitiveType::String | PrimitiveType::Email | PrimitiveType::Url => "\"\"".to_string(),
                PrimitiveType::Int | PrimitiveType::Int32 => "0".to_string(),
                PrimitiveType::Int64 => "0L".to_string(),
                PrimitiveType::Float | PrimitiveType::Float32 | PrimitiveType::Float64 => "0.0".to_string(),
                PrimitiveType::Bool => "false".to_string(),
                PrimitiveType::DateTime | PrimitiveType::Timestamp => {
                    "Clock.System.now()".to_string()
                }
                _ => "\"\"".to_string(),
            },
            TypeRef::Array(_) => "emptyList()".to_string(),
            TypeRef::Map { .. } => "emptyMap()".to_string(),
            TypeRef::Optional(_) => "null".to_string(),
            TypeRef::Custom(_) => "null".to_string(),
            TypeRef::ModuleRef { .. } => "null".to_string(),
        }
    }

    /// Convert a field name to Kotlin property name (camelCase)
    pub fn to_kotlin_property_name(&self, name: &str) -> String {
        // If it's already camelCase or has underscores, convert properly
        let chars: Vec<char> = name.chars().collect();
        let mut result = String::new();
        let mut capitalize_next = false;

        for c in &chars {
            if *c == '_' {
                capitalize_next = true;
            } else if capitalize_next {
                result.extend(c.to_uppercase());
                capitalize_next = false;
            } else {
                result.push(*c);
            }
        }

        // Ensure first character is lowercase for property names
        if let Some(first) = result.chars().next() {
            if first.is_uppercase() && result.len() > 1 {
                let mut capitalized = String::new();
                capitalized.extend(first.to_lowercase());
                capitalized.push_str(&result[1..]);
                capitalized
            } else {
                result
            }
        } else {
            result
        }
    }

    /// Convert a field name to Kotlin class name (PascalCase)
    pub fn to_kotlin_class_name(&self, name: &str) -> String {
        let mut result = String::new();
        let mut capitalize_next = true;

        for c in name.chars() {
            if c == '_' {
                capitalize_next = true;
            } else if capitalize_next {
                result.extend(c.to_uppercase());
                capitalize_next = false;
            } else {
                result.push(c);
            }
        }

        result
    }
}

impl Default for KotlinTypeMapper {
    fn default() -> Self {
        Self::new()
    }
}

/// Kotlin naming utilities
pub struct KotlinNaming;

impl KotlinNaming {
    /// Convert to snake_case
    pub fn to_snake_case(name: &str) -> String {
        let mut result = String::new();
        for (i, c) in name.chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                result.push('_');
                result.extend(c.to_lowercase());
            } else {
                result.extend(c.to_lowercase());
            }
        }
        result
    }

    /// Convert to camelCase
    pub fn to_camel_case(name: &str) -> String {
        let mut result = String::new();
        let mut capitalize_next = false;

        for c in name.chars() {
            if c == '_' {
                capitalize_next = true;
            } else if capitalize_next {
                result.extend(c.to_uppercase());
                capitalize_next = false;
            } else {
                result.push(c);
            }
        }

        result
    }

    /// Convert to PascalCase
    pub fn to_pascal_case(name: &str) -> String {
        let camel = Self::to_camel_case(name);
        let mut result = String::new();
        if let Some(first) = camel.chars().next() {
            result.extend(first.to_uppercase());
            result.push_str(&camel[1..]);
        }
        result
    }

    /// Pluralize a name
    pub fn pluralize(name: &str) -> String {
        if let Some(stripped) = name.strip_suffix('y') {
            format!("{}ies", stripped)
        } else if name.ends_with('s') || name.ends_with('x') || name.ends_with('z') {
            format!("{}es", name)
        } else {
            format!("{}s", name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primitive_to_kotlin() {
        let mapper = KotlinTypeMapper::new();

        assert_eq!(mapper.primitive_to_kotlin(&PrimitiveType::String), "String");
        assert_eq!(mapper.primitive_to_kotlin(&PrimitiveType::Int), "Int");
        assert_eq!(mapper.primitive_to_kotlin(&PrimitiveType::Int64), "Long");
        assert_eq!(mapper.primitive_to_kotlin(&PrimitiveType::Float), "Double");
        assert_eq!(mapper.primitive_to_kotlin(&PrimitiveType::Bool), "Boolean");
        assert_eq!(mapper.primitive_to_kotlin(&PrimitiveType::DateTime), "Instant");
        assert_eq!(mapper.primitive_to_kotlin(&PrimitiveType::Uuid), "String");
    }

    #[test]
    fn test_to_kotlin_type() {
        let mapper = KotlinTypeMapper::new();

        // Optional string
        let optional_string = TypeRef::Optional(Box::new(TypeRef::Primitive(PrimitiveType::String)));
        assert_eq!(mapper.to_kotlin_type(&optional_string), "String?");

        // Array of integers
        let array_int = TypeRef::Array(Box::new(TypeRef::Primitive(PrimitiveType::Int)));
        assert_eq!(mapper.to_kotlin_type(&array_int), "List<Int>");

        // Map type
        let map_type = TypeRef::Map {
            key: Box::new(TypeRef::Primitive(PrimitiveType::String)),
            value: Box::new(TypeRef::Primitive(PrimitiveType::Int)),
        };
        assert_eq!(mapper.to_kotlin_type(&map_type), "Map<String, Int>");
    }

    #[test]
    fn test_to_kotlin_property_name() {
        let mapper = KotlinTypeMapper::new();

        assert_eq!(mapper.to_kotlin_property_name("user_id"), "userId");
        assert_eq!(mapper.to_kotlin_property_name("firstName"), "firstName");
        assert_eq!(mapper.to_kotlin_property_name("created_at"), "createdAt");
    }

    #[test]
    fn test_to_kotlin_class_name() {
        let mapper = KotlinTypeMapper::new();

        assert_eq!(mapper.to_kotlin_class_name("user"), "User");
        assert_eq!(mapper.to_kotlin_class_name("order_item"), "OrderItem");
        assert_eq!(mapper.to_kotlin_class_name("first_name"), "FirstName");
    }
}
