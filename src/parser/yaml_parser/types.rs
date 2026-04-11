//! YAML schema type definitions (intermediate representation)
//!
//! All `YamlXxx` structs that map directly to YAML schema file structure.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

// ============================================================================
// Constants
// ============================================================================

/// The name of the special Metadata composition type that gets converted
/// to a single JSONB column with audit fields instead of separate columns.
pub const AUDIT_METADATA_TYPE_NAME: &str = "Metadata";

// ============================================================================
// YAML Schema Structures (Intermediate representation)
// ============================================================================

/// Root structure for model YAML files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlModelSchema {
    /// Models defined in this file
    #[serde(default)]
    pub models: Vec<YamlModel>,
    /// Enums defined in this file
    #[serde(default)]
    pub enums: Vec<YamlEnum>,
    /// Custom types defined in this file
    #[serde(default)]
    pub types: Vec<YamlTypeDef>,
    /// File-level generator config — applies to ALL models in this file.
    /// Per-entity `generators:` inside a model entry takes precedence over this.
    /// Example:
    ///   generators:
    ///     disabled: [viewmodels, components]
    #[serde(default)]
    pub generators: Option<GeneratorsConfig>,

    // ==========================================================================
    // DDD & AUTHORIZATION EXTENSIONS
    // ==========================================================================

    /// DDD Entity definitions (enhanced models with behavior)
    #[serde(default)]
    pub entities: IndexMap<String, YamlEntity>,
    /// DDD Value Object definitions
    #[serde(default)]
    pub value_objects: IndexMap<String, YamlValueObject>,
    /// DDD Domain Service definitions
    #[serde(default)]
    pub domain_services: IndexMap<String, YamlDomainService>,
    /// Event Sourcing configurations
    #[serde(default)]
    pub event_sourced: IndexMap<String, YamlEventSourced>,
    /// Authorization configuration (RBAC/ABAC)
    #[serde(default)]
    pub authorization: Option<YamlAuthorization>,
    /// DDD Use Case definitions (Application layer operations)
    #[serde(default)]
    pub usecases: IndexMap<String, YamlUseCase>,
    /// DDD Domain Event definitions
    #[serde(default)]
    pub events: IndexMap<String, YamlDomainEvent>,
    /// CQRS Projection definitions (read models)
    #[serde(default)]
    pub projections: IndexMap<String, YamlProjection>,
    /// Application Service definitions
    #[serde(default)]
    pub services: IndexMap<String, YamlAppService>,
    /// Event Handler definitions
    #[serde(default)]
    pub handlers: IndexMap<String, YamlHandler>,
    /// Event Subscription definitions (cross-module)
    #[serde(default)]
    pub subscribes_to: IndexMap<String, IndexMap<String, YamlSubscription>>,
    /// Integration/ACL adapter definitions
    #[serde(default)]
    pub integration: IndexMap<String, YamlIntegration>,
    /// Presentation layer configuration (HTTP/gRPC)
    #[serde(default)]
    pub presentation: Option<YamlPresentation>,
    /// DTO definitions (request/response)
    #[serde(default)]
    pub dtos: IndexMap<String, YamlDto>,
    /// API Versioning configuration
    #[serde(default)]
    pub versioning: Option<YamlVersioning>,
    /// Repository trait definitions
    #[serde(default)]
    pub traits: IndexMap<String, YamlRepositoryTrait>,
}

/// A model definition in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlModel {
    /// Model name
    pub name: String,
    /// Database collection/table name
    #[serde(default)]
    pub collection: Option<String>,
    /// Enable soft delete for this model
    #[serde(default)]
    pub soft_delete: Option<bool>,
    /// Extend shared types - fields from these types are injected into the model as table columns
    /// Example: extends: [Metadata] will inject created_at, updated_at, etc. as columns
    #[serde(default)]
    pub extends: Vec<String>,
    /// Model-local type definitions (used as JSONB fields within this model)
    #[serde(default)]
    pub types: IndexMap<String, IndexMap<String, YamlField>>,
    /// Model fields
    #[serde(default)]
    pub fields: IndexMap<String, YamlField>,
    /// Model relations
    #[serde(default)]
    pub relations: IndexMap<String, YamlRelation>,
    /// Model indexes
    #[serde(default)]
    pub indexes: Vec<YamlIndex>,
    /// Per-model generator overrides (e.g. disable viewmodels for this entity)
    #[serde(default)]
    pub generators: Option<GeneratorsConfig>,
}

/// A field definition in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum YamlField {
    /// Simple form: just the type string
    Simple(String),
    /// Full form: type with attributes
    Full {
        #[serde(rename = "type")]
        field_type: String,
        #[serde(default)]
        attributes: Vec<String>,
        #[serde(default)]
        description: Option<String>,
    },
}

/// A relation definition in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlRelation {
    /// Target type (e.g., "Role[]", "Profile")
    #[serde(rename = "type")]
    pub target_type: String,
    /// Relation attributes
    #[serde(default)]
    pub attributes: Vec<String>,
}

/// An index definition in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlIndex {
    /// Index type: index, unique, fulltext, gin
    #[serde(rename = "type", default = "default_index_type")]
    pub index_type: String,
    /// Fields in this index
    pub fields: Vec<String>,
    /// Optional index name
    #[serde(default)]
    pub name: Option<String>,
    /// Optional where clause (for partial indexes)
    #[serde(rename = "where", default)]
    pub where_clause: Option<String>,
}

fn default_index_type() -> String {
    "index".to_string()
}

/// An enum definition in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlEnum {
    /// Enum name
    pub name: String,
    /// Enum variants
    pub variants: Vec<YamlEnumVariant>,
}

/// An enum variant in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum YamlEnumVariant {
    /// Simple form: just the name
    Simple(String),
    /// Full form: name with attributes
    Full {
        name: String,
        #[serde(default)]
        value: Option<i32>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        default: Option<bool>,
    },
}

/// A custom type definition in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlTypeDef {
    /// Type name
    pub name: String,
    /// Type fields
    pub fields: IndexMap<String, YamlField>,
}

// ============================================================================
// DDD Entity YAML Structures
// ============================================================================

