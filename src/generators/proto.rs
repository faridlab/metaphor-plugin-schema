//! Proto file generator
//!
//! Generates Protocol Buffer definitions from schema.

use super::{GenerateError, GeneratedOutput, Generator};
use crate::ast::{EnumDef, Model, PrimitiveType, TypeRef};
use crate::resolver::ResolvedSchema;
use std::fmt::Write;
use std::path::PathBuf;

/// Generates Proto files from schema
pub struct ProtoGenerator {
    package_prefix: String,
}

impl ProtoGenerator {
    pub fn new() -> Self {
        Self {
            package_prefix: String::new(),
        }
    }

    pub fn with_package_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.package_prefix = prefix.into();
        self
    }

    fn generate_model(&self, model: &Model, module_name: &str) -> Result<String, GenerateError> {
        let mut output = String::new();

        // Header
        writeln!(output, "syntax = \"proto3\";").unwrap();
        writeln!(output).unwrap();

        // Package
        let package = if self.package_prefix.is_empty() {
            format!("{}.domain.entity", module_name)
        } else {
            format!("{}.{}.domain.entity", self.package_prefix, module_name)
        };
        writeln!(output, "package {};", package).unwrap();
        writeln!(output).unwrap();

        // Imports
        writeln!(output, "import \"buf/validate/validate.proto\";").unwrap();
        writeln!(output, "import \"google/protobuf/timestamp.proto\";").unwrap();
        writeln!(output).unwrap();

        // Message
        writeln!(output, "message {} {{", model.name).unwrap();

        let mut field_num = 1;

        // Fields
        for field in &model.fields {
            let proto_type = self.type_to_proto(&field.type_ref);
            let validations = self.generate_validations(field);

            write!(output, "  {} {} = {}", proto_type, field.name, field_num).unwrap();

            if !validations.is_empty() {
                write!(output, " [{}]", validations).unwrap();
            }

            writeln!(output, ";").unwrap();
            field_num += 1;
        }

        writeln!(output, "}}").unwrap();

        Ok(output)
    }

    fn generate_enum(&self, enum_def: &EnumDef, module_name: &str) -> Result<String, GenerateError> {
        let mut output = String::new();

        // Header
        writeln!(output, "syntax = \"proto3\";").unwrap();
        writeln!(output).unwrap();

        // Package
        let package = if self.package_prefix.is_empty() {
            format!("{}.domain.entity", module_name)
        } else {
            format!("{}.{}.domain.entity", self.package_prefix, module_name)
        };
        writeln!(output, "package {};", package).unwrap();
        writeln!(output).unwrap();

        // Enum
        writeln!(output, "enum {} {{", enum_def.name).unwrap();

        // Add UNSPECIFIED as first value (proto3 requirement)
        let prefix = to_screaming_snake_case(&enum_def.name);
        writeln!(output, "  {}_UNSPECIFIED = 0;", prefix).unwrap();

        for (i, variant) in enum_def.variants.iter().enumerate() {
            let value = variant.value.unwrap_or((i + 1) as i32);
            let variant_name = to_screaming_snake_case(&variant.name);
            writeln!(output, "  {}_{} = {};", prefix, variant_name, value).unwrap();
        }

        writeln!(output, "}}").unwrap();

        Ok(output)
    }

    fn type_to_proto(&self, type_ref: &TypeRef) -> String {
        match type_ref {
            TypeRef::Primitive(p) => p.proto_type().to_string(),
            TypeRef::Custom(name) => name.clone(),
            TypeRef::Array(inner) => format!("repeated {}", self.type_to_proto(inner)),
            TypeRef::Optional(inner) => format!("optional {}", self.type_to_proto(inner)),
            TypeRef::Map { key, value } => {
                format!("map<{}, {}>", self.type_to_proto(key), self.type_to_proto(value))
            }
            TypeRef::ModuleRef { module, name } => format!("{}.{}", module, name),
        }
    }

    fn generate_validations(&self, field: &crate::ast::Field) -> String {
        let mut validations = Vec::new();

        for attr in &field.attributes {
            match attr.name.as_str() {
                "required" => {
                    validations.push("(buf.validate.field).required = true".to_string());
                }
                "email" => {
                    validations.push("(buf.validate.field).string.email = true".to_string());
                }
                "uuid" => {
                    validations.push("(buf.validate.field).string.uuid = true".to_string());
                }
                "url" => {
                    validations.push("(buf.validate.field).string.uri = true".to_string());
                }
                "min" => {
                    if let Some(val) = attr.first_arg().and_then(|v| v.as_int()) {
                        if matches!(field.type_ref, TypeRef::Primitive(PrimitiveType::String)) {
                            validations.push(format!("(buf.validate.field).string.min_len = {}", val));
                        } else {
                            validations.push(format!("(buf.validate.field).int64.gte = {}", val));
                        }
                    }
                }
                "max" => {
                    if let Some(val) = attr.first_arg().and_then(|v| v.as_int()) {
                        if matches!(field.type_ref, TypeRef::Primitive(PrimitiveType::String)) {
                            validations.push(format!("(buf.validate.field).string.max_len = {}", val));
                        } else {
                            validations.push(format!("(buf.validate.field).int64.lte = {}", val));
                        }
                    }
                }
                "pattern" => {
                    if let Some(val) = attr.first_arg().and_then(|v| v.as_str()) {
                        validations.push(format!("(buf.validate.field).string.pattern = \"{}\"", val));
                    }
                }
                _ => {}
            }
        }

        // Add type-specific validations
        match &field.type_ref {
            TypeRef::Primitive(PrimitiveType::Email) => {
                if !validations.iter().any(|v| v.contains("email")) {
                    validations.push("(buf.validate.field).string.email = true".to_string());
                }
            }
            TypeRef::Primitive(PrimitiveType::Uuid) => {
                if !validations.iter().any(|v| v.contains("uuid")) {
                    validations.push("(buf.validate.field).string.uuid = true".to_string());
                }
            }
            TypeRef::Primitive(PrimitiveType::Url) => {
                if !validations.iter().any(|v| v.contains("uri")) {
                    validations.push("(buf.validate.field).string.uri = true".to_string());
                }
            }
            _ => {}
        }

        validations.join(", ")
    }
}

impl Default for ProtoGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl Generator for ProtoGenerator {
    fn generate(&self, schema: &ResolvedSchema) -> Result<GeneratedOutput, GenerateError> {
        let mut output = GeneratedOutput::new();
        let module_name = &schema.schema.name;

        // Generate model protos
        for model in &schema.schema.models {
            let content = self.generate_model(model, module_name)?;
            let path = PathBuf::from(format!(
                "proto/domain/entity/{}.proto",
                to_snake_case(&model.name)
            ));
            output.add_file(path, content);
        }

        // Generate enum protos
        for enum_def in &schema.schema.enums {
            let content = self.generate_enum(enum_def, module_name)?;
            let path = PathBuf::from(format!(
                "proto/domain/entity/{}.proto",
                to_snake_case(&enum_def.name)
            ));
            output.add_file(path, content);
        }

        Ok(output)
    }

    fn name(&self) -> &'static str {
        "proto"
    }
}

use crate::utils::to_snake_case;


fn to_screaming_snake_case(name: &str) -> String {
    to_snake_case(name).to_uppercase()
}
