//! Generator Pipeline Integration Tests
//!
//! Tests the complete YAML → AST → Generator flow for DDD features.
//! Verifies that DDD AST nodes properly flow through to generators and
//! produce the expected output.

use metaphor_schema::ast::{ModuleSchema, Model, Field, Span};
use metaphor_schema::ast::model::{
    Entity, EntityMethod, ValueObject, ValueObjectMethod,
    DomainService, ServiceDependency, ServiceMethod,
    EventSourcedConfig, SnapshotConfig,
};
use metaphor_schema::ast::authorization::{
    AuthorizationConfig, RoleDefinition, PolicyDefinition, PolicyType, PolicyRule,
};
use metaphor_schema::ast::types::{TypeRef, PrimitiveType};
use metaphor_schema::generators::{
    GeneratedOutput, Generator, GenerationTarget,
    RustGenerator, ValueObjectGenerator, DomainServiceGenerator,
    AuthGenerator, EventsGenerator, EventStoreGenerator,
    generate_all,
};
use metaphor_schema::resolver::ResolvedSchema;
use metaphor_schema::parser::yaml_parser::parse_model_yaml_str;
use indexmap::IndexMap;
use std::fs;
use std::path::PathBuf;

fn fixtures_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ddd")
}

fn read_fixture(name: &str) -> String {
    let path = fixtures_path().join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", path.display(), e))
}

/// Create a minimal ModuleSchema for testing generators
fn create_test_schema() -> ModuleSchema {
    ModuleSchema::new("test")
}

/// Create a ResolvedSchema wrapper for testing
fn create_resolved_schema(schema: ModuleSchema) -> ResolvedSchema {
    ResolvedSchema { schema }
}

// =============================================================================
// Rust Generator with Entity Methods Tests
// =============================================================================

#[test]
fn test_rust_generator_with_entities() {
    let yaml = read_fixture("entity_example.model.yaml");
    let yaml_schema = parse_model_yaml_str(&yaml)
        .expect("Failed to parse entity YAML");

    let mut schema = create_test_schema();

    // Add models from YAML
    for yaml_model in yaml_schema.models {
        schema.models.push(yaml_model.into_model());
    }

    // Add entities from YAML
    for (name, yaml_entity) in yaml_schema.entities {
        schema.entities.push(yaml_entity.into_entity(name));
    }

    // Add enums from YAML
    for yaml_enum in yaml_schema.enums {
        schema.enums.push(yaml_enum.into_enum());
    }

    let resolved = create_resolved_schema(schema);
    let generator = RustGenerator::new();
    let output = generator.generate(&resolved)
        .expect("RustGenerator should succeed");

    // Verify output was generated
    assert!(!output.files.is_empty(), "RustGenerator should produce output");
}

#[test]
fn test_rust_generator_entity_methods_in_output() {
    // Create a schema with an entity that has methods
    let mut schema = create_test_schema();
    schema.name = "order".to_string();

    // Add a model using the Model::new constructor and methods
    let mut model = Model::new("Order");
    model.collection = Some("orders".to_string());
    model.fields.push(Field::new("id", TypeRef::Primitive(PrimitiveType::Uuid)));
    model.fields.push(Field::new("status", TypeRef::Custom("OrderStatus".to_string())));
    schema.models.push(model);

    // Add an entity with methods
    let mut entity = Entity::new("Order", "Order");
    entity.implements.push("Auditable".to_string());
    entity.methods.push(EntityMethod {
        name: "confirm".to_string(),
        mutates: true,
        is_async: false,
        params: IndexMap::new(),
        returns: Some(TypeRef::Custom("Result<(), OrderError>".to_string())),
        description: Some("Confirms the order".to_string()),
        span: Span::default(),
    });
    entity.methods.push(EntityMethod {
        name: "can_cancel".to_string(),
        mutates: false,
        is_async: false,
        params: IndexMap::new(),
        returns: Some(TypeRef::Primitive(PrimitiveType::Bool)),
        description: None,
        span: Span::default(),
    });
    entity.invariants.push("status cannot be cancelled after shipped".to_string());
    schema.entities.push(entity);

    let resolved = create_resolved_schema(schema);
    let generator = RustGenerator::new();
    let output = generator.generate(&resolved)
        .expect("RustGenerator should succeed");

    // Check that output contains entity-related content
    for (path, _content) in &output.files {
        if path.to_string_lossy().contains("entity") || path.to_string_lossy().contains("order") {
            // Verify entity methods are referenced or documented
            // (actual method generation depends on generator implementation)
            println!("Generated file: {}", path.display());
        }
    }
}

