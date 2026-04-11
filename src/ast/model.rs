//! Model AST definitions
//!
//! Defines AST nodes for model schemas including models, fields, relations, and enums.

use super::{Span, TypeRef};
use crate::utils::{to_snake_case, pluralize};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// A model definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Model {
    /// Model name (PascalCase)
    pub name: String,
    /// Database collection/table name
    pub collection: Option<String>,
    /// Model fields
    pub fields: Vec<Field>,
    /// Model relations
    pub relations: Vec<Relation>,
    /// Model indexes
    pub indexes: Vec<Index>,
    /// Model-level attributes
    pub attributes: Vec<Attribute>,
    /// Generator targets disabled for this specific model (from YAML `generators.disabled`)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_generators: Vec<String>,
    /// Generator targets enabled for this specific model (from YAML `generators.only` / `generators.enabled`)
    /// When non-empty: ONLY these targets are generated for this model (whitelist).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enabled_generators: Vec<String>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl Model {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Get the collection name (defaults to snake_case pluralized model name)
    pub fn collection_name(&self) -> String {
        self.collection
            .clone()
            .unwrap_or_else(|| to_snake_case_plural(&self.name))
    }

    /// Find a field by name
    pub fn find_field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Find a relation by name
    pub fn find_relation(&self, name: &str) -> Option<&Relation> {
        self.relations.iter().find(|r| r.name == name)
    }

    /// Get the primary key field
    pub fn primary_key(&self) -> Option<&Field> {
        self.fields.iter().find(|f| f.has_attribute("id"))
    }

    /// Check if model has soft delete
    ///
    /// Returns true if:
    /// - Model has a direct `deleted_at` field
    /// - Model has `@soft_delete` attribute
    /// - Model has a `metadata` field with `@audit_metadata` attribute (contains `deleted_at` via Timestamps composite)
    pub fn has_soft_delete(&self) -> bool {
        self.fields.iter().any(|f| f.name == "deleted_at")
            || self.has_attribute("soft_delete")
            || self.fields.iter().any(|f| f.name == "metadata" && f.has_attribute("audit_metadata"))
    }

    /// Check if model has a specific attribute
    pub fn has_attribute(&self, name: &str) -> bool {
        self.attributes.iter().any(|a| a.name == name)
    }

    /// Check if model has a strongly-typed ID (Uuid primary key)
    pub fn has_typed_id(&self) -> bool {
        use super::PrimitiveType;
        self.primary_key()
            .map(|f| matches!(&f.type_ref, TypeRef::Primitive(PrimitiveType::Uuid)))
            .unwrap_or(true) // Default Uuid when no explicit PK
    }
}

