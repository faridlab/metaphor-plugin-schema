//! Type system definitions
//!
//! Defines all primitive and complex types supported by the schema system.

use serde::{Deserialize, Serialize};

/// A type reference in the schema
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeRef {
    /// Primitive type (string, int, bool, etc.)
    Primitive(PrimitiveType),
    /// Custom type reference (user-defined type or enum)
    Custom(String),
    /// Array type (e.g., String[])
    Array(Box<TypeRef>),
    /// Map type (e.g., Map<String, Int>)
    Map {
        key: Box<TypeRef>,
        value: Box<TypeRef>,
    },
    /// Optional/nullable type (e.g., String?)
    Optional(Box<TypeRef>),
    /// Cross-module reference (e.g., sapiens.User)
    ModuleRef { module: String, name: String },
}

impl Default for TypeRef {
    fn default() -> Self {
        Self::Primitive(PrimitiveType::String)
    }
}

impl TypeRef {
    pub fn primitive(p: PrimitiveType) -> Self {
        Self::Primitive(p)
    }

    pub fn custom(name: impl Into<String>) -> Self {
        Self::Custom(name.into())
    }

    pub fn array(inner: TypeRef) -> Self {
        Self::Array(Box::new(inner))
    }

    pub fn optional(inner: TypeRef) -> Self {
        Self::Optional(Box::new(inner))
    }

    pub fn module_ref(module: impl Into<String>, name: impl Into<String>) -> Self {
        Self::ModuleRef {
            module: module.into(),
            name: name.into(),
        }
    }

    /// Check if this type is optional
    pub fn is_optional(&self) -> bool {
        matches!(self, Self::Optional(_))
    }

    /// Check if this type is an array
    pub fn is_array(&self) -> bool {
        matches!(self, Self::Array(_))
    }

    /// Get the inner type (for Optional and Array)
    pub fn inner_type(&self) -> Option<&TypeRef> {
        match self {
            Self::Optional(inner) | Self::Array(inner) => Some(inner),
            _ => None,
        }
    }

    /// Get the base name of this type (for display)
    pub fn base_name(&self) -> String {
        match self {
            Self::Primitive(p) => p.as_str().to_string(),
            Self::Custom(name) => name.clone(),
            Self::Array(inner) => format!("{}[]", inner.base_name()),
            Self::Map { key, value } => format!("Map<{}, {}>", key.base_name(), value.base_name()),
            Self::Optional(inner) => format!("{}?", inner.base_name()),
            Self::ModuleRef { module, name } => format!("{}.{}", module, name),
        }
    }
}

/// Primitive types supported by the schema
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrimitiveType {
    // Basic types
    String,
    Int,
    Int32,
    Int64,
    Float,
    Float32,
    Float64,
    Bool,
    Bytes,

    // Special string types with built-in validation
    Uuid,
    Email,
    Url,
    Phone,
    Slug,
    Ip,
    IpV4,
    IpV6,
    Mac,
    Json,
    Markdown,
    Html,

    // Date/Time types
    DateTime,
    Date,
    Time,
    Duration,
    Timestamp,

    // Numeric types
    Decimal,
    Money,
    Percentage,

    // Binary types
    Binary,
    Base64,
}