/// DDD Entity definition in YAML
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlEntity {
    /// Reference to the model this entity is based on
    #[serde(default)]
    pub model: Option<String>,
    /// Description of this entity
    #[serde(default)]
    pub description: Option<String>,
    /// Traits/interfaces this entity implements (e.g., Auditable, SoftDeletable)
    #[serde(default)]
    pub implements: Vec<String>,
    /// Value objects used by this entity (field_name -> ValueObject name)
    #[serde(default)]
    pub value_objects: IndexMap<String, String>,
    /// Entity methods (behavior)
    #[serde(default)]
    pub methods: Vec<YamlEntityMethod>,
    /// Business invariants that must always be true
    #[serde(default)]
    pub invariants: Vec<String>,
}

/// Entity method definition in YAML
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlEntityMethod {
    /// Method name
    pub name: String,
    /// Description of what this method does
    #[serde(default)]
    pub description: Option<String>,
    /// Whether this method mutates the entity state
    #[serde(default)]
    pub mutates: Option<bool>,
    /// Whether this method is async
    #[serde(rename = "async", default)]
    pub is_async: Option<bool>,
    /// Method parameters (param_name -> type)
    #[serde(default)]
    pub params: IndexMap<String, String>,
    /// Return type (if any)
    #[serde(default)]
    pub returns: Option<String>,
}

// ============================================================================
// DDD Value Object YAML Structures
// ============================================================================

/// DDD Value Object definition in YAML
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlValueObject {
    /// Description of this value object
    #[serde(default)]
    pub description: Option<String>,
    /// Inner type for wrapper value objects (e.g., Email wraps String)
    #[serde(default)]
    pub inner_type: Option<String>,
    /// Fields for composite value objects
    #[serde(default)]
    pub fields: IndexMap<String, YamlField>,
    /// Validation rule/constraint
    #[serde(default)]
    pub validation: Option<String>,
    /// Additional traits to derive
    #[serde(default)]
    pub derives: Vec<String>,
    /// Methods on this value object
    #[serde(default)]
    pub methods: Vec<YamlValueObjectMethod>,
    /// Error messages for validation failures
    #[serde(default)]
    pub messages: IndexMap<String, String>,
}

/// Value object method definition in YAML
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlValueObjectMethod {
    /// Method name
    pub name: String,
    /// Description of what this method does
    #[serde(default)]
    pub description: Option<String>,
    /// Return type
    #[serde(default)]
    pub returns: Option<String>,
    /// Method parameters (param_name -> type)
    #[serde(default)]
    pub params: IndexMap<String, String>,
    /// Whether this is a const method (doesn't take &self)
    #[serde(rename = "const", default)]
    pub is_const: Option<bool>,
}

// ============================================================================
// DDD Domain Service YAML Structures
// ============================================================================

/// DDD Domain Service definition in YAML
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlDomainService {
    /// Description of this service
    #[serde(default)]
    pub description: Option<String>,
    /// Whether this service is stateless
    #[serde(default)]
    pub stateless: Option<bool>,
    /// Dependencies required by this service
    #[serde(default)]
    pub dependencies: Vec<YamlServiceDependency>,
    /// Methods provided by this service
    #[serde(default)]
    pub methods: Vec<YamlServiceMethod>,
}

/// Service dependency in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum YamlServiceDependency {
    /// Simple string form: "UserRepository"
    Simple(String),
    /// Full form with type specification
    Full {
        /// Dependency name
        name: String,
        /// Dependency type: repository, service, client
        #[serde(rename = "type")]
        dep_type: String,
    },
}

/// Service method definition in YAML
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlServiceMethod {
    /// Method name
    pub name: String,
    /// Description of what this method does
    #[serde(default)]
    pub description: Option<String>,
    /// Whether this method is async
    #[serde(rename = "async", default)]
    pub is_async: Option<bool>,
    /// Method parameters (param_name -> type)
    #[serde(default)]
    pub params: IndexMap<String, String>,
    /// Return type (if any)
    #[serde(default)]
    pub returns: Option<String>,
    /// Error type (if any)
    #[serde(default)]
    pub error: Option<String>,
}

// ============================================================================
// Use Case YAML Structures (Application Layer)
// ============================================================================

/// DDD Use Case definition in YAML
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlUseCase {
    /// Description of this use case
    #[serde(default)]
    pub description: Option<String>,
    /// Actor who initiates this use case (User, Admin, System, etc.)
    #[serde(default)]
    pub actor: Option<String>,
    /// Input parameters/DTO (param_name -> type)
    #[serde(default)]
    pub input: IndexMap<String, String>,
    /// Output type
    #[serde(default)]
    pub output: Option<String>,
    /// Execution steps (for documentation/pseudocode)
    #[serde(default)]
    pub steps: Vec<String>,
    /// Whether this use case is async
    #[serde(rename = "async", default)]
    pub is_async: Option<bool>,
}

// ============================================================================
// Event Sourcing YAML Structures
// ============================================================================

/// Event Sourcing configuration in YAML
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlEventSourced {
    /// Description of this event-sourced aggregate
    #[serde(default)]
    pub description: Option<String>,
    /// Domain events this aggregate can emit
    #[serde(default)]
    pub events: Vec<String>,
    /// Snapshot configuration
    #[serde(default)]
    pub snapshot: Option<YamlSnapshotConfig>,
    /// Event handlers (event_name -> handler expression)
    #[serde(default)]
    pub handlers: IndexMap<String, String>,
}

/// Snapshot configuration in YAML
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlSnapshotConfig {
    /// Whether snapshots are enabled
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Take snapshot every N events
    #[serde(default)]
    pub every_n_events: Option<u32>,
    /// Maximum age of snapshot in seconds
    #[serde(default)]
    pub max_age_seconds: Option<u64>,
    /// Storage backend for snapshots
    #[serde(default)]
    pub storage: Option<String>,
}

// ============================================================================
// Authorization YAML Structures
// ============================================================================

/// Authorization configuration in YAML
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlAuthorization {
    /// Resource permissions (resource_name -> allowed_actions)
    #[serde(default)]
    pub permissions: IndexMap<String, Vec<String>>,
    /// Role definitions
    #[serde(default)]
    pub roles: IndexMap<String, YamlRoleDefinition>,
    /// Policy definitions
    #[serde(default)]
    pub policies: IndexMap<String, YamlPolicy>,
    /// Resource-level policy mappings
    #[serde(default)]
    pub resource_policies: IndexMap<String, YamlResourcePolicy>,
    /// ABAC attribute definitions
    #[serde(default)]
    pub attributes: Option<YamlAbacAttributes>,
    /// ABAC policies
    #[serde(default)]
    pub abac_policies: IndexMap<String, YamlAbacPolicy>,
}

