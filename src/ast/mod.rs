//! Abstract Syntax Tree definitions for schema files
//!
//! This module defines the AST nodes that represent parsed schema files.
//! The AST is the intermediate representation between parsing and code generation.

pub mod authorization;
pub mod expressions;
pub mod hook;
pub mod model;
pub mod types;
pub mod workflow;

// Re-export main types
pub use model::{
    Attribute, AttributeValue, EnumDef, EnumVariant, Field, ForeignKeyAction, Index, IndexType, Model, Relation,
    RelationType, TypeDef, TypeDefField,
    // DDD Entity & Value Object types
    Entity, EntityMethod, ValueObject, ValueObjectMethod,
    // Domain Service types
    DomainService, ServiceDependency, ServiceMethod,
    // Event Sourcing types
    EventSourcedConfig, SnapshotConfig,
    // Use Case types
    UseCase,
    // Domain Event types
    DomainEvent, EventStorage, EventField,
    // CQRS Projection types
    Projection, ProjectionStorage, SourceEvent, ProjectionField, ProjectionIndex,
    // Application Service types
    AppService, AppServiceMethod,
    // Event Handler types
    Handler, HandlerRetryPolicy, Subscription,
    // Integration/ACL types
    Integration, IntegrationMethod,
    // Presentation layer types
    Presentation, HttpConfig, RouteGroup, Endpoint, GrpcConfig, GrpcService, GrpcMethod,
    // DTO types
    Dto, DtoField, ComputedDtoField,
    // Versioning types
    Versioning,
    // Repository Trait types
    RepositoryTrait, TraitMethod,
};
pub use types::{PrimitiveType, TypeRef};
pub use hook::{
    Action, ActionType, ComputedField, Permission, PermissionAction, Rule, State, StateMachine,
    Transition, Trigger, TriggerEvent, Hook,
};
pub use workflow::{
    Workflow, WorkflowTrigger, WorkflowConfig, WorkflowHandler, TransactionMode, Step, StepType,
    ActionStep, WaitStep, ConditionStep, ParallelStep, LoopStep,
    SubprocessStep, HumanTaskStep, TransitionStep, TerminalStep,
    CompensationStep, RetryPolicy, BackoffStrategy,
};
pub use authorization::{
    AuthorizationConfig, RoleDefinition, PolicyDefinition, PolicyType, PolicyRule,
    ResourcePolicy, ResourcePolicyRule, AbacAttributes, AbacPolicy,
};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::parser::YamlField;

/// Source location information for error reporting
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    /// Start line (1-indexed)
    pub start_line: usize,
    /// Start column (1-indexed)
    pub start_col: usize,
    /// End line (1-indexed)
    pub end_line: usize,
    /// End column (1-indexed)
    pub end_col: usize,
    /// Source file path
    pub file: Option<String>,
}

impl Span {
    pub fn new(start_line: usize, start_col: usize, end_line: usize, end_col: usize) -> Self {
        Self {
            start_line,
            start_col,
            end_line,
            end_col,
            file: None,
        }
    }

    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }
}

/// A complete schema file (model, hook, or workflow)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SchemaFile {
    Model(ModelFile),
    Hook(HookFile),
    Workflow(WorkflowFile),
}

/// Contents of a *.model.schema file
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelFile {
    /// File path
    pub path: Option<String>,
    /// Type definitions (shared types)
    pub type_defs: Vec<TypeDef>,
    /// Enum definitions
    pub enums: Vec<EnumDef>,
    /// Model definitions
    pub models: Vec<Model>,
}

/// Contents of a *.hook.yaml file (entity lifecycle behaviors)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HookFile {
    /// File path
    pub path: Option<String>,
    /// Hook definitions
    pub hooks: Vec<Hook>,
}

/// Contents of a *.workflow.yaml file (multi-step business processes)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowFile {
    /// File path
    pub path: Option<String>,
    /// Workflow definitions
    pub workflows: Vec<Workflow>,
}