/// A field in a model
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Field {
    /// Field name (snake_case)
    pub name: String,
    /// Field type
    pub type_ref: TypeRef,
    /// Field attributes
    pub attributes: Vec<Attribute>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl Field {
    pub fn new(name: impl Into<String>, type_ref: TypeRef) -> Self {
        Self {
            name: name.into(),
            type_ref,
            attributes: Vec::new(),
            span: Span::default(),
        }
    }

    /// Check if field has a specific attribute
    pub fn has_attribute(&self, name: &str) -> bool {
        self.attributes.iter().any(|a| a.name == name)
    }

    /// Get an attribute by name
    pub fn get_attribute(&self, name: &str) -> Option<&Attribute> {
        self.attributes.iter().find(|a| a.name == name)
    }

    /// Find an attribute by name (alias for get_attribute)
    pub fn find_attribute(&self, name: &str) -> Option<&Attribute> {
        self.get_attribute(name)
    }

    /// Check if field is required (has @required or no ?)
    pub fn is_required(&self) -> bool {
        self.has_attribute("required") || !self.type_ref.is_optional()
    }

    /// Check if field is the primary key
    pub fn is_primary_key(&self) -> bool {
        self.has_attribute("id")
    }

    /// Check if field is unique
    pub fn is_unique(&self) -> bool {
        self.has_attribute("unique")
    }

    /// Get the default value if any
    pub fn default_value(&self) -> Option<&AttributeValue> {
        self.get_attribute("default")
            .and_then(|a| a.args.first())
            .map(|(_, v)| v)
    }
}

/// A relation between models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    /// Relation field name
    pub name: String,
    /// Related model type
    pub target: TypeRef,
    /// Relation type
    pub relation_type: RelationType,
    /// Relation attributes
    pub attributes: Vec<Attribute>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl Default for Relation {
    fn default() -> Self {
        Self {
            name: String::new(),
            target: TypeRef::Custom(String::new()),
            relation_type: RelationType::One,
            attributes: Vec::new(),
            span: Span::default(),
        }
    }
}

impl Relation {
    /// Get the foreign key field name
    pub fn foreign_key(&self) -> Option<String> {
        self.attributes
            .iter()
            .find(|a| a.name == "foreign_key")
            .and_then(|a| a.args.first())
            .and_then(|(_, v)| match v {
                AttributeValue::String(s) => Some(s.clone()),
                _ => None,
            })
    }

    /// Get the ON DELETE action for this relation's foreign key
    pub fn on_delete(&self) -> ForeignKeyAction {
        self.attributes
            .iter()
            .find(|a| a.name == "on_delete")
            .and_then(|a| a.first_arg())
            .and_then(|v| match v {
                AttributeValue::Ident(s) => match s.as_str() {
                    "cascade" => Some(ForeignKeyAction::Cascade),
                    "set_null" => Some(ForeignKeyAction::SetNull),
                    "set_default" => Some(ForeignKeyAction::SetDefault),
                    "restrict" => Some(ForeignKeyAction::Restrict),
                    "no_action" => Some(ForeignKeyAction::NoAction),
                    _ => None,
                },
                _ => None,
            })
            .unwrap_or_default()
    }

    /// Get the ON UPDATE action for this relation's foreign key
    pub fn on_update(&self) -> ForeignKeyAction {
        self.attributes
            .iter()
            .find(|a| a.name == "on_update")
            .and_then(|a| a.first_arg())
            .and_then(|v| match v {
                AttributeValue::Ident(s) => match s.as_str() {
                    "cascade" => Some(ForeignKeyAction::Cascade),
                    "set_null" => Some(ForeignKeyAction::SetNull),
                    "set_default" => Some(ForeignKeyAction::SetDefault),
                    "restrict" => Some(ForeignKeyAction::Restrict),
                    "no_action" => Some(ForeignKeyAction::NoAction),
                    _ => None,
                },
                _ => None,
            })
            .unwrap_or_default()
    }

    /// Get the referenced column name (defaults to "id")
    pub fn references(&self) -> String {
        self.attributes
            .iter()
            .find(|a| a.name == "references")
            .and_then(|a| a.first_arg())
            .and_then(|v| v.as_str())
            .unwrap_or("id")
            .to_string()
    }

    /// Get the join table name for many-to-many relations
    pub fn join_table(&self) -> Option<String> {
        self.attributes
            .iter()
            .find(|a| a.name == "join_table")
            .and_then(|a| a.first_arg())
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Get the foreign key column name for this side of many-to-many relation
    pub fn join_foreign_key(&self) -> Option<String> {
        self.attributes
            .iter()
            .find(|a| a.name == "join_fk")
            .and_then(|a| a.first_arg())
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Check if this relation should have a database foreign key constraint
    pub fn has_database_fk(&self) -> bool {
        // Has explicit foreign_key attribute, OR
        // Is one-to-many/one-to-one (not many-to-many) and not explicitly skipped
        self.foreign_key().is_some()
            || (self.relation_type != RelationType::ManyToMany && !self.has_attribute("no_fk"))
    }

    /// Check if relation has a specific attribute
    pub fn has_attribute(&self, name: &str) -> bool {
        self.attributes.iter().any(|a| a.name == name)
    }
}

/// Relation type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationType {
    /// One-to-one relation
    #[default]
    One,
    /// One-to-many relation (this side is "one")
    Many,
    /// Many-to-many relation
    ManyToMany,
}

/// Foreign key action for ON DELETE and ON UPDATE
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForeignKeyAction {
    /// NO ACTION - default behavior
    #[default]
    NoAction,
    /// CASCADE - delete/update dependent rows
    Cascade,
    /// SET NULL - set foreign key to NULL
    SetNull,
    /// SET DEFAULT - set foreign key to default value
    SetDefault,
    /// RESTRICT - prevent deletion/update
    Restrict,
}

/// An index definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Index {
    /// Index type
    pub index_type: IndexType,
    /// Fields included in the index
    pub fields: Vec<String>,
    /// Index name (optional, auto-generated if not specified)
    pub name: Option<String>,
    /// Additional attributes
    pub attributes: Vec<Attribute>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

/// Index type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexType {
    /// Regular index
    #[default]
    Index,
    /// Unique index
    Unique,
    /// Full-text search index
    Fulltext,
    /// GIN index (for JSONB)
    Gin,
}

/// An enum definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnumDef {
    /// Enum name (PascalCase)
    pub name: String,
    /// Enum variants
    pub variants: Vec<EnumVariant>,
    /// Enum attributes
    pub attributes: Vec<Attribute>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl EnumDef {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }
}

