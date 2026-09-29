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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsErrorType {
    Error,
    SyntaxError,
    TypeError,
    ReferenceError,
    RangeError,
}

impl JsErrorType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Error => "Error",
            Self::SyntaxError => "SyntaxError",
            Self::TypeError => "TypeError",
            Self::ReferenceError => "ReferenceError",
            Self::RangeError => "RangeError",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum JsError {
    Lex { message: String, span: Span },
    Parse { message: String, span: Span },
    Runtime {
        message: String,
        error_type: JsErrorType,
    },
    /// Control-flow sentinel: wraps a `Flow` value (Return/Throw) so it can
    /// travel through `JsResult<()>` without a separate error type.
    Flow(crate::interpreter::Flow),
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
            error_type: JsErrorType::Error,
        }
    }

    pub fn type_error(message: impl Into<String>) -> Self {
        Self::Runtime {
            message: message.into(),
            error_type: JsErrorType::TypeError,
        }
    }

    pub fn reference_error(message: impl Into<String>) -> Self {
        Self::Runtime {
            message: message.into(),
            error_type: JsErrorType::ReferenceError,
        }
    }

    pub fn range_error(message: impl Into<String>) -> Self {
        Self::Runtime {
            message: message.into(),
            error_type: JsErrorType::RangeError,
        }
    }

    pub fn syntax_error(message: impl Into<String>) -> Self {
        Self::Runtime {
            message: message.into(),
            error_type: JsErrorType::SyntaxError,
        }
    }

    /// Wrap a `Flow` value as a `JsError` so it can travel through
    /// `JsResult<()>` in the VM's dispatch loop.
    pub fn flow(flow: crate::interpreter::Flow) -> Self {
        Self::Flow(flow)
    }

    /// Extract the wrapped `Flow` if this is a flow sentinel.
    pub fn as_flow(&self) -> Option<&crate::interpreter::Flow> {
        match self {
            JsError::Flow(f) => Some(f),
            _ => None,
        }
    }

    pub fn error_type(&self) -> Option<&JsErrorType> {
        match self {
            JsError::Runtime { error_type, .. } => Some(error_type),
            JsError::Parse { .. } | JsError::Lex { .. } | JsError::Flow(_) => None,
        }
    }
}

impl fmt::Display for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsError::Lex { message, span } => {
                write!(f, "SyntaxError: lex error at {}:{}: {}", span.line, span.column, message)
            }
            JsError::Parse { message, span } => write!(
                f,
                "SyntaxError: parse error at {}:{}: {}",
                span.line, span.column, message
            ),
            JsError::Runtime {
                message,
                error_type,
            } => write!(f, "{}: {}", error_type.as_str(), message),
            JsError::Flow(flow) => write!(f, "Flow: {flow:?}"),
        }
    }
}

impl std::error::Error for JsError {}
