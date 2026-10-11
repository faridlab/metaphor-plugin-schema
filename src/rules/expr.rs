//! The rule expression language: one strict grammar for rule conditions,
//! assertions and transition guards, and a checker that resolves every name
//! an expression uses against the model it governs.
//!
//! The syntax is Rust/JS style, the dialect most hook files already write:
//!
//! ```text
//! expr    := or ( "?" expr ":" expr )?
//! or      := and ( "||" and )*
//! and     := cmp ( "&&" cmp )*
//! cmp     := add ( ( "==" | "!=" | "<" | "<=" | ">" | ">=" | "in" | "not" "in" | "matches" ) add )?
//! add     := mul ( ( "+" | "-" ) mul )*
//! mul     := unary ( ( "*" | "/" | "%" ) unary )*
//! unary   := ( "!" | "-" ) unary | postfix
//! postfix := primary ( "." name ( "(" args ")" )? )*
//! primary := number | string | "true" | "false" | "null" | "[" args "]"
//!          | "(" expr ")" | "$" name | name ( "(" args ")" )?
//! ```
//!
//! Strings take single or double quotes. `5.minutes` is a duration. `$this`,
//! `$old`, `$actor` and `$now` name the row being written, the row before the
//! write, the person writing and the time of the write. Nothing else parses:
//! SQL words (`AND`, `IS NULL`, `<>`), `=` for comparison, `=>`, set braces
//! and `Σ` are errors, so an expression either means one thing or is refused.
//! Unlike the computed-field parser, the whole text must be consumed.

use serde::{Deserialize, Serialize};

use crate::ast::expressions::{BinaryOp, Expression, FieldRef, Literal, UnaryOp};
use crate::ast::Model;

/// Whether a declared expression can be compiled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ExpressionCheck {
    /// The rule carries no expression (a uniqueness key, or nothing).
    NotAnExpression,
    /// It parses, and every name and function it uses is known.
    Ready,
    /// It parses, but names or functions in it are not known for its model:
    /// a field it does not have, a lookup into another entity, a validator
    /// that only hand-written code can supply.
    Unresolved { names: Vec<String>, functions: Vec<String> },
    /// It does not parse in the rule grammar: prose, SQL or set notation.
    Unparsed { error: String },
}

impl ExpressionCheck {
    pub fn label(&self) -> &'static str {
        match self {
            ExpressionCheck::NotAnExpression => "no expression",
            ExpressionCheck::Ready => "ready",
            ExpressionCheck::Unresolved { .. } => "unresolved",
            ExpressionCheck::Unparsed { .. } => "unparsed",
        }
    }
}

/// Functions the rule language defines.
const FUNCTIONS: &[&str] = &[
    "now", "count", "sum", "min", "max", "avg", "abs", "ceil", "floor", "round", "len", "coalesce", "lower",
    "upper", "exists",
];
/// Methods the rule language defines on a value.
const METHODS: &[&str] = &[
    "length", "len", "contains", "starts_with", "ends_with", "trim", "lower", "upper", "is_empty", "has_role",
];
/// Duration units: `30.days`, `5.minutes`.
const DURATIONS: &[&str] = &[
    "second", "seconds", "minute", "minutes", "hour", "hours", "day", "days", "week", "weeks", "month",
    "months", "year", "years",
];
/// Context variables and the members each offers beyond the model's fields.
const CONTEXTS: &[&str] = &["this", "old", "actor", "now"];

/// Parse `source` in the rule grammar.
pub fn parse(source: &str) -> Result<Expression, String> {
    let tokens = lex(source)?;
    let mut p = Parser { tokens, pos: 0 };
    let expr = p.expr()?;
    match p.peek() {
        None => Ok(expr),
        Some(Tok::Name(w)) if is_sql_word(w) => Err(sql_hint(w)),
        Some(t) => Err(format!("unexpected `{}` after a complete expression", t.text())),
    }
}

fn is_sql_word(w: &str) -> bool {
    matches!(w, "AND" | "OR" | "NOT" | "IS" | "BETWEEN" | "LIKE" | "and" | "or" | "is")
}

fn sql_hint(w: &str) -> String {
    format!("`{w}` is SQL or prose; write `&&`, `||`, `!` or `== null`")
}