/// An enum variant
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnumVariant {
    /// Variant name (snake_case or SCREAMING_CASE)
    pub name: String,
    /// Optional numeric value
    pub value: Option<i32>,
    /// Variant attributes (e.g., @label("Active User"))
    pub attributes: Vec<Attribute>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

/// A custom type definition (for shared/reusable types)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TypeDef {
    /// Type name
    pub name: String,
    /// Type fields
    pub fields: Vec<TypeDefField>,
    /// Type attributes
    pub attributes: Vec<Attribute>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

/// A field in a custom type definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TypeDefField {
    /// Field name
    pub name: String,
    /// Field type
    pub type_ref: TypeRef,
    /// Field attributes
    pub attributes: Vec<Attribute>,
}

// =============================================================================
// DDD ENTITY & VALUE OBJECT DEFINITIONS
// =============================================================================

/// A DDD Entity definition (enhanced model with behavior)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Entity {
    /// Entity name (PascalCase)
    pub name: String,
    /// Reference to underlying model
    pub model_ref: String,
    /// Description
    pub description: Option<String>,
    /// Implemented traits/behaviors (e.g., Auditable, SoftDeletable)
    pub implements: Vec<String>,
    /// Value object field mappings (field_name -> ValueObject type)
    pub value_objects: IndexMap<String, String>,
    /// Entity methods (domain behavior)
    pub methods: Vec<EntityMethod>,
    /// Business invariants (always-true conditions)
    pub invariants: Vec<String>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl Entity {
    pub fn new(name: impl Into<String>, model_ref: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            model_ref: model_ref.into(),
            ..Default::default()
        }
    }

    /// Check if entity implements a specific trait
    pub fn implements(&self, trait_name: &str) -> bool {
        self.implements.iter().any(|t| t == trait_name)
    }

    /// Find a method by name
    pub fn find_method(&self, name: &str) -> Option<&EntityMethod> {
        self.methods.iter().find(|m| m.name == name)
    }
}

