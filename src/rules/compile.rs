//! Compile a declared row rule into the code that checks it.
//!
//! A row rule is a condition over one row's own fields, checked on create,
//! update or both: no other row and no relation. It may also read the row
//! before the write (`$old.x`, a transition rule), the writer (`$actor.id`) or
//! the time of the write (`now()`) — a context rule. A row-layer rule compiles
//! to two forms that give the same answer:
//!
//! - a Rust expression over `backbone_core::row_rule`, evaluated by the
//!   entity's generated write guard against the row a write would store;
//! - an SQL expression over the table's columns, used now by the audit query
//!   that counts the stored rows breaking the rule, and later by the rule's
//!   `CHECK` constraint.
//!
//! Both follow SQL's semantics: an operator over a null is null, and a rule
//! holds unless it is definitely false. `x == null` and `x != null` test for
//! null itself. A transition or context rule has only the Rust form: no `CHECK`
//! can read the row before the write, the writer or the moment of the write. A
//! context rule about time checks an update only when the update changes a
//! field the rule reads. `$actor.has_role(..)` needs a lookup into
//! authorization data and is not compiled here. Anything else outside the
//! subset is not compiled and says why.

use std::collections::HashSet;

use crate::ast::expressions::{BinaryOp, Expression, FieldRef, Literal, UnaryOp};
use crate::ast::{Model, PrimitiveType, TypeRef};

use super::expr::{self, ExpressionCheck};
use super::{DeclaredRule, RuleBody};

/// One row rule, compiled.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledRowRule {
    pub name: String,
    /// The stable code a violation carries.
    pub code: String,
    pub message: String,
    /// The field a violation names: the first one the rule reads.
    pub path: String,
    pub on_create: bool,
    pub on_update: bool,
    /// A Rust expression of type `backbone_core::row_rule::Val`, reading the row as `row`,
    /// the row before the write as `old_row` and the write's context as `ctx`.
    pub rust: String,
    /// The same rule as an SQL boolean expression over the table's columns; `None` for a
    /// transition or context rule, which no `CHECK` can express.
    pub sql: Option<String>,
    /// `row`, `transition` (reads `$old`) or `context` (reads the writer or the time).
    pub layer: &'static str,
    /// Whether the rule reads the row before the write.
    pub reads_old: bool,
    /// For a rule about the time of the write, the fields it reads: an update is checked
    /// only when it changes one of them. Empty for every other rule.
    pub time_fields: Vec<String>,
    /// Where the rule is declared, for the generated comment.
    pub source: String,
}

/// What a field holds, as far as a rule can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    /// A Postgres enum: compared as its text label.
    Enum,
    Number,
    Bool,
    /// A date or time, compared in its own order.
    Time,
    List,
}

fn kind_of(t: &TypeRef, enums: &HashSet<String>) -> Option<Kind> {
    use PrimitiveType as P;
    match t {
        TypeRef::Optional(inner) => kind_of(inner, enums),
        TypeRef::Array(_) => Some(Kind::List),
        TypeRef::Custom(name) if enums.contains(name) => Some(Kind::Enum),
        TypeRef::Primitive(p) => match p {
            P::Int | P::Int32 | P::Int64 | P::Float | P::Float32 | P::Float64 | P::Decimal | P::Money
            | P::Percentage => Some(Kind::Number),
            P::Bool => Some(Kind::Bool),
            P::DateTime | P::Date | P::Time | P::Timestamp => Some(Kind::Time),
            P::String | P::Uuid | P::Email | P::Url | P::Phone | P::Slug | P::Ip | P::IpV4 | P::IpV6
            | P::Mac | P::Markdown | P::Html => Some(Kind::Text),
            _ => None,
        },
        _ => None,
    }
}

/// Compile `rule` for `model`, or say why it is not a row rule this compiler takes.
pub fn compile_row_rule(rule: &DeclaredRule, model: &Model, enums: &HashSet<String>) -> Result<CompiledRowRule, String> {
    let text = match &rule.body {
        RuleBody::Condition(t) | RuleBody::Assertion(t) => t,
        _ => return Err("not an expression".into()),
    };
    if rule.expression != ExpressionCheck::Ready {
        return Err(format!("expression is {}", rule.expression.label()));
    }
    let on_create = rule.when.iter().any(|w| w == "create");
    let on_update = rule.when.iter().any(|w| w == "update");
    if rule.when.is_empty() || rule.when.iter().any(|w| w != "create" && w != "update") {
        return Err(format!("applies on {:?}, not only on create and update", rule.when));
    }
    let ast = expr::parse(text)?;
    let mut c = Compiler { model, enums, fields: Vec::new(), row_var: "row", reads_old: false, reads_actor: false, reads_time: false };
    let (rust, sql, _) = c.emit(&ast)?;
    let layer = if c.reads_actor || c.reads_time {
        "context"
    } else if c.reads_old {
        "transition"
    } else {
        "row"
    };
    let sql = (layer == "row").then_some(sql);
    let time_fields = if c.reads_time { c.fields.clone() } else { Vec::new() };
    let path = c.fields.first().cloned().unwrap_or_default();
    let code = code_of(rule.code.as_deref().unwrap_or(&rule.name));
    let message = rule.message.clone().unwrap_or_else(|| format!("breaks the rule `{}`: {}", rule.name, text));
    Ok(CompiledRowRule {
        name: rule.name.clone(),
        code,
        message,
        path,
        on_create,
        on_update,
        rust,
        sql,
        layer,
        reads_old: c.reads_old,
        time_fields,
        source: rule.source.to_string(),
    })
}

/// A stable snake_case code from a rule's name: `Name Length` and `nameLength` both read `name_length`.
fn code_of(name: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() && prev_lower {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            prev_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        } else {
            if !out.ends_with('_') && !out.is_empty() {
                out.push('_');
            }
            prev_lower = false;
        }
    }
    out.trim_end_matches('_').to_string()
}

/// Every row rule of `model` that compiles, in declaration order.
pub fn row_rules_of(rules: &[DeclaredRule], model: &Model, enums: &HashSet<String>) -> Vec<CompiledRowRule> {
    rules
        .iter()
        .filter(|r| r.bound_model.as_deref() == Some(model.name.as_str()))
        .filter_map(|r| compile_row_rule(r, model, enums).ok())
        .collect()
}

/// The row rules the generator compiles for `model` of `schema`.
pub fn model_row_rules(schema: &crate::ast::ModuleSchema, model: &Model) -> Vec<CompiledRowRule> {
    let enums: HashSet<String> = schema.enums.iter().map(|e| e.name.clone()).collect();
    row_rules_of(&schema.rule_register.rules, model, &enums)
}

/// A uniqueness rule written as a count over the rule's own entity, compiled.
///
/// `count(Bucket, slug == $this.slug && id != $this.id && metadata.deleted_at == null) == 0`
/// says no other live bucket shares the slug: a partial unique index on `slug` where the
/// filter holds, emitted when the rule is enforced. Until then the audit lists the stored
/// duplicates. The write guard does not check it: that would need a query on every write.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledUnique {
    pub name: String,
    pub code: String,
    /// The fields that must not repeat together (`f == $this.f`).
    pub keys: Vec<String>,
    /// The rows the uniqueness covers, as an SQL condition: the index's `WHERE`.
    pub filter_sql: Option<String>,
    pub source: String,
}