/// Role definition in YAML
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlRoleDefinition {
    /// Description of this role
    #[serde(default)]
    pub description: Option<String>,
    /// Assigned permissions (supports wildcards like "users.*")
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Role level in hierarchy (higher = more privileged)
    #[serde(default)]
    pub level: Option<i32>,
    /// Parent role name (inherits permissions from)
    #[serde(default)]
    pub inherits: Option<String>,
    /// Own resource permissions (e.g., owner-based access)
    #[serde(default)]
    pub own_resources: IndexMap<String, String>,
}

/// Policy definition in YAML
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlPolicy {
    /// Description of this policy
    #[serde(default)]
    pub description: Option<String>,
    /// Policy type: any, all, none
    #[serde(rename = "type", default)]
    pub policy_type: Option<String>,
    /// Policy rules
    #[serde(default)]
    pub rules: Vec<YamlPolicyRule>,
}

/// Policy rule in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum YamlPolicyRule {
    /// Simple permission string
    Simple(String),
    /// Full rule specification
    Full {
        /// Permission requirement
        #[serde(default)]
        permission: Option<String>,
        /// Role requirement
        #[serde(default)]
        role: Option<String>,
        /// Owner check
        #[serde(default)]
        owner: Option<YamlOwnerRule>,
        /// Custom condition
        #[serde(default)]
        condition: Option<String>,
        /// Error message
        #[serde(default)]
        message: Option<String>,
        /// Negation
        #[serde(default)]
        not: Option<Box<YamlPolicyRule>>,
        /// Reference to another policy
        #[serde(default)]
        policy: Option<String>,
    },
}

/// Owner rule in YAML
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlOwnerRule {
    /// Resource name
    #[serde(default)]
    pub resource: Option<String>,
    /// Field to check for ownership
    pub field: String,
    /// Actor field to compare against
    #[serde(default)]
    pub actor_field: Option<String>,
}

/// Resource policy in YAML
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlResourcePolicy {
    /// Read access rules
    #[serde(default)]
    pub read: Vec<YamlResourcePolicyRule>,
    /// Create access rules
    #[serde(default)]
    pub create: Vec<YamlResourcePolicyRule>,
    /// Update access rules
    #[serde(default)]
    pub update: Vec<YamlResourcePolicyRule>,
    /// Delete access rules
    #[serde(default)]
    pub delete: Vec<YamlResourcePolicyRule>,
    /// Custom action rules (action_name -> rules)
    #[serde(default, flatten)]
    pub custom: IndexMap<String, Vec<YamlResourcePolicyRule>>,
}

/// Resource policy rule in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum YamlResourcePolicyRule {
    /// Simple string (policy reference or permission)
    Simple(String),
    /// Full rule
    Full {
        /// Reference to a policy
        #[serde(default)]
        policy: Option<String>,
        /// Permission requirement
        #[serde(default)]
        permission: Option<String>,
        /// Owner check field
        #[serde(default)]
        owner: Option<String>,
        /// Custom condition
        #[serde(default)]
        condition: Option<String>,
        /// Error message
        #[serde(default)]
        message: Option<String>,
    },
}

/// ABAC attributes in YAML
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlAbacAttributes {
    /// Subject (user/actor) attributes
    #[serde(default)]
    pub subject: Vec<String>,
    /// Resource attributes
    #[serde(default)]
    pub resource: Vec<String>,
    /// Environment attributes
    #[serde(default)]
    pub environment: Vec<String>,
}

/// ABAC policy in YAML
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlAbacPolicy {
    /// Description of this policy
    #[serde(default)]
    pub description: Option<String>,
    /// Condition expression
    #[serde(default)]
    pub condition: Option<String>,
}

// ============================================================================
// Model Index YAML Structures (index.model.yaml)
// ============================================================================

/// Module-level model index file (index.model.yaml)
/// This is NOT a standard model but a configuration file that:
/// - Defines shared types available to all models
/// - Imports model files
/// - Module-level configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlModelIndexSchema {
    /// Module name
    #[serde(default)]
    pub module: Option<String>,
    /// Version
    #[serde(default)]
    pub version: Option<u32>,
    /// Shared types available to all models in this module
    #[serde(default)]
    pub shared_types: IndexMap<String, YamlSharedType>,
    /// Model file imports
    #[serde(default)]
    pub imports: Vec<String>,
    /// Module-level configuration
    #[serde(default)]
    pub config: Option<YamlModelModuleConfig>,
}

/// A shared type definition that can be:
/// - A type with fields (like Timestamps, Actors)
/// - A composition of other types (like AuditLog: [Timestamps, Actors])
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum YamlSharedType {
    /// Composition: array of type names to extend
    Composition(Vec<String>),
    /// Direct fields definition
    Fields(IndexMap<String, YamlField>),
}

/// Module-level configuration for models
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlModelModuleConfig {
    /// Database type (postgresql, mongodb)
    #[serde(default)]
    pub database: Option<String>,
    /// Enable soft delete by default
    #[serde(default)]
    pub soft_delete: Option<bool>,
    /// Enable audit logging by default
    #[serde(default)]
    pub audit: Option<bool>,
    /// Add default timestamp fields
    #[serde(default)]
    pub default_timestamps: Option<bool>,
    /// Generator configuration — controls which code generation targets are enabled
    #[serde(default)]
    pub generators: Option<GeneratorsConfig>,
}

/// Configuration for which code generators are enabled/disabled.
///
/// When `enabled` is set, ONLY those targets are generated (whitelist mode).
/// When `disabled` is set, all targets EXCEPT those are generated (blacklist mode).
/// If both are set, `enabled` takes precedence.
/// If neither is set, all targets are generated (default behavior).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GeneratorsConfig {
    /// Whitelist: only generate these targets
    #[serde(default)]
    pub enabled: Option<Vec<String>>,
    /// Blacklist: generate all targets except these
    #[serde(default)]
    pub disabled: Option<Vec<String>>,
    /// Opt-in flag for CQRS/Projection generation.
    /// When absent or false, `Cqrs` and `Projection` targets are skipped.
    /// Set to `true` to generate CQRS command/query objects and read-model projections.
    #[serde(default)]
    pub cqrs: Option<bool>,
}

// ============================================================================
// Hook YAML Structures (Entity Lifecycle Behaviors)
// ============================================================================

