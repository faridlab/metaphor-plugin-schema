//! The relations an entity's list and get routes will expand for `?include=<name>`.
//!
//! One function decides them, and both artifacts that state them read it: the Rust entity's
//! `EntityRepoMeta::relations()` (what the service enforces, refusing any other name with a 400
//! `invalid_query`, reason `unknown_relation`) and the webapp schema's
//! `<entity>IncludableRelations` export (what a screen may ask for). Keeping them on one function
//! means a screen cannot offer a name the service refuses.

use crate::ast::model::RelationType;
use crate::ast::{AttributeValue, Model, ModuleSchema, TypeRef};
use crate::webgen::parser::to_camel_case;

/// One expandable relation, as the service carries it in `relations()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludableRelation {
    /// The name in `?include=<name>`: camelCase, as the response carries the expanded object.
    pub name: String,
    /// The target's table (`collection`, unqualified — the repository qualifies it).
    pub table: String,
    /// The local foreign-key field, camelCase.
    pub fk: String,
    /// The target model of a same-module relation. `None` for a cross-module `@include`, whose
    /// target is only a table name here.
    pub target: Option<String>,
}

/// The relations of `model` that `?include=` may expand, in the order `relations()` lists them.
///
/// Two sources, and only these two:
///
/// 1. a to-one `relations:` entry whose target is a model of this module and which names its key
///    with `@foreign_key` (a bare key name counts) — a relation without a `@foreign_key` cannot
///    be expanded, so it is not listed;
/// 2. a field carrying `@include("name", "schema.table")`, the way a key into another module's
///    table (which is no relation here) declares what `?include=` may hydrate.
///
/// A foreign-key field that is neither — a key into another module with no `@include`, or a key
/// whose relation the schema never declares — is not includable.
pub fn includable_relations(schema: &ModuleSchema, model: &Model) -> Vec<IncludableRelation> {
    let collections: std::collections::HashMap<&str, String> = schema
        .models
        .iter()
        .map(|m| (m.name.as_str(), m.collection_name()))
        .collect();

    let mut relations: Vec<IncludableRelation> = model
        .relations
        .iter()
        .filter(|r| matches!(r.relation_type, RelationType::One))
        .filter_map(|r| {
            let target = match &r.target {
                TypeRef::Custom(n) => Some(n.as_str()),
                TypeRef::Optional(inner) => match inner.as_ref() {
                    TypeRef::Custom(n) => Some(n.as_str()),
                    _ => None,
                },
                _ => None,
            }?;
            let table = collections.get(target)?.clone();
            // Read the relation's @foreign_key locally — a bare FK name parses as `Ident`,
            // which the shared `Relation::foreign_key()` ignores (and we don't want to widen
            // it: that would change FK-constraint emission in migration generation). Accept
            // String or Ident here.
            let fk = r
                .attributes
                .iter()
                .find(|a| a.name == "foreign_key")
                .and_then(|a| a.args.first())
                .and_then(|(_, v)| match v {
                    AttributeValue::String(s) | AttributeValue::Ident(s) => Some(s.clone()),
                    _ => None,
                })?;
            Some(IncludableRelation {
                name: to_camel_case(&r.name),
                table,
                fk: to_camel_case(&fk),
                target: Some(target.to_string()),
            })
        })
        .collect();

    for field in &model.fields {
        let Some(attr) = field.attributes.iter().find(|a| a.name == "include") else { continue };
        let text = |i: usize| match attr.args.get(i).map(|(_, v)| v) {
            Some(AttributeValue::String(s) | AttributeValue::Ident(s)) => Some(s.clone()),
            _ => None,
        };
        if let (Some(rel), Some(table)) = (text(0), text(1)) {
            relations.push(IncludableRelation {
                name: to_camel_case(&rel),
                table,
                fk: to_camel_case(&field.name),
                target: None,
            });
        }
    }
    relations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Attribute, Field, PrimitiveType};
    use crate::generators::rust::RustGenerator;
    use crate::generators::Generator;
    use crate::resolver::ResolvedSchema;
    use std::path::PathBuf;

    fn id() -> Field {
        Field {
            name: "id".to_string(),
            type_ref: TypeRef::Primitive(PrimitiveType::Uuid),
            attributes: vec![Attribute::new("id")],
            ..Default::default()
        }
    }

    fn uuid_field(name: &str, attributes: Vec<Attribute>) -> Field {
        Field {
            name: name.to_string(),
            type_ref: TypeRef::Primitive(PrimitiveType::Uuid),
            attributes,
            ..Default::default()
        }
    }

    fn to_one(name: &str, target: &str, foreign_key: Option<AttributeValue>) -> crate::ast::Relation {
        crate::ast::Relation {
            name: name.to_string(),
            target: TypeRef::Custom(target.to_string()),
            relation_type: RelationType::One,
            attributes: foreign_key
                .map(|v| vec![Attribute::new("foreign_key").with_arg(v)])
                .unwrap_or_default(),
            ..Default::default()
        }
    }

    /// `Project`, `Ticket` (a declared relation to Project, and a key into another module with no
    /// `@include`), `Timesheet` (a key into another module that declares `@include`), and
    /// `Stage` (a relation naming no `@foreign_key`, a relation to a model that is not in the
    /// module, and a to-many).
    fn schema() -> ModuleSchema {
        let mut project = Model::new("Project");
        project.fields = vec![id()];
        let mut ticket = Model::new("Ticket");
        ticket.fields = vec![id(), uuid_field("project_id", vec![]), uuid_field("owner_id", vec![])];
        ticket.relations.push(to_one(
            "project",
            "Project",
            Some(AttributeValue::Ident("project_id".to_string())),
        ));
        let mut timesheet = Model::new("Timesheet");
        timesheet.fields = vec![
            id(),
            uuid_field(
                "employee_id",
                vec![Attribute::new("include")
                    .with_arg(AttributeValue::String("employee".to_string()))
                    .with_arg(AttributeValue::String("employee.employees".to_string()))],
            ),
        ];
        let mut stage = Model::new("Stage");
        stage.fields = vec![id(), uuid_field("project_id", vec![]), uuid_field("approver_id", vec![])];
        stage.relations.push(to_one("project", "Project", None));
        stage.relations.push(to_one(
            "approver",
            "Person",
            Some(AttributeValue::String("approver_id".to_string())),
        ));
        let mut many = to_one("tickets", "Ticket", Some(AttributeValue::Ident("project_id".to_string())));
        many.relation_type = RelationType::Many;
        stage.relations.push(many);
        let mut schema = ModuleSchema::new("test");
        schema.models = vec![project, ticket, timesheet, stage];
        schema
    }

    fn names(schema: &ModuleSchema, model: &str) -> Vec<String> {
        let model = schema.models.iter().find(|m| m.name == model).unwrap();
        includable_relations(schema, model).into_iter().map(|r| r.name).collect()
    }

    #[test]
    fn a_declared_relation_is_includable_under_its_camel_case_name() {
        let s = schema();
        let ticket = s.models.iter().find(|m| m.name == "Ticket").unwrap();
        assert_eq!(
            includable_relations(&s, ticket),
            vec![IncludableRelation {
                name: "project".into(),
                table: "projects".into(),
                fk: "projectId".into(),
                target: Some("Project".into()),
            }]
        );
    }

    #[test]
    fn a_cross_module_key_is_includable_only_when_it_declares_include() {
        let s = schema();
        // `Ticket.owner_id` points into another module and declares nothing: not includable.
        assert!(!names(&s, "Ticket").contains(&"owner".to_string()));
        // `Timesheet.employee_id` declares it: includable, with no same-module target.
        let timesheet = s.models.iter().find(|m| m.name == "Timesheet").unwrap();
        assert_eq!(
            includable_relations(&s, timesheet),
            vec![IncludableRelation {
                name: "employee".into(),
                table: "employee.employees".into(),
                fk: "employeeId".into(),
                target: None,
            }]
        );
    }

    #[test]
    fn a_relation_the_service_could_not_expand_is_not_listed() {
        let s = schema();
        // No @foreign_key, a target outside the module, and a to-many: none can be hydrated.
        assert!(names(&s, "Stage").is_empty(), "{:?}", names(&s, "Stage"));
    }

    #[test]
    fn the_rust_entity_and_the_webapp_export_state_the_same_names() {
        let s = schema();
        let output = RustGenerator::new().generate(&ResolvedSchema { schema: s.clone() }).unwrap();
        for model in &s.models {
            let file = output
                .files
                .get(&PathBuf::from(format!("src/domain/entity/{}.rs", crate::utils::to_snake_case(&model.name))))
                .unwrap_or_else(|| panic!("{} entity file", model.name));
            let from_rust: Vec<String> = match file.split("fn relations()").nth(1) {
                Some(body) => {
                    let list = body.split_once("&[").map(|(_, r)| r).unwrap_or("");
                    let list = list.split_once("]\n").map(|(l, _)| l).unwrap_or("");
                    list.split("(\"").skip(1).filter_map(|t| t.split('"').next()).map(str::to_string).collect()
                }
                None => Vec::new(),
            };
            assert_eq!(from_rust, names(&s, &model.name), "{}", model.name);
        }
    }
}
