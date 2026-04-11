//! Schema parser module
//!
//! This module contains the lexer and parsers for schema files.
//!
//! # Parsing Flow
//!
//! ## Custom DSL (Legacy)
//! ```text
//! Schema Text → Lexer → Tokens → Parser → AST
//! ```
//!
//! ## YAML Format (Recommended)
//! ```text
//! YAML Text → serde_yaml → AST
//! ```
//!
//! ## Parsers
//!
//! - `ModelParser` - Parses `*.model.schema` files (legacy DSL)
//! - `HookParser` - Parses `*.hook.schema` files (legacy DSL)
//! - `yaml_parser` - Parses `*.model.yaml`, `*.hook.yaml`, and `*.workflow.yaml` files (recommended)

pub mod lexer;
pub mod model_parser;
pub mod workflow_parser;
pub mod yaml_parser;

pub use lexer::{Lexer, Token, TokenKind};
pub use model_parser::ModelParser;
pub use workflow_parser::WorkflowParser;
pub use yaml_parser::{
    parse_model_yaml, parse_model_yaml_str,
    parse_hook_yaml, parse_hook_yaml_str,
    parse_workflow_yaml, parse_workflow_yaml_str,
    parse_hook_yaml_flexible, parse_hook_index_yaml_str,
    parse_model_yaml_flexible, parse_model_index_yaml_str,
    is_hook_index_file, is_model_index_file,
    resolve_shared_types,
    YamlWorkflowSchema, YamlHookParseResult, YamlHookIndexSchema,
    YamlModelIndexSchema, YamlModelParseResult, YamlSharedType, YamlField,
};

use crate::ast::{ModelFile, HookFile, WorkflowFile};
use std::path::Path;
use thiserror::Error;

