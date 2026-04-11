//! Helper functions for type parsing, attribute parsing, and serialization

use crate::ast::model::{Attribute, AttributeValue, RelationType};
use crate::ast::types::{TypeRef, PrimitiveType};
use crate::ast::hook::ActionKind;
use crate::ast::expressions::Expression;
use super::types::YamlField;
use indexmap::IndexMap;

/// Parse a type string into a TypeRef
pub(crate) fn parse_type_ref(s: &str) -> TypeRef {
    let (type_ref, _) = parse_type_string(s);
    type_ref
}

/// Convert a serde_yaml::Value to an Expression
pub(crate) fn yaml_value_to_expr(value: serde_yaml::Value) -> Expression {
    match value {
        serde_yaml::Value::Null => Expression::Literal(crate::ast::expressions::Literal::Null),
        serde_yaml::Value::Bool(b) => Expression::Literal(crate::ast::expressions::Literal::Bool(b)),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Expression::Literal(crate::ast::expressions::Literal::Int(i))
            } else if let Some(f) = n.as_f64() {
                Expression::Literal(crate::ast::expressions::Literal::Float(f))
            } else {
                Expression::Raw(n.to_string())
            }
        }
        serde_yaml::Value::String(s) => {
            // Check if it's a template expression
            if s.contains("{{") || s.starts_with("$") {
                Expression::Raw(s)
            } else {
                Expression::Literal(crate::ast::expressions::Literal::String(s))
            }
        }
        _ => Expression::Raw(format!("{:?}", value)),
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Serialize type fields to a JSON string for JSONB validation schema
pub(crate) fn serialize_type_fields_to_json(fields: &IndexMap<String, YamlField>) -> String {
    let mut schema = serde_json::Map::new();

    for (field_name, yaml_field) in fields {
        let mut field_schema = serde_json::Map::new();

        // Get type information
        let (type_str, attributes) = match yaml_field {
            YamlField::Simple(s) => (s.clone(), vec![]),
            YamlField::Full { field_type, attributes, .. } => (field_type.clone(), attributes.clone()),
        };

        // Determine if optional
        let is_optional = type_str.ends_with('?');
        let base_type = type_str.trim_end_matches('?').trim_end_matches("[]");

        field_schema.insert("type".to_string(), serde_json::Value::String(base_type.to_string()));
        field_schema.insert("optional".to_string(), serde_json::Value::Bool(is_optional));

        // Add validation rules from attributes
        let mut rules = Vec::new();
        for attr in &attributes {
            if attr.starts_with('@') {
                rules.push(serde_json::Value::String(attr.clone()));
            }
        }
        if !rules.is_empty() {
            field_schema.insert("rules".to_string(), serde_json::Value::Array(rules));
        }

        schema.insert(field_name.clone(), serde_json::Value::Object(field_schema));
    }

    serde_json::to_string(&schema).unwrap_or_else(|_| "{}".to_string())
}

/// Create a TypeRef from a type name string, handling cross-module references (e.g., "sapiens.User")
pub(crate) fn make_type_ref_from_name(type_str: &str) -> TypeRef {
    if type_str.contains('.') {
        let parts: Vec<&str> = type_str.splitn(2, '.').collect();
        if parts.len() == 2 {
            TypeRef::ModuleRef {
                module: parts[0].to_string(),
                name: parts[1].to_string(),
            }
        } else {
            TypeRef::Custom(type_str.to_string())
        }
    } else {
        TypeRef::Custom(type_str.to_string())
    }
}

/// Parse a type string like "uuid", "string?", "email"
pub(crate) fn parse_type_string(s: &str) -> (TypeRef, Vec<Attribute>) {
    let s = s.trim();
    let mut attrs = Vec::new();

    // Check for optional marker
    let (type_str, optional) = if let Some(stripped) = s.strip_suffix('?') {
        (stripped, true)
    } else {
        (s, false)
    };

    // Check for array marker
    let (type_str, is_array) = if let Some(stripped) = type_str.strip_suffix("[]") {
        (stripped, true)
    } else {
        (type_str, false)
    };

    // Try to parse as primitive type first
    let type_ref = if let Some(prim) = PrimitiveType::from_str(type_str) {
        // Some primitives add validation attributes
        match prim {
            PrimitiveType::Email => attrs.push(Attribute::new("email")),
            PrimitiveType::Url => attrs.push(Attribute::new("url")),
            PrimitiveType::Phone => attrs.push(Attribute::new("phone")),
            PrimitiveType::Ip => attrs.push(Attribute::new("ip")),
            PrimitiveType::Uuid => attrs.push(Attribute::new("uuid")),
            _ => {}
        }
        TypeRef::Primitive(prim)
    } else {
        // Custom type or cross-module reference
        make_type_ref_from_name(type_str)
    };

    let type_ref = if is_array {
        TypeRef::Array(Box::new(type_ref))
    } else if optional {
        TypeRef::Optional(Box::new(type_ref))
    } else {
        type_ref
    };

    (type_ref, attrs)
}