/// Module-level hook index file (index.hook.yaml)
/// This is NOT a standard hook but a configuration file that:
/// - Imports other hook files
/// - Defines module-level config, events, scheduled jobs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlHookIndexSchema {
    /// Module name
    #[serde(default)]
    pub module: Option<String>,
    /// Version
    #[serde(default)]
    pub version: Option<u32>,
    /// Hook file imports
    #[serde(default)]
    pub imports: Vec<String>,
    /// Module-level configuration
    #[serde(default)]
    pub config: Option<YamlModuleConfig>,
    /// Shared domain events
    #[serde(default)]
    pub events: IndexMap<String, YamlDomainEvent>,
    /// Scheduled jobs
    #[serde(default)]
    pub scheduled_jobs: IndexMap<String, YamlScheduledJob>,
}

/// Module-level configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlModuleConfig {
    /// Default user role
    #[serde(default)]
    pub default_user_role: Option<String>,
    /// Auth settings
    #[serde(default)]
    pub auth: Option<serde_yaml::Value>,
    /// Audit settings
    #[serde(default)]
    pub audit: Option<serde_yaml::Value>,
    /// Notification settings
    #[serde(default)]
    pub notifications: Option<serde_yaml::Value>,
}

/// Domain event definition (enhanced for DDD)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlDomainEvent {
    /// Event description
    #[serde(default)]
    pub description: Option<String>,
    /// Aggregate this event belongs to
    #[serde(default)]
    pub aggregate: Option<String>,
    /// Event version for schema evolution
    #[serde(default)]
    pub version: Option<u32>,
    /// Storage configuration
    #[serde(default)]
    pub storage: Option<YamlEventStorage>,
    /// Event fields/payload - can be simple map or list of field definitions
    #[serde(default)]
    pub fields: Vec<YamlEventField>,
    /// Version migrations for backward compatibility
    #[serde(default)]
    pub migrations: IndexMap<String, IndexMap<String, String>>,
}

/// Event storage configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlEventStorage {
    /// Whether to persist this event to event store
    #[serde(default = "default_true")]
    pub store: bool,
    /// Retention period (e.g., "7_years", "90_days")
    #[serde(default)]
    pub retention: Option<String>,
    /// Fields containing PII (for GDPR compliance)
    #[serde(default)]
    pub pii_fields: Vec<String>,
    /// Fields to index for fast lookup
    #[serde(default)]
    pub index_fields: Vec<String>,
}

fn default_true() -> bool {
    true
}

/// Event field definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlEventField {
    /// Field name
    pub name: String,
    /// Field type
    #[serde(rename = "type")]
    pub field_type: String,
    /// Field description
    #[serde(default)]
    pub description: Option<String>,
}

// =============================================================================
// CQRS PROJECTION DEFINITIONS
// =============================================================================

/// CQRS Projection definition (read model)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlProjection {
    /// Projection description
    #[serde(default)]
    pub description: Option<String>,
    /// Whether this is an aggregate projection
    #[serde(default)]
    pub aggregation: Option<bool>,
    /// Partition key for time-series projections
    #[serde(default)]
    pub partition_by: Option<String>,
    /// Storage configuration
    #[serde(default)]
    pub storage: Option<YamlProjectionStorage>,
    /// Events that build this projection
    #[serde(default)]
    pub source_events: Vec<YamlSourceEvent>,
    /// External events from other modules
    #[serde(default)]
    pub external_events: Vec<YamlSourceEvent>,
    /// Projection fields
    #[serde(default)]
    pub fields: Vec<YamlProjectionField>,
    /// Indexes
    #[serde(default)]
    pub indexes: Vec<YamlProjectionIndex>,
}

/// Projection storage configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlProjectionStorage {
    /// Storage type (postgres, redis, etc.)
    #[serde(rename = "type", default)]
    pub storage_type: Option<String>,
    /// Table/collection name
    #[serde(default)]
    pub table: Option<String>,
}

/// Source event for projection
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum YamlSourceEvent {
    /// Simple event name
    Simple(String),
    /// Event with action configuration
    WithAction(IndexMap<String, YamlSourceEventAction>),
}

/// Source event action configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlSourceEventAction {
    /// Action type (insert, update, delete)
    #[serde(default)]
    pub action: Option<String>,
    /// Fields to update (for partial updates)
    #[serde(default)]
    pub fields: Vec<String>,
}

/// Projection field definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlProjectionField {
    /// Field name
    pub name: String,
    /// Field type
    #[serde(rename = "type")]
    pub field_type: String,
    /// Is primary key
    #[serde(default)]
    pub primary: Option<bool>,
    /// Source field mapping
    #[serde(default)]
    pub from: Option<String>,
    /// Default value
    #[serde(default)]
    pub default: Option<String>,
    /// Is nullable
    #[serde(default)]
    pub nullable: Option<bool>,
}

/// Projection index definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlProjectionIndex {
    /// Fields in index
    pub fields: Vec<String>,
    /// Is unique index
    #[serde(default)]
    pub unique: Option<bool>,
}

// =============================================================================
// APPLICATION SERVICE DEFINITIONS
// =============================================================================

/// Application Service definition
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlAppService {
    /// Service description
    #[serde(default)]
    pub description: Option<String>,
    /// Whether async
    #[serde(rename = "async", default)]
    pub is_async: Option<bool>,
    /// Dependencies
    #[serde(default)]
    pub dependencies: Vec<YamlServiceDep>,
    /// Service methods
    #[serde(default)]
    pub methods: Vec<YamlAppServiceMethod>,
}

/// Service dependency
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum YamlServiceDep {
    /// Simple string format "name: Type"
    Simple(String),
    /// Map format { name: Type }
    Map(IndexMap<String, String>),
}

/// Application service method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlAppServiceMethod {
    /// Method name
    pub name: String,
    /// Method description
    #[serde(default)]
    pub description: Option<String>,
    /// Parameters
    #[serde(default)]
    pub params: Vec<IndexMap<String, String>>,
    /// Return type
    #[serde(default)]
    pub returns: Option<String>,
}

// =============================================================================
// EVENT HANDLER DEFINITIONS
// =============================================================================

/// Event Handler definition
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlHandler {
    /// Handler description
    #[serde(default)]
    pub description: Option<String>,
    /// Event this handler responds to
    #[serde(default)]
    pub event: Option<String>,
    /// Dependencies
    #[serde(default)]
    pub dependencies: Vec<YamlServiceDep>,
    /// Retry policy
    #[serde(default)]
    pub retry: Option<YamlHandlerRetryPolicy>,
    /// Whether to dispatch asynchronously
    #[serde(default)]
    pub async_dispatch: Option<bool>,
    /// Transaction requirement
    #[serde(default)]
    pub transaction: Option<String>,
}