/// Parser error type
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Lexer error at line {line}, column {col}: {message}")]
    LexerError {
        line: usize,
        col: usize,
        message: String,
    },

    #[error("Syntax error at line {line}, column {col}: {message}")]
    SyntaxError {
        line: usize,
        col: usize,
        message: String,
    },

    #[error("Unexpected token at line {line}, column {col}: expected {expected}, got {got}")]
    UnexpectedToken {
        line: usize,
        col: usize,
        expected: String,
        got: String,
    },

    #[error("Unexpected end of file: {message}")]
    UnexpectedEof { message: String },

    #[error("Invalid type: {0}")]
    InvalidType(String),

    #[error("Invalid attribute: {0}")]
    InvalidAttribute(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

impl ParseError {
    pub fn syntax(line: usize, col: usize, message: impl Into<String>) -> Self {
        Self::SyntaxError {
            line,
            col,
            message: message.into(),
        }
    }

    pub fn unexpected_token(
        line: usize,
        col: usize,
        expected: impl Into<String>,
        got: impl Into<String>,
    ) -> Self {
        Self::UnexpectedToken {
            line,
            col,
            expected: expected.into(),
            got: got.into(),
        }
    }

    pub fn unexpected_eof(message: impl Into<String>) -> Self {
        Self::UnexpectedEof {
            message: message.into(),
        }
    }

    /// Get the line and column of this error (if available)
    pub fn location(&self) -> Option<(usize, usize)> {
        match self {
            Self::LexerError { line, col, .. } => Some((*line, *col)),
            Self::SyntaxError { line, col, .. } => Some((*line, *col)),
            Self::UnexpectedToken { line, col, .. } => Some((*line, *col)),
            _ => None,
        }
    }

    /// Format this error with source context showing the problematic line
    pub fn format_with_source(&self, source: &str, filename: Option<&str>) -> String {
        use std::fmt::Write;
        let mut output = String::new();

        // Header with error type
        if let Some(name) = filename {
            writeln!(output, "error in {}:", name).ok();
        } else {
            writeln!(output, "error:").ok();
        }

        writeln!(output, "  {}", self).ok();

        // Show source context if we have location info
        if let Some((line, col)) = self.location() {
            let lines: Vec<&str> = source.lines().collect();
            if line > 0 && line <= lines.len() {
                let line_content = lines[line - 1];
                let line_num_width = line.to_string().len().max(3);

                writeln!(output).ok();

                // Show line before if available
                if line > 1 {
                    writeln!(
                        output,
                        "  {:>width$} │ {}",
                        line - 1,
                        lines[line - 2],
                        width = line_num_width
                    ).ok();
                }

                // Show error line
                writeln!(
                    output,
                    "  {:>width$} │ {}",
                    line,
                    line_content,
                    width = line_num_width
                ).ok();

                // Show pointer to error position
                let pointer_padding = " ".repeat(col.saturating_sub(1));
                writeln!(
                    output,
                    "  {:>width$} │ {}^",
                    "",
                    pointer_padding,
                    width = line_num_width
                ).ok();

                // Show line after if available
                if line < lines.len() {
                    writeln!(
                        output,
                        "  {:>width$} │ {}",
                        line + 1,
                        lines[line],
                        width = line_num_width
                    ).ok();
                }
            }
        }

        output
    }
}

/// Parse a model schema file
pub fn parse_model(source: &str) -> Result<ModelFile, ParseError> {
    let lexer = Lexer::new(source);
    let mut parser = ModelParser::new(lexer);
    parser.parse()
}

/// Parse a model schema file from path
pub fn parse_model_file(path: impl AsRef<Path>) -> Result<ModelFile, ParseError> {
    let source = std::fs::read_to_string(path.as_ref())?;
    let mut file = parse_model(&source)?;
    file.path = Some(path.as_ref().display().to_string());
    Ok(file)
}

/// Parse a hook schema file (legacy DSL)
pub fn parse_hook(source: &str) -> Result<HookFile, ParseError> {
    let lexer = Lexer::new(source);
    let mut parser = WorkflowParser::new(lexer);
    parser.parse()
}

/// Parse a hook schema file from path (legacy DSL)
pub fn parse_hook_file(path: impl AsRef<Path>) -> Result<HookFile, ParseError> {
    let source = std::fs::read_to_string(path.as_ref())?;
    let mut file = parse_hook(&source)?;
    file.path = Some(path.as_ref().display().to_string());
    Ok(file)
}

/// Parse a model YAML file from string content
pub fn parse_yaml_model(source: &str) -> Result<ModelFile, ParseError> {
    let yaml_schema = parse_model_yaml_str(source)
        .map_err(|e| ParseError::SyntaxError {
            line: 1,
            col: 1,
            message: e.to_string(),
        })?;

    // Convert YAML schema to ModelFile
    let models = yaml_schema.models.into_iter().map(|m| m.into_model()).collect();
    let enums = yaml_schema.enums.into_iter().map(|e| e.into_enum()).collect();

    Ok(ModelFile {
        path: None,
        type_defs: Vec::new(),
        enums,
        models,
    })
}

/// Parse a hook YAML file from string content (entity lifecycle behaviors)
pub fn parse_yaml_hook(source: &str) -> Result<HookFile, ParseError> {
    let yaml_schema = parse_hook_yaml_str(source)
        .map_err(|e| ParseError::SyntaxError {
            line: 1,
            col: 1,
            message: e.to_string(),
        })?;

    // Convert YAML schema to HookFile
    Ok(HookFile {
        path: None,
        hooks: vec![yaml_schema.into_hook()],
    })
}

/// Result of flexible hook parsing
pub enum HookParseResult {
    /// Standard hook file
    Hook(HookFile),
    /// Index/module configuration file (skipped for code generation)
    Index(Box<YamlHookIndexSchema>),
}

/// Result of flexible model parsing
pub enum ModelParseResult {
    /// Standard model file
    Model(ModelFile),
    /// Index/module configuration file with shared_types
    Index(YamlModelIndexSchema),
}

/// Parse a hook YAML file that might be either standard hook or index file
/// Returns None for index files (which should be skipped in code generation)
pub fn parse_yaml_hook_flexible(source: &str) -> Result<HookParseResult, ParseError> {
    use yaml_parser::YamlHookParseResult;

    let result = parse_hook_yaml_flexible(source)
        .map_err(|e| ParseError::SyntaxError {
            line: 1,
            col: 1,
            message: e.to_string(),
        })?;

    match result {
        YamlHookParseResult::Hook(yaml_schema) => {
            Ok(HookParseResult::Hook(HookFile {
                path: None,
                hooks: vec![yaml_schema.into_hook()],
            }))
        }
        YamlHookParseResult::Index(index_schema) => {
            Ok(HookParseResult::Index(Box::new(index_schema)))
        }
    }
}

/// Parse a model YAML file that might be either standard model or index file
pub fn parse_yaml_model_flexible(source: &str) -> Result<ModelParseResult, ParseError> {
    use yaml_parser::YamlModelParseResult;

    let result = parse_model_yaml_flexible(source)
        .map_err(|e| ParseError::SyntaxError {
            line: 1,
            col: 1,
            message: e.to_string(),
        })?;

    match result {
        YamlModelParseResult::Model(yaml_schema) => {
            let yaml_schema = *yaml_schema;
            let models = yaml_schema.models.into_iter().map(|m| m.into_model()).collect();
            let enums = yaml_schema.enums.into_iter().map(|e| e.into_enum()).collect();

            Ok(ModelParseResult::Model(ModelFile {
                path: None,
                type_defs: Vec::new(),
                enums,
                models,
            }))
        }
        YamlModelParseResult::Index(index_schema) => {
            Ok(ModelParseResult::Index(index_schema))
        }
    }
}

/// Parse a workflow YAML file from string content (multi-step business processes)
pub fn parse_yaml_workflow(source: &str) -> Result<WorkflowFile, ParseError> {
    let yaml_schema = parse_workflow_yaml_str(source)
        .map_err(|e| ParseError::SyntaxError {
            line: 1,
            col: 1,
            message: e.to_string(),
        })?;

    // Convert YAML schema to WorkflowFile
    Ok(WorkflowFile {
        path: None,
        workflows: vec![yaml_schema.into_workflow()],
    })
}

/// Parse a workflow YAML file from path (multi-step business processes)
pub fn parse_workflow_file(path: impl AsRef<Path>) -> Result<WorkflowFile, ParseError> {
    let source = std::fs::read_to_string(path.as_ref())?;
    let mut file = parse_yaml_workflow(&source)?;
    file.path = Some(path.as_ref().display().to_string());
    Ok(file)
}

/// Parse a standalone expression string into an Expression AST
///
/// This is useful for parsing computed field expressions or rule conditions
/// that are stored as raw strings in YAML schemas.
///
/// # Examples
/// ```ignore
/// use metaphor_schema::parser::parse_expression_str;
///
/// let expr = parse_expression_str("parent_id == null")?;
/// let expr = parse_expression_str("is_active && is_verified")?;
/// let expr = parse_expression_str("count(items)")?;
/// ```
pub fn parse_expression_str(source: &str) -> Result<crate::ast::expressions::Expression, ParseError> {
    let lexer = Lexer::new(source);
    let mut parser = ExpressionParser::new(lexer);
    parser.parse()
}

/// Lightweight expression parser for standalone expression strings
struct ExpressionParser<'a> {
    lexer: Lexer<'a>,
    current: Option<Token>,
}