/// Parse `source` and resolve its names against `model`.
pub fn check(source: &str, model: Option<&Model>) -> ExpressionCheck {
    let expr = match parse(source) {
        Ok(e) => e,
        Err(error) => return ExpressionCheck::Unparsed { error },
    };
    let mut names = Vec::new();
    let mut functions = Vec::new();
    resolve(&expr, model, &mut names, &mut functions);
    names.sort();
    names.dedup();
    functions.sort();
    functions.dedup();
    if names.is_empty() && functions.is_empty() {
        ExpressionCheck::Ready
    } else {
        ExpressionCheck::Unresolved { names, functions }
    }
}

fn has_name(model: Option<&Model>, name: &str) -> bool {
    model.is_some_and(|m| m.fields.iter().any(|f| f.name == name) || m.relations.iter().any(|r| r.name == name))
}

fn resolve(expr: &Expression, model: Option<&Model>, names: &mut Vec<String>, functions: &mut Vec<String>) {
    match expr {
        Expression::Literal(_) | Expression::Raw(_) => {}
        Expression::FieldRef(FieldRef::Simple(n)) => {
            if !has_name(model, n) {
                names.push(n.clone());
            }
        }
        Expression::FieldRef(FieldRef::Path(p)) => {
            if let Some(first) = p.first() {
                if !has_name(model, first) {
                    names.push(p.join("."));
                }
            }
        }
        // Another entity's field: a lookup the rule language cannot resolve
        // against this model.
        Expression::FieldRef(FieldRef::CrossRef { model: other, field }) => {
            names.push(format!("{other}.{field}"));
        }
        Expression::Context(c) => {
            if !CONTEXTS.contains(&c.as_str()) {
                names.push(format!("${c}"));
            }
        }
        Expression::Binary { left, right, .. } => {
            resolve(left, model, names, functions);
            resolve(right, model, names, functions);
        }
        Expression::Unary { expr, .. } => resolve(expr, model, names, functions),
        Expression::Conditional { condition, then_expr, else_expr } => {
            for e in [condition, then_expr, else_expr] {
                resolve(e, model, names, functions);
            }
        }
        Expression::Array(items) => items.iter().for_each(|e| resolve(e, model, names, functions)),
        Expression::FunctionCall { name, args } => {
            if !FUNCTIONS.contains(&name.as_str()) {
                functions.push(name.clone());
            }
            // An aggregate's first argument names a relation, and its
            // predicate speaks of that relation's rows, not of this model.
            if matches!(name.as_str(), "count" | "sum" | "min" | "max" | "avg" | "exists") {
                // `count(lines)`, `sum(lines.debit)`: the relation is the root
                // of the first argument, however deep the path. An entity name
                // (`count(User, …)`) is a lookup into another entity.
                if let Some(root) = args.first().and_then(root_name) {
                    if root.chars().next().is_some_and(char::is_uppercase) {
                        names.push(format!("{root} (another entity)"));
                    } else if !has_name(model, &root) {
                        names.push(root);
                    }
                }
                return;
            }
            args.iter().for_each(|a| resolve(a, model, names, functions));
        }
        Expression::Aggregate { collection, predicate, .. } => {
            resolve(collection, model, names, functions);
            if let Some(p) = predicate {
                resolve(p, model, names, functions);
            }
        }
        Expression::MemberAccess { object, member } => {
            match object.as_ref() {
                // `30.days`
                Expression::Literal(Literal::Int(_) | Literal::Float(_)) if DURATIONS.contains(&member.as_str()) => {}
                // `$this.x` / `$old.x` name a field of the model.
                Expression::Context(c) if c == "this" || c == "old" => {
                    if !has_name(model, member) {
                        names.push(format!("${c}.{member}"));
                    }
                }
                Expression::Context(c) if c == "actor" => {}
                // `account.subtype`: a relation's field; the relation must exist.
                _ => resolve(object, model, names, functions),
            }
        }
        Expression::MethodCall { object, method, args } => {
            if !METHODS.contains(&method.as_str()) {
                functions.push(format!(".{method}"));
            }
            if !matches!(object.as_ref(), Expression::Context(c) if c == "actor") {
                resolve(object, model, names, functions);
            }
            args.iter().for_each(|a| resolve(a, model, names, functions));
        }
    }
}

/// The identifier a path starts from: `lines` in `lines.debit.amount`.
fn root_name(e: &Expression) -> Option<String> {
    match e {
        Expression::FieldRef(FieldRef::Simple(n)) => Some(n.clone()),
        Expression::FieldRef(FieldRef::Path(p)) => p.first().cloned(),
        Expression::MemberAccess { object, .. } | Expression::MethodCall { object, .. } => root_name(object),
        _ => None,
    }
}