/// An entity method (domain behavior)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EntityMethod {
    /// Method name (snake_case)
    pub name: String,
    /// Whether this mutates entity state
    pub mutates: bool,
    /// Whether this is async
    pub is_async: bool,
    /// Method parameters (name -> type)
    pub params: IndexMap<String, TypeRef>,
    /// Return type
    pub returns: Option<TypeRef>,
    /// Description
    pub description: Option<String>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl EntityMethod {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Check if method is a query (doesn't mutate)
    pub fn is_query(&self) -> bool {
        !self.mutates
    }

    /// Check if method is a command (mutates state)
    pub fn is_command(&self) -> bool {
        self.mutates
    }
}

/// A DDD Value Object definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValueObject {
    /// Value object name (PascalCase)
    pub name: String,
    /// Description
    pub description: Option<String>,
    /// Inner/underlying type (for simple value objects)
    pub inner_type: Option<TypeRef>,
    /// Validation rule or expression
    pub validation: Option<String>,
    /// Value object methods
    pub methods: Vec<ValueObjectMethod>,
    /// Fields (for composite value objects)
    pub fields: Vec<Field>,
    /// Additional derives (e.g., Copy, Hash)
    pub derives: Vec<String>,
    /// Validation messages
    pub messages: IndexMap<String, String>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl ValueObject {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Check if this is a simple (single-value) value object
    pub fn is_simple(&self) -> bool {
        self.inner_type.is_some() && self.fields.is_empty()
    }

    /// Check if this is a composite (multi-field) value object
    pub fn is_composite(&self) -> bool {
        !self.fields.is_empty()
    }

    /// Find a method by name
    pub fn find_method(&self, name: &str) -> Option<&ValueObjectMethod> {
        self.methods.iter().find(|m| m.name == name)
    }
}

/// A value object method
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValueObjectMethod {
    /// Method name
    pub name: String,
    /// Return type
    pub returns: TypeRef,
    /// Parameters
    pub params: IndexMap<String, TypeRef>,
    /// Whether method is const/pure (doesn't mutate)
    pub is_const: bool,
    /// Description
    pub description: Option<String>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl ValueObjectMethod {
    pub fn new(name: impl Into<String>, returns: TypeRef) -> Self {
        Self {
            name: name.into(),
            returns,
            is_const: true, // Value objects are immutable by default
            ..Default::default()
        }
    }
}

// =============================================================================
// DOMAIN SERVICE DEFINITIONS
// =============================================================================

/// A DDD Domain Service definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DomainService {
    /// Service name (PascalCase)
    pub name: String,
    /// Description
    pub description: Option<String>,
    /// Whether stateless (true by default for domain services)
    pub stateless: bool,
    /// Service dependencies
    pub dependencies: Vec<ServiceDependency>,
    /// Service methods
    pub methods: Vec<ServiceMethod>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl DomainService {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            stateless: true,
            ..Default::default()
        }
    }

    /// Find a method by name
    pub fn find_method(&self, name: &str) -> Option<&ServiceMethod> {
        self.methods.iter().find(|m| m.name == name)
    }
}

/// Service dependency types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceDependency {
    /// Repository dependency
    Repository(String),
    /// Other service dependency
    Service(String),
    /// External client dependency
    Client(String),
}

impl Default for ServiceDependency {
    fn default() -> Self {
        Self::Repository(String::new())
    }
}

/// A service method definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServiceMethod {
    /// Method name
    pub name: String,
    /// Whether async
    pub is_async: bool,
    /// Method parameters (name -> type)
    pub params: IndexMap<String, TypeRef>,
    /// Return type
    pub returns: Option<TypeRef>,
    /// Error type (defaults to anyhow::Error)
    pub error_type: Option<String>,
    /// Description
    pub description: Option<String>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl ServiceMethod {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_async: true, // Domain services are typically async
            ..Default::default()
        }
    }
}

// =============================================================================
// EVENT SOURCING DEFINITIONS
// =============================================================================