/// Handler retry policy configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlHandlerRetryPolicy {
    /// Maximum retry attempts
    #[serde(default)]
    pub max_attempts: Option<u32>,
    /// Backoff strategy
    #[serde(default)]
    pub backoff: Option<String>,
    /// Initial delay
    #[serde(default)]
    pub initial_delay: Option<String>,
    /// Maximum delay
    #[serde(default)]
    pub max_delay: Option<String>,
}

/// Event subscription definition
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlSubscription {
    /// Handler name
    #[serde(default)]
    pub handler: Option<String>,
    /// Subscription description
    #[serde(default)]
    pub description: Option<String>,
    /// Condition for handling
    #[serde(default)]
    pub condition: Option<String>,
}

// =============================================================================
// INTEGRATION/ACL DEFINITIONS
// =============================================================================

/// Integration adapter definition (Anti-Corruption Layer)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlIntegration {
    /// Integration description
    #[serde(default)]
    pub description: Option<String>,
    /// Whether async
    #[serde(rename = "async", default)]
    pub is_async: Option<bool>,
    /// Methods
    #[serde(default)]
    pub methods: Vec<YamlIntegrationMethod>,
}

/// Integration method definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlIntegrationMethod {
    /// Method name
    pub name: String,
    /// Parameters
    #[serde(default)]
    pub params: Vec<IndexMap<String, String>>,
    /// Return type
    #[serde(default)]
    pub returns: Option<String>,
}

// =============================================================================
// PRESENTATION LAYER DEFINITIONS
// =============================================================================

/// Presentation layer configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlPresentation {
    /// HTTP configuration
    #[serde(default)]
    pub http: Option<YamlHttpConfig>,
    /// gRPC configuration
    #[serde(default)]
    pub grpc: Option<YamlGrpcConfig>,
}

/// HTTP configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlHttpConfig {
    /// API prefix
    #[serde(default)]
    pub prefix: Option<String>,
    /// Route groups
    #[serde(default)]
    pub routes: IndexMap<String, YamlRouteGroup>,
}

/// Route group definition
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlRouteGroup {
    /// Route prefix
    #[serde(default)]
    pub prefix: Option<String>,
    /// Middleware
    #[serde(default)]
    pub middleware: Vec<String>,
    /// Endpoints
    #[serde(default)]
    pub endpoints: Vec<YamlEndpoint>,
}

/// Endpoint definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlEndpoint {
    /// Endpoint name
    pub name: String,
    /// HTTP method
    pub method: String,
    /// Path
    pub path: String,
    /// Use case to invoke
    #[serde(default)]
    pub usecase: Option<String>,
    /// Query parameters
    #[serde(default)]
    pub query_params: Vec<YamlQueryParam>,
    /// Path parameters
    #[serde(default)]
    pub params: Vec<YamlQueryParam>,
    /// Request body type
    #[serde(default)]
    pub body: Option<String>,
    /// Response type
    #[serde(default)]
    pub response: Option<String>,
    /// HTTP status code
    #[serde(default)]
    pub status: Option<u16>,
    /// Authorization requirement
    #[serde(default)]
    pub authorization: Option<serde_yaml::Value>,
    /// Is public endpoint
    #[serde(default)]
    pub public: Option<bool>,
    /// Rate limiting
    #[serde(default)]
    pub rate_limit: Option<YamlRateLimit>,
}

/// Query parameter definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlQueryParam {
    /// Parameter name
    pub name: String,
    /// Parameter type
    #[serde(rename = "type")]
    pub param_type: String,
    /// Default value
    #[serde(default)]
    pub default: Option<serde_yaml::Value>,
    /// Maximum value
    #[serde(default)]
    pub max: Option<i64>,
    /// Is optional
    #[serde(default)]
    pub optional: Option<bool>,
}

/// Rate limit configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlRateLimit {
    /// Maximum requests
    #[serde(default)]
    pub max: Option<u32>,
    /// Time window
    #[serde(default)]
    pub window: Option<String>,
    /// Rate limit key
    #[serde(default)]
    pub key: Option<String>,
}

/// gRPC configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlGrpcConfig {
    /// Package name
    #[serde(default)]
    pub package: Option<String>,
    /// Services
    #[serde(default)]
    pub services: IndexMap<String, YamlGrpcService>,
}

/// gRPC service definition
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlGrpcService {
    /// Service description
    #[serde(default)]
    pub description: Option<String>,
    /// Methods
    #[serde(default)]
    pub methods: Vec<YamlGrpcMethod>,
}

/// gRPC method definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlGrpcMethod {
    /// Method name
    pub name: String,
    /// Input type
    pub input: String,
    /// Output type
    pub output: String,
    /// Authorization
    #[serde(default)]
    pub authorization: Option<serde_yaml::Value>,
    /// Is public
    #[serde(default)]
    pub public: Option<bool>,
}

// =============================================================================
// DTO DEFINITIONS
// =============================================================================

/// DTO definition
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlDto {
    /// DTO description
    #[serde(default)]
    pub description: Option<String>,
    /// Is generic type
    #[serde(default)]
    pub generic: Option<bool>,
    /// Source entity for auto-mapping
    #[serde(default)]
    pub from_entity: Option<String>,
    /// Fields
    #[serde(default)]
    pub fields: Vec<YamlDtoField>,
    /// Fields to exclude
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Computed fields
    #[serde(default)]
    pub computed: Vec<YamlComputedDtoField>,
}

/// DTO field definition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum YamlDtoField {
    /// Simple field name (for from_entity mapping)
    Simple(String),
    /// Full field definition
    Full {
        name: String,
        #[serde(rename = "type")]
        field_type: String,
        #[serde(default)]
        validation: Option<serde_yaml::Value>,
        #[serde(default)]
        optional: Option<bool>,
        #[serde(default)]
        default: Option<serde_yaml::Value>,
    },
}

/// Computed DTO field
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlComputedDtoField {
    /// Field name
    pub name: String,
    /// Field type
    #[serde(rename = "type")]
    pub field_type: String,
    /// Expression
    #[serde(default)]
    pub expression: Option<String>,
}

// =============================================================================
// VERSIONING DEFINITIONS
// =============================================================================