// ── Lexer ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Int(i64),
    Float(f64),
    Str(String),
    Name(String),
    Context(String),
    Sym(&'static str),
}

impl Tok {
    fn text(&self) -> String {
        match self {
            Tok::Int(n) => n.to_string(),
            Tok::Float(n) => n.to_string(),
            Tok::Str(s) => format!("'{s}'"),
            Tok::Name(n) => n.clone(),
            Tok::Context(c) => format!("${c}"),
            Tok::Sym(s) => s.to_string(),
        }
    }
}

const SYMBOLS: &[&str] = &[
    "||", "&&", "==", "!=", "<=", ">=", "<", ">", "+", "-", "*", "/", "%", "!", "(", ")", "[", "]", ",", ".", "?",
    ":",
];

fn lex(source: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '\'' || c == '"' {
            let quote = c;
            let mut s = String::new();
            i += 1;
            loop {
                match chars.get(i) {
                    None => return Err("a string is not closed".to_string()),
                    // A backslash escapes only a quote or itself; any other `\x` stays as written,
                    // so a pattern keeps its regex escapes (`\d`, `\.`, `\+`).
                    Some('\\') => {
                        match chars.get(i + 1) {
                            Some(&n) if n == '\'' || n == '"' || n == '\\' => s.push(n),
                            Some(&n) => {
                                s.push('\\');
                                s.push(n);
                            }
                            None => s.push('\\'),
                        }
                        i += 2;
                    }
                    Some(&ch) if ch == quote => {
                        i += 1;
                        break;
                    }
                    Some(&ch) => {
                        s.push(ch);
                        i += 1;
                    }
                }
            }
            out.push(Tok::Str(s));
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            // A fraction only when a digit follows the dot: `30.days` is a duration.
            if i + 1 < chars.len() && chars[i] == '.' && chars[i + 1].is_ascii_digit() {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                out.push(Tok::Float(text.parse().map_err(|_| format!("`{text}` is not a number"))?));
            } else {
                let text: String = chars[start..i].iter().collect();
                out.push(Tok::Int(text.parse().map_err(|_| format!("`{text}` is not a number"))?));
            }
            continue;
        }
        if c.is_alphabetic() || c == '_' || c == '$' {
            let context = c == '$';
            let start = if context { i + 1 } else { i };
            i = start;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if word.is_empty() {
                return Err("`$` must name a context: $this, $old, $actor or $now".to_string());
            }
            if !context && word.chars().any(|ch| !ch.is_ascii()) {
                return Err(format!("`{word}` is not a name the rule language reads"));
            }
            out.push(if context { Tok::Context(word) } else { Tok::Name(word) });
            continue;
        }
        let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
        if rest == "=>" {
            return Err("`=>` is not an operator here; write the implication as `!a || b`".to_string());
        }
        if rest == "<>" {
            return Err("`<>` is SQL; write `!=`".to_string());
        }
        if c == '=' && rest != "==" {
            return Err("`=` compares nothing here; write `==`".to_string());
        }
        match SYMBOLS.iter().find(|s| rest.starts_with(**s)) {
            Some(s) => {
                out.push(Tok::Sym(s));
                i += s.len();
            }
            None => return Err(format!("unexpected `{c}`")),
        }
    }
    Ok(out)
}

// ── Parser ───────────────────────────────────────────────────────────────

