//! A model's field attributes, compiled into its write guard.
//!
//! The attributes a model declares on its fields — `@required`, `@max`, `@min`, `@length`,
//! `@range`, `@positive`, `@non_negative`, `@pattern`, `@alpha_dash`, `@alpha_num`, `@values`
//! and the `email` / `url` types — become checks in the same `<Model>RowRules` guard the hook
//! rules compile into, so the generic write path checks them on every create and update.
//!
//! - **Every check refuses.** A field marked `@required` must hold a value: not null, and for
//!   text not blank. A `@default` fills a field the write leaves out, so it carries no required
//!   check, except a text field, which still refuses a blank the writer supplies. An
//!   input-excluded or server-filled field, the id and audit columns, a tenant column and a
//!   state-machine field carry no checks: the write does not supply them, so the client cannot
//!   be held to them.
//! - **`@shadow(<why>)` keeps a field's checks in shadow**: a break is logged and the write goes
//!   ahead, with the reason beside the field.
//! - **An update is checked only for the fields it changes**, so an edit is never refused for a
//!   field it did not touch. A create checks every field.
//!
//! A null value passes every check but `required`, as SQL's `CHECK` would.

use crate::ast::model::{AttributeValue, Field, LifecycleShape, Model};
use crate::ast::types::{PrimitiveType, TypeRef};

use super::compile::{rust_num, rust_str, sql_str, CompiledRowRule};

/// Columns the server fills, never the writer.
const SERVER_FILLED: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "metadata",
    "created_by",
    "updated_by",
    "deleted_by",
    "org_unit_id",
    "company_id",
];

/// Attributes that take a field out of the writer's hands.
const NOT_WRITTEN: &[&str] = &[
    "audit_metadata",
    "exclude_from_dto",
    "readonly",
    "no_input",
    "computed",
    "stored",
    "hashed",
    "id",
];

/// What a field holds, for choosing its checks.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Text,
    Email,
    Url,
    Number,
    Other,
}

fn kind_of(type_ref: &TypeRef) -> Option<Kind> {
    match type_ref {
        TypeRef::Optional(inner) => kind_of(inner),
        TypeRef::Array(_) | TypeRef::Map { .. } => None,
        TypeRef::Primitive(p) => Some(match p {
            PrimitiveType::Email => Kind::Email,
            PrimitiveType::Url => Kind::Url,
            PrimitiveType::String
            | PrimitiveType::Phone
            | PrimitiveType::Slug
            | PrimitiveType::Ip
            | PrimitiveType::IpV4
            | PrimitiveType::IpV6
            | PrimitiveType::Mac
            | PrimitiveType::Markdown
            | PrimitiveType::Html
            | PrimitiveType::Uuid => Kind::Text,
            PrimitiveType::Int
            | PrimitiveType::Int32
            | PrimitiveType::Int64
            | PrimitiveType::Float
            | PrimitiveType::Float32
            | PrimitiveType::Float64
            | PrimitiveType::Decimal
            | PrimitiveType::Money
            | PrimitiveType::Percentage => Kind::Number,
            _ => Kind::Other,
        }),
        TypeRef::Custom(_) | TypeRef::ModuleRef { .. } => Some(Kind::Other),
    }
}

fn number(v: &AttributeValue) -> Option<f64> {
    match v {
        AttributeValue::Int(i) => Some(*i as f64),
        AttributeValue::Float(f) => Some(*f),
        AttributeValue::String(s) | AttributeValue::Ident(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// A number as a violation parameter: an integer when it is whole.
fn param(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}_i64", n as i64)
    } else {
        format!("{n:?}_f64")
    }
}

/// `12.0` → `12`, for messages.
fn show(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn written(field: &Field, state_fields: &[String]) -> bool {
    !(SERVER_FILLED.contains(&field.name.as_str())
        || field.is_primary_key()
        || NOT_WRITTEN.iter().any(|a| field.has_attribute(a))
        || state_fields.iter().any(|s| s == &field.name)
        || matches!(
            field.lifecycle.as_ref().map(|l| l.shape),
            Some(LifecycleShape::HandSet | LifecycleShape::Projection)
        ))
}

struct Builder<'a> {
    model: &'a Model,
    field: &'a Field,
    out: Vec<CompiledRowRule>,
}

impl Builder<'_> {
    fn value(&self) -> String {
        format!("r::field(row, {})", rust_str(&self.field.name))
    }

    fn push(&mut self, code: &str, message: String, rust: String, sql: String, params: Vec<(String, String)>, source: String) {
        let name = &self.field.name;
        self.out.push(CompiledRowRule {
            name: format!("{name}_{code}"),
            code: code.to_string(),
            message,
            path: name.clone(),
            on_create: true,
            on_update: true,
            rust,
            sql: Some(sql),
            layer: "row",
            reads_old: false,
            changed_fields: vec![name.clone()],
            // Every check refuses unless the field keeps its checks in shadow (`@shadow(<why>)`).
            refuse: !self.field.has_attribute("shadow"),
            params,
            source: format!("{source} on {}.{name}", self.model.name),
        });
    }
}