/// API Versioning configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlVersioning {
    /// Versioning strategy
    #[serde(default)]
    pub strategy: Option<String>,
    /// Current version
    #[serde(default)]
    pub current: Option<String>,
    /// Supported versions
    #[serde(default)]
    pub supported: Vec<String>,
    /// Deprecated versions
    #[serde(default)]
    pub deprecated: Vec<String>,
    /// Version definitions
    #[serde(default)]
    pub versions: IndexMap<String, YamlVersionDef>,
    /// Deprecation settings
    #[serde(default)]
    pub deprecation: Option<YamlDeprecationConfig>,
    /// Version negotiation settings
    #[serde(default)]
    pub negotiation: Option<YamlNegotiationConfig>,
}

/// Version definition
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlVersionDef {
    /// Release date
    #[serde(default)]
    pub released: Option<String>,
    /// Version status
    #[serde(default)]
    pub status: Option<String>,
    /// Routes configuration
    #[serde(default)]
    pub routes: Option<YamlVersionRoutes>,
    /// Breaking changes
    #[serde(default)]
    pub breaking_changes: Vec<YamlBreakingChange>,
    /// DTOs for this version
    #[serde(default)]
    pub dtos: IndexMap<String, YamlVersionDto>,
    /// Migrations from previous versions
    #[serde(default)]
    pub migrations: IndexMap<String, YamlVersionMigration>,
}

/// Version routes configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlVersionRoutes {
    /// Route prefix
    #[serde(default)]
    pub prefix: Option<String>,
}

/// Breaking change definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlBreakingChange {
    /// Change type
    #[serde(rename = "type")]
    pub change_type: String,
    /// Affected entity
    #[serde(default)]
    pub entity: Option<String>,
    /// Affected enum
    #[serde(rename = "enum", default)]
    pub enum_name: Option<String>,
    /// Field renamed from
    #[serde(default)]
    pub from: Option<String>,
    /// Field renamed to
    #[serde(default)]
    pub to: Option<String>,
    /// Added field
    #[serde(default)]
    pub field: Option<String>,
    /// Is required
    #[serde(default)]
    pub required: Option<bool>,
    /// Added enum values
    #[serde(default)]
    pub added: Vec<String>,
}

/// Version DTO definition
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlVersionDto {
    /// Fields
    #[serde(default)]
    pub fields: Vec<serde_yaml::Value>,
}

/// Version migration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlVersionMigration {
    /// Field mappings
    #[serde(flatten)]
    pub mappings: IndexMap<String, serde_yaml::Value>,
}

/// Deprecation configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlDeprecationConfig {
    /// Add warning header
    #[serde(default)]
    pub warning_header: Option<bool>,
    /// Add sunset header
    #[serde(default)]
    pub sunset_header: Option<bool>,
    /// Deprecation notices
    #[serde(default)]
    pub notices: IndexMap<String, YamlDeprecationNotice>,
}

/// Deprecation notice
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlDeprecationNotice {
    /// Deprecated date
    #[serde(default)]
    pub deprecated_at: Option<String>,
    /// Sunset date
    #[serde(default)]
    pub sunset_at: Option<String>,
    /// Message
    #[serde(default)]
    pub message: Option<String>,
    /// Migration guide URL
    #[serde(default)]
    pub migration_guide: Option<String>,
}

/// Version negotiation configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlNegotiationConfig {
    /// Default version
    #[serde(default)]
    pub default_version: Option<String>,
    /// Accept header format
    #[serde(default)]
    pub accept_header: Option<YamlAcceptHeader>,
    /// Custom header configuration
    #[serde(default)]
    pub custom_header: Option<YamlCustomHeader>,
}

/// Accept header configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlAcceptHeader {
    /// Format pattern
    #[serde(default)]
    pub format: Option<String>,
}

/// Custom header configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlCustomHeader {
    /// Header name
    #[serde(default)]
    pub name: Option<String>,
}

// =============================================================================
// REPOSITORY TRAIT DEFINITIONS
// =============================================================================

/// Repository trait definition
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YamlRepositoryTrait {
    /// Trait description
    #[serde(default)]
    pub description: Option<String>,
    /// Parent trait to extend
    #[serde(default)]
    pub extends: Option<String>,
    /// Entity type
    #[serde(default)]
    pub entity: Option<String>,
    /// Whether async
    #[serde(rename = "async", default)]
    pub is_async: Option<bool>,
    /// Error type
    #[serde(default)]
    pub error_type: Option<String>,
    /// Auto-generated CRUD methods
    #[serde(default)]
    pub auto_methods: Vec<String>,
    /// Custom query methods
    #[serde(default)]
    pub methods: Vec<YamlTraitMethod>,
}

/// Trait method definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlTraitMethod {
    /// Method name
    pub name: String,
    /// Parameters
    #[serde(default)]
    pub params: Vec<IndexMap<String, String>>,
    /// Return type
    #[serde(default)]
    pub returns: Option<String>,
}

/// Scheduled job definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlScheduledJob {
    /// Schedule expression (cron or interval)
    pub schedule: String,
    /// Handler function
    pub handler: String,
}

/// Root structure for hook YAML files (entity lifecycle behaviors)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlHookSchema {
    /// Hook name
    pub name: String,
    /// Model reference
    pub model: String,
    /// State machine definition
    #[serde(default)]
    pub states: Option<YamlStateMachine>,
    /// Validation rules
    #[serde(default)]
    pub rules: IndexMap<String, YamlRule>,
    /// Permission definitions
    #[serde(default)]
    pub permissions: IndexMap<String, YamlPermission>,
    /// Trigger definitions
    #[serde(default)]
    pub triggers: IndexMap<String, YamlTrigger>,
    /// Computed fields
    #[serde(default)]
    pub computed: IndexMap<String, String>,
}

/// State machine in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlStateMachine {
    /// The field that holds the state
    pub field: String,
    /// State definitions
    #[serde(default)]
    pub values: IndexMap<String, YamlState>,
    /// Transitions
    #[serde(default)]
    pub transitions: IndexMap<String, YamlTransition>,
}

/// A state in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum YamlState {
    /// Simple form: just markers
    Simple(Option<String>),
    /// Full form: with hooks
    Full {
        #[serde(default)]
        initial: Option<bool>,
        #[serde(rename = "final", default)]
        final_state: Option<bool>,
        #[serde(default)]
        on_enter: Vec<YamlAction>,
        #[serde(default)]
        on_exit: Vec<YamlAction>,
    },
}

