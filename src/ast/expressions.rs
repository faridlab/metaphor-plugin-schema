//! Expression AST definitions
//!
//! Defines AST nodes for expressions used in rules, conditions, and computed fields.

use serde::{Deserialize, Serialize};

/// An expression in the schema language
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Expression {
    /// Literal value
    Literal(Literal),
    /// Field reference (e.g., `field.name` or just `name`)
    FieldRef(FieldRef),
    /// Binary operation (e.g., `a > b`, `a && b`)
    Binary {
        left: Box<Expression>,
        op: BinaryOp,
        right: Box<Expression>,
    },
    /// Unary operation (e.g., `!a`)
    Unary {
        op: UnaryOp,
        expr: Box<Expression>,
    },
    /// Function call (e.g., `count(items)`, `now()`)
    FunctionCall {
        name: String,
        args: Vec<Expression>,
    },
    /// Aggregate expression (e.g., `any(items, item.active)`)
    Aggregate {
        function: AggregateFunc,
        collection: Box<Expression>,
        predicate: Option<Box<Expression>>,
    },
    /// Conditional expression (ternary)
    Conditional {
        condition: Box<Expression>,
        then_expr: Box<Expression>,
        else_expr: Box<Expression>,
    },
    /// Array literal
    Array(Vec<Expression>),
    /// Member access (e.g., `user.profile.name`)
    MemberAccess {
        object: Box<Expression>,
        member: String,
    },
    /// Method call (e.g., `value.contains("test")`)
    MethodCall {
        object: Box<Expression>,
        method: String,
        args: Vec<Expression>,
    },
    /// Context variable (e.g., `$user`, `$now`, `$old`)
    Context(String),
    /// Raw expression string (unparsed, from YAML)
    Raw(String),
}

impl Default for Expression {
    fn default() -> Self {
        Self::Literal(Literal::Bool(true))
    }
}

impl Expression {
    /// Create a field reference expression
    pub fn field(name: impl Into<String>) -> Self {
        Self::FieldRef(FieldRef::Simple(name.into()))
    }

    /// Create a literal string
    pub fn string(value: impl Into<String>) -> Self {
        Self::Literal(Literal::String(value.into()))
    }

    /// Create a literal integer
    pub fn int(value: i64) -> Self {
        Self::Literal(Literal::Int(value))
    }

    /// Create a literal boolean
    pub fn bool(value: bool) -> Self {
        Self::Literal(Literal::Bool(value))
    }

    /// Create a binary expression
    pub fn binary(left: Expression, op: BinaryOp, right: Expression) -> Self {
        Self::Binary {
            left: Box::new(left),
            op,
            right: Box::new(right),
        }
    }

    /// Create a function call
    pub fn call(name: impl Into<String>, args: Vec<Expression>) -> Self {
        Self::FunctionCall {
            name: name.into(),
            args,
        }
    }

    /// Create a context reference
    pub fn context(name: impl Into<String>) -> Self {
        Self::Context(name.into())
    }

    /// Create a raw (unparsed) expression
    pub fn raw(value: impl Into<String>) -> Self {
        Self::Raw(value.into())
    }
}

/// Literal values
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
}

/// Field reference
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldRef {
    /// Simple field name
    Simple(String),
    /// Nested field path (e.g., `profile.address.city`)
    Path(Vec<String>),
    /// Cross-reference (e.g., `User.email`)
    CrossRef { model: String, field: String },
}

impl FieldRef {
    pub fn simple(name: impl Into<String>) -> Self {
        Self::Simple(name.into())
    }

    pub fn path(parts: Vec<String>) -> Self {
        Self::Path(parts)
    }

    pub fn cross_ref(model: impl Into<String>, field: impl Into<String>) -> Self {
        Self::CrossRef {
            model: model.into(),
            field: field.into(),
        }
    }

    /// Get the full path as a string
    pub fn to_path_string(&self) -> String {
        match self {
            Self::Simple(name) => name.clone(),
            Self::Path(parts) => parts.join("."),
            Self::CrossRef { model, field } => format!("{}.{}", model, field),
        }
    }
}