/// The checks `model`'s field attributes declare, in field order. `state_fields` are the
/// fields a state machine moves, which only its verbs write.
pub fn field_rules(model: &Model, state_fields: &[String]) -> Vec<CompiledRowRule> {
    let mut out = Vec::new();
    for field in &model.fields {
        if !written(field, state_fields) {
            continue;
        }
        let Some(kind) = kind_of(&field.type_ref) else { continue };
        let mut b = Builder { model, field, out: Vec::new() };
        let col = field.name.clone();
        let v = b.value();
        let text = matches!(kind, Kind::Text | Kind::Email | Kind::Url);

        // required — refuses. A default fills an omitted value, but text still refuses a supplied
        // blank, unless the default is itself blank.
        let blank_default = matches!(field.default_value(), Some(AttributeValue::String(s) | AttributeValue::Ident(s)) if s.trim().is_empty());
        let defaulted = field.default_value().is_some();
        if field.has_attribute("required") && (!defaulted || (text && !field.type_ref.is_optional() && !blank_default)) {
            if text {
                b.push(
                    "required",
                    format!("{col} is required"),
                    format!("r::and(r::is_not_null({v}), r::not(r::is_empty(r::trim({v}))))"),
                    format!("({col} IS NOT NULL AND btrim({col}::text) <> '')"),
                    Vec::new(),
                    "@required".into());
            } else if field.type_ref.is_optional() {
                b.push(
                    "required",
                    format!("{col} is required"),
                    format!("r::is_not_null({v})"),
                    format!("({col} IS NOT NULL)"),
                    Vec::new(),
                    "@required".into());
            }
        }

        for attr in &field.attributes {
            let first = attr.first_arg().and_then(number);
            match (attr.name.as_str(), text, kind == Kind::Number) {
                ("max", true, _) => {
                    if let Some(n) = first {
                        b.push("max_length", format!("{col} must be at most {} characters", show(n)),
                            format!("r::le(r::length({v}), {})", rust_num(n)),
                            format!("(char_length({col}) <= {})", show(n)), vec![("max".into(), param(n))], format!("@max({})", show(n)));
                    }
                }
                ("min", true, _) => {
                    if let Some(n) = first {
                        b.push("min_length", format!("{col} must be at least {} characters", show(n)),
                            format!("r::ge(r::length({v}), {})", rust_num(n)),
                            format!("(char_length({col}) >= {})", show(n)), vec![("min".into(), param(n))], format!("@min({})", show(n)));
                    }
                }
                ("max", _, true) => {
                    if let Some(n) = first {
                        b.push("max", format!("{col} must be at most {}", show(n)),
                            format!("r::le({v}, {})", rust_num(n)),
                            format!("({col} <= {})", show(n)), vec![("max".into(), param(n))], format!("@max({})", show(n)));
                    }
                }
                ("min", _, true) => {
                    if let Some(n) = first {
                        b.push("min", format!("{col} must be at least {}", show(n)),
                            format!("r::ge({v}, {})", rust_num(n)),
                            format!("({col} >= {})", show(n)), vec![("min".into(), param(n))], format!("@min({})", show(n)));
                    }
                }
                ("length", true, _) => {
                    let max = attr.get_named("max").and_then(number);
                    let min = attr.get_named("min").and_then(number);
                    if max.is_none() && min.is_none() {
                        if let Some(n) = attr.args.iter().find(|(k, _)| k.is_none()).and_then(|(_, a)| number(a)) {
                            b.push("length", format!("{col} must be exactly {} characters", show(n)),
                                format!("r::eq(r::length({v}), {})", rust_num(n)),
                                format!("(char_length({col}) = {})", show(n)), vec![("length".into(), param(n))], format!("@length({})", show(n)));
                        }
                    }
                    if let Some(n) = max {
                        b.push("max_length", format!("{col} must be at most {} characters", show(n)),
                            format!("r::le(r::length({v}), {})", rust_num(n)),
                            format!("(char_length({col}) <= {})", show(n)), vec![("max".into(), param(n))], format!("@length(max={})", show(n)));
                    }
                    if let Some(n) = min {
                        b.push("min_length", format!("{col} must be at least {} characters", show(n)),
                            format!("r::ge(r::length({v}), {})", rust_num(n)),
                            format!("(char_length({col}) >= {})", show(n)), vec![("min".into(), param(n))], format!("@length(min={})", show(n)));
                    }
                }
                ("range", _, true) => {
                    let nums: Vec<f64> = attr.args.iter().filter_map(|(_, a)| number(a)).collect();
                    if let [lo, hi] = nums[..] {
                        b.push("range", format!("{col} must be between {} and {}", show(lo), show(hi)),
                            format!("r::and(r::ge({v}, {}), r::le({v}, {}))", rust_num(lo), rust_num(hi)),
                            format!("({col} BETWEEN {} AND {})", show(lo), show(hi)), vec![("min".into(), param(lo)), ("max".into(), param(hi))], format!("@range({}, {})", show(lo), show(hi)));
                    }
                }
                ("positive", _, true) => b.push("positive", format!("{col} must be positive"),
                    format!("r::gt({v}, {})", rust_num(0.0)), format!("({col} > 0)"), Vec::new(), "@positive".into()),
                ("non_negative" | "nonnegative", _, true) => b.push("non_negative", format!("{col} must not be negative"),
                    format!("r::ge({v}, {})", rust_num(0.0)), format!("({col} >= 0)"), Vec::new(), "@non_negative".into()),
                ("pattern" | "regex", true, _) => {
                    if let Some(p) = attr.first_arg().and_then(|a| a.as_str()) {
                        if regex::Regex::new(p).is_ok() {
                            b.push("pattern", format!("{col} does not have the expected form"),
                                format!("r::matches({v}, {})", rust_str(p)),
                                format!("({col} ~ {})", sql_str(p)), vec![("pattern".into(), rust_str(p))], format!("@{}", attr.name));
                        }
                    }
                }
                ("alpha_dash", true, _) => {
                    let p = "^[a-zA-Z0-9_-]+$";
                    b.push("alpha_dash", format!("{col} may hold only letters, digits, '_' and '-'"),
                        format!("r::matches({v}, {})", rust_str(p)), format!("({col} ~ {})", sql_str(p)), Vec::new(), "@alpha_dash".into());
                }
                ("alpha_num", true, _) => {
                    let p = "^[a-zA-Z0-9]+$";
                    b.push("alpha_num", format!("{col} may hold only letters and digits"),
                        format!("r::matches({v}, {})", rust_str(p)), format!("({col} ~ {})", sql_str(p)), Vec::new(), "@alpha_num".into());
                }
                ("values", _, _) => {
                    let values: Vec<String> = attr
                        .args
                        .iter()
                        .filter_map(|(_, a)| a.as_str().map(str::to_string))
                        .flat_map(|s| s.split('|').map(|p| p.trim().to_string()).collect::<Vec<_>>())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if !values.is_empty() {
                        let rs: Vec<String> = values.iter().map(|s| format!("r::text({})", rust_str(s))).collect();
                        let ss: Vec<String> = values.iter().map(|s| sql_str(s)).collect();
                        b.push("values", format!("{col} must be one of {}", values.join(", ")),
                            format!("r::in_list({v}, r::Val::List(vec![{}]))", rs.join(", ")),
                            format!("({col}::text IN ({}))", ss.join(", ")), Vec::new(), "@values".into());
                    }
                }
                _ => {}
            }
        }

        // The format a type promises.
        match kind {
            Kind::Email => {
                let p = r"^[^@\s]+@[^@\s]+\.[^@\s]+$";
                b.push("email", format!("{col} must be an email address"),
                    format!("r::matches({v}, {})", rust_str(p)), format!("({col} ~ {})", sql_str(p)), Vec::new(), "type email".into());
            }
            Kind::Url => {
                let p = r"^[a-zA-Z][a-zA-Z0-9+.-]*://\S+$";
                b.push("url", format!("{col} must be an absolute URL"),
                    format!("r::matches({v}, {})", rust_str(p)), format!("({col} ~ {})", sql_str(p)), Vec::new(), "type url".into());
            }
            _ => {}
        }
        out.extend(b.out);
    }
    out
}