/// Parse a relation type string like "Role[]", "Profile", "sapiens.User"
pub(crate) fn parse_relation_type(s: &str) -> (TypeRef, RelationType) {
    let s = s.trim();

    if let Some(inner) = s.strip_suffix("[]") {
        (TypeRef::Array(Box::new(make_type_ref_from_name(inner))), RelationType::Many)
    } else if let Some(inner) = s.strip_suffix('?') {
        (
            TypeRef::Optional(Box::new(make_type_ref_from_name(inner))),
            RelationType::One,
        )
    } else {
        (make_type_ref_from_name(s), RelationType::One)
    }
}

/// Parse an attribute string like "@unique", "@default(uuid)", "@min(3)"
pub(crate) fn parse_attribute_string(s: &str) -> Option<Attribute> {
    let s = s.trim();
    if !s.starts_with('@') {
        return None;
    }

    let s = &s[1..]; // Remove @

    if let Some(paren_pos) = s.find('(') {
        let name = &s[..paren_pos];
        let args_str = &s[paren_pos + 1..s.len() - 1]; // Remove ( and )
        let mut attr = Attribute::new(name);

        // Parse arguments - handle quoted strings with commas inside
        let args = split_attr_args(args_str);
        for arg in args {
            let arg = arg.trim();
            if arg.is_empty() {
                continue;
            }

            // Check for named argument (key:value)
            // Only treat : as separator if it's outside of quoted strings
            let colon_pos = find_colon_outside_quotes(&arg);
            if let Some(eq_pos) = colon_pos {
                let key = arg[..eq_pos].trim();
                let val = arg[eq_pos + 1..].trim();
                attr.args.push((Some(key.to_string()), parse_attr_value(val)));
            } else {
                attr.args.push((None, parse_attr_value(arg)));
            }
        }

        Some(attr)
    } else {
        Some(Attribute::new(s))
    }
}

/// Split attribute arguments, respecting quoted strings
/// Example: `'{"regular":0,"express":0}', true" -> ["'{"regular":0,"express":0}'", "true"]
pub(crate) fn split_attr_args(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let chars: Vec<char> = s.chars().collect();

    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
                current.push(ch);
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
                current.push(ch);
            }
            ',' if !in_single_quote && !in_double_quote => {
                result.push(current.trim().to_string());
                current = String::new();
            }
            _ => {
                current.push(ch);
            }
        }
        i += 1;
    }

    if !current.is_empty() {
        result.push(current.trim().to_string());
    }

    result
}

/// Parse an attribute value
pub(crate) fn parse_attr_value(s: &str) -> AttributeValue {
    let s = s.trim();

    // String literal
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        // Use char-based slicing to handle multi-byte characters correctly
        let chars: Vec<char> = s.chars().collect();
        if chars.len() >= 2 {
            let inner: String = chars[1..chars.len() - 1].iter().collect();
            return AttributeValue::String(inner);
        } else {
            return AttributeValue::String(String::new());
        }
    }

    // Boolean
    if s == "true" {
        return AttributeValue::Bool(true);
    }
    if s == "false" {
        return AttributeValue::Bool(false);
    }

    // Integer
    if let Ok(i) = s.parse::<i64>() {
        return AttributeValue::Int(i);
    }

    // Float
    if let Ok(f) = s.parse::<f64>() {
        return AttributeValue::Float(f);
    }

    // Identifier
    AttributeValue::Ident(s.to_string())
}

/// Find a colon (:) that's outside of quoted strings
/// Returns the position of the colon, or None if all colons are inside quotes
pub(crate) fn find_colon_outside_quotes(s: &str) -> Option<usize> {
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let chars: Vec<char> = s.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
        match ch {
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            ':' if !in_single_quote && !in_double_quote => {
                return Some(i);
            }
            _ => {}
        }
    }
    None
}

/// Parse an action string like "send_email(welcome, email)"
pub(crate) fn parse_action_string(s: &str) -> (ActionKind, Vec<Expression>) {
    let s = s.trim();

    if let Some(paren_pos) = s.find('(') {
        let name = &s[..paren_pos];
        let args_str = &s[paren_pos + 1..s.len() - 1];
        let args: Vec<Expression> = args_str
            .split(',')
            .map(|a| Expression::Raw(a.trim().to_string()))
            .collect();
        (ActionKind::from_str(name), args)
    } else {
        (ActionKind::from_str(s), vec![])
    }
}
