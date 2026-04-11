//! Lexer/Tokenizer for schema files
//!
//! Converts raw schema text into a stream of tokens.

use logos::Logos;
use std::fmt;

/// Token types for the schema language
#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r]+")]
pub enum TokenKind {
    // Keywords
    #[token("model")]
    Model,
    #[token("enum")]
    Enum,
    #[token("type")]
    Type,
    #[token("workflow")]
    Workflow,
    #[token("from")]
    From,
    #[token("fields")]
    Fields,
    #[token("relations")]
    Relations,
    #[token("indexes")]
    Indexes,
    #[token("collection")]
    Collection,
    #[token("states")]
    States,
    #[token("transitions")]
    Transitions,
    #[token("rules")]
    Rules,
    #[token("permissions")]
    Permissions,
    #[token("triggers")]
    Triggers,
    #[token("computed")]
    Computed,
    #[token("initial")]
    Initial,
    #[token("final")]
    Final,
    #[token("on_enter")]
    OnEnter,
    #[token("on_exit")]
    OnExit,
    #[token("when")]
    When,
    #[token("condition")]
    Condition,
    #[token("message")]
    Message,
    #[token("allow")]
    Allow,
    #[token("deny")]
    Deny,
    #[token("only")]
    Only,
    #[token("except")]
    Except,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("null")]
    Null,

    // Operators
    #[token("->")]
    Arrow,
    #[token("=>")]
    FatArrow,
    #[token("...")]
    Spread,
    #[token("@")]
    At,
    #[token(":")]
    Colon,
    #[token(",")]
    Comma,
    #[token(".")]
    Dot,
    #[token("?")]
    Question,
    #[token("!")]
    Bang,
    #[token("=")]
    Eq,
    #[token("==")]
    EqEq,
    #[token("!=")]
    Ne,
    #[token("<")]
    Lt,
    #[token("<=")]
    Le,
    #[token(">")]
    Gt,
    #[token(">=")]
    Ge,
    #[token("&&")]
    And,
    #[token("||")]
    Or,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,

    // Brackets
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,

    // Literals
    #[regex(r#""([^"\\]|\\.)*""#, |lex| {
        let s = lex.slice();
        s[1..s.len()-1].to_string()
    })]
    String(String),

    #[regex(r#"'([^'\\]|\\.)*'"#, |lex| {
        let s = lex.slice();
        s[1..s.len()-1].to_string()
    })]
    SingleQuoteString(String),

    #[regex(r"-?[0-9]+\.[0-9]+", |lex| lex.slice().parse::<f64>().ok())]
    Float(f64),

    #[regex(r"-?[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    Integer(i64),

    // Identifier
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())]
    Ident(String),

    // Context variable ($user, $now, etc.)
    #[regex(r"\$[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice()[1..].to_string())]
    ContextVar(String),

    // Comments
    #[regex(r"//[^\n]*", logos::skip)]
    LineComment,

    #[regex(r"#[^\n]*", logos::skip)]
    HashComment,

    // Newline (we track these for line counting)
    #[token("\n")]
    Newline,
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Model => write!(f, "model"),
            Self::Enum => write!(f, "enum"),
            Self::Type => write!(f, "type"),
            Self::Workflow => write!(f, "workflow"),
            Self::From => write!(f, "from"),
            Self::Fields => write!(f, "fields"),
            Self::Relations => write!(f, "relations"),
            Self::Indexes => write!(f, "indexes"),
            Self::Collection => write!(f, "collection"),
            Self::States => write!(f, "states"),
            Self::Transitions => write!(f, "transitions"),
            Self::Rules => write!(f, "rules"),
            Self::Permissions => write!(f, "permissions"),
            Self::Triggers => write!(f, "triggers"),
            Self::Computed => write!(f, "computed"),
            Self::Initial => write!(f, "initial"),
            Self::Final => write!(f, "final"),
            Self::OnEnter => write!(f, "on_enter"),
            Self::OnExit => write!(f, "on_exit"),
            Self::When => write!(f, "when"),
            Self::Condition => write!(f, "condition"),
            Self::Message => write!(f, "message"),
            Self::Allow => write!(f, "allow"),
            Self::Deny => write!(f, "deny"),
            Self::Only => write!(f, "only"),
            Self::Except => write!(f, "except"),
            Self::If => write!(f, "if"),
            Self::Else => write!(f, "else"),
            Self::True => write!(f, "true"),
            Self::False => write!(f, "false"),
            Self::Null => write!(f, "null"),
            Self::Arrow => write!(f, "->"),
            Self::FatArrow => write!(f, "=>"),
            Self::Spread => write!(f, "..."),
            Self::At => write!(f, "@"),
            Self::Colon => write!(f, ":"),
            Self::Comma => write!(f, ","),
            Self::Dot => write!(f, "."),
            Self::Question => write!(f, "?"),
            Self::Bang => write!(f, "!"),
            Self::Eq => write!(f, "="),
            Self::EqEq => write!(f, "=="),
            Self::Ne => write!(f, "!="),
            Self::Lt => write!(f, "<"),
            Self::Le => write!(f, "<="),
            Self::Gt => write!(f, ">"),
            Self::Ge => write!(f, ">="),
            Self::And => write!(f, "&&"),
            Self::Or => write!(f, "||"),
            Self::Plus => write!(f, "+"),
            Self::Minus => write!(f, "-"),
            Self::Star => write!(f, "*"),
            Self::Slash => write!(f, "/"),
            Self::Percent => write!(f, "%"),
            Self::LBrace => write!(f, "{{"),
            Self::RBrace => write!(f, "}}"),
            Self::LParen => write!(f, "("),
            Self::RParen => write!(f, ")"),
            Self::LBracket => write!(f, "["),
            Self::RBracket => write!(f, "]"),
            Self::String(s) => write!(f, "\"{}\"", s),
            Self::SingleQuoteString(s) => write!(f, "'{}'", s),
            Self::Float(n) => write!(f, "{}", n),
            Self::Integer(n) => write!(f, "{}", n),
            Self::Ident(s) => write!(f, "{}", s),
            Self::ContextVar(s) => write!(f, "${}", s),
            Self::LineComment => write!(f, "// comment"),
            Self::HashComment => write!(f, "# comment"),
            Self::Newline => write!(f, "\\n"),
        }
    }
}