/// Binary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOp {
    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,

    // Logical
    And,
    Or,

    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,

    // String
    Contains,
    StartsWith,
    EndsWith,
    Matches,

    // Collection
    In,
    NotIn,
}

impl BinaryOp {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "==" | "=" | "eq" => Some(Self::Eq),
            "!=" | "<>" | "ne" => Some(Self::Ne),
            "<" | "lt" => Some(Self::Lt),
            "<=" | "le" => Some(Self::Le),
            ">" | "gt" => Some(Self::Gt),
            ">=" | "ge" => Some(Self::Ge),
            "&&" | "and" => Some(Self::And),
            "||" | "or" => Some(Self::Or),
            "+" => Some(Self::Add),
            "-" => Some(Self::Sub),
            "*" => Some(Self::Mul),
            "/" => Some(Self::Div),
            "%" | "mod" => Some(Self::Mod),
            "contains" => Some(Self::Contains),
            "starts_with" | "startsWith" => Some(Self::StartsWith),
            "ends_with" | "endsWith" => Some(Self::EndsWith),
            "matches" | "~" => Some(Self::Matches),
            "in" => Some(Self::In),
            "not_in" | "notIn" => Some(Self::NotIn),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::And => "&&",
            Self::Or => "||",
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Mod => "%",
            Self::Contains => "contains",
            Self::StartsWith => "starts_with",
            Self::EndsWith => "ends_with",
            Self::Matches => "matches",
            Self::In => "in",
            Self::NotIn => "not_in",
        }
    }

    pub fn precedence(&self) -> u8 {
        match self {
            Self::Or => 1,
            Self::And => 2,
            Self::Eq | Self::Ne => 3,
            Self::Lt | Self::Le | Self::Gt | Self::Ge => 4,
            Self::In | Self::NotIn => 4,
            Self::Contains | Self::StartsWith | Self::EndsWith | Self::Matches => 5,
            Self::Add | Self::Sub => 6,
            Self::Mul | Self::Div | Self::Mod => 7,
        }
    }
}

/// Unary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    /// Logical not
    Not,
    /// Numeric negation
    Neg,
}

impl UnaryOp {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "!" | "not" => Some(Self::Not),
            "-" => Some(Self::Neg),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Not => "!",
            Self::Neg => "-",
        }
    }
}

/// Aggregate functions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateFunc {
    /// Count elements
    Count,
    /// Sum of elements
    Sum,
    /// Average of elements
    Avg,
    /// Minimum value
    Min,
    /// Maximum value
    Max,
    /// Any element matches predicate
    Any,
    /// All elements match predicate
    All,
    /// No element matches predicate
    None,
    /// First element
    First,
    /// Last element
    Last,
}

impl AggregateFunc {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "count" => Some(Self::Count),
            "sum" => Some(Self::Sum),
            "avg" | "average" => Some(Self::Avg),
            "min" | "minimum" => Some(Self::Min),
            "max" | "maximum" => Some(Self::Max),
            "any" | "some" => Some(Self::Any),
            "all" | "every" => Some(Self::All),
            "none" => Some(Self::None),
            "first" => Some(Self::First),
            "last" => Some(Self::Last),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Sum => "sum",
            Self::Avg => "avg",
            Self::Min => "min",
            Self::Max => "max",
            Self::Any => "any",
            Self::All => "all",
            Self::None => "none",
            Self::First => "first",
            Self::Last => "last",
        }
    }
}

/// Built-in context variables
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinContext {
    /// Current user
    User,
    /// Current timestamp
    Now,
    /// Old value (for updates)
    Old,
    /// New value (for updates)
    New,
    /// Current entity
    This,
}

impl BuiltinContext {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "user" | "$user" => Some(Self::User),
            "now" | "$now" => Some(Self::Now),
            "old" | "$old" => Some(Self::Old),
            "new" | "$new" => Some(Self::New),
            "this" | "$this" | "self" => Some(Self::This),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "$user",
            Self::Now => "$now",
            Self::Old => "$old",
            Self::New => "$new",
            Self::This => "$this",
        }
    }
}
