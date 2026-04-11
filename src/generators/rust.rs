//! Rust code generator
//!
//! Generates Rust structs and enums from schema.
//!
//! ## Entity Trait
//!
//! Generated entities implement the `Entity` trait providing:
//! - `id()` - Get entity identifier
//! - `is_new()` - Check if entity is newly created
//! - `created_at()` / `updated_at()` - Timestamp accessors
//!
//! ## Generated Entity Methods
//!
//! Each entity includes methods based on its fields:
//! - Relationship accessors
//! - Status checkers (is_active(), is_deleted(), etc.)
//! - Computed properties

use super::{GenerateError, GeneratedOutput, Generator};
use crate::ast::{AttributeValue, EnumDef, Entity, EntityMethod, Model, PrimitiveType, TypeRef};
use crate::ast::hook::StateMachine;
use crate::resolver::ResolvedSchema;
use crate::utils::{escape_rust_keyword, to_pascal_case, to_snake_case};
use std::fmt::Write;
use std::path::PathBuf;

/// Generates Rust code from schema
pub struct RustGenerator;

impl RustGenerator {
    pub fn new() -> Self {
        Self
    }

    /// Check if model has audit metadata JSONB field (new pattern)
    /// Only checks for @audit_metadata attribute to avoid ambiguity with non-audit metadata fields
    fn has_audit_metadata(&self, model: &Model) -> bool {
        model.fields.iter().any(|f| f.has_attribute("audit_metadata"))
    }

    /// Check if model has a created_at field (legacy pattern)
    fn has_created_at(&self, model: &Model) -> bool {
        model.fields.iter().any(|f| f.name == "created_at")
    }

    /// Check if model has an updated_at field (legacy pattern)
    fn has_updated_at(&self, model: &Model) -> bool {
        model.fields.iter().any(|f| f.name == "updated_at")
    }

    /// Check if model has soft delete (deleted_at field) (legacy pattern)
    fn has_soft_delete(&self, model: &Model) -> bool {
        model.fields.iter().any(|f| f.name == "deleted_at")
    }

    /// Check if model has an is_deleted boolean field
    fn has_is_deleted_field(&self, model: &Model) -> bool {
        model.fields.iter().any(|f| f.name == "is_deleted")
    }

    /// Check if a specific field is optional (wrapped in Option<>)
    fn is_field_optional(&self, model: &Model, field_name: &str) -> bool {
        model.fields.iter()
            .find(|f| f.name == field_name)
            .map(|f| matches!(f.type_ref, TypeRef::Optional(_)))
            .unwrap_or(false)
    }

    /// Get the Rust type for a specific field (planned for future validation)
    fn _get_field_type(&self, model: &Model, field_name: &str) -> Option<String> {
        model.fields.iter()
            .find(|f| f.name == field_name)
            .map(|f| self.type_to_rust(&f.type_ref))
    }

    /// Check if model has a status field (planned for future status handling)
    fn _has_status_field(&self, model: &Model) -> bool {
        model.fields.iter().any(|f| f.name == "status" || f.name.ends_with("_status"))
    }

    /// Check if model has a field with @hashed attribute (for password hashing)
    fn has_hashed_field(&self, model: &Model) -> bool {
        model.fields.iter().any(|f| f.has_attribute("hashed"))
    }