/// A complete module schema (all files combined)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModuleSchema {
    /// Module name
    pub name: String,
    /// Generator configuration from index.model.yaml config.generators
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generators_config: Option<crate::parser::yaml_parser::GeneratorsConfig>,
    /// All type definitions
    pub type_defs: Vec<TypeDef>,
    /// All enum definitions
    pub enums: Vec<EnumDef>,
    /// All model definitions
    pub models: Vec<Model>,
    /// All hook definitions (entity lifecycle behaviors)
    pub hooks: Vec<Hook>,
    /// All workflow definitions (multi-step business processes)
    pub workflows: Vec<Workflow>,
    /// Resolved shared types from index.model.yaml
    /// Maps type name -> fields (with compositions already resolved)
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub shared_types: IndexMap<String, IndexMap<String, YamlField>>,

    // ==========================================================================
    // DDD & AUTHORIZATION EXTENSIONS
    // ==========================================================================

    /// DDD Entity definitions (enhanced models with behavior)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<Entity>,
    /// DDD Value Object definitions
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub value_objects: Vec<ValueObject>,
    /// DDD Domain Service definitions
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domain_services: Vec<DomainService>,
    /// Event Sourcing configurations
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_sourced: Vec<EventSourcedConfig>,
    /// Authorization configuration (RBAC/ABAC)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization: Option<AuthorizationConfig>,
    /// DDD Use Case definitions (Application layer operations)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub usecases: Vec<UseCase>,
    /// DDD Domain Event definitions
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<DomainEvent>,

    // ==========================================================================
    // CQRS, APPLICATION LAYER & PRESENTATION EXTENSIONS
    // ==========================================================================

    /// CQRS Projection definitions (read models)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projections: Vec<Projection>,
    /// Application Service definitions
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<AppService>,
    /// Event Handler definitions
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub handlers: Vec<Handler>,
    /// Event Subscription definitions (cross-module)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subscriptions: Vec<Subscription>,
    /// Integration/ACL adapter definitions
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub integrations: Vec<Integration>,
    /// Presentation layer configuration (HTTP/gRPC)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<Presentation>,
    /// DTO definitions (request/response)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dtos: Vec<Dto>,
    /// API Versioning configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub versioning: Option<Versioning>,
    /// Repository trait definitions
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traits: Vec<RepositoryTrait>,
}

impl ModuleSchema {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Merge a model file into this schema
    pub fn merge_model_file(&mut self, file: ModelFile) {
        self.type_defs.extend(file.type_defs);
        self.enums.extend(file.enums);
        self.models.extend(file.models);
    }

    /// Merge a hook file into this schema
    pub fn merge_hook_file(&mut self, file: HookFile) {
        self.hooks.extend(file.hooks);
    }

    /// Merge a workflow file into this schema
    pub fn merge_workflow_file(&mut self, file: WorkflowFile) {
        self.workflows.extend(file.workflows);
    }

    /// Merge DDD extensions from parsed YAML schema
    ///
    /// This method adds DDD-specific data (entities, value objects, domain services,
    /// event sourcing configs, authorization, use cases, and domain events) to the module schema.
    pub fn merge_ddd_extensions(
        &mut self,
        entities: Vec<Entity>,
        value_objects: Vec<ValueObject>,
        domain_services: Vec<DomainService>,
        event_sourced: Vec<EventSourcedConfig>,
        authorization: Option<AuthorizationConfig>,
        usecases: Vec<UseCase>,
        events: Vec<DomainEvent>,
    ) {
        self.entities.extend(entities);
        self.value_objects.extend(value_objects);
        self.domain_services.extend(domain_services);
        self.event_sourced.extend(event_sourced);
        // For authorization, merge or replace (latest wins)
        if authorization.is_some() {
            self.authorization = authorization;
        }
        self.usecases.extend(usecases);
        self.events.extend(events);
    }

    /// Merge CQRS, Application Layer & Presentation extensions from parsed YAML schema
    ///
    /// This method adds projections, services, handlers, subscriptions, integrations,
    /// presentation config, DTOs, versioning, and repository traits to the module schema.
    pub fn merge_cqrs_extensions(
        &mut self,
        projections: Vec<Projection>,
        services: Vec<AppService>,
        handlers: Vec<Handler>,
        subscriptions: Vec<Subscription>,
        integrations: Vec<Integration>,
        presentation: Option<Presentation>,
        dtos: Vec<Dto>,
        versioning: Option<Versioning>,
        traits: Vec<RepositoryTrait>,
    ) {
        self.projections.extend(projections);
        self.services.extend(services);
        self.handlers.extend(handlers);
        self.subscriptions.extend(subscriptions);
        self.integrations.extend(integrations);
        // For presentation, merge or replace (latest wins)
        if presentation.is_some() {
            self.presentation = presentation;
        }
        self.dtos.extend(dtos);
        // For versioning, merge or replace (latest wins)
        if versioning.is_some() {
            self.versioning = versioning;
        }
        self.traits.extend(traits);
    }
}