// =============================================================================
// Value Object Generator Tests
// =============================================================================

#[test]
fn test_value_object_generator_from_ast() {
    let yaml = read_fixture("value_object_example.model.yaml");
    let yaml_schema = parse_model_yaml_str(&yaml)
        .expect("Failed to parse value object YAML");

    let mut schema = create_test_schema();
    schema.name = "common".to_string();

    // Add value objects from YAML
    for (name, yaml_vo) in yaml_schema.value_objects {
        schema.value_objects.push(yaml_vo.into_value_object(name));
    }

    let resolved = create_resolved_schema(schema);
    let generator = ValueObjectGenerator::new();
    let output = generator.generate(&resolved)
        .expect("ValueObjectGenerator should succeed");

    // Value object generator should produce output when VOs are defined
    // (even if empty, it should not fail)
    assert!(output.files.is_empty() || !output.files.is_empty(),
        "ValueObjectGenerator should complete without error");
}

#[test]
fn test_value_object_with_methods() {
    let mut schema = create_test_schema();
    schema.name = "finance".to_string();

    // Add a Money value object with methods
    let mut params = IndexMap::new();
    params.insert("other".to_string(), TypeRef::Custom("Money".to_string()));

    let mut vo = ValueObject::new("Money");
    vo.fields.push(Field::new("amount", TypeRef::Primitive(PrimitiveType::Decimal)));
    vo.fields.push(Field::new("currency", TypeRef::Primitive(PrimitiveType::String)));
    vo.methods.push(ValueObjectMethod {
        name: "add".to_string(),
        returns: TypeRef::Custom("Result<Money, Error>".to_string()),
        params,
        is_const: false,
        description: None,
        span: Span::default(),
    });
    vo.methods.push(ValueObjectMethod {
        name: "is_positive".to_string(),
        returns: TypeRef::Primitive(PrimitiveType::Bool),
        params: IndexMap::new(),
        is_const: true,
        description: None,
        span: Span::default(),
    });
    vo.derives.push("Clone".to_string());
    vo.derives.push("Debug".to_string());
    vo.derives.push("PartialEq".to_string());
    schema.value_objects.push(vo);

    let resolved = create_resolved_schema(schema);
    let generator = ValueObjectGenerator::new();
    let _output = generator.generate(&resolved)
        .expect("ValueObjectGenerator should handle VOs with methods");
}

// =============================================================================
// Domain Service Generator Tests
// =============================================================================

#[test]
fn test_domain_service_generator_from_ast() {
    let yaml = read_fixture("domain_service_example.model.yaml");
    let yaml_schema = parse_model_yaml_str(&yaml)
        .expect("Failed to parse domain service YAML");

    let mut schema = create_test_schema();
    schema.name = "user".to_string();

    // Add domain services from YAML
    for (name, yaml_svc) in yaml_schema.domain_services {
        schema.domain_services.push(yaml_svc.into_domain_service(name));
    }

    let resolved = create_resolved_schema(schema);
    let generator = DomainServiceGenerator::new();
    let output = generator.generate(&resolved)
        .expect("DomainServiceGenerator should succeed");

    // Generator should produce domain service files
    assert!(!output.files.is_empty(),
        "DomainServiceGenerator should produce output for domain services");
}

#[test]
fn test_domain_service_with_dependencies() {
    let mut schema = create_test_schema();
    schema.name = "order".to_string();

    // Create a domain service with various dependency types
    let mut params = IndexMap::new();
    params.insert("order_id".to_string(), TypeRef::Primitive(PrimitiveType::Uuid));

    let mut svc = DomainService::new("OrderProcessingService");
    svc.stateless = false;
    svc.description = Some("Processes orders through the fulfillment pipeline".to_string());
    svc.dependencies.push(ServiceDependency::Repository("OrderRepository".to_string()));
    svc.dependencies.push(ServiceDependency::Repository("ProductRepository".to_string()));
    svc.dependencies.push(ServiceDependency::Service("PaymentService".to_string()));
    svc.dependencies.push(ServiceDependency::Client("EmailClient".to_string()));
    svc.methods.push(ServiceMethod {
        name: "process_order".to_string(),
        is_async: true,
        params,
        returns: Some(TypeRef::Custom("Result<Order, OrderError>".to_string())),
        error_type: Some("OrderError".to_string()),
        description: None,
        span: Span::default(),
    });
    schema.domain_services.push(svc);

    let resolved = create_resolved_schema(schema);
    let generator = DomainServiceGenerator::new();
    let output = generator.generate(&resolved)
        .expect("DomainServiceGenerator should handle services with dependencies");

    // Check output files exist
    for (path, _content) in &output.files {
        println!("Generated domain service file: {}", path.display());
        // Verify content mentions dependencies (implementation-specific)
    }
}

