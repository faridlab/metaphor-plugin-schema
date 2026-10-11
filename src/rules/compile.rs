//! Compile a declared row rule into the code that checks it.
//!
//! A row rule is a condition over one row's own fields, checked on create,
//! update or both: no other row, no relation, no `$old`, `$actor` or `now()`.
//! It compiles to two forms that give the same answer:
//!
//! - a Rust expression over `backbone_core::row_rule`, evaluated by the
//!   entity's generated write guard against the row a write would store;
//! - an SQL expression over the table's columns, used now by the audit query
//!   that counts the stored rows breaking the rule, and later by the rule's
//!   `CHECK` constraint.
//!
//! Both follow SQL's semantics: an operator over a null is null, and a rule
//! holds unless it is definitely false. `x == null` and `x != null` test for
//! null itself. Anything outside the row subset is not compiled and says why.

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
    /// A Rust expression of type `backbone_core::row_rule::Val`, reading the row as `row`.
    pub rust: String,
    /// The same rule as an SQL boolean expression over the table's columns.
    pub sql: String,
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
    let mut c = Compiler { model, enums, fields: Vec::new() };
    let (rust, sql, _) = c.emit(&ast)?;
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

/// The Rust items of `model`'s write guard: `<Model>RowRules`, checking each compiled row rule
/// against the row a create or update would store. Every rule runs in shadow.
pub fn render_guard(model: &Model, rules: &[CompiledRowRule]) -> String {
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
         \x20       use backbone_core::write_guard::{{GuardOutcome, WriteKind}};\n\
         \x20       let (create, update) = (ctx.kind == WriteKind::Create, ctx.kind == WriteKind::Update);\n\
         \x20       let Some(after) = ctx.after.filter(|_| create || update) else {{\n\
         \x20           return GuardOutcome::allow();\n\
         \x20       }};\n\
         \x20       let Ok(row) = serde_json::to_value(after) else {{\n\
         \x20           return GuardOutcome::allow();\n\
         \x20       }};\n\
         \x20       let row = &row;\n\
         \x20       let mut shadow = Vec::new();\n"
    ));
    for rule in rules {
        let on = match (rule.on_create, rule.on_update) {
            (true, true) => "create || update",
            (true, false) => "create",
            _ => "update",
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
pub fn audit_query(model: &Model, rule: &CompiledRowRule) -> String {
    format!(
        "SELECT '{}' AS rule, count(*) AS breaking FROM {} WHERE NOT ({});",
        rule.code.replace('\'', "''"),
        model.qualified_table_name(),
        rule.sql
    )
}

struct Compiler<'a> {
    model: &'a Model,
    enums: &'a HashSet<String>,
    /// The fields the rule reads, first first.
    fields: Vec<String>,
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
        Ok((format!("r::field(row, {})", rust_str(name)), sql, ty))
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
                Expression::Context(c) => Err(format!("reads `${c}`, which is not the row alone")),
                _ if member == "length" || member == "len" => {
                    let (r, s, t) = self.emit(object)?;
                    Ok(self.length(r, s, t))
                }
                _ => Err(format!("`.{member}` reads a relation or a duration")),
            },
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
        let (rr, rs, _) = self.emit(right)?;
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
        let (or, os, ot) = self.emit(object)?;
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
        assert_eq!(c.sql, "((char_length(\"name\") >= 1) AND (char_length(\"name\") <= 255))");
        assert_eq!((c.path.as_str(), c.code.as_str()), ("name", "a_rule"));
        assert!(c.on_create && c.on_update);
    }

    #[test]
    fn a_null_comparison_tests_for_null_and_an_enum_compares_as_text() {
        let c = compile("root_path != null && status in ['active', 'archived']").unwrap();
        assert!(c.rust.starts_with("r::and(r::is_not_null(r::field(row, \"root_path\")), r::in_list("));
        assert_eq!(c.sql, "((\"root_path\" IS NOT NULL) AND (\"status\"::text IN ('active', 'archived')))");
    }

    #[test]
    fn text_methods_patterns_and_lists_compile() {
        assert_eq!(compile("!root_path.contains('..')").unwrap().sql, "(NOT (strpos(\"root_path\", '..') > 0))");
        assert_eq!(compile("name matches '^[a-z0-9-]+$'").unwrap().sql, "(\"name\" ~ '^[a-z0-9-]+$')");
        assert_eq!(compile("tags.length <= 10").unwrap().sql, "(cardinality(\"tags\") <= 10)");
        assert_eq!(compile("name.trim().length > 0").unwrap().sql, "(char_length(btrim(\"name\")) > 0)");
    }

    #[test]
    fn what_is_not_the_row_alone_is_refused_with_a_reason() {
        for (text, says) in [
            ("$old.name == name", "`$old`"),
            ("$actor.id == name", "`$actor`"),
            ("risk_score > now()", "not a row-rule function"),
            ("metadata != null", "cannot read"),
            ("name matches '('", "does not compile"),
        ] {
            let err = compile(text).unwrap_err();
            assert!(err.contains(says), "{text}: {err}");
        }
        // A rule bound to a verb is not a generic write's rule.
        let verb = compile_row_rule(&rule("name.length > 0", &["login"]), &bucket(), &enums());
        assert!(verb.unwrap_err().contains("not only on create and update"));
    }

    #[test]
    fn the_audit_query_counts_definite_breaks_over_every_row() {
        let c = compile("risk_score <= 1.0").unwrap();
        assert_eq!(
            audit_query(&bucket(), &c),
            "SELECT 'a_rule' AS rule, count(*) AS breaking FROM bucket.buckets WHERE NOT ((\"risk_score\" <= 1.0));"
        );
    }
}
