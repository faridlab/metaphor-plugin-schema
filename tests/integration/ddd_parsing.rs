//! DDD Parsing Integration Tests
//!
//! Tests the parsing of DDD features from YAML into AST nodes:
//! - Entities with methods and invariants
//! - Value objects (wrapper and composite)
//! - Domain services with dependencies
//! - Event sourcing configuration
//! - Authorization configuration (RBAC/ABAC)

use metaphor_schema::parser::yaml_parser::{
    parse_model_yaml_str, YamlModelSchema,
};
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

// =============================================================================
// Entity Parsing Tests
// =============================================================================

#[test]
fn test_parse_entity_with_methods() {
    let yaml = read_fixture("entity_example.model.yaml");
    let schema: YamlModelSchema = parse_model_yaml_str(&yaml)
        .expect("Failed to parse entity example YAML");

    // Verify model is parsed
    assert_eq!(schema.models.len(), 1);
    assert_eq!(schema.models[0].name, "User");

    // Verify entity is parsed
    assert_eq!(schema.entities.len(), 1);
    let entity = schema.entities.get("User").expect("User entity not found");

    // Check model reference
    assert_eq!(entity.model.as_deref(), Some("User"));

    // Check implements
    assert!(entity.implements.contains(&"Auditable".to_string()));
    assert!(entity.implements.contains(&"SoftDeletable".to_string()));

    // Check value objects mapping
    assert_eq!(entity.value_objects.get("email"), Some(&"Email".to_string()));

    // Check methods
    assert_eq!(entity.methods.len(), 4);

    let verify_method = entity.methods.iter()
        .find(|m| m.name == "verify_email")
        .expect("verify_email method not found");
    assert!(verify_method.mutates.unwrap_or(false));
    assert_eq!(verify_method.returns.as_deref(), Some("Result<(), UserError>"));

    let can_login = entity.methods.iter()
        .find(|m| m.name == "can_login")
        .expect("can_login method not found");
    assert!(!can_login.mutates.unwrap_or(false));
    assert_eq!(can_login.returns.as_deref(), Some("bool"));

    let suspend = entity.methods.iter()
        .find(|m| m.name == "suspend")
        .expect("suspend method not found");
    assert!(suspend.mutates.unwrap_or(false));
    assert!(suspend.is_async.unwrap_or(false));

    let reactivate = entity.methods.iter()
        .find(|m| m.name == "reactivate")
        .expect("reactivate method not found");
    assert!(reactivate.params.contains_key("reason"));

    // Check invariants
    assert_eq!(entity.invariants.len(), 3);
    assert!(entity.invariants.iter().any(|i| i.contains("email must be unique")));
}

#[test]
fn test_entity_converts_to_ast() {
    let yaml = read_fixture("entity_example.model.yaml");
    let schema: YamlModelSchema = parse_model_yaml_str(&yaml)
        .expect("Failed to parse entity example YAML");

    let yaml_entity = schema.entities.get("User").expect("User entity not found");
    let entity = yaml_entity.clone().into_entity("User".to_string());

    assert_eq!(entity.name, "User");
    assert_eq!(entity.model_ref, "User");
    assert_eq!(entity.implements.len(), 2);
    assert_eq!(entity.methods.len(), 4);
    assert_eq!(entity.invariants.len(), 3);
}

// =============================================================================
// Value Object Parsing Tests
// =============================================================================

#[test]
fn test_parse_value_object_wrapper() {
    let yaml = read_fixture("value_object_example.model.yaml");
    let schema: YamlModelSchema = parse_model_yaml_str(&yaml)
        .expect("Failed to parse value object example YAML");

    // Check Email (wrapper type)
    let email = schema.value_objects.get("Email")
        .expect("Email value object not found");

    assert_eq!(email.inner_type.as_deref(), Some("String"));
    assert_eq!(email.validation.as_deref(), Some("email_format"));
    assert_eq!(email.methods.len(), 2);

    let domain_method = email.methods.iter()
        .find(|m| m.name == "domain")
        .expect("domain method not found");
    assert_eq!(domain_method.returns.as_deref(), Some("&str"));
}