    /// Get the hashed field name (e.g., "password_hash")
    fn get_hashed_field_name<'a>(&self, model: &'a Model) -> Option<&'a str> {
        model.fields.iter()
            .find(|f| f.has_attribute("hashed"))
            .map(|f| f.name.as_str())
    }

    /// Check if hashed field is optional
    fn is_hashed_field_optional(&self, model: &Model) -> bool {
        model.fields.iter()
            .find(|f| f.has_attribute("hashed"))
            .map(|f| f.type_ref.is_optional())
            .unwrap_or(false)
    }

    /// Return the `StateMachine` config for this model if its hook defines one.
    ///
    /// Looks through `schema.schema.hooks` for a hook associated with this model
    /// (matched by `model_ref` or `name`) that carries a `state_machine` definition.
    fn find_state_machine_field<'a>(&self, model: &Model, schema: &'a ResolvedSchema) -> Option<&'a StateMachine> {
        schema.schema.hooks
            .iter()
            .find(|h| h.model_ref == model.name || h.name == model.name)
            .and_then(|h| h.state_machine.as_ref())
    }

    /// Get the primary key field name
    fn get_pk_field<'a>(&self, model: &'a Model) -> &'a str {
        model.fields.iter()
            .find(|f| f.is_primary_key())
            .map(|f| f.name.as_str())
            .unwrap_or("id")
    }

    /// Get the primary key type
    fn get_pk_type(&self, model: &Model) -> String {
        model.fields.iter()
            .find(|f| f.is_primary_key())
            .map(|f| self.type_to_rust(&f.type_ref))
            .unwrap_or_else(|| "Uuid".to_string())
    }

    /// Generate strongly-typed ID newtype for an entity
    fn generate_typed_id(&self, entity_name: &str, output: &mut String) {
        let id_type = format!("{}Id", entity_name);

        writeln!(output, "/// Strongly-typed ID for {}", entity_name).unwrap();
        writeln!(output, "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]").unwrap();
        writeln!(output, "#[serde(transparent)]").unwrap();
        writeln!(output, "pub struct {}(pub Uuid);", id_type).unwrap();
        writeln!(output).unwrap();
        writeln!(output, "impl {} {{", id_type).unwrap();
        writeln!(output, "    pub fn new(id: Uuid) -> Self {{ Self(id) }}").unwrap();
        writeln!(output, "    pub fn generate() -> Self {{ Self(Uuid::new_v4()) }}").unwrap();
        writeln!(output, "    pub fn into_inner(self) -> Uuid {{ self.0 }}").unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "impl std::fmt::Display for {} {{", id_type).unwrap();
        writeln!(output, "    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{").unwrap();
        writeln!(output, "        write!(f, \"{{}}\", self.0)").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "impl std::str::FromStr for {} {{", id_type).unwrap();
        writeln!(output, "    type Err = uuid::Error;").unwrap();
        writeln!(output, "    fn from_str(s: &str) -> Result<Self, Self::Err> {{").unwrap();
        writeln!(output, "        Ok(Self(Uuid::parse_str(s)?))").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "impl From<Uuid> for {} {{", id_type).unwrap();
        writeln!(output, "    fn from(id: Uuid) -> Self {{ Self(id) }}").unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "impl From<{}> for Uuid {{", id_type).unwrap();
        writeln!(output, "    fn from(id: {}) -> Self {{ id.0 }}", id_type).unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // AsRef<Uuid> — enables generic code to access inner Uuid by reference
        writeln!(output, "impl AsRef<Uuid> for {} {{", id_type).unwrap();
        writeln!(output, "    fn as_ref(&self) -> &Uuid {{ &self.0 }}").unwrap();
        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // Deref — allows transparent use as &Uuid
        writeln!(output, "impl std::ops::Deref for {} {{", id_type).unwrap();
        writeln!(output, "    type Target = Uuid;").unwrap();
        writeln!(output, "    fn deref(&self) -> &Self::Target {{ &self.0 }}").unwrap();
        writeln!(output, "}}").unwrap();
    }

    /// Find the Entity definition for a model (if exists)
    fn find_entity_for_model<'a>(&self, model: &Model, schema: &'a ResolvedSchema) -> Option<&'a Entity> {
        schema.schema.entities.iter().find(|e| e.model_ref == model.name)
    }

    /// Check if a field is an audit/system field that should be auto-defaulted in constructors
    fn is_system_field(&self, field: &crate::ast::Field, model: &Model) -> bool {
        let pk_field = self.get_pk_field(model);
        let system_names = ["created_at", "updated_at", "deleted_at",
                           "created_by", "updated_by", "deleted_by",
                           "is_deleted"];
        field.name == pk_field
            || field.has_attribute("audit_metadata")
            || system_names.contains(&field.name.as_str())
    }

    /// Get the default value expression for a field in the new() constructor
    fn field_default_expr(&self, field: &crate::ast::Field, model: &Model) -> String {
        let pk_field = self.get_pk_field(model);
        let pk_type = self.get_pk_type(model);

        if field.name == pk_field {
            if pk_type.starts_with("Option<") {
                "None".to_string()
            } else if pk_type == "Uuid" {
                "Uuid::new_v4()".to_string()
            } else {
                "Default::default()".to_string()
            }
        } else if field.has_attribute("audit_metadata") {
            "AuditMetadata::default()".to_string()
        } else if field.type_ref.is_optional() {
            "None".to_string()
        } else {
            // System timestamp fields use Utc::now(), everything else uses Default::default()
            // (bool defaults to false, numbers to 0, String to "", etc.)
            match field.name.as_str() {
                "created_at" | "updated_at" => "Utc::now()".to_string(),
                _ => "Default::default()".to_string(),
            }
        }
    }

    /// Get the builder default expression for a field's @default attribute.
    /// Returns None if the field has no @default.
    fn builder_field_default_expr(
        &self,
        field: &crate::ast::Field,
        schema: &ResolvedSchema,
    ) -> Option<String> {
        let attr_value = field.default_value()?;
        let rust_type = self.type_to_rust(&field.type_ref);
        let is_decimal = matches!(rust_type.as_str(), "Decimal");

        match attr_value {
            AttributeValue::Ident(ident) => {
                match ident.as_str() {
                    "uuid" => Some("Uuid::new_v4()".to_string()),
                    "now" => Some("Utc::now()".to_string()),
                    "true" => Some("true".to_string()),
                    "false" => Some("false".to_string()),
                    _ => {
                        // Check if field type is a schema-defined enum
                        let is_enum = schema.schema.enums.iter().any(|e| {
                            to_pascal_case(&e.name) == rust_type
                        });
                        if is_enum {
                            // Use Default::default() — generated enums always impl Default
                            Some(format!("{}::default()", rust_type))
                        } else {
                            Some("Default::default()".to_string())
                        }
                    }
                }
            }
            AttributeValue::String(s) => {
                let s_trimmed = s.trim();
                if s_trimmed.starts_with('{') || s_trimmed.starts_with('[') {
                    Some(format!("serde_json::json!({})", s_trimmed))
                } else {
                    Some(format!("\"{}\".to_string()", s))
                }
            }
            AttributeValue::Int(i) => {
                if is_decimal {
                    Some(format!("Decimal::from({})", i))
                } else {
                    Some(format!("{}", i))
                }
            }
            AttributeValue::Float(f) => {
                if is_decimal {
                    Some(format!("Decimal::try_from({}_f64).unwrap_or_default()", f))
                } else {
                    Some(format!("{}_f64", f))
                }
            }
            AttributeValue::Bool(b) => Some(format!("{}", b)),
            _ => None,
        }
    }

    /// Generate entity methods implementation block
    fn generate_entity_methods(&self, model: &Model, entity: Option<&Entity>, state_machine: Option<&StateMachine>, output: &mut String) {
        let name = &model.name;
        let pk_field = self.get_pk_field(model);
        let pk_type = self.get_pk_type(model);

        writeln!(output).unwrap();
        writeln!(output, "impl {} {{", name).unwrap();

        // Builder static method
        writeln!(output, "    /// Create a builder for {}", name).unwrap();
        writeln!(output, "    pub fn builder() -> {}Builder {{", name).unwrap();
        writeln!(output, "        {}Builder::default()", name).unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Constructor: new() with required fields
        {
            // Collect constructor params (required, non-system fields)
            let param_fields: Vec<&crate::ast::Field> = model.fields.iter()
                .filter(|f| !self.is_system_field(f, model) && !f.type_ref.is_optional())
                .collect();

            // Build parameter list
            let params: Vec<String> = param_fields.iter().map(|f| {
                let rust_type = self.type_to_rust(&f.type_ref);
                let field_name = escape_rust_keyword(&f.name);
                format!("{}: {}", field_name, rust_type)
            }).collect();

            writeln!(output, "    /// Create a new {} with required fields", name).unwrap();
            writeln!(output, "    pub fn new({}) -> Self {{", params.join(", ")).unwrap();
            writeln!(output, "        Self {{").unwrap();

            for field in &model.fields {
                let field_name = escape_rust_keyword(&field.name);
                if self.is_system_field(field, model) || field.type_ref.is_optional() {
                    writeln!(output, "            {}: {},", field_name, self.field_default_expr(field, model)).unwrap();
                } else {
                    writeln!(output, "            {},", field_name).unwrap();
                }
            }

            writeln!(output, "        }}").unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();
        }

        // ID accessor
        writeln!(output, "    /// Get the entity's unique identifier").unwrap();
        writeln!(output, "    pub fn id(&self) -> &{} {{", pk_type.trim_start_matches("Option<").trim_end_matches('>')).unwrap();
        if pk_type.starts_with("Option<") {
            writeln!(output, "        self.{}.as_ref().expect(\"Entity must have an ID\")", pk_field).unwrap();
        } else {
            writeln!(output, "        &self.{}", pk_field).unwrap();
        }
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();

        // Strongly-typed ID accessor (only for Uuid primary keys)
        if pk_type == "Uuid" {
            writeln!(output, "    /// Get a strongly-typed ID for this entity").unwrap();
            writeln!(output, "    pub fn typed_id(&self) -> {}Id {{", name).unwrap();
            writeln!(output, "        {}Id(self.{})", name, pk_field).unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();
        }

        // is_new check (if id is optional)
        if pk_type.starts_with("Option<") {
            writeln!(output, "    /// Check if this is a new entity (not yet persisted)").unwrap();
            writeln!(output, "    pub fn is_new(&self) -> bool {{").unwrap();
            writeln!(output, "        self.{}.is_none()", pk_field).unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();
        }

        // Audit metadata accessors - new pattern with JSONB metadata field
        if self.has_audit_metadata(model) {
            // created_at accessor
            writeln!(output, "    /// Get when this entity was created").unwrap();
            writeln!(output, "    pub fn created_at(&self) -> Option<&DateTime<Utc>> {{").unwrap();
            writeln!(output, "        self.metadata.created_at.as_ref()").unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();

            // updated_at accessor
            writeln!(output, "    /// Get when this entity was last updated").unwrap();
            writeln!(output, "    pub fn updated_at(&self) -> Option<&DateTime<Utc>> {{").unwrap();
            writeln!(output, "        self.metadata.updated_at.as_ref()").unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();

            // is_deleted accessor
            writeln!(output, "    /// Check if this entity is soft deleted").unwrap();
            writeln!(output, "    pub fn is_deleted(&self) -> bool {{").unwrap();
            writeln!(output, "        self.metadata.deleted_at.is_some()").unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();

            // is_active accessor
            writeln!(output, "    /// Check if this entity is active (not deleted)").unwrap();
            writeln!(output, "    pub fn is_active(&self) -> bool {{").unwrap();
            writeln!(output, "        self.metadata.deleted_at.is_none()").unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();

            // deleted_at accessor
            writeln!(output, "    /// Get when this entity was deleted").unwrap();
            writeln!(output, "    pub fn deleted_at(&self) -> Option<&DateTime<Utc>> {{").unwrap();
            writeln!(output, "        self.metadata.deleted_at.as_ref()").unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();

            // created_by accessor
            writeln!(output, "    /// Get who created this entity").unwrap();
            writeln!(output, "    pub fn created_by(&self) -> Option<&Uuid> {{").unwrap();
            writeln!(output, "        self.metadata.created_by.as_ref()").unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();

            // updated_by accessor
            writeln!(output, "    /// Get who last updated this entity").unwrap();
            writeln!(output, "    pub fn updated_by(&self) -> Option<&Uuid> {{").unwrap();
            writeln!(output, "        self.metadata.updated_by.as_ref()").unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();

            // deleted_by accessor
            writeln!(output, "    /// Get who deleted this entity").unwrap();
            writeln!(output, "    pub fn deleted_by(&self) -> Option<&Uuid> {{").unwrap();
            writeln!(output, "        self.metadata.deleted_by.as_ref()").unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();
        }
        // Legacy pattern: Timestamp accessors - handle both optional and non-optional datetime fields
        else if self.has_created_at(model) {
            writeln!(output, "    /// Get when this entity was created").unwrap();
            if self.is_field_optional(model, "created_at") {
                writeln!(output, "    pub fn created_at(&self) -> Option<&DateTime<Utc>> {{").unwrap();
                writeln!(output, "        self.created_at.as_ref()").unwrap();
            } else {
                writeln!(output, "    pub fn created_at(&self) -> &DateTime<Utc> {{").unwrap();
                writeln!(output, "        &self.created_at").unwrap();
            }
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();
        }

        // Legacy pattern: updated_at accessor
        if !self.has_audit_metadata(model) && self.has_updated_at(model) {
            writeln!(output, "    /// Get when this entity was last updated").unwrap();
            if self.is_field_optional(model, "updated_at") {
                writeln!(output, "    pub fn updated_at(&self) -> Option<&DateTime<Utc>> {{").unwrap();
                writeln!(output, "        self.updated_at.as_ref()").unwrap();
            } else {
                writeln!(output, "    pub fn updated_at(&self) -> &DateTime<Utc> {{").unwrap();
                writeln!(output, "        &self.updated_at").unwrap();
            }
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();
        }

        // Legacy pattern: Soft delete methods - handle both is_deleted boolean and optional deleted_at
        if !self.has_audit_metadata(model) && self.has_soft_delete(model) {
            let deleted_at_optional = self.is_field_optional(model, "deleted_at");
            let has_is_deleted = self.has_is_deleted_field(model);

            writeln!(output, "    /// Check if this entity is soft deleted").unwrap();
            writeln!(output, "    pub fn is_deleted(&self) -> bool {{").unwrap();
            if has_is_deleted {
                // Use the explicit is_deleted boolean field
                writeln!(output, "        self.is_deleted").unwrap();
            } else if deleted_at_optional {
                // Use deleted_at.is_some() for optional deleted_at
                writeln!(output, "        self.deleted_at.is_some()").unwrap();
            } else {
                // Non-optional deleted_at without is_deleted field - always return false
                // (This is a schema design issue, but we handle it gracefully)
                writeln!(output, "        false // Note: deleted_at is non-optional, consider adding is_deleted field").unwrap();
            }
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();

            writeln!(output, "    /// Check if this entity is active (not deleted)").unwrap();
            writeln!(output, "    pub fn is_active(&self) -> bool {{").unwrap();
            if has_is_deleted {
                writeln!(output, "        !self.is_deleted").unwrap();
            } else if deleted_at_optional {
                writeln!(output, "        self.deleted_at.is_none()").unwrap();
            } else {
                writeln!(output, "        true // Note: deleted_at is non-optional, consider adding is_deleted field").unwrap();
            }
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();

            writeln!(output, "    /// Get when this entity was deleted").unwrap();
            if deleted_at_optional {
                writeln!(output, "    pub fn deleted_at(&self) -> Option<&DateTime<Utc>> {{").unwrap();
                writeln!(output, "        self.deleted_at.as_ref()").unwrap();
            } else if has_is_deleted {
                // Non-optional deleted_at with is_deleted flag - return Option based on flag
                writeln!(output, "    pub fn deleted_at(&self) -> Option<&DateTime<Utc>> {{").unwrap();
                writeln!(output, "        if self.is_deleted {{ Some(&self.deleted_at) }} else {{ None }}").unwrap();
            } else {
                // Non-optional deleted_at without is_deleted - return direct reference
                writeln!(output, "    pub fn deleted_at(&self) -> &DateTime<Utc> {{").unwrap();
                writeln!(output, "        &self.deleted_at").unwrap();
            }
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();
        }

        // Status-based methods
        for field in &model.fields {
            if field.name == "status" {
                let status_type = self.type_to_rust(&field.type_ref);
                let inner_type = status_type.trim_start_matches("Option<").trim_end_matches('>');

                writeln!(output, "    /// Get the current status").unwrap();
                if status_type.starts_with("Option<") {
                    writeln!(output, "    pub fn status(&self) -> Option<&{}> {{", inner_type).unwrap();
                    writeln!(output, "        self.status.as_ref()").unwrap();
                } else {
                    writeln!(output, "    pub fn status(&self) -> &{} {{", inner_type).unwrap();
                    writeln!(output, "        &self.status").unwrap();
                }
                writeln!(output, "    }}").unwrap();
                writeln!(output).unwrap();
                break;
            }
        }

        // ===================================================================
        // Password verification for @hashed fields
        // ===================================================================
        if self.has_hashed_field(model) {
            if let Some(hash_field) = self.get_hashed_field_name(model) {
                let is_optional = self.is_hashed_field_optional(model);
                writeln!(output).unwrap();
                writeln!(output, "    /// Verify a password against the stored hash").unwrap();
                writeln!(output, "    ///").unwrap();
                writeln!(output, "    /// Uses argon2 for secure password verification.").unwrap();
                if is_optional {
                    writeln!(output, "    /// Returns true if no password is set (hash is None) and no password is provided.").unwrap();
                    writeln!(output, "    pub fn verify_password(&self, password: Option<&str>) -> bool {{").unwrap();
                    writeln!(output, "        match (&self.{}, password) {{", hash_field).unwrap();
                    writeln!(output, "            (Some(hash), Some(pwd)) => {{").unwrap();
                    writeln!(output, "                use argon2::PasswordVerifier;").unwrap();
                    writeln!(output, "                use argon2::password_hash::PasswordHash;").unwrap();
                    writeln!(output, "                let parsed_hash = PasswordHash::new(hash);").unwrap();
                    writeln!(output, "                match parsed_hash {{").unwrap();
                    writeln!(output, "                    Ok(h) => {{").unwrap();
                    writeln!(output, "                        let argon2 = argon2::Argon2::default();").unwrap();
                    writeln!(output, "                        argon2.verify_password(pwd.as_bytes(), &h).is_ok()").unwrap();
                    writeln!(output, "                    }}").unwrap();
                    writeln!(output, "                    Err(_) => false,").unwrap();
                    writeln!(output, "                }}").unwrap();
                    writeln!(output, "            }}").unwrap();
                    writeln!(output, "            (None, _) => true,  // No password required").unwrap();
                    writeln!(output, "            (Some(_), None) => false,  // Password required but not provided").unwrap();
                    writeln!(output, "        }}").unwrap();
                    writeln!(output, "    }}").unwrap();
                } else {
                    writeln!(output, "    pub fn verify_password(&self, password: &str) -> bool {{").unwrap();
                    writeln!(output, "        use argon2::PasswordVerifier;").unwrap();
                    writeln!(output, "        use argon2::password_hash::PasswordHash;").unwrap();
                    writeln!(output, "        let parsed_hash = PasswordHash::new(&self.{});", hash_field).unwrap();
                    writeln!(output, "        match parsed_hash {{").unwrap();
                    writeln!(output, "            Ok(hash) => {{").unwrap();
                    writeln!(output, "                let argon2 = argon2::Argon2::default();").unwrap();
                    writeln!(output, "                argon2.verify_password(password.as_bytes(), &hash).is_ok()").unwrap();
                    writeln!(output, "            }}").unwrap();
                    writeln!(output, "            Err(_) => false,").unwrap();
                    writeln!(output, "        }}").unwrap();
                    writeln!(output, "    }}").unwrap();
                }
                writeln!(output).unwrap();

                // Also generate a hash_password helper as associated function
                writeln!(output, "    /// Hash a password for storage").unwrap();
                writeln!(output, "    ///").unwrap();
                writeln!(output, "    /// Uses argon2 with default parameters.").unwrap();
                writeln!(output, "    pub fn hash_password(password: &str) -> Result<String, String> {{").unwrap();
                writeln!(output, "        use argon2::PasswordHasher;").unwrap();
                writeln!(output, "        use argon2::password_hash::{{SaltString, rand_core::OsRng}};").unwrap();
                writeln!(output, "        let argon2 = argon2::Argon2::default();").unwrap();
                writeln!(output, "        let salt = SaltString::generate(&mut OsRng);").unwrap();
                writeln!(output, "        argon2.hash_password(password.as_bytes(), &salt)").unwrap();
                writeln!(output, "            .map(|hash| hash.to_string())").unwrap();
                writeln!(output, "            .map_err(|e| e.to_string())").unwrap();
                writeln!(output, "    }}").unwrap();
                writeln!(output).unwrap();
            }
        }

        // ===================================================================
        // FLUENT SETTERS for optional fields (with_* methods)
        // ===================================================================
        {
            let optional_fields: Vec<&crate::ast::Field> = model.fields.iter()
                .filter(|f| f.type_ref.is_optional() && !self.is_system_field(f, model))
                .collect();

            if !optional_fields.is_empty() {
                writeln!(output).unwrap();
                writeln!(output, "    // ==========================================================").unwrap();
                writeln!(output, "    // Fluent Setters (with_* for optional fields)").unwrap();
                writeln!(output, "    // ==========================================================").unwrap();

                for field in &optional_fields {
                    let field_name = escape_rust_keyword(&field.name);
                    let inner_type = self.type_to_rust(field.type_ref.inner_type().unwrap());
                    writeln!(output).unwrap();
                    writeln!(output, "    /// Set the {} field (chainable)", field.name).unwrap();
                    writeln!(output, "    pub fn with_{}(mut self, value: {}) -> Self {{", field_name, inner_type).unwrap();
                    writeln!(output, "        self.{} = Some(value);", field_name).unwrap();
                    writeln!(output, "        self").unwrap();
                    writeln!(output, "    }}").unwrap();
                }
            }
        }

        // ===================================================================
        // TRANSITION_TO — state machine transition method (Phase 2)
        // ===================================================================
        if let Some(sm) = state_machine {
            let sm_field = &sm.field;
            // Find the Rust type for the state machine field (for explicit parse turbofish)
            let sm_field_rust_type = model.fields.iter()
                .find(|f| f.name == *sm_field)
                .map(|f| self.type_to_rust(&f.type_ref))
                .unwrap_or_else(|| "String".to_string());
            writeln!(output).unwrap();
            writeln!(output, "    // ==========================================================").unwrap();
            writeln!(output, "    // State Machine").unwrap();
            writeln!(output, "    // ==========================================================").unwrap();
            writeln!(output).unwrap();
            writeln!(output, "    /// Transition to a new state via the {} state machine.", sm_field).unwrap();
            writeln!(output, "    ///").unwrap();
            writeln!(output, "    /// Returns `Err` if the transition is not permitted from the current state.").unwrap();
            writeln!(output, "    /// Use this method instead of assigning `self.{}` directly.", sm_field).unwrap();
            writeln!(output, "    pub fn transition_to(&mut self, new_state: {name}State) -> Result<(), StateMachineError> {{",
                name = name).unwrap();
            // Convert entity's field type to state machine's state type via Display/FromStr
            // parse::<{Name}State>() returns Result<_, StateMachineError> so ? works directly
            writeln!(output,
                "        let current = self.{field}.to_string().parse::<{name}State>()?;",
                field = sm_field, name = name).unwrap();
            writeln!(output, "        let mut sm = {name}StateMachine::from_state(current);",
                name = name).unwrap();
            writeln!(output, "        sm.transition_to_state(new_state)?;").unwrap();
            // Convert state machine's state type back to entity's field type via Display/FromStr
            // Explicit turbofish type avoids type inference failures for non-String error types
            writeln!(output,
                "        self.{field} = new_state.to_string().parse::<{field_type}>()\
                \n            .map_err(|e| StateMachineError::InvalidState(e.to_string()))?;",
                field = sm_field, field_type = sm_field_rust_type).unwrap();
            writeln!(output, "        Ok(())").unwrap();
            writeln!(output, "    }}").unwrap();
        }

        // ===================================================================
        // APPLY PATCH — apply partial updates from a field map
        // ===================================================================
        {
            let patchable_fields: Vec<&crate::ast::Field> = model.fields.iter()
                .filter(|f| !self.is_system_field(f, model))
                .collect();

            if !patchable_fields.is_empty() {
                writeln!(output).unwrap();
                writeln!(output, "    // ==========================================================").unwrap();
                writeln!(output, "    // Partial Update").unwrap();
                writeln!(output, "    // ==========================================================").unwrap();
                writeln!(output).unwrap();
                writeln!(output, "    /// Apply partial updates from a map of field name to JSON value").unwrap();
                writeln!(output, "    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {{").unwrap();
                writeln!(output, "        for (key, value) in fields {{").unwrap();
                writeln!(output, "            match key.as_str() {{").unwrap();

                for field in &patchable_fields {
                    // Skip state machine field — callers must use transition_to() (Phase 2)
                    if state_machine.map_or(false, |sm| sm.field == field.name) {
                        continue;
                    }
                    let field_name = escape_rust_keyword(&field.name);
                    writeln!(output, "                \"{}\" => {{", field.name).unwrap();
                    writeln!(output, "                    if let Ok(v) = serde_json::from_value(value) {{ self.{} = v; }}", field_name).unwrap();
                    writeln!(output, "                }}").unwrap();
                }

                writeln!(output, "                _ => {{}} // ignore unknown fields").unwrap();
                writeln!(output, "            }}").unwrap();
                writeln!(output, "        }}").unwrap();
                writeln!(output, "    }}").unwrap();
            }
        }

        // Custom section for user-defined methods (preserved across regeneration)
        writeln!(output).unwrap();
        writeln!(output, "    // <<< CUSTOM METHODS START >>>").unwrap();

        // Generate DDD method stubs and invariant checks inside the custom block.
        // On first generation these are todo!() stubs; once implemented by the user,
        // the merge_custom_methods_block() function preserves the entire block verbatim.
        if let Some(entity) = entity {
            if !entity.methods.is_empty() {
                writeln!(output).unwrap();
                writeln!(output, "    // ==========================================================").unwrap();
                writeln!(output, "    // DDD Entity Methods").unwrap();
                writeln!(output, "    // ==========================================================").unwrap();

                // Collect auto-generated method names to detect conflicts
                let reserved_methods = self.get_reserved_method_names(model);

                for method in &entity.methods {
                    // Skip if method name conflicts with auto-generated methods
                    if reserved_methods.contains(&method.name.as_str()) {
                        writeln!(output, "    // Note: Method '{}' is auto-generated and skipped here", method.name).unwrap();
                        continue;
                    }
                    self.generate_ddd_method(method, output);
                }
            }

            // Generate invariant check methods
            if !entity.invariants.is_empty() {
                writeln!(output).unwrap();
                writeln!(output, "    /// Check all business invariants").unwrap();
                writeln!(output, "    pub fn check_invariants(&self) -> Result<(), Vec<&'static str>> {{").unwrap();
                writeln!(output, "        let mut errors = Vec::new();").unwrap();
                for (i, invariant) in entity.invariants.iter().enumerate() {
                    writeln!(output, "        // Invariant {}: {}", i + 1, invariant).unwrap();
                    writeln!(output, "        // TODO: Implement invariant check").unwrap();
                }
                writeln!(output, "        if errors.is_empty() {{ Ok(()) }} else {{ Err(errors) }}").unwrap();
                writeln!(output, "    }}").unwrap();
            }
        }

        writeln!(output, "    // <<< CUSTOM METHODS END >>>").unwrap();

        writeln!(output, "}}").unwrap();

        // Generate Entity trait implementation
        self.generate_entity_trait_impl(model, output);

        // Generate backbone_core::PersistentEntity implementation
        self.generate_persistent_entity_impl(model, output);
        // Note: EntityRepoMeta is generated in generate_model() where schema is available
    }

    /// Generate builder struct and implementation for an entity
    fn generate_builder(
        &self,
        model: &Model,
        schema: &ResolvedSchema,
        output: &mut String,
    ) {
        let name = &model.name;
        let builder_name = format!("{}Builder", name);

        // Collect non-system fields for builder
        let builder_fields: Vec<&crate::ast::Field> = model.fields.iter()
            .filter(|f| !self.is_system_field(f, model))
            .collect();

        // ---- Builder struct ----
        writeln!(output).unwrap();
        writeln!(output, "/// Builder for {} entity", name).unwrap();
        writeln!(output, "///").unwrap();
        writeln!(output, "/// Provides a fluent API for constructing {} instances.", name).unwrap();
        writeln!(output, "/// System fields (id, metadata, timestamps) are auto-initialized.").unwrap();
        writeln!(output, "#[derive(Debug, Clone, Default)]").unwrap();
        writeln!(output, "pub struct {} {{", builder_name).unwrap();

        for field in &builder_fields {
            let field_name = escape_rust_keyword(&field.name);
            if field.type_ref.is_optional() {
                // Entity type is Option<T>; builder stores Option<InnerT> (no double-wrapping)
                let inner_type = self.type_to_rust(field.type_ref.inner_type().unwrap());
                writeln!(output, "    {}: Option<{}>,", field_name, inner_type).unwrap();
            } else {
                // Entity type is T; builder stores Option<T>
                let rust_type = self.type_to_rust(&field.type_ref);
                writeln!(output, "    {}: Option<{}>,", field_name, rust_type).unwrap();
            }
        }

        writeln!(output, "}}").unwrap();
        writeln!(output).unwrap();

        // ---- Builder impl ----
        writeln!(output, "impl {} {{", builder_name).unwrap();

        // Setter methods
        for field in &builder_fields {
            let field_name = escape_rust_keyword(&field.name);
            // Setter always takes the inner type (unwrapped for optionals)
            let setter_type = if field.type_ref.is_optional() {
                self.type_to_rust(field.type_ref.inner_type().unwrap())
            } else {
                self.type_to_rust(&field.type_ref)
            };

            // Doc comment
            if let Some(default_expr) = self.builder_field_default_expr(field, schema) {
                writeln!(output, "    /// Set the {} field (default: `{}`)", field.name, default_expr).unwrap();
            } else if field.type_ref.is_optional() {
                writeln!(output, "    /// Set the {} field (optional)", field.name).unwrap();
            } else {
                writeln!(output, "    /// Set the {} field (required)", field.name).unwrap();
            }

            writeln!(output, "    pub fn {}(mut self, value: {}) -> Self {{", field_name, setter_type).unwrap();
            writeln!(output, "        self.{} = Some(value);", field_name).unwrap();
            writeln!(output, "        self").unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output).unwrap();
        }

        // build() method
        writeln!(output, "    /// Build the {} entity", name).unwrap();
        writeln!(output, "    ///").unwrap();
        writeln!(output, "    /// Returns Err if any required field without a default is missing.").unwrap();
        writeln!(output, "    pub fn build(self) -> Result<{}, String> {{", name).unwrap();

        // Phase 1: Validate required fields without @default
        for field in &builder_fields {
            if !field.type_ref.is_optional()
                && self.builder_field_default_expr(field, schema).is_none()
            {
                let field_name = escape_rust_keyword(&field.name);
                writeln!(output, "        let {} = self.{}.ok_or_else(|| \"{} is required\".to_string())?;",
                    field_name, field_name, field.name).unwrap();
            }
        }

        writeln!(output).unwrap();
        writeln!(output, "        Ok({} {{", name).unwrap();

        // Phase 2: Construct entity with all fields
        for field in &model.fields {
            let field_name = escape_rust_keyword(&field.name);

            if self.is_system_field(field, model) {
                // System: auto-initialize
                writeln!(output, "            {}: {},", field_name, self.field_default_expr(field, model)).unwrap();
            } else if field.type_ref.is_optional() {
                // Optional: pass through (builder Option<T> -> entity Option<T>)
                writeln!(output, "            {}: self.{},", field_name, field_name).unwrap();
            } else if let Some(default_expr) = self.builder_field_default_expr(field, schema) {
                // Required with @default: unwrap_or
                writeln!(output, "            {}: self.{}.unwrap_or({}),", field_name, field_name, default_expr).unwrap();
            } else {
                // Required without default: use validated local variable
                writeln!(output, "            {},", field_name).unwrap();
            }
        }

        writeln!(output, "        }})").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output, "}}").unwrap();
    }

    /// Extract custom error types from entity method return signatures
    /// Returns a list of error type names that need to be generated
    fn extract_error_types(&self, entity: Option<&Entity>, schema: &ResolvedSchema) -> Vec<String> {
        let mut error_types = Vec::new();

        if let Some(entity) = entity {
            for method in &entity.methods {
                if let Some(return_type) = &method.returns {
                    // Look for Result<T, E> patterns
                    let type_str = self.type_to_rust(return_type);
                    if type_str.starts_with("Result<") {
                        // Extract the error type from Result<T, E>
                        if let Some(comma_pos) = type_str.rfind(',') {
                            let error_part = type_str[comma_pos + 1..].trim();
                            let error_type = error_part.trim_end_matches('>').trim();

                            // Check if it's a custom error type (not a standard type)
                            let standard_types = ["String", "&str", "anyhow::Error", "std::io::Error",
                                                   "Box<dyn std::error::Error>", "()"];
                            if !standard_types.contains(&error_type) &&
                               !error_type.starts_with("Vec<") &&
                               !error_type.starts_with("Option<") &&
                               !schema.schema.enums.iter().any(|e| e.name == error_type)
                               && !error_types.contains(&error_type.to_string()) {
                                error_types.push(error_type.to_string());
                            }
                        }
                    }
                }
            }
        }

        error_types
    }

    /// Generate domain error type definitions as enums with common variants
    fn generate_error_types(&self, error_types: &[String], output: &mut String) {
        if error_types.is_empty() {
            return;
        }

        writeln!(output, "use thiserror::Error;").unwrap();
        writeln!(output).unwrap();

        for error_type in error_types {
            writeln!(output, "/// Domain error for {} operations", error_type.to_lowercase().replace("_", " ")).unwrap();
            writeln!(output, "#[derive(Debug, Clone, Error)]").unwrap();
            writeln!(output, "pub enum {} {{", error_type).unwrap();
            writeln!(output, "    #[error(\"{{0}}\")]").unwrap();
            writeln!(output, "    Message(String),").unwrap();
            writeln!(output).unwrap();
            writeln!(output, "    #[error(\"Not found: {{0}}\")]").unwrap();
            writeln!(output, "    NotFound(String),").unwrap();
            writeln!(output).unwrap();
            writeln!(output, "    #[error(\"Validation failed: {{0}}\")]").unwrap();
            writeln!(output, "    ValidationFailed(String),").unwrap();
            writeln!(output).unwrap();
            writeln!(output, "    #[error(\"Conflict: {{0}}\")]").unwrap();
            writeln!(output, "    Conflict(String),").unwrap();
            writeln!(output, "}}").unwrap();
            writeln!(output).unwrap();

            // From<String> conversion for ergonomic error creation
            writeln!(output, "impl From<String> for {} {{", error_type).unwrap();
            writeln!(output, "    fn from(msg: String) -> Self {{ Self::Message(msg) }}").unwrap();
            writeln!(output, "}}").unwrap();
            writeln!(output).unwrap();
            writeln!(output, "impl From<&str> for {} {{", error_type).unwrap();
            writeln!(output, "    fn from(msg: &str) -> Self {{ Self::Message(msg.to_string()) }}").unwrap();
            writeln!(output, "}}").unwrap();
            writeln!(output).unwrap();
        }
    }

    /// Generate a single DDD method from EntityMethod definition
    fn generate_ddd_method(&self, method: &EntityMethod, output: &mut String) {
        writeln!(output).unwrap();

        // Add doc comment if description is provided
        if let Some(desc) = &method.description {
            writeln!(output, "    /// {}", desc).unwrap();
        }

        // Build method signature
        let async_kw = if method.is_async { "async " } else { "" };
        let self_ref = if method.mutates { "&mut self" } else { "&self" };

        // Build parameter list (prefix with _ since body is todo!())
        let params: Vec<String> = method.params.iter()
            .map(|(name, type_ref)| format!("_{}: {}", name, self.type_to_rust(type_ref)))
            .collect();

        let param_str = if params.is_empty() {
            String::new()
        } else {
            format!(", {}", params.join(", "))
        };

        // Build return type
        let return_type = method.returns.as_ref()
            .map(|t| format!(" -> {}", self.type_to_rust(t)))
            .unwrap_or_default();

        // Write method signature
        writeln!(output, "    pub {}fn {}({}{}){}{{", async_kw, method.name, self_ref, param_str, return_type).unwrap();
        writeln!(output, "        // TODO: Implement {} method", method.name).unwrap();
        writeln!(output, "        todo!(\"Implement {}\");", method.name).unwrap();
        writeln!(output, "    }}").unwrap();
    }

    /// Generate Entity trait implementation for a model
    fn generate_entity_trait_impl(&self, model: &Model, output: &mut String) {
        let name = &model.name;
        let pk_field = self.get_pk_field(model);
        let pk_type = self.get_pk_type(model);
        let inner_pk_type = pk_type.trim_start_matches("Option<").trim_end_matches('>');

        writeln!(output).unwrap();
        writeln!(output, "impl super::Entity for {} {{", name).unwrap();
        writeln!(output, "    type Id = {};", inner_pk_type).unwrap();
        writeln!(output).unwrap();
        writeln!(output, "    fn entity_id(&self) -> &Self::Id {{").unwrap();
        if pk_type.starts_with("Option<") {
            writeln!(output, "        self.{}.as_ref().expect(\"Entity must have an ID\")", pk_field).unwrap();
        } else {
            writeln!(output, "        &self.{}", pk_field).unwrap();
        }
        writeln!(output, "    }}").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "    fn entity_type() -> &'static str {{").unwrap();
        writeln!(output, "        \"{}\"", name).unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output, "}}").unwrap();
    }

    /// Generate backbone_core::PersistentEntity implementation for a model
    fn generate_persistent_entity_impl(&self, model: &Model, output: &mut String) {
        let name = &model.name;
        let pk_field = self.get_pk_field(model);
        let has_audit = self.has_audit_metadata(model);

        // Determine if created_at/updated_at are direct fields or via metadata
        let has_direct_created_at = model.fields.iter().any(|f| f.name == "created_at");
        let has_direct_updated_at = model.fields.iter().any(|f| f.name == "updated_at");
        let has_direct_deleted_at = model.fields.iter().any(|f| f.name == "deleted_at");

        writeln!(output).unwrap();
        writeln!(output, "impl backbone_core::PersistentEntity for {} {{", name).unwrap();

        // entity_id — always Uuid-based
        writeln!(output, "    fn entity_id(&self) -> String {{").unwrap();
        writeln!(output, "        self.{}.to_string()", pk_field).unwrap();
        writeln!(output, "    }}").unwrap();

        writeln!(output, "    fn set_entity_id(&mut self, id: String) {{").unwrap();
        writeln!(output, "        if let Ok(uuid) = uuid::Uuid::parse_str(&id) {{").unwrap();
        writeln!(output, "            self.{} = uuid;", pk_field).unwrap();
        writeln!(output, "        }}").unwrap();
        writeln!(output, "    }}").unwrap();

        // created_at
        writeln!(output, "    fn created_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {{").unwrap();
        if has_audit {
            writeln!(output, "        self.metadata.created_at").unwrap();
        } else if has_direct_created_at {
            writeln!(output, "        Some(self.created_at)").unwrap();
        } else {
            writeln!(output, "        None").unwrap();
        }
        writeln!(output, "    }}").unwrap();

        writeln!(output, "    fn set_created_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {{").unwrap();
        if has_audit {
            writeln!(output, "        self.metadata.created_at = Some(ts);").unwrap();
        } else if has_direct_created_at {
            writeln!(output, "        self.created_at = ts;").unwrap();
        } else {
            writeln!(output, "        let _ = ts;").unwrap();
        }
        writeln!(output, "    }}").unwrap();

        // updated_at
        writeln!(output, "    fn updated_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {{").unwrap();
        if has_audit {
            writeln!(output, "        self.metadata.updated_at").unwrap();
        } else if has_direct_updated_at {
            writeln!(output, "        Some(self.updated_at)").unwrap();
        } else {
            writeln!(output, "        None").unwrap();
        }
        writeln!(output, "    }}").unwrap();

        writeln!(output, "    fn set_updated_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {{").unwrap();
        if has_audit {
            writeln!(output, "        self.metadata.updated_at = Some(ts);").unwrap();
        } else if has_direct_updated_at {
            writeln!(output, "        self.updated_at = ts;").unwrap();
        } else {
            writeln!(output, "        let _ = ts;").unwrap();
        }
        writeln!(output, "    }}").unwrap();

        // deleted_at
        writeln!(output, "    fn deleted_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {{").unwrap();
        if has_audit {
            writeln!(output, "        self.metadata.deleted_at").unwrap();
        } else if has_direct_deleted_at {
            writeln!(output, "        self.deleted_at").unwrap();
        } else {
            writeln!(output, "        None").unwrap();
        }
        writeln!(output, "    }}").unwrap();

        writeln!(output, "    fn set_deleted_at(&mut self, ts: Option<chrono::DateTime<chrono::Utc>>) {{").unwrap();
        if has_audit {
            writeln!(output, "        self.metadata.deleted_at = ts;").unwrap();
        } else if has_direct_deleted_at {
            writeln!(output, "        self.deleted_at = ts;").unwrap();
        } else {
            writeln!(output, "        let _ = ts;").unwrap();
        }
        writeln!(output, "    }}").unwrap();

        writeln!(output, "}}").unwrap();
    }

    /// Generate `impl backbone_orm::EntityRepoMeta for {Name}` block.
    ///
    /// `column_types()` — hints for UUID and enum fields so `run_filtered_query`
    /// can emit the right PostgreSQL type casts.
    /// `search_fields()` — text fields eligible for full-text `search=` queries.
    fn generate_entity_repo_meta_impl(
        &self,
        model: &Model,
        enum_names: &std::collections::HashSet<String>,
        output: &mut String,
    ) {
        let name = &model.name;

        // Collect UUID column hints (id + *_id fields whose type is Uuid)
        let uuid_cols: Vec<&str> = model.fields.iter()
            .filter(|f| {
                let is_uuid = match &f.type_ref {
                    TypeRef::Primitive(PrimitiveType::Uuid) => true,
                    TypeRef::Optional(inner) => matches!(inner.as_ref(), TypeRef::Primitive(PrimitiveType::Uuid)),
                    _ => false,
                };
                is_uuid && (f.name == "id" || f.name.ends_with("_id"))
            })
            .map(|f| f.name.as_str())
            .collect();

        // Collect enum column hints: field -> snake_case pg type name
        let enum_cols: Vec<(&str, String)> = model.fields.iter()
            .filter_map(|f| {
                let type_name = match &f.type_ref {
                    TypeRef::Custom(n) => Some(n.as_str()),
                    TypeRef::Optional(inner) => match inner.as_ref() {
                        TypeRef::Custom(n) => Some(n.as_str()),
                        _ => None,
                    },
                    _ => None,
                };
                type_name.and_then(|n| {
                    if enum_names.contains(n) {
                        Some((f.name.as_str(), to_snake_case(n)))
                    } else {
                        None
                    }
                })
            })
            .collect();

        // Collect search fields: text-like primitives
        let search_fields: Vec<&str> = model.fields.iter()
            .filter(|f| matches!(
                f.type_ref,
                TypeRef::Primitive(PrimitiveType::String)
                | TypeRef::Primitive(PrimitiveType::Email)
                | TypeRef::Primitive(PrimitiveType::Slug)
            ))
            .map(|f| f.name.as_str())
            .collect();

        writeln!(output).unwrap();
        writeln!(output, "impl backbone_orm::EntityRepoMeta for {name} {{").unwrap();

        // column_types()
        writeln!(output, "    fn column_types() -> std::collections::HashMap<String, String> {{").unwrap();
        if uuid_cols.is_empty() && enum_cols.is_empty() {
            writeln!(output, "        std::collections::HashMap::new()").unwrap();
        } else {
            writeln!(output, "        let mut m = std::collections::HashMap::new();").unwrap();
            for col in &uuid_cols {
                writeln!(output, "        m.insert(\"{col}\".to_string(), \"uuid\".to_string());").unwrap();
            }
            for (col, pg_type) in &enum_cols {
                writeln!(output, "        m.insert(\"{col}\".to_string(), \"{pg_type}\".to_string());").unwrap();
            }
            writeln!(output, "        m").unwrap();
        }
        writeln!(output, "    }}").unwrap();

        // search_fields()
        writeln!(output, "    fn search_fields() -> &'static [&'static str] {{").unwrap();
        if search_fields.is_empty() {
            writeln!(output, "        &[]").unwrap();
        } else {
            let joined: Vec<String> = search_fields.iter().map(|f| format!("\"{f}\"")).collect();
            writeln!(output, "        &[{}]", joined.join(", ")).unwrap();
        }
        writeln!(output, "    }}").unwrap();

        writeln!(output, "}}").unwrap();
    }

    fn generate_model(&self, model: &Model, schema: &ResolvedSchema) -> Result<String, GenerateError> {
        let mut output = String::new();

        // Find matching Entity definition early (needed for error type extraction)
        let entity = self.find_entity_for_model(model, schema);

        // Detect state machine hook for this model (Phase 2)
        let state_machine = self.find_state_machine_field(model, schema);

        // Extract error types from entity methods
        let error_types = self.extract_error_types(entity, schema);

        // Collect which imports are actually needed
        let mut needs_datetime = false;
        let mut needs_naive_date = false;
        let mut needs_naive_time = false;
        let mut needs_duration = false;
        let mut needs_uuid = false;
        let mut needs_decimal = false;
        let mut needs_other_entities = false;  // For wildcard import of other entities
        let mut needs_value_objects = false;  // For value_objects import
        let needs_audit_metadata = self.has_audit_metadata(model);
        let has_hashed_field = self.has_hashed_field(model);
        let mut custom_types: Vec<String> = Vec::new();

        for field in &model.fields {
            self.collect_type_imports(&field.type_ref, &mut needs_datetime, &mut needs_naive_date,
                                      &mut needs_naive_time, &mut needs_duration, &mut needs_uuid, &mut needs_decimal,
                                      &mut needs_other_entities, &mut needs_value_objects, &mut custom_types, schema);
        }

        // AuditMetadata accessor methods need DateTime<Utc> and Uuid
        if needs_audit_metadata {
            needs_datetime = true;
            needs_uuid = true;
        }

        // Collect imports from entity method parameters and return types
        if let Some(entity) = entity {
            for method in &entity.methods {
                // Collect from parameters
                for (_name, type_ref) in &method.params {
                    self.collect_type_imports(type_ref, &mut needs_datetime, &mut needs_naive_date,
                                              &mut needs_naive_time, &mut needs_duration, &mut needs_uuid, &mut needs_decimal,
                                              &mut needs_other_entities, &mut needs_value_objects, &mut custom_types, schema);
                }
                // Collect from return type
                if let Some(return_type) = &method.returns {
                    self.collect_type_imports(return_type, &mut needs_datetime, &mut needs_naive_date,
                                              &mut needs_naive_time, &mut needs_duration, &mut needs_uuid, &mut needs_decimal,
                                              &mut needs_other_entities, &mut needs_value_objects, &mut custom_types, schema);
                }
            }
        }

        // Write imports based on what's needed
        // Note: argon2 is used with inline `use` statements in generated methods
        // to avoid top-level import conflicts

        let mut chrono_imports = Vec::new();
        if needs_datetime {
            chrono_imports.push("DateTime");
            chrono_imports.push("Utc");
        }
        if needs_duration {
            chrono_imports.push("Duration");
        }
        if needs_naive_date {
            chrono_imports.push("NaiveDate");
        }
        if needs_naive_time {
            chrono_imports.push("NaiveTime");
        }
        if !chrono_imports.is_empty() {
            writeln!(output, "use chrono::{{{}}};", chrono_imports.join(", ")).unwrap();
        }

        writeln!(output, "use serde::{{Deserialize, Serialize}};").unwrap();
        writeln!(output, "use sqlx::FromRow;").unwrap();
        if needs_uuid {
            writeln!(output, "use uuid::Uuid;").unwrap();
        }
        if needs_decimal {
            writeln!(output, "use rust_decimal::Decimal;").unwrap();
        }

        // Note: has_hashed_field is used to determine if password methods should be generated
        let _ = has_hashed_field;

        // Import custom types (enums defined in the same module)
        if !custom_types.is_empty() {
            writeln!(output).unwrap();
            for custom_type in &custom_types {
                writeln!(output, "use super::{};", custom_type).unwrap();
            }
        }

        // Import AuditMetadata for models with @audit_metadata field
        if needs_audit_metadata {
            writeln!(output, "use super::AuditMetadata;").unwrap();
        }

        // Import wildcard for other entity types if referenced in domain methods
        if needs_other_entities {
            writeln!(output).unwrap();
            writeln!(output, "use super::*;").unwrap();
        }

        // Import value_objects if referenced in domain methods
        if needs_value_objects {
            writeln!(output).unwrap();
            writeln!(output, "use crate::domain::value_objects::*;").unwrap();
        }

        // Import state machine types when the model has a state machine hook (Phase 2)
        if state_machine.is_some() {
            writeln!(output).unwrap();
            writeln!(output, "use crate::domain::state_machine::{{{name}StateMachine, {name}State, StateMachineError}};",
                name = model.name).unwrap();
        }

        // Generate error types used in entity methods (before struct definition)
        if !error_types.is_empty() {
            writeln!(output).unwrap();
            self.generate_error_types(&error_types, &mut output);
        }

        // Generate strongly-typed ID newtype (before struct definition)
        let pk_type = self.get_pk_type(model);
        if pk_type == "Uuid" {
            writeln!(output).unwrap();
            self.generate_typed_id(&model.name, &mut output);
        }

        writeln!(output).unwrap();

        // Struct derives
        writeln!(
            output,
            "#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]"
        )
        .unwrap();

        // Struct
        writeln!(output, "pub struct {} {{", model.name).unwrap();

        for field in &model.fields {
            // Use AuditMetadata struct for @audit_metadata fields instead of serde_json::Value
            let rust_type = if field.has_attribute("audit_metadata") {
                "AuditMetadata".to_string()
            } else {
                self.type_to_rust(&field.type_ref)
            };

            // Add serde attributes if needed
            if field.has_attribute("json_name") {
                if let Some(name) = field
                    .get_attribute("json_name")
                    .and_then(|a| a.first_arg())
                    .and_then(|v| v.as_str())
                {
                    writeln!(output, "    #[serde(rename = \"{}\")]", name).unwrap();
                }
            }

            if field.has_attribute("skip_serializing") {
                writeln!(output, "    #[serde(skip_serializing)]").unwrap();
            }

            if field.type_ref.is_optional() {
                writeln!(output, "    #[serde(skip_serializing_if = \"Option::is_none\")]").unwrap();
            }

            // Add sqlx(json) and serde(default) attributes for AuditMetadata JSONB deserialization
            // serde(default) ensures metadata is not required in request bodies (server populates it)
            if field.has_attribute("audit_metadata") {
                writeln!(output, "    #[serde(default)]").unwrap();
                writeln!(output, "    #[sqlx(json)]").unwrap();
            }

            let field_name = escape_rust_keyword(&field.name);
            // State machine field is pub(crate): external code must use transition_to() (Phase 2)
            let visibility = if state_machine.map_or(false, |sm| sm.field == field.name) {
                "pub(crate)"
            } else {
                "pub"
            };
            writeln!(output, "    {} {}: {},", visibility, field_name, rust_type).unwrap();
        }

        writeln!(output, "}}").unwrap();

        // Generate entity methods implementation block (including DDD methods if entity exists)
        // Note: entity was already found at the beginning of this function for error type extraction
        self.generate_entity_methods(model, entity, state_machine, &mut output);

        // Generate backbone_orm::EntityRepoMeta implementation (needs schema for enum detection)
        {
            let enum_names: std::collections::HashSet<String> =
                schema.schema.enums.iter().map(|e| e.name.clone()).collect();
            self.generate_entity_repo_meta_impl(model, &enum_names, &mut output);
        }

        // Generate builder struct for fluent entity construction
        self.generate_builder(model, schema, &mut output);

        Ok(output)
    }

    /// Collect which imports are needed based on field types
    fn collect_type_imports(
        &self,
        type_ref: &TypeRef,
        needs_datetime: &mut bool,
        needs_naive_date: &mut bool,
        needs_naive_time: &mut bool,
        needs_duration: &mut bool,
        needs_uuid: &mut bool,
        needs_decimal: &mut bool,
        needs_other_entities: &mut bool,
        needs_value_objects: &mut bool,
        custom_types: &mut Vec<String>,
        schema: &ResolvedSchema,
    ) {
        use crate::ast::PrimitiveType;

        match type_ref {
            TypeRef::Primitive(p) => match p {
                PrimitiveType::DateTime | PrimitiveType::Timestamp => *needs_datetime = true,
                PrimitiveType::Date => *needs_naive_date = true,
                PrimitiveType::Time => *needs_naive_time = true,
                PrimitiveType::Duration => *needs_duration = true,
                PrimitiveType::Uuid => *needs_uuid = true,
                PrimitiveType::Decimal | PrimitiveType::Money | PrimitiveType::Percentage => {
                    *needs_decimal = true
                }
                _ => {}
            },
            TypeRef::Custom(name) => {
                // Handle common Rust types that may appear as Custom in return types
                match name.as_str() {
                    "Decimal" => *needs_decimal = true,
                    "DateTime" | "Utc" | "NaiveDateTime" => {
                        *needs_datetime = true;
                    }
                    "NaiveDate" => *needs_naive_date = true,
                    "NaiveTime" => *needs_naive_time = true,
                    "Duration" => {
                        *needs_duration = true;
                    }
                    "Uuid" => *needs_uuid = true,
                    // Check if this is an enum defined in the schema
                    _ => {
                        // Check if this is a value object (shared_type or value_object)
                        let is_value_object = schema.schema.shared_types.contains_key(name)
                            || schema.schema.value_objects.iter().any(|vo| &vo.name == name);

                        if is_value_object {
                            *needs_value_objects = true;
                        } else if schema.schema.enums.iter().any(|e| &e.name == name) {
                            let pascal_name = to_pascal_case(name);
                            if !custom_types.contains(&pascal_name) {
                                custom_types.push(pascal_name);
                            }
                        } else {
                            // Other custom types (entities) require wildcard import
                            *needs_other_entities = true;
                        }
                    }
                }
            }
            TypeRef::Optional(inner) => {
                self.collect_type_imports(inner, needs_datetime, needs_naive_date,
                                         needs_naive_time, needs_duration, needs_uuid, needs_decimal, needs_other_entities, needs_value_objects, custom_types, schema);
            }
            TypeRef::Array(inner) => {
                self.collect_type_imports(inner, needs_datetime, needs_naive_date,
                                         needs_naive_time, needs_duration, needs_uuid, needs_decimal, needs_other_entities, needs_value_objects, custom_types, schema);
            }
            TypeRef::Map { key, value } => {
                self.collect_type_imports(key, needs_datetime, needs_naive_date,
                                         needs_naive_time, needs_duration, needs_uuid, needs_decimal, needs_other_entities, needs_value_objects, custom_types, schema);
                self.collect_type_imports(value, needs_datetime, needs_naive_date,
                                         needs_naive_time, needs_duration, needs_uuid, needs_decimal, needs_other_entities, needs_value_objects, custom_types, schema);
            }
            TypeRef::ModuleRef { .. } => {}
        }
    }

    /// Get the set of auto-generated method names for a model
    /// These are reserved and should not be generated from DDD methods
    fn get_reserved_method_names(&self, model: &Model) -> Vec<&'static str> {
        let mut reserved = Vec::new();

        // Always generated: id accessor
        reserved.push("id");

        // Generated if primary key is optional
        if self.get_pk_type(model).starts_with("Option<") {
            reserved.push("is_new");
        }

        // Generated based on timestamp fields
        if self.has_created_at(model) {
            reserved.push("created_at");
        }
        if self.has_updated_at(model) {
            reserved.push("updated_at");
        }

        // Generated for soft delete entities
        if self.has_soft_delete(model) {
            reserved.push("is_deleted");
            reserved.push("is_active");
            reserved.push("deleted_at");
        }

        // Generated for entities with status field
        if model.fields.iter().any(|f| f.name == "status") {
            reserved.push("status");
        }

        // Generated for entities with @hashed password field
        if self.has_hashed_field(model) {
            reserved.push("verify_password");
            reserved.push("hash_password");
        }

        reserved
    }

    fn generate_enum(&self, enum_def: &EnumDef) -> Result<String, GenerateError> {
        let mut output = String::new();

        // Imports - handle conflict when enum name is "Type"
        let enum_name = &enum_def.name;
        if enum_name == "Type" {
            // Use fully qualified path to avoid conflict
            writeln!(output, "use serde::{{Deserialize, Serialize}};").unwrap();
            writeln!(output, "use std::str::FromStr;").unwrap();
            writeln!(output).unwrap();
        } else {
            writeln!(output, "use serde::{{Deserialize, Serialize}};").unwrap();
            writeln!(output, "use sqlx::Type;").unwrap();
            writeln!(output, "use std::str::FromStr;").unwrap();
            writeln!(output).unwrap();
        }

        // Convert to PascalCase first (needed for enum)
        let pascal_enum_name = to_pascal_case(enum_name);
        let escaped_enum_name = escape_rust_keyword(&pascal_enum_name);

        // Enum derives - Type IS a derive macro from sqlx
        writeln!(
            output,
            "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]"
        )
        .unwrap();
        writeln!(output, "#[serde(rename_all = \"snake_case\")]").unwrap();
        // Use the actual PostgreSQL enum type name (snake_case)
        writeln!(output, "#[sqlx(type_name = \"{}\", rename_all = \"snake_case\")]",
                 to_snake_case(enum_name)).unwrap();

        // Enum - use escaped PascalCase name
        writeln!(output, "pub enum {} {{", escaped_enum_name).unwrap();

        for variant in &enum_def.variants {
            let variant_name = to_pascal_case(&variant.name);

            // Check for label attribute
            if let Some(label) = variant
                .attributes
                .iter()
                .find(|a| a.name == "label")
                .and_then(|a| a.first_arg())
                .and_then(|v| v.as_str())
            {
                writeln!(output, "    /// {}", label).unwrap();
            }

            writeln!(output, "    {},", variant_name).unwrap();
        }

        writeln!(output, "}}").unwrap();

        // Implement Display
        writeln!(output).unwrap();
        writeln!(output, "impl std::fmt::Display for {} {{", escaped_enum_name).unwrap();
        writeln!(
            output,
            "    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{"
        )
        .unwrap();
        writeln!(output, "        match self {{").unwrap();

        for variant in &enum_def.variants {
            let variant_name = to_pascal_case(&variant.name);
            let display_name = to_snake_case(&variant.name);
            writeln!(
                output,
                "            Self::{} => write!(f, \"{}\"),",
                variant_name, display_name
            )
            .unwrap();
        }

        writeln!(output, "        }}").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output, "}}").unwrap();

        // Implement FromStr
        writeln!(output).unwrap();
        writeln!(output, "impl FromStr for {} {{", escaped_enum_name).unwrap();
        writeln!(output, "    type Err = String;").unwrap();
        writeln!(output).unwrap();
        writeln!(
            output,
            "    fn from_str(s: &str) -> Result<Self, Self::Err> {{"
        )
        .unwrap();
        writeln!(output, "        match s.to_lowercase().as_str() {{").unwrap();

        for variant in &enum_def.variants {
            let variant_name = to_pascal_case(&variant.name);
            let match_name = to_snake_case(&variant.name);
            writeln!(
                output,
                "            \"{}\" => Ok(Self::{}),",
                match_name, variant_name
            )
            .unwrap();
        }

        writeln!(
            output,
            "            _ => Err(format!(\"Unknown {} variant: {{}}\", s)),",
            enum_def.name
        )
        .unwrap();
        writeln!(output, "        }}").unwrap();
        writeln!(output, "    }}").unwrap();
        writeln!(output, "}}").unwrap();

        // Implement Default if there's a default variant
        if let Some(default_variant) = enum_def
            .variants
            .iter()
            .find(|v| v.attributes.iter().any(|a| a.name == "default"))
        {
            writeln!(output).unwrap();
            writeln!(output, "impl Default for {} {{", escaped_enum_name).unwrap();
            writeln!(output, "    fn default() -> Self {{").unwrap();
            writeln!(
                output,
                "        Self::{}",
                to_pascal_case(&default_variant.name)
            )
            .unwrap();
            writeln!(output, "    }}").unwrap();
            writeln!(output, "}}").unwrap();
        }

        Ok(output)
    }

    fn type_to_rust(&self, type_ref: &TypeRef) -> String {
        match type_ref {
            TypeRef::Primitive(p) => p.rust_type().to_string(),
            TypeRef::Custom(name) => {
                // Preserve Rust primitive types and common types as-is
                match name.as_str() {
                    // Rust primitive types - must be lowercase
                    "u8" | "u16" | "u32" | "u64" | "u128" | "usize" |
                    "i8" | "i16" | "i32" | "i64" | "i128" | "isize" |
                    "f32" | "f64" | "bool" | "str" | "String" |
                    "char" | "()" => name.clone(),
                    // Common Rust types - preserve exact casing
                    "Vec" | "Option" | "Result" | "Box" | "Arc" | "Rc" |
                    "HashMap" | "BTreeMap" | "HashSet" | "BTreeSet" |
                    "DateTime" | "Utc" | "NaiveDate" | "NaiveTime" | "NaiveDateTime" |
                    "Duration" | "Uuid" | "Decimal" | "Value" => name.clone(),
                    // Custom types - convert to PascalCase
                    _ => to_pascal_case(name),
                }
            }
            TypeRef::Array(inner) => format!("Vec<{}>", self.type_to_rust(inner)),
            TypeRef::Optional(inner) => format!("Option<{}>", self.type_to_rust(inner)),
            TypeRef::Map { key, value } => {
                format!(
                    "std::collections::HashMap<{}, {}>",
                    self.type_to_rust(key),
                    self.type_to_rust(value)
                )
            }
            TypeRef::ModuleRef { module, name } => format!("{}::{}", module, name),
        }
    }
}

