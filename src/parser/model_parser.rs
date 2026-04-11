//! Model schema parser
//!
//! Parses `*.model.schema` files into AST.

use super::lexer::{Lexer, Token, TokenKind};
use super::ParseError;
use crate::ast::{
    Attribute, AttributeValue, EnumDef, EnumVariant, Field, Index, IndexType, Model, ModelFile,
    PrimitiveType, Relation, RelationType, Span, TypeDef, TypeDefField, TypeRef,
};

/// Parser for model schema files
pub struct ModelParser<'source> {
    lexer: Lexer<'source>,
    current: Option<Token>,
}

impl<'source> ModelParser<'source> {
    pub fn new(lexer: Lexer<'source>) -> Self {
        Self {
            lexer,
            current: None,
        }
    }

    /// Parse the entire file
    pub fn parse(&mut self) -> Result<ModelFile, ParseError> {
        self.advance()?;

        let mut file = ModelFile::default();

        while self.current.is_some() {
            match self.current_kind() {
                Some(TokenKind::Model) => {
                    let model = self.parse_model()?;
                    file.models.push(model);
                }
                Some(TokenKind::Enum) => {
                    let enum_def = self.parse_enum()?;
                    file.enums.push(enum_def);
                }
                Some(TokenKind::Type) => {
                    let type_def = self.parse_type_def()?;
                    file.type_defs.push(type_def);
                }
                Some(kind) => {
                    return Err(ParseError::syntax(
                        self.current_line(),
                        self.current_col(),
                        format!("Expected 'model', 'enum', or 'type', got '{}'", kind),
                    ));
                }
                None => break,
            }
        }

        Ok(file)
    }

    /// Parse a model definition
    fn parse_model(&mut self) -> Result<Model, ParseError> {
        let start_line = self.current_line();
        let start_col = self.current_col();

        self.expect(TokenKind::Model)?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::LBrace)?;

        let mut model = Model::new(name);
        model.span = Span::new(start_line, start_col, 0, 0);

        while !self.check(TokenKind::RBrace) {
            match self.current_kind() {
                Some(TokenKind::Collection) => {
                    self.advance()?;
                    self.expect(TokenKind::Colon)?;
                    model.collection = Some(self.expect_ident()?);
                }
                Some(TokenKind::Fields) => {
                    self.advance()?;
                    self.expect(TokenKind::LBrace)?;
                    while !self.check(TokenKind::RBrace) {
                        let field = self.parse_field()?;
                        model.fields.push(field);
                    }
                    self.expect(TokenKind::RBrace)?;
                }
                Some(TokenKind::Relations) => {
                    self.advance()?;
                    self.expect(TokenKind::LBrace)?;
                    while !self.check(TokenKind::RBrace) {
                        let relation = self.parse_relation()?;
                        model.relations.push(relation);
                    }
                    self.expect(TokenKind::RBrace)?;
                }
                Some(TokenKind::Indexes) => {
                    self.advance()?;
                    self.expect(TokenKind::LBrace)?;
                    while !self.check(TokenKind::RBrace) {
                        let index = self.parse_index()?;
                        model.indexes.push(index);
                    }
                    self.expect(TokenKind::RBrace)?;
                }
                Some(TokenKind::At) => {
                    let attr = self.parse_attribute()?;
                    model.attributes.push(attr);
                }
                Some(kind) => {
                    return Err(ParseError::syntax(
                        self.current_line(),
                        self.current_col(),
                        format!(
                            "Unexpected token in model body: '{}'. Expected fields, relations, indexes, or attribute",
                            kind
                        ),
                    ));
                }
                None => {
                    return Err(ParseError::unexpected_eof("in model definition"));
                }
            }
        }

        model.span.end_line = self.current_line();
        model.span.end_col = self.current_col();
        self.expect(TokenKind::RBrace)?;

