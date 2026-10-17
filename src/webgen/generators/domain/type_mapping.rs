//! Type mapping utilities for Schema → TypeScript/Zod conversion
//!
//! This module provides mappings from Backbone schema types to:
//! - TypeScript types
//! - Zod validation schemas
//! - Default values

use crate::rules::fields::BrowserCheck;
use crate::webgen::ast::entity::{EnumDefinition, FieldDefinition, FieldType};

/// The service's wording for a value that is not a UUID (its `invalid_uuid` check).
pub const UUID_MESSAGE: &str = "This field must be a valid UUID";
/// The wording for a value that is not an ISO date and time.
pub const DATETIME_MESSAGE: &str = "This field must be a date and time";
/// The wording for a value that is not an ISO calendar date.
pub const DATE_MESSAGE: &str = "This field must be a date";
/// The largest integer a JSON number holds exactly (`Number.MAX_SAFE_INTEGER`).
const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// `s` as a double-quoted JavaScript string literal.
fn js_str(s: &str) -> String {
    serde_json::Value::from(s).to_string()
}

/// One `.superRefine` running `checks` over a value that is there, each refusal carrying its
/// code and values in `params` and its message. A null never reaches it: the refinement sits
/// before `.nullable()`, as the service passes a null to every check but `required`.
pub fn super_refine(checks: &[BrowserCheck]) -> String {
    let mut body = String::new();
    for c in checks {
        let mut params = serde_json::Map::new();
        params.insert("code".into(), serde_json::Value::from(c.code.as_str()));
        for (k, v) in &c.params {
            params.insert(k.clone(), v.clone());
        }
        body.push_str(&format!(
            "    if (!({})) ctx.addIssue({{ code: z.ZodIssueCode.custom, message: {}, params: {} }});\n",
            c.test,
            js_str(&c.message),
            serde_json::Value::Object(params)
        ));
    }
    if body.is_empty() {
        String::new()
    } else {
        format!(".superRefine((v, ctx) => {{\n{body}  }})")
    }
}

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

    /// Well-known framework types that are not generated as standalone files.
    /// They map to self-contained, import-free TypeScript/Zod so generated code
    /// compiles without a shared-types module.
    fn is_freeform_custom(name: &str) -> bool {
        matches!(name, "Metadata" | "Json" | "JsonValue")
    }

    /// Well-known scalar aliases (often from proto schemas) that map to numbers.
    pub fn is_numeric_scalar(name: &str) -> bool {
        matches!(
            name.to_lowercase().as_str(),
            "int"
                | "int32"
                | "int64"
                | "uint"
                | "uint32"
                | "uint64"
                | "sint32"
                | "sint64"
                | "fixed32"
                | "fixed64"
                | "float"
                | "float32"
                | "float64"
                | "double"
                | "decimal"
                | "bigint"
                | "long"
        )
    }

    /// Integer scalars as wide as an i64, which the service writes as a JSON number.
    pub fn is_int64_scalar(name: &str) -> bool {
        matches!(name.to_lowercase().as_str(), "int64" | "uint64" | "sint64" | "fixed64" | "long" | "bigint")
    }

    /// A decimal's literal default as the string a decimal field holds (`0.5` → `'0.5'`).
    pub fn decimal_default(value: &str) -> String {
        format!("'{}'", value.trim().trim_matches(|c| c == '\'' || c == '"'))
    }

    /// Schema `@default(...)` values that denote a runtime function (e.g. a
    /// generated UUID or timestamp) rather than a literal. These must NOT be
    /// emitted as Zod `.default('uuid')` or as bare identifiers in factories.
    pub fn is_function_default(value: &str) -> bool {
        matches!(
            value.trim().to_lowercase().as_str(),
            "uuid"
                | "uuidv4"
                | "uuid_v4"
                | "gen_random_uuid"
                | "cuid"
                | "nanoid"
                | "now"
                | "now()"
                | "current_timestamp"
                | "autoincrement"
                | "auto"
                | "auto_increment"
                | "identity"
        )
    }

    /// Convert a schema field type to TypeScript type
    pub fn to_typescript_type(&self, field_type: &FieldType, optional: bool) -> String {
        let base_type = self.base_typescript_type(field_type);
        // Avoid a doubled `| null` when the base type is already nullable
        // (e.g. an `Optional(_)` field that is also marked optional).
        if optional && !base_type.contains("| null") {
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
            FieldType::Float => "number".to_string(),
            // rust_decimal writes a decimal as a string, and the OpenAPI document says so.
            FieldType::Decimal => "string".to_string(),
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
            FieldType::Custom(name) if Self::is_freeform_custom(name) => {
                "Record<string, unknown>".to_string()
            }
            FieldType::Custom(name) if Self::is_numeric_scalar(name) => "number".to_string(),
            FieldType::Custom(name) => name.clone(),
            FieldType::Array(inner) => format!("{}[]", self.base_typescript_type(inner)),
            FieldType::Optional(inner) => format!("{} | null", self.base_typescript_type(inner)),
        }
    }

    /// A field's Zod schema, with a code on every refusal it can make.
    ///
    /// - `checks` are the service's own checks for the field (the write guard's, read from the
    ///   rule register). They run in one `.superRefine`, each refusal carrying the guard's code
    ///   and values in `params` and the guard's message.
    /// - The type's own shape checks that a refinement can carry (a whole number, a time of
    ///   day, an IP address) join that `.superRefine`, coded the same way.
    /// - The shape checks screens read off the schema stay Zod's own, with the service's
    ///   wording: `.uuid()`, `.date()`, `.datetime()`, `z.enum` and a text field's `.max` (the
    ///   screens choose a line or a text area by it). Zod 3 takes no `params` on these, so their
    ///   code is read off the issue itself by `shared/validation`.
    ///
    /// The field's attributes add no checks of their own: the browser refuses what the service
    /// refuses and nothing more. A field the service fills, or that no write supplies, has no
    /// guard check and so carries only its type; a check the browser enforced there would block
    /// a form over a value the person cannot change.
    pub fn to_zod_schema(&self, field: &FieldDefinition, enums: &[EnumDefinition], checks: &[BrowserCheck]) -> String {
        let (mut schema, mut refusals) = Self::base_zod(&field.type_name, enums, &field.name);
        if Self::is_text(&field.type_name) {
            schema.push_str(&Self::transforms(field));
            if let Some((max, message)) = Self::text_max(field, checks) {
                schema.push_str(&format!(".max({max}, {{ message: {} }})", js_str(&message)));
            }
        }
        // A required check over a nullable non-text value is the type's own: the base schema
        // already refuses a missing value.
        refusals.extend(checks.iter().filter(|c| c.code != "max_length" && c.test != "v != null").cloned());
        schema.push_str(&super_refine(&refusals));
        self.finish(field, schema)
    }

    /// Optionality and the default, after the checks.
    fn finish(&self, field: &FieldDefinition, mut schema: String) -> String {
        // Handle optionality
        if field.optional {
            schema = format!("{}.nullable().optional()", schema);
        }

        // Handle default value. Skip runtime-function defaults (uuid/now/…) —
        // they are server-assigned, not literal Zod defaults.
        if let Some(default) = &field.default_value {
            if !Self::is_function_default(default) {
                schema = format!(
                    "{}.default({})",
                    schema,
                    Self::format_default_value(default, &field.type_name)
                );
            }
        }

        schema
    }

    /// Whether a field holds text the way the service's checks count it: the types its
    /// length checks apply to, and whose Zod schema is a plain `z.string()` underneath.
    fn is_text(field_type: &FieldType) -> bool {
        match field_type {
            FieldType::Optional(inner) => Self::is_text(inner),
            FieldType::String
            | FieldType::Text
            | FieldType::Email
            | FieldType::Phone
            | FieldType::Url
            | FieldType::Uuid
            | FieldType::Ip => true,
            _ => false,
        }
    }

    /// The value rewrites a field declares (`@trim`, `@lowercase`, `@uppercase`). They change
    /// what is sent and never refuse, so they carry no code.
    fn transforms(field: &FieldDefinition) -> String {
        field
            .attributes
            .iter()
            .filter_map(|a| match a.name.as_str() {
                "trim" => Some(".trim()"),
                "lowercase" => Some(".toLowerCase()"),
                "uppercase" => Some(".toUpperCase()"),
                _ => None,
            })
            .collect()
    }

    /// A text field's maximum length and its message. The service's own check gives both. A
    /// field it does not check (filled by the service, or written by no form) still declares
    /// its column's size, which no stored value exceeds, so the maximum is kept for the screens
    /// that read it; a field whose checks are kept in shadow (`@shadow`) holds longer values,
    /// so it gets none.
    fn text_max(field: &FieldDefinition, checks: &[BrowserCheck]) -> Option<(String, String)> {
        if let Some(c) = checks.iter().find(|c| c.code == "max_length") {
            let max = c.params.iter().find(|(k, _)| k == "max")?.1.to_string();
            return Some((max, c.message.clone()));
        }
        if field.attributes.iter().any(|a| a.name == "shadow") {
            return None;
        }
        let declared = field.attributes.iter().find_map(|a| match a.name.as_str() {
            "max" => a.first_arg().and_then(|v| v.trim().parse::<u64>().ok()),
            "length" => a.args.iter().find_map(|v| v.strip_prefix("max=").and_then(|n| n.trim().parse::<u64>().ok())),
            _ => None,
        })?;
        Some((declared.to_string(), format!("This field must be at most {declared} characters")))
    }

    /// A type's Zod schema and the shape checks it needs beyond Zod's own (run in the field's
    /// `.superRefine`, coded). The Zod checks kept here carry the service's wording.
    fn base_zod(field_type: &FieldType, enums: &[EnumDefinition], field: &str) -> (String, Vec<BrowserCheck>) {
        let check = |code: &str, message: &str, test: &str| BrowserCheck {
            field: field.to_string(),
            code: code.to_string(),
            message: message.to_string(),
            params: Vec::new(),
            test: test.to_string(),
        };
        let plain = |s: &str| (s.to_string(), Vec::new());
        match field_type {
            FieldType::String | FieldType::Text | FieldType::Email | FieldType::Phone | FieldType::Url => plain("z.string()"),
            FieldType::Int => ("z.number()".to_string(), vec![check("integer", "This field must be a whole number", "Number.isInteger(v)")]),
            FieldType::Float => plain("z.number()"),
            // A string or a number in, the decimal string out (`shared/types/decimal`).
            FieldType::Decimal => plain("decimalSchema"),
            FieldType::Bool => plain("z.boolean()"),
            FieldType::Uuid => (format!("z.string().uuid({{ message: {} }})", js_str(UUID_MESSAGE)), Vec::new()),
            FieldType::DateTime => (format!("z.string().datetime({{ message: {} }})", js_str(DATETIME_MESSAGE)), Vec::new()),
            FieldType::Date => (format!("z.string().date({})", js_str(DATE_MESSAGE)), Vec::new()),
            FieldType::Time => (
                "z.string()".to_string(),
                vec![check("invalid_time", "This field must be a time of day", r#"/^([01]\d|2[0-3]):[0-5]\d:[0-5]\d(\.\d+)?$/.test(v)"#)],
            ),
            FieldType::Json => plain("z.record(z.unknown())"),
            FieldType::Ip => (
                "z.string()".to_string(),
                vec![check(
                    "invalid_ip",
                    "This field must be an IP address",
                    r#"/^(\d{1,3}\.){3}\d{1,3}$/.test(v) || /^([0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}$/.test(v)"#,
                )],
            ),
            FieldType::Enum(name) | FieldType::Custom(name) => {
                if let Some(enum_def) = enums.iter().find(|e| &e.name == name) {
                    let variants: Vec<String> = enum_def.variants.iter().map(|v| format!("'{}'", v.name)).collect();
                    let names: Vec<&str> = enum_def.variants.iter().map(|v| v.name.as_str()).collect();
                    let message = format!("This field must be one of {}", names.join(", "));
                    (format!("z.enum([{}], {{ message: {} }})", variants.join(", "), js_str(&message)), Vec::new())
                } else if matches!(field_type, FieldType::Enum(_)) {
                    (format!("{}Schema", name), Vec::new())
                } else if Self::is_freeform_custom(name) {
                    // Free-form framework types (e.g. Metadata) → permissive record
                    plain("z.record(z.unknown())")
                } else if Self::is_int64_scalar(name) {
                    // serde writes an i64 as a JSON number, which is exact only up to 2^53.
                    let mut range = check("range", format!("This field must be between -{MAX_SAFE_INTEGER} and {MAX_SAFE_INTEGER}").as_str(), "!Number.isInteger(v) || Number.isSafeInteger(v)");
                    range.params = vec![("min".to_string(), serde_json::Value::from(-MAX_SAFE_INTEGER)), ("max".to_string(), serde_json::Value::from(MAX_SAFE_INTEGER))];
                    ("z.number()".to_string(), vec![check("integer", "This field must be a whole number", "Number.isInteger(v)"), range])
                } else if Self::is_numeric_scalar(name) {
                    plain("z.number()")
                } else {
                    // Assume it's another entity reference
                    (format!("{}Schema", name), Vec::new())
                }
            }
            FieldType::Array(inner) => {
                // An element's own checks run on each element, inside the array.
                let (element, checks) = Self::base_zod(inner, enums, field);
                (format!("z.array({}{})", element, super_refine(&checks)), Vec::new())
            }
            FieldType::Optional(inner) => Self::base_zod(inner, enums, field),
        }
    }

    /// Format a default value for Zod
    fn format_default_value(value: &str, field_type: &FieldType) -> String {
        match field_type {
            FieldType::String
            | FieldType::Text
            | FieldType::Email
            | FieldType::Phone
            | FieldType::Url
            | FieldType::Uuid
            | FieldType::Date
            | FieldType::Time
            | FieldType::DateTime
            | FieldType::Ip => {
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
            FieldType::Int | FieldType::Float => value.to_string(),
            FieldType::Decimal => Self::decimal_default(value),
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
            FieldType::Custom(name) if Self::is_numeric_scalar(name) => {
                if value.parse::<f64>().is_ok() {
                    value.to_string()
                } else {
                    "0".to_string()
                }
            }
            FieldType::Enum(_) | FieldType::Custom(_) => {
                if value.starts_with('"') || value.starts_with('\'') {
                    value.to_string()
                } else {
                    format!("'{}'", value)
                }
            }
            FieldType::Optional(inner) => Self::format_default_value(value, inner),
        }
    }

    /// Get default value for a field type
    pub fn default_value_for_type(&self, field_type: &FieldType) -> String {
        match field_type {
            FieldType::String
            | FieldType::Text
            | FieldType::Email
            | FieldType::Phone
            | FieldType::Url
            | FieldType::Ip => "''".to_string(),
            FieldType::Uuid => "crypto.randomUUID()".to_string(),
            FieldType::Int => "0".to_string(),
            FieldType::Float => "0.0".to_string(),
            FieldType::Decimal => "'0'".to_string(),
            FieldType::Bool => "false".to_string(),
            // Zod infers date/datetime fields as ISO `string`, so factory
            // defaults must be strings (not `Date`).
            FieldType::DateTime => "new Date().toISOString()".to_string(),
            FieldType::Date => "new Date().toISOString().split('T')[0]".to_string(),
            FieldType::Time => "new Date().toISOString().split('T')[1].split('.')[0]".to_string(),
            FieldType::Json => "{}".to_string(),
            FieldType::Array(_) => "[]".to_string(),
            FieldType::Custom(name) if Self::is_freeform_custom(name) => "{}".to_string(),
            FieldType::Custom(name) if Self::is_numeric_scalar(name) => "0".to_string(),
            FieldType::Enum(name) | FieldType::Custom(name) => {
                // Fallback when the enum's variants are unknown to this mapper.
                // Callers with enum context emit a concrete first variant instead.
                format!("'' as unknown as {}", name)
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
        let is_vo_name = name.ends_with("_address")
            || name.ends_with("_email")
            || name.ends_with("_phone")
            || name.ends_with("_money")
            || name.ends_with("_amount")
            || name.ends_with("_price")
            || name == "email"
            || name == "phone"
            || name == "address";

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
        if name.contains("money")
            || name.contains("amount")
            || name.contains("price")
            || name.contains("cost")
        {
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
pub(crate) mod tests {
    use super::*;
    use crate::webgen::ast::entity::FieldAttribute;

    #[test]
    fn test_typescript_type_mapping() {
        let mapper = TypeMapper::new();

        assert_eq!(
            mapper.to_typescript_type(&FieldType::String, false),
            "string"
        );
        assert_eq!(
            mapper.to_typescript_type(&FieldType::String, true),
            "string | null"
        );
        assert_eq!(mapper.to_typescript_type(&FieldType::Int, false), "number");
        assert_eq!(
            mapper.to_typescript_type(&FieldType::Bool, false),
            "boolean"
        );
        assert_eq!(
            mapper.to_typescript_type(&FieldType::DateTime, false),
            "Date"
        );
        assert_eq!(mapper.to_typescript_type(&FieldType::Uuid, false), "string");
    }

    fn field(name: &str, type_name: FieldType, attrs: &[(&str, &[&str])], optional: bool) -> FieldDefinition {
        FieldDefinition {
            name: name.to_string(),
            type_name,
            attributes: attrs
                .iter()
                .map(|(n, args)| FieldAttribute::new(*n, args.iter().map(|a| a.to_string()).collect()))
                .collect(),
            description: None,
            optional,
            default_value: None,
        }
    }

    /// Every message a schema carries is either in a coded refinement or on one of the Zod
    /// checks `shared/validation` reads a code off.
    pub(crate) fn assert_every_message_coded(schema: &str) {
        let native = regex::Regex::new(r"(\.uuid\(\{ |\.datetime\(\{ |\.max\(\d+, \{ |\], \{ )$").unwrap();
        for (idx, _) in schema.match_indices("message:") {
            let after = &schema[idx..];
            let end = after.find("});").or_else(|| after.find("})")).unwrap_or(after.len());
            let coded = after[..end].contains("params: {\"code\":");
            assert!(coded || native.is_match(&schema[..idx]), "uncoded message at {idx} in: {schema}");
        }
    }

    #[test]
    fn a_field_the_service_does_not_check_gets_no_attribute_check() {
        let mapper = TypeMapper::new();
        // A service-filled total: `@non_negative`, but no guard check, so no browser check.
        let total = field("total_debits", FieldType::Decimal, &[("non_negative", &[]), ("positive", &[])], false);
        assert_eq!(mapper.to_zod_schema(&total, &[], &[]), "decimalSchema");
        let email = field("email", FieldType::Email, &[], false);
        assert_eq!(mapper.to_zod_schema(&email, &[], &[]), "z.string()");
    }

    #[test]
    fn an_optional_text_field_counts_its_maximum_in_characters() {
        let mapper = TypeMapper::new();
        let note = field("note", FieldType::Optional(Box::new(FieldType::String)), &[("max", &["120"])], true);
        assert_eq!(
            mapper.to_zod_schema(&note, &[], &[]),
            "z.string().max(120, { message: \"This field must be at most 120 characters\" }).nullable().optional()"
        );
        let label = field("label", FieldType::String, &[("length", &["max=255"])], false);
        assert_eq!(
            mapper.to_zod_schema(&label, &[], &[]),
            "z.string().max(255, { message: \"This field must be at most 255 characters\" })"
        );
        // A field whose checks are shadowed holds longer values: no maximum.
        let shadowed = field("memo", FieldType::String, &[("max", &["10"]), ("shadow", &["widen later"])], false);
        assert_eq!(mapper.to_zod_schema(&shadowed, &[], &[]), "z.string()");
        // A number's maximum is not a length, and with no guard check there is none.
        let qty = field("qty", FieldType::Optional(Box::new(FieldType::Int)), &[("max", &["5"])], true);
        assert!(!mapper.to_zod_schema(&qty, &[], &[]).contains(".max("));
    }

    #[test]
    fn every_type_check_carries_a_code() {
        let mapper = TypeMapper::new();
        let variant = |n: &str| crate::webgen::ast::entity::EnumVariant { name: n.to_string(), description: None, is_default: false };
        let enums = vec![EnumDefinition { name: "Stage".to_string(), variants: vec![variant("draft"), variant("posted")] }];
        let cases = [
            (field("qty", FieldType::Int, &[], false), "params: {\"code\":\"integer\"}"),
            (field("at", FieldType::Time, &[], false), "params: {\"code\":\"invalid_time\"}"),
            (field("ip", FieldType::Ip, &[], false), "params: {\"code\":\"invalid_ip\"}"),
            (field("ref_id", FieldType::Uuid, &[], false), ".uuid({ message: \"This field must be a valid UUID\" })"),
            (field("on", FieldType::Date, &[], false), ".date(\"This field must be a date\")"),
            (field("ts", FieldType::DateTime, &[], false), ".datetime({ message: \"This field must be a date and time\" })"),
            (
                field("stage", FieldType::Enum("Stage".into()), &[], false),
                "z.enum(['draft', 'posted'], { message: \"This field must be one of draft, posted\" })",
            ),
            (field("ids", FieldType::Array(Box::new(FieldType::Int)), &[], false), "z.array(z.number().superRefine("),
        ];
        for (f, expected) in cases {
            let schema = mapper.to_zod_schema(&f, &enums, &[]);
            assert!(schema.contains(expected), "{} → {schema}", f.name);
            assert!(!schema.contains(".uuid()") && !schema.contains(".int(") && !schema.contains(".time("), "{schema}");
            assert_every_message_coded(&schema);
        }
    }

    #[test]
    fn the_guard_checks_run_coded_and_the_text_maximum_stays_zods() {
        let mapper = TypeMapper::new();
        let name = field("name", FieldType::String, &[("required", &[]), ("max", &["80"])], false);
        let checks = vec![
            BrowserCheck {
                field: "name".into(),
                code: "required".into(),
                message: "This field is required".into(),
                params: vec![],
                test: "v != null && String(v).trim() !== \"\"".into(),
            },
            BrowserCheck {
                field: "name".into(),
                code: "max_length".into(),
                message: "This field must be at most 80 characters".into(),
                params: vec![("max".into(), serde_json::Value::from(80))],
                test: "[...String(v)].length <= 80".into(),
            },
        ];
        let schema = mapper.to_zod_schema(&name, &[], &checks);
        assert!(
            schema.starts_with("z.string().max(80, { message: \"This field must be at most 80 characters\" }).superRefine("),
            "{schema}"
        );
        assert!(schema.contains("params: {\"code\":\"required\"}"), "{schema}");
        assert!(!schema.contains("\"max_length\""), "the maximum is Zod's own, not repeated in the refinement: {schema}");
        assert_every_message_coded(&schema);
    }

    /// A decimal travels as a string (rust_decimal writes one, and the OpenAPI document says so):
    /// its TypeScript type is `string`, its schema the shared `decimalSchema`, which takes a
    /// string or a number and hands on a decimal string, and its defaults are strings.
    #[test]
    fn a_decimal_is_typed_as_the_string_the_service_sends() {
        let mapper = TypeMapper::new();
        assert_eq!(mapper.to_typescript_type(&FieldType::Decimal, false), "string");
        assert_eq!(mapper.default_value_for_type(&FieldType::Decimal), "'0'");
        let mut rate = field("rate", FieldType::Decimal, &[], false);
        rate.default_value = Some("0.5".into());
        assert_eq!(mapper.to_zod_schema(&rate, &[], &[]), "decimalSchema.default('0.5')");
        let check = BrowserCheck {
            field: "rate".into(),
            code: "non_negative".into(),
            message: "This field must not be negative".into(),
            params: vec![],
            test: "Number(v) >= 0".into(),
        };
        let schema = mapper.to_zod_schema(&field("rate", FieldType::Decimal, &[], false), &[], &[check]);
        assert!(schema.starts_with("decimalSchema.superRefine("), "{schema}");
        assert_every_message_coded(&schema);
        let (path, shared) = crate::webgen::generators::shared_runtime::shared_files().into_iter().find(|(p, _)| p.ends_with("decimal.ts")).expect("a shared decimal schema");
        assert_eq!(path, "shared/types/decimal.ts");
        assert!(shared.contains("export const decimalSchema") && shared.contains("params: { code: 'invalid_decimal' }"), "{shared}");
    }

    /// An int64 arrives as a JSON number (serde writes i64 as one), so it stays a `number`; a
    /// value past 2^53 cannot be one exactly, so it is refused with the service's `range` code.
    #[test]
    fn an_int64_is_a_whole_number_within_the_safe_range() {
        let mapper = TypeMapper::new();
        let ty = FieldType::Custom("int64".into());
        assert_eq!(mapper.to_typescript_type(&ty, false), "number");
        let schema = mapper.to_zod_schema(&field("size_bytes", ty, &[], false), &[], &[]);
        assert!(schema.starts_with("z.number().superRefine("), "{schema}");
        assert!(schema.contains("params: {\"code\":\"integer\"}"), "{schema}");
        assert!(schema.contains("Number.isSafeInteger(v)") && schema.contains("{\"code\":\"range\",\"max\":9007199254740991,\"min\":-9007199254740991}"), "{schema}");
        assert_every_message_coded(&schema);
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
        assert_eq!(
            mapper.detect_value_object_type(&email_field),
            Some(ValueObjectType::Email)
        );
    }
}