#[test]
fn test_parse_value_object_composite() {
    let yaml = read_fixture("value_object_example.model.yaml");
    let schema: YamlModelSchema = parse_model_yaml_str(&yaml)
        .expect("Failed to parse value object example YAML");

    // Check Money (composite type)
    let money = schema.value_objects.get("Money")
        .expect("Money value object not found");

    // Should have fields
    assert!(money.fields.len() >= 2);
    assert!(money.fields.contains_key("amount"));
    assert!(money.fields.contains_key("currency"));

    // Check methods
    assert!(money.methods.len() >= 4);
    let add_method = money.methods.iter()
        .find(|m| m.name == "add")
        .expect("add method not found");
    assert!(add_method.params.contains_key("other"));

    let is_zero = money.methods.iter()
        .find(|m| m.name == "is_zero")
        .expect("is_zero method not found");
    assert!(is_zero.is_const.unwrap_or(false));

    // Check derives
    assert!(money.derives.contains(&"Clone".to_string()));
    assert!(money.derives.contains(&"Debug".to_string()));

    // Check custom messages
    assert!(money.messages.contains_key("amount_positive"));
}

#[test]
fn test_value_object_converts_to_ast() {
    let yaml = read_fixture("value_object_example.model.yaml");
    let schema: YamlModelSchema = parse_model_yaml_str(&yaml)
        .expect("Failed to parse value object example YAML");

    let yaml_vo = schema.value_objects.get("Money").expect("Money not found");
    let vo = yaml_vo.clone().into_value_object("Money".to_string());

    assert_eq!(vo.name, "Money");
    assert!(vo.fields.len() >= 2);
    assert!(vo.methods.len() >= 4);
}

// =============================================================================
// Domain Service Parsing Tests
// =============================================================================

#[test]
fn test_parse_domain_service() {
    let yaml = read_fixture("domain_service_example.model.yaml");
    let schema: YamlModelSchema = parse_model_yaml_str(&yaml)
        .expect("Failed to parse domain service example YAML");

    assert_eq!(schema.domain_services.len(), 4);

    // Check PasswordService (stateless)
    let password_svc = schema.domain_services.get("PasswordService")
        .expect("PasswordService not found");
    assert!(password_svc.stateless.unwrap_or(false));
    assert_eq!(password_svc.methods.len(), 3);

    let hash_method = password_svc.methods.iter()
        .find(|m| m.name == "hash")
        .expect("hash method not found");
    assert!(hash_method.is_async.unwrap_or(false));

    // Check UserRegistrationService (with dependencies)
    let reg_svc = schema.domain_services.get("UserRegistrationService")
        .expect("UserRegistrationService not found");
    assert!(!reg_svc.stateless.unwrap_or(true));

    // Check dependencies exist (dependencies are Simple(String) or Full variants)
    assert!(reg_svc.dependencies.len() >= 4);

    // Check PaymentService dependencies exist
    let payment_svc = schema.domain_services.get("PaymentService")
        .expect("PaymentService not found");
    assert!(payment_svc.dependencies.len() >= 4);
}

#[test]
fn test_domain_service_converts_to_ast() {
    let yaml = read_fixture("domain_service_example.model.yaml");
    let schema: YamlModelSchema = parse_model_yaml_str(&yaml)
        .expect("Failed to parse domain service example YAML");

    let yaml_svc = schema.domain_services.get("OrderService").expect("OrderService not found");
    let svc = yaml_svc.clone().into_domain_service("OrderService".to_string());

    assert_eq!(svc.name, "OrderService");
    assert!(!svc.stateless);
    assert!(svc.dependencies.len() >= 4);
    assert!(svc.methods.len() >= 3);
}