impl<'a> ExpressionParser<'a> {
    fn new(mut lexer: Lexer<'a>) -> Self {
        let current = lexer.next_token();
        Self { lexer, current }
    }

    fn parse(&mut self) -> Result<crate::ast::expressions::Expression, ParseError> {
        self.parse_or_expression()
    }

    fn parse_or_expression(&mut self) -> Result<crate::ast::expressions::Expression, ParseError> {
        use crate::ast::expressions::{Expression, BinaryOp};

        let mut left = self.parse_and_expression()?;

        while self.check(TokenKind::Or) {
            self.advance()?;
            let right = self.parse_and_expression()?;
            left = Expression::binary(left, BinaryOp::Or, right);
        }

        Ok(left)
    }

    fn parse_and_expression(&mut self) -> Result<crate::ast::expressions::Expression, ParseError> {
        use crate::ast::expressions::{Expression, BinaryOp};

        let mut left = self.parse_comparison_expression()?;

        while self.check(TokenKind::And) {
            self.advance()?;
            let right = self.parse_comparison_expression()?;
            left = Expression::binary(left, BinaryOp::And, right);
        }

        Ok(left)
    }

    fn parse_comparison_expression(&mut self) -> Result<crate::ast::expressions::Expression, ParseError> {
        use crate::ast::expressions::{Expression, BinaryOp};

        let left = self.parse_additive_expression()?;

        let op = match self.current_kind() {
            Some(TokenKind::EqEq) => Some(BinaryOp::Eq),
            Some(TokenKind::Eq) => Some(BinaryOp::Eq),
            Some(TokenKind::Ne) => Some(BinaryOp::Ne),
            Some(TokenKind::Lt) => Some(BinaryOp::Lt),
            Some(TokenKind::Le) => Some(BinaryOp::Le),
            Some(TokenKind::Gt) => Some(BinaryOp::Gt),
            Some(TokenKind::Ge) => Some(BinaryOp::Ge),
            _ => None,
        };

        if let Some(op) = op {
            self.advance()?;
            let right = self.parse_additive_expression()?;
            return Ok(Expression::binary(left, op, right));
        }

        Ok(left)
    }