/// Event sourcing configuration for an aggregate
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EventSourcedConfig {
    /// Entity/aggregate name
    pub entity_name: String,
    /// Description
    pub description: Option<String>,
    /// Domain events this entity can emit
    pub events: Vec<String>,
    /// Snapshot configuration
    pub snapshot: Option<SnapshotConfig>,
    /// Event handlers mapping event type -> handler expression
    pub handlers: IndexMap<String, String>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl EventSourcedConfig {
    pub fn new(entity_name: impl Into<String>) -> Self {
        Self {
            entity_name: entity_name.into(),
            ..Default::default()
        }
    }

    /// Check if snapshots are enabled
    pub fn snapshots_enabled(&self) -> bool {
        self.snapshot.as_ref().map(|s| s.enabled).unwrap_or(false)
    }
}

/// Snapshot configuration for event sourced aggregates
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapshotConfig {
    /// Whether snapshots are enabled
    pub enabled: bool,
    /// Create snapshot every N events
    pub every_n_events: u32,
    /// Maximum age before snapshot (in seconds)
    pub max_age_seconds: Option<u64>,
    /// Storage backend (e.g., "postgres", "redis")
    pub storage: Option<String>,
}

impl SnapshotConfig {
    pub fn new(every_n_events: u32) -> Self {
        Self {
            enabled: true,
            every_n_events,
            ..Default::default()
        }
    }
}

// =============================================================================
// USE CASE DEFINITIONS (Application Layer)
// =============================================================================

/// DDD Use Case - Application layer operation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UseCase {
    /// Use case name (PascalCase, e.g., CreateUser, UploadFile)
    pub name: String,
    /// Description of what this use case does
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Actor who initiates (User, Admin, System, Anonymous)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    /// Input parameters (param_name -> type)
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub input: IndexMap<String, TypeRef>,
    /// Output type
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<TypeRef>,
    /// Execution steps (documentation/pseudocode)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<String>,
    /// Whether this use case is async
    #[serde(default)]
    pub is_async: bool,
    /// Source span
    #[serde(skip)]
    pub span: Span,
}

impl UseCase {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_async: true,
            ..Default::default()
        }
    }
}

// =============================================================================
// DDD DOMAIN EVENT
// =============================================================================

/// DDD Domain Event - Facts about what happened in the domain
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DomainEvent {
    /// Event name (PascalCase, e.g., UserRegistered, OrderPlaced)
    pub name: String,
    /// Description of what this event represents
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Aggregate this event belongs to
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<String>,
    /// Event version for schema evolution
    #[serde(default)]
    pub version: u32,
    /// Storage configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<EventStorage>,
    /// Event fields/payload
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<EventField>,
    /// Version migrations (from_v1 -> field defaults)
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub migrations: IndexMap<String, IndexMap<String, String>>,
    /// Source span
    #[serde(skip)]
    pub span: Span,
}

/// Event storage configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EventStorage {
    /// Whether to persist this event to event store
    #[serde(default)]
    pub store: bool,
    /// Retention period (e.g., "7_years", "90_days")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention: Option<String>,
    /// Fields containing PII (for GDPR compliance)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pii_fields: Vec<String>,
    /// Fields to index for fast lookup
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub index_fields: Vec<String>,
}

/// Event field definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EventField {
    /// Field name
    pub name: String,
    /// Field type
    pub field_type: TypeRef,
    /// Field description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl DomainEvent {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: 1,
            storage: Some(EventStorage {
                store: true,
                ..Default::default()
            }),
            ..Default::default()
        }
    }
}

// =============================================================================
// CQRS PROJECTION
// =============================================================================

/// CQRS Projection - Denormalized read model built from events
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Projection {
    /// Projection name
    pub name: String,
    /// Description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether this is an aggregate projection
    #[serde(default)]
    pub aggregation: bool,
    /// Partition key for time-series
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partition_by: Option<String>,
    /// Storage configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<ProjectionStorage>,
    /// Source events
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_events: Vec<SourceEvent>,
    /// External events from other modules
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_events: Vec<SourceEvent>,
    /// Projection fields
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<ProjectionField>,
    /// Indexes
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<ProjectionIndex>,
    /// Source span
    #[serde(skip)]
    pub span: Span,
}

/// Projection storage configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectionStorage {
    /// Storage type
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_type: Option<String>,
    /// Table name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
}

/// Source event for projection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEvent {
    /// Event name
    pub name: String,
    /// Action type
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// Fields to update
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
}

/// Projection field
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectionField {
    /// Field name
    pub name: String,
    /// Field type
    pub field_type: TypeRef,
    /// Is primary key
    #[serde(default)]
    pub primary: bool,
    /// Source field mapping
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// Default value
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Is nullable
    #[serde(default)]
    pub nullable: bool,
}

