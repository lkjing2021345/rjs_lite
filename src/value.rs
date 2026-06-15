use crate::ast::Stmt;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

pub type ObjectRef = Rc<RefCell<Object>>;

#[derive(Clone)]
pub enum Value {
    Number(f64),
    String(String),
    Bool(bool),
    Null,
    Undefined,
    Object(ObjectRef),
}

pub struct Object {
    pub props: HashMap<String, Value>,
    pub proto: Option<ObjectRef>,
    pub internal: Internal,
}

#[derive(Clone)]
pub enum Internal {
    Plain,
    Array(Vec<Value>),
    Function {
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    Native(&'static str),
}

impl Object {
    pub fn plain() -> ObjectRef {
        Rc::new(RefCell::new(Object {
            props: HashMap::new(),
            proto: None,
            internal: Internal::Plain,
        }))
    }

    pub fn with_internal(internal: Internal) -> ObjectRef {
        Rc::new(RefCell::new(Object {
            props: HashMap::new(),
            proto: None,
            internal,
        }))
    }

    /// Look up `name` on this object then walk the prototype chain.
    pub fn lookup(this: &ObjectRef, name: &str) -> Option<Value> {
        if let Some(v) = this.borrow().props.get(name).cloned() {
            return Some(v);
        }
        let proto = this.borrow().proto.clone();
        match proto {
            Some(p) => Object::lookup(&p, name),
            None => None,
        }
    }
}

#[derive(Clone)]
pub struct Function {
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
}

impl Value {
    pub fn array(items: Vec<Value>) -> Value {
        Value::Object(Object::with_internal(Internal::Array(items)))
    }

    pub fn function(params: Vec<String>, body: Vec<Stmt>) -> Value {
        Value::Object(Object::with_internal(Internal::Function { params, body }))
    }

    pub fn native(name: &'static str) -> Value {
        Value::Object(Object::with_internal(Internal::Native(name)))
    }

    pub fn is_callable(&self) -> bool {
        matches!(
            self,
            Value::Object(o)
                if matches!(o.borrow().internal, Internal::Function { .. } | Internal::Native(_))
        )
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(v) => *v,
            Value::Null | Value::Undefined => false,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            Value::Object(_) => true,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Bool(_) => "boolean",
            Value::Null => "object",
            Value::Undefined => "undefined",
            Value::Object(o) => match o.borrow().internal {
                Internal::Function { .. } | Internal::Native(_) => "function",
                _ => "object",
            },
        }
    }

    pub fn to_number(&self) -> f64 {
        match self {
            Value::Number(n) => *n,
            Value::Bool(true) => 1.0,
            Value::Bool(false) => 0.0,
            Value::Null => 0.0,
            Value::Undefined => f64::NAN,
            Value::String(s) => {
                let t = s.trim();
                if t.is_empty() {
                    0.0
                } else {
                    t.parse().unwrap_or(f64::NAN)
                }
            }
            Value::Object(_) => f64::NAN,
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
            (Value::Object(a), Value::Object(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number(n) if n.fract() == 0.0 && n.is_finite() => write!(f, "{}", *n as i64),
            Value::Number(n) if n.is_nan() => write!(f, "NaN"),
            Value::Number(n) if n.is_infinite() => {
                write!(f, "{}", if *n > 0.0 { "Infinity" } else { "-Infinity" })
            }
            Value::Number(n) => write!(f, "{n}"),
            Value::String(s) => write!(f, "{s}"),
            Value::Bool(v) => write!(f, "{v}"),
            Value::Null => write!(f, "null"),
            Value::Undefined => write!(f, "undefined"),
            Value::Object(o) => {
                let obj = o.borrow();
                match &obj.internal {
                    Internal::Function { .. } | Internal::Native(_) => {
                        write!(f, "function () {{ [native code] }}")
                    }
                    Internal::Array(items) => {
                        let parts: Vec<String> = items
                            .iter()
                            .map(|v| match v {
                                Value::Null | Value::Undefined => String::new(),
                                other => other.to_string(),
                            })
                            .collect();
                        write!(f, "{}", parts.join(","))
                    }
                    Internal::Plain => {
                        // Error-like objects print "name: message".
                        if let (Some(name), Some(msg)) =
                            (obj.props.get("name"), obj.props.get("message"))
                        {
                            return write!(f, "{name}: {msg}");
                        }
                        write!(f, "[object Object]")
                    }
                }
            }
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}