        Ok(model)
    }

    /// Parse a field definition
    fn parse_field(&mut self) -> Result<Field, ParseError> {
        let start_line = self.current_line();
        let start_col = self.current_col();

        let name = self.expect_ident()?;
        let type_ref = self.parse_type_ref()?;

        let mut field = Field::new(name, type_ref);
        field.span = Span::new(start_line, start_col, self.current_line(), self.current_col());

        // Parse attributes
        while self.check(TokenKind::At) {
            let attr = self.parse_attribute()?;
            field.attributes.push(attr);
        }

        Ok(field)
    }

    /// Parse a type reference
    fn parse_type_ref(&mut self) -> Result<TypeRef, ParseError> {
        let base_type = self.parse_base_type()?;

        // Check for array suffix
        if self.check(TokenKind::LBracket) {
            self.advance()?;
            self.expect(TokenKind::RBracket)?;
            return Ok(TypeRef::Array(Box::new(base_type)));
        }

        // Check for optional suffix
        if self.check(TokenKind::Question) {
            self.advance()?;
            return Ok(TypeRef::Optional(Box::new(base_type)));
        }

        Ok(base_type)
    }

    /// Parse base type (without array/optional modifiers)
    fn parse_base_type(&mut self) -> Result<TypeRef, ParseError> {
        let name = self.expect_ident()?;

        // Check for module reference (module.Type)
        if self.check(TokenKind::Dot) {
            self.advance()?;
            let type_name = self.expect_ident()?;
            return Ok(TypeRef::module_ref(name, type_name));
        }

        // Check for Map type
        if name == "Map" {
            self.expect(TokenKind::Lt)?;
            let key = self.parse_type_ref()?;
            self.expect(TokenKind::Comma)?;
            let value = self.parse_type_ref()?;
            self.expect(TokenKind::Gt)?;
            return Ok(TypeRef::Map {
                key: Box::new(key),
                value: Box::new(value),
            });
        }

        // Check if it's a primitive type
        if let Some(primitive) = PrimitiveType::from_str(&name) {
            return Ok(TypeRef::Primitive(primitive));
        }

        // Otherwise it's a custom type
        Ok(TypeRef::Custom(name))
    }

    /// Parse an attribute
    fn parse_attribute(&mut self) -> Result<Attribute, ParseError> {
        let start_line = self.current_line();
        let start_col = self.current_col();

        self.expect(TokenKind::At)?;
        let name = self.expect_ident()?;

        let mut attr = Attribute::new(name);
        attr.span = Span::new(start_line, start_col, 0, 0);

        // Parse arguments if present
        if self.check(TokenKind::LParen) {
            self.advance()?;
            while !self.check(TokenKind::RParen) {
                let arg = self.parse_attribute_arg()?;
                attr.args.push(arg);

                if self.check(TokenKind::Comma) {
                    self.advance()?;
                } else {
                    break;
                }
            }
            self.expect(TokenKind::RParen)?;
        }

        attr.span.end_line = self.current_line();
        attr.span.end_col = self.current_col();

        Ok(attr)
    }

    /// Parse an attribute argument
    fn parse_attribute_arg(&mut self) -> Result<(Option<String>, AttributeValue), ParseError> {
        // Check for named argument (name = value)
        if let Some(TokenKind::Ident(name)) = self.current_kind() {
            let saved_name = name.clone();
            if self.peek_check(TokenKind::Eq) {
                self.advance()?; // consume ident
                self.advance()?; // consume =
                let value = self.parse_attribute_value()?;
                return Ok((Some(saved_name), value));
            }
        }

        // Positional argument
        let value = self.parse_attribute_value()?;
        Ok((None, value))
    }

    /// Parse an attribute value
    fn parse_attribute_value(&mut self) -> Result<AttributeValue, ParseError> {
        match self.current_kind() {
            Some(TokenKind::String(s)) => {
                let value = s.clone();
                self.advance()?;
                Ok(AttributeValue::String(value))
            }
            Some(TokenKind::SingleQuoteString(s)) => {
                let value = s.clone();
                self.advance()?;
                Ok(AttributeValue::String(value))
            }
            Some(TokenKind::Integer(n)) => {
                let value = *n;
                self.advance()?;
                Ok(AttributeValue::Int(value))
            }
            Some(TokenKind::Float(n)) => {
                let value = *n;
                self.advance()?;
                Ok(AttributeValue::Float(value))
            }
            Some(TokenKind::True) => {
                self.advance()?;
                Ok(AttributeValue::Bool(true))
            }
            Some(TokenKind::False) => {
                self.advance()?;
                Ok(AttributeValue::Bool(false))
            }
            Some(TokenKind::Ident(s)) => {
                let value = s.clone();
                self.advance()?;
                Ok(AttributeValue::Ident(value))
            }
            Some(TokenKind::LBracket) => {
                self.advance()?;
                let mut values = Vec::new();
                while !self.check(TokenKind::RBracket) {
                    values.push(self.parse_attribute_value()?);
                    if self.check(TokenKind::Comma) {
                        self.advance()?;
                    } else {
                        break;
                    }
                }
                self.expect(TokenKind::RBracket)?;
                Ok(AttributeValue::Array(values))
            }
            Some(kind) => Err(ParseError::syntax(
                self.current_line(),
                self.current_col(),
                format!("Expected attribute value, got '{}'", kind),
            )),
            None => Err(ParseError::unexpected_eof("Expected attribute value")),
        }
    }

    /// Parse a relation definition
    fn parse_relation(&mut self) -> Result<Relation, ParseError> {
        let start_line = self.current_line();
        let start_col = self.current_col();

        let name = self.expect_ident()?;
        let target_type = self.parse_type_ref()?;

        let mut relation = Relation {
            name,
            target: target_type.clone(),
            relation_type: infer_relation_type(&target_type),
            attributes: Vec::new(),
            span: Span::new(start_line, start_col, 0, 0),
        };

        // Parse attributes
        while self.check(TokenKind::At) {
            let attr = self.parse_attribute()?;

            // Check for explicit relation type
            if attr.name == "one" {
                relation.relation_type = RelationType::One;
            } else if attr.name == "many" {
                relation.relation_type = RelationType::Many;
            } else if attr.name == "many_to_many" {
                relation.relation_type = RelationType::ManyToMany;
            }

            relation.attributes.push(attr);
        }

        relation.span.end_line = self.current_line();
        relation.span.end_col = self.current_col();

        Ok(relation)
    }

    /// Parse an index definition
    fn parse_index(&mut self) -> Result<Index, ParseError> {
        let start_line = self.current_line();
        let start_col = self.current_col();

        // Expect @@ prefix
        self.expect(TokenKind::At)?;
        self.expect(TokenKind::At)?;

        let kind_name = self.expect_ident()?;
        let index_type = match kind_name.to_lowercase().as_str() {
            "index" => IndexType::Index,
            "unique" => IndexType::Unique,
            "fulltext" => IndexType::Fulltext,
            "gin" => IndexType::Gin,
            _ => {
                return Err(ParseError::syntax(
                    self.current_line(),
                    self.current_col(),
                    format!("Unknown index type: {}", kind_name),
                ))
            }
        };

        self.expect(TokenKind::LParen)?;

        // Parse field list
        let mut fields = Vec::new();
        while !self.check(TokenKind::RParen) {
            fields.push(self.expect_ident()?);
            if self.check(TokenKind::Comma) {
                self.advance()?;
            } else {
                break;
            }
        }
        self.expect(TokenKind::RParen)?;

        let mut index = Index {
            index_type,
            fields,
            name: None,
            attributes: Vec::new(),
            span: Span::new(start_line, start_col, self.current_line(), self.current_col()),
        };

        // Parse optional attributes
        while self.check(TokenKind::At) && !self.peek_check(TokenKind::At) {
            let attr = self.parse_attribute()?;
            index.attributes.push(attr);
        }

        Ok(index)
    }

    /// Parse an enum definition
    fn parse_enum(&mut self) -> Result<EnumDef, ParseError> {
        let start_line = self.current_line();
        let start_col = self.current_col();

        self.expect(TokenKind::Enum)?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::LBrace)?;

        let mut enum_def = EnumDef::new(name);
        enum_def.span = Span::new(start_line, start_col, 0, 0);

        while !self.check(TokenKind::RBrace) {
            let variant = self.parse_enum_variant()?;
            enum_def.variants.push(variant);
        }

        enum_def.span.end_line = self.current_line();
        enum_def.span.end_col = self.current_col();
        self.expect(TokenKind::RBrace)?;

        Ok(enum_def)
    }

    /// Parse an enum variant
    fn parse_enum_variant(&mut self) -> Result<EnumVariant, ParseError> {
        let start_line = self.current_line();
        let start_col = self.current_col();

        let name = self.expect_ident()?;

        let mut variant = EnumVariant {
            name,
            value: None,
            attributes: Vec::new(),
            span: Span::new(start_line, start_col, 0, 0),
        };

        // Check for explicit value
        if self.check(TokenKind::Eq) {
            self.advance()?;
            if let Some(TokenKind::Integer(n)) = self.current_kind() {
                variant.value = Some(*n as i32);
                self.advance()?;
            }
        }

        // Parse attributes
        while self.check(TokenKind::At) {
            let attr = self.parse_attribute()?;
            variant.attributes.push(attr);
        }

        variant.span.end_line = self.current_line();
        variant.span.end_col = self.current_col();

        Ok(variant)
    }

    /// Parse a type definition
    fn parse_type_def(&mut self) -> Result<TypeDef, ParseError> {
        let start_line = self.current_line();
        let start_col = self.current_col();

        self.expect(TokenKind::Type)?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::LBrace)?;

        let mut type_def = TypeDef {
            name,
            fields: Vec::new(),
            attributes: Vec::new(),
            span: Span::new(start_line, start_col, 0, 0),
        };

        while !self.check(TokenKind::RBrace) {
            // Check for spread operator
            if self.check(TokenKind::Spread) {
                self.advance()?;
                let spread_type = self.expect_ident()?;
                // For now, we'll add a marker field for spread
                type_def.fields.push(TypeDefField {
                    name: format!("...{}", spread_type),
                    type_ref: TypeRef::Custom(spread_type),
                    attributes: Vec::new(),
                });
                continue;
            }

            let field_name = self.expect_ident()?;
            let field_type = self.parse_type_ref()?;

            let mut field = TypeDefField {
                name: field_name,
                type_ref: field_type,
                attributes: Vec::new(),
            };

            // Parse attributes
            while self.check(TokenKind::At) {
                let attr = self.parse_attribute()?;
                field.attributes.push(attr);
            }

            type_def.fields.push(field);
        }

        type_def.span.end_line = self.current_line();
        type_def.span.end_col = self.current_col();
        self.expect(TokenKind::RBrace)?;

        Ok(type_def)
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

    fn peek_check(&mut self, expected: TokenKind) -> bool {
        matches!(self.lexer.peek(), Some(token) if std::mem::discriminant(&token.kind) == std::mem::discriminant(&expected))
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

/// Infer relation type from type reference
fn infer_relation_type(type_ref: &TypeRef) -> RelationType {
    match type_ref {
        TypeRef::Array(_) => RelationType::Many,
        TypeRef::Optional(inner) => infer_relation_type(inner),
        _ => RelationType::One,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_model() {
        let source = r#"
            model User {
                collection: users

                fields {
                    id      uuid    @id @default(uuid)
                    email   email   @unique @required
                    name    string
                }
            }
        "#;

        let result = crate::parser::parse_model(source);
        assert!(result.is_ok());

        let file = result.unwrap();
        assert_eq!(file.models.len(), 1);

        let user = &file.models[0];
        assert_eq!(user.name, "User");
        assert_eq!(user.collection, Some("users".to_string()));
        assert_eq!(user.fields.len(), 3);
    }

    #[test]
    fn test_parse_enum() {
        let source = r#"
            enum Status {
                active
                inactive
                pending = 5
            }
        "#;

        let result = crate::parser::parse_model(source);
        assert!(result.is_ok());

        let file = result.unwrap();
        assert_eq!(file.enums.len(), 1);

        let status = &file.enums[0];
        assert_eq!(status.name, "Status");
        assert_eq!(status.variants.len(), 3);
        assert_eq!(status.variants[2].value, Some(5));
    }

    #[test]
    fn test_parse_relations() {
        let source = r#"
            model User {
                fields {
                    id uuid @id
                }

                relations {
                    profile     Profile     @one
                    posts       Post[]      @many
                    roles       Role[]      @many_to_many
                }
            }
        "#;

        let result = crate::parser::parse_model(source);
        assert!(result.is_ok());

        let file = result.unwrap();
        let user = &file.models[0];
        assert_eq!(user.relations.len(), 3);
        assert_eq!(user.relations[0].relation_type, RelationType::One);
        assert_eq!(user.relations[1].relation_type, RelationType::Many);
        assert_eq!(user.relations[2].relation_type, RelationType::ManyToMany);
    }
}
