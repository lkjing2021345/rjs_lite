use crate::error::{JsError, JsResult, Span};
use crate::token::{Token, TokenKind, keyword_or_identifier};

pub fn lex(source: &str) -> JsResult<Vec<Token>> {
    Lexer::new(source).lex()
}

struct Lexer {
    chars: Vec<char>,
    index: usize,
    byte: usize,
    line: usize,
    column: usize,
    tokens: Vec<Token>,
}

impl Lexer {
    fn new(source: &str) -> Self {
        Self {
            chars: source.chars().collect(),
            index: 0,
            byte: 0,
            line: 1,
            column: 1,
            tokens: Vec::new(),
        }
    }

    fn lex(mut self) -> JsResult<Vec<Token>> {
        if self.peek() == Some('\u{FEFF}') {
            self.advance();
        }
        while let Some(ch) = self.peek() {
            match ch {
                ' ' | '\t' | '\r' | '\n' => {
                    self.advance();
                }
                '0'..='9' => self.number()?,
                '"' | '\'' => self.string(ch)?,
                c if is_ident_start(c) => self.identifier(),
                '+' => self.plus(),
                '-' => self.minus(),
                '*' => self.op_assign(TokenKind::Star, TokenKind::StarAssign),
                '%' => self.op_assign(TokenKind::Percent, TokenKind::PercentAssign),
                '(' => self.single(TokenKind::LeftParen),
                ')' => self.single(TokenKind::RightParen),
                '{' => self.single(TokenKind::LeftBrace),
                '}' => self.single(TokenKind::RightBrace),
                '[' => self.single(TokenKind::LeftBracket),
                ']' => self.single(TokenKind::RightBracket),
                '?' => self.single(TokenKind::Question),
                ':' => self.single(TokenKind::Colon),
                ',' => self.single(TokenKind::Comma),
                ';' => self.single(TokenKind::Semicolon),
                '.' => self.dot_or_spread(),
                '~' => self.single(TokenKind::Tilde),
                '^' => self.single(TokenKind::Caret),
                '!' => self.eq_chain(
                    TokenKind::Bang,
                    TokenKind::NotEqual,
                    TokenKind::StrictNotEqual,
                ),
                '=' => self.equals(),
                '<' => self.less_than(),
                '>' => self.greater_than(),
                '&' => self.ampersand(),
                '|' => self.pipe(),
                '`' => self.template_string(),
                '/' => self.slash()?,
                '#' => self.private_name()?,
                _ => {
                    return Err(JsError::lex(
                        format!("unexpected character `{ch}`"),
                        self.here(),
                    ));
                }
            }
        }
        self.tokens.push(Token::new(TokenKind::Eof, self.here()));
        Ok(self.tokens)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.index).copied()
    }
    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.index + 1).copied()
    }
    fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.index += 1;
        self.byte += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(ch)
    }
    fn here(&self) -> Span {
        Span::new(self.byte, self.byte, self.line, self.column)
    }
    fn span(&self, start: usize, line: usize, column: usize) -> Span {
        Span::new(start, self.byte, line, column)
    }
    fn single(&mut self, kind: TokenKind) {
        let (s, l, c) = (self.byte, self.line, self.column);
        self.advance();
        self.tokens.push(Token::new(kind, self.span(s, l, c)));
    }
    fn plus(&mut self) {
        let (s, l, c) = (self.byte, self.line, self.column);
        self.advance();
        let kind = match self.peek() {
            Some('+') => {
                self.advance();
                TokenKind::PlusPlus
            }
            Some('=') => {
                self.advance();
                TokenKind::PlusAssign
            }
            _ => TokenKind::Plus,
        };
        self.tokens.push(Token::new(kind, self.span(s, l, c)));
    }
    fn minus(&mut self) {
        let (s, l, c) = (self.byte, self.line, self.column);
        self.advance();
        let kind = match self.peek() {
            Some('-') => {
                self.advance();
                TokenKind::MinusMinus
            }
            Some('=') => {
                self.advance();
                TokenKind::MinusAssign
            }
            _ => TokenKind::Minus,
        };
        self.tokens.push(Token::new(kind, self.span(s, l, c)));
    }
    fn op_assign(&mut self, single: TokenKind, assign: TokenKind) {
        let (s, l, c) = (self.byte, self.line, self.column);
        self.advance();
        let kind = if self.peek() == Some('=') {
            self.advance();
            assign
        } else {
            single
        };
        self.tokens.push(Token::new(kind, self.span(s, l, c)));
    }
    fn two(&mut self, single: TokenKind, ch: char, double: TokenKind) {
        let (s, l, c) = (self.byte, self.line, self.column);
        self.advance();
        let kind = if self.peek() == Some(ch) {
            self.advance();
            double
        } else {
            single
        };
        self.tokens.push(Token::new(kind, self.span(s, l, c)));
    }
    fn ampersand(&mut self) {
        let (s, l, c) = (self.byte, self.line, self.column);
        self.advance();
        let kind = if self.peek() == Some('&') {
            self.advance();
            TokenKind::And
        } else {
            TokenKind::Ampersand
        };
        self.tokens.push(Token::new(kind, self.span(s, l, c)));
    }
    fn dot_or_spread(&mut self) {
        let (s, l, c) = (self.byte, self.line, self.column);
        self.advance();
        if self.peek() == Some('.') {
            self.advance();
            if self.peek() == Some('.') {
                self.advance();
                self.tokens.push(Token::new(TokenKind::DotDotDot, self.span(s, l, c)));
            } else {
                self.index -= 2;
                self.tokens.push(Token::new(TokenKind::Dot, self.span(s, l, c)));
            }
        } else {
            self.tokens.push(Token::new(TokenKind::Dot, self.span(s, l, c)));
        }
    }
    fn pipe(&mut self) {
        let (s, l, c) = (self.byte, self.line, self.column);
        self.advance();
        let kind = if self.peek() == Some('|') {
            self.advance();
            TokenKind::Or
        } else {
            TokenKind::Pipe
        };
        self.tokens.push(Token::new(kind, self.span(s, l, c)));
    }
    fn less_than(&mut self) {
        let (s, l, c) = (self.byte, self.line, self.column);
        self.advance();
        let kind = match self.peek() {
            Some('<') => {
                self.advance();
                TokenKind::LeftShift
            }
            Some('=') => {
                self.advance();
                TokenKind::LessEqual
            }
            _ => TokenKind::Less,
        };
        self.tokens.push(Token::new(kind, self.span(s, l, c)));
    }
    fn greater_than(&mut self) {
        let (s, l, c) = (self.byte, self.line, self.column);
        self.advance();
        let kind = match self.peek() {
            Some('>') => {
                self.advance();
                if self.peek() == Some('>') {
                    self.advance();
                    TokenKind::UnsignedRightShift
                } else {
                    TokenKind::RightShift
                }
            }
            Some('=') => {
                self.advance();
                TokenKind::GreaterEqual
            }
            _ => TokenKind::Greater,
        };
        self.tokens.push(Token::new(kind, self.span(s, l, c)));
    }
    fn eq_chain(&mut self, single: TokenKind, double: TokenKind, triple: TokenKind) {
        let (s, l, c) = (self.byte, self.line, self.column);
        self.advance();
        let kind = if self.peek() == Some('=') {
            self.advance();
            if self.peek() == Some('=') {
                self.advance();
                triple
            } else {
                double
            }
        } else {
            single
        };
        self.tokens.push(Token::new(kind, self.span(s, l, c)));
    }
    fn equals(&mut self) {
        let (s, l, c) = (self.byte, self.line, self.column);
        self.advance();
        let kind = match self.peek() {
            Some('=') => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::StrictEqual
                } else {
                    TokenKind::Equal
                }
            }
            Some('>') => {
                self.advance();
                TokenKind::Arrow
            }
            _ => TokenKind::Assign,
        };
        self.tokens.push(Token::new(kind, self.span(s, l, c)));
    }
    fn double(&mut self, expected: char, kind: TokenKind) -> JsResult<()> {
        let (s, l, c) = (self.byte, self.line, self.column);
        self.advance();
        if self.peek() != Some(expected) {
            return Err(JsError::lex(
                "expected repeated operator",
                self.span(s, l, c),
            ));
        }
        self.advance();
        self.tokens.push(Token::new(kind, self.span(s, l, c)));
        Ok(())
    }
    fn template_string(&mut self) {
        let (s, l, c) = (self.byte, self.line, self.column);
        self.advance();
        let mut buf = String::new();
        while let Some(ch) = self.peek() {
            if ch == '`' {
                self.advance();
                break;
            }
            if ch == '\\' {
                self.advance();
                if let Some(next) = self.peek() {
                    self.advance();
                    buf.push(match next {
                        'n' => '\n', 't' => '\t', 'r' => '\r',
                        '\\' => '\\', '`' => '`', '$' => '$',
                        c => c,
                    });
                }
                continue;
            }
            self.advance();
            buf.push(ch);
        }
        self.tokens.push(Token::new(TokenKind::String(buf), self.span(s, l, c)));
    }
    fn slash(&mut self) -> JsResult<()> {
        match self.peek_next() {
            Some('/') => {
                while self.peek().is_some_and(|c| c != '\n') {
                    self.advance();
                }
                Ok(())
            }
            Some('*') => {
                let (s, l, c) = (self.byte, self.line, self.column);
                self.advance();
                self.advance();
                while let Some(ch) = self.peek() {
                    if ch == '*' && self.peek_next() == Some('/') {
                        self.advance();
                        self.advance();
                        return Ok(());
                    }
                    self.advance();
                }
                Err(JsError::lex(
                    "unterminated block comment",
                    self.span(s, l, c),
                ))
            }
            _ => {
                let (s, l, c) = (self.byte, self.line, self.column);
                self.advance();
                let is_regexp = match self.tokens.last() {
                    None => true,
                    Some(t) => matches!(
                        t.kind,
                        TokenKind::LeftParen | TokenKind::LeftBracket | TokenKind::LeftBrace
                            | TokenKind::Comma | TokenKind::Semicolon | TokenKind::Colon
                            | TokenKind::Question | TokenKind::Bang | TokenKind::Tilde
                            | TokenKind::Assign | TokenKind::PlusAssign | TokenKind::MinusAssign
                            | TokenKind::StarAssign | TokenKind::SlashAssign | TokenKind::PercentAssign
                            | TokenKind::Return | TokenKind::Throw | TokenKind::Case
                            | TokenKind::Delete | TokenKind::Void | TokenKind::Typeof
                            | TokenKind::In | TokenKind::Instanceof
                            | TokenKind::Equal | TokenKind::NotEqual | TokenKind::StrictEqual | TokenKind::StrictNotEqual
                            | TokenKind::Less | TokenKind::LessEqual | TokenKind::Greater | TokenKind::GreaterEqual
                            | TokenKind::Plus | TokenKind::Minus | TokenKind::Star | TokenKind::Slash | TokenKind::Percent
                            | TokenKind::And | TokenKind::Or | TokenKind::Ampersand | TokenKind::Pipe | TokenKind::Caret
                            | TokenKind::LeftShift | TokenKind::RightShift | TokenKind::UnsignedRightShift
                            | TokenKind::Arrow
                    ),
                };
                if is_regexp {
                    let mut pattern = String::new();
                    let mut in_class = false;
                    let mut valid = true;
                    loop {
                        match self.peek() {
                            None => { valid = false; break; }
                            Some('\n') => { valid = false; break; }
                            Some('/') if !in_class => {
                                self.advance();
                                break;
                            }
                            Some('\\') => {
                                pattern.push('\\');
                                self.advance();
                                if let Some(ch) = self.peek() {
                                    pattern.push(ch);
                                    self.advance();
                                } else { valid = false; break; }
                            }
                            Some('[') => { in_class = true; pattern.push('['); self.advance(); }
                            Some(']') => { in_class = false; pattern.push(']'); self.advance(); }
                            Some(ch) => { pattern.push(ch); self.advance(); }
                        }
                    }
                    if valid {
                        let mut flags = String::new();
                        while let Some(ch) = self.peek() {
                            if matches!(ch, 'g' | 'i' | 'm' | 's' | 'u' | 'y' | 'd') {
                                flags.push(ch);
                                self.advance();
                            } else { break; }
                        }
                        self.tokens.push(Token::new(
                            TokenKind::RegExp(pattern, flags),
                            self.span(s, l, c),
                        ));
                        return Ok(());
                    }
                }
                self.op_assign(TokenKind::Slash, TokenKind::SlashAssign);
                Ok(())
            }
        }
    }
    fn number(&mut self) -> JsResult<()> {
        let (s, l, c) = (self.byte, self.line, self.column);
        let mut text = String::new();
        while let Some(d) = self.peek().filter(|x| x.is_ascii_digit()) {
            text.push(d);
            self.advance();
        }
        if self.peek() == Some('.') && self.peek_next().is_some_and(|x| x.is_ascii_digit()) {
            text.push('.');
            self.advance();
            while let Some(d) = self.peek().filter(|x| x.is_ascii_digit()) {
                text.push(d);
                self.advance();
            }
        }
        // BigInt literal: digits followed by `n` (e.g. `1n`, `0x1Fn`).
        // A `.` was not consumed, so a float can never be a BigInt.
        if self.peek() == Some('n') {
            self.advance();
            self.tokens
                .push(Token::new(TokenKind::BigInt(text), self.span(s, l, c)));
            return Ok(());
        }
        let n = text
            .parse()
            .map_err(|_| JsError::lex("invalid number", self.span(s, l, c)))?;
        self.tokens
            .push(Token::new(TokenKind::Number(n), self.span(s, l, c)));
        Ok(())
    }
    fn string(&mut self, quote: char) -> JsResult<()> {
        let (s, l, c) = (self.byte, self.line, self.column);
        self.advance();
        let mut out = String::new();
        while let Some(ch) = self.peek() {
            if ch == quote {
                self.advance();
                self.tokens
                    .push(Token::new(TokenKind::String(out), self.span(s, l, c)));
                return Ok(());
            }
            if ch == '\\' {
                self.advance();
                let e = self
                    .advance()
                    .ok_or_else(|| JsError::lex("unterminated string", self.span(s, l, c)))?;
                out.push(match e {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    other => other,
                });
            } else {
                out.push(ch);
                self.advance();
            }
        }
        Err(JsError::lex("unterminated string", self.span(s, l, c)))
    }
    fn identifier(&mut self) {
        let (s, l, c) = (self.byte, self.line, self.column);
        let mut text = String::new();
        while let Some(ch) = self.peek().filter(|x| is_ident_part(*x)) {
            text.push(ch);
            self.advance();
        }
        self.tokens
            .push(Token::new(keyword_or_identifier(text), self.span(s, l, c)));
    }

    /// Lex a private name such as `#field`. The leading `#` is stored as part
    /// of the name so it can never collide with an ordinary property key.
    fn private_name(&mut self) -> JsResult<()> {
        let (s, l, c) = (self.byte, self.line, self.column);
        self.advance(); // consume `#`
        if !self.peek().is_some_and(is_ident_start) {
            return Err(JsError::lex(
                "unexpected character `#`",
                self.span(s, l, c),
            ));
        }
        let mut text = String::from("#");
        while let Some(ch) = self.peek().filter(|x| is_ident_part(*x)) {
            text.push(ch);
            self.advance();
        }
        self.tokens
            .push(Token::new(TokenKind::PrivateName(text), self.span(s, l, c)));
        Ok(())
    }
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_ascii_alphabetic()
}
fn is_ident_part(ch: char) -> bool {
    is_ident_start(ch) || ch.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lexes_comments() {
        let t = lex("let x=1;//x\nconst y='a';").unwrap();
        assert!(matches!(t[0].kind, TokenKind::Let));
        assert!(matches!(t[5].kind, TokenKind::Const));
    }
    #[test]
    fn rejects_bad_string() {
        assert!(lex("'oops").is_err());
    }
}
