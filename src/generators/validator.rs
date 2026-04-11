//! Validator generator
//!
//! Phase 1: Generates a **constructor function** returning `EntityValidator<E>` with
//! schema-derived `FieldRule` instances, instead of a per-entity validator struct with
//! duplicated validation loop logic.
//!
//! ## Before (was 150+ generated lines per entity):
//! ```rust,ignore
//! pub struct OrderValidator;
//! impl OrderValidator {
//!     pub fn validate(&self, entity: &Order) -> ValidationResult {
//!         let mut errors = vec![];
//!         if let Err(e) = self.validate_customer_id(&entity.customer_id) { errors.push(e); }
//!         // ... per field
//!     }
//!     fn validate_customer_id(&self, value: &Uuid) -> ValidationResult { /* identical loop */ }
//! }
//! ```
//!
//! ## After (constructor + field rules):
//! ```rust,ignore
//! pub fn order_validator() -> EntityValidator<Order> {
//!     EntityValidator::new()
//!         .rule(RequiredUuid::new("customer_id", |e: &Order| &e.customer_id))
//!         .rule(MaxLength::new("notes", |e: &Order| e.notes.as_deref().unwrap_or(""), 500))
//! }
//! pub type OrderValidator = EntityValidator<Order>;
//! ```

use super::{GenerateError, GeneratedOutput, Generator, build_generated_path, build_subdirectory_mod};
use crate::ast::model::{Field, Model};
use crate::ast::hook::Hook;
use crate::ast::PrimitiveType;
use crate::ast::TypeRef;
use crate::resolver::ResolvedSchema;
use crate::utils::{escape_rust_keyword, to_snake_case};
use std::fmt::Write;
use std::path::PathBuf;

/// Generates validator constructor functions from schema
pub struct ValidatorGenerator {
    group_by_domain: bool,
}

impl ValidatorGenerator {
    pub fn new() -> Self {
        Self { group_by_domain: false }
    }

    pub fn with_group_by_domain(mut self, group: bool) -> Self {
        self.group_by_domain = group;
        self
    }

    /// Determine which FieldRule imports are needed for this model.
    fn collect_rule_imports(&self, model: &Model, hook: Option<&Hook>) -> Vec<&'static str> {
        let mut imports: std::collections::HashSet<&'static str> = std::collections::HashSet::new();

        for field in &model.fields {
            if field.is_primary_key() { continue; }

            let is_optional = field.type_ref.is_optional();
            let inner = self.unwrap_optional(&field.type_ref);

            if field.has_attribute("required") || !is_optional {
                match inner {
                    TypeRef::Primitive(PrimitiveType::Uuid) => { imports.insert("RequiredUuid"); }
                    TypeRef::Primitive(PrimitiveType::String)
                    | TypeRef::Primitive(PrimitiveType::Email)
                    | TypeRef::Primitive(PrimitiveType::Url) => { imports.insert("RequiredString"); }
                    _ => {}
                }
            }
            if is_optional && self.is_string_inner(inner) {
                imports.insert("OptionalNotBlank");
            }
            if field.has_attribute("max_length") || field.has_attribute("maxLength") {
                imports.insert("MaxLength");
            }
            if field.has_attribute("min") || field.has_attribute("non_negative") {
                imports.insert("NonNegative");
            }
            if field.has_attribute("pattern") {
                imports.insert("Regex");
            }
        }

        // Hook rules — use rule.name as the rule kind
        if let Some(h) = hook {
            for rule in &h.rules {
                match rule.name.as_str() {
                    "max_length" => { imports.insert("MaxLength"); }
                    "min_length" => { imports.insert("MaxLength"); } // no MinLength yet, skip
                    "required"   => { imports.insert("RequiredString"); }
                    "pattern"    => { imports.insert("Regex"); }
                    _ => {}
                }
            }
        }