/// A token with position information
#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub col: usize,
    pub span: std::ops::Range<usize>,
}

impl Token {
    pub fn new(kind: TokenKind, line: usize, col: usize, span: std::ops::Range<usize>) -> Self {
        Self {
            kind,
            line,
            col,
            span,
        }
    }
}

/// Lexer that produces tokens with position information
pub struct Lexer<'source> {
    inner: logos::Lexer<'source, TokenKind>,
    source: &'source str,
    line: usize,
    col: usize,
    last_newline_pos: usize,
    peeked: Option<Option<Token>>,
}

impl<'source> Lexer<'source> {
    pub fn new(source: &'source str) -> Self {
        Self {
            inner: TokenKind::lexer(source),
            source,
            line: 1,
            col: 1,
            last_newline_pos: 0,
            peeked: None,
        }
    }

    /// Peek at the next token without consuming it
    pub fn peek(&mut self) -> Option<&Token> {
        if self.peeked.is_none() {
            self.peeked = Some(self.next_token());
        }
        self.peeked.as_ref().and_then(|opt| opt.as_ref())
    }

    /// Get the next token
    pub fn next_token(&mut self) -> Option<Token> {
        // If we have a peeked token, return it
        if let Some(peeked) = self.peeked.take() {
            return peeked;
        }

        loop {
            match self.inner.next() {
                Some(Ok(kind)) => {
                    let span = self.inner.span();

                    // Calculate position
                    let col = span.start - self.last_newline_pos + 1;

                    // Handle newlines
                    if matches!(kind, TokenKind::Newline) {
                        self.line += 1;
                        self.last_newline_pos = span.end;
                        // Skip newlines, they're just for line counting
                        continue;
                    }

                    return Some(Token::new(kind, self.line, col, span));
                }
                Some(Err(_)) => {
                    // Lexer error - skip this character
                    // In production, we'd want better error handling
                    continue;
                }
                None => return None,
            }
        }
    }

    /// Check if there are more tokens
    pub fn has_more(&mut self) -> bool {
        self.peek().is_some()
    }

    /// Get the current line number
    pub fn current_line(&self) -> usize {
        self.line
    }

    /// Get the current column number
    pub fn current_col(&self) -> usize {
        self.col
    }

    /// Get the source text
    pub fn source(&self) -> &'source str {
        self.source
    }
}

impl<'source> Iterator for Lexer<'source> {
    type Item = Token;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_token()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tokens() {
        let source = "model User { fields { id uuid } }";
        let lexer = Lexer::new(source);

        let tokens: Vec<_> = lexer.collect();
        assert_eq!(tokens.len(), 9);

        assert!(matches!(tokens[0].kind, TokenKind::Model));
        assert!(matches!(tokens[1].kind, TokenKind::Ident(ref s) if s == "User"));
        assert!(matches!(tokens[2].kind, TokenKind::LBrace));
    }

    #[test]
    fn test_attributes() {
        let source = "@id @default(uuid) @unique";
        let lexer = Lexer::new(source);

        let tokens: Vec<_> = lexer.collect();
        // @id @default(uuid) @unique = @, id, @, default, (, uuid, ), @, unique = 9 tokens
        assert_eq!(tokens.len(), 9);

        assert!(matches!(tokens[0].kind, TokenKind::At));
        assert!(matches!(tokens[1].kind, TokenKind::Ident(ref s) if s == "id"));
    }

    #[test]
    fn test_string_literals() {
        let source = r#""hello world" 'single quotes'"#;
        let lexer = Lexer::new(source);

        let tokens: Vec<_> = lexer.collect();
        assert_eq!(tokens.len(), 2);

        assert!(matches!(tokens[0].kind, TokenKind::String(ref s) if s == "hello world"));
        assert!(matches!(tokens[1].kind, TokenKind::SingleQuoteString(ref s) if s == "single quotes"));
    }

    #[test]
    fn test_line_counting() {
        let source = "model User {\n  fields {\n    id uuid\n  }\n}";
        let lexer = Lexer::new(source);

        let tokens: Vec<_> = lexer.collect();

        // First token on line 1
        assert_eq!(tokens[0].line, 1);

        // "fields" should be on line 2
        let fields_token = tokens.iter().find(|t| matches!(t.kind, TokenKind::Fields));
        assert!(fields_token.is_some());
        assert_eq!(fields_token.unwrap().line, 2);
    }

    #[test]
    fn test_context_variables() {
        let source = "$user $now $old";
        let lexer = Lexer::new(source);

        let tokens: Vec<_> = lexer.collect();
        assert_eq!(tokens.len(), 3);

        assert!(matches!(tokens[0].kind, TokenKind::ContextVar(ref s) if s == "user"));
        assert!(matches!(tokens[1].kind, TokenKind::ContextVar(ref s) if s == "now"));
        assert!(matches!(tokens[2].kind, TokenKind::ContextVar(ref s) if s == "old"));
    }
}
