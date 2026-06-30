use crate::ast::Stmt;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
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
    pub non_enumerable_props: HashSet<String>,
    pub proto: Option<ObjectRef>,
    pub internal: Internal,
}

pub type ArraySlot = Option<Value>;

#[derive(Clone)]
pub enum Internal {
    Plain,
    Array(Vec<ArraySlot>),
    Function {
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    Native(&'static str),
    Bound {
        target: ObjectRef,
        bound_this: Value,
        bound_args: Vec<Value>,
    },
}

impl Object {
    pub fn plain() -> ObjectRef {
        Rc::new(RefCell::new(Object {
            props: HashMap::new(),
            non_enumerable_props: HashSet::new(),
            proto: None,
            internal: Internal::Plain,
        }))
    }

    pub fn with_internal(internal: Internal) -> ObjectRef {
        Rc::new(RefCell::new(Object {
            props: HashMap::new(),
            non_enumerable_props: HashSet::new(),
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
        Value::Object(Object::with_internal(Internal::Array(
            items.into_iter().map(Some).collect(),
        )))
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
                if matches!(o.borrow().internal, Internal::Function { .. } | Internal::Native(_) | Internal::Bound { .. })
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
                Internal::Function { .. } | Internal::Native(_) | Internal::Bound { .. } => "function",
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

    pub fn to_primitive(&self) -> Value {
        match self {
            Value::Object(o) => {
                let obj = o.borrow();
                if let Some(v) = obj.props.get("valueOf") {
                    if v.is_callable() {
                        return Value::Undefined;
                    }
                }
                if let Some(v) = obj.props.get("toString") {
                    if v.is_callable() {
                        return Value::Undefined;
                    }
                }
                match &obj.internal {
                    Internal::Array(items) => {
                        let parts: Vec<String> = items
                            .iter()
                            .map(|v| match v {
                                Some(Value::Null | Value::Undefined) | None => String::new(),
                                Some(other) => other.to_string(),
                            })
                            .collect();
                        Value::String(parts.join(","))
                    }
                    Internal::Function { .. } | Internal::Native(_) | Internal::Bound { .. } => {
                        Value::String("function () { [native code] }".into())
                    }
                    Internal::Plain => Value::String("[object Object]".into()),
                }
            }
            other => other.clone(),
        }
    }

    pub fn abstract_eq(&self, other: &Value) -> bool {
        if std::mem::discriminant(self) == std::mem::discriminant(other) {
            return self == other;
        }
        if matches!((self, other), (Value::Null, Value::Undefined) | (Value::Undefined, Value::Null))
        {
            return true;
        }
        if let (Value::Number(a), Value::String(b)) = (self, other) {
            return *a == Value::String(b.clone()).to_number();
        }
        if let (Value::String(a), Value::Number(b)) = (self, other) {
            return Value::String(a.clone()).to_number() == *b;
        }
        if let Value::Bool(b) = self {
            return Value::Number(if *b { 1.0 } else { 0.0 }).abstract_eq(other);
        }
        if let Value::Bool(b) = other {
            return self.abstract_eq(&Value::Number(if *b { 1.0 } else { 0.0 }));
        }
        if matches!(self, Value::String(_) | Value::Number(_)) && matches!(other, Value::Object(_)) {
            return self.abstract_eq(&other.to_primitive());
        }
        if matches!(self, Value::Object(_)) && matches!(other, Value::String(_) | Value::Number(_)) {
            return self.to_primitive().abstract_eq(other);
        }
        false
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
                    Internal::Function { .. } | Internal::Native(_) | Internal::Bound { .. } => {
                        write!(f, "function () {{ [native code] }}")
                    }
                    Internal::Array(items) => {
                        let parts: Vec<String> = items
                            .iter()
                            .map(|v| match v {
                                Some(Value::Null | Value::Undefined) | None => String::new(),
                                Some(other) => other.to_string(),
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