        let mut v: Vec<&'static str> = imports.into_iter().collect();
        v.sort();
        v
    }

    fn unwrap_optional<'a>(&self, t: &'a TypeRef) -> &'a TypeRef {
        match t {
            TypeRef::Optional(inner) => inner.as_ref(),
            other => other,
        }
    }

    fn is_string_inner(&self, t: &TypeRef) -> bool {
        matches!(t,
            TypeRef::Primitive(PrimitiveType::String)
            | TypeRef::Primitive(PrimitiveType::Email)
            | TypeRef::Primitive(PrimitiveType::Url)
        )
    }

    /// Generate the `.rule(...)` call for a single field.
    fn field_rules(&self, name: &str, field: &Field) -> Vec<String> {
        let mut rules = Vec::new();
        let fname = &field.name;
        let is_optional = field.type_ref.is_optional();
        let inner = self.unwrap_optional(&field.type_ref);

        if field.is_primary_key() {
            return rules;
        }

        // Required / optional rules
        if !is_optional {
            match inner {
                TypeRef::Primitive(PrimitiveType::Uuid) => {
                    rules.push(format!(
                        "        .rule(RequiredUuid::new(\"{fname}\", |e: &{name}| &e.{rust_field}))",
                        fname = fname,
                        name = name,
                        rust_field = escape_rust_keyword(fname),
                    ));
                }
                TypeRef::Primitive(PrimitiveType::String)
                | TypeRef::Primitive(PrimitiveType::Email)
                | TypeRef::Primitive(PrimitiveType::Url) => {
                    rules.push(format!(
                        "        .rule(RequiredString::new(\"{fname}\", |e: &{name}| &e.{rust_field}))",
                        fname = fname,
                        name = name,
                        rust_field = escape_rust_keyword(fname),
                    ));
                }
                _ => {}
            }
        } else if self.is_string_inner(inner) {
            rules.push(format!(
                "        .rule(OptionalNotBlank::new(\"{fname}\", |e: &{name}| e.{rust_field}.as_deref()))",
                fname = fname,
                name = name,
                rust_field = escape_rust_keyword(fname),
            ));
        }

        // Attribute rules
        for attr in &field.attributes {
            match attr.name.as_str() {
                "max_length" | "maxLength" => {
                    if let Some(n) = attr.first_arg().and_then(|v| v.as_int()) {
                        if self.is_string_inner(inner) {
                            let accessor = if is_optional {
                                format!("e.{}.as_deref().unwrap_or(\"\")", escape_rust_keyword(fname))
                            } else {
                                format!("e.{}.as_str()", escape_rust_keyword(fname))
                            };
                            rules.push(format!(
                                "        .rule(MaxLength::new(\"{fname}\", |e: &{name}| {acc}, {n}))",
                                fname = fname, name = name, acc = accessor, n = n,
                            ));
                        }
                    }
                }
                "min" | "non_negative" => {
                    rules.push(format!(
                        "        .rule(NonNegative::new(\"{fname}\", |e: &{name}| e.{rust_field}))",
                        fname = fname,
                        name = name,
                        rust_field = escape_rust_keyword(fname),
                    ));
                }
                "pattern" => {
                    if let Some(pat) = attr.first_arg().and_then(|v| v.as_str()) {
                        let accessor = if is_optional {
                            format!("e.{}.as_deref().unwrap_or(\"\")", escape_rust_keyword(fname))
                        } else {
                            format!("e.{}.as_str()", escape_rust_keyword(fname))
                        };
                        rules.push(format!(
                            "        .rule(Regex::new(\"{fname}\", |e: &{name}| {acc}, r\"{pat}\"))",
                            fname = fname, name = name, acc = accessor, pat = pat,
                        ));
                    }
                }
                _ => {}
            }
        }

        rules
    }

    /// Generate a validator constructor function for one entity.
    fn generate_validator(&self, model: &Model, hook: Option<&Hook>) -> Result<String, GenerateError> {
        let mut output = String::new();
        let name = &model.name;
        let snake_name = to_snake_case(name);

        writeln!(output, "//! Validator for {} entity", name).unwrap();
        writeln!(output, "//!").unwrap();
        writeln!(output, "//! Generated by metaphor-schema. Do not edit manually.").unwrap();
        writeln!(output, "//!").unwrap();
        writeln!(output, "//! Returns an `EntityValidator<{name}>` pre-loaded with schema-derived").unwrap();
        writeln!(output, "//! field rules. Extend in the `// <<< CUSTOM` zone.").unwrap();
        writeln!(output).unwrap();

        // Imports
        let rule_imports = self.collect_rule_imports(model, hook);
        writeln!(output, "use backbone_core::{{EntityValidator, ValidationErrors, ValidationError}};").unwrap();
        if !rule_imports.is_empty() {
            writeln!(output, "use backbone_core::{{{}}};", rule_imports.join(", ")).unwrap();
        }
        writeln!(output, "use crate::domain::entity::{};", name).unwrap();
        writeln!(output).unwrap();

        // Type alias
        writeln!(output, "/// Validator type alias for {} entities.", name).unwrap();
        writeln!(output, "pub type {}Validator = EntityValidator<{}>;", name, name).unwrap();
        writeln!(output).unwrap();

        // Constructor function
        writeln!(output, "/// Build a validator for {} with all schema-defined field rules.", name).unwrap();
        writeln!(output, "pub fn {}_validator() -> {}Validator {{", snake_name, name).unwrap();
        writeln!(output, "    EntityValidator::new()").unwrap();

        // Emit field rules
        let mut any_rules = false;
        for field in &model.fields {
            for rule in self.field_rules(name, field) {
                writeln!(output, "{}", rule).unwrap();
                any_rules = true;
            }
        }

        // Hook rules in the CUSTOM zone
        writeln!(output, "    // <<< CUSTOM RULES").unwrap();
        if let Some(hook) = hook {
            for rule in &hook.rules {
                writeln!(output, "    // hook rule: {} (implement in CUSTOM zone)", rule.name).unwrap();
            }
        }
        writeln!(output, "    // END CUSTOM RULES").unwrap();

        if !any_rules {
            writeln!(output, "        // No schema-derived rules — add custom rules above.").unwrap();
        }

        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "// <<< CUSTOM").unwrap();
        writeln!(output, "// END CUSTOM").unwrap();

        Ok(output)
    }

    /// Generate the shared validation types module (re-export from backbone-core).
    fn generate_shared_types(&self) -> String {
        let mut output = String::new();
        writeln!(output, "//! Shared validation types — re-exported from backbone-core.").unwrap();
        writeln!(output, "//!").unwrap();
        writeln!(output, "//! Generated by metaphor-schema. Do not edit manually.").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "pub use backbone_core::{{").unwrap();
        writeln!(output, "    ValidationError, ValidationErrors, EntityValidator,").unwrap();
        writeln!(output, "    RequiredString, MaxLength, NonNegative, OptionalNotBlank, Regex, RequiredUuid,").unwrap();
        writeln!(output, "}};").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "/// Convenience result type for validation.").unwrap();
        writeln!(output, "pub type ValidationResult = Result<(), ValidationErrors>;").unwrap();
        output
    }
}