/// Compile a `count(<this entity>, …) == 0` rule into a uniqueness rule, or say why not.
pub fn compile_uniqueness(rule: &DeclaredRule, model: &Model, enums: &HashSet<String>) -> Result<CompiledUnique, String> {
    let text = match &rule.body {
        RuleBody::Condition(t) | RuleBody::Assertion(t) => t,
        _ => return Err("not an expression".into()),
    };
    let ast = expr::parse(text)?;
    let Expression::Binary { left, op: BinaryOp::Eq, right } = &ast else {
        return Err("not `count(..) == 0`".into());
    };
    let (Expression::FunctionCall { name, args }, Expression::Literal(Literal::Int(0))) = (left.as_ref(), right.as_ref()) else {
        return Err("not `count(..) == 0`".into());
    };
    let [Expression::FieldRef(FieldRef::Simple(entity)), predicate] = (name == "count").then_some(args.as_slice()).unwrap_or(&[]) else {
        return Err("not `count(<entity>, <predicate>) == 0`".into());
    };
    if entity != &model.name {
        return Err(format!("counts `{entity}`, not `{}`: a lookup into another entity", model.name));
    }
    let mut terms = Vec::new();
    conjuncts(predicate, &mut terms);
    let this_field = |e: &Expression| match e {
        Expression::MemberAccess { object, member } if matches!(object.as_ref(), Expression::Context(c) if c == "this") => Some(member.clone()),
        _ => None,
    };
    let plain_field = |e: &Expression| match e {
        Expression::FieldRef(FieldRef::Simple(f)) => Some(f.clone()),
        _ => None,
    };
    let mut keys = Vec::new();
    let mut filters = Vec::new();
    for term in terms {
        match term {
            Expression::Binary { left, op, right } if matches!(op, BinaryOp::Eq | BinaryOp::Ne) => {
                let pair = plain_field(left)
                    .zip(this_field(right))
                    .or_else(|| plain_field(right).zip(this_field(left)));
                match (pair, op) {
                    (Some((f, g)), BinaryOp::Eq) if f == g => {
                        if model.find_field(&f).is_none() {
                            return Err(format!("`{f}` is not a field of {}", model.name));
                        }
                        keys.push(f);
                    }
                    // `id != $this.id`: the row itself is not a duplicate; an index implies it.
                    (Some((f, g)), BinaryOp::Ne) if f == g && f == "id" => {}
                    (Some(_), _) => return Err("compares a field with another field of the row being written".into()),
                    (None, _) => filters.push(term),
                }
            }
            other => filters.push(other),
        }
    }
    if keys.is_empty() {
        return Err("names no field that must not repeat (`f == $this.f`)".into());
    }
    let mut c = Compiler { model, enums, fields: Vec::new(), row_var: "row", reads_old: false, reads_actor: false, reads_time: false };
    let mut sqls = Vec::new();
    for f in filters {
        let (_, sql, _) = c.emit(f)?;
        if c.reads_old || c.reads_actor || c.reads_time {
            return Err("its filter reads the writer, the time or the row being written, which an index cannot".into());
        }
        sqls.push(sql);
    }
    Ok(CompiledUnique {
        name: rule.name.clone(),
        code: code_of(rule.code.as_deref().unwrap_or(&rule.name)),
        keys,
        filter_sql: (!sqls.is_empty()).then(|| sqls.join(" AND ")),
        source: rule.source.to_string(),
    })
}

fn conjuncts<'e>(e: &'e Expression, out: &mut Vec<&'e Expression>) {
    match e {
        Expression::Binary { left, op: BinaryOp::And, right } => {
            conjuncts(left, out);
            conjuncts(right, out);
        }
        other => out.push(other),
    }
}

/// Every uniqueness rule of `model` that compiles, in declaration order.
pub fn model_uniqueness_rules(schema: &crate::ast::ModuleSchema, model: &Model) -> Vec<CompiledUnique> {
    let enums: HashSet<String> = schema.enums.iter().map(|e| e.name.clone()).collect();
    schema
        .rule_register
        .rules
        .iter()
        .filter(|r| r.bound_model.as_deref() == Some(model.name.as_str()))
        .filter_map(|r| compile_uniqueness(r, model, &enums).ok())
        .collect()
}

/// The audit query for a uniqueness rule: how many groups of stored rows already share its
/// keys. Rows with a null key are left out, as a unique index treats nulls as distinct.
pub fn uniqueness_audit_query(model: &Model, rule: &CompiledUnique) -> String {
    let keys: Vec<String> = rule.keys.iter().map(|k| format!("\"{k}\"")).collect();
    let mut wheres: Vec<String> = keys.iter().map(|k| format!("{k} IS NOT NULL")).collect();
    if let Some(f) = &rule.filter_sql {
        wheres.push(f.clone());
    }
    format!(
        "SELECT '{}' AS rule, count(*) AS breaking FROM (SELECT 1 FROM {} WHERE {} GROUP BY {} HAVING count(*) > 1) AS duplicates;",
        rule.code.replace('\'', "''"),
        model.qualified_table_name(),
        wheres.join(" AND "),
        keys.join(", ")
    )
}

/// A rule refusing the delete of a row others still reference, compiled:
/// `count(RolePermission, permission_id == $this.id) == 0` on delete. The guard counts the
/// referencing rows on the request's own connection (deletes are soft, so a foreign key's
/// `ON DELETE` never fires).
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledDeleteGuard {
    pub name: String,
    pub code: String,
    pub message: String,
    /// `SELECT count(*) …` with the deleted row's id as `$1`.
    pub sql: String,
    pub source: String,
}