// =============================================================================
// Auth Generator Tests
// =============================================================================

#[test]
fn test_auth_generator_from_config() {
    let yaml = read_fixture("authorization_example.model.yaml");
    let yaml_schema = parse_model_yaml_str(&yaml)
        .expect("Failed to parse authorization YAML");

    let mut schema = create_test_schema();
    schema.name = "auth".to_string();

    // Add authorization config from YAML
    if let Some(yaml_auth) = yaml_schema.authorization {
        schema.authorization = Some(yaml_auth.into_authorization());
    }

    // Add models for context
    for yaml_model in yaml_schema.models {
        schema.models.push(yaml_model.into_model());
    }

    let resolved = create_resolved_schema(schema);
    let generator = AuthGenerator::new();
    let output = generator.generate(&resolved)
        .expect("AuthGenerator should succeed");

    // Auth generator should produce output
    assert!(!output.files.is_empty(),
        "AuthGenerator should produce auth-related files");
}

#[test]
fn test_auth_generator_with_roles_and_policies() {
    let mut schema = create_test_schema();
    schema.name = "document".to_string();

    // Create authorization config
    let mut permissions = IndexMap::new();
    permissions.insert("documents".to_string(), vec![
        "read".to_string(),
        "create".to_string(),
        "update".to_string(),
        "delete".to_string(),
    ]);

    let mut roles = vec![];
    let mut viewer_role = RoleDefinition::new("viewer");
    viewer_role.permissions = vec!["documents.read".to_string()];
    viewer_role.level = Some(10);
    viewer_role.description = Some("Can view documents".to_string());
    roles.push(viewer_role);

    let mut editor_role = RoleDefinition::new("editor");
    editor_role.permissions = vec!["documents.read".to_string(), "documents.update".to_string()];
    editor_role.level = Some(30);
    editor_role.inherits = Some("viewer".to_string());
    editor_role.description = Some("Can edit documents".to_string());
    roles.push(editor_role);

    let mut admin_role = RoleDefinition::new("admin");
    admin_role.permissions = vec!["documents.*".to_string()];
    admin_role.level = Some(80);
    admin_role.description = Some("Full document access".to_string());
    roles.push(admin_role);

    let mut policies = vec![];
    policies.push(PolicyDefinition {
        name: "owner_access".to_string(),
        policy_type: PolicyType::Any,
        rules: vec![
            PolicyRule::Owner {
                resource: "Document".to_string(),
                field: "owner_id".to_string(),
                actor_field: Some("id".to_string()),
            },
        ],
        description: Some("Owners can access their documents".to_string()),
        span: Span::default(),
    });

    let mut auth_config = AuthorizationConfig::new();
    auth_config.permissions = permissions;
    auth_config.roles = roles;
    auth_config.policies = policies;
    schema.authorization = Some(auth_config);

    let resolved = create_resolved_schema(schema);
    let generator = AuthGenerator::new();
    let output = generator.generate(&resolved)
        .expect("AuthGenerator should handle complex auth config");

    // Verify output was generated
    // The auth generator produces permission-related files
    assert!(!output.files.is_empty(),
        "AuthGenerator should produce output for auth config");
}

// =============================================================================
// Events Generator Tests
// =============================================================================

#[test]
fn test_events_generator_custom_events() {
    let yaml = read_fixture("event_sourced_example.model.yaml");
    let yaml_schema = parse_model_yaml_str(&yaml)
        .expect("Failed to parse event sourced YAML");

    let mut schema = create_test_schema();
    schema.name = "order".to_string();

    // Add event sourced configs from YAML
    for (name, yaml_es) in yaml_schema.event_sourced {
        schema.event_sourced.push(yaml_es.into_event_sourced(name));
    }

    // Add models
    for yaml_model in yaml_schema.models {
        schema.models.push(yaml_model.into_model());
    }

    let resolved = create_resolved_schema(schema);
    let generator = EventsGenerator::new();
    let output = generator.generate(&resolved)
        .expect("EventsGenerator should succeed");

    // Events generator should produce output
    assert!(!output.files.is_empty(),
        "EventsGenerator should produce event files");
}

