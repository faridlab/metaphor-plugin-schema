//! Hook schema parser (legacy DSL)
//!
//! Parses `*.hook.schema` files into AST (formerly `*.workflow.schema`).

use super::lexer::{Lexer, Token, TokenKind};
use super::ParseError;
use crate::ast::expressions::{BinaryOp, Expression, FieldRef, Literal};
use crate::ast::hook::{
    Action, ActionKind, ActionType, ComputedField, FieldRestriction, Permission, PermissionAction,
    Rule, RuleWhen, State, StateMachine, Transition, Trigger, TriggerEvent, Hook,
};
use crate::ast::{Span, HookFile};

/// Parser for hook schema files (legacy DSL)
pub struct WorkflowParser<'source> {
    lexer: Lexer<'source>,
    current: Option<Token>,
}

impl<'source> WorkflowParser<'source> {
    pub fn new(lexer: Lexer<'source>) -> Self {
        Self {
            lexer,
            current: None,
        }
    }

    /// Parse the entire file
    pub fn parse(&mut self) -> Result<HookFile, ParseError> {
        self.advance()?;

        let mut file = HookFile::default();

        while self.current.is_some() {
            if !self.check(TokenKind::Workflow) {
                return Err(ParseError::syntax(
                    self.current_line(),
                    self.current_col(),
                    format!(
                        "Expected 'workflow', got '{}'",
                        self.current_kind()
                            .map(|k| format!("{}", k))
                            .unwrap_or_else(|| "EOF".to_string())
                    ),
                ));
            }
            let hook = self.parse_hook()?;
            file.hooks.push(hook);
        }

        Ok(file)
    }

    /// Helper: Parse a block of items enclosed in braces
    fn parse_block<T, F>(&mut self, mut parse_item: F) -> Result<Vec<T>, ParseError>
    where
        F: FnMut(&mut Self) -> Result<T, ParseError>,
    {
        self.expect(TokenKind::LBrace)?;
        let mut items = Vec::new();
        while !self.check(TokenKind::RBrace) {
            items.push(parse_item(self)?);
        }
        self.expect(TokenKind::RBrace)?;
        Ok(items)
    }

    /// Helper: Parse actions in on_enter/on_exit blocks
    fn parse_action_block(&mut self) -> Result<Vec<Action>, ParseError> {
        self.advance()?;
        self.parse_block(|p| p.parse_action())
    }

    /// Parse a hook definition (legacy 'workflow' keyword)
    fn parse_hook(&mut self) -> Result<Hook, ParseError> {
        let start_line = self.current_line();
        let start_col = self.current_col();

        self.expect(TokenKind::Workflow)?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::From)?;

        // Model reference can be a string or identifier
        let model_ref = self.parse_string_or_ident()?;

        self.expect(TokenKind::LBrace)?;

        let mut hook = Hook::new(name, model_ref);
        hook.span = Span::new(start_line, start_col, 0, 0);

        while !self.check(TokenKind::RBrace) {
            self.parse_hook_section(&mut hook)?;
        }

        hook.span.end_line = self.current_line();
        hook.span.end_col = self.current_col();
        self.expect(TokenKind::RBrace)?;