    fn parse_additive_expression(&mut self) -> Result<crate::ast::expressions::Expression, ParseError> {
        use crate::ast::expressions::{Expression, BinaryOp};

        let mut left = self.parse_multiplicative_expression()?;

        loop {
            let op = match self.current_kind() {
                Some(TokenKind::Plus) => Some(BinaryOp::Add),
                Some(TokenKind::Minus) => Some(BinaryOp::Sub),
                _ => None,
            };

            if let Some(op) = op {
                self.advance()?;
                let right = self.parse_multiplicative_expression()?;
                left = Expression::binary(left, op, right);
            } else {
                break;
            }
        }

        Ok(left)
    }

    fn parse_multiplicative_expression(&mut self) -> Result<crate::ast::expressions::Expression, ParseError> {
        use crate::ast::expressions::{Expression, BinaryOp};

        let mut left = self.parse_unary_expression()?;

        loop {
            let op = match self.current_kind() {
                Some(TokenKind::Star) => Some(BinaryOp::Mul),
                Some(TokenKind::Slash) => Some(BinaryOp::Div),
                Some(TokenKind::Percent) => Some(BinaryOp::Mod),
                _ => None,
            };

            if let Some(op) = op {
                self.advance()?;
                let right = self.parse_unary_expression()?;
                left = Expression::binary(left, op, right);
            } else {
                break;
            }
        }

        Ok(left)
    }

    fn parse_unary_expression(&mut self) -> Result<crate::ast::expressions::Expression, ParseError> {
        use crate::ast::expressions::{Expression, UnaryOp};

        if self.check(TokenKind::Bang) {
            self.advance()?;
            let expr = self.parse_unary_expression()?;
            return Ok(Expression::Unary {
                op: UnaryOp::Not,
                expr: Box::new(expr),
            });
        }

        if self.check(TokenKind::Minus) {
            self.advance()?;
            let expr = self.parse_unary_expression()?;
            return Ok(Expression::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(expr),
            });
        }

