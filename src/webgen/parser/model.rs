//! Model YAML parser for .model.yaml files

use std::fs;
use std::path::Path;
use crate::webgen::ast::entity::{ModelSchema, EntityDefinition, FieldDefinition, FieldType, RelationDefinition, RelationType, EnumDefinition, EnumVariant, FieldAttribute, IndexDefinition, IndexType};
use crate::webgen::{Error, Result};
use serde_yaml::Value;

/// Parser for model.yaml files
pub struct ModelParser;

impl ModelParser {
    /// Parse a single model.yaml file
    pub fn parse_file(path: &Path) -> Result<ModelSchema> {
        let content = fs::read_to_string(path)
            .map_err(|e| Error::Parse(format!("Failed to read {}: {}", path.display(), e)))?;

        Self::parse_content(&content, path)
    }

    /// Parse model schema from YAML content
    pub fn parse_content(content: &str, path: &Path) -> Result<ModelSchema> {
        let root: Value = serde_yaml::from_str(content)
            .map_err(|e| Error::Parse(format!("Failed to parse YAML from {}: {}", path.display(), e)))?;

        let mapping = root.as_mapping()
            .ok_or_else(|| Error::Parse("Root must be a mapping".to_string()))?;

        // Parse models
        let models = if let Some(models_value) = mapping.get(Value::String("models".to_string())) {
            Self::parse_models(models_value, path)?
        } else {
            Vec::new()
        };

        // Parse enums
        let enums = if let Some(enums_value) = mapping.get(Value::String("enums".to_string())) {
            Self::parse_enums(enums_value)?
        } else {
            Vec::new()
        };

        Ok(ModelSchema { models, enums })
    }

    /// Parse models section
    fn parse_models(value: &Value, path: &Path) -> Result<Vec<EntityDefinition>> {
        let sequence = value.as_sequence()
            .ok_or_else(|| Error::Parse("Models must be a sequence".to_string()))?;

        sequence.iter()
            .map(|v| Self::parse_entity_from_value(v, path))
            .collect()
    }

    /// Parse enums section
    fn parse_enums(value: &Value) -> Result<Vec<EnumDefinition>> {
        let sequence = value.as_sequence()
            .ok_or_else(|| Error::Parse("Enums must be a sequence".to_string()))?;

        sequence.iter()
            .map(Self::parse_enum_from_value)
            .collect()
    }

    /// Parse a single entity from YAML value
    fn parse_entity_from_value(value: &Value, path: &Path) -> Result<EntityDefinition> {
        let mapping = value.as_mapping()
            .ok_or_else(|| Error::Parse("Entity must be a mapping".to_string()))?;

        let name = mapping.get(Value::String("name".to_string()))
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Parse("Entity missing name".to_string()))?
            .to_string();

