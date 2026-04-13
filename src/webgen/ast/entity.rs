//! Entity AST for model.yaml schema definitions

use serde::{Deserialize, Serialize};

/// Model schema containing entity definitions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSchema {
    pub models: Vec<EntityDefinition>,
    pub enums: Vec<EnumDefinition>,
}

/// Entity definition from model.yaml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityDefinition {
    pub name: String,
    pub collection: String,
    pub fields: Vec<FieldDefinition>,
    pub relations: Vec<RelationDefinition>,
    pub indexes: Vec<IndexDefinition>,
    /// Whether this entity supports soft delete (uses metadata.deleted_at)
    #[serde(default)]
    pub soft_delete: bool,
}

impl EntityDefinition {
    /// Check if this entity has soft delete enabled
    pub fn has_soft_delete(&self) -> bool {
        self.soft_delete
    }
}

/// Field definition within an entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDefinition {
    pub name: String,
    pub type_name: FieldType,
    pub attributes: Vec<FieldAttribute>,
    pub description: Option<String>,
    pub optional: bool,
    pub default_value: Option<String>,
}

/// Field type enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FieldType {
    String,
    Int,
    Float,
    Bool,
    Uuid,
    DateTime,
    Date,
    Time,
    Email,
    Phone,
    Url,
    Json,
    Text,
    Decimal,
    Ip,

    // Enum reference
    Enum(String),

    // Custom/unknown type
    Custom(String),

    // Array/ repeated types
    Array(Box<FieldType>),

    // Optional wrapper (used when parsing)
    Optional(Box<FieldType>),
}

impl FieldType {
    /// Check if this type is an optional variant
    pub fn is_optional(&self) -> bool {
        matches!(self, Self::Optional(_))
    }

    /// Get the inner type if optional
    pub fn inner_type(&self) -> Option<&FieldType> {
        match self {
            Self::Optional(inner) => Some(inner.as_ref()),
            _ => None,
        }
    }

    /// Get the base type name (without Optional wrapper)
    pub fn base_type(&self) -> &FieldType {
        self.inner_type().unwrap_or(self)
    }
}

/// Field attribute (e.g., @unique, @default, @required)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldAttribute {
    pub name: String,
    pub args: Vec<String>,
}

impl FieldAttribute {
    /// Create a new attribute
    pub fn new(name: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            name: name.into(),
            args,
        }
    }

    /// Check if attribute has a specific name
    pub fn is(&self, name: &str) -> bool {
        self.name == name || self.name.starts_with(&format!("{}(", name))
    }

    /// Get argument value by index
    pub fn arg(&self, index: usize) -> Option<&String> {
        self.args.get(index)
    }

    /// Get first argument if present
    pub fn first_arg(&self) -> Option<&String> {
        self.args.first()
    }
}

/// Relation definition between entities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationDefinition {
    pub name: String,
    pub target_entity: String,
    pub relation_type: RelationType,
    pub attributes: Vec<String>,
}

/// Relation type enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RelationType {
    OneToOne,
    OneToMany,
    ManyToOne,
    ManyToMany,
}

impl RelationType {
    /// Check if this is a "to many" relation
    pub fn is_many(&self) -> bool {
        matches!(self, Self::OneToMany | Self::ManyToMany)
    }

    /// Check if this is a "to one" relation
    pub fn is_one(&self) -> bool {
        matches!(self, Self::OneToOne | Self::ManyToOne)
    }
}

/// Index definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexDefinition {
    pub index_type: IndexType,
    pub fields: Vec<String>,
}

/// Index type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum IndexType {
    Index,
    Unique,
}

/// Enum definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumDefinition {
    pub name: String,
    pub variants: Vec<EnumVariant>,
}

/// Enum variant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumVariant {
    pub name: String,
    pub description: Option<String>,
    pub is_default: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_type_optional() {
        let optional_type = FieldType::Optional(Box::new(FieldType::String));
        assert!(optional_type.is_optional());
        assert_eq!(optional_type.inner_type(), Some(&FieldType::String));
    }

    #[test]
    fn test_relation_type_is_many() {
        assert!(RelationType::OneToMany.is_many());
        assert!(RelationType::ManyToMany.is_many());
        assert!(!RelationType::OneToOne.is_many());
        assert!(!RelationType::ManyToOne.is_many());
    }

    #[test]
    fn test_field_attribute() {
        let attr = FieldAttribute::new("min", vec!["3".to_string()]);
        assert!(attr.is("min"));
        assert_eq!(attr.first_arg(), Some(&"3".to_string()));
    }
}
