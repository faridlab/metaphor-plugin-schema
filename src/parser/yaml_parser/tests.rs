use super::*;
use super::helpers::{parse_type_string, split_attr_args, find_colon_outside_quotes};
use crate::ast::types::{TypeRef, PrimitiveType};
use indexmap::IndexMap;

    #[test]
    fn test_split_attr_args_with_json_content() {
        // Test that JSON content with colons is not split incorrectly
        // Use actual escaped double quotes to represent the input correctly
        let input = "\'{\"regular\":0,\"express\":0,\"kilat\":0,\"sihir\":0}\'";
        let result = split_attr_args(input);
        assert_eq!(result.len(), 1, "Should parse as single argument");
        assert!(result[0].contains("\"regular\":0"), "Should contain full JSON");
        assert!(result[0].contains("\"kilat\":0"), "Should contain kilat key");
    }

    #[test]
    fn test_split_attr_args_multiple_args() {
        // Test normal argument splitting with multiple arguments
        let input = "'arg1', 'arg2', true";
        let result = split_attr_args(input);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], "'arg1'");
        assert_eq!(result[1], "'arg2'");
        assert_eq!(result[2], "true");
    }

    #[test]
    fn test_find_colon_outside_quotes_json() {
        // Test that colons inside JSON strings are NOT detected
        let json_arg = "\'{\"regular\":0,\"express\":0}\'";
        let result = find_colon_outside_quotes(json_arg);
        assert_eq!(result, None, "Should not find colon inside JSON");

        // Test that colons in named arguments ARE detected
        let named_arg = "format:json";
        let result = find_colon_outside_quotes(named_arg);
        assert_eq!(result, Some(6), "Should find colon in named arg");
    }

    #[test]
    fn test_find_colon_outside_quotes_nested() {
        // Test quote tracking with nested quotes
        // Input: "nes:ted":"value"
        // Position 4 has colon INSIDE quotes (should not be found)
        // Position 9 has colon AFTER closing quote (should be found)
        let input2 = "\"nes:ted\":\"value\"";
        let result2 = find_colon_outside_quotes(input2);
        // Should find the colon at position 9 (after closing quote), not at position 4
        assert_eq!(result2, Some(9), "Should find colon outside quoted strings, not inside");
    }

    #[test]
    fn test_parse_simple_model() {
        let yaml = r#"
models:
  - name: User
    collection: users
    fields:
      id: uuid
      email: email
      username: string
"#;

        let schema = parse_model_yaml_str(yaml).unwrap();
        assert_eq!(schema.models.len(), 1);
        assert_eq!(schema.models[0].name, "User");
        assert_eq!(schema.models[0].fields.len(), 3);
    }

    #[test]
    fn test_parse_file_level_types_basic() {
        // Test that file-level types can be parsed
        let yaml = r#"
types:
  - name: Address
    fields:
      street: string
      city: string
"#;
        let result = parse_model_yaml_str(yaml);
        if let Err(_e) = &result {
            // Test intentionally ignores parsing errors
        }
        let schema = result.unwrap();
        assert_eq!(schema.types.len(), 1);
        assert_eq!(schema.types[0].name, "Address");
        assert_eq!(schema.types[0].fields.len(), 2);
    }

    #[test]
    fn test_parse_field_with_attributes() {
        let yaml = r#"
models:
  - name: User
    fields:
      email:
        type: string
        attributes: ["@unique", "@required", "@email"]
        description: "User's email address"
"#;

        let schema = parse_model_yaml_str(yaml).unwrap();
        let model = schema.into_models().remove(0);
        let email_field = model.find_field("email").unwrap();

        assert!(email_field.has_attribute("unique"));
        assert!(email_field.has_attribute("required"));
        assert!(email_field.has_attribute("email"));
        assert!(email_field.has_attribute("description"));
    }

    #[test]
    fn test_parse_type_string() {
        let (t, _) = parse_type_string("uuid");
        assert!(matches!(t, TypeRef::Primitive(PrimitiveType::Uuid)));

        let (t, _) = parse_type_string("string?");
        assert!(matches!(t, TypeRef::Optional(_)));

        let (t, _) = parse_type_string("string[]");
        assert!(matches!(t, TypeRef::Array(_)));

        let (t, attrs) = parse_type_string("email");
        assert!(matches!(t, TypeRef::Primitive(PrimitiveType::Email)));
        assert!(attrs.iter().any(|a| a.name == "email"));
    }

    #[test]
    fn test_parse_model_index_with_shared_types() {
        let yaml = r#"
module: sapiens
version: 2

shared_types:
  Timestamps:
    created_at:
      type: datetime
      attributes: ["@default(now)"]
    updated_at:
      type: datetime
      attributes: ["@updated_at"]
    deleted_at: datetime?

  Actors:
    created_by:
      type: uuid?
      attributes: ["@foreign_key(User.id)"]
    updated_by:
      type: uuid?
      attributes: ["@foreign_key(User.id)"]
    deleted_by:
      type: uuid?
      attributes: ["@foreign_key(User.id)"]

  AuditLog: [Timestamps, Actors]

imports:
  - user.model.yaml
  - role.model.yaml
"#;

        let result = parse_model_yaml_flexible(yaml).unwrap();
        match result {
            YamlModelParseResult::Index(index) => {
                assert_eq!(index.module, Some("sapiens".to_string()));
                assert_eq!(index.version, Some(2));
                assert_eq!(index.shared_types.len(), 3);
                assert_eq!(index.imports.len(), 2);

                // Check that Timestamps has fields
                let timestamps = index.shared_types.get("Timestamps").unwrap();
                assert!(matches!(timestamps, YamlSharedType::Fields(_)));

                // Check that Actors has fields
                let actors = index.shared_types.get("Actors").unwrap();
                assert!(matches!(actors, YamlSharedType::Fields(_)));

                // Check that AuditLog is a composition
                let audit_log = index.shared_types.get("AuditLog").unwrap();
                match audit_log {
                    YamlSharedType::Composition(types) => {
                        assert_eq!(types.len(), 2);
                        assert_eq!(types[0], "Timestamps");
                        assert_eq!(types[1], "Actors");
                    }
                    _ => panic!("Expected AuditLog to be a Composition"),
                }
            }
            _ => panic!("Expected Index result"),
        }
    }

    #[test]
    fn test_resolve_shared_types_composition() {
        let yaml = r#"
module: test
shared_types:
  Timestamps:
    created_at: datetime
    updated_at: datetime
  Actors:
    created_by: uuid?
    updated_by: uuid?
  AuditLog: [Timestamps, Actors]
"#;

        let index: YamlModelIndexSchema = serde_yaml::from_str(yaml).unwrap();
        let resolved = resolve_shared_types(&index.shared_types);

        // Check that AuditLog has all fields from Timestamps and Actors merged
        let audit_log = resolved.get("AuditLog").unwrap();
        assert_eq!(audit_log.len(), 4);
        assert!(audit_log.contains_key("created_at"));
        assert!(audit_log.contains_key("updated_at"));
        assert!(audit_log.contains_key("created_by"));
        assert!(audit_log.contains_key("updated_by"));
    }

    #[test]
    fn test_model_extends_shared_type() {
        let yaml = r#"
models:
  - name: User
    collection: users
    extends: [Timestamps]
    fields:
      id: uuid
      email: email
"#;

        let schema = parse_model_yaml_str(yaml).unwrap();
        assert_eq!(schema.models.len(), 1);
        assert_eq!(schema.models[0].extends.len(), 1);
        assert_eq!(schema.models[0].extends[0], "Timestamps");

        // Create shared types context
        let mut shared_types: IndexMap<String, IndexMap<String, YamlField>> = IndexMap::new();
        let mut timestamps_fields: IndexMap<String, YamlField> = IndexMap::new();
        timestamps_fields.insert("created_at".to_string(), YamlField::Simple("datetime".to_string()));
        timestamps_fields.insert("updated_at".to_string(), YamlField::Simple("datetime".to_string()));
        shared_types.insert("Timestamps".to_string(), timestamps_fields);

        // Convert with context - should inject extended fields
        let model = schema.models.into_iter().next().unwrap()
            .into_model_with_context(&shared_types, &IndexMap::new());

        // Check that extended fields are present
        assert!(model.find_field("created_at").is_some());
        assert!(model.find_field("updated_at").is_some());
        assert!(model.find_field("id").is_some());
        assert!(model.find_field("email").is_some());

        // Check that inherited attribute is added
        let created_at = model.find_field("created_at").unwrap();
        assert!(created_at.has_attribute("inherited"));
    }

    #[test]
    fn test_model_local_types() {
        let yaml = r#"
models:
  - name: Order
    collection: orders
    types:
      Address:
        street: string
        city: string
        zip: string
      ContactInfo:
        email: email
        phone: phone?
    fields:
      id: uuid
      shipping_address: Address
      billing_address: Address?
"#;

        let schema = parse_model_yaml_str(yaml).unwrap();
        let model = &schema.models[0];

        // Check local types are parsed
        assert_eq!(model.types.len(), 2);
        assert!(model.types.contains_key("Address"));
        assert!(model.types.contains_key("ContactInfo"));

        // Check Address type fields
        let address_type = model.types.get("Address").unwrap();
        assert_eq!(address_type.len(), 3);
        assert!(address_type.contains_key("street"));
        assert!(address_type.contains_key("city"));
        assert!(address_type.contains_key("zip"));
    }

    #[test]
    fn test_field_type_as_jsonb() {
        let yaml = r#"
models:
  - name: User
    collection: users
    fields:
      id: uuid
      metadata: Metadata
"#;

        let schema = parse_model_yaml_str(yaml).unwrap();

        // Create shared types context with Metadata type
        let mut shared_types: IndexMap<String, IndexMap<String, YamlField>> = IndexMap::new();
        let mut metadata_fields: IndexMap<String, YamlField> = IndexMap::new();
        metadata_fields.insert("created_at".to_string(), YamlField::Full {
            field_type: "datetime".to_string(),
            attributes: vec!["@default(now)".to_string()],
            description: None,
        });
        metadata_fields.insert("updated_at".to_string(), YamlField::Simple("datetime".to_string()));
        shared_types.insert("Metadata".to_string(), metadata_fields);

        // Convert with context - metadata field should become JSONB
        let model = schema.models.into_iter().next().unwrap()
            .into_model_with_context(&shared_types, &IndexMap::new());

        // Check that metadata is now a JSONB field
        let metadata_field = model.find_field("metadata").unwrap();
        assert!(matches!(metadata_field.type_ref, TypeRef::Primitive(PrimitiveType::Json)));

        // Check that jsonb_type attribute is present
        assert!(metadata_field.has_attribute("jsonb_type"));

        // Check that jsonb_schema attribute contains field definitions
        assert!(metadata_field.has_attribute("jsonb_schema"));
    }

    #[test]
    fn test_yaml_field_get_type_name() {
        // Primitive type - should return None
        let field = YamlField::Simple("string".to_string());
        assert!(field.get_type_name().is_none());

        // Custom type - should return the type name
        let field = YamlField::Simple("Metadata".to_string());
        assert_eq!(field.get_type_name(), Some("Metadata".to_string()));

        // Optional custom type - should return the base type name
        let field = YamlField::Simple("Metadata?".to_string());
        assert_eq!(field.get_type_name(), Some("Metadata".to_string()));

        // Array custom type - should return the base type name
        let field = YamlField::Simple("Metadata[]".to_string());
        assert_eq!(field.get_type_name(), Some("Metadata".to_string()));

        // Full form
        let field = YamlField::Full {
            field_type: "Address".to_string(),
            attributes: vec![],
            description: None,
        };
        assert_eq!(field.get_type_name(), Some("Address".to_string()));
    }

    #[test]
    fn test_extends_with_explicit_override() {
        let yaml = r#"
models:
  - name: User
    extends: [Timestamps]
    fields:
      id: uuid
      created_at:
        type: datetime
        attributes: ["@default(now)", "@immutable"]
"#;

        let schema = parse_model_yaml_str(yaml).unwrap();

        // Create shared types context
        let mut shared_types: IndexMap<String, IndexMap<String, YamlField>> = IndexMap::new();
        let mut timestamps_fields: IndexMap<String, YamlField> = IndexMap::new();
        timestamps_fields.insert("created_at".to_string(), YamlField::Simple("datetime".to_string()));
        timestamps_fields.insert("updated_at".to_string(), YamlField::Simple("datetime".to_string()));
        shared_types.insert("Timestamps".to_string(), timestamps_fields);

        let model = schema.models.into_iter().next().unwrap()
            .into_model_with_context(&shared_types, &IndexMap::new());

        // Check that explicit created_at takes precedence over inherited one
        let created_at = model.find_field("created_at").unwrap();
        assert!(created_at.has_attribute("immutable"));
        // Explicit field should NOT have inherited attribute
        assert!(!created_at.has_attribute("inherited"));

        // updated_at should be inherited
        let updated_at = model.find_field("updated_at").unwrap();
        assert!(updated_at.has_attribute("inherited"));
    }

    #[test]
    fn test_file_level_types_reusable_across_models() {
        // File-level types defined alongside models and enums
        // can be reused across multiple models in the same file
        let yaml = r#"
# File-level types (same level as models and enums)
types:
  - name: Address
    fields:
      street: string
      city: string
      zip: string
      country: string

  - name: ContactInfo
    fields:
      email: email
      phone: phone?

models:
  - name: Customer
    collection: customers
    fields:
      id: uuid
      name: string
      billing_address: Address
      shipping_address: Address
      contact: ContactInfo

  - name: Supplier
    collection: suppliers
    fields:
      id: uuid
      company_name: string
      headquarters: Address
      contact: ContactInfo

enums:
  - name: CustomerStatus
    variants: [active, inactive]
"#;

        let schema = parse_model_yaml_str(yaml).unwrap();

        // Check file-level types are parsed
        assert_eq!(schema.types.len(), 2);
        assert_eq!(schema.types[0].name, "Address");
        assert_eq!(schema.types[1].name, "ContactInfo");

        // Check enums are still parsed
        assert_eq!(schema.enums.len(), 1);
        assert_eq!(schema.enums[0].name, "CustomerStatus");

        // Convert models - file-level types should be available to all models
        let models = schema.into_models();
        assert_eq!(models.len(), 2);

        // Customer model should have Address and ContactInfo as JSONB fields
        let customer = &models[0];
        assert_eq!(customer.name, "Customer");

        let billing_address = customer.find_field("billing_address").unwrap();
        assert!(matches!(billing_address.type_ref, TypeRef::Primitive(PrimitiveType::Json)));
        assert!(billing_address.has_attribute("jsonb_type"));

        let shipping_address = customer.find_field("shipping_address").unwrap();
        assert!(matches!(shipping_address.type_ref, TypeRef::Primitive(PrimitiveType::Json)));

        let contact = customer.find_field("contact").unwrap();
        assert!(matches!(contact.type_ref, TypeRef::Primitive(PrimitiveType::Json)));

        // Supplier model should also have access to the same types
        let supplier = &models[1];
        assert_eq!(supplier.name, "Supplier");

        let headquarters = supplier.find_field("headquarters").unwrap();
        assert!(matches!(headquarters.type_ref, TypeRef::Primitive(PrimitiveType::Json)));
        assert!(headquarters.has_attribute("jsonb_type"));

        let supplier_contact = supplier.find_field("contact").unwrap();
        assert!(matches!(supplier_contact.type_ref, TypeRef::Primitive(PrimitiveType::Json)));
    }

    #[test]
    fn test_file_level_types_with_shared_types_context() {
        // File-level types work together with module-level shared types
        let yaml = r#"
types:
  - name: LocalAddress
    fields:
      street: string
      city: string

models:
  - name: Store
    collection: stores
    extends: [Timestamps]
    fields:
      id: uuid
      name: string
      location: LocalAddress
      metadata: Metadata
"#;

        let schema = parse_model_yaml_str(yaml).unwrap();

        // Create module-level shared types (from index.model.yaml)
        let mut shared_types: IndexMap<String, IndexMap<String, YamlField>> = IndexMap::new();

        // Timestamps shared type
        let mut timestamps_fields: IndexMap<String, YamlField> = IndexMap::new();
        timestamps_fields.insert("created_at".to_string(), YamlField::Simple("datetime".to_string()));
        timestamps_fields.insert("updated_at".to_string(), YamlField::Simple("datetime".to_string()));
        shared_types.insert("Timestamps".to_string(), timestamps_fields);

        // Metadata shared type
        let mut metadata_fields: IndexMap<String, YamlField> = IndexMap::new();
        metadata_fields.insert("version".to_string(), YamlField::Simple("int".to_string()));
        metadata_fields.insert("tags".to_string(), YamlField::Simple("string[]".to_string()));
        shared_types.insert("Metadata".to_string(), metadata_fields);

        // Convert with shared types context
        let models = schema.into_models_with_context(&shared_types);
        let store = &models[0];

        // Timestamps should be extended (inherited as columns)
        let created_at = store.find_field("created_at").unwrap();
        assert!(created_at.has_attribute("inherited"));

        let updated_at = store.find_field("updated_at").unwrap();
        assert!(updated_at.has_attribute("inherited"));

        // LocalAddress (file-level type) should become JSONB
        let location = store.find_field("location").unwrap();
        assert!(matches!(location.type_ref, TypeRef::Primitive(PrimitiveType::Json)));
        assert!(location.has_attribute("jsonb_type"));

        // Metadata (shared type) should also become JSONB
        let metadata = store.find_field("metadata").unwrap();
        assert!(matches!(metadata.type_ref, TypeRef::Primitive(PrimitiveType::Json)));
        assert!(metadata.has_attribute("jsonb_type"));
    }

    // ==========================================================================
    // DDD & AUTHORIZATION PARSING TESTS
    // ==========================================================================

    #[test]
    fn test_parse_entities() {
        let yaml = r#"
models:
  - name: User
    collection: users
    fields:
      id: uuid
      email: string

entities:
  User:
    model: User
    implements:
      - Auditable
      - SoftDeletable
    value_objects:
      email: Email
      password_hash: PasswordHash
    methods:
      - name: verify_email
        mutates: true
        description: "Mark the user's email as verified"
      - name: is_locked
        returns: bool
        description: "Check if the account is locked"
    invariants:
      - "email must be unique"
      - "password must meet policy"
"#;

        let schema = parse_model_yaml_str(yaml).unwrap();
        assert_eq!(schema.entities.len(), 1);

        let user_entity = schema.entities.get("User").unwrap();
        assert_eq!(user_entity.model, Some("User".to_string()));
        assert_eq!(user_entity.implements.len(), 2);
        assert_eq!(user_entity.value_objects.len(), 2);
        assert_eq!(user_entity.methods.len(), 2);
        assert_eq!(user_entity.invariants.len(), 2);

        // Convert to AST
        let entities = schema.into_entities();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "User");
        assert_eq!(entities[0].methods.len(), 2);
        assert!(entities[0].methods[0].mutates);
        assert!(!entities[0].methods[1].mutates);
    }

    #[test]
    fn test_parse_value_objects() {
        let yaml = r#"
value_objects:
  Email:
    inner_type: String
    validation: email_format
    derives:
      - Clone
      - PartialEq
      - Eq
      - Hash
    methods:
      - name: domain
        returns: "&str"
        description: "Get the domain part of the email"
    messages:
      invalid: "Invalid email format"

  Money:
    fields:
      amount: decimal
      currency: string
    methods:
      - name: add
        params:
          other: Money
        returns: "Result<Money, CurrencyMismatch>"
"#;

        let schema = parse_model_yaml_str(yaml).unwrap();
        assert_eq!(schema.value_objects.len(), 2);

        let email_vo = schema.value_objects.get("Email").unwrap();
        assert_eq!(email_vo.inner_type, Some("String".to_string()));
        assert_eq!(email_vo.derives.len(), 4);
        assert_eq!(email_vo.methods.len(), 1);

        let money_vo = schema.value_objects.get("Money").unwrap();
        assert_eq!(money_vo.fields.len(), 2);
        assert_eq!(money_vo.methods.len(), 1);

        // Convert to AST
        let value_objects = schema.into_value_objects();
        assert_eq!(value_objects.len(), 2);
    }

    #[test]
    fn test_parse_domain_services() {
        let yaml = r#"
domain_services:
  PasswordHashingService:
    description: "Handles password hashing and verification"
    stateless: true
    dependencies:
      - UserRepository
      - name: HashingClient
        type: client
    methods:
      - name: hash
        async: true
        params:
          plain_password: "&str"
        returns: "Result<PasswordHash, HashError>"
      - name: verify
        async: true
        params:
          hash: PasswordHash
          plain_password: "&str"
        returns: bool
"#;

        let schema = parse_model_yaml_str(yaml).unwrap();
        assert_eq!(schema.domain_services.len(), 1);

        let svc = schema.domain_services.get("PasswordHashingService").unwrap();
        assert_eq!(svc.stateless, Some(true));
        assert_eq!(svc.dependencies.len(), 2);
        assert_eq!(svc.methods.len(), 2);

        // Convert to AST
        let services = schema.into_domain_services();
        assert_eq!(services.len(), 1);
        assert!(services[0].stateless);
        assert_eq!(services[0].methods.len(), 2);
    }

    #[test]
    fn test_parse_event_sourced() {
        let yaml = r#"
event_sourced:
  User:
    description: "Event-sourced user aggregate"
    events:
      - UserRegistered
      - UserActivated
      - UserDeactivated
      - EmailChanged
    snapshot:
      enabled: true
      every_n_events: 100
      max_age_seconds: 86400
    handlers:
      UserRegistered: "apply_user_registered"
      EmailChanged: "apply_email_changed"
"#;

        let schema = parse_model_yaml_str(yaml).unwrap();
        assert_eq!(schema.event_sourced.len(), 1);

        let es = schema.event_sourced.get("User").unwrap();
        assert_eq!(es.events.len(), 4);
        assert!(es.snapshot.is_some());
        let snapshot = es.snapshot.as_ref().unwrap();
        assert_eq!(snapshot.enabled, Some(true));
        assert_eq!(snapshot.every_n_events, Some(100));

        // Convert to AST
        let event_sourced = schema.into_event_sourced();
        assert_eq!(event_sourced.len(), 1);
        assert_eq!(event_sourced[0].entity_name, "User");
        assert!(event_sourced[0].snapshot.is_some());
    }

    #[test]
    fn test_parse_authorization() {
        let yaml = r#"
authorization:
  permissions:
    users:
      - read
      - create
      - update
      - delete
    roles:
      - read
      - create

  roles:
    admin:
      description: "System administrator"
      permissions:
        - "users.*"
        - "roles.*"
      level: 80
    user:
      description: "Regular user"
      permissions:
        - "users.read"
      level: 10
      own_resources:
        users: "update"

  policies:
    resource_owner:
      type: all
      rules:
        - permission: "users.read"
        - owner:
            field: owner_id
            actor_field: id

  resource_policies:
    users:
      read:
        - resource_owner
      update:
        - owner: owner_id
      delete:
        - permission: "users.delete"

  attributes:
    subject:
      - department
      - role
      - clearance_level
    resource:
      - classification
      - owner_id
    environment:
      - time_of_day
      - ip_range

  abac_policies:
    same_department:
      description: "Users can only access resources in their department"
      condition: "subject.department == resource.department"
"#;

        let schema = parse_model_yaml_str(yaml).unwrap();
        assert!(schema.authorization.is_some());

        let auth = schema.authorization.as_ref().unwrap();
        assert_eq!(auth.permissions.len(), 2);
        assert_eq!(auth.roles.len(), 2);
        assert_eq!(auth.policies.len(), 1);
        assert_eq!(auth.resource_policies.len(), 1);
        assert!(auth.attributes.is_some());
        assert_eq!(auth.abac_policies.len(), 1);

        // Convert to AST
        let auth_config = schema.into_authorization();
        assert!(auth_config.is_some());
        let auth_config = auth_config.unwrap();
        assert_eq!(auth_config.roles.len(), 2);
        assert_eq!(auth_config.policies.len(), 1);
    }

    #[test]
    fn test_parse_complete_ddd_schema() {
        // Test parsing a complete schema with all DDD features
        let yaml = r#"
models:
  - name: User
    collection: users
    fields:
      id: uuid
      email: string
      status: UserStatus

enums:
  - name: UserStatus
    variants:
      - Active
      - Inactive
      - Banned

entities:
  User:
    model: User
    implements: [Auditable]
    methods:
      - name: activate
        mutates: true

value_objects:
  Email:
    inner_type: String
    validation: email_format

domain_services:
  UserService:
    stateless: true
    methods:
      - name: create_user
        async: true
        returns: "Result<User, Error>"

event_sourced:
  User:
    events:
      - UserCreated
      - UserUpdated

authorization:
  permissions:
    users: [read, create, update, delete]
  roles:
    admin:
      permissions: ["users.*"]
"#;

        let schema = parse_model_yaml_str(yaml).unwrap();

        // Check all sections were parsed
        assert_eq!(schema.models.len(), 1);
        assert_eq!(schema.enums.len(), 1);
        assert_eq!(schema.entities.len(), 1);
        assert_eq!(schema.value_objects.len(), 1);
        assert_eq!(schema.domain_services.len(), 1);
        assert_eq!(schema.event_sourced.len(), 1);
        assert!(schema.authorization.is_some());
    }
