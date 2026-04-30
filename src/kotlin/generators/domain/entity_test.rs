// Tests for entity generation
//
// Tests verify that:
// - Snake_case schema field names are converted to camelCase Kotlin properties
// - No @SerialName annotations are emitted (API responses use camelCase, matching property names)
// - Template rendering produces valid Kotlin code

use super::*;
use crate::ast::{Attribute, Field, Model, PrimitiveType, TypeRef};
use pretty_assertions::assert_eq;

#[test]
fn test_field_data_converts_snake_case_to_camel_case() {
    let generator = MobileGenerator::new("com.test").unwrap();
    let field = Field::new("country_id", TypeRef::Primitive(PrimitiveType::String));

    let field_data = FieldData::from_field(&generator, &field).unwrap();

    assert_eq!(field_data.name, "countryId");
    assert_eq!(field_data.original_name, "country_id");
}

#[test]
fn test_field_data_preserves_already_camel_case() {
    let generator = MobileGenerator::new("com.test").unwrap();
    let field = Field::new("name", TypeRef::Primitive(PrimitiveType::String));

    let field_data = FieldData::from_field(&generator, &field).unwrap();

    assert_eq!(field_data.name, "name");
    assert_eq!(field_data.original_name, "name");
}

#[test]
fn test_field_data_multiple_snake_case_fields() {
    let generator = MobileGenerator::new("com.test").unwrap();

    let test_cases = vec![
        ("postal_code", "postalCode"),
        ("province_id", "provinceId"),
        ("city_id", "cityId"),
        ("district_id", "districtId"),
        ("user_id", "userId"),
        ("created_at", "createdAt"),
        ("updated_at", "updatedAt"),
        ("first_name", "firstName"),
        ("last_name", "lastName"),
        ("phone_number", "phoneNumber"),
        ("id", "id"),
        ("name", "name"),
        ("email", "email"),
        ("status", "status"),
    ];

    for (original, expected_camel) in test_cases {
        let field = Field::new(original, TypeRef::Primitive(PrimitiveType::String));

        let field_data = FieldData::from_field(&generator, &field).unwrap();

        assert_eq!(
            field_data.name, expected_camel,
            "Field '{}' should convert to '{}'",
            original, expected_camel
        );
    }
}

#[test]
fn test_entity_template_emits_no_serial_name_annotations() {
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
        disabled_generators: vec![],
        enabled_generators: vec![],
        span: Default::default(),
    };

    let entity_data =
        EntityData::from_model(&generator, &model, "com.test.entity", "com.test").unwrap();

    let content = generator
        .handlebars
        .render("entity", &entity_data)
        .expect("Template render failed");

    // No @SerialName annotations — backbone-framework API responses use camelCase,
    // which matches the Kotlin property names directly.
    assert!(
        !content.contains("@SerialName"),
        "Generated entity should not contain @SerialName annotations"
    );

    // Properties use camelCase derived from snake_case schema names
    assert!(content.contains("val countryId: String"));
    assert!(content.contains("val postalCode: String?"));

    // Schema field names are still preserved in doc comments
    assert!(content.contains("/** country_id */"));
    assert!(content.contains("/** postal_code */"));
}

#[test]
fn test_entity_template_no_annotation_for_matching_names() {
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
        disabled_generators: vec![],
        enabled_generators: vec![],
        span: Default::default(),
    };

    let entity_data =
        EntityData::from_model(&generator, &model, "com.test.entity", "com.test").unwrap();

    let content = generator
        .handlebars
        .render("entity", &entity_data)
        .expect("Template render failed");

    assert!(!content.contains("@SerialName"));
}

#[test]
fn test_field_data_preserves_nullable_information() {
    let generator = MobileGenerator::new("com.test").unwrap();

    let field_required = Field::new("country_id", TypeRef::Primitive(PrimitiveType::String));

    let field_optional = Field::new(
        "postal_code",
        TypeRef::Optional(Box::new(TypeRef::Primitive(PrimitiveType::String))),
    );

    let field_data_required = FieldData::from_field(&generator, &field_required).unwrap();
    let field_data_optional = FieldData::from_field(&generator, &field_optional).unwrap();

    assert!(!field_data_required.is_nullable);
    assert!(field_data_optional.is_nullable);
}