impl Default for RustGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl Generator for RustGenerator {
    fn generate(&self, schema: &ResolvedSchema) -> Result<GeneratedOutput, GenerateError> {
        let mut output = GeneratedOutput::new();

        // Generate model files
        for model in &schema.schema.models {
            let content = self.generate_model(model, schema)?;
            let path = PathBuf::from(format!(
                "src/domain/entity/{}.rs",
                to_snake_case(&model.name)
            ));
            output.add_file(path, content);
        }

        // Generate enum files
        for enum_def in &schema.schema.enums {
            let content = self.generate_enum(enum_def)?;
            let path = PathBuf::from(format!(
                "src/domain/entity/{}.rs",
                to_snake_case(&enum_def.name)
            ));
            output.add_file(path, content);
        }

        // Generate mod.rs
        let mut mod_content = String::new();

        writeln!(mod_content, "//! Domain Entities").unwrap();
        writeln!(mod_content, "//!").unwrap();
        writeln!(mod_content, "//! Generated by metaphor-schema. Do not edit manually.").unwrap();
        writeln!(mod_content).unwrap();

        // Collect unique module names (models + enums) to avoid duplicates
        use std::collections::HashSet;
        let mut module_names = HashSet::new();

        // Module declarations with deduplication and keyword escaping
        // All entity modules are public to allow external access
        for model in &schema.schema.models {
            let snake_name = to_snake_case(&model.name);
            let escaped_name = escape_rust_keyword(&snake_name);
            if module_names.insert(escaped_name.clone()) {
                writeln!(mod_content, "pub mod {};", escaped_name).unwrap();
            }
        }
        for enum_def in &schema.schema.enums {
            let snake_name = to_snake_case(&enum_def.name);
            let escaped_name = escape_rust_keyword(&snake_name);
            if module_names.insert(escaped_name.clone()) {
                writeln!(mod_content, "pub mod {};", escaped_name).unwrap();
            }
        }
        writeln!(mod_content).unwrap();

        // Re-exports with deduplication and keyword escaping
        writeln!(mod_content, "// Re-exports").unwrap();
        let mut reexports = HashSet::new();
        for model in &schema.schema.models {
            let snake_name = to_snake_case(&model.name);
            let escaped_name = escape_rust_keyword(&snake_name);
            let reexport_line = format!("pub use {}::{};", escaped_name, model.name);
            if reexports.insert(reexport_line.clone()) {
                writeln!(mod_content, "{}", reexport_line).unwrap();
            }
            // Re-export builder
            let builder_reexport = format!("pub use {}::{}Builder;", escaped_name, model.name);
            if reexports.insert(builder_reexport.clone()) {
                writeln!(mod_content, "{}", builder_reexport).unwrap();
            }
            // Re-export typed ID (only for models with Uuid primary key)
            let pk_type = self.get_pk_type(model);
            if pk_type == "Uuid" {
                let id_reexport = format!("pub use {}::{}Id;", escaped_name, model.name);
                if reexports.insert(id_reexport.clone()) {
                    writeln!(mod_content, "{}", id_reexport).unwrap();
                }
            }
        }
        for enum_def in &schema.schema.enums {
            let snake_name = to_snake_case(&enum_def.name);
            let pascal_name = to_pascal_case(&enum_def.name);
            let escaped_name = escape_rust_keyword(&snake_name);
            let reexport_line = format!("pub use {}::{};", escaped_name, pascal_name);
            if reexports.insert(reexport_line.clone()) {
                writeln!(mod_content, "{}", reexport_line).unwrap();
            }
        }
        writeln!(mod_content).unwrap();

        // Entity trait
        writeln!(mod_content, "// ==========================================================================").unwrap();
        writeln!(mod_content, "// Entity Trait").unwrap();
        writeln!(mod_content, "// ==========================================================================").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "use std::fmt::Debug;").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "/// Trait for domain entities").unwrap();
        writeln!(mod_content, "///").unwrap();
        writeln!(mod_content, "/// All generated entities implement this trait, providing").unwrap();
        writeln!(mod_content, "/// a common interface for working with domain objects.").unwrap();
        writeln!(mod_content, "pub trait Entity: Debug + Clone {{").unwrap();
        writeln!(mod_content, "    /// The type of the entity's unique identifier").unwrap();
        writeln!(mod_content, "    type Id: Debug + Clone + PartialEq;").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "    /// Get the entity's unique identifier").unwrap();
        writeln!(mod_content, "    fn entity_id(&self) -> &Self::Id;").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "    /// Get the entity type name").unwrap();
        writeln!(mod_content, "    fn entity_type() -> &'static str;").unwrap();
        writeln!(mod_content, "}}").unwrap();
        writeln!(mod_content).unwrap();

        // AuditMetadata struct for JSONB audit fields
        writeln!(mod_content, "// ==========================================================================").unwrap();
        writeln!(mod_content, "// Audit Metadata").unwrap();
        writeln!(mod_content, "// ==========================================================================").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "use chrono::{{DateTime, Utc}};").unwrap();
        writeln!(mod_content, "use serde::{{Deserialize, Serialize}};").unwrap();
        writeln!(mod_content, "use uuid::Uuid;").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "/// Audit metadata stored as JSONB in the database").unwrap();
        writeln!(mod_content, "///").unwrap();
        writeln!(mod_content, "/// This struct consolidates audit fields (timestamps and actors) into a single").unwrap();
        writeln!(mod_content, "/// JSONB column for efficient storage and flexible querying.").unwrap();
        writeln!(mod_content, "#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]").unwrap();
        writeln!(mod_content, "pub struct AuditMetadata {{").unwrap();
        writeln!(mod_content, "    /// Timestamp when the record was created").unwrap();
        writeln!(mod_content, "    #[serde(skip_serializing_if = \"Option::is_none\")]").unwrap();
        writeln!(mod_content, "    pub created_at: Option<DateTime<Utc>>,").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "    /// Timestamp when the record was last updated").unwrap();
        writeln!(mod_content, "    #[serde(skip_serializing_if = \"Option::is_none\")]").unwrap();
        writeln!(mod_content, "    pub updated_at: Option<DateTime<Utc>>,").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "    /// Timestamp when the record was soft-deleted").unwrap();
        writeln!(mod_content, "    #[serde(skip_serializing_if = \"Option::is_none\")]").unwrap();
        writeln!(mod_content, "    pub deleted_at: Option<DateTime<Utc>>,").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "    /// User ID who created the record").unwrap();
        writeln!(mod_content, "    #[serde(skip_serializing_if = \"Option::is_none\")]").unwrap();
        writeln!(mod_content, "    pub created_by: Option<Uuid>,").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "    /// User ID who last updated the record").unwrap();
        writeln!(mod_content, "    #[serde(skip_serializing_if = \"Option::is_none\")]").unwrap();
        writeln!(mod_content, "    pub updated_by: Option<Uuid>,").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "    /// User ID who deleted the record").unwrap();
        writeln!(mod_content, "    #[serde(skip_serializing_if = \"Option::is_none\")]").unwrap();
        writeln!(mod_content, "    pub deleted_by: Option<Uuid>,").unwrap();
        writeln!(mod_content, "}}").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "impl AuditMetadata {{").unwrap();
        writeln!(mod_content, "    /// Create new audit metadata with created_at set to now").unwrap();
        writeln!(mod_content, "    pub fn new() -> Self {{").unwrap();
        writeln!(mod_content, "        Self {{").unwrap();
        writeln!(mod_content, "            created_at: Some(Utc::now()),").unwrap();
        writeln!(mod_content, "            ..Default::default()").unwrap();
        writeln!(mod_content, "        }}").unwrap();
        writeln!(mod_content, "    }}").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "    /// Create with creator ID").unwrap();
        writeln!(mod_content, "    pub fn with_creator(creator_id: Uuid) -> Self {{").unwrap();
        writeln!(mod_content, "        Self {{").unwrap();
        writeln!(mod_content, "            created_at: Some(Utc::now()),").unwrap();
        writeln!(mod_content, "            created_by: Some(creator_id),").unwrap();
        writeln!(mod_content, "            ..Default::default()").unwrap();
        writeln!(mod_content, "        }}").unwrap();
        writeln!(mod_content, "    }}").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "    /// Touch the record (update updated_at)").unwrap();
        writeln!(mod_content, "    pub fn touch(&mut self) {{").unwrap();
        writeln!(mod_content, "        self.updated_at = Some(Utc::now());").unwrap();
        writeln!(mod_content, "    }}").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "    /// Touch with updater ID").unwrap();
        writeln!(mod_content, "    pub fn touch_by(&mut self, updater_id: Uuid) {{").unwrap();
        writeln!(mod_content, "        self.updated_at = Some(Utc::now());").unwrap();
        writeln!(mod_content, "        self.updated_by = Some(updater_id);").unwrap();
        writeln!(mod_content, "    }}").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "    /// Soft delete the record").unwrap();
        writeln!(mod_content, "    pub fn soft_delete(&mut self) {{").unwrap();
        writeln!(mod_content, "        self.deleted_at = Some(Utc::now());").unwrap();
        writeln!(mod_content, "    }}").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "    /// Soft delete with deleter ID").unwrap();
        writeln!(mod_content, "    pub fn soft_delete_by(&mut self, deleter_id: Uuid) {{").unwrap();
        writeln!(mod_content, "        self.deleted_at = Some(Utc::now());").unwrap();
        writeln!(mod_content, "        self.deleted_by = Some(deleter_id);").unwrap();
        writeln!(mod_content, "    }}").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "    /// Restore a soft-deleted record").unwrap();
        writeln!(mod_content, "    pub fn restore(&mut self) {{").unwrap();
        writeln!(mod_content, "        self.deleted_at = None;").unwrap();
        writeln!(mod_content, "        self.deleted_by = None;").unwrap();
        writeln!(mod_content, "    }}").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "    /// Check if record is deleted").unwrap();
        writeln!(mod_content, "    pub fn is_deleted(&self) -> bool {{").unwrap();
        writeln!(mod_content, "        self.deleted_at.is_some()").unwrap();
        writeln!(mod_content, "    }}").unwrap();
        writeln!(mod_content).unwrap();
        writeln!(mod_content, "    /// Check if record is active (not deleted)").unwrap();
        writeln!(mod_content, "    pub fn is_active(&self) -> bool {{").unwrap();
        writeln!(mod_content, "        self.deleted_at.is_none()").unwrap();
        writeln!(mod_content, "    }}").unwrap();
        writeln!(mod_content, "}}").unwrap();

        output.add_file(PathBuf::from("src/domain/entity/mod.rs"), mod_content);

        Ok(output)
    }

    fn name(&self) -> &'static str {
        "rust"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Attribute, Field, ModuleSchema, PrimitiveType};
    use crate::ast::hook::{Hook, StateMachine, State, Transition};

    fn make_model_with_status() -> Model {
        let mut model = Model::new("Payment");
        model.fields = vec![
            Field {
                name: "id".to_string(),
                type_ref: TypeRef::Primitive(PrimitiveType::Uuid),
                attributes: vec![Attribute::new("id")],
                ..Default::default()
            },
            Field {
                name: "status".to_string(),
                type_ref: TypeRef::Custom("PaymentStatus".to_string()),
                attributes: vec![],
                ..Default::default()
            },
            Field {
                name: "amount".to_string(),
                type_ref: TypeRef::Primitive(PrimitiveType::Decimal),
                attributes: vec![],
                ..Default::default()
            },
        ];
        model
    }

    fn make_schema_with_state_machine() -> ResolvedSchema {
        let mut schema = ModuleSchema::new("test");
        schema.models.push(make_model_with_status());

        let mut hook = Hook::new("Payment", "Payment");
        hook.state_machine = Some(StateMachine {
            field: "status".to_string(),
            states: vec![
                State { name: "pending".to_string(), initial: true, ..Default::default() },
                State { name: "paid".to_string(), ..Default::default() },
            ],
            transitions: vec![
                Transition::new("pay", vec!["pending".to_string()], "paid"),
            ],
            span: Default::default(),
        });
        schema.hooks.push(hook);

        ResolvedSchema { schema }
    }

    #[test]
    fn test_state_machine_generates_transition_to() {
        let schema = make_schema_with_state_machine();
        let generator = RustGenerator::new();
        let output = generator.generate(&schema).unwrap();

        let payment_file = output.files.get(&PathBuf::from("src/domain/entity/payment.rs"))
            .expect("payment.rs should be generated");

        assert!(
            payment_file.contains("pub fn transition_to("),
            "Expected transition_to() method in payment.rs"
        );
        assert!(
            payment_file.contains("PaymentStateMachine::new(self.status)"),
            "Expected PaymentStateMachine::new call"
        );
        assert!(
            payment_file.contains("sm.transition(new_state)"),
            "Expected sm.transition call"
        );
        assert!(
            payment_file.contains("use crate::domain::state_machine::PaymentStateMachine"),
            "Expected PaymentStateMachine import"
        );
    }

    #[test]
    fn test_state_machine_field_skipped_in_apply_patch() {
        let schema = make_schema_with_state_machine();
        let generator = RustGenerator::new();
        let output = generator.generate(&schema).unwrap();

        let payment_file = output.files.get(&PathBuf::from("src/domain/entity/payment.rs"))
            .expect("payment.rs should be generated");

        // apply_patch should NOT contain a direct assignment to status
        // (the status arm should be absent from the match block)
        let patch_section = payment_file.find("pub fn apply_patch")
            .map(|start| &payment_file[start..start + 600])
            .unwrap_or("");

        assert!(
            !patch_section.contains("\"status\""),
            "apply_patch should not include a direct match arm for the state machine field 'status'"
        );
    }

    #[test]
    fn test_state_machine_field_is_pub_crate() {
        let schema = make_schema_with_state_machine();
        let generator = RustGenerator::new();
        let output = generator.generate(&schema).unwrap();

        let payment_file = output.files.get(&PathBuf::from("src/domain/entity/payment.rs"))
            .expect("payment.rs should be generated");

        assert!(
            payment_file.contains("pub(crate) status:"),
            "State machine field 'status' should be pub(crate) in struct definition"
        );
    }

    #[test]
    fn test_no_state_machine_keeps_pub_and_no_transition_to() {
        let mut module_schema = ModuleSchema::new("test");
        let mut model = Model::new("Product");
        model.fields = vec![
            Field {
                name: "id".to_string(),
                type_ref: TypeRef::Primitive(PrimitiveType::Uuid),
                attributes: vec![Attribute::new("id")],
                ..Default::default()
            },
            Field {
                name: "name".to_string(),
                type_ref: TypeRef::Primitive(PrimitiveType::String),
                attributes: vec![],
                ..Default::default()
            },
        ];
        module_schema.models.push(model);
        let schema = ResolvedSchema { schema: module_schema };

        let generator = RustGenerator::new();
        let output = generator.generate(&schema).unwrap();

        let product_file = output.files.get(&PathBuf::from("src/domain/entity/product.rs"))
            .expect("product.rs should be generated");

        assert!(
            !product_file.contains("transition_to"),
            "Models without state machine should not have transition_to()"
        );
        assert!(
            product_file.contains("pub name:"),
            "Regular fields should remain pub"
        );
    }
}
