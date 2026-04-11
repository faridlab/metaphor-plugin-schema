//! Shared type resolution: handles type composition and extension

use super::types::{YamlSharedType, YamlField, AUDIT_METADATA_TYPE_NAME};
use indexmap::IndexMap;

/// Resolve shared types, handling composition (type extension)
/// Returns a map of type name -> resolved fields
///
/// Special handling for "Metadata" composition: instead of expanding into
/// individual audit columns (created_at, updated_at, etc.), it creates a
/// single "metadata" JSONB field with @audit_metadata attribute.
pub fn resolve_shared_types(shared_types: &IndexMap<String, YamlSharedType>) -> IndexMap<String, IndexMap<String, YamlField>> {
    let mut resolved: IndexMap<String, IndexMap<String, YamlField>> = IndexMap::new();

    // First pass: resolve all direct field definitions
    for (name, shared_type) in shared_types {
        if let YamlSharedType::Fields(fields) = shared_type {
            resolved.insert(name.clone(), fields.clone());
        }
    }

    // Second pass: resolve compositions by merging fields from referenced types
    for (name, shared_type) in shared_types {
        if let YamlSharedType::Composition(type_names) = shared_type {
            // Special handling for Metadata: consolidate into single JSONB field
            if name == AUDIT_METADATA_TYPE_NAME {
                let mut metadata_field: IndexMap<String, YamlField> = IndexMap::new();
                metadata_field.insert(
                    "metadata".to_string(),
                    YamlField::Full {
                        field_type: "json".to_string(),
                        attributes: vec![
                            "@default('{}')".to_string(),
                            "@audit_metadata".to_string(),
                        ],
                        description: Some("Audit metadata (created_at, updated_at, deleted_at, created_by, updated_by, deleted_by)".to_string()),
                    },
                );
                resolved.insert(name.clone(), metadata_field);
                continue;
            }

            // Standard composition handling for non-Metadata types
            let mut merged_fields: IndexMap<String, YamlField> = IndexMap::new();

            for type_name in type_names {
                if let Some(fields) = resolved.get(type_name) {
                    // Merge fields from the referenced type
                    for (field_name, field) in fields {
                        merged_fields.insert(field_name.clone(), field.clone());
                    }
                }
            }

            resolved.insert(name.clone(), merged_fields);
        }
    }

    resolved
}