/// Compile a delete-guard rule, or say why not.
pub fn compile_delete_guard(
    rule: &DeclaredRule,
    model: &Model,
    schema: &crate::ast::ModuleSchema,
    enums: &HashSet<String>,
) -> Result<CompiledDeleteGuard, String> {
    let text = match &rule.body {
        RuleBody::Condition(t) | RuleBody::Assertion(t) => t,
        _ => return Err("not an expression".into()),
    };
    if rule.when.is_empty() || rule.when.iter().any(|w| w != "delete" && w != "soft_delete") {
        return Err(format!("applies on {:?}, not only on delete", rule.when));
    }
    let ast = expr::parse(text)?;
    let Expression::Binary { left, op: BinaryOp::Eq, right } = &ast else {
        return Err("not `count(..) == 0`".into());
    };
    let (Expression::FunctionCall { name, args }, Expression::Literal(Literal::Int(0))) = (left.as_ref(), right.as_ref()) else {
        return Err("not `count(..) == 0`".into());
    };
    let [Expression::FieldRef(FieldRef::Simple(entity)), predicate] = (name == "count").then_some(args.as_slice()).unwrap_or(&[]) else {
        return Err("not `count(<entity>, <predicate>) == 0`".into());
    };
    let other = schema
        .models
        .iter()
        .find(|m| &m.name == entity)
        .ok_or_else(|| format!("counts `{entity}`, which is not a model of this module"))?;
    let mut terms = Vec::new();
    conjuncts(predicate, &mut terms);
    let mut reference = None;
    let mut filters = Vec::new();
    for term in terms {
        let is_this_id = |e: &Expression| {
            matches!(e, Expression::MemberAccess { object, member }
                if member == "id" && matches!(object.as_ref(), Expression::Context(c) if c == "this"))
        };
        match term {
            Expression::Binary { left, op: BinaryOp::Eq, right } if is_this_id(right) || is_this_id(left) => {
                let fk = if is_this_id(right) { left } else { right };
                let Expression::FieldRef(FieldRef::Simple(f)) = fk.as_ref() else {
                    return Err("the referencing side is not a field".into());
                };
                if other.find_field(f).is_none() {
                    return Err(format!("`{f}` is not a field of {entity}"));
                }
                reference = Some(f.clone());
            }
            other_term => filters.push(other_term),
        }
    }
    let fk = reference.ok_or("names no field referencing the row (`fk == $this.id`)")?;
    let mut c = Compiler { model: other, enums, fields: Vec::new(), row_var: "row", reads_old: false, reads_actor: false, reads_time: false };
    let mut wheres = vec![format!("\"{fk}\" = $1::uuid")];
    for f in filters {
        let (_, sql, _) = c.emit(f)?;
        if c.reads_old || c.reads_actor || c.reads_time {
            return Err("its filter reads the writer, the time or the row being deleted".into());
        }
        wheres.push(sql);
    }
    let _ = model;
    Ok(CompiledDeleteGuard {
        name: rule.name.clone(),
        code: code_of(rule.code.as_deref().unwrap_or(&rule.name)),
        message: rule.message.clone().unwrap_or_else(|| format!("breaks the rule `{}`: {}", rule.name, text)),
        sql: format!("SELECT count(*) FROM {} WHERE {}", other.qualified_table_name(), wheres.join(" AND ")),
        source: rule.source.to_string(),
    })
}

/// Every delete-guard rule of `model` that compiles, in declaration order.
pub fn model_delete_guards(schema: &crate::ast::ModuleSchema, model: &Model) -> Vec<CompiledDeleteGuard> {
    let enums: HashSet<String> = schema.enums.iter().map(|e| e.name.clone()).collect();
    schema
        .rule_register
        .rules
        .iter()
        .filter(|r| r.bound_model.as_deref() == Some(model.name.as_str()))
        .filter_map(|r| compile_delete_guard(r, model, schema, &enums).ok())
        .collect()
}

/// One guard of `model`'s verb, from the source states it applies in.
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionGuard {
    /// The states the guarded transition leaves; empty or `*` for any.
    pub from: Vec<String>,
    pub text: String,
    /// A Rust expression over the record as `row`, or why the guard does not compile (it reads
    /// the writer, the time or another row).
    pub rust: Result<String, String>,
}

/// The guards of `model`'s verb `verb`, one per guarded transition. A verb can leave several
/// states under different guards (`post` from `approved` needs `balanced`; from `draft` it also
/// needs `!requires_approval`), so each guard carries the states it applies in.
pub fn transition_guards(schema: &crate::ast::ModuleSchema, model: &Model, verb: &str) -> Vec<TransitionGuard> {
    let enums: HashSet<String> = schema.enums.iter().map(|e| e.name.clone()).collect();
    let mut out = Vec::new();
    for machine in schema.rule_register.machines.iter().filter(|m| m.bound_model.as_deref() == Some(model.name.as_str())) {
        for t in machine.transitions.iter().filter(|t| t.verb.as_deref() == Some(verb)) {
            let Some(guard) = t.guard.as_deref() else { continue };
            let text = machine.composed_guard(guard);
            let rust = expr::parse(&text).and_then(|ast| {
                let mut c = Compiler { model, enums: &enums, fields: Vec::new(), row_var: "row", reads_old: false, reads_actor: false, reads_time: false };
                let (rust, _, _) = c.emit(&ast)?;
                if c.reads_old || c.reads_actor || c.reads_time {
                    return Err("reads the writer, the time or the row before the write".into());
                }
                Ok(rust)
            });
            out.push(TransitionGuard { from: t.from.clone(), text, rust });
        }
    }
    out
}

/// Whether `field` of `model` is an enum field, the only kind of state field a generated
/// transition writes.
pub fn state_field_is_enum(model: &Model, field: &str) -> bool {
    model.find_field(field).is_some_and(|f| {
        let mut t = &f.type_ref;
        while let TypeRef::Optional(inner) = t {
            t = inner;
        }
        matches!(t, TypeRef::Custom(_))
    })
}

/// Whether `model`'s generated transition verbs are mounted: its hook has a machine the
/// generator emits transition routes for (on an enum state field), and the machine declares
/// `verbs: generated`.
pub fn mounts_generated_verbs(schema: &crate::ast::ModuleSchema, model: &Model) -> bool {
    let has_routes = schema
        .hooks
        .iter()
        .find(|h| h.model_ref == model.name || h.name == model.name)
        .and_then(|h| h.state_machine.as_ref())
        .is_some_and(|sm| state_field_is_enum(model, &sm.field));
    has_routes
        && schema
            .rule_register
            .machines
            .iter()
            .any(|m| m.bound_model.as_deref() == Some(model.name.as_str()) && m.verbs.as_deref() == Some("generated"))
}

/// The state fields of the machines declared on `model`: fields only a verb may move.
pub fn model_state_fields(schema: &crate::ast::ModuleSchema, model: &Model) -> Vec<String> {
    let mut fields: Vec<String> = schema
        .rule_register
        .machines
        .iter()
        .filter(|m| m.bound_model.as_deref() == Some(model.name.as_str()))
        .map(|m| m.field.clone())
        .filter(|f| model.find_field(f).is_some())
        .collect();
    fields.sort();
    fields.dedup();
    fields
}

/// Whether the generator emits a write guard for `model`.
pub fn model_has_guard(schema: &crate::ast::ModuleSchema, model: &Model) -> bool {
    !model_row_rules(schema, model).is_empty()
        || !model_delete_guards(schema, model).is_empty()
        || !model_state_fields(schema, model).is_empty()
}

/// A foreign key a field declares (`@foreign_key(Entity.column)`, quoted or bare) on another
/// entity of the same module: the reference an existence rule rests on.
#[derive(Debug, Clone, PartialEq)]
pub struct DeclaredForeignKey {
    pub field: String,
    pub target: String,
    pub target_column: String,
    /// The declaration named the target's table where a column belongs
    /// (`@foreign_key(Event.events)`); read as the target's primary key.
    pub names_table: bool,
}

/// The same-module foreign keys `model`'s fields declare, in field order. A reference into
/// another module is left out: the workspace validator checks those.
pub fn declared_foreign_keys(schema: &crate::ast::ModuleSchema, model: &Model) -> Vec<DeclaredForeignKey> {
    use crate::ast::AttributeValue;
    model
        .fields
        .iter()
        .filter_map(|f| {
            let (_, v) = f.attributes.iter().find(|a| a.name == "foreign_key")?.args.first()?;
            let (AttributeValue::String(s) | AttributeValue::Ident(s)) = v else { return None };
            let [entity, column] = s.split('.').collect::<Vec<_>>()[..] else { return None };
            let target = schema.models.iter().find(|m| m.name == entity)?;
            let names_table = target.find_field(column).is_none() && column == target.collection_name();
            let target_column = if names_table {
                target.primary_key().map(|p| p.name.clone()).unwrap_or_else(|| "id".into())
            } else {
                column.to_string()
            };
            Some(DeclaredForeignKey { field: f.name.clone(), target: entity.to_string(), target_column, names_table })
        })
        .collect()
}