/// Projection index
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectionIndex {
    /// Fields in index
    pub fields: Vec<String>,
    /// Is unique
    #[serde(default)]
    pub unique: bool,
}

// =============================================================================
// APPLICATION SERVICE
// =============================================================================

/// Application Service - Shared application logic
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppService {
    /// Service name
    pub name: String,
    /// Description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Is async
    #[serde(default)]
    pub is_async: bool,
    /// Dependencies
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<(String, String)>,
    /// Methods
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<AppServiceMethod>,
    /// Source span
    #[serde(skip)]
    pub span: Span,
}

/// Application service method
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppServiceMethod {
    /// Method name
    pub name: String,
    /// Description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Parameters
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub params: IndexMap<String, TypeRef>,
    /// Return type
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returns: Option<TypeRef>,
}

// =============================================================================
// EVENT HANDLER
// =============================================================================

/// Event Handler - Reacts to domain events
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Handler {
    /// Handler name
    pub name: String,
    /// Description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Event this handler responds to
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    /// Dependencies
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<(String, String)>,
    /// Retry policy
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<HandlerRetryPolicy>,
    /// Async dispatch
    #[serde(default)]
    pub async_dispatch: bool,
    /// Transaction requirement
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction: Option<String>,
    /// Source span
    #[serde(skip)]
    pub span: Span,
}

/// Handler retry policy
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HandlerRetryPolicy {
    /// Max attempts
    #[serde(default)]
    pub max_attempts: u32,
    /// Backoff strategy
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backoff: Option<String>,
    /// Initial delay
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_delay: Option<String>,
    /// Max delay
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_delay: Option<String>,
}

/// Event subscription
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Subscription {
    /// Source module
    pub module: String,
    /// Event name
    pub event: String,
    /// Handler name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handler: Option<String>,
    /// Description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Condition
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

// =============================================================================
// INTEGRATION / ACL
// =============================================================================

/// Integration adapter (Anti-Corruption Layer)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Integration {
    /// Integration name
    pub name: String,
    /// Description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Is async
    #[serde(default)]
    pub is_async: bool,
    /// Methods
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<IntegrationMethod>,
    /// Source span
    #[serde(skip)]
    pub span: Span,
}

/// Integration method
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IntegrationMethod {
    /// Method name
    pub name: String,
    /// Parameters
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub params: IndexMap<String, TypeRef>,
    /// Return type
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returns: Option<TypeRef>,
}

// =============================================================================
// PRESENTATION LAYER
// =============================================================================

/// Presentation layer configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Presentation {
    /// HTTP configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpConfig>,
    /// gRPC configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grpc: Option<GrpcConfig>,
}

/// HTTP configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HttpConfig {
    /// API prefix
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Route groups
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub routes: IndexMap<String, RouteGroup>,
}

/// Route group
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RouteGroup {
    /// Prefix
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Middleware
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub middleware: Vec<String>,
    /// Endpoints
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub endpoints: Vec<Endpoint>,
}

/// HTTP Endpoint
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Endpoint {
    /// Name
    pub name: String,
    /// HTTP method
    pub method: String,
    /// Path
    pub path: String,
    /// Use case
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usecase: Option<String>,
    /// Response type
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
    /// Status code
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// Is public
    #[serde(default)]
    pub public: bool,
}

/// gRPC configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GrpcConfig {
    /// Package name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// Services
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub services: IndexMap<String, GrpcService>,
}

/// gRPC service
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GrpcService {
    /// Description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Methods
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<GrpcMethod>,
}

/// gRPC method
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GrpcMethod {
    /// Name
    pub name: String,
    /// Input type
    pub input: String,
    /// Output type
    pub output: String,
    /// Is public
    #[serde(default)]
    pub public: bool,
}

// =============================================================================
// DTO
// =============================================================================

