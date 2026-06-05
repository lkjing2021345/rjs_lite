use crate::ast::Stmt;
use std::fmt;

#[derive(Clone, Debug)]
pub enum Value {
    Number(f64),
    String(String),
    Bool(bool),
    Null,
    Undefined,
    Function(Function),
    NativeFunction(&'static str, usize),
}

#[derive(Clone, Debug)]
pub struct Function {
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
}

impl Value {
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(v) => *v,
            Value::Null | Value::Undefined => false,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            Value::Function(_) | Value::NativeFunction(_, _) => true,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Bool(_) => "boolean",
            Value::Null => "null",
            Value::Undefined => "undefined",
            Value::Function(_) | Value::NativeFunction(_, _) => "function",
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Null, Value::Null) | (Value::Undefined, Value::Undefined) => true,
            _ => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number(n) if n.fract() == 0.0 => write!(f, "{}", *n as i64),
            Value::Number(n) => write!(f, "{n}"),
            Value::String(s) => write!(f, "{s}"),
            Value::Bool(v) => write!(f, "{v}"),
            Value::Null => write!(f, "null"),
            Value::Undefined => write!(f, "undefined"),
            Value::Function(_) | Value::NativeFunction(_, _) => write!(f, "[Function]"),
        }
    }
}