/// The audit query for a declared foreign key: how many stored rows reference a row that does
/// not exist. Run before the constraint is added, since `VALIDATE` fails on any such row.
pub fn foreign_key_audit_query(schema: &crate::ast::ModuleSchema, model: &Model, fk: &DeclaredForeignKey) -> Option<String> {
    let target = schema.models.iter().find(|m| m.name == fk.target)?;
    Some(format!(
        "SELECT 'fk_{}_{}' AS rule, count(*) AS breaking FROM {} c WHERE c.\"{}\" IS NOT NULL \
         AND NOT EXISTS (SELECT 1 FROM {} p WHERE p.\"{}\" = c.\"{}\");",
        model.collection_name(),
        fk.field,
        model.qualified_table_name(),
        fk.field,
        target.qualified_table_name(),
        fk.target_column,
        fk.field
    ))
}

/// The Rust items of `model`'s write guard: `<Model>RowRules`, checking each compiled row rule
/// against the row a create or update would store. Every rule runs in shadow.
pub fn render_guard(
    model: &Model,
    rules: &[CompiledRowRule],
    deletes: &[CompiledDeleteGuard],
    state_fields: &[String],
) -> String {
    let name = &model.name;
    let mut out = String::new();
    out.push_str(&format!(
        "\n/// The row rules the schema declares for {name}, checked against the row a create or\n\
         /// update would store. The generated build attaches it to [`{name}Service`]. Every rule\n\
         /// runs in shadow: a break is logged under `backbone::write_guard` and the write goes ahead.\n\
         pub struct {name}RowRules;\n\n\
         #[async_trait::async_trait]\n\
         impl backbone_core::write_guard::WriteGuard<{name}> for {name}RowRules {{\n\
         \x20   async fn check(\n\
         \x20       &self,\n\
         \x20       ctx: &backbone_core::write_guard::WriteCtx<'_, {name}>,\n\
         \x20   ) -> backbone_core::write_guard::GuardOutcome {{\n\
         \x20       use backbone_core::row_rule as r;\n\
         \x20       use backbone_core::violation::Violation;\n\
         \x20       use backbone_core::write_guard::{{GuardOutcome, WriteKind}};\n"
    ));
    if !deletes.is_empty() {
        // A delete: count the rows still referencing the row, on the request's own connection.
        out.push_str(
            "        if matches!(ctx.kind, WriteKind::Delete | WriteKind::HardDelete) {\n\
             \x20           let Some(Ok(old)) = ctx.before.map(serde_json::to_value) else {\n\
             \x20               return GuardOutcome::allow();\n\
             \x20           };\n\
             \x20           let mut shadow = Vec::new();\n",
        );
        for d in deletes {
            out.push_str(&format!(
                "            // `{}` ({})\n            if !r::holds(&r::eq(r::referencing({:?}, r::field(&old, \"id\")).await, r::Val::Num(0.0_f64))) {{\n                shadow.push(Violation::new(\"\", {:?}, {:?}));\n            }}\n",
                d.name, d.source, d.sql, d.code, d.message
            ));
        }
        out.push_str("            return GuardOutcome { refuse: Vec::new(), shadow };\n        }\n");
    }
    if rules.is_empty() && state_fields.is_empty() {
        out.push_str("        GuardOutcome::allow()\n    }\n}\n");
        return out;
    }
    out.push_str(&format!(
        "\x20       let (create, update) = (ctx.kind == WriteKind::Create, ctx.kind == WriteKind::Update);\n\
         \x20       let Some(after) = ctx.after.filter(|_| create || update) else {{\n\
         \x20           return GuardOutcome::allow();\n\
         \x20       }};\n\
         \x20       let Ok(row) = serde_json::to_value(after) else {{\n\
         \x20           return GuardOutcome::allow();\n\
         \x20       }};\n\
         \x20       let row = &row;\n"
    ));
    // `old` serves the change test of a time rule and the reads of a transition rule; bind
    // only what the rules use, so the generated module stays warning-free.
    if !state_fields.is_empty() || rules.iter().any(|r| r.reads_old || (!r.time_fields.is_empty() && r.on_update)) {
        out.push_str("        let old = ctx.before.and_then(|b| serde_json::to_value(b).ok());\n");
    }
    if rules.iter().any(|r| r.reads_old) {
        out.push_str("        let old_row = old.as_ref().unwrap_or(&serde_json::Value::Null);\n");
    }
    out.push_str("        let mut shadow = Vec::new();\n");
    // A state field moves only through its verbs: a generic write changing it is a break.
    for f in state_fields {
        out.push_str(&format!(
            "        // `{f}` belongs to a state machine: only its verbs move it\n\
             \x20       if update && ctx.transition.is_none() && r::changed(old.as_ref(), row, &[{f:?}]) {{\n\
             \x20           shadow.push(Violation::new({f:?}, \"state_field_verbs_only\", \"the state field moves only through its verbs\"));\n\
             \x20       }}\n"
        ));
    }
    for rule in rules {
        // A rule about the time of the write checks an update only when it changes what it reads.
        let update = if rule.time_fields.is_empty() {
            "update".to_string()
        } else {
            let fields: Vec<String> = rule.time_fields.iter().map(|f| format!("{f:?}")).collect();
            format!("(update && r::changed(old.as_ref(), row, &[{}]))", fields.join(", "))
        };
        let on = match (rule.on_create, rule.on_update) {
            (true, true) => format!("create || {update}"),
            (true, false) => "create".to_string(),
            _ => update,
        };
        out.push_str(&format!(
            "        // `{}` ({})\n        if ({on}) && !r::holds(&{}) {{\n            shadow.push(Violation::new({:?}, {:?}, {:?}));\n        }}\n",
            rule.name, rule.source, rule.rust, rule.path, rule.code, rule.message
        ));
    }
    out.push_str("        GuardOutcome { refuse: Vec::new(), shadow }\n    }\n}\n");
    out
}

/// The audit query for one compiled rule: how many stored rows break it, soft-deleted rows
/// included, since a `CHECK` constraint covers every row. A null result is not a break.
/// `None` for a transition or context rule, which stored rows cannot be checked against.
pub fn audit_query(model: &Model, rule: &CompiledRowRule) -> Option<String> {
    Some(format!(
        "SELECT '{}' AS rule, count(*) AS breaking FROM {} WHERE NOT ({});",
        rule.code.replace('\'', "''"),
        model.qualified_table_name(),
        rule.sql.as_ref()?
    ))
}

struct Compiler<'a> {
    model: &'a Model,
    enums: &'a HashSet<String>,
    /// The fields the rule reads, first first.
    fields: Vec<String>,
    /// The row a field reference reads: `row`, or `old_row` inside `$old.`.
    row_var: &'static str,
    reads_old: bool,
    reads_actor: bool,
    reads_time: bool,
}

/// What an emitted sub-expression is, so the SQL side can pick the right function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ty {
    Null,
    Bool,
    Number,
    Text,
    Enum,
    Time,
    List,
    /// A key inside a JSON field: untyped text, fit only for a null test.
    JsonKey,
}