        Ok(hook)
    }

    /// Parse a single section within a hook (states, rules, permissions, etc.)
    fn parse_hook_section(&mut self, hook: &mut Hook) -> Result<(), ParseError> {
        let kind = self.current_kind().cloned();

        match kind {
            Some(TokenKind::States) => {
                hook.state_machine = Some(self.parse_state_machine()?);
            }
            Some(TokenKind::Rules) => {
                self.advance()?;
                hook.rules = self.parse_block(|p| p.parse_rule())?;
            }
            Some(TokenKind::Permissions) => {
                self.advance()?;
                hook.permissions = self.parse_block(|p| p.parse_permission())?;
            }
            Some(TokenKind::Triggers) => {
                self.advance()?;
                hook.triggers = self.parse_block(|p| p.parse_trigger())?;
            }
            Some(TokenKind::Computed) => {
                self.advance()?;
                hook.computed_fields = self.parse_block(|p| p.parse_computed_field())?;
            }
            Some(kind) => {
                return Err(ParseError::syntax(
                    self.current_line(),
                    self.current_col(),
                    format!("Unexpected token in hook: '{}'", kind),
                ));
            }
            None => {
                return Err(ParseError::unexpected_eof("in hook definition"));
            }
        }
        Ok(())
    }

    /// Parse state machine definition
    fn parse_state_machine(&mut self) -> Result<StateMachine, ParseError> {
        let start_line = self.current_line();
        let start_col = self.current_col();

        self.expect(TokenKind::States)?;

        let field = self.parse_optional_field_spec()?;

        self.expect(TokenKind::LBrace)?;

        let mut state_machine = StateMachine {
            field,
            states: Vec::new(),
            transitions: Vec::new(),
            span: Span::new(start_line, start_col, 0, 0),
        };

        while !self.check(TokenKind::RBrace) {
            self.parse_state_machine_entry(&mut state_machine)?;
        }

        state_machine.span.end_line = self.current_line();
        state_machine.span.end_col = self.current_col();
        self.expect(TokenKind::RBrace)?;

        Ok(state_machine)
    }

    /// Parse optional field specification like (field_name)
    fn parse_optional_field_spec(&mut self) -> Result<String, ParseError> {
        if !self.check(TokenKind::LParen) {
            return Ok("status".to_string());
        }
        self.advance()?;
        let field_name = self.expect_ident()?;
        self.expect(TokenKind::RParen)?;
        Ok(field_name)
    }

    /// Parse a single entry in state machine (transitions block or state)
    fn parse_state_machine_entry(&mut self, state_machine: &mut StateMachine) -> Result<(), ParseError> {
        match self.current_kind().cloned() {
            Some(TokenKind::Transitions) => {
                self.advance()?;
                state_machine.transitions = self.parse_block(|p| p.parse_transition())?;
            }
            Some(TokenKind::Ident(_)) => {
                state_machine.states.push(self.parse_state()?);
            }
            Some(kind) => {
                return Err(ParseError::syntax(
                    self.current_line(),
                    self.current_col(),
                    format!("Unexpected token in states block: '{}'", kind),
                ));
            }
            None => {
                return Err(ParseError::unexpected_eof("in states block"));
            }
        }
        Ok(())
    }

    /// Parse a state definition
    fn parse_state(&mut self) -> Result<State, ParseError> {
        let start_line = self.current_line();
        let start_col = self.current_col();

        let name = self.expect_ident()?;

        let mut state = State::new(name);
        state.span = Span::new(start_line, start_col, 0, 0);

        // Check for inline attributes (@initial, @final)
        self.parse_state_attributes(&mut state)?;

        // Check for block with hooks
        if self.check(TokenKind::LBrace) {
            self.advance()?;
            while !self.check(TokenKind::RBrace) {
                self.parse_state_body_entry(&mut state)?;
            }
            self.expect(TokenKind::RBrace)?;
        }

        state.span.end_line = self.current_line();
        state.span.end_col = self.current_col();

        Ok(state)
    }

    /// Parse inline state attributes like @initial, @final
    fn parse_state_attributes(&mut self, state: &mut State) -> Result<(), ParseError> {
        while self.check(TokenKind::At) {
            self.advance()?;
            self.apply_state_attribute(state)?;
        }
        Ok(())
    }

    /// Apply a single state attribute
    fn apply_state_attribute(&mut self, state: &mut State) -> Result<(), ParseError> {
        match self.current_kind().cloned() {
            Some(TokenKind::Initial) => {
                state.initial = true;
                self.advance()?;
            }
            Some(TokenKind::Final) => {
                state.final_state = true;
                self.advance()?;
            }
            Some(TokenKind::Ident(attr_name)) => {
                self.advance()?;
                match attr_name.as_str() {
                    "initial" => state.initial = true,
                    "final" => state.final_state = true,
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Parse a single entry in state body (on_enter, on_exit, initial, final)
    fn parse_state_body_entry(&mut self, state: &mut State) -> Result<(), ParseError> {
        match self.current_kind().cloned() {
            Some(TokenKind::OnEnter) => {
                state.on_enter = self.parse_action_block()?;
            }
            Some(TokenKind::OnExit) => {
                state.on_exit = self.parse_action_block()?;
            }
            Some(TokenKind::Initial) => {
                self.advance()?;
                state.initial = true;
            }
            Some(TokenKind::Final) => {
                self.advance()?;
                state.final_state = true;
            }
            Some(kind) => {
                return Err(ParseError::syntax(
                    self.current_line(),
                    self.current_col(),
                    format!("Unexpected token in state: '{}'", kind),
                ));
            }
            None => {
                return Err(ParseError::unexpected_eof("in state definition"));
            }
        }
        Ok(())
    }

    /// Parse a transition definition
    fn parse_transition(&mut self) -> Result<Transition, ParseError> {
        let start_line = self.current_line();
        let start_col = self.current_col();

        let name = self.expect_ident()?;
        self.expect(TokenKind::Colon)?;

        // Parse source states
        let from = self.parse_state_list()?;

        self.expect(TokenKind::Arrow)?;

        // Parse target state
        let to = self.expect_ident()?;

        let mut transition = Transition::new(name, from, to);
        transition.span = Span::new(start_line, start_col, 0, 0);

        // Parse role restriction
        while self.check(TokenKind::At) {
            self.advance()?;
            let attr = self.expect_ident()?;
            if attr == "role" {
                self.expect(TokenKind::LParen)?;
                while !self.check(TokenKind::RParen) {
                    let role = self.expect_ident()?;
                    transition.allowed_roles.push(role);
                    if self.check(TokenKind::Comma) {
                        self.advance()?;
                    }
                }
                self.expect(TokenKind::RParen)?;
            }
        }

        transition.span.end_line = self.current_line();
        transition.span.end_col = self.current_col();

        Ok(transition)
    }

    /// Parse a list of states (for transition sources)
    fn parse_state_list(&mut self) -> Result<Vec<String>, ParseError> {
        let mut states = Vec::new();

        // Check for star (any state)
        if self.check(TokenKind::Star) {
            self.advance()?;
            return Ok(vec!["*".to_string()]);
        }

        // Check for array syntax
        if self.check(TokenKind::LBracket) {
            self.advance()?;
            while !self.check(TokenKind::RBracket) {
                states.push(self.expect_ident()?);
                if self.check(TokenKind::Comma) {
                    self.advance()?;
                }
            }
            self.expect(TokenKind::RBracket)?;
        } else {
            // Single state
            states.push(self.expect_ident()?);
        }

        Ok(states)
    }

    /// Parse a rule definition
    fn parse_rule(&mut self) -> Result<Rule, ParseError> {
        let start_line = self.current_line();
        let start_col = self.current_col();

        let name = self.expect_ident()?;
        self.expect(TokenKind::LBrace)?;

        let mut rule = Rule {
            name,
            when: Vec::new(),
            condition: Expression::default(),
            message: String::new(),
            code: None,
            span: Span::new(start_line, start_col, 0, 0),
        };

        while !self.check(TokenKind::RBrace) {
            match self.current_kind() {
                Some(TokenKind::When) => {
                    self.advance()?;
                    self.expect(TokenKind::Colon)?;
                    rule.when = self.parse_rule_when()?;
                }
                Some(TokenKind::Condition) => {
                    self.advance()?;
                    self.expect(TokenKind::Colon)?;
                    rule.condition = self.parse_expression()?;
                }
                Some(TokenKind::Message) => {
                    self.advance()?;
                    self.expect(TokenKind::Colon)?;
                    rule.message = self.parse_string_or_ident()?;
                }
                Some(TokenKind::Ident(s)) if s == "code" => {
                    self.advance()?;
                    self.expect(TokenKind::Colon)?;
                    rule.code = Some(self.parse_string_or_ident()?);
                }
                Some(kind) => {
                    return Err(ParseError::syntax(
                        self.current_line(),
                        self.current_col(),
                        format!("Unexpected token in rule: '{}'", kind),
                    ));
                }
                None => {
                    return Err(ParseError::unexpected_eof("in rule definition"));
                }
            }
        }

        rule.span.end_line = self.current_line();
        rule.span.end_col = self.current_col();
        self.expect(TokenKind::RBrace)?;

        Ok(rule)
    }

    /// Parse rule when clause
    fn parse_rule_when(&mut self) -> Result<Vec<RuleWhen>, ParseError> {
        let mut whens = Vec::new();

        if self.check(TokenKind::LBracket) {
            self.advance()?;
            while !self.check(TokenKind::RBracket) {
                whens.push(self.parse_single_when()?);
                if self.check(TokenKind::Comma) {
                    self.advance()?;
                }
            }
            self.expect(TokenKind::RBracket)?;
        } else {
            whens.push(self.parse_single_when()?);
        }

        Ok(whens)
    }

    fn parse_single_when(&mut self) -> Result<RuleWhen, ParseError> {
        let value = self.expect_ident()?;
        match value.to_lowercase().as_str() {
            "create" => Ok(RuleWhen::Create),
            "update" => Ok(RuleWhen::Update),
            "delete" => Ok(RuleWhen::Delete),
            "always" => Ok(RuleWhen::Always),
            _ => Ok(RuleWhen::Transition(value)),
        }
    }

    /// Parse a permission definition
    fn parse_permission(&mut self) -> Result<Permission, ParseError> {
        let start_line = self.current_line();
        let start_col = self.current_col();

        let role = self.expect_ident()?;
        self.expect(TokenKind::LBrace)?;

        let mut permission = Permission::new(role);
        permission.span = Span::new(start_line, start_col, 0, 0);

        while !self.check(TokenKind::RBrace) {
            let action = self.parse_permission_action()?;
            permission.actions.push(action);
        }

        permission.span.end_line = self.current_line();
        permission.span.end_col = self.current_col();
        self.expect(TokenKind::RBrace)?;

        Ok(permission)
    }

    /// Parse a permission action
    fn parse_permission_action(&mut self) -> Result<PermissionAction, ParseError> {
        let start_line = self.current_line();
        let start_col = self.current_col();

        let is_allow = match self.current_kind() {
            Some(TokenKind::Allow) => {
                self.advance()?;
                true
            }
            Some(TokenKind::Deny) => {
                self.advance()?;
                false
            }
            Some(kind) => {
                return Err(ParseError::syntax(
                    self.current_line(),
                    self.current_col(),
                    format!("Expected 'allow' or 'deny', got '{}'", kind),
                ));
            }
            None => {
                return Err(ParseError::unexpected_eof("Expected 'allow' or 'deny'"));
            }
        };

        self.expect(TokenKind::Colon)?;

        // Parse action type
        let action_name = self.expect_ident()?;
        let action = ActionType::from_str(&action_name);

        let mut perm_action = PermissionAction {
            action,
            allowed: is_allow,
            fields: None,
            condition: None,
            span: Span::new(start_line, start_col, 0, 0),
        };

        // Check for field restrictions or conditions
        while self.check(TokenKind::LBrace) || self.check(TokenKind::Only) || self.check(TokenKind::Except) || self.check(TokenKind::If) {
            if self.check(TokenKind::Only) {
                self.advance()?;
                self.expect(TokenKind::Colon)?;
                let fields = self.parse_field_list()?;
                perm_action.fields = Some(FieldRestriction::Only(fields));
            } else if self.check(TokenKind::Except) {
                self.advance()?;
                self.expect(TokenKind::Colon)?;
                let fields = self.parse_field_list()?;
                perm_action.fields = Some(FieldRestriction::Except(fields));
            } else if self.check(TokenKind::If) {
                self.advance()?;
                self.expect(TokenKind::Colon)?;
                perm_action.condition = Some(self.parse_expression()?);
            } else {
                break;
            }
        }

        perm_action.span.end_line = self.current_line();
        perm_action.span.end_col = self.current_col();

        Ok(perm_action)
    }

    /// Parse a field list
    fn parse_field_list(&mut self) -> Result<Vec<String>, ParseError> {
        let mut fields = Vec::new();

        if self.check(TokenKind::LBracket) {
            self.advance()?;
            while !self.check(TokenKind::RBracket) {
                fields.push(self.expect_ident()?);
                if self.check(TokenKind::Comma) {
                    self.advance()?;
                }
            }
            self.expect(TokenKind::RBracket)?;
        } else {
            fields.push(self.expect_ident()?);
        }

        Ok(fields)
    }

    /// Parse a trigger definition
    fn parse_trigger(&mut self) -> Result<Trigger, ParseError> {
        let start_line = self.current_line();
        let start_col = self.current_col();

        let event_name = self.expect_ident()?;
        let event = TriggerEvent::from_str(&event_name).ok_or_else(|| {
            ParseError::syntax(
                self.current_line(),
                self.current_col(),
                format!("Unknown trigger event: {}", event_name),
            )
        })?;

        self.expect(TokenKind::LBrace)?;

        let mut trigger = Trigger {
            event,
            actions: Vec::new(),
            condition: None,
            span: Span::new(start_line, start_col, 0, 0),
        };

        while !self.check(TokenKind::RBrace) {
            if self.check(TokenKind::If) {
                self.advance()?;
                self.expect(TokenKind::Colon)?;
                trigger.condition = Some(self.parse_expression()?);
            } else {
                let action = self.parse_action()?;
                trigger.actions.push(action);
            }
        }

        trigger.span.end_line = self.current_line();
        trigger.span.end_col = self.current_col();
        self.expect(TokenKind::RBrace)?;

        Ok(trigger)
    }

    /// Parse an action
    fn parse_action(&mut self) -> Result<Action, ParseError> {
        let start_line = self.current_line();
        let start_col = self.current_col();

        let action_name = self.expect_ident()?;
        let action_type = ActionKind::from_str(&action_name);

        let mut action = Action {
            action_type,
            args: Vec::new(),
            span: Span::new(start_line, start_col, 0, 0),
        };

        // Parse arguments if present
        if self.check(TokenKind::LParen) {
            self.advance()?;
            while !self.check(TokenKind::RParen) {
                action.args.push(self.parse_expression()?);
                if self.check(TokenKind::Comma) {
                    self.advance()?;
                }
            }
            self.expect(TokenKind::RParen)?;
        }

        action.span.end_line = self.current_line();
        action.span.end_col = self.current_col();

        Ok(action)
    }

    /// Parse a computed field definition
    fn parse_computed_field(&mut self) -> Result<ComputedField, ParseError> {
        let start_line = self.current_line();
        let start_col = self.current_col();

        let name = self.expect_ident()?;
        self.expect(TokenKind::Colon)?;
        let expression = self.parse_expression()?;

        let mut computed = ComputedField::new(name, expression);
        computed.span = Span::new(start_line, start_col, self.current_line(), self.current_col());

        Ok(computed)
    }

    /// Parse an expression
    fn parse_expression(&mut self) -> Result<Expression, ParseError> {
        self.parse_or_expression()
    }

    fn parse_or_expression(&mut self) -> Result<Expression, ParseError> {
        let mut left = self.parse_and_expression()?;

        while self.check(TokenKind::Or) {
            self.advance()?;
            let right = self.parse_and_expression()?;
            left = Expression::binary(left, BinaryOp::Or, right);
        }

        Ok(left)
    }

    fn parse_and_expression(&mut self) -> Result<Expression, ParseError> {
        let mut left = self.parse_comparison_expression()?;

        while self.check(TokenKind::And) {
            self.advance()?;
            let right = self.parse_comparison_expression()?;
            left = Expression::binary(left, BinaryOp::And, right);
        }

        Ok(left)
    }

    fn parse_comparison_expression(&mut self) -> Result<Expression, ParseError> {
        let left = self.parse_primary_expression()?;

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
            let right = self.parse_primary_expression()?;
            return Ok(Expression::binary(left, op, right));
        }

        Ok(left)
    }

    fn parse_primary_expression(&mut self) -> Result<Expression, ParseError> {
        match self.current_kind() {
            Some(TokenKind::LParen) => {
                self.advance()?;
                let expr = self.parse_expression()?;
                self.expect(TokenKind::RParen)?;
                Ok(expr)
            }
            Some(TokenKind::Bang) => {
                self.advance()?;
                let expr = self.parse_primary_expression()?;
                Ok(Expression::Unary {
                    op: crate::ast::expressions::UnaryOp::Not,
                    expr: Box::new(expr),
                })
            }
            Some(TokenKind::String(s)) => {
                let value = s.clone();
                self.advance()?;
                Ok(Expression::Literal(Literal::String(value)))
            }
            Some(TokenKind::SingleQuoteString(s)) => {
                let value = s.clone();
                self.advance()?;
                Ok(Expression::Literal(Literal::String(value)))
            }
            Some(TokenKind::Integer(n)) => {
                let value = *n;
                self.advance()?;
                Ok(Expression::Literal(Literal::Int(value)))
            }
            Some(TokenKind::Float(n)) => {
                let value = *n;
                self.advance()?;
                Ok(Expression::Literal(Literal::Float(value)))
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

                Ok(Expression::FieldRef(FieldRef::Simple(name)))
            }
            Some(kind) => Err(ParseError::syntax(
                self.current_line(),
                self.current_col(),
                format!("Unexpected token in expression: '{}'", kind),
            )),
            None => Err(ParseError::unexpected_eof("in expression")),
        }
    }

    fn parse_function_call(&mut self, name: String) -> Result<Expression, ParseError> {
        self.expect(TokenKind::LParen)?;
        let args = self.parse_comma_separated_exprs(TokenKind::RParen)?;
        self.expect(TokenKind::RParen)?;
        Ok(Expression::FunctionCall { name, args })
    }

    fn parse_member_access(&mut self, object: Expression) -> Result<Expression, ParseError> {
        self.expect(TokenKind::Dot)?;
        let member = self.expect_ident()?;

        let expr = Expression::MemberAccess {
            object: Box::new(object),
            member,
        };

        // Check for chained member access
        if self.check(TokenKind::Dot) {
            return self.parse_member_access(expr);
        }

        // Check for method call - convert obj.method(args) -> MethodCall
        if !self.check(TokenKind::LParen) {
            return Ok(expr);
        }

        self.parse_method_call_from_member_access(expr)
    }

    /// Convert member access to method call when followed by parentheses
    fn parse_method_call_from_member_access(&mut self, expr: Expression) -> Result<Expression, ParseError> {
        let Expression::MemberAccess { object, member } = expr else {
            return Ok(expr);
        };

        self.advance()?;
        let args = self.parse_comma_separated_exprs(TokenKind::RParen)?;
        self.expect(TokenKind::RParen)?;

        Ok(Expression::MethodCall {
            object,
            method: member,
            args,
        })
    }

    /// Parse comma-separated expressions until terminator
    fn parse_comma_separated_exprs(&mut self, terminator: TokenKind) -> Result<Vec<Expression>, ParseError> {
        let mut args = Vec::new();
        while !self.check(terminator.clone()) {
            args.push(self.parse_expression()?);
            if self.check(TokenKind::Comma) {
                self.advance()?;
            }
        }
        Ok(args)
    }

    /// Parse a string or identifier
    fn parse_string_or_ident(&mut self) -> Result<String, ParseError> {
        match self.current_kind() {
            Some(TokenKind::String(s)) | Some(TokenKind::SingleQuoteString(s)) => {
                let value = s.clone();
                self.advance()?;
                Ok(value)
            }
            Some(TokenKind::Ident(s)) => {
                let value = s.clone();
                self.advance()?;
                Ok(value)
            }
            Some(kind) => Err(ParseError::syntax(
                self.current_line(),
                self.current_col(),
                format!("Expected string or identifier, got '{}'", kind),
            )),
            None => Err(ParseError::unexpected_eof("Expected string or identifier")),
        }
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

    fn check(&self, expected: TokenKind) -> bool {
        matches!(self.current_kind(), Some(kind) if std::mem::discriminant(kind) == std::mem::discriminant(&expected))
    }

    fn expect(&mut self, expected: TokenKind) -> Result<(), ParseError> {
        if self.check(expected.clone()) {
            self.advance()?;
            Ok(())
        } else {
            Err(ParseError::unexpected_token(
                self.current_line(),
                self.current_col(),
                format!("{}", expected),
                self.current_kind()
                    .map(|k| format!("{}", k))
                    .unwrap_or_else(|| "EOF".to_string()),
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
            Some(kind) => Err(ParseError::unexpected_token(
                self.current_line(),
                self.current_col(),
                "identifier",
                format!("{}", kind),
            )),
            None => Err(ParseError::unexpected_eof("Expected identifier")),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_parse_simple_workflow() {
        let source = r#"
            workflow UserWorkflow from "User" {
                rules {
                    email_required {
                        when: create
                        condition: email != ""
                        message: "Email is required"
                    }
                }
            }
        "#;

        let result = crate::parser::parse_hook(source);
        assert!(result.is_ok());

        let file = result.unwrap();
        assert_eq!(file.hooks.len(), 1);
        assert_eq!(file.hooks[0].name, "UserWorkflow");
        assert_eq!(file.hooks[0].rules.len(), 1);
    }

    #[test]
    fn test_parse_state_machine() {
        let source = r#"
            workflow OrderWorkflow from "Order" {
                states {
                    pending @initial
                    confirmed
                    shipped
                    delivered @final

                    transitions {
                        confirm: pending -> confirmed @role(admin, manager)
                        ship: confirmed -> shipped
                        deliver: shipped -> delivered
                    }
                }
            }
        "#;

        let result = crate::parser::parse_hook(source);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());

        let file = result.unwrap();
        let hook = &file.hooks[0];
        let sm = hook.state_machine.as_ref().unwrap();

        assert_eq!(sm.states.len(), 4);
        assert!(sm.states[0].initial);
        assert!(sm.states[3].final_state);
        assert_eq!(sm.transitions.len(), 3);
    }
}