        self.parse_primary_expression()
    }

    fn parse_primary_expression(&mut self) -> Result<crate::ast::expressions::Expression, ParseError> {
        use crate::ast::expressions::{Expression, Literal, FieldRef};

        match self.current_kind().cloned() {
            Some(TokenKind::LParen) => {
                self.advance()?;
                let expr = self.parse_or_expression()?;
                self.expect(TokenKind::RParen)?;

                // Check for ternary operator after parenthesized expression
                if self.check(TokenKind::Question) {
                    return self.parse_ternary(expr);
                }

                // Check for member access after parenthesized expression: (expr).member
                if self.check(TokenKind::Dot) {
                    return self.parse_member_access(expr);
                }

                Ok(expr)
            }
            Some(TokenKind::String(s)) | Some(TokenKind::SingleQuoteString(s)) => {
                let value = s.clone();
                self.advance()?;
                Ok(Expression::Literal(Literal::String(value)))
            }
            Some(TokenKind::Integer(n)) => {
                let value = n;
                self.advance()?;
                // Check for member access on integer literals (e.g., 24.hours)
                let expr = Expression::Literal(Literal::Int(value));
                if self.check(TokenKind::Dot) {
                    return self.parse_member_access(expr);
                }
                Ok(expr)
            }
            Some(TokenKind::Float(n)) => {
                let value = n;
                self.advance()?;
                // Check for member access on float literals (e.g., 0.5.value)
                let expr = Expression::Literal(Literal::Float(value));
                if self.check(TokenKind::Dot) {
                    return self.parse_member_access(expr);
                }
                Ok(expr)
            }
            Some(TokenKind::True) => {
                self.advance()?;
                Ok(Expression::Literal(Literal::Bool(true)))
            }
            Some(TokenKind::False) => {
                self.advance()?;
                Ok(Expression::Literal(Literal::Bool(false)))
            }
            Some(TokenKind::Null) => {
                self.advance()?;
                Ok(Expression::Literal(Literal::Null))
            }
            Some(TokenKind::ContextVar(s)) => {
                let name = s.clone();
                self.advance()?;
                Ok(Expression::Context(name))
            }
            Some(TokenKind::Ident(s)) => {
                let name = s.clone();
                self.advance()?;

                // Check for function call
                if self.check(TokenKind::LParen) {
                    return self.parse_function_call(name);
                }

                // Check for member access
                if self.check(TokenKind::Dot) {
                    return self.parse_member_access(Expression::FieldRef(FieldRef::Simple(name)));
                }

                // Check for ternary operator
                let expr = Expression::FieldRef(FieldRef::Simple(name));
                if self.check(TokenKind::Question) {
                    return self.parse_ternary(expr);
                }

                Ok(expr)
            }
            Some(kind) => Err(ParseError::syntax(
                self.current_line(),
                self.current_col(),
                format!("Unexpected token in expression: '{}'", kind),
            )),
            None => {
                // End of input - return null as default
                Ok(Expression::Literal(Literal::Null))
            }
        }
    }

    fn parse_ternary(&mut self, condition: crate::ast::expressions::Expression) -> Result<crate::ast::expressions::Expression, ParseError> {
        use crate::ast::expressions::Expression;

        self.expect(TokenKind::Question)?;
        let then_expr = self.parse_or_expression()?;
        self.expect(TokenKind::Colon)?;
        let else_expr = self.parse_or_expression()?;

        Ok(Expression::Conditional {
            condition: Box::new(condition),
            then_expr: Box::new(then_expr),
            else_expr: Box::new(else_expr),
        })
    }

    fn parse_function_call(&mut self, name: String) -> Result<crate::ast::expressions::Expression, ParseError> {
        use crate::ast::expressions::Expression;

        self.expect(TokenKind::LParen)?;
        let mut args = Vec::new();

        while !self.check(TokenKind::RParen) && self.current.is_some() {
            args.push(self.parse_or_expression()?);
            if self.check(TokenKind::Comma) {
                self.advance()?;
            } else {
                break;
            }
        }
        self.expect(TokenKind::RParen)?;

        Ok(Expression::FunctionCall { name, args })
    }

    fn parse_member_access(&mut self, object: crate::ast::expressions::Expression) -> Result<crate::ast::expressions::Expression, ParseError> {
        use crate::ast::expressions::Expression;

        self.expect(TokenKind::Dot)?;
        let member = self.expect_ident()?;

        let expr = Expression::MemberAccess {
            object: Box::new(object),
            member: member.clone(),
        };

        // Check for chained member access
        if self.check(TokenKind::Dot) {
            return self.parse_member_access(expr);
        }

        // Check for method call
        if self.check(TokenKind::LParen) {
            self.advance()?;
            let mut args = Vec::new();
            while !self.check(TokenKind::RParen) && self.current.is_some() {
                args.push(self.parse_or_expression()?);
                if self.check(TokenKind::Comma) {
                    self.advance()?;
                } else {
                    break;
                }
            }
            self.expect(TokenKind::RParen)?;

            if let Expression::MemberAccess { object, member } = expr {
                return Ok(Expression::MethodCall {
                    object,
                    method: member,
                    args,
                });
            }
        }

        Ok(expr)
    }

    // Helper methods

    fn advance(&mut self) -> Result<(), ParseError> {
        self.current = self.lexer.next_token();
        Ok(())
    }

    fn current_kind(&self) -> Option<&TokenKind> {
        self.current.as_ref().map(|t| &t.kind)
    }

    fn current_line(&self) -> usize {
        self.current.as_ref().map(|t| t.line).unwrap_or(1)
    }

    fn current_col(&self) -> usize {
        self.current.as_ref().map(|t| t.col).unwrap_or(1)
    }

    fn check(&self, kind: TokenKind) -> bool {
        match &self.current {
            Some(token) => std::mem::discriminant(&token.kind) == std::mem::discriminant(&kind),
            None => false,
        }
    }

    fn expect(&mut self, kind: TokenKind) -> Result<(), ParseError> {
        if self.check(kind.clone()) {
            self.advance()?;
            Ok(())
        } else {
            let got = self.current.as_ref()
                .map(|t| format!("{}", t.kind))
                .unwrap_or_else(|| "EOF".to_string());
            Err(ParseError::unexpected_token(
                self.current_line(),
                self.current_col(),
                format!("{:?}", kind),
                got,
            ))
        }
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        match self.current_kind() {
            Some(TokenKind::Ident(s)) => {
                let name = s.clone();
                self.advance()?;
                Ok(name)
            }
            Some(kind) => Err(ParseError::syntax(
                self.current_line(),
                self.current_col(),
                format!("Expected identifier, got '{}'", kind),
            )),
            None => Err(ParseError::unexpected_eof("Expected identifier")),
        }
    }
}
