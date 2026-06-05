use std::fmt;

pub type JsResult<T> = Result<T, JsError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
}

impl Span {
    pub fn new(start: usize, end: usize, line: usize, column: usize) -> Self {
        Self {
            start,
            end,
            line,
            column,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum JsError {
    Lex { message: String, span: Span },
    Parse { message: String, span: Span },
    Runtime { message: String },
}

impl JsError {
    pub fn lex(message: impl Into<String>, span: Span) -> Self {
        Self::Lex {
            message: message.into(),
            span,
        }
    }

    pub fn parse(message: impl Into<String>, span: Span) -> Self {
        Self::Parse {
            message: message.into(),
            span,
        }
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        Self::Runtime {
            message: message.into(),
        }
    }
}

impl fmt::Display for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsError::Lex { message, span } => {
                write!(f, "lex error at {}:{}: {}", span.line, span.column, message)
            }
            JsError::Parse { message, span } => write!(
                f,
                "parse error at {}:{}: {}",
                span.line, span.column, message
            ),
            JsError::Runtime { message } => write!(f, "runtime error: {message}"),
        }
    }
}

impl std::error::Error for JsError {}