fn rust_str(s: &str) -> String {
    format!("{s:?}")
}

fn sql_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn rust_num(n: f64) -> String {
    format!("r::Val::Num({n:?}_f64)")
}

impl Compiler<'_> {
    fn field(&mut self, name: &str) -> Result<(String, String, Ty), String> {
        let Some(field) = self.model.find_field(name) else {
            return Err(format!("`{name}` is not a field of {}", self.model.name));
        };
        let kind = kind_of(&field.type_ref, self.enums)
            .ok_or_else(|| format!("field `{name}` has a type a row rule cannot read"))?;
        if !self.fields.iter().any(|f| f == name) {
            self.fields.push(name.to_string());
        }
        let col = format!("\"{name}\"");
        let (sql, ty) = match kind {
            Kind::Text => (col, Ty::Text),
            Kind::Enum => (format!("{col}::text"), Ty::Enum),
            Kind::Number => (col, Ty::Number),
            Kind::Bool => (col, Ty::Bool),
            Kind::Time => (col, Ty::Time),
            Kind::List => (col, Ty::List),
        };
        Ok((format!("r::field({}, {})", self.row_var, rust_str(name)), sql, ty))
    }

    fn emit(&mut self, e: &Expression) -> Result<(String, String, Ty), String> {
        match e {
            Expression::Literal(l) => Ok(match l {
                Literal::String(s) => (format!("r::text({})", rust_str(s)), sql_str(s), Ty::Text),
                Literal::Int(n) => (rust_num(*n as f64), n.to_string(), Ty::Number),
                Literal::Float(n) => (rust_num(*n), format!("{n:?}"), Ty::Number),
                Literal::Bool(b) => (format!("r::Val::Bool({b})"), b.to_string().to_uppercase(), Ty::Bool),
                Literal::Null => ("r::Val::Null".into(), "NULL".into(), Ty::Null),
            }),
            Expression::FieldRef(FieldRef::Simple(n)) => self.field(n),
            Expression::FieldRef(_) => Err("reads another row or entity".into()),
            Expression::MemberAccess { object, member } => match object.as_ref() {
                Expression::Context(c) if c == "this" => self.field(member),
                Expression::Context(c) if c == "old" => {
                    self.reads_old = true;
                    let outer = std::mem::replace(&mut self.row_var, "old_row");
                    let read = self.field(member);
                    self.row_var = outer;
                    read
                }
                Expression::Context(c) if c == "actor" && member == "id" => {
                    self.reads_actor = true;
                    Ok(("r::actor(ctx.actor.as_deref())".into(), "NULL".into(), Ty::Text))
                }
                Expression::Context(c) => Err(format!("reads `${c}.{member}`, which a row rule cannot")),
                // `metadata.deleted_at`: a key inside a JSON field, read as text.
                Expression::FieldRef(FieldRef::Simple(f)) if self.is_json(f) => {
                    if !self.fields.iter().any(|x| x == f) {
                        self.fields.push(f.clone());
                    }
                    Ok((
                        format!("r::field({}.get({}).unwrap_or(&serde_json::Value::Null), {})", self.row_var, rust_str(f), rust_str(member)),
                        format!("(\"{f}\"->>{})", sql_str(member)),
                        Ty::JsonKey,
                    ))
                }
                _ if member == "length" || member == "len" => {
                    let (r, s, t) = self.emit(object)?;
                    Ok(self.length(r, s, t))
                }
                _ => Err(format!("`.{member}` reads a relation or a duration")),
            },
            Expression::Context(c) if c == "now" => Ok(self.now()),
            Expression::Context(c) => Err(format!("reads `${c}`, which is not the row alone")),
            Expression::Array(items) => {
                let mut rs = Vec::new();
                let mut ss = Vec::new();
                for item in items {
                    let (r, s, _) = self.emit(item)?;
                    rs.push(r);
                    ss.push(s);
                }
                Ok((format!("r::Val::List(vec![{}])", rs.join(", ")), format!("({})", ss.join(", ")), Ty::List))
            }
            Expression::Unary { op, expr } => {
                let (r, s, t) = self.emit(expr)?;
                Ok(match op {
                    UnaryOp::Not => (format!("r::not({r})"), format!("(NOT {s})"), Ty::Bool),
                    UnaryOp::Neg => (format!("r::neg({r})"), format!("(-{s})"), t),
                })
            }
            Expression::Conditional { condition, then_expr, else_expr } => {
                let (cr, cs, _) = self.emit(condition)?;
                let (tr, ts, tt) = self.emit(then_expr)?;
                let (er, es, _) = self.emit(else_expr)?;
                Ok((format!("r::cond({cr}, {tr}, {er})"), format!("(CASE WHEN {cs} THEN {ts} ELSE {es} END)"), tt))
            }
            Expression::Binary { left, op, right } => self.binary(left, *op, right),
            Expression::MethodCall { object, method, args } => self.method(object, method, args),
            Expression::FunctionCall { name, args } => self.function(name, args),
            Expression::Aggregate { .. } => Err("aggregates other rows".into()),
            Expression::Raw(_) => Err("raw text".into()),
        }
    }

    fn is_json(&self, name: &str) -> bool {
        self.model.find_field(name).is_some_and(|f| {
            let mut t = &f.type_ref;
            while let TypeRef::Optional(inner) = t {
                t = inner;
            }
            matches!(t, TypeRef::Primitive(PrimitiveType::Json) | TypeRef::Map { .. })
        })
    }

    /// The time of the write: `now()` or `$now`.
    fn now(&mut self) -> (String, String, Ty) {
        self.reads_time = true;
        ("r::at(ctx.at)".into(), "now()".into(), Ty::Time)
    }

    fn length(&self, r: String, s: String, t: Ty) -> (String, String, Ty) {
        let sql = if t == Ty::List { format!("cardinality({s})") } else { format!("char_length({s})") };
        (format!("r::length({r})"), sql, Ty::Number)
    }

    fn binary(&mut self, left: &Expression, op: BinaryOp, right: &Expression) -> Result<(String, String, Ty), String> {
        // `x == null` / `x != null` test for null itself.
        let is_null_lit = |e: &Expression| matches!(e, Expression::Literal(Literal::Null));
        if matches!(op, BinaryOp::Eq | BinaryOp::Ne) && (is_null_lit(left) || is_null_lit(right)) {
            let other = if is_null_lit(left) { right } else { left };
            let (r, s, _) = self.emit(other)?;
            return Ok(if op == BinaryOp::Eq {
                (format!("r::is_null({r})"), format!("({s} IS NULL)"), Ty::Bool)
            } else {
                (format!("r::is_not_null({r})"), format!("({s} IS NOT NULL)"), Ty::Bool)
            });
        }
        if op == BinaryOp::Matches {
            let Expression::Literal(Literal::String(pattern)) = right else {
                return Err("`matches` takes a pattern written as a string".into());
            };
            regex::Regex::new(pattern).map_err(|e| format!("pattern `{pattern}` does not compile: {e}"))?;
            let (r, s, _) = self.emit(left)?;
            return Ok((format!("r::matches({r}, {})", rust_str(pattern)), format!("({s} ~ {})", sql_str(pattern)), Ty::Bool));
        }
        let (lr, ls, lt) = self.emit(left)?;
        let (rr, rs, rt) = self.emit(right)?;
        if lt == Ty::JsonKey || rt == Ty::JsonKey {
            return Err("a key inside a JSON field has no type; a rule can only test it for null".into());
        }
        let (f, sql, ty) = match op {
            BinaryOp::Eq => ("eq", format!("({ls} = {rs})"), Ty::Bool),
            BinaryOp::Ne => ("ne", format!("({ls} <> {rs})"), Ty::Bool),
            BinaryOp::Lt => ("lt", format!("({ls} < {rs})"), Ty::Bool),
            BinaryOp::Le => ("le", format!("({ls} <= {rs})"), Ty::Bool),
            BinaryOp::Gt => ("gt", format!("({ls} > {rs})"), Ty::Bool),
            BinaryOp::Ge => ("ge", format!("({ls} >= {rs})"), Ty::Bool),
            BinaryOp::And => ("and", format!("({ls} AND {rs})"), Ty::Bool),
            BinaryOp::Or => ("or", format!("({ls} OR {rs})"), Ty::Bool),
            BinaryOp::Add => ("add", format!("({ls} + {rs})"), Ty::Number),
            BinaryOp::Sub => ("sub", format!("({ls} - {rs})"), Ty::Number),
            BinaryOp::Mul => ("mul", format!("({ls} * {rs})"), Ty::Number),
            // Numeric division, and null on a zero divisor, as the Rust side does.
            BinaryOp::Div => ("div", format!("(({ls})::numeric / NULLIF({rs}, 0))"), Ty::Number),
            BinaryOp::Mod => ("rem", format!("({ls} % NULLIF({rs}, 0))"), Ty::Number),
            BinaryOp::In => ("in_list", format!("({ls} IN {rs})"), Ty::Bool),
            BinaryOp::NotIn => ("not_in", format!("({ls} NOT IN {rs})"), Ty::Bool),
            BinaryOp::Contains => return self.contains(lr, ls, lt, rr, rs),
            BinaryOp::StartsWith => ("starts_with", format!("starts_with({ls}, {rs})"), Ty::Bool),
            BinaryOp::EndsWith => ("ends_with", format!("(right({ls}, char_length({rs})) = {rs})"), Ty::Bool),
            BinaryOp::Matches => unreachable!("handled above"),
        };
        Ok((format!("r::{f}({lr}, {rr})"), sql, ty))
    }

    fn contains(&self, lr: String, ls: String, lt: Ty, rr: String, rs: String) -> Result<(String, String, Ty), String> {
        let sql = if lt == Ty::List { format!("({rs} = ANY({ls}))") } else { format!("(strpos({ls}, {rs}) > 0)") };
        Ok((format!("r::contains({lr}, {rr})"), sql, Ty::Bool))
    }

    fn method(&mut self, object: &Expression, method: &str, args: &[Expression]) -> Result<(String, String, Ty), String> {
        if matches!(object, Expression::Context(c) if c == "actor") {
            // `$actor.has_role('admin')`: asks the roles source the composing service installed.
            if let ("has_role", [Expression::Literal(Literal::String(role))]) = (method, args) {
                self.reads_actor = true;
                return Ok((format!("r::has_role(ctx.actor.as_deref(), {}).await", rust_str(role)), "NULL".into(), Ty::Bool));
            }
            return Err(format!("`$actor.{method}(..)` is not a writer check a rule can make"));
        }
        let (or, os, ot) = self.emit(object)?;
        if ot == Ty::JsonKey {
            return Err("a key inside a JSON field has no type; a rule can only test it for null".into());
        }
        match (method, args) {
            ("length" | "len", []) => Ok(self.length(or, os, ot)),
            ("trim", []) => Ok((format!("r::trim({or})"), format!("btrim({os})"), Ty::Text)),
            ("lower", []) => Ok((format!("r::lower({or})"), format!("lower({os})"), Ty::Text)),
            ("upper", []) => Ok((format!("r::upper({or})"), format!("upper({os})"), Ty::Text)),
            ("is_empty", []) => {
                let (_, ls, _) = self.length(String::new(), os, ot);
                Ok((format!("r::is_empty({or})"), format!("({ls} = 0)"), Ty::Bool))
            }
            ("contains", [arg]) => {
                let (ar, asql, _) = self.emit(arg)?;
                self.contains(or, os, ot, ar, asql)
            }
            ("starts_with", [arg]) => {
                let (ar, asql, _) = self.emit(arg)?;
                Ok((format!("r::starts_with({or}, {ar})"), format!("starts_with({os}, {asql})"), Ty::Bool))
            }
            ("ends_with", [arg]) => {
                let (ar, asql, _) = self.emit(arg)?;
                Ok((format!("r::ends_with({or}, {ar})"), format!("(right({os}, char_length({asql})) = {asql})"), Ty::Bool))
            }
            _ => Err(format!("`.{method}` with {} argument(s) is not a row-rule method", args.len())),
        }
    }

    fn function(&mut self, name: &str, args: &[Expression]) -> Result<(String, String, Ty), String> {
        match (name, args) {
            ("now", []) => Ok(self.now()),
            ("abs" | "ceil" | "floor" | "round", [arg]) => {
                let (r, s, _) = self.emit(arg)?;
                Ok((format!("r::{name}({r})"), format!("{name}({s})"), Ty::Number))
            }
            ("lower" | "upper", [arg]) => {
                let (r, s, _) = self.emit(arg)?;
                Ok((format!("r::{name}({r})"), format!("{name}({s})"), Ty::Text))
            }
            ("len", [arg]) => {
                let (r, s, t) = self.emit(arg)?;
                Ok(self.length(r, s, t))
            }
            ("coalesce", args) if !args.is_empty() => {
                let mut rs = Vec::new();
                let mut ss = Vec::new();
                let mut ty = Ty::Null;
                for a in args {
                    let (r, s, t) = self.emit(a)?;
                    if ty == Ty::Null {
                        ty = t;
                    }
                    rs.push(r);
                    ss.push(s);
                }
                Ok((format!("r::coalesce(vec![{}])", rs.join(", ")), format!("COALESCE({})", ss.join(", ")), ty))
            }
            _ => Err(format!("`{name}()` is not a row-rule function")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Field;
    use crate::rules::{EnforcementStatus, SourceRef};

    fn bucket() -> Model {
        let mut m = Model::new("Bucket");
        m.schema = Some("bucket".into());
        m.fields.push(Field::new("name", TypeRef::Primitive(PrimitiveType::String)));
        m.fields.push(Field::new("root_path", TypeRef::Optional(Box::new(TypeRef::Primitive(PrimitiveType::String)))));
        m.fields.push(Field::new("risk_score", TypeRef::Primitive(PrimitiveType::Decimal)));
        m.fields.push(Field::new("status", TypeRef::Custom("BucketStatus".into())));
        m.fields.push(Field::new("tags", TypeRef::Array(Box::new(TypeRef::Primitive(PrimitiveType::String)))));
        m.fields.push(Field::new("metadata", TypeRef::Primitive(PrimitiveType::Json)));
        m
    }

    fn rule(text: &str, when: &[&str]) -> DeclaredRule {
        let m = bucket();
        DeclaredRule {
            source: SourceRef { file: "bucket.hook.yaml".into(), line: Some(3) },
            id: None,
            name: "a rule".into(),
            target: "Bucket".into(),
            alternatives: vec![],
            bound_model: Some("Bucket".into()),
            body: RuleBody::Condition(text.into()),
            when: when.iter().map(|w| w.to_string()).collect(),
            code: None,
            message: None,
            severity: None,
            enforcement: None,
            justification: None,
            enforced_by: None,
            status: EnforcementStatus::Unenforced,
            expression: expr::check(text, Some(&m)),
        }
    }

    fn enums() -> HashSet<String> {
        ["BucketStatus".to_string()].into()
    }

    fn compile(text: &str) -> Result<CompiledRowRule, String> {
        compile_row_rule(&rule(text, &["create", "update"]), &bucket(), &enums())
    }

    #[test]
    fn a_length_range_compiles_to_both_forms() {
        let c = compile("name.length >= 1 && name.length <= 255").unwrap();
        assert_eq!(
            c.rust,
            "r::and(r::ge(r::length(r::field(row, \"name\")), r::Val::Num(1.0_f64)), \
             r::le(r::length(r::field(row, \"name\")), r::Val::Num(255.0_f64)))"
        );
        assert_eq!(c.sql.as_deref().unwrap(), "((char_length(\"name\") >= 1) AND (char_length(\"name\") <= 255))");
        assert_eq!((c.path.as_str(), c.code.as_str()), ("name", "a_rule"));
        assert!(c.on_create && c.on_update);
    }

    #[test]
    fn a_null_comparison_tests_for_null_and_an_enum_compares_as_text() {
        let c = compile("root_path != null && status in ['active', 'archived']").unwrap();
        assert!(c.rust.starts_with("r::and(r::is_not_null(r::field(row, \"root_path\")), r::in_list("));
        assert_eq!(c.sql.as_deref().unwrap(), "((\"root_path\" IS NOT NULL) AND (\"status\"::text IN ('active', 'archived')))");
    }

    #[test]
    fn text_methods_patterns_and_lists_compile() {
        assert_eq!(compile("!root_path.contains('..')").unwrap().sql.as_deref().unwrap(), "(NOT (strpos(\"root_path\", '..') > 0))");
        assert_eq!(compile("name matches '^[a-z0-9-]+$'").unwrap().sql.as_deref().unwrap(), "(\"name\" ~ '^[a-z0-9-]+$')");
        assert_eq!(compile("tags.length <= 10").unwrap().sql.as_deref().unwrap(), "(cardinality(\"tags\") <= 10)");
        assert_eq!(compile("name.trim().length > 0").unwrap().sql.as_deref().unwrap(), "(char_length(btrim(\"name\")) > 0)");
    }

    #[test]
    fn what_a_row_rule_cannot_read_is_refused_with_a_reason() {
        for (text, says) in [
            ("$actor.can('admin')", "unresolved"),
            ("$actor.email == name", "`$actor.email`"),
            ("metadata != null", "cannot read"),
            ("risk_score > metadata.created_at", "only test it for null"),
            ("name matches '('", "does not compile"),
            ("(now() - risk_score).years <= 150", "relation or a duration"),
        ] {
            let err = compile(text).unwrap_err();
            assert!(err.contains(says), "{text}: {err}");
        }
        // A rule bound to a verb is not a generic write's rule.
        let verb = compile_row_rule(&rule("name.length > 0", &["login"]), &bucket(), &enums());
        assert!(verb.unwrap_err().contains("not only on create and update"));
    }

    #[test]
    fn a_rule_reading_the_row_before_the_write_is_a_transition_rule_without_sql() {
        let c = compile("risk_score >= $old.risk_score").unwrap();
        assert_eq!(c.rust, "r::ge(r::field(row, \"risk_score\"), r::field(old_row, \"risk_score\"))");
        assert_eq!((c.layer, c.reads_old, c.sql.is_none()), ("transition", true, true));
        assert!(c.time_fields.is_empty());
    }

    #[test]
    fn a_rule_reading_the_writer_or_the_time_is_a_context_rule() {
        let owner = compile("$actor.id == name").unwrap();
        assert_eq!(owner.rust, "r::eq(r::actor(ctx.actor.as_deref()), r::field(row, \"name\"))");
        assert_eq!((owner.layer, owner.sql.is_none()), ("context", true));
        assert!(owner.time_fields.is_empty(), "only a time rule waits for a change");

        let expiry = compile("root_path == null || $this.risk_score > now()").unwrap();
        assert!(expiry.rust.ends_with("r::gt(r::field(row, \"risk_score\"), r::at(ctx.at)))"), "{}", expiry.rust);
        assert_eq!(expiry.time_fields, vec!["root_path".to_string(), "risk_score".to_string()]);
    }

    #[test]
    fn a_generic_write_moving_a_state_field_is_reported_and_a_verb_is_not() {
        let guard = render_guard(&bucket(), &[], &[], &["status".to_string()]);
        assert!(guard.contains("let old = ctx.before.and_then(|b| serde_json::to_value(b).ok());"), "{guard}");
        assert!(
            guard.contains("if update && ctx.transition.is_none() && r::changed(old.as_ref(), row, &[\"status\"]) {"),
            "{guard}"
        );
        assert!(guard.contains("\"state_field_verbs_only\""), "{guard}");
        assert!(!guard.contains("let old_row"), "nothing reads the row before the write: {guard}");
    }

    #[test]
    fn a_verbs_guards_are_kept_apart_by_the_state_they_leave() {
        let mut journal = Model::new("Journal");
        for (n, t) in [("status", PrimitiveType::String), ("total_debit", PrimitiveType::Decimal), ("total_credit", PrimitiveType::Decimal), ("requires_approval", PrimitiveType::Bool)] {
            journal.fields.push(crate::ast::Field::new(n, TypeRef::Primitive(t)));
        }
        let mut schema = crate::ast::ModuleSchema::new("accounting");
        schema.models = vec![journal.clone()];
        let hook = "module: accounting\nstate_machines:\n  - entity: Journal\n    field: status\n    states: [draft, approved, posted]\n    transitions:\n      - { from: approved, to: posted, on: post, guard: balanced }\n      - { from: draft, to: posted, on: post, guard: \"balanced && !requires_approval\" }\n    guards:\n      balanced: \"total_debit == total_credit\"\n";
        schema.rule_register.read_file("journal.hook.yaml", hook);
        schema.rule_register.bind(&schema.models.clone());
        let guards = transition_guards(&schema, &journal, "post");
        assert_eq!(guards.len(), 2, "{:?}", schema.rule_register.warnings);
        assert_eq!(guards[0].from, vec!["approved".to_string()]);
        assert_eq!(guards[0].rust.as_deref().unwrap(), "r::eq(r::field(row, \"total_debit\"), r::field(row, \"total_credit\"))");
        assert_eq!(guards[1].from, vec!["draft".to_string()]);
        assert!(guards[1].rust.as_deref().unwrap().contains("r::not(r::field(row, \"requires_approval\"))"), "{:?}", guards[1]);
        assert!(transition_guards(&schema, &journal, "void").is_empty());
    }

    #[test]
    fn a_role_check_asks_the_roles_source() {
        let c = compile("$actor.has_role('admin') || name != null").unwrap();
        assert_eq!(c.rust, "r::or(r::has_role(ctx.actor.as_deref(), \"admin\").await, r::is_not_null(r::field(row, \"name\")))");
        assert_eq!((c.layer, c.sql.is_none()), ("context", true));
    }

    #[test]
    fn a_delete_guard_counts_the_rows_still_referencing_the_row() {
        let mut lock = Model::new("FileLock");
        lock.schema = Some("bucket".into());
        lock.fields.push(crate::ast::Field::new("bucket_id", TypeRef::Primitive(PrimitiveType::Uuid)));
        lock.fields.push(crate::ast::Field::new("active", TypeRef::Primitive(PrimitiveType::Bool)));
        let mut schema = crate::ast::ModuleSchema::new("bucket");
        schema.models = vec![bucket(), lock];
        let d = compile_delete_guard(
            &rule("count(FileLock, bucket_id == $this.id && active == true) == 0", &["delete"]),
            &bucket(),
            &schema,
            &enums(),
        )
        .unwrap();
        assert_eq!(d.sql, "SELECT count(*) FROM bucket.file_locks WHERE \"bucket_id\" = $1::uuid AND (\"active\" = TRUE)");
        for (text, when, says) in [
            ("count(FileLock, bucket_id == $this.id) == 0", "create", "not only on delete"),
            ("count(Other, x == $this.id) == 0", "delete", "not a model of this module"),
            ("count(FileLock, active == true) == 0", "delete", "names no field referencing"),
        ] {
            let err = compile_delete_guard(&rule(text, &[when]), &bucket(), &schema, &enums()).unwrap_err();
            assert!(err.contains(says), "{text}: {err}");
        }
        // A guard with only delete rules ends after the delete branch: nothing else is bound.
        let guard = render_guard(&bucket(), &[], &[d], &[]);
        assert!(guard.contains("if matches!(ctx.kind, WriteKind::Delete | WriteKind::HardDelete) {"), "{guard}");
        assert!(guard.contains("r::referencing(\"SELECT count(*) FROM bucket.file_locks"), "{guard}");
        assert!(guard.contains("        GuardOutcome::allow()\n    }\n}\n") && !guard.contains("let row"), "{guard}");
    }

    #[test]
    fn the_guard_checks_a_time_rule_on_update_only_when_its_fields_change() {
        let mut rules = vec![compile("risk_score > now()").unwrap(), compile("risk_score >= $old.risk_score").unwrap()];
        rules[1].on_create = false;
        let guard = render_guard(&bucket(), &rules, &[], &[]);
        assert!(guard.contains("let old_row = old.as_ref().unwrap_or(&serde_json::Value::Null);"));
        assert!(guard.contains("if (create || (update && r::changed(old.as_ref(), row, &[\"risk_score\"]))) && !r::holds("), "{guard}");
        assert!(guard.contains("if (update) && !r::holds(&r::ge("), "{guard}");
        // A guard with only row rules binds no `old`; one with only time rules binds no `old_row`.
        assert!(!render_guard(&bucket(), &[compile("name.length > 0").unwrap()], &[], &[]).contains("let old"));
        let time_only = render_guard(&bucket(), &[compile("risk_score > now()").unwrap()], &[], &[]);
        assert!(time_only.contains("let old =") && !time_only.contains("let old_row"), "{time_only}");
        // A time rule checked on create alone never asks whether an update changed anything.
        let mut on_create = compile("risk_score > now()").unwrap();
        on_create.on_update = false;
        assert!(!render_guard(&bucket(), &[on_create], &[], &[]).contains("let old"));
    }

    #[test]
    fn a_count_of_its_own_entity_equal_to_zero_is_a_uniqueness_rule() {
        let u = compile_uniqueness(
            &rule("count(Bucket, name == $this.name && id != $this.id && metadata.deleted_at == null) == 0", &["create", "update"]),
            &bucket(),
            &enums(),
        )
        .unwrap();
        assert_eq!(u.keys, vec!["name".to_string()]);
        assert_eq!(u.filter_sql.as_deref(), Some("((\"metadata\"->>'deleted_at') IS NULL)"));
        assert_eq!(
            uniqueness_audit_query(&bucket(), &u),
            "SELECT 'a_rule' AS rule, count(*) AS breaking FROM (SELECT 1 FROM bucket.buckets \
             WHERE \"name\" IS NOT NULL AND ((\"metadata\"->>'deleted_at') IS NULL) GROUP BY \"name\" \
             HAVING count(*) > 1) AS duplicates;"
        );
        for (text, says) in [
            ("count(User, name == $this.name) == 0", "another entity"),
            ("count(Bucket, status == 'active') == 0", "no field that must not repeat"),
            ("count(Bucket, name == $this.name && risk_score > now()) == 0", "an index cannot"),
            ("name.length > 0", "not `count(..) == 0`"),
        ] {
            let err = compile_uniqueness(&rule(text, &["create"]), &bucket(), &enums()).unwrap_err();
            assert!(err.contains(says), "{text}: {err}");
        }
    }

    #[test]
    fn a_declared_foreign_key_is_audited_for_orphans_quoted_or_not() {
        use crate::ast::{Attribute, AttributeValue};
        let mut comment = Model::new("FileComment");
        comment.schema = Some("bucket".into());
        for (name, v) in [
            ("file_id", AttributeValue::Ident("Bucket.id".into())),
            ("parent_id", AttributeValue::String("FileComment.id".into())),
            ("user_id", AttributeValue::Ident("sapiens.User.id".into())),
            ("bucket_id", AttributeValue::Ident("Bucket.buckets".into())),
        ] {
            let mut f = crate::ast::Field::new(name, TypeRef::Primitive(PrimitiveType::Uuid));
            f.attributes.push(Attribute::new("foreign_key").with_arg(v));
            comment.fields.push(f);
        }
        let mut schema = crate::ast::ModuleSchema::new("bucket");
        schema.models = vec![bucket(), comment.clone()];
        let fks = declared_foreign_keys(&schema, &comment);
        assert_eq!(fks.iter().map(|f| f.field.as_str()).collect::<Vec<_>>(), ["file_id", "parent_id", "bucket_id"], "another module's reference is left out");
        // `Bucket.buckets` names the table: read as the primary key, and flagged.
        assert_eq!((fks[2].target_column.as_str(), fks[2].names_table), ("id", true));
        assert!(!fks[0].names_table);
        assert_eq!(
            foreign_key_audit_query(&schema, &comment, &fks[0]).unwrap(),
            "SELECT 'fk_file_comments_file_id' AS rule, count(*) AS breaking FROM bucket.file_comments c \
             WHERE c.\"file_id\" IS NOT NULL AND NOT EXISTS (SELECT 1 FROM bucket.buckets p WHERE p.\"id\" = c.\"file_id\");"
        );
    }

    #[test]
    fn the_audit_query_counts_definite_breaks_over_every_row() {
        assert_eq!(audit_query(&bucket(), &compile("$actor.id == name").unwrap()), None);
        let c = compile("risk_score <= 1.0").unwrap();
        assert_eq!(
            audit_query(&bucket(), &c).unwrap(),
            "SELECT 'a_rule' AS rule, count(*) AS breaking FROM bucket.buckets WHERE NOT ((\"risk_score\" <= 1.0));"
        );
    }
}