struct Parser {
    tokens: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }

    fn peek_at(&self, n: usize) -> Option<&Tok> {
        self.tokens.get(self.pos + n)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn eat(&mut self, sym: &str) -> bool {
        if matches!(self.peek(), Some(Tok::Sym(s)) if *s == sym) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_word(&mut self, word: &str) -> bool {
        if matches!(self.peek(), Some(Tok::Name(w)) if w == word) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, sym: &str) -> Result<(), String> {
        if self.eat(sym) {
            Ok(())
        } else {
            Err(match self.peek() {
                Some(t) => format!("expected `{sym}`, found `{}`", t.text()),
                None => format!("expected `{sym}` before the end"),
            })
        }
    }

    fn expr(&mut self) -> Result<Expression, String> {
        let condition = self.or()?;
        if self.eat("?") {
            let then_expr = self.expr()?;
            self.expect(":")?;
            let else_expr = self.expr()?;
            return Ok(Expression::Conditional {
                condition: Box::new(condition),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            });
        }
        Ok(condition)
    }

    fn binary_chain(
        &mut self,
        ops: &[(&'static str, BinaryOp)],
        next: fn(&mut Self) -> Result<Expression, String>,
    ) -> Result<Expression, String> {
        let mut left = next(self)?;
        'outer: loop {
            for (sym, op) in ops {
                if self.eat(sym) {
                    let right = next(self)?;
                    left = bin(left, *op, right);
                    continue 'outer;
                }
            }
            return Ok(left);
        }
    }

    fn or(&mut self) -> Result<Expression, String> {
        self.binary_chain(&[("||", BinaryOp::Or)], Self::and)
    }

    fn and(&mut self) -> Result<Expression, String> {
        self.binary_chain(&[("&&", BinaryOp::And)], Self::cmp)
    }

    fn cmp(&mut self) -> Result<Expression, String> {
        let left = self.add()?;
        let op = match self.peek() {
            Some(Tok::Sym("==")) => Some(BinaryOp::Eq),
            Some(Tok::Sym("!=")) => Some(BinaryOp::Ne),
            Some(Tok::Sym("<")) => Some(BinaryOp::Lt),
            Some(Tok::Sym("<=")) => Some(BinaryOp::Le),
            Some(Tok::Sym(">")) => Some(BinaryOp::Gt),
            Some(Tok::Sym(">=")) => Some(BinaryOp::Ge),
            _ => None,
        };
        if let Some(op) = op {
            self.pos += 1;
            let right = self.add()?;
            return Ok(bin(left, op, right));
        }
        if self.eat_word("in") {
            let right = self.add()?;
            return Ok(bin(left, BinaryOp::In, right));
        }
        if matches!(self.peek(), Some(Tok::Name(w)) if w == "not")
            && matches!(self.peek_at(1), Some(Tok::Name(w)) if w == "in")
        {
            self.pos += 2;
            let right = self.add()?;
            return Ok(bin(left, BinaryOp::NotIn, right));
        }
        if self.eat_word("matches") {
            let right = self.add()?;
            return Ok(bin(left, BinaryOp::Matches, right));
        }
        Ok(left)
    }

    fn add(&mut self) -> Result<Expression, String> {
        self.binary_chain(&[("+", BinaryOp::Add), ("-", BinaryOp::Sub)], Self::mul)
    }

    fn mul(&mut self) -> Result<Expression, String> {
        self.binary_chain(&[("*", BinaryOp::Mul), ("/", BinaryOp::Div), ("%", BinaryOp::Mod)], Self::unary)
    }

    fn unary(&mut self) -> Result<Expression, String> {
        if self.eat("!") {
            return Ok(Expression::Unary { op: UnaryOp::Not, expr: Box::new(self.unary()?) });
        }
        if self.eat("-") {
            return Ok(Expression::Unary { op: UnaryOp::Neg, expr: Box::new(self.unary()?) });
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expression, String> {
        let mut expr = self.primary()?;
        while self.eat(".") {
            let member = match self.next() {
                Some(Tok::Name(n)) => n,
                Some(t) => return Err(format!("expected a name after `.`, found `{}`", t.text())),
                None => return Err("expected a name after `.`".to_string()),
            };
            if self.eat("(") {
                let args = self.args(")")?;
                expr = Expression::MethodCall { object: Box::new(expr), method: member, args };
            } else {
                expr = Expression::MemberAccess { object: Box::new(expr), member };
            }
        }
        Ok(expr)
    }

    fn args(&mut self, close: &str) -> Result<Vec<Expression>, String> {
        let mut args = Vec::new();
        if self.eat(close) {
            return Ok(args);
        }
        loop {
            args.push(self.expr()?);
            if self.eat(close) {
                return Ok(args);
            }
            self.expect(",")?;
        }
    }

    fn primary(&mut self) -> Result<Expression, String> {
        match self.next() {
            Some(Tok::Int(n)) => Ok(Expression::Literal(Literal::Int(n))),
            Some(Tok::Float(n)) => Ok(Expression::Literal(Literal::Float(n))),
            Some(Tok::Str(s)) => Ok(Expression::Literal(Literal::String(s))),
            Some(Tok::Context(c)) => Ok(Expression::Context(c)),
            Some(Tok::Sym("(")) => {
                let e = self.expr()?;
                self.expect(")")?;
                Ok(e)
            }
            Some(Tok::Sym("[")) => Ok(Expression::Array(self.args("]")?)),
            Some(Tok::Name(n)) => match n.as_str() {
                "true" => Ok(Expression::Literal(Literal::Bool(true))),
                "false" => Ok(Expression::Literal(Literal::Bool(false))),
                "null" => Ok(Expression::Literal(Literal::Null)),
                w if is_sql_word(w) => Err(sql_hint(w)),
                _ => {
                    if self.eat("(") {
                        let args = self.args(")")?;
                        Ok(Expression::FunctionCall { name: n, args })
                    } else {
                        Ok(Expression::FieldRef(FieldRef::Simple(n)))
                    }
                }
            },
            Some(t) => Err(format!("unexpected `{}`", t.text())),
            None => Err("the expression ends too early".to_string()),
        }
    }
}

fn bin(left: Expression, op: BinaryOp, right: Expression) -> Expression {
    Expression::Binary { left: Box::new(left), op, right: Box::new(right) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Field, PrimitiveType, TypeRef};

    fn journal() -> Model {
        let mut m = Model::new("Journal");
        for name in ["total_debit", "total_credit", "status", "posted_at", "requires_approval"] {
            m.fields.push(Field::new(name, TypeRef::Primitive(PrimitiveType::String)));
        }
        m
    }

    #[test]
    fn the_majority_dialect_parses_and_resolves() {
        let m = journal();
        for src in [
            "total_debit == total_credit",
            "status != 'posted' || posted_at != null",
            "status in ['draft', 'posted'] && !requires_approval",
            "$this.status == $old.status",
            "posted_at > now() - 30.days",
            "$actor.has_role('admin') || $actor.id == $this.status",
            "total_debit >= 0 ? true : false",
        ] {
            assert_eq!(check(src, Some(&m)), ExpressionCheck::Ready, "{src}");
        }
    }

    #[test]
    fn a_backslash_in_a_string_stays_unless_it_escapes_a_quote_or_itself() {
        // The string on the right of `a == '…'` / `a matches '…'`, as the lexer read it.
        fn string_of(src: &str) -> String {
            match parse(src).unwrap() {
                Expression::Binary { right, .. } => match *right {
                    Expression::Literal(Literal::String(s)) => s,
                    other => panic!("{src}: expected a string, got {other:?}"),
                },
                other => panic!("{src}: expected a comparison, got {other:?}"),
            }
        }
        // A pattern keeps its regex escapes: `\d` is a digit class, not the letter d.
        assert_eq!(string_of(r"phone matches '^\+?[1-9]\d{1,14}$'"), r"^\+?[1-9]\d{1,14}$");
        assert_eq!(string_of(r"a == 'it\'s'"), "it's");
        assert_eq!(string_of(r"a == 'back\\slash'"), r"back\slash");
    }

    #[test]
    fn sql_prose_and_set_notation_are_refused_with_a_reason() {
        for (src, says) in [
            ("schedule_end IS NULL OR schedule_end >= schedule_date", "SQL"),
            ("a <> b", "`<>` is SQL"),
            ("a = b", "write `==`"),
            ("x present => y present", "write the implication as `!a || b`"),
            ("subtype in {receivable, payable}", "unexpected `{`"),
            ("Σ cost_share <= 100", "is not a name"),
            ("total_debit == total_credit total_debit", "after a complete expression"),
        ] {
            match check(src, None) {
                ExpressionCheck::Unparsed { error } => assert!(error.contains(says), "{src}: {error}"),
                other => panic!("{src}: expected unparsed, got {other:?}"),
            }
        }
    }

    #[test]
    fn names_the_model_lacks_and_unknown_functions_are_listed() {
        let m = journal();
        assert_eq!(
            check("get_bucket(bucket_id).status == 'active' && is_valid_url(link)", Some(&m)),
            ExpressionCheck::Unresolved {
                names: vec!["bucket_id".into(), "link".into()],
                functions: vec!["get_bucket".into(), "is_valid_url".into()],
            }
        );
        // The relation an aggregate reads must exist, however deep its path.
        assert_eq!(
            check("sum(lines.debit) == sum(lines.credit)", Some(&m)),
            ExpressionCheck::Unresolved { names: vec!["lines".into()], functions: vec![] }
        );
        assert_eq!(
            check("count(User, email == $this.status) == 0", Some(&m)),
            ExpressionCheck::Unresolved { names: vec!["User (another entity)".into()], functions: vec![] }
        );
        assert_eq!(
            check("$this.missing == 1", Some(&m)),
            ExpressionCheck::Unresolved { names: vec!["$this.missing".into()], functions: vec![] }
        );
    }
}