/// A transition in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlTransition {
    /// Source states
    pub from: YamlStateList,
    /// Target state
    pub to: String,
    /// Allowed roles
    #[serde(default)]
    pub roles: Vec<String>,
    /// Guard condition
    #[serde(default)]
    pub condition: Option<String>,
    /// Error message
    #[serde(default)]
    pub message: Option<String>,
}

/// State list (single or multiple)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum YamlStateList {
    Single(String),
    Multiple(Vec<String>),
}

impl YamlStateList {
    pub fn into_vec(self) -> Vec<String> {
        match self {
            Self::Single(s) => vec![s],
            Self::Multiple(v) => v,
        }
    }
}

/// A rule in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlRule {
    /// When to apply (create, update, delete)
    #[serde(default)]
    pub when: Vec<String>,
    /// Condition expression
    pub condition: String,
    /// Error message
    pub message: String,
    /// Error code
    #[serde(default)]
    pub code: Option<String>,
    /// Severity (error, warning)
    #[serde(default)]
    pub severity: Option<String>,
}

/// Permission definition in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlPermission {
    /// Allowed actions
    #[serde(default)]
    pub allow: Vec<YamlPermissionAction>,
    /// Denied actions
    #[serde(default)]
    pub deny: Vec<YamlPermissionAction>,
}

/// A permission action in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum YamlPermissionAction {
    /// Simple: just the action name
    Simple(String),
    /// Full: action with restrictions
    Full {
        action: String,
        #[serde(default)]
        only: Option<Vec<String>>,
        #[serde(default)]
        except: Option<Vec<String>>,
        #[serde(rename = "if", default)]
        condition: Option<String>,
    },
}

/// A trigger in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlTrigger {
    /// Actions to perform
    #[serde(default)]
    pub actions: Vec<YamlAction>,
    /// Condition
    #[serde(rename = "if", default)]
    pub condition: Option<String>,
}

/// An action in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum YamlAction {
    /// Simple string action
    Simple(String),
    /// Full action with details
    Full {
        #[serde(rename = "type")]
        action_type: String,
        #[serde(flatten)]
        params: IndexMap<String, serde_yaml::Value>,
    },
}

// ============================================================================
// Workflow YAML Structures (Multi-step Business Processes)
// ============================================================================

/// Root structure for workflow YAML files (multi-step business processes)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlWorkflowSchema {
    /// Workflow name
    pub name: String,
    /// Workflow description
    #[serde(default)]
    pub description: Option<String>,
    /// Version number
    #[serde(default = "default_version")]
    pub version: u32,
    /// Workflow trigger
    #[serde(default)]
    pub trigger: Option<YamlWorkflowTrigger>,
    /// Workflow configuration
    #[serde(default)]
    pub config: Option<YamlWorkflowConfig>,
    /// Workflow context variables
    #[serde(default)]
    pub context: IndexMap<String, serde_yaml::Value>,
    /// Workflow steps
    #[serde(default)]
    pub steps: Vec<YamlStep>,
    /// Success handlers
    #[serde(default)]
    pub on_success: Vec<YamlWorkflowHandler>,
    /// Failure handlers
    #[serde(default)]
    pub on_failure: Vec<YamlWorkflowHandler>,
    /// Compensation steps
    #[serde(default)]
    pub compensation: Vec<YamlCompensation>,
}

fn default_version() -> u32 {
    1
}

/// Workflow trigger in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlWorkflowTrigger {
    /// Trigger on event
    #[serde(default)]
    pub event: Option<String>,
    /// Trigger on endpoint
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Trigger on schedule
    #[serde(default)]
    pub schedule: Option<String>,
    /// Extract data from trigger
    #[serde(default)]
    pub extract: IndexMap<String, String>,
}

/// Workflow configuration in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlWorkflowConfig {
    /// Timeout duration
    #[serde(default)]
    pub timeout: Option<String>,
    /// Transaction mode
    #[serde(default)]
    pub transaction_mode: Option<String>,
    /// Retry policy
    #[serde(default)]
    pub retry_policy: Option<YamlRetryPolicy>,
    /// Action on timeout
    #[serde(default)]
    pub on_timeout: Option<String>,
    /// Whether to persist workflow state
    #[serde(default)]
    pub persistence: Option<bool>,
}

/// Retry policy in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlRetryPolicy {
    /// Maximum attempts
    #[serde(default)]
    pub max_attempts: Option<u32>,
    /// Backoff strategy
    #[serde(default)]
    pub backoff: Option<YamlBackoff>,
}

/// Backoff configuration in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum YamlBackoff {
    /// Simple string (e.g., "exponential")
    Simple(String),
    /// Full configuration
    Full {
        #[serde(rename = "type")]
        backoff_type: String,
        #[serde(default)]
        initial: Option<String>,
        #[serde(default)]
        max: Option<String>,
    },
}

/// A workflow step in YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlStep {
    /// Step name
    pub name: String,
    /// Step type
    #[serde(rename = "type", default)]
    pub step_type: Option<String>,
    /// Condition to execute
    #[serde(default)]
    pub condition: Option<String>,
    /// Success handler
    #[serde(default)]
    pub on_success: Option<YamlStepOutcome>,
    /// Failure handler
    #[serde(default)]
    pub on_failure: Option<YamlStepFailure>,

    // Action step fields
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub entity: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub params: Option<IndexMap<String, serde_yaml::Value>>,
    #[serde(default)]
    pub rules: Option<Vec<String>>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub compensation: Option<YamlCompensationAction>,

    // Wait step fields
    #[serde(default)]
    pub wait_for: Option<YamlWaitFor>,
    #[serde(default)]
    pub on_event: Option<YamlStepOutcome>,
    #[serde(default)]
    pub on_timeout: Option<YamlStepOutcome>,

    // Condition step fields
    #[serde(default)]
    pub conditions: Option<Vec<YamlConditionBranch>>,
    /// Default branch for decision steps (when no condition matches)
    #[serde(default)]
    pub default: Option<serde_yaml::Value>,

    // Parallel step fields
    #[serde(default)]
    pub branches: Option<Vec<YamlParallelBranch>>,
    #[serde(default)]
    pub join: Option<String>,
    #[serde(default)]
    pub on_complete: Option<YamlStepOutcome>,

    // Loop step fields
    #[serde(default)]
    pub foreach: Option<String>,
    #[serde(rename = "as", default)]
    pub as_var: Option<String>,
    #[serde(default)]
    pub index_var: Option<String>,
    #[serde(default)]
    pub steps: Option<Vec<YamlStep>>,

    // Subprocess step fields
    #[serde(default)]
    pub flow: Option<String>,
    #[serde(default)]
    pub wait: Option<bool>,

    // Human task step fields
    #[serde(default)]
    pub task: Option<YamlTaskConfig>,

    // Transition step fields
    #[serde(default)]
    pub transition: Option<String>,

    // Terminal step fields
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub emit: Option<YamlEmitConfig>,
    #[serde(default)]
    pub actions: Option<Vec<YamlStep>>,
    #[serde(default)]
    pub compensate: Option<bool>,
}

