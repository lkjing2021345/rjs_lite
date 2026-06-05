use crate::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Number(f64),
    String(String),
    Identifier(String),
    Let,
    Const,
    Function,
    Return,
    If,
    Else,
    While,
    True,
    False,
    Null,
    Undefined,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Assign,
    Equal,
    NotEqual,
    StrictEqual,
    StrictNotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
    Bang,
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    Comma,
    Semicolon,
    Dot,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

pub fn keyword_or_identifier(text: String) -> TokenKind {
    match text.as_str() {
        "let" => TokenKind::Let,
        "const" => TokenKind::Const,
        "function" => TokenKind::Function,
        "return" => TokenKind::Return,
        "if" => TokenKind::If,
        "else" => TokenKind::Else,
        "while" => TokenKind::While,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "null" => TokenKind::Null,
        "undefined" => TokenKind::Undefined,
        _ => TokenKind::Identifier(text),
    }
}
