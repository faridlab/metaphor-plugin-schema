// Tests for entity generation
//
// Tests verify that:
// - @SerialName annotations are generated for snake_case fields
// - Fields with matching names don't get annotations
// - Template rendering produces valid Kotlin code

use super::*;
use crate::ast::{Attribute, Field, Model, PrimitiveType, TypeRef};
use pretty_assertions::assert_eq;

#[test]
fn test_field_data_needs_serial_name_for_snake_case() {
    // Arrange
    let generator = MobileGenerator::new("com.test").unwrap();
    let field = Field::new("country_id", TypeRef::Primitive(PrimitiveType::String));

    // Act
    let field_data = FieldData::from_field(&generator, &field).unwrap();

    // Assert
    assert_eq!(field_data.name, "countryId");
    assert_eq!(field_data.original_name, "country_id");
    assert!(field_data.name_needs_serial_name, "country_id should need SerialName");
}

#[test]
fn test_field_data_no_serial_name_for_camel_case() {
    // Arrange
    let generator = MobileGenerator::new("com.test").unwrap();
    let field = Field::new("name", TypeRef::Primitive(PrimitiveType::String));

    // Act
    let field_data = FieldData::from_field(&generator, &field).unwrap();

    // Assert
    assert_eq!(field_data.name, "name");
    assert_eq!(field_data.original_name, "name");
    assert!(!field_data.name_needs_serial_name, "name should not need SerialName");
}

#[test]
fn test_field_data_multiple_snake_case_fields() {
    // Arrange
    let generator = MobileGenerator::new("com.test").unwrap();

    let test_cases = vec![
        ("postal_code", "postalCode", true),
        ("province_id", "provinceId", true),
        ("city_id", "cityId", true),
        ("district_id", "districtId", true),
        ("user_id", "userId", true),
        ("created_at", "createdAt", true),
        ("updated_at", "updatedAt", true),
        ("first_name", "firstName", true),
        ("last_name", "lastName", true),
        ("phone_number", "phoneNumber", true),
        // Fields that don't need SerialName
        ("id", "id", false),
        ("name", "name", false),
        ("email", "email", false),
        ("status", "status", false),
    ];

    for (original, expected_camel, needs_annotation) in test_cases {
        let field = Field::new(original, TypeRef::Primitive(PrimitiveType::String));

        let field_data = FieldData::from_field(&generator, &field).unwrap();

        assert_eq!(
            field_data.name, expected_camel,
            "Field '{}' should convert to '{}'",
            original, expected_camel
        );
        assert_eq!(
            field_data.name_needs_serial_name, needs_annotation,
            "Field '{}' name_needs_serial_name should be {}",
            original, needs_annotation
        );
    }
}

#[test]
fn test_entity_template_includes_serial_name_annotation() {
    // Arrange
    let generator = MobileGenerator::new("com.test").unwrap();
    let model = Model {
        name: "TestEntity".to_string(),
        collection: Some("test_entities".to_string()),
        fields: vec![
            Field {
                name: "id".to_string(),
                type_ref: TypeRef::Primitive(PrimitiveType::String),
                attributes: vec![Attribute {
                    name: "id".to_string(),
                    args: vec![],
                    span: Default::default(),
                }],
                span: Default::default(),
            },
            Field {
                name: "name".to_string(),
                type_ref: TypeRef::Primitive(PrimitiveType::String),
                attributes: vec![],
                span: Default::default(),
            },
            Field {
                name: "country_id".to_string(),
                type_ref: TypeRef::Primitive(PrimitiveType::String),
                attributes: vec![],
                span: Default::default(),
            },
            Field {
                name: "postal_code".to_string(),
                type_ref: TypeRef::Optional(Box::new(TypeRef::Primitive(PrimitiveType::String))),
                attributes: vec![],
                span: Default::default(),
            },
        ],
        relations: vec![],
        indexes: vec![],
        attributes: vec![],
        span: Default::default(),
    };

    // Act
    let entity_data =
        EntityData::from_model(&generator, &model, "com.test.entity", "com.test").unwrap();

    // Render the template
    let content = generator
        .handlebars
        .render("entity", &entity_data)
        .expect("Template render failed");

    // Assert - check that SerialName annotations are present
    assert!(content.contains("@SerialName(\"country_id\")"));
    assert!(content.contains("@SerialName(\"postal_code\")"));

    // Assert - check that fields without name changes don't have SerialName
    // The "name" field should not have @SerialName annotation
    // Verify by checking the structure around the name field
    let name_field_pattern = "/** name */\n    val name: String";
    assert!(content.contains(name_field_pattern));

    // Assert - verify the complete structure
    assert!(content.contains("val countryId: String"));
    assert!(content.contains("val postalCode: String?"));
}

#[test]
fn test_entity_template_no_annotation_for_matching_names() {
    // Arrange
    let generator = MobileGenerator::new("com.test").unwrap();
    let model = Model {
        name: "SimpleEntity".to_string(),
        collection: Some("simple_entities".to_string()),
        fields: vec![
            Field {
                name: "id".to_string(),
                type_ref: TypeRef::Primitive(PrimitiveType::String),
                attributes: vec![Attribute {
                    name: "id".to_string(),
                    args: vec![],
                    span: Default::default(),
                }],
                span: Default::default(),
            },
            Field {
                name: "email".to_string(),
                type_ref: TypeRef::Primitive(PrimitiveType::String),
                attributes: vec![],
                span: Default::default(),
            },
            Field {
                name: "status".to_string(),
                type_ref: TypeRef::Primitive(PrimitiveType::String),
                attributes: vec![],
                span: Default::default(),
            },
        ],
        relations: vec![],
        indexes: vec![],
        attributes: vec![],
        span: Default::default(),
    };

    // Act
    let entity_data =
        EntityData::from_model(&generator, &model, "com.test.entity", "com.test").unwrap();

    // Render the template
    let content = generator
        .handlebars
        .render("entity", &entity_data)
        .expect("Template render failed");

    // Assert - no @SerialName annotations should be present
    assert!(!content.contains("@SerialName"));
}

#[test]
fn test_field_data_preserves_nullable_information() {
    // Arrange
    let generator = MobileGenerator::new("com.test").unwrap();

    // Non-nullable field
    let field_required = Field::new("country_id", TypeRef::Primitive(PrimitiveType::String));

    // Nullable field
    let field_optional = Field::new(
        "postal_code",
        TypeRef::Optional(Box::new(TypeRef::Primitive(PrimitiveType::String))),
    );

    // Act
    let field_data_required = FieldData::from_field(&generator, &field_required).unwrap();
    let field_data_optional = FieldData::from_field(&generator, &field_optional).unwrap();

    // Assert
    assert!(!field_data_required.is_nullable);
    assert!(field_data_optional.is_nullable);
    assert!(field_data_required.name_needs_serial_name);
    assert!(field_data_optional.name_needs_serial_name);
}