/// The field checks the generator compiles for `model` of `schema`.
pub fn model_field_rules(schema: &crate::ast::ModuleSchema, model: &Model) -> Vec<CompiledRowRule> {
    field_rules(model, &super::compile::model_state_fields(schema, model))
}

/// A field check as the browser runs it: the same code, message and values the write guard
/// answers with, and a TypeScript test over the field's value `v`. The browser runs it only on
/// a value that is there; a null passes, as it does on the service, unless the check is
/// `required`.
#[derive(Debug, Clone, PartialEq)]
pub struct BrowserCheck {
    pub field: String,
    pub code: String,
    /// The service's message said under the field: "This field must be at most 80 characters".
    pub message: String,
    pub params: Vec<(String, serde_json::Value)>,
    /// A TypeScript boolean expression over `v` that holds when the value passes.
    pub test: String,
}

/// The field checks of `model` the browser can run, from the same compiled rules the write guard
/// refuses with. A check kept in shadow is left out: the service lets that write through.
pub fn browser_checks(schema: &crate::ast::ModuleSchema, model: &Model) -> Vec<BrowserCheck> {
    model_field_rules(schema, model)
        .into_iter()
        .filter(|r| r.refuse)
        .filter_map(|r| {
            let field = model.fields.iter().find(|f| f.name == r.path)?;
            let test = browser_test(&r.code, field, &r.params)?;
            let message = r
                .message
                .strip_prefix(&format!("{} ", r.path))
                .map(|rest| format!("This field {rest}"))
                .unwrap_or_else(|| r.message.clone());
            let params = r
                .params
                .iter()
                .filter(|(k, _)| k != "pattern")
                .map(|(k, v)| (k.clone(), param_value(v)))
                .collect();
            Some(BrowserCheck { field: r.path.clone(), code: r.code.clone(), message, params, test })
        })
        .collect()
}