impl Default for ValidatorGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl Generator for ValidatorGenerator {
    fn generate(&self, schema: &ResolvedSchema) -> Result<GeneratedOutput, GenerateError> {
        let mut output = GeneratedOutput::new();
        let model_names: Vec<String> = schema.schema.models.iter()
            .map(|m| m.name.clone())
            .collect();

        // shared_types.rs
        output.add_file(
            PathBuf::from("src/application/validator/shared_types.rs"),
            self.generate_shared_types(),
        );

        for model in &schema.schema.models {
            let snake_name = to_snake_case(&model.name);
            // Find matching hook for this model's validation rules.
            let hook = schema.schema.hooks.iter().find(|h| {
                (h.model_ref == model.name || h.name == model.name) && !h.rules.is_empty()
            });
            let content = self.generate_validator(model, hook)?;

            if self.group_by_domain {
                let path = build_generated_path(
                    "src/application/validator",
                    &model.name,
                    &format!("{}_validator.rs", snake_name),
                    true,
                );
                output.add_file(path, content);

                let sub_mod_path = PathBuf::from(format!(
                    "src/application/validator/{}/mod.rs", snake_name
                ));
                let sub_mod = build_subdirectory_mod(
                    &model.name,
                    &format!("{}_validator", snake_name),
                );
                output.add_file(sub_mod_path, sub_mod);
            } else {
                let path = PathBuf::from(format!(
                    "src/application/validator/{}_validator.rs",
                    snake_name
                ));
                output.add_file(path, content);
            }
        }

        // mod.rs
        if !model_names.is_empty() {
            let mut mod_content = String::new();
            writeln!(mod_content, "//! Entity validators").unwrap();
            writeln!(mod_content, "//!").unwrap();
            writeln!(mod_content, "//! Generated by metaphor-schema. Do not edit manually.").unwrap();
            writeln!(mod_content).unwrap();
            writeln!(mod_content, "pub mod shared_types;").unwrap();
            writeln!(mod_content, "pub use shared_types::{{ValidationError, ValidationErrors, ValidationResult, EntityValidator}};").unwrap();
            writeln!(mod_content).unwrap();
            for name in &model_names {
                writeln!(mod_content, "pub mod {}_validator;", to_snake_case(name)).unwrap();
            }
            writeln!(mod_content).unwrap();
            for name in &model_names {
                let snake = to_snake_case(name);
                writeln!(mod_content, "pub use {snake}_validator::{{{name}Validator, {snake}_validator}};",
                    snake = snake, name = name).unwrap();
            }
            writeln!(mod_content).unwrap();
            writeln!(mod_content, "// <<< CUSTOM").unwrap();
            writeln!(mod_content, "// END CUSTOM").unwrap();
            output.add_file(PathBuf::from("src/application/validator/mod.rs"), mod_content);
        }

        Ok(output)
    }