// =============================================================================
// Event Sourcing Parsing Tests
// =============================================================================

#[test]
fn test_parse_event_sourced_config() {
    let yaml = read_fixture("event_sourced_example.model.yaml");
    let schema: YamlModelSchema = parse_model_yaml_str(&yaml)
        .expect("Failed to parse event sourced example YAML");

    assert_eq!(schema.event_sourced.len(), 3);

    // Check Order event sourcing
    let order_es = schema.event_sourced.get("Order")
        .expect("Order event sourced config not found");
    assert!(order_es.events.len() >= 8);
    assert!(order_es.events.contains(&"OrderPlaced".to_string()));
    assert!(order_es.events.contains(&"OrderConfirmed".to_string()));

    // Check snapshot config
    let snapshot = order_es.snapshot.as_ref()
        .expect("Order should have snapshot config");
    assert!(snapshot.enabled.unwrap_or(false));
    assert_eq!(snapshot.every_n_events, Some(50));

    // Check handlers
    assert!(order_es.handlers.len() >= 3);
    assert_eq!(order_es.handlers.get("OrderPlaced"), Some(&"handle_order_placed".to_string()));

    // Check Account (with more config)
    let account_es = schema.event_sourced.get("Account")
        .expect("Account event sourced config not found");
    let account_snap = account_es.snapshot.as_ref().expect("Account should have snapshot");
    assert_eq!(account_snap.every_n_events, Some(100));
    assert_eq!(account_snap.max_age_seconds, Some(3600));
    assert_eq!(account_snap.storage.as_deref(), Some("database"));

    // Check AuditLog (no snapshot)
    let audit_es = schema.event_sourced.get("AuditLog")
        .expect("AuditLog event sourced config not found");
    let audit_snap = audit_es.snapshot.as_ref().expect("AuditLog should have snapshot");
    assert!(!audit_snap.enabled.unwrap_or(true));
}

#[test]
fn test_event_sourced_converts_to_ast() {
    let yaml = read_fixture("event_sourced_example.model.yaml");
    let schema: YamlModelSchema = parse_model_yaml_str(&yaml)
        .expect("Failed to parse event sourced example YAML");

    let yaml_es = schema.event_sourced.get("Order").expect("Order ES not found");
    let es = yaml_es.clone().into_event_sourced("Order".to_string());

    assert_eq!(es.entity_name, "Order");
    assert!(es.events.len() >= 8);
    assert!(es.snapshot.is_some());
    assert!(es.snapshot.as_ref().unwrap().enabled);
}

// =============================================================================
// Authorization Parsing Tests
// =============================================================================

#[test]
fn test_parse_authorization_config() {
    let yaml = read_fixture("authorization_example.model.yaml");
    let schema: YamlModelSchema = parse_model_yaml_str(&yaml)
        .expect("Failed to parse authorization example YAML");

    let auth = schema.authorization.as_ref()
        .expect("Authorization config not found");

    // Check permissions
    assert!(auth.permissions.len() >= 4);
    let user_perms = auth.permissions.get("users").expect("users permissions not found");
    assert!(user_perms.contains(&"read".to_string()));
    assert!(user_perms.contains(&"create".to_string()));
    assert!(user_perms.contains(&"delete".to_string()));

    let doc_perms = auth.permissions.get("documents").expect("documents permissions not found");
    assert!(doc_perms.contains(&"share".to_string()));
    assert!(doc_perms.contains(&"archive".to_string()));

    // Check roles
    assert!(auth.roles.len() >= 6);

    let guest = auth.roles.get("guest").expect("guest role not found");
    assert_eq!(guest.level, Some(0));

    let user = auth.roles.get("user").expect("user role not found");
    assert_eq!(user.level, Some(10));
    assert_eq!(user.inherits.as_deref(), Some("guest"));

    let admin = auth.roles.get("admin").expect("admin role not found");
    assert_eq!(admin.level, Some(80));
    assert!(admin.permissions.iter().any(|p| p.contains("*")));

    let superadmin = auth.roles.get("superadmin").expect("superadmin role not found");
    assert_eq!(superadmin.level, Some(100));
    assert!(superadmin.permissions.contains(&"*".to_string()));

    // Check policies
    assert!(auth.policies.len() >= 5);

    let owner_policy = auth.policies.get("document_owner")
        .expect("document_owner policy not found");
    assert_eq!(owner_policy.policy_type.as_deref(), Some("any"));
    assert!(owner_policy.rules.len() >= 1);
}