        let collection = mapping.get(Value::String("collection".to_string()))
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Parse("Entity missing collection".to_string()))?
            .to_string();

        let fields = Self::parse_fields(
            mapping.get(Value::String("fields".to_string())),
            path
        )?;
        let relations = Self::parse_relations(
            mapping.get(Value::String("relations".to_string())),
            &fields
        )?;
        let indexes = Self::parse_indexes(
            mapping.get(Value::String("indexes".to_string()))
        );

        // Parse soft_delete flag (defaults to false if not specified)
        let soft_delete = mapping.get(Value::String("soft_delete".to_string()))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        Ok(EntityDefinition {
            name,
            collection,
            fields,
            relations,
            indexes,
            soft_delete,
        })
    }

    /// Parse a single enum from YAML value
    fn parse_enum_from_value(value: &Value) -> Result<EnumDefinition> {
        let mapping = value.as_mapping()
            .ok_or_else(|| Error::Parse("Enum must be a mapping".to_string()))?;

        let name = mapping.get(Value::String("name".to_string()))
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Parse("Enum missing name".to_string()))?
            .to_string();

        let variants_value = mapping.get(Value::String("variants".to_string()))
            .ok_or_else(|| Error::Parse("Enum missing variants".to_string()))?;

        let variants = variants_value.as_sequence()
            .ok_or_else(|| Error::Parse("Enum variants must be a sequence".to_string()))?
            .iter()
            .map(Self::parse_enum_variant_from_value)
            .collect::<Result<Vec<_>>>()?;

        Ok(EnumDefinition { name, variants })
    }

    /// Parse a single enum variant from YAML value
    fn parse_enum_variant_from_value(value: &Value) -> Result<EnumVariant> {
        if let Some(name) = value.as_str() {
            return Ok(EnumVariant {
                name: name.to_string(),
                description: None,
                is_default: false,
            });
        }

        let mapping = value.as_mapping()
            .ok_or_else(|| Error::Parse("Enum variant must be a string or mapping".to_string()))?;

        let name = mapping.get(Value::String("name".to_string()))
            .or_else(|| mapping.get(Value::String("variant".to_string())))
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Parse("Enum variant missing name".to_string()))?
            .to_string();

        let description = mapping.get(Value::String("description".to_string()))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let is_default = mapping.get(Value::String("default".to_string()))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        Ok(EnumVariant {
            name,
            description,
            is_default,
        })
    }

    /// Parse field definitions from YAML value
    fn parse_fields(fields_value: Option<&Value>, path: &Path) -> Result<Vec<FieldDefinition>> {
        let mut fields = Vec::new();

        if let Some(value) = fields_value {
            let mapping = value.as_mapping()
                .ok_or_else(|| Error::Parse("Fields must be a mapping".to_string()))?;

            for (key, value) in mapping {
                let name = key.as_str()
                    .ok_or_else(|| Error::Parse(format!("Invalid field name in {}", path.display())))?
                    .to_string();

                // Support both formats:
                // 1. Shorthand: "last_login: datetime?"
                // 2. Full: "last_login: { type: datetime?, description: ... }"
                match value {
                    // Shorthand format - value is a string like "datetime?" or "UserStatus"
                    Value::String(type_str) => {
                        let (type_name, optional) = Self::parse_type_string(type_str);
                        fields.push(FieldDefinition {
                            name,
                            type_name,
                            attributes: Vec::new(),
                            description: None,
                            optional,
                            default_value: None,
                        });
                    }
                    // Full format - value is a mapping with type, attributes, description, etc.
                    _ => {
                        let field_map = value.as_mapping()
                            .ok_or_else(|| Error::Parse(format!("Field '{}' must be a mapping or string in {}", name, path.display())))?;

                        let type_value = field_map.get(Value::String("type".to_string()))
                            .ok_or_else(|| Error::Parse(format!("Field '{}' missing type in {}", name, path.display())))?;

                        let type_str = type_value.as_str()
                            .ok_or_else(|| Error::Parse(format!("Field '{}' type must be a string in {}", name, path.display())))?;

                        let (type_name, optional) = Self::parse_type_string(type_str);
                        let attributes = Self::parse_field_attributes(field_map)?;
                        let description = Self::parse_field_description(field_map);
                        let default_value = Self::parse_field_default(&attributes);

                        fields.push(FieldDefinition {
                            name,
                            type_name,
                            attributes,
                            description,
                            optional,
                            default_value,
                        });
                    }
                }
            }
        }

        Ok(fields)
    }

    /// Parse type string like "string?", "email?", "UserStatus", "Role[]"
    fn parse_type_string(type_str: &str) -> (FieldType, bool) {
        let type_str = type_str.trim();

        // Check for optional (trailing ?)
        let optional = type_str.ends_with('?');
        let base_type = if optional {
            &type_str[..type_str.len() - 1]
        } else {
            type_str
        };

        // Check for array (trailing [])
        let is_array = base_type.ends_with("[]");
        let inner_type = if is_array {
            &base_type[..base_type.len() - 2]
        } else {
            base_type
        };

        let parse_base_type = |t: &str| -> FieldType {
            match t {
                "string" => FieldType::String,
                "int" | "integer" => FieldType::Int,
                "float" | "double" | "decimal" => FieldType::Float,
                "bool" | "boolean" => FieldType::Bool,
                "uuid" => FieldType::Uuid,
                "datetime" | "timestamp" => FieldType::DateTime,
                "date" => FieldType::Date,
                "time" => FieldType::Time,
                "email" => FieldType::Email,
                "phone" => FieldType::Phone,
                "url" => FieldType::Url,
                "json" | "jsonb" => FieldType::Json,
                "text" => FieldType::Text,
                "ip" => FieldType::Ip,
                t => FieldType::Custom(t.to_string()),
            }
        };

        let base_field_type = parse_base_type(inner_type);

        if is_array {
            let element_type = if optional {
                FieldType::Optional(Box::new(base_field_type))
            } else {
                base_field_type
            };
            (FieldType::Array(Box::new(element_type)), optional)
        } else if optional {
            (FieldType::Optional(Box::new(base_field_type)), true)
        } else {
            (base_field_type, false)
        }
    }

    /// Parse field attributes from YAML mapping
    fn parse_field_attributes(field_map: &serde_yaml::Mapping) -> Result<Vec<FieldAttribute>> {
        let mut attributes = Vec::new();

        if let Some(attrs_value) = field_map.get(serde_yaml::Value::String("attributes".to_string())) {
            if let Some(attrs) = attrs_value.as_sequence() {
                for attr in attrs {
                    if let Some(attr_str) = attr.as_str() {
                        if let Some(parsed) = Self::parse_attribute_string(attr_str) {
                            attributes.push(parsed);
                        }
                    }
                }
            }
        }

        Ok(attributes)
    }

    /// Parse a single attribute string like "@min(3)", "@unique", "@default(uuid)"
    fn parse_attribute_string(attr_str: &str) -> Option<FieldAttribute> {
        let attr_str = attr_str.trim();

        if !attr_str.starts_with('@') {
            return None;
        }

        let content = &attr_str[1..]; // Remove @

        // Check if has parentheses
        if let Some(paren_start) = content.find('(') {
            if content.ends_with(')') {
                let name = content[..paren_start].to_string();
                let args_str = &content[paren_start + 1..content.len() - 1];

                // Parse arguments (handle quoted strings)
                let args = Self::parse_attribute_args(args_str);

                return Some(FieldAttribute::new(name, args));
            }
        }

        // Simple attribute without arguments
        Some(FieldAttribute::new(content.to_string(), Vec::new()))
    }

    /// Parse attribute arguments, handling quoted strings
    fn parse_attribute_args(args_str: &str) -> Vec<String> {
        let mut args = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        let mut escape_next = false;

        for ch in args_str.chars() {
            if escape_next {
                current.push(ch);
                escape_next = false;
            } else if ch == '\\' {
                escape_next = true;
            } else if ch == '"' {
                in_quotes = !in_quotes;
            } else if ch == ',' && !in_quotes {
                args.push(current.trim().to_string());
                current = String::new();
            } else {
                current.push(ch);
            }
        }

        if !current.is_empty() {
            args.push(current.trim().to_string());
        }

        args
    }

    /// Parse field description
    fn parse_field_description(field_map: &serde_yaml::Mapping) -> Option<String> {
        field_map.get(serde_yaml::Value::String("description".to_string()))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Parse field default value from attributes
    fn parse_field_default(attributes: &[FieldAttribute]) -> Option<String> {
        attributes.iter()
            .find(|a| a.name == "default")
            .and_then(|a| a.first_arg().cloned())
    }

    /// Parse relation definitions from YAML value
    fn parse_relations(
        relations_value: Option<&Value>,
        _fields: &[FieldDefinition],
    ) -> Result<Vec<RelationDefinition>> {
        let mut relations = Vec::new();

        if let Some(value) = relations_value {
            let mapping = value.as_mapping()
                .ok_or_else(|| Error::Parse("Relations must be a mapping".to_string()))?;

            for (key, value) in mapping {
                let name = key.as_str()
                    .ok_or_else(|| Error::Parse("Invalid relation name".to_string()))?
                    .to_string();

                let rel_map = value.as_mapping()
                    .ok_or_else(|| Error::Parse(format!("Relation '{}' must be a mapping", name)))?;

                let type_value = rel_map.get(Value::String("type".to_string()))
                    .ok_or_else(|| Error::Parse(format!("Relation '{}' missing type", name)))?;

                let type_str = type_value.as_str()
                    .ok_or_else(|| Error::Parse(format!("Relation '{}' type must be a string", name)))?;

                let (target_entity, relation_type) = Self::parse_relation_type(type_str)?;

                let attributes = rel_map.get(Value::String("attributes".to_string()))
                    .and_then(|v| v.as_sequence())
                    .map(|seq| {
                        seq.iter()
                            .filter_map(|v| v.as_str())
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default();

                relations.push(RelationDefinition {
                    name,
                    target_entity,
                    relation_type,
                    attributes,
                });
            }
        }

        Ok(relations)
    }

    /// Parse relation type from string like "Role[]", "User?", "User"
    fn parse_relation_type(type_str: &str) -> Result<(String, RelationType)> {
        let type_str = type_str.trim();

        if let Some(target) = type_str.strip_suffix("[]") {
            // ManyToMany or OneToMany
            Ok((target.to_string(), RelationType::ManyToMany)) // Default to ManyToMany, will be refined by attributes
        } else if let Some(target) = type_str.strip_suffix('?') {
            // Optional relation
            Ok((target.to_string(), RelationType::OneToOne))
        } else {
            // Required relation
            Ok((type_str.to_string(), RelationType::ManyToOne))
        }
    }

    /// Parse index definitions from YAML value
    fn parse_indexes(indexes_value: Option<&Value>) -> Vec<IndexDefinition> {
        if let Some(value) = indexes_value {
            if let Some(sequence) = value.as_sequence() {
                return sequence.iter().filter_map(|idx| {
                    let mapping = idx.as_mapping()?;
                    let index_type_value = mapping.get(Value::String("type".to_string()))?;
                    let index_type = match index_type_value.as_str() {
                        Some("unique") => IndexType::Unique,
                        _ => IndexType::Index,
                    };
                    let fields_value = mapping.get(Value::String("fields".to_string()))?;
                    let fields = fields_value.as_sequence()?.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();

                    Some(IndexDefinition { index_type, fields })
                }).collect();
            }
        }
        Vec::new()
    }
}

/// Convenience function to parse a model file
pub fn parse_model_file(path: &Path) -> Result<ModelSchema> {
    ModelParser::parse_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_attribute_string() {
        let attr = ModelParser::parse_attribute_string("@min(3)").unwrap();
        assert_eq!(attr.name, "min");
        assert_eq!(attr.args, vec!["3"]);

        let attr = ModelParser::parse_attribute_string("@unique").unwrap();
        assert_eq!(attr.name, "unique");
        assert!(attr.args.is_empty());

        let attr = ModelParser::parse_attribute_string("@default('value')").unwrap();
        assert_eq!(attr.name, "default");
        assert_eq!(attr.args, vec!["value"]);
    }

    #[test]
    fn test_parse_type_string() {
        let (ty, opt) = ModelParser::parse_type_string("string?");
        assert!(opt);
        assert_eq!(ty, FieldType::Optional(Box::new(FieldType::String)));

        let (ty, opt) = ModelParser::parse_type_string("int");
        assert!(!opt);
        assert_eq!(ty, FieldType::Int);

        let (ty, opt) = ModelParser::parse_type_string("Role[]");
        assert!(!opt);
        assert!(matches!(ty, FieldType::Array(_)));
    }
}