/// DTO definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Dto {
    /// DTO name
    pub name: String,
    /// Description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Is generic
    #[serde(default)]
    pub generic: bool,
    /// Source entity
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_entity: Option<String>,
    /// Fields
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<DtoField>,
    /// Excluded fields
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    /// Computed fields
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub computed: Vec<ComputedDtoField>,
    /// Source span
    #[serde(skip)]
    pub span: Span,
}

/// DTO field
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DtoField {
    /// Simple field name
    Simple(String),
    /// Full field definition
    Full {
        name: String,
        field_type: TypeRef,
        optional: bool,
    },
}

/// Computed DTO field
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComputedDtoField {
    /// Name
    pub name: String,
    /// Type
    pub field_type: TypeRef,
    /// Expression
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
}

// =============================================================================
// VERSIONING
// =============================================================================

/// API Versioning configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Versioning {
    /// Strategy
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
    /// Current version
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    /// Supported versions
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported: Vec<String>,
    /// Deprecated versions
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deprecated: Vec<String>,
}

// =============================================================================
// REPOSITORY TRAIT
// =============================================================================

/// Repository trait definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepositoryTrait {
    /// Trait name
    pub name: String,
    /// Description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Parent trait
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    /// Entity type
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity: Option<String>,
    /// Is async
    #[serde(default)]
    pub is_async: bool,
    /// Error type
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
    /// Auto methods
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auto_methods: Vec<String>,
    /// Custom methods
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<TraitMethod>,
    /// Source span
    #[serde(skip)]
    pub span: Span,
}

/// Trait method
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TraitMethod {
    /// Name
    pub name: String,
    /// Parameters
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub params: IndexMap<String, TypeRef>,
    /// Return type
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returns: Option<TypeRef>,
}

/// An attribute on a field, model, enum, etc.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Attribute {
    /// Attribute name (without @)
    pub name: String,
    /// Attribute arguments (can be positional or named)
    pub args: Vec<(Option<String>, AttributeValue)>,
    /// Source location
    #[serde(skip)]
    pub span: Span,
}

impl Attribute {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            args: Vec::new(),
            span: Span::default(),
        }
    }

    pub fn with_arg(mut self, value: AttributeValue) -> Self {
        self.args.push((None, value));
        self
    }

    pub fn with_named_arg(mut self, name: impl Into<String>, value: AttributeValue) -> Self {
        self.args.push((Some(name.into()), value));
        self
    }

    /// Get first positional argument
    pub fn first_arg(&self) -> Option<&AttributeValue> {
        self.args.first().map(|(_, v)| v)
    }

    /// Get a named argument
    pub fn get_named(&self, name: &str) -> Option<&AttributeValue> {
        self.args
            .iter()
            .find(|(n, _)| n.as_deref() == Some(name))
            .map(|(_, v)| v)
    }

    /// Get first positional argument as string
    pub fn get_string_arg(&self) -> Option<&str> {
        self.first_arg().and_then(|v| v.as_str())
    }
}

/// Attribute value types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttributeValue {
    /// String value
    String(String),
    /// Integer value
    Int(i64),
    /// Float value
    Float(f64),
    /// Boolean value
    Bool(bool),
    /// Identifier reference (e.g., field name, enum variant)
    Ident(String),
    /// Array of values
    Array(Vec<AttributeValue>),
    /// Nested object/map
    Object(IndexMap<String, AttributeValue>),
}

impl Default for AttributeValue {
    fn default() -> Self {
        Self::Bool(true)
    }
}

impl AttributeValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) | Self::Ident(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

// Helper function to convert PascalCase to snake_case plural
fn to_snake_case_plural(name: &str) -> String {
    pluralize(&to_snake_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_snake_case_plural() {
        assert_eq!(to_snake_case_plural("User"), "users");
        assert_eq!(to_snake_case_plural("UserRole"), "user_roles");
        assert_eq!(to_snake_case_plural("APIKey"), "api_keys");
        assert_eq!(to_snake_case_plural("MFADevice"), "mfa_devices");
    }
}