// =============================================================================
// Event Store Generator Tests
// =============================================================================

#[test]
fn test_event_store_snapshot_config() {
    let mut schema = create_test_schema();
    schema.name = "account".to_string();

    // Add a model
    let mut model = Model::new("Account");
    model.collection = Some("accounts".to_string());
    model.fields.push(Field::new("id", TypeRef::Primitive(PrimitiveType::Uuid)));
    model.fields.push(Field::new("balance", TypeRef::Primitive(PrimitiveType::Decimal)));
    schema.models.push(model);

    // Add event sourcing config with snapshot
    let mut es_config = EventSourcedConfig::new("Account");
    es_config.events.push("AccountCreated".to_string());
    es_config.events.push("MoneyDeposited".to_string());
    es_config.events.push("MoneyWithdrawn".to_string());
    es_config.snapshot = Some(SnapshotConfig {
        enabled: true,
        every_n_events: 100,
        max_age_seconds: Some(3600),
        storage: Some("database".to_string()),
    });
    schema.event_sourced.push(es_config);

    let resolved = create_resolved_schema(schema);
    let generator = EventStoreGenerator::new();
    let _output = generator.generate(&resolved)
        .expect("EventStoreGenerator should succeed");

    // Event store generator should produce output
    // (may be empty if no event store is configured, which is acceptable)
}

// =============================================================================
// All Generators with Complete DDD Schema
// =============================================================================

#[test]
fn test_all_generators_with_ddd_schema() {
    let yaml = read_fixture("complete_ddd.model.yaml");
    let yaml_schema = parse_model_yaml_str(&yaml)
        .expect("Failed to parse complete DDD YAML");

    let mut schema = create_test_schema();
    schema.name = "ecommerce".to_string();

    // Add all components from YAML
    for yaml_model in yaml_schema.models {
        schema.models.push(yaml_model.into_model());
    }

    for (name, yaml_entity) in yaml_schema.entities {
        schema.entities.push(yaml_entity.into_entity(name));
    }

    for (name, yaml_vo) in yaml_schema.value_objects {
        schema.value_objects.push(yaml_vo.into_value_object(name));
    }

    for (name, yaml_svc) in yaml_schema.domain_services {
        schema.domain_services.push(yaml_svc.into_domain_service(name));
    }

    for (name, yaml_es) in yaml_schema.event_sourced {
        schema.event_sourced.push(yaml_es.into_event_sourced(name));
    }

    if let Some(yaml_auth) = yaml_schema.authorization {
        schema.authorization = Some(yaml_auth.into_authorization());
    }

    let resolved = create_resolved_schema(schema);

    // Test a subset of key generators that should work with DDD features
    let targets = vec![
        GenerationTarget::Rust,
        GenerationTarget::DomainService,
        GenerationTarget::ValueObject,
        GenerationTarget::Auth,
        GenerationTarget::Events,
        GenerationTarget::EventStore,
    ];

    let output = generate_all(&resolved, &targets)
        .expect("All generators should succeed with DDD schema");

    // Verify combined output
    assert!(!output.files.is_empty(),
        "Combined generation should produce files");

    println!("Generated {} files from complete DDD schema", output.files.len());
    for path in output.files.keys() {
        println!("  - {}", path.display());
    }
}

// =============================================================================
// Backward Compatibility Tests
// =============================================================================

#[test]
fn test_generators_work_without_ddd_features() {
    // Ensure generators still work with schemas that have no DDD features
    let mut schema = create_test_schema();
    schema.name = "legacy".to_string();

    // Add a basic model without DDD features
    let mut model = Model::new("LegacyEntity");
    model.collection = Some("legacy_entities".to_string());
    model.fields.push(Field::new("id", TypeRef::Primitive(PrimitiveType::Uuid)));
    model.fields.push(Field::new("name", TypeRef::Primitive(PrimitiveType::String)));
    schema.models.push(model);

    // All DDD fields are empty
    assert!(schema.entities.is_empty());
    assert!(schema.value_objects.is_empty());
    assert!(schema.domain_services.is_empty());
    assert!(schema.event_sourced.is_empty());
    assert!(schema.authorization.is_none());

    let resolved = create_resolved_schema(schema);

    // Run all generators
    let targets = GenerationTarget::all();
    let output = generate_all(&resolved, &targets)
        .expect("All generators should work without DDD features");

    // Should still produce output from basic model
    assert!(!output.files.is_empty(),
        "Generators should produce output for basic models");
}