/// Wait configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlWaitFor {
    /// Event to wait for (single event mode)
    #[serde(default)]
    pub event: Option<String>,
    /// Multiple events to wait for (multi-event mode)
    #[serde(default)]
    pub events: Option<Vec<YamlWaitEvent>>,
    /// Condition (for single event mode)
    #[serde(default)]
    pub condition: Option<String>,
    /// Timeout
    #[serde(default)]
    pub timeout: Option<String>,
}

/// Wait event configuration (for multi-event wait)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlWaitEvent {
    /// Event name
    pub event: String,
    /// Condition for this event
    #[serde(default)]
    pub condition: Option<String>,
    /// Next step when this event is received
    #[serde(default)]
    pub next: Option<String>,
    /// Variables to set when this event is received
    #[serde(default)]
    pub set: Option<IndexMap<String, String>>,
}

/// Step outcome
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlStepOutcome {
    /// Variables to set
    #[serde(default)]
    pub set: Option<IndexMap<String, String>>,
    /// Next step
    #[serde(default)]
    pub next: Option<String>,
    /// Log configuration
    #[serde(default)]
    pub log: Option<YamlLogConfig>,
    /// Action (for timeout/event handlers)
    #[serde(default)]
    pub action: Option<String>,
}

/// Log configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlLogConfig {
    /// Log level
    #[serde(default)]
    pub level: Option<String>,
    /// Log message
    pub message: String,
}

/// Step failure handler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlStepFailure {
    /// Number of retries
    #[serde(default)]
    pub retry: Option<u32>,
    /// Backoff strategy
    #[serde(default)]
    pub backoff: Option<String>,
    /// Handler when exhausted
    #[serde(default)]
    pub on_exhausted: Option<YamlStepOutcome>,
    /// Next step on failure
    #[serde(default)]
    pub next: Option<String>,
}

/// Condition branch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlConditionBranch {
    /// Condition (if)
    #[serde(rename = "if", default)]
    pub condition: Option<String>,
    /// Else branch
    #[serde(rename = "else", default)]
    pub else_branch: Option<bool>,
    /// Next step
    #[serde(default)]
    pub next: Option<String>,
    /// Variables to set
    #[serde(default)]
    pub set: Option<IndexMap<String, String>>,
}

/// Parallel branch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlParallelBranch {
    /// Branch name
    pub name: String,
    /// Condition
    #[serde(default)]
    pub condition: Option<String>,
    /// Steps in branch
    #[serde(default)]
    pub steps: Vec<YamlStep>,
}

/// Task configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlTaskConfig {
    /// Task title
    pub title: String,
    /// Task description
    #[serde(default)]
    pub description: Option<String>,
    /// Assignee
    #[serde(default)]
    pub assignee: Option<String>,
    /// Assignee role
    #[serde(default)]
    pub assignee_role: Option<String>,
    /// Department
    #[serde(default)]
    pub department: Option<String>,
    /// Form fields
    #[serde(default)]
    pub form: Option<YamlTaskForm>,
    /// Timeout
    #[serde(default)]
    pub timeout: Option<String>,
    /// Reminder interval
    #[serde(default)]
    pub reminder: Option<String>,
}

/// Task form
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlTaskForm {
    /// Form fields
    #[serde(default)]
    pub fields: Vec<YamlTaskFormField>,
}

/// Task form field
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlTaskFormField {
    /// Field name
    pub name: String,
    /// Field type
    #[serde(rename = "type")]
    pub field_type: String,
    /// Required flag
    #[serde(default)]
    pub required: Option<bool>,
    /// Default value
    #[serde(default)]
    pub default: Option<serde_yaml::Value>,
    /// Label
    #[serde(default)]
    pub label: Option<String>,
    /// Validation rules
    #[serde(default)]
    pub validation: Option<Vec<String>>,
}

/// Emit configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlEmitConfig {
    /// Event name
    pub event: String,
    /// Event data
    #[serde(default)]
    pub data: Option<IndexMap<String, String>>,
}

/// Compensation action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlCompensationAction {
    /// Action name
    pub action: String,
    /// Parameters
    #[serde(default)]
    pub params: Option<IndexMap<String, String>>,
}

/// Compensation step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlCompensation {
    /// Compensation name
    #[serde(default)]
    pub name: Option<String>,
    /// Condition
    #[serde(default)]
    pub condition: Option<String>,
    /// Action
    #[serde(default)]
    pub action: Option<String>,
    /// Entity
    #[serde(default)]
    pub entity: Option<String>,
    /// Entity ID
    #[serde(default)]
    pub id: Option<String>,
    /// Where clause
    #[serde(rename = "where", default)]
    pub where_clause: Option<String>,
    /// Parameters
    #[serde(default)]
    pub params: Option<IndexMap<String, String>>,
    /// Transition
    #[serde(default)]
    pub transition: Option<String>,
    /// Step type (for loop)
    #[serde(rename = "type", default)]
    pub comp_type: Option<String>,
    /// Loop foreach
    #[serde(default)]
    pub foreach: Option<String>,
    /// Loop variable
    #[serde(rename = "as", default)]
    pub as_var: Option<String>,
    /// Nested steps
    #[serde(default)]
    pub steps: Option<Vec<YamlCompensation>>,
}

/// Workflow handler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlWorkflowHandler {
    /// Emit event
    #[serde(default)]
    pub emit: Option<String>,
    /// Notify target
    #[serde(default)]
    pub notify: Option<String>,
    /// Action
    #[serde(default)]
    pub action: Option<String>,
    /// Message
    #[serde(default)]
    pub message: Option<String>,
    /// Data
    #[serde(default)]
    pub data: Option<IndexMap<String, String>>,
    /// Parameters
    #[serde(default)]
    pub params: Option<IndexMap<String, String>>,
}