#[test]
fn test_authorization_converts_to_ast() {
    let yaml = read_fixture("authorization_example.model.yaml");
    let schema: YamlModelSchema = parse_model_yaml_str(&yaml)
        .expect("Failed to parse authorization example YAML");

    let yaml_auth = schema.authorization.as_ref().expect("Authorization not found");
    let auth = yaml_auth.clone().into_authorization();

    assert!(auth.permissions.len() >= 4);
    assert!(auth.roles.len() >= 6);
    assert!(auth.policies.len() >= 5);
}

// =============================================================================
// Complete DDD Schema Test
// =============================================================================

#[test]
fn test_parse_complete_ddd_schema() {
    let yaml = read_fixture("complete_ddd.model.yaml");
    let schema: YamlModelSchema = parse_model_yaml_str(&yaml)
        .expect("Failed to parse complete DDD example YAML");

    // Verify all DDD features are present
    assert!(schema.models.len() >= 2, "Should have at least 2 models");
    assert!(schema.enums.len() >= 2, "Should have at least 2 enums");
    assert!(schema.value_objects.len() >= 3, "Should have at least 3 value objects");
    assert!(schema.entities.len() >= 2, "Should have at least 2 entities");
    assert!(schema.domain_services.len() >= 2, "Should have at least 2 domain services");
    assert!(schema.event_sourced.len() >= 2, "Should have at least 2 event sourced configs");
    assert!(schema.authorization.is_some(), "Should have authorization config");

    // Verify Order entity links correctly
    let order_entity = schema.entities.get("Order").expect("Order entity not found");
    assert_eq!(order_entity.model.as_deref(), Some("Order"));
    assert!(order_entity.value_objects.contains_key("total"));
    assert!(order_entity.value_objects.contains_key("shipping_address"));

    // Verify domain service has dependencies
    let order_svc = schema.domain_services.get("OrderService").expect("OrderService not found");
    assert!(order_svc.dependencies.len() >= 2, "OrderService should have dependencies");
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[test]
fn test_empty_ddd_fields_dont_fail() {
    // A minimal schema without DDD features should still parse
    let yaml = r#"
models:
  - name: SimpleModel
    collection: simple_models
    fields:
      id:
        type: uuid
        primary_key: true
      name:
        type: string
"#;

    let schema: YamlModelSchema = parse_model_yaml_str(yaml)
        .expect("Failed to parse minimal YAML");

    assert_eq!(schema.models.len(), 1);
    assert!(schema.entities.is_empty());
    assert!(schema.value_objects.is_empty());
    assert!(schema.domain_services.is_empty());
    assert!(schema.event_sourced.is_empty());
    assert!(schema.authorization.is_none());
}

#[test]
fn test_partial_ddd_features() {
    // Schema with only some DDD features
    let yaml = r#"
models:
  - name: Product
    collection: products
    fields:
      id:
        type: uuid
        primary_key: true
      name:
        type: string

value_objects:
  Price:
    inner_type: decimal
    validation: positive_amount
"#;

    let schema: YamlModelSchema = parse_model_yaml_str(yaml)
        .expect("Failed to parse partial DDD YAML");

    assert_eq!(schema.models.len(), 1);
    assert_eq!(schema.value_objects.len(), 1);
    assert!(schema.entities.is_empty());
    assert!(schema.domain_services.is_empty());
}