    fn name(&self) -> &'static str {
        "validator"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Attribute, ModuleSchema, PrimitiveType, TypeRef};

    fn create_test_model() -> Model {
        let mut model = Model::new("Order");
        model.fields = vec![
            Field {
                name: "id".to_string(),
                type_ref: TypeRef::Primitive(PrimitiveType::Uuid),
                attributes: vec![Attribute::new("id")],
                ..Default::default()
            },
            Field {
                name: "customer_id".to_string(),
                type_ref: TypeRef::Primitive(PrimitiveType::Uuid),
                attributes: vec![],
                ..Default::default()
            },
            Field {
                name: "notes".to_string(),
                type_ref: TypeRef::Optional(Box::new(TypeRef::Primitive(PrimitiveType::String))),
                attributes: vec![
                    Attribute::new("max_length")
                        .with_arg(crate::ast::model::AttributeValue::Int(500))
                ],
                ..Default::default()
            },
        ];
        model
    }

    fn create_test_schema() -> ResolvedSchema {
        let mut schema = ModuleSchema::new("test");
        schema.models.push(create_test_model());
        ResolvedSchema { schema }
    }

    #[test]
    fn test_validator_creates_files() {
        let schema = create_test_schema();
        let gen = ValidatorGenerator::new();
        let out = gen.generate(&schema).unwrap();

        assert!(out.files.contains_key(&PathBuf::from("src/application/validator/order_validator.rs")));
        assert!(out.files.contains_key(&PathBuf::from("src/application/validator/shared_types.rs")));
        assert!(out.files.contains_key(&PathBuf::from("src/application/validator/mod.rs")));
    }

    #[test]
    fn test_validator_emits_constructor_function() {
        let schema = create_test_schema();
        let gen = ValidatorGenerator::new();
        let out = gen.generate(&schema).unwrap();

        let content = out.files
            .get(&PathBuf::from("src/application/validator/order_validator.rs"))
            .unwrap();

        assert!(content.contains("pub fn order_validator()"));
        assert!(content.contains("EntityValidator::new()"));
        assert!(content.contains("pub type OrderValidator = EntityValidator<Order>"));
        assert!(content.contains("backbone_core"));
    }

    #[test]
    fn test_shared_types_re_exports_backbone_core() {
        let schema = create_test_schema();
        let gen = ValidatorGenerator::new();
        let out = gen.generate(&schema).unwrap();

        let content = out.files
            .get(&PathBuf::from("src/application/validator/shared_types.rs"))
            .unwrap();

        assert!(content.contains("backbone_core"));
        assert!(content.contains("ValidationError"));
        assert!(!content.contains("pub enum ValidationError")); // must NOT redefine
    }
}