/// A guard param (`80_i64`, `1.5_f64`) as the value it names.
fn param_value(v: &str) -> serde_json::Value {
    let bare = v.trim_end_matches("_i64").trim_end_matches("_f64");
    bare.parse::<i64>()
        .map(serde_json::Value::from)
        .or_else(|_| bare.parse::<f64>().map(serde_json::Value::from))
        .unwrap_or_else(|_| serde_json::Value::from(v))
}

/// The TypeScript test for one check code over the value `v`, or `None` for a code the browser
/// cannot run. Lengths count characters, as the service does, not UTF-16 units.
fn browser_test(code: &str, field: &Field, params: &[(String, String)]) -> Option<String> {
    let num = |name: &str| params.iter().find(|(k, _)| k == name).map(|(_, v)| param_value(v).to_string());
    let regex = |p: &str| format!("new RegExp({}).test(String(v))", serde_json::Value::from(p));
    let attr_pattern = || {
        field
            .attributes
            .iter()
            .find(|a| a.name == "pattern" || a.name == "regex")
            .and_then(|a| a.first_arg())
            .and_then(|a| a.as_str())
            .map(str::to_string)
    };
    Some(match code {
        "required" if field.type_ref.is_optional() && !matches!(kind_of(&field.type_ref), Some(Kind::Text | Kind::Email | Kind::Url)) => {
            "v != null".to_string()
        }
        "required" => "v != null && String(v).trim() !== \"\"".to_string(),
        "max_length" => format!("[...String(v)].length <= {}", num("max")?),
        "min_length" => format!("[...String(v)].length >= {}", num("min")?),
        "length" => format!("[...String(v)].length === {}", num("length")?),
        "max" => format!("Number(v) <= {}", num("max")?),
        "min" => format!("Number(v) >= {}", num("min")?),
        "range" => format!("Number(v) >= {} && Number(v) <= {}", num("min")?, num("max")?),
        "positive" => "Number(v) > 0".to_string(),
        "non_negative" => "Number(v) >= 0".to_string(),
        "pattern" => regex(&attr_pattern()?),
        "alpha_dash" => regex("^[a-zA-Z0-9_-]+$"),
        "alpha_num" => regex("^[a-zA-Z0-9]+$"),
        "email" => regex(r"^[^@\s]+@[^@\s]+\.[^@\s]+$"),
        "url" => regex(r"^[a-zA-Z][a-zA-Z0-9+.-]*://\S+$"),
        "values" => {
            let values: Vec<serde_json::Value> = field
                .attributes
                .iter()
                .filter(|a| a.name == "values")
                .flat_map(|a| a.args.iter().filter_map(|(_, v)| v.as_str().map(str::to_string)).collect::<Vec<_>>())
                .flat_map(|s| s.split('|').map(|p| p.trim().to_string()).collect::<Vec<_>>())
                .filter(|s| !s.is_empty())
                .map(serde_json::Value::from)
                .collect();
            format!("{}.includes(String(v))", serde_json::Value::from(values))
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_browser_gets_each_check_with_the_guards_code_and_message() {
        let m = model("      name:\n        type: string\n        attributes: [\"@required\", \"@max(80)\"]\n      score:\n        type: int?\n        attributes: [\"@range(1, 12)\"]\n      email:\n        type: email?\n");
        let schema = crate::ast::ModuleSchema::new("helpdesk");
        let checks = browser_checks(&schema, &m);
        let of = |code: &str| checks.iter().find(|c| c.code == code).unwrap_or_else(|| panic!("{code} in {checks:?}"));
        assert_eq!(of("required").test, "v != null && String(v).trim() !== \"\"");
        assert_eq!(of("max_length").message, "This field must be at most 80 characters");
        assert_eq!(of("max_length").test, "[...String(v)].length <= 80");
        assert_eq!(of("max_length").params, vec![("max".to_string(), serde_json::Value::from(80))]);
        assert_eq!(of("range").test, "Number(v) >= 1 && Number(v) <= 12");
        assert!(of("email").test.starts_with("new RegExp(\"^[^@\\\\s]+@"), "{}", of("email").test);
    }

    fn model(fields_yaml: &str) -> Model {
        let yaml = format!(
            "models:\n  - name: Ticket\n    collection: tickets\n    fields:\n      id:\n        type: uuid\n        attributes: [\"@id\", \"@default(uuid)\"]\n{fields_yaml}      metadata:\n        type: Metadata\n        attributes: [\"@audit_metadata\"]\n"
        );
        let schema = crate::parser::yaml_parser::parse_model_yaml_str(&yaml).expect("parses");
        schema.into_models().expect("converts").remove(0)
    }

    fn rule<'a>(rules: &'a [CompiledRowRule], name: &str) -> &'a CompiledRowRule {
        rules.iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no {name} in {:?}", rules.iter().map(|r| &r.name).collect::<Vec<_>>()))
    }

    #[test]
    fn every_field_check_refuses_and_shadow_keeps_them_reporting() {
        let m = model("      title:\n        type: string\n        attributes: [\"@required\", \"@max(80)\"]\n");
        let rules = field_rules(&m, &[]);
        let required = rule(&rules, "title_required");
        assert!(required.refuse);
        assert_eq!(required.rust, "r::and(r::is_not_null(r::field(row, \"title\")), r::not(r::is_empty(r::trim(r::field(row, \"title\")))))");
        assert_eq!(required.changed_fields, vec!["title".to_string()], "an update is checked only when it changes the field");
        let max = rule(&rules, "title_max_length");
        assert!(max.refuse, "a length limit refuses too");
        assert_eq!(max.params, vec![("max".to_string(), "80_i64".to_string())]);
        assert_eq!(max.sql.as_deref(), Some("(char_length(title) <= 80)"));
        assert!(rules.iter().all(|r| r.path != "id" && r.path != "metadata"), "server-filled fields carry no checks");
    }

    /// A default fills a text field a create leaves out, but a blank the writer supplies is still
    /// refused: the default is no licence to send "". A blank default, and a field nobody marked
    /// required, carry no check.
    #[test]
    fn a_defaulted_required_text_field_still_refuses_a_supplied_blank() {
        let m = model(
            "      status_label:\n        type: string\n        attributes: [\"@required\", \"@default(open)\"]\n      prefix:\n        type: string\n        attributes: [\"@required\", \"@default('')\"]\n      note:\n        type: string\n        attributes: []\n",
        );
        let rules = field_rules(&m, &[]);
        let required: Vec<&str> = rules.iter().filter(|r| r.code == "required").map(|r| r.path.as_str()).collect();
        assert_eq!(required, ["status_label"]);
        assert_eq!(
            rule(&rules, "status_label_required").rust,
            "r::and(r::is_not_null(r::field(row, \"status_label\")), r::not(r::is_empty(r::trim(r::field(row, \"status_label\")))))"
        );
    }

    #[test]
    fn a_state_field_is_left_to_its_verbs() {
        let m = model("      stage:\n        type: string\n        attributes: [\"@required\"]\n");
        assert!(field_rules(&m, &["stage".to_string()]).is_empty());
    }

    #[test]
    fn length_reads_both_spellings_and_numbers_read_as_bounds() {
        let m = model(
            "      code:\n        type: string\n        attributes: [\"@length(3)\"]\n      label:\n        type: string\n        attributes: [\"@length(max=120)\"]\n      qty:\n        type: int\n        attributes: [\"@min(1)\", \"@max(500)\"]\n      rate:\n        type: decimal\n        attributes: [\"@non_negative\"]\n",
        );
        let rules = field_rules(&m, &[]);
        assert_eq!(rule(&rules, "code_length").rust, "r::eq(r::length(r::field(row, \"code\")), r::Val::Num(3.0_f64))");
        assert_eq!(rule(&rules, "label_max_length").params, vec![("max".to_string(), "120_i64".to_string())]);
        assert_eq!(rule(&rules, "qty_min").rust, "r::ge(r::field(row, \"qty\"), r::Val::Num(1.0_f64))", "@min(1) is a lower bound of 1, not non-negative");
        assert_eq!(rule(&rules, "qty_max").sql.as_deref(), Some("(qty <= 500)"));
        assert_eq!(rule(&rules, "rate_non_negative").sql.as_deref(), Some("(rate >= 0)"));
    }

    #[test]
    fn a_pattern_keeps_its_colons_and_commas() {
        let m = model("      name:\n        type: string\n        attributes: [\"@pattern(^[a-z_]+:[a-z_*]{1,3}$)\"]\n");
        let rules = field_rules(&m, &[]);
        assert_eq!(rule(&rules, "name_pattern").rust, "r::matches(r::field(row, \"name\"), \"^[a-z_]+:[a-z_*]{1,3}$\")");
    }

    #[test]
    fn the_rendered_guard_refuses_and_a_shadowed_field_only_reports() {
        let m = model("      title:\n        type: string\n        attributes: [\"@required\", \"@max(80)\"]\n      note:\n        type: string\n        attributes: [\"@max(10)\", \"@shadow(stored notes run longer; widen after the cleanup)\"]\n");
        let rules = field_rules(&m, &[]);
        let guard = super::super::compile::render_guard(&m, &rules, &[], &[]);
        assert!(guard.contains("let mut refuse = Vec::new();"), "{guard}");
        assert!(guard.contains("refuse.push(Violation::new(\"title\", \"required\", \"title is required\"));"), "{guard}");
        assert!(guard.contains("refuse.push(Violation::new(\"title\", \"max_length\", \"title must be at most 80 characters\").with_param(\"max\", 80_i64));"), "{guard}");
        assert!(guard.contains("shadow.push(Violation::new(\"note\", \"max_length\""), "{guard}");
        assert!(guard.contains("(create || (update && r::changed(old.as_ref(), row, &[\"title\"])))"), "{guard}");
        assert!(guard.contains("GuardOutcome { refuse, shadow }"), "{guard}");
    }
}