impl PrimitiveType {
    /// Get the string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Int => "int",
            Self::Int32 => "int32",
            Self::Int64 => "int64",
            Self::Float => "float",
            Self::Float32 => "float32",
            Self::Float64 => "float64",
            Self::Bool => "bool",
            Self::Bytes => "bytes",
            Self::Uuid => "uuid",
            Self::Email => "email",
            Self::Url => "url",
            Self::Phone => "phone",
            Self::Slug => "slug",
            Self::Ip => "ip",
            Self::IpV4 => "ipv4",
            Self::IpV6 => "ipv6",
            Self::Mac => "mac",
            Self::Json => "json",
            Self::Markdown => "markdown",
            Self::Html => "html",
            Self::DateTime => "datetime",
            Self::Date => "date",
            Self::Time => "time",
            Self::Duration => "duration",
            Self::Timestamp => "timestamp",
            Self::Decimal => "decimal",
            Self::Money => "money",
            Self::Percentage => "percentage",
            Self::Binary => "binary",
            Self::Base64 => "base64",
        }
    }

    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "string" | "str" => Some(Self::String),
            "int" | "integer" => Some(Self::Int),
            "int32" | "i32" => Some(Self::Int32),
            "int64" | "i64" | "long" => Some(Self::Int64),
            "float" | "f64" => Some(Self::Float),
            "float32" | "f32" => Some(Self::Float32),
            "float64" => Some(Self::Float64),
            "bool" | "boolean" => Some(Self::Bool),
            "bytes" => Some(Self::Bytes),
            "uuid" | "guid" => Some(Self::Uuid),
            "email" => Some(Self::Email),
            "url" | "uri" => Some(Self::Url),
            "phone" => Some(Self::Phone),
            "slug" => Some(Self::Slug),
            "ip" => Some(Self::Ip),
            "ipv4" => Some(Self::IpV4),
            "ipv6" => Some(Self::IpV6),
            "mac" => Some(Self::Mac),
            "json" => Some(Self::Json),
            "markdown" | "md" => Some(Self::Markdown),
            "html" => Some(Self::Html),
            "datetime" => Some(Self::DateTime),
            "date" => Some(Self::Date),
            "time" => Some(Self::Time),
            "duration" => Some(Self::Duration),
            "timestamp" => Some(Self::Timestamp),
            "decimal" => Some(Self::Decimal),
            "money" | "currency" => Some(Self::Money),
            "percentage" | "percent" => Some(Self::Percentage),
            "binary" | "blob" => Some(Self::Binary),
            "base64" => Some(Self::Base64),
            _ => None,
        }
    }

    /// Get the Rust type for this primitive
    pub fn rust_type(&self) -> &'static str {
        match self {
            Self::String | Self::Email | Self::Url | Self::Phone | Self::Slug | Self::Ip
            | Self::IpV4 | Self::IpV6 | Self::Mac | Self::Markdown | Self::Html => "String",
            Self::Int | Self::Int32 => "i32",
            Self::Int64 => "i64",
            Self::Float | Self::Float64 => "f64",
            Self::Float32 => "f32",
            Self::Bool => "bool",
            Self::Bytes | Self::Binary | Self::Base64 => "Vec<u8>",
            Self::Uuid => "Uuid",
            Self::Json => "serde_json::Value",
            Self::DateTime | Self::Timestamp => "DateTime<Utc>",
            Self::Date => "NaiveDate",
            Self::Time => "NaiveTime",
            Self::Duration => "Duration",
            Self::Decimal | Self::Money | Self::Percentage => "Decimal",
        }
    }

    /// Get the PostgreSQL type for this primitive
    pub fn postgres_type(&self) -> &'static str {
        match self {
            Self::String | Self::Email | Self::Url | Self::Phone | Self::Slug | Self::Ip
            | Self::IpV4 | Self::IpV6 | Self::Mac | Self::Markdown | Self::Html => "TEXT",
            Self::Int | Self::Int32 => "INTEGER",
            Self::Int64 => "BIGINT",
            Self::Float | Self::Float64 => "DOUBLE PRECISION",
            Self::Float32 => "REAL",
            Self::Bool => "BOOLEAN",
            Self::Bytes | Self::Binary | Self::Base64 => "BYTEA",
            Self::Uuid => "UUID",
            Self::Json => "JSONB",
            Self::DateTime | Self::Timestamp => "TIMESTAMPTZ",
            Self::Date => "DATE",
            Self::Time => "TIME",
            Self::Duration => "INTERVAL",
            Self::Decimal | Self::Money | Self::Percentage => "NUMERIC",
        }
    }

    /// Get the Proto type for this primitive
    pub fn proto_type(&self) -> &'static str {
        match self {
            Self::String | Self::Email | Self::Url | Self::Phone | Self::Slug | Self::Ip
            | Self::IpV4 | Self::IpV6 | Self::Mac | Self::Markdown | Self::Html | Self::Uuid => {
                "string"
            }
            Self::Int | Self::Int32 => "int32",
            Self::Int64 => "int64",
            Self::Float | Self::Float32 => "float",
            Self::Float64 => "double",
            Self::Bool => "bool",
            Self::Bytes | Self::Binary | Self::Base64 => "bytes",
            Self::Json => "google.protobuf.Struct",
            Self::DateTime | Self::Timestamp => "google.protobuf.Timestamp",
            Self::Date | Self::Time | Self::Duration => "string",
            Self::Decimal | Self::Money | Self::Percentage => "string",
        }
    }

    /// Check if this type requires special validation
    pub fn has_builtin_validation(&self) -> bool {
        matches!(
            self,
            Self::Email
                | Self::Url
                | Self::Phone
                | Self::Uuid
                | Self::Ip
                | Self::IpV4
                | Self::IpV6
                | Self::Mac
                | Self::Slug
        )
    }
}
