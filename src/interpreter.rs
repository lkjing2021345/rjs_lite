use crate::ast::{BinaryOp, Expr, Program, Stmt, UnaryOp};
use crate::error::{JsError, JsResult};
use crate::value::{Internal, Object, ObjectRef, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone)]
struct Binding {
    value: Value,
    mutable: bool,
}

#[derive(Clone)]
struct Env {
    values: HashMap<String, Binding>,
    parent: Option<Rc<RefCell<Env>>>,
}

impl Env {
    fn new() -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            values: HashMap::new(),
            parent: None,
        }))
    }

    fn child(parent: Rc<RefCell<Env>>) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            values: HashMap::new(),
            parent: Some(parent),
        }))
    }

    fn define(&mut self, name: String, value: Value, mutable: bool) {
        self.values.insert(name, Binding { value, mutable });
    }

    fn get(&self, name: &str) -> Option<Value> {
        self.values
            .get(name)
            .map(|b| b.value.clone())
            .or_else(|| self.parent.as_ref().and_then(|p| p.borrow().get(name)))
    }

    fn assign(&mut self, name: &str, value: Value) -> JsResult<()> {
        if let Some(binding) = self.values.get_mut(name) {
            if !binding.mutable {
                return Err(JsError::type_error(format!("assignment to constant variable `{name}`")));
            }
            binding.value = value;
            return Ok(());
        }
        if let Some(parent) = &self.parent {
            return parent.borrow_mut().assign(name, value);
        }
        self.define(name.to_string(), value, true);
        Ok(())
    }
}

enum Flow {
    Value(Value),
    Return(Value),
    Throw(Value),
    Break,
    Continue,
}

struct RefTarget {
    object: ObjectRef,
    property: String,
}

pub struct Interpreter {
    env: Rc<RefCell<Env>>,
    closures: HashMap<usize, Rc<RefCell<Env>>>,
    output: Vec<String>,
    global: ObjectRef,
    object_proto: ObjectRef,
    function_proto: ObjectRef,
    array_proto: ObjectRef,
    error_proto: ObjectRef,
    string_proto: ObjectRef,
    number_proto: ObjectRef,
    boolean_proto: ObjectRef,
    step_limit: Option<usize>,
    steps: usize,
    max_call_depth: Option<usize>,
    call_depth: usize,
    output_limit: Option<usize>,
    output_truncated: bool,
    strict: bool,
}

impl Interpreter {
    pub fn new() -> Self {
        Self::new_with_limits(None, None, None)
    }

    pub fn with_step_limit(step_limit: usize) -> Self {
        Self::new_with_limits(Some(step_limit), None, None)
    }

    pub fn with_call_depth_limit(max_call_depth: usize) -> Self {
        Self::new_with_limits(None, Some(max_call_depth), None)
    }

    pub fn with_limits(step_limit: usize, max_call_depth: usize) -> Self {
        Self::new_with_limits(Some(step_limit), Some(max_call_depth), None)
    }

    pub fn with_output_limit(output_limit: usize) -> Self {
        Self::new_with_limits(None, None, Some(output_limit))
    }

    pub fn with_limits_and_output_limit(
        step_limit: usize,
        max_call_depth: usize,
        output_limit: usize,
    ) -> Self {
        Self::new_with_limits(Some(step_limit), Some(max_call_depth), Some(output_limit))
    }

    fn new_with_limits(
        step_limit: Option<usize>,
        max_call_depth: Option<usize>,
        output_limit: Option<usize>,
    ) -> Self {
        let env = Env::new();
        let global = Object::plain();
        let object_proto = Object::plain();
        let function_proto = Object::plain();
        let array_proto = Object::plain();
        let error_proto = Object::plain();
        let string_proto = Object::plain();
        let number_proto = Object::plain();
        let boolean_proto = Object::plain();
        let mut this = Self {
            env,
            closures: HashMap::new(),
            output: Vec::new(),
            global,
            object_proto,
            function_proto,
            array_proto,
            error_proto,
            string_proto,
            number_proto,
            boolean_proto,
            step_limit,
            steps: 0,
            max_call_depth,
            call_depth: 0,
            output_limit,
            output_truncated: false,
            strict: false,
        };
        this.install_builtins();
        this
    }

    fn install_builtins(&mut self) {
        self.define_native("print");
        self.define_native("Error");
        self.define_native("TypeError");
        self.define_native("SyntaxError");
        self.define_native("ReferenceError");
        self.define_native("RangeError");
        self.define_native("Object");
        self.define_native("Array");
        self.define_native("String");
        self.define_native("Number");
        self.define_native("Boolean");
        self.define_native("isNaN");
        self.define_native("isFinite");
        self.define_native("parseInt");
        self.define_native("parseFloat");
        self.define_native("eval");
        self.define_native("Function");

        self.define_global("Infinity", Value::Number(f64::INFINITY), false);
        self.define_global("NaN", Value::Number(f64::NAN), false);
        self.define_global("undefined", Value::Undefined, false);

        let json = Object::plain();
        json.borrow_mut().proto = Some(self.object_proto.clone());
        Self::define_non_enumerable(&json, "stringify", self.native_method("JSON.stringify"));
        self.define_global("JSON", Value::Object(json), false);

        let math = Object::plain();
        math.borrow_mut().proto = Some(self.object_proto.clone());
        for (name, value) in [
            ("E", std::f64::consts::E),
            ("LN10", std::f64::consts::LN_10),
            ("LN2", std::f64::consts::LN_2),
            ("LOG10E", std::f64::consts::LOG10_E),
            ("LOG2E", std::f64::consts::LOG2_E),
            ("PI", std::f64::consts::PI),
            ("SQRT1_2", std::f64::consts::FRAC_1_SQRT_2),
            ("SQRT2", std::f64::consts::SQRT_2),
        ] {
            math.borrow_mut().props.insert(name.to_string(), Value::Number(value));
        }
        Self::define_non_enumerable(&math, "abs", self.native_method("Math.abs"));
        Self::define_non_enumerable(&math, "acos", self.native_method("Math.acos"));
        Self::define_non_enumerable(&math, "acosh", self.native_method("Math.acosh"));
        Self::define_non_enumerable(&math, "asin", self.native_method("Math.asin"));
        Self::define_non_enumerable(&math, "asinh", self.native_method("Math.asinh"));
        Self::define_non_enumerable(&math, "atan", self.native_method("Math.atan"));
        Self::define_non_enumerable(&math, "atanh", self.native_method("Math.atanh"));
        Self::define_non_enumerable(&math, "atan2", self.native_method("Math.atan2"));
        Self::define_non_enumerable(&math, "cbrt", self.native_method("Math.cbrt"));
        Self::define_non_enumerable(&math, "ceil", self.native_method("Math.ceil"));
        Self::define_non_enumerable(&math, "clz32", self.native_method("Math.clz32"));
        Self::define_non_enumerable(&math, "cos", self.native_method("Math.cos"));
        Self::define_non_enumerable(&math, "cosh", self.native_method("Math.cosh"));
        Self::define_non_enumerable(&math, "exp", self.native_method("Math.exp"));
        Self::define_non_enumerable(&math, "expm1", self.native_method("Math.expm1"));
        Self::define_non_enumerable(&math, "floor", self.native_method("Math.floor"));
        Self::define_non_enumerable(&math, "fround", self.native_method("Math.fround"));
        Self::define_non_enumerable(&math, "hypot", self.native_method("Math.hypot"));
        Self::define_non_enumerable(&math, "imul", self.native_method("Math.imul"));
        Self::define_non_enumerable(&math, "log", self.native_method("Math.log"));
        Self::define_non_enumerable(&math, "log10", self.native_method("Math.log10"));
        Self::define_non_enumerable(&math, "log1p", self.native_method("Math.log1p"));
        Self::define_non_enumerable(&math, "log2", self.native_method("Math.log2"));
        Self::define_non_enumerable(&math, "max", self.native_method("Math.max"));
        Self::define_non_enumerable(&math, "min", self.native_method("Math.min"));
        Self::define_non_enumerable(&math, "pow", self.native_method("Math.pow"));
        Self::define_non_enumerable(&math, "random", self.native_method("Math.random"));
        Self::define_non_enumerable(&math, "round", self.native_method("Math.round"));
        Self::define_non_enumerable(&math, "sign", self.native_method("Math.sign"));
        Self::define_non_enumerable(&math, "sin", self.native_method("Math.sin"));
        Self::define_non_enumerable(&math, "sinh", self.native_method("Math.sinh"));
        Self::define_non_enumerable(&math, "sqrt", self.native_method("Math.sqrt"));
        Self::define_non_enumerable(&math, "tan", self.native_method("Math.tan"));
        Self::define_non_enumerable(&math, "tanh", self.native_method("Math.tanh"));
        Self::define_non_enumerable(&math, "trunc", self.native_method("Math.trunc"));
        self.define_global("Math", Value::Object(math), false);

        self.function_proto.borrow_mut().proto = Some(self.object_proto.clone());
        Self::define_non_enumerable(
            &self.function_proto,
            "call",
            self.native_method("Function.prototype.call"),
        );
        Self::define_non_enumerable(
            &self.function_proto,
            "apply",
            self.native_method("Function.prototype.apply"),
        );
        Self::define_non_enumerable(
            &self.function_proto,
            "bind",
            self.native_method("Function.prototype.bind"),
        );
        self.array_proto.borrow_mut().proto = Some(self.object_proto.clone());
        self.error_proto.borrow_mut().proto = Some(self.object_proto.clone());
        self.string_proto.borrow_mut().proto = Some(self.object_proto.clone());
        self.number_proto.borrow_mut().proto = Some(self.object_proto.clone());
        self.boolean_proto.borrow_mut().proto = Some(self.object_proto.clone());

        if let Some(Value::Object(object_ctor)) = self.env.borrow().get("Object") {
            Self::define_non_enumerable(
                &object_ctor,
                "prototype",
                Value::Object(self.object_proto.clone()),
            );
            Self::define_non_enumerable(&object_ctor, "keys", self.native_method("Object.keys"));
            Self::define_non_enumerable(
                &object_ctor,
                "values",
                self.native_method("Object.values"),
            );
            Self::define_non_enumerable(
                &object_ctor,
                "defineProperty",
                self.native_method("Object.defineProperty"),
            );
            Self::define_non_enumerable(
                &object_ctor,
                "getOwnPropertyDescriptor",
                self.native_method("Object.getOwnPropertyDescriptor"),
            );
            Self::define_non_enumerable(
                &object_ctor,
                "create",
                self.native_method("Object.create"),
            );
            Self::define_non_enumerable(
                &object_ctor,
                "getPrototypeOf",
                self.native_method("Object.getPrototypeOf"),
            );
            Self::define_non_enumerable(
                &object_ctor,
                "assign",
                self.native_method("Object.assign"),
            );
            Self::define_non_enumerable(
                &object_ctor,
                "entries",
                self.native_method("Object.entries"),
            );
            Self::define_non_enumerable(
                &object_ctor,
                "is",
                self.native_method("Object.is"),
            );
            Self::define_non_enumerable(
                &object_ctor,
                "getOwnPropertyNames",
                self.native_method("Object.getOwnPropertyNames"),
            );
            Self::define_non_enumerable(
                &object_ctor,
                "setPrototypeOf",
                self.native_method("Object.setPrototypeOf"),
            );
            Self::define_non_enumerable(
                &object_ctor,
                "preventExtensions",
                self.native_method("Object.preventExtensions"),
            );
            Self::define_non_enumerable(
                &object_ctor,
                "isExtensible",
                self.native_method("Object.isExtensible"),
            );
            Self::define_non_enumerable(
                &object_ctor,
                "freeze",
                self.native_method("Object.freeze"),
            );
            Self::define_non_enumerable(
                &object_ctor,
                "isFrozen",
                self.native_method("Object.isFrozen"),
            );
            Self::define_non_enumerable(
                &object_ctor,
                "seal",
                self.native_method("Object.seal"),
            );
            Self::define_non_enumerable(
                &object_ctor,
                "isSealed",
                self.native_method("Object.isSealed"),
            );
            Self::define_non_enumerable(
                &self.object_proto,
                "toString",
                self.native_method("Object.prototype.toString"),
            );
            Self::define_non_enumerable(
                &self.object_proto,
                "hasOwnProperty",
                self.native_method("Object.prototype.hasOwnProperty"),
            );
            Self::define_non_enumerable(
                &self.object_proto,
                "propertyIsEnumerable",
                self.native_method("Object.prototype.propertyIsEnumerable"),
            );
        }
        if let Some(Value::Object(array_ctor)) = self.env.borrow().get("Array") {
            Self::define_non_enumerable(
                &array_ctor,
                "prototype",
                Value::Object(self.array_proto.clone()),
            );
            Self::define_non_enumerable(
                &array_ctor,
                "isArray",
                self.native_method("Array.isArray"),
            );
            Self::define_non_enumerable(
                &array_ctor,
                "from",
                self.native_method("Array.from"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "map",
                self.native_method("Array.prototype.map"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "filter",
                self.native_method("Array.prototype.filter"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "forEach",
                self.native_method("Array.prototype.forEach"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "join",
                self.native_method("Array.prototype.join"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "indexOf",
                self.native_method("Array.prototype.indexOf"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "includes",
                self.native_method("Array.prototype.includes"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "push",
                self.native_method("Array.prototype.push"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "slice",
                self.native_method("Array.prototype.slice"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "pop",
                self.native_method("Array.prototype.pop"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "shift",
                self.native_method("Array.prototype.shift"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "splice",
                self.native_method("Array.prototype.splice"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "unshift",
                self.native_method("Array.prototype.unshift"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "sort",
                self.native_method("Array.prototype.sort"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "reverse",
                self.native_method("Array.prototype.reverse"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "concat",
                self.native_method("Array.prototype.concat"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "reduce",
                self.native_method("Array.prototype.reduce"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "reduceRight",
                self.native_method("Array.prototype.reduceRight"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "some",
                self.native_method("Array.prototype.some"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "every",
                self.native_method("Array.prototype.every"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "find",
                self.native_method("Array.prototype.find"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "findIndex",
                self.native_method("Array.prototype.findIndex"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "fill",
                self.native_method("Array.prototype.fill"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "flat",
                self.native_method("Array.prototype.flat"),
            );
            Self::define_non_enumerable(
                &self.array_proto,
                "lastIndexOf",
                self.native_method("Array.prototype.lastIndexOf"),
            );
        }
        if let Some(Value::Object(string_ctor)) = self.env.borrow().get("String") {
            Self::define_non_enumerable(
                &string_ctor,
                "prototype",
                Value::Object(self.string_proto.clone()),
            );
            Self::define_non_enumerable(
                &string_ctor,
                "fromCharCode",
                self.native_method("String.fromCharCode"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "slice",
                self.native_method("String.prototype.slice"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "substring",
                self.native_method("String.prototype.substring"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "indexOf",
                self.native_method("String.prototype.indexOf"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "lastIndexOf",
                self.native_method("String.prototype.lastIndexOf"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "charAt",
                self.native_method("String.prototype.charAt"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "charCodeAt",
                self.native_method("String.prototype.charCodeAt"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "trim",
                self.native_method("String.prototype.trim"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "trimStart",
                self.native_method("String.prototype.trimStart"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "trimEnd",
                self.native_method("String.prototype.trimEnd"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "toLowerCase",
                self.native_method("String.prototype.toLowerCase"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "toUpperCase",
                self.native_method("String.prototype.toUpperCase"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "concat",
                self.native_method("String.prototype.concat"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "replace",
                self.native_method("String.prototype.replace"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "split",
                self.native_method("String.prototype.split"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "startsWith",
                self.native_method("String.prototype.startsWith"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "endsWith",
                self.native_method("String.prototype.endsWith"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "includes",
                self.native_method("String.prototype.includes"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "repeat",
                self.native_method("String.prototype.repeat"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "padStart",
                self.native_method("String.prototype.padStart"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "padEnd",
                self.native_method("String.prototype.padEnd"),
            );
        }
        if let Some(Value::Object(number_ctor)) = self.env.borrow().get("Number") {
            Self::define_non_enumerable(
                &number_ctor,
                "prototype",
                Value::Object(self.number_proto.clone()),
            );
        }
        if let Some(Value::Object(boolean_ctor)) = self.env.borrow().get("Boolean") {
            Self::define_non_enumerable(
                &boolean_ctor,
                "prototype",
                Value::Object(self.boolean_proto.clone()),
            );
        }
        for name in [
            "Error",
            "TypeError",
            "SyntaxError",
            "ReferenceError",
            "RangeError",
        ] {
            if let Some(Value::Object(ctor)) = self.env.borrow().get(name) {
                Self::define_non_enumerable(
                    &ctor,
                    "prototype",
                    Value::Object(self.error_proto.clone()),
                );
            }
        }
        for name in ["Number.isNaN", "Number.isFinite", "Number.parseInt", "Number.parseFloat", "Number.isInteger"] {
            self.define_native(name);
        }
        if let Some(Value::Object(number_ctor)) = self.env.borrow().get("Number") {
            Self::define_non_enumerable(&number_ctor, "MAX_VALUE", Value::Number(f64::MAX));
            Self::define_non_enumerable(&number_ctor, "MIN_VALUE", Value::Number(f64::MIN_POSITIVE));
            Self::define_non_enumerable(&number_ctor, "NaN", Value::Number(f64::NAN));
            Self::define_non_enumerable(&number_ctor, "NEGATIVE_INFINITY", Value::Number(f64::NEG_INFINITY));
            Self::define_non_enumerable(&number_ctor, "POSITIVE_INFINITY", Value::Number(f64::INFINITY));
            Self::define_non_enumerable(&number_ctor, "EPSILON", Value::Number(f64::EPSILON));
            Self::define_non_enumerable(&number_ctor, "MAX_SAFE_INTEGER", Value::Number(9007199254740991.0));
            Self::define_non_enumerable(&number_ctor, "MIN_SAFE_INTEGER", Value::Number(-9007199254740991.0));
            Self::define_non_enumerable(&number_ctor, "toFixed", self.native_method("Number.prototype.toFixed"));
            Self::define_non_enumerable(&number_ctor, "toString", self.native_method("Number.prototype.toString"));
        }
        Self::define_non_enumerable(&self.boolean_proto, "toString", self.native_method("Boolean.prototype.toString"));
        Self::define_non_enumerable(&self.boolean_proto, "valueOf", self.native_method("Boolean.prototype.valueOf"));
        Self::define_non_enumerable(&self.error_proto, "toString", self.native_method("Error.prototype.toString"));
        self.define_native("Symbol");
        Self::define_non_enumerable(&self.array_proto, "entries", self.native_method("Array.prototype.entries"));
        Self::define_non_enumerable(&self.array_proto, "keys", self.native_method("Array.prototype.keys"));
        Self::define_non_enumerable(&self.array_proto, "values", self.native_method("Array.prototype.values"));
    }

    fn define_non_enumerable(object: &ObjectRef, property: &str, value: Value) {
        let mut object = object.borrow_mut();
        object.props.insert(property.to_string(), value);
        object.non_enumerable_props.insert(property.to_string());
    }

    fn define_native(&mut self, name: &'static str) {
        let value = self.native_method(name);
        self.define_global(name, value, false);
    }

    fn native_method(&self, name: &'static str) -> Value {
        let obj = Object::with_internal(Internal::Native(name));
        obj.borrow_mut().proto = Some(self.function_proto.clone());
        Self::define_non_enumerable(&obj, "prototype", Value::Object(Object::plain()));
        Value::Object(obj)
    }

    fn define_global(&mut self, name: &str, value: Value, mutable: bool) {
        self.env
            .borrow_mut()
            .define(name.to_string(), value.clone(), mutable);
        self.global
            .borrow_mut()
            .props
            .insert(name.to_string(), value);
    }

    fn same_value_zero(left: &Value, right: &Value) -> bool {
        match (left, right) {
            (Value::Number(a), Value::Number(b)) => a == b || (a.is_nan() && b.is_nan()),
            _ => left == right,
        }
    }

    fn array_slice_bound(value: f64, len: isize) -> isize {
        if value.is_nan() {
            return 0;
        }
        let index = if value < 0.0 {
            len + value as isize
        } else {
            value as isize
        };
        index.clamp(0, len)
    }

    pub fn run(&mut self, program: &Program) -> JsResult<Value> {
        self.detect_strict_mode(&program.statements);
        match self.eval_statements(&program.statements)? {
            Flow::Value(v) | Flow::Return(v) => Ok(v),
            Flow::Throw(v) => Err(JsError::runtime(v.to_string())),
            Flow::Break => Err(JsError::syntax_error("break used outside loop")),
            Flow::Continue => Err(JsError::syntax_error("continue used outside loop")),
        }
    }

    fn detect_strict_mode(&mut self, statements: &[Stmt]) {
        if let Some(Stmt::Expr(Expr::String(s))) = statements.first() {
            if s == "use strict" {
                self.strict = true;
            }
        }
    }

    pub fn take_output(&mut self) -> Vec<String> {
        std::mem::take(&mut self.output)
    }

    pub fn list_variables(&self) -> Vec<(String, &'static str)> {
        let env = self.env.borrow();
        let mut names: Vec<(String, &'static str)> = env
            .values
            .iter()
            .map(|(k, v)| (k.clone(), if v.mutable { "let" } else { "const" }))
            .collect();
        names.sort_by(|a, b| a.0.cmp(&b.0));
        names
    }

    pub fn take_output_with_truncation(self) -> (Vec<String>, bool) {
        (self.output, self.output_truncated)
    }

    fn eval_statements(&mut self, statements: &[Stmt]) -> JsResult<Flow> {
        let mut last = Value::Undefined;
        for stmt in statements {
            match self.eval_stmt(stmt)? {
                Flow::Value(v) => last = v,
                flow @ (Flow::Return(_) | Flow::Throw(_) | Flow::Break | Flow::Continue) => {
                    return Ok(flow);
                }
            }
        }
        Ok(Flow::Value(last))
    }

    fn eval_stmt(&mut self, stmt: &Stmt) -> JsResult<Flow> {
        self.step()?;
        match stmt {
            Stmt::VarDecl {
                name,
                value,
                mutable,
            } => {
                let value = self.eval_expr(value)?;
                self.env
                    .borrow_mut()
                    .define(name.clone(), value.clone(), *mutable);
                if self.env.borrow().parent.is_none() {
                    self.global.borrow_mut().props.insert(name.clone(), value);
                }
                Ok(Flow::Value(Value::Undefined))
            }
            Stmt::VarDecls {
                declarations,
                mutable,
            } => {
                for (name, expr) in declarations {
                    let value = self.eval_expr(expr)?;
                    self.env
                        .borrow_mut()
                        .define(name.clone(), value.clone(), *mutable);
                    if self.env.borrow().parent.is_none() {
                        self.global.borrow_mut().props.insert(name.clone(), value);
                    }
                }
                Ok(Flow::Value(Value::Undefined))
            }
            Stmt::FunctionDecl { name, params, body } => {
                let value = self.make_function(params.clone(), body.clone());
                self.env
                    .borrow_mut()
                    .define(name.clone(), value.clone(), false);
                if self.env.borrow().parent.is_none() {
                    self.global.borrow_mut().props.insert(name.clone(), value);
                }
                Ok(Flow::Value(Value::Undefined))
            }
            Stmt::Return(value) => Ok(Flow::Return(
                value
                    .as_ref()
                    .map(|v| self.eval_expr(v))
                    .transpose()?
                    .unwrap_or(Value::Undefined),
            )),
            Stmt::Throw(value) => Ok(Flow::Throw(self.eval_expr(value)?)),
            Stmt::Try {
                block,
                catch_param,
                catch_block,
                finally_block,
            } => {
                let mut result = match self.with_child(block)? {
                    Flow::Throw(v) => {
                        if let (Some(param), Some(catch)) = (catch_param, catch_block) {
                            let previous = self.env.clone();
                            self.env = Env::child(previous.clone());
                            self.env.borrow_mut().define(param.clone(), v, true);
                            let r = self.eval_statements(catch);
                            self.env = previous;
                            r?
                        } else {
                            Flow::Throw(v)
                        }
                    }
                    other => other,
                };
                if let Some(finally) = finally_block {
                    let finally_result = self.with_child(finally)?;
                    if !matches!(finally_result, Flow::Value(_)) {
                        result = finally_result;
                    }
                }
                Ok(result)
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                if self.eval_expr(condition)?.is_truthy() {
                    self.with_child(then_branch)
                } else {
                    self.with_child(else_branch)
                }
            }
            Stmt::While { condition, body } => {
                let mut last = Value::Undefined;
                while self.eval_expr(condition)?.is_truthy() {
                    match self.with_child(body)? {
                        Flow::Value(v) => last = v,
                        Flow::Break => break,
                        Flow::Continue => continue,
                        r @ (Flow::Return(_) | Flow::Throw(_)) => return Ok(r),
                    }
                }
                Ok(Flow::Value(last))
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    match self.eval_stmt(init)? {
                        Flow::Value(_) => {}
                        r @ (Flow::Return(_) | Flow::Throw(_) | Flow::Break | Flow::Continue) => {
                            return Ok(r);
                        }
                    }
                }
                let mut last = Value::Undefined;
                loop {
                    self.step()?;
                    if let Some(condition) = condition
                        && !self.eval_expr(condition)?.is_truthy()
                    {
                        break;
                    }
                    match self.with_child(body)? {
                        Flow::Value(v) => last = v,
                        Flow::Break => break,
                        Flow::Continue => {}
                        r @ (Flow::Return(_) | Flow::Throw(_)) => return Ok(r),
                    }
                    if let Some(update) = update {
                        self.eval_expr(update)?;
                    }
                }
                Ok(Flow::Value(last))
            }
            Stmt::ForIn {
                left,
                right,
                body,
            } => {
                let obj = self.eval_expr(right)?;
                let keys = if let Value::Object(ref o) = obj {
                    let mut keys = Vec::new();
                    let mut current = Some(o.clone());
                    while let Some(c) = current {
                        let c = c.borrow();
                        for key in c.props.keys() {
                            if !c.non_enumerable_props.contains(key) && !keys.contains(key) {
                                keys.push(key.clone());
                            }
                        }
                        if let Internal::Array(items) = &c.internal {
                            for (i, item) in items.iter().enumerate() {
                                if item.is_some() {
                                    let s = i.to_string();
                                    if !keys.contains(&s) { keys.push(s); }
                                }
                            }
                        }
                        current = c.proto.clone();
                    }
                    keys
                } else {
                    Vec::new()
                };
                let mut last = Value::Undefined;
                for key in keys {
                    self.assign_target(left, Value::String(key))?;
                    match self.with_child(body)? {
                        Flow::Value(v) => last = v,
                        Flow::Break => break,
                        Flow::Continue => continue,
                        r @ (Flow::Return(_) | Flow::Throw(_)) => return Ok(r),
                    }
                }
                Ok(Flow::Value(last))
            }
            Stmt::Switch {
                discriminant,
                cases,
                default,
            } => {
                let discriminant = self.eval_expr(discriminant)?;
                let mut matched = false;
                let mut last = Value::Undefined;
                for (test, body) in cases {
                    if !matched {
                        matched = self.eval_expr(test)? == discriminant;
                    }
                    if matched {
                        match self.with_child(body)? {
                            Flow::Value(v) => last = v,
                            Flow::Break => return Ok(Flow::Value(last)),
                            r @ (Flow::Return(_) | Flow::Throw(_) | Flow::Continue) => {
                                return Ok(r);
                            }
                        }
                    }
                }
                if !matched {
                    match self.with_child(default)? {
                        Flow::Value(v) => last = v,
                        Flow::Break => return Ok(Flow::Value(last)),
                        r @ (Flow::Return(_) | Flow::Throw(_) | Flow::Continue) => return Ok(r),
                    }
                }
                Ok(Flow::Value(last))
            }
            Stmt::Break => Ok(Flow::Break),
            Stmt::Continue => Ok(Flow::Continue),
            Stmt::Block(stmts) => self.with_child(stmts),
            Stmt::Expr(expr) => Ok(Flow::Value(self.eval_expr(expr)?)),
        }
    }

    fn with_child(&mut self, statements: &[Stmt]) -> JsResult<Flow> {
        let previous = self.env.clone();
        self.env = Env::child(previous.clone());
        let result = self.eval_statements(statements);
        self.env = previous;
        result
    }

    fn make_function(&mut self, params: Vec<String>, body: Vec<Stmt>) -> Value {
        let obj = Object::with_internal(Internal::Function { params, body });
        obj.borrow_mut().proto = Some(self.function_proto.clone());
        let proto = Object::plain();
        proto.borrow_mut().proto = Some(self.object_proto.clone());
        Self::define_non_enumerable(&proto, "constructor", Value::Object(obj.clone()));
        Self::define_non_enumerable(&obj, "prototype", Value::Object(proto));
        let value = Value::Object(obj.clone());
        self.remember_closure(&value);
        value
    }

    fn remember_closure(&mut self, function: &Value) {
        if let Value::Object(obj) = function {
            self.closures
                .insert(Rc::as_ptr(obj) as usize, self.env.clone());
        }
    }

    fn eval_expr(&mut self, expr: &Expr) -> JsResult<Value> {
        self.step()?;
        match expr {
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::String(s) => Ok(Value::String(s.clone())),
            Expr::Bool(v) => Ok(Value::Bool(*v)),
            Expr::Null => Ok(Value::Null),
            Expr::Undefined => Ok(Value::Undefined),
            Expr::This => Ok(self
                .env
                .borrow()
                .get("this")
                .unwrap_or(Value::Object(self.global.clone()))),
            Expr::Identifier(name) => self
                .env
                .borrow()
                .get(name)
                .ok_or_else(|| JsError::reference_error(format!("{name} is not defined"))),
            Expr::Array(items) => {
                let values = items
                    .iter()
                    .map(|e| self.eval_expr(e))
                    .collect::<JsResult<Vec<_>>>()?;
                let obj =
                    Object::with_internal(Internal::Array(values.into_iter().map(Some).collect()));
                obj.borrow_mut().proto = Some(self.array_proto.clone());
                Ok(Value::Object(obj))
            }
            Expr::Object(props) => {
                let obj = Object::plain();
                obj.borrow_mut().proto = Some(self.object_proto.clone());
                for (k, e) in props {
                    let v = self.eval_expr(e)?;
                    obj.borrow_mut().props.insert(k.clone(), v);
                }
                Ok(Value::Object(obj))
            }
            Expr::Function { params, body } => Ok(self.make_function(params.clone(), body.clone())),
            Expr::Assign { target, value } => {
                let value = self.eval_expr(value)?;
                self.assign_target(target, value.clone())?;
                Ok(value)
            }
            Expr::CompoundAssign { target, op, value } => {
                let left = self.get_target(target)?;
                let right = self.eval_expr(value)?;
                let value = self.binary(left, *op, right)?;
                self.assign_target(target, value.clone())?;
                Ok(value)
            }
            Expr::Update {
                target,
                delta,
                prefix,
            } => {
                let old = self.get_target(target)?;
                let new_value = Value::Number(old.to_number() + delta);
                self.assign_target(target, new_value.clone())?;
                if *prefix { Ok(new_value) } else { Ok(old) }
            }
            Expr::Unary { op, expr } => {
                let value = self.eval_expr(expr)?;
                match op {
                    UnaryOp::Not => Ok(Value::Bool(!value.is_truthy())),
                    UnaryOp::Negate => Ok(Value::Number(-value.to_number())),
                    UnaryOp::Delete => {
                        match expr.as_ref() {
                            Expr::Member { object, property } => {
                                let obj = self.eval_expr(object)?;
                                if let Value::Object(ref o) = obj {
                                    let existed = o.borrow_mut().props.remove(property);
                                    return Ok(Value::Bool(existed.is_some()));
                                }
                                Ok(Value::Bool(true))
                            }
                            Expr::Index { object, index } => {
                                let obj = self.eval_expr(object)?;
                                let key = self.eval_expr(index)?.to_string();
                                if let Value::Object(ref o) = obj {
                                    let existed = o.borrow_mut().props.remove(&key);
                                    return Ok(Value::Bool(existed.is_some()));
                                }
                                Ok(Value::Bool(true))
                            }
                            _ => Ok(Value::Bool(true)),
                        }
                    }
                    UnaryOp::Void => Ok(Value::Undefined),
                    UnaryOp::BitwiseNot => Ok(Value::Number(
                        !(value.to_number() as i32) as f64,
                    )),
                }
            }
            Expr::Typeof(expr) => match expr.as_ref() {
                Expr::Identifier(name) => Ok(Value::String(
                    self.env
                        .borrow()
                        .get(name)
                        .map(|v| v.type_name().to_string())
                        .unwrap_or_else(|| "undefined".into()),
                )),
                _ => Ok(Value::String(self.eval_expr(expr)?.type_name().to_string())),
            },
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                if self.eval_expr(condition)?.is_truthy() {
                    self.eval_expr(then_expr)
                } else {
                    self.eval_expr(else_expr)
                }
            }
            Expr::Binary {
                left,
                op: BinaryOp::And,
                right,
            } => {
                let left = self.eval_expr(left)?;
                if !left.is_truthy() {
                    Ok(left)
                } else {
                    self.eval_expr(right)
                }
            }
            Expr::Binary {
                left,
                op: BinaryOp::Or,
                right,
            } => {
                let left = self.eval_expr(left)?;
                if left.is_truthy() {
                    Ok(left)
                } else {
                    self.eval_expr(right)
                }
            }
            Expr::Binary { left, op, right } => {
                let l = self.eval_expr(left)?;
                let r = self.eval_expr(right)?;
                self.binary(l, *op, r)
            }
            Expr::Call { callee, args } => {
                let (callee_value, this_value) = self.eval_callee(callee)?;
                let args = args
                    .iter()
                    .map(|a| self.eval_expr(a))
                    .collect::<JsResult<Vec<_>>>()?;
                self.call(callee_value, args, this_value, false)
            }
            Expr::New { callee, args } => {
                let callee = self.eval_expr(callee)?;
                let args = args
                    .iter()
                    .map(|a| self.eval_expr(a))
                    .collect::<JsResult<Vec<_>>>()?;
                self.call(callee, args, Value::Undefined, true)
            }
            Expr::Member { .. } | Expr::Index { .. } => self.get_target(expr),
        }
    }

    fn eval_callee(&mut self, expr: &Expr) -> JsResult<(Value, Value)> {
        match expr {
            Expr::Member { object, property } => {
                let object_value = self.eval_expr(object)?;
                let method = self.get_property_on_value(&object_value, property);
                Ok((method, object_value))
            }
            Expr::Index { object, index } => {
                let object_value = self.eval_expr(object)?;
                let key = self.eval_expr(index)?.to_string();
                let method = self.get_property_on_value(&object_value, &key);
                Ok((method, object_value))
            }
            _ => {
                let this = if self.strict {
                    Value::Undefined
                } else {
                    Value::Object(self.global.clone())
                };
                Ok((self.eval_expr(expr)?, this))
            }
        }
    }

    fn get_ref(&mut self, target: &Expr) -> JsResult<RefTarget> {
        match target {
            Expr::Member { object, property } => {
                let object = self.eval_expr(object)?;
                if let Value::Object(o) = object {
                    Ok(RefTarget {
                        object: o,
                        property: property.clone(),
                    })
                } else {
                    Err(JsError::type_error("member access on non-object"))
                }
            }
            Expr::Index { object, index } => {
                let object = self.eval_expr(object)?;
                let property = self.eval_expr(index)?.to_string();
                if let Value::Object(o) = object {
                    Ok(RefTarget {
                        object: o,
                        property,
                    })
                } else {
                    Err(JsError::type_error("index access on non-object"))
                }
            }
            _ => Err(JsError::type_error("target is not a reference")),
        }
    }

    fn get_target(&mut self, target: &Expr) -> JsResult<Value> {
        match target {
            Expr::Identifier(name) => self
                .env
                .borrow()
                .get(name)
                .ok_or_else(|| JsError::reference_error(format!("{name} is not defined"))),
            Expr::Member { object, property } => {
                let obj = self.eval_expr(object)?;
                Ok(self.get_property_on_value(&obj, property))
            }
            Expr::Index { object, index } => {
                let obj = self.eval_expr(object)?;
                let key = self.eval_expr(index)?.to_string();
                Ok(self.get_property_on_value(&obj, &key))
            }
            _ => self.eval_expr(target),
        }
    }

    fn assign_target(&mut self, target: &Expr, value: Value) -> JsResult<()> {
        match target {
            Expr::Identifier(name) => self.env.borrow_mut().assign(name, value),
            Expr::Member { .. } | Expr::Index { .. } => {
                let r = self.get_ref(target)?;
                self.set_property(&r.object, &r.property, value);
                Ok(())
            }
            _ => Err(JsError::type_error("target is not assignable")),
        }
    }

    fn get_property(&self, object: &ObjectRef, property: &str) -> Value {
        if let Internal::Array(items) = &object.borrow().internal {
            if property == "length" {
                return Value::Number(items.len() as f64);
            }
            if let Ok(i) = property.parse::<usize>() {
                return items.get(i).cloned().flatten().unwrap_or(Value::Undefined);
            }
        }
        Object::lookup(object, property).unwrap_or(Value::Undefined)
    }

    fn get_property_on_value(&self, value: &Value, property: &str) -> Value {
        match value {
            Value::String(s) => {
                if property == "length" {
                    return Value::Number(s.len() as f64);
                }
                Object::lookup(&self.string_proto, property).unwrap_or(Value::Undefined)
            }
            Value::Number(_) => {
                Object::lookup(&self.number_proto, property).unwrap_or(Value::Undefined)
            }
            Value::Bool(_) => {
                Object::lookup(&self.boolean_proto, property).unwrap_or(Value::Undefined)
            }
            Value::Object(o) => self.get_property(o, property),
            _ => Value::Undefined,
        }
    }

    fn set_property(&self, object: &ObjectRef, property: &str, value: Value) {
        if let Internal::Array(items) = &mut object.borrow_mut().internal {
            if property == "length" {
                let n = value.to_number();
                if n.is_finite() && n >= 0.0 && n.fract() == 0.0 {
                    items.resize(n as usize, None);
                }
                return;
            }
            if let Ok(i) = property.parse::<usize>() {
                if i >= items.len() {
                    items.resize(i + 1, None);
                }
                items[i] = Some(value);
                return;
            }
        }
        object
            .borrow_mut()
            .props
            .insert(property.to_string(), value);
    }

    fn is_own_enumerable_property(&self, object: &ObjectRef, property: &str) -> bool {
        let object = object.borrow();
        if let Internal::Array(items) = &object.internal {
            if property == "length" {
                return false;
            }
            if let Ok(index) = property.parse::<usize>() {
                return items.get(index).is_some_and(Option::is_some);
            }
        }
        object.props.contains_key(property) && !object.non_enumerable_props.contains(property)
    }

    fn data_descriptor(
        &self,
        value: Value,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) -> Value {
        let descriptor = Object::plain();
        descriptor.borrow_mut().proto = Some(self.object_proto.clone());
        descriptor.borrow_mut().props.insert("value".into(), value);
        descriptor
            .borrow_mut()
            .props
            .insert("writable".into(), Value::Bool(writable));
        descriptor
            .borrow_mut()
            .props
            .insert("enumerable".into(), Value::Bool(enumerable));
        descriptor
            .borrow_mut()
            .props
            .insert("configurable".into(), Value::Bool(configurable));
        Value::Object(descriptor)
    }

    fn get_own_property_descriptor(&self, object: &ObjectRef, property: &str) -> Value {
        let object = object.borrow();
        if let Internal::Array(items) = &object.internal {
            if property == "length" {
                return self.data_descriptor(Value::Number(items.len() as f64), true, false, false);
            }
            if let Ok(index) = property.parse::<usize>() {
                return items
                    .get(index)
                    .cloned()
                    .flatten()
                    .map(|value| self.data_descriptor(value, true, true, true))
                    .unwrap_or(Value::Undefined);
            }
        }
        object
            .props
            .get(property)
            .cloned()
            .map(|value| {
                let enumerable = !object.non_enumerable_props.contains(property);
                self.data_descriptor(value, true, enumerable, true)
            })
            .unwrap_or(Value::Undefined)
    }

    fn binary(&self, left: Value, op: BinaryOp, right: Value) -> JsResult<Value> {
        match op {
            BinaryOp::Add => match (left, right) {
                (Value::String(a), b) => Ok(Value::String(a + &b.to_string())),
                (a, Value::String(b)) => Ok(Value::String(a.to_string() + &b)),
                (a, b) => Ok(Value::Number(a.to_number() + b.to_number())),
            },
            BinaryOp::Subtract => Ok(Value::Number(left.to_number() - right.to_number())),
            BinaryOp::Multiply => Ok(Value::Number(left.to_number() * right.to_number())),
            BinaryOp::Divide => Ok(Value::Number(left.to_number() / right.to_number())),
            BinaryOp::Remainder => Ok(Value::Number(left.to_number() % right.to_number())),
            BinaryOp::Equal => Ok(Value::Bool(left.abstract_eq(&right))),
            BinaryOp::NotEqual => Ok(Value::Bool(!left.abstract_eq(&right))),
            BinaryOp::StrictEqual => Ok(Value::Bool(left == right)),
            BinaryOp::StrictNotEqual => Ok(Value::Bool(left != right)),
            BinaryOp::Less => Ok(Value::Bool(left.to_number() < right.to_number())),
            BinaryOp::LessEqual => Ok(Value::Bool(left.to_number() <= right.to_number())),
            BinaryOp::Greater => Ok(Value::Bool(left.to_number() > right.to_number())),
            BinaryOp::GreaterEqual => Ok(Value::Bool(left.to_number() >= right.to_number())),
            BinaryOp::Instanceof => Ok(Value::Bool(self.instanceof(left, right))),
            BinaryOp::In => {
                let Value::Object(ref o) = right else {
                    return Err(JsError::type_error("right-hand side of in must be an object"));
                };
                let key = left.to_string();
                let has = Object::lookup(o, &key).is_some();
                Ok(Value::Bool(has))
            }
            BinaryOp::BitwiseAnd => {
                Ok(Value::Number((left.to_number() as i32 & right.to_number() as i32) as f64))
            }
            BinaryOp::BitwiseOr => {
                Ok(Value::Number((left.to_number() as i32 | right.to_number() as i32) as f64))
            }
            BinaryOp::BitwiseXor => {
                Ok(Value::Number((left.to_number() as i32 ^ right.to_number() as i32) as f64))
            }
            BinaryOp::LeftShift => {
                Ok(Value::Number(((left.to_number() as i32) << (right.to_number() as u32)) as f64))
            }
            BinaryOp::RightShift => {
                Ok(Value::Number(((left.to_number() as i32) >> (right.to_number() as u32)) as f64))
            }
            BinaryOp::UnsignedRightShift => {
                Ok(Value::Number(((left.to_number() as u32) >> (right.to_number() as u32)) as f64))
            }
            BinaryOp::And | BinaryOp::Or => unreachable!("short-circuited before binary eval"),
        }
    }

    fn instanceof(&self, left: Value, right: Value) -> bool {
        let Value::Object(obj) = left else {
            return false;
        };
        let Value::Object(ctor) = right else {
            return false;
        };
        let Some(Value::Object(proto)) = ctor.borrow().props.get("prototype").cloned() else {
            return false;
        };
        let mut current = obj.borrow().proto.clone();
        while let Some(p) = current {
            if Rc::ptr_eq(&p, &proto) {
                return true;
            }
            current = p.borrow().proto.clone();
        }
        false
    }

    fn call(
        &mut self,
        callee: Value,
        args: Vec<Value>,
        this_value: Value,
        construct: bool,
    ) -> JsResult<Value> {
        let Value::Object(func) = callee else {
            return Err(JsError::type_error(format!(
                "{} is not a function",
                callee.type_name()
            )));
        };
        let internal = func.borrow().internal.clone();
        match internal {
            Internal::Native(name) => self.call_native(
                name,
                args,
                this_value,
                if construct { Some(func) } else { None },
            ),
            Internal::Function { params, body } => {
                self.enter_call()?;
                let previous = self.env.clone();
                let closure_env = self
                    .closures
                    .get(&(Rc::as_ptr(&func) as usize))
                    .cloned()
                    .unwrap_or_else(|| previous.clone());
                self.env = Env::child(closure_env);
                let this_obj = if construct {
                    let obj = Object::plain();
                    if let Some(Value::Object(proto)) =
                        func.borrow().props.get("prototype").cloned()
                    {
                        obj.borrow_mut().proto = Some(proto);
                    } else {
                        obj.borrow_mut().proto = Some(self.object_proto.clone());
                    }
                    Value::Object(obj)
                } else {
                    this_value
                };
                self.env
                    .borrow_mut()
                    .define("this".into(), this_obj.clone(), true);
                for (index, name) in params.into_iter().enumerate() {
                    let value = args.get(index).cloned().unwrap_or(Value::Undefined);
                    self.env.borrow_mut().define(name, value, true);
                }
                let result = self.eval_statements(&body);
                self.env = previous;
                self.leave_call();
                match result? {
                    Flow::Value(v) | Flow::Return(v) => {
                        if construct {
                            if matches!(v, Value::Object(_)) {
                                Ok(v)
                            } else {
                                Ok(this_obj)
                            }
                        } else {
                            Ok(v)
                        }
                    }
                    Flow::Throw(v) => Ok(v).and_then(|v| Err(JsError::runtime(v.to_string()))),
                    Flow::Break => Err(JsError::syntax_error("break used outside loop")),
                    Flow::Continue => Err(JsError::syntax_error("continue used outside loop")),
                }
            }
            Internal::Bound {
                target,
                bound_this,
                bound_args,
            } => {
                let mut combined = bound_args;
                combined.extend(args);
                self.call(Value::Object(target), combined, bound_this, construct)
            }
            _ => Err(JsError::type_error("object is not a function")),
        }
    }

    fn call_native(
        &mut self,
        name: &'static str,
        args: Vec<Value>,
        this_value: Value,
        construct: Option<ObjectRef>,
    ) -> JsResult<Value> {
        match name {
            "print" => {
                self.push_output(
                    args.first()
                        .cloned()
                        .unwrap_or(Value::Undefined)
                        .to_string(),
                );
                Ok(Value::Undefined)
            }
            "Error" | "TypeError" | "SyntaxError" | "ReferenceError" | "RangeError" => {
                let obj = Object::plain();
                obj.borrow_mut().proto = Some(self.error_proto.clone());
                obj.borrow_mut()
                    .props
                    .insert("name".into(), Value::String(name.into()));
                obj.borrow_mut().props.insert(
                    "message".into(),
                    args.first()
                        .cloned()
                        .unwrap_or(Value::String(String::new())),
                );
                Ok(Value::Object(obj))
            }
            "Object" => {
                if let Some(value @ Value::Object(_)) = args.first().cloned() {
                    Ok(value)
                } else {
                    let obj = Object::plain();
                    obj.borrow_mut().proto = Some(self.object_proto.clone());
                    Ok(Value::Object(obj))
                }
            }
            "Object.keys" => {
                let Some(Value::Object(object)) = args.first().cloned() else {
                    return Err(JsError::type_error("Object.keys expects object"));
                };
                let object = object.borrow();
                let mut keys = Vec::new();
                if let Internal::Array(items) = &object.internal {
                    keys.extend(
                        items
                            .iter()
                            .enumerate()
                            .filter(|(_, value)| value.is_some())
                            .map(|(index, _)| index.to_string()),
                    );
                }
                let mut named_keys = object.props.keys().cloned().collect::<Vec<_>>();
                named_keys.sort();
                for key in named_keys {
                    if !keys.contains(&key) && !object.non_enumerable_props.contains(&key) {
                        keys.push(key);
                    }
                }
                let values: Vec<Value> = keys.into_iter().map(Value::String).collect();
                let obj =
                    Object::with_internal(Internal::Array(values.into_iter().map(Some).collect()));
                obj.borrow_mut().proto = Some(self.array_proto.clone());
                Ok(Value::Object(obj))
            }
            "Object.defineProperty" => {
                let Some(Value::Object(target)) = args.first().cloned() else {
                    return Err(JsError::type_error("Object.defineProperty expects object"));
                };
                let key = args.get(1).cloned().unwrap_or(Value::Undefined).to_string();
                let Some(Value::Object(descriptor)) = args.get(2).cloned() else {
                    return Err(JsError::type_error(
                        "Object.defineProperty expects descriptor object",
                    ));
                };
                let value = descriptor.borrow().props.get("value").cloned();
                let writable = descriptor
                    .borrow()
                    .props
                    .get("writable")
                    .map_or(true, |v| v.is_truthy());
                let enumerable = descriptor
                    .borrow()
                    .props
                    .get("enumerable")
                    .map_or(false, |v| v.is_truthy());
                if let Some(value) = value {
                    self.set_property(&target, &key, value);
                    if !writable {
                        let mut target = target.borrow_mut();
                        target.non_enumerable_props.insert("__writable_".to_string() + &key);
                    }
                    if !enumerable {
                        let mut target = target.borrow_mut();
                        target.non_enumerable_props.insert(key.clone());
                    }
                }
                Ok(Value::Object(target))
            }
            "Object.getOwnPropertyDescriptor" => {
                let Some(Value::Object(object)) = args.first().cloned() else {
                    return Err(JsError::type_error(
                        "Object.getOwnPropertyDescriptor expects object",
                    ));
                };
                let property = args.get(1).cloned().unwrap_or(Value::Undefined).to_string();
                Ok(self.get_own_property_descriptor(&object, &property))
            }
"Object.create" => {
                let proto = args.first().cloned().unwrap_or(Value::Undefined);
                let obj = Object::plain();
                match proto {
                    Value::Object(proto) => {
                        obj.borrow_mut().proto = Some(proto);
                        Ok(Value::Object(obj))
                    }
                    Value::Null => Ok(Value::Object(obj)),
                    _ => Err(JsError::type_error("Object.create expects object or null")),
                }
            }
            "Function.prototype.call" => {
                let this_arg = args.first().cloned().unwrap_or(Value::Undefined);
                let call_args: Vec<Value> = args.iter().skip(1).cloned().collect();
                self.call(this_value, call_args, this_arg, false)
            }
            "Function.prototype.apply" => {
                let this_arg = args.first().cloned().unwrap_or(Value::Undefined);
                let apply_args = if let Some(Value::Object(o)) = args.get(1)
                    && let Internal::Array(items) = &o.borrow().internal
                {
                    items.iter().filter_map(|v| v.clone()).collect()
                } else if args.len() > 1 {
                    Vec::new()
                } else {
                    Vec::new()
                };
                self.call(this_value, apply_args, this_arg, false)
            }
            "Function.prototype.bind" => {
                let Value::Object(target) = this_value else {
                    return Err(JsError::type_error("bind requires a function"));
                };
                let bound_this = args.first().cloned().unwrap_or(Value::Undefined);
                let bound_args: Vec<Value> = args.iter().skip(1).cloned().collect();
                let obj = Object::with_internal(Internal::Bound {
                    target: target.clone(),
                    bound_this,
                    bound_args,
                });
                obj.borrow_mut().proto = Some(self.function_proto.clone());
                let proto = Object::plain();
                proto.borrow_mut().proto = Some(self.object_proto.clone());
                Self::define_non_enumerable(&proto, "constructor", Value::Object(obj.clone()));
                Self::define_non_enumerable(&obj, "prototype", Value::Object(proto));
                Ok(Value::Object(obj))
            }
            "Object.getPrototypeOf" => {
                let Some(Value::Object(object)) = args.first().cloned() else {
                    return Err(JsError::type_error("Object.getPrototypeOf expects object"));
                };
                Ok(object
                    .borrow()
                    .proto
                    .clone()
                    .map(Value::Object)
                    .unwrap_or(Value::Null))
            }
            "Array" => {
                let obj =
                    Object::with_internal(Internal::Array(args.into_iter().map(Some).collect()));
                obj.borrow_mut().proto = Some(self.array_proto.clone());
                Ok(Value::Object(obj))
            }
            "Array.isArray" => {
                let is_array = args.first().is_some_and(
                    |value| matches!(value, Value::Object(object) if matches!(object.borrow().internal, Internal::Array(_))),
                );
                Ok(Value::Bool(is_array))
            }
            "String" => Ok(Value::String(
                args.first()
                    .cloned()
                    .unwrap_or(Value::Undefined)
                    .to_string(),
            )),
            "Number" => Ok(Value::Number(
                args.first()
                    .cloned()
                    .unwrap_or(Value::Undefined)
                    .to_number(),
            )),
            "Boolean" => Ok(Value::Bool(
                args.first()
                    .cloned()
                    .unwrap_or(Value::Undefined)
                    .is_truthy(),
            )),
            "isNaN" => Ok(Value::Bool(
                args.first()
                    .cloned()
                    .unwrap_or(Value::Undefined)
                    .to_number()
                    .is_nan(),
            )),
            "isFinite" => Ok(Value::Bool(
                args.first()
                    .map(|v| v.to_number().is_finite())
                    .unwrap_or(false),
            )),
            "parseInt" => {
                let s = args.first().cloned().unwrap_or(Value::Undefined).to_string();
                let radix = args.get(1).map(|v| v.to_number() as u32).unwrap_or(10);
                let s = s.trim();
                if s.is_empty() { return Ok(Value::Number(f64::NAN)); }
                match i64::from_str_radix(s, radix) {
                    Ok(n) => Ok(Value::Number(n as f64)),
                    Err(_) => Ok(Value::Number(f64::NAN)),
                }
            },
            "parseFloat" => {
                let s = args.first().cloned().unwrap_or(Value::Undefined).to_string();
                let s = s.trim();
                if s.is_empty() { return Ok(Value::Number(f64::NAN)); }
                Ok(Value::Number(s.parse::<f64>().unwrap_or(f64::NAN)))
            },
            "eval" => {
                let code = args.first().cloned().unwrap_or(Value::Undefined).to_string();
                let tokens = crate::lexer::lex(&code)
                    .map_err(|e| JsError::syntax_error(e.to_string()))?;
                let program = crate::parser::parse(tokens)
                    .map_err(|e| JsError::syntax_error(e.to_string()))?;
                match self.eval_statements(&program.statements)? {
                    Flow::Value(v) | Flow::Return(v) => Ok(v),
                    Flow::Throw(v) => Err(JsError::runtime(v.to_string())),
                    Flow::Break => Err(JsError::syntax_error("break used outside loop")),
                    Flow::Continue => Err(JsError::syntax_error("continue used outside loop")),
                }
            },
            "Function" => {
                let body = args.last().cloned().unwrap_or(Value::Undefined).to_string();
                let params: Vec<String> = args.iter().take(args.len().saturating_sub(1))
                    .map(|v| v.to_string())
                    .collect();
                let code = if params.is_empty() {
                    body
                } else {
                    format!("function({}){{ {} }}", params.join(","), body)
                };
                let tokens = crate::lexer::lex(&code)
                    .map_err(|e| JsError::syntax_error(e.to_string()))?;
                let program = crate::parser::parse(tokens)
                    .map_err(|e| JsError::syntax_error(e.to_string()))?;
                for stmt in &program.statements {
                    if let Stmt::FunctionDecl { params, body, .. } = stmt {
                        return Ok(self.make_function(params.clone(), body.clone()));
                    }
                }
                Ok(Value::Undefined)
            },
            "JSON.stringify" => {
                let value = args.first().cloned().unwrap_or(Value::Undefined);
                Ok(Value::String(self.json_stringify(&value)?))
            }
            "Object.values" => {
                let Some(Value::Object(object)) = args.first().cloned() else {
                    return Err(JsError::type_error("Object.values expects object"));
                };
                let object = object.borrow();
                let mut values = Vec::new();
                let is_array = matches!(&object.internal, Internal::Array(_));
                if let Internal::Array(items) = &object.internal {
                    values.extend(items.iter().filter_map(Clone::clone));
                }
                let mut named_keys = object.props.keys().cloned().collect::<Vec<_>>();
                named_keys.sort();
                for key in named_keys {
                    if (!is_array || key.parse::<usize>().is_err())
                        && !object.non_enumerable_props.contains(&key)
                        && let Some(value) = object.props.get(&key)
                    {
                        values.push(value.clone());
                    }
                }
                let obj =
                    Object::with_internal(Internal::Array(values.into_iter().map(Some).collect()));
                obj.borrow_mut().proto = Some(self.array_proto.clone());
                Ok(Value::Object(obj))
            }
            "Object.prototype.toString" => Ok(Value::String(format!(
                "[object {}]",
                match this_value.type_name() {
                    "undefined" => "Undefined",
                    "boolean" => "Boolean",
                    "number" => "Number",
                    "string" => "String",
                    _ => "Object",
                }
            ))),
            "Object.prototype.hasOwnProperty" => {
                let key = args
                    .first()
                    .cloned()
                    .unwrap_or(Value::Undefined)
                    .to_string();
                let Value::Object(object) = this_value else {
                    return Ok(Value::Bool(false));
                };
                let object = object.borrow();
                let has_array_index = if let Internal::Array(items) = &object.internal {
                    key.parse::<usize>()
                        .is_ok_and(|index| items.get(index).is_some_and(Option::is_some))
                        || key == "length"
                } else {
                    false
                };
                Ok(Value::Bool(
                    has_array_index || object.props.contains_key(&key),
                ))
            }
            "Object.prototype.propertyIsEnumerable" => {
                let key = args
                    .first()
                    .cloned()
                    .unwrap_or(Value::Undefined)
                    .to_string();
                let Value::Object(object) = this_value else {
                    return Ok(Value::Bool(false));
                };
                Ok(Value::Bool(self.is_own_enumerable_property(&object, &key)))
            }
            "Array.prototype.join" => {
                let sep = args
                    .first()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| ",".into());
                if let Value::Object(o) = this_value
                    && let Internal::Array(items) = &o.borrow().internal
                {
                    return Ok(Value::String(
                        items
                            .iter()
                            .map(|v| v.clone().unwrap_or(Value::Undefined).to_string())
                            .collect::<Vec<_>>()
                            .join(&sep),
                    ));
                }
                Ok(Value::String(String::new()))
            }
            "Array.prototype.indexOf" => {
                let needle = args.first().cloned().unwrap_or(Value::Undefined);
                let from_index = args
                    .get(1)
                    .cloned()
                    .unwrap_or(Value::Number(0.0))
                    .to_number();
                if let Value::Object(o) = this_value
                    && let Internal::Array(items) = &o.borrow().internal
                {
                    let len = items.len() as isize;
                    let mut start = if from_index.is_nan() {
                        0
                    } else if from_index < 0.0 {
                        len + from_index as isize
                    } else {
                        from_index as isize
                    };
                    if start < 0 {
                        start = 0;
                    }
                    for (index, value) in items.iter().enumerate().skip(start as usize) {
                        if value.as_ref().is_some_and(|value| *value == needle) {
                            return Ok(Value::Number(index as f64));
                        }
                    }
                }
                Ok(Value::Number(-1.0))
            }
            "Array.prototype.includes" => {
                let needle = args.first().cloned().unwrap_or(Value::Undefined);
                let from_index = args
                    .get(1)
                    .cloned()
                    .unwrap_or(Value::Number(0.0))
                    .to_number();
                if let Value::Object(o) = this_value
                    && let Internal::Array(items) = &o.borrow().internal
                {
                    let len = items.len() as isize;
                    let mut start = if from_index.is_nan() {
                        0
                    } else if from_index < 0.0 {
                        len + from_index as isize
                    } else {
                        from_index as isize
                    };
                    if start < 0 {
                        start = 0;
                    }
                    return Ok(Value::Bool(items.iter().skip(start as usize).any(
                        |value| {
                            let value = value.clone().unwrap_or(Value::Undefined);
                            Self::same_value_zero(&value, &needle)
                        },
                    )));
                }
                Ok(Value::Bool(false))
            }
            "Array.prototype.push" => {
                if let Value::Object(o) = this_value
                    && let Internal::Array(items) = &mut o.borrow_mut().internal
                {
                    items.extend(args.into_iter().map(Some));
                    return Ok(Value::Number(items.len() as f64));
                }
                Ok(Value::Number(0.0))
            }
            "Array.prototype.slice" => {
                if let Value::Object(o) = this_value
                    && let Internal::Array(items) = &o.borrow().internal
                {
                    let len = items.len() as isize;
                    let start = args
                        .first()
                        .cloned()
                        .unwrap_or(Value::Number(0.0))
                        .to_number();
                    let end = match args.get(1) {
                        Some(Value::Undefined) | None => len as f64,
                        Some(value) => value.to_number(),
                    };
                    let start = Self::array_slice_bound(start, len);
                    let end = Self::array_slice_bound(end, len);
                    let selected = if end <= start {
                        Vec::new()
                    } else {
                        items[start as usize..end as usize].to_vec()
                    };
                    let obj = Object::with_internal(Internal::Array(selected));
                    obj.borrow_mut().proto = Some(self.array_proto.clone());
                    return Ok(Value::Object(obj));
                }
                let obj = Object::with_internal(Internal::Array(Vec::new()));
                obj.borrow_mut().proto = Some(self.array_proto.clone());
                Ok(Value::Object(obj))
            }
            "Array.prototype.pop" => {
                if let Value::Object(o) = this_value
                    && let Internal::Array(items) = &mut o.borrow_mut().internal
                {
                    return Ok(items.pop().flatten().unwrap_or(Value::Undefined));
                }
                Ok(Value::Undefined)
            }
            "Array.prototype.shift" => {
                if let Value::Object(o) = this_value
                    && let Internal::Array(items) = &mut o.borrow_mut().internal
                {
                    if items.is_empty() {
                        return Ok(Value::Undefined);
                    }
                    return Ok(items.remove(0).unwrap_or(Value::Undefined));
                }
                Ok(Value::Undefined)
            }
            "Array.prototype.map" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                if let Value::Object(o) = this_value {
                    let source_array = Value::Object(o.clone());
                    let items = if let Internal::Array(items) = &o.borrow().internal {
                        items.clone()
                    } else {
                        Vec::new()
                    };
                    let mapped = items
                        .into_iter()
                        .enumerate()
                        .map(|(index, value)| {
                            self.call(
                                callback.clone(),
                                vec![
                                    value.unwrap_or(Value::Undefined),
                                    Value::Number(index as f64),
                                    source_array.clone(),
                                ],
                                Value::Object(self.global.clone()),
                                false,
                            )
                        })
                        .collect::<JsResult<Vec<_>>>()?;
                    let obj = Object::with_internal(Internal::Array(
                        mapped.into_iter().map(Some).collect(),
                    ));
                    obj.borrow_mut().proto = Some(self.array_proto.clone());
                    return Ok(Value::Object(obj));
                }
                let obj = Object::with_internal(Internal::Array(Vec::new()));
                obj.borrow_mut().proto = Some(self.array_proto.clone());
                Ok(Value::Object(obj))
            }
            "Array.prototype.filter" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                if let Value::Object(o) = this_value {
                    let source_array = Value::Object(o.clone());
                    let items = if let Internal::Array(items) = &o.borrow().internal {
                        items.clone()
                    } else {
                        Vec::new()
                    };
                    let mut filtered = Vec::new();
                    for (index, value) in items.into_iter().enumerate() {
                        let callback_value = value.clone().unwrap_or(Value::Undefined);
                        if self
                            .call(
                                callback.clone(),
                                vec![
                                    callback_value,
                                    Value::Number(index as f64),
                                    source_array.clone(),
                                ],
                                Value::Object(self.global.clone()),
                                false,
                            )?
                            .is_truthy()
                        {
                            filtered.push(value);
                        }
                    }
                    let obj = Object::with_internal(Internal::Array(filtered));
                    obj.borrow_mut().proto = Some(self.array_proto.clone());
                    return Ok(Value::Object(obj));
                }
                let obj = Object::with_internal(Internal::Array(Vec::new()));
                obj.borrow_mut().proto = Some(self.array_proto.clone());
                Ok(Value::Object(obj))
            }
            "Array.prototype.forEach" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                if let Value::Object(o) = this_value {
                    let source_array = Value::Object(o.clone());
                    let initial_len = if let Internal::Array(items) = &o.borrow().internal {
                        items.len()
                    } else {
                        0
                    };
                    for index in 0..initial_len {
                        let value = {
                            let object = o.borrow();
                            if let Internal::Array(items) = &object.internal {
                                items.get(index).cloned().flatten()
                            } else {
                                None
                            }
                        };
                        let Some(value) = value else {
                            continue;
                        };
                        self.call(
                            callback.clone(),
                            vec![value, Value::Number(index as f64), source_array.clone()],
                            Value::Object(self.global.clone()),
                            false,
                        )?;
                    }
                }
                Ok(Value::Undefined)
            }
            "Array.prototype.splice" => {
                if let Value::Object(o) = this_value
                    && let Internal::Array(items) = &mut o.borrow_mut().internal
                {
                    let len = items.len();
                    let start = args
                        .first()
                        .map(|v| v.to_number())
                        .unwrap_or(0.0);
                    let start = if start < 0.0 {
                        ((len as f64) + start).max(0.0) as usize
                    } else {
                        (start as usize).min(len)
                    };
                    let delete_count = args
                        .get(1)
                        .map(|v| v.to_number())
                        .unwrap_or(len as f64);
                    let delete_count = if delete_count.is_nan() || delete_count < 0.0 {
                        0
                    } else {
                        (delete_count as usize).min(len - start)
                    };
                    let removed: Vec<Option<Value>> = items.splice(start..start + delete_count, args.iter().skip(2).cloned().map(Some)).collect();
                    let removed_values: Vec<Option<Value>> = removed.into_iter().collect();
                    let obj = Object::with_internal(Internal::Array(removed_values));
                    obj.borrow_mut().proto = Some(self.array_proto.clone());
                    return Ok(Value::Object(obj));
                }
                let obj = Object::with_internal(Internal::Array(Vec::new()));
                obj.borrow_mut().proto = Some(self.array_proto.clone());
                Ok(Value::Object(obj))
            }
            "Array.prototype.unshift" => {
                if let Value::Object(o) = this_value
                    && let Internal::Array(items) = &mut o.borrow_mut().internal
                {
                    let head: Vec<Option<Value>> = args.into_iter().map(Some).collect();
                    items.splice(0..0, head);
                    return Ok(Value::Number(items.len() as f64));
                }
                Ok(Value::Number(0.0))
            }
            "Array.prototype.sort" => {
                let compare_fn = args.first().cloned();
                let items = if let Value::Object(ref o) = this_value
                    && let Internal::Array(items) = &o.borrow().internal
                {
                    items.clone()
                } else {
                    return Ok(this_value);
                };
                let mut items = items;
                if let Some(ref cmp) = compare_fn {
                    let mut scratch = Vec::new();
                    for item in &items {
                        scratch.push(item.clone());
                    }
                    scratch.sort_by(|a, b| {
                        let a = a.clone().unwrap_or(Value::Undefined);
                        let b = b.clone().unwrap_or(Value::Undefined);
                        if let Ok(result) = self.call(cmp.clone(), vec![a, b], Value::Object(self.global.clone()), false) {
                            let n = result.to_number();
                            if n < 0.0 { return std::cmp::Ordering::Less; }
                            if n > 0.0 { return std::cmp::Ordering::Greater; }
                        }
                        std::cmp::Ordering::Equal
                    });
                    items = scratch;
                } else {
                    items.sort_by(|a, b| {
                        let a = a.clone().unwrap_or(Value::Undefined);
                        let b = b.clone().unwrap_or(Value::Undefined);
                        a.to_string().cmp(&b.to_string())
                    });
                }
                if let Value::Object(ref o) = this_value
                    && let Internal::Array(arr) = &mut o.borrow_mut().internal
                {
                    *arr = items;
                    return Ok(Value::Object(o.clone()));
                }
                Ok(this_value)
            }
            "Array.prototype.reverse" => {
                if let Value::Object(ref o) = this_value
                    && let Internal::Array(items) = &mut o.borrow_mut().internal
                {
                    items.reverse();
                    return Ok(Value::Object(o.clone()));
                }
                Ok(this_value)
            }
            "Array.prototype.concat" => {
                let mut result: Vec<Option<Value>> = Vec::new();
                if let Value::Object(ref o) = this_value
                    && let Internal::Array(items) = &o.borrow().internal
                {
                    result.extend(items.iter().cloned());
                } else {
                    result.push(Some(this_value));
                }
                for arg in &args {
                    if let Value::Object(o) = arg
                        && let Internal::Array(items) = &o.borrow().internal
                    {
                        result.extend(items.iter().cloned());
                    } else {
                        result.push(Some(arg.clone()));
                    }
                }
                let obj = Object::with_internal(Internal::Array(result));
                obj.borrow_mut().proto = Some(self.array_proto.clone());
                Ok(Value::Object(obj))
            }
            "Array.prototype.reduce" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                let has_initial = args.len() > 1;
                let initial = args.get(1).cloned().unwrap_or(Value::Undefined);
                if let Value::Object(o) = this_value {
                    let source_array = Value::Object(o.clone());
                    let items = if let Internal::Array(items) = &o.borrow().internal {
                        items.clone()
                    } else {
                        Vec::new()
                    };
                    let mut accumulator = if has_initial {
                        initial
                    } else {
                        items.first().cloned().flatten().unwrap_or(Value::Undefined)
                    };
                    let start = if has_initial { 0 } else { 1 };
                    for (index, value) in items.iter().enumerate().skip(start) {
                        accumulator = self.call(
                            callback.clone(),
                            vec![accumulator, value.clone().unwrap_or(Value::Undefined), Value::Number(index as f64), source_array.clone()],
                            Value::Object(self.global.clone()),
                            false,
                        )?;
                    }
                    return Ok(accumulator);
                }
                Err(JsError::type_error("reduce requires array"))
            }
            "Array.prototype.reduceRight" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                let has_initial = args.len() > 1;
                let initial = args.get(1).cloned().unwrap_or(Value::Undefined);
                if let Value::Object(o) = this_value {
                    let source_array = Value::Object(o.clone());
                    let items = if let Internal::Array(items) = &o.borrow().internal {
                        items.clone()
                    } else {
                        Vec::new()
                    };
                    let len = items.len();
                    let mut accumulator = if has_initial {
                        initial
                    } else if len > 0 {
                        items.last().cloned().flatten().unwrap_or(Value::Undefined)
                    } else {
                        return Err(JsError::type_error("reduceRight requires array with at least one element"));
                    };
                    let start = if has_initial { len as isize - 1 } else { len as isize - 2 };
                    for i in (0..=start).rev() {
                        if i < 0 { break; }
                        let i = i as usize;
                        accumulator = self.call(
                            callback.clone(),
                            vec![accumulator, items[i].clone().unwrap_or(Value::Undefined), Value::Number(i as f64), source_array.clone()],
                            Value::Object(self.global.clone()),
                            false,
                        )?;
                    }
                    return Ok(accumulator);
                }
                Err(JsError::type_error("reduceRight requires array"))
            }
            "Array.prototype.some" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                if let Value::Object(o) = this_value {
                    let source_array = Value::Object(o.clone());
                    let items = if let Internal::Array(items) = &o.borrow().internal {
                        items.clone()
                    } else {
                        Vec::new()
                    };
                    for (index, value) in items.iter().enumerate() {
                        let val = value.clone().unwrap_or(Value::Undefined);
                        if self.call(
                            callback.clone(),
                            vec![val, Value::Number(index as f64), source_array.clone()],
                            Value::Object(self.global.clone()),
                            false,
                        )?.is_truthy() {
                            return Ok(Value::Bool(true));
                        }
                    }
                    return Ok(Value::Bool(false));
                }
                Ok(Value::Bool(false))
            }
            "Array.prototype.every" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                if let Value::Object(o) = this_value {
                    let source_array = Value::Object(o.clone());
                    let items = if let Internal::Array(items) = &o.borrow().internal {
                        items.clone()
                    } else {
                        Vec::new()
                    };
                    for (index, value) in items.iter().enumerate() {
                        let val = value.clone().unwrap_or(Value::Undefined);
                        if !self.call(
                            callback.clone(),
                            vec![val, Value::Number(index as f64), source_array.clone()],
                            Value::Object(self.global.clone()),
                            false,
                        )?.is_truthy() {
                            return Ok(Value::Bool(false));
                        }
                    }
                    return Ok(Value::Bool(true));
                }
                Ok(Value::Bool(true))
            }
            "Array.prototype.find" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                if let Value::Object(o) = this_value {
                    let source_array = Value::Object(o.clone());
                    let items = if let Internal::Array(items) = &o.borrow().internal {
                        items.clone()
                    } else {
                        Vec::new()
                    };
                    for (index, value) in items.iter().enumerate() {
                        let val = value.clone().unwrap_or(Value::Undefined);
                        if self.call(
                            callback.clone(),
                            vec![val.clone(), Value::Number(index as f64), source_array.clone()],
                            Value::Object(self.global.clone()),
                            false,
                        )?.is_truthy() {
                            return Ok(val);
                        }
                    }
                    return Ok(Value::Undefined);
                }
                Ok(Value::Undefined)
            }
            "Array.prototype.findIndex" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                if let Value::Object(o) = this_value {
                    let source_array = Value::Object(o.clone());
                    let items = if let Internal::Array(items) = &o.borrow().internal {
                        items.clone()
                    } else {
                        Vec::new()
                    };
                    for (index, value) in items.iter().enumerate() {
                        let val = value.clone().unwrap_or(Value::Undefined);
                        if self.call(
                            callback.clone(),
                            vec![val, Value::Number(index as f64), source_array.clone()],
                            Value::Object(self.global.clone()),
                            false,
                        )?.is_truthy() {
                            return Ok(Value::Number(index as f64));
                        }
                    }
                    return Ok(Value::Number(-1.0));
                }
                Ok(Value::Number(-1.0))
            }
            "Array.prototype.fill" => {
                let fill_value = args.first().cloned().unwrap_or(Value::Undefined);
                if let Value::Object(ref o) = this_value
                    && let Internal::Array(items) = &mut o.borrow_mut().internal
                {
                    let len = items.len() as isize;
                    let start = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
                    let start = Self::array_slice_bound(start, len);
                    let end = args.get(2).map(|v| v.to_number()).unwrap_or(len as f64);
                    let end = Self::array_slice_bound(end, len);
                    for i in start..end {
                        if (i as usize) < items.len() {
                            items[i as usize] = Some(fill_value.clone());
                        }
                    }
                    return Ok(Value::Object(o.clone()));
                }
                Ok(this_value)
            }
            "Array.prototype.flat" => {
                let depth = args.first().map(|v| v.to_number()).unwrap_or(1.0);
                let src_items = if let Value::Object(ref o) = this_value
                    && let Internal::Array(items) = &o.borrow().internal
                {
                    items.clone()
                } else {
                    Vec::new()
                };
                fn flatten(items: &[Option<Value>], depth: f64) -> Vec<Option<Value>> {
                    let mut result = Vec::new();
                    for item in items {
                        if let Some(Value::Object(o)) = item
                            && let Internal::Array(inner) = &o.borrow().internal
                            && depth > 0.0
                        {
                            result.extend(flatten(inner, depth - 1.0));
                        } else {
                            result.push(item.clone());
                        }
                    }
                    result
                }
                let flattened = flatten(&src_items, depth);
                let obj = Object::with_internal(Internal::Array(flattened));
                obj.borrow_mut().proto = Some(self.array_proto.clone());
                Ok(Value::Object(obj))
            }
            "Array.prototype.lastIndexOf" => {
                let needle = args.first().cloned().unwrap_or(Value::Undefined);
                if let Value::Object(o) = this_value
                    && let Internal::Array(items) = &o.borrow().internal
                {
                    let len = items.len() as isize;
                    let from_index = args.get(1).map(|v| v.to_number()).unwrap_or(len as f64);
                    let mut start = if from_index.is_nan() {
                        len - 1
                    } else if from_index < 0.0 {
                        len + from_index as isize
                    } else {
                        (from_index as isize).min(len - 1)
                    };
                    if start < 0 { start = -1; }
                    for i in (0..=start).rev() {
                        if i < 0 { break; }
                        let i = i as usize;
                        if i < items.len() && items[i].as_ref().is_some_and(|v| *v == needle) {
                            return Ok(Value::Number(i as f64));
                        }
                    }
                }
                Ok(Value::Number(-1.0))
            }
            "Array.from" => {
                let callback = if args.len() > 1 {
                    args.get(1).cloned()
                } else {
                    None
                };
                let this_arg = args.get(2).cloned().unwrap_or(Value::Undefined);
                let mut result = Vec::new();
                let source = args.first().cloned().unwrap_or(Value::Undefined);
                if let Value::Object(o) = &source {
                    let length = if let Internal::Array(items) = &o.borrow().internal {
                        items.len()
                    } else {
                        o.borrow()
                            .props
                            .get("length")
                            .map(|v| v.to_number() as usize)
                            .unwrap_or(0)
                    };
                    for i in 0..length {
                        let val = if let Internal::Array(items) = &o.borrow().internal {
                            items.get(i).cloned().flatten().unwrap_or(Value::Undefined)
                        } else {
                            o.borrow()
                                .props
                                .get(&i.to_string())
                                .cloned()
                                .unwrap_or(Value::Undefined)
                        };
                        let val = if let Some(ref cb) = callback {
                            self.call(cb.clone(), vec![val, Value::Number(i as f64)], this_arg.clone(), false)?
                        } else {
                            val
                        };
                        result.push(Some(val));
                    }
                }
                let obj = Object::with_internal(Internal::Array(result));
                obj.borrow_mut().proto = Some(self.array_proto.clone());
                Ok(Value::Object(obj))
            }
            "String.prototype.slice" => {
                let s = self.this_str(&this_value);
                let len = s.len() as isize;
                let start = args.first().map(|v| v.to_number()).unwrap_or(0.0);
                let start = Self::array_slice_bound(start, len);
                let end = args.get(1).map(|v| v.to_number()).unwrap_or(len as f64);
                let end = Self::array_slice_bound(end, len);
                let result = if end > start {
                    s.chars().skip(start as usize).take((end - start) as usize).collect()
                } else {
                    String::new()
                };
                Ok(Value::String(result))
            }
            "String.prototype.substring" => {
                let s = self.this_str(&this_value);
                let len = s.len();
                let start = args.first().map(|v| v.to_number()).unwrap_or(0.0).clamp(0.0, len as f64) as usize;
                let end = args.get(1).map(|v| v.to_number()).unwrap_or(len as f64).clamp(0.0, len as f64) as usize;
                let (start, end) = (start.min(end), start.max(end));
                Ok(Value::String(s.chars().skip(start).take(end - start).collect()))
            }
            "String.prototype.indexOf" => {
                let s = self.this_str(&this_value);
                let needle = args.first().cloned().unwrap_or(Value::Undefined).to_string();
                let from = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).clamp(0.0, s.len() as f64) as usize;
                if let Some(pos) = s[from..].find(&needle) {
                    Ok(Value::Number((from + pos) as f64))
                } else {
                    Ok(Value::Number(-1.0))
                }
            }
            "String.prototype.lastIndexOf" => {
                let s = self.this_str(&this_value);
                let needle = args.first().cloned().unwrap_or(Value::Undefined).to_string();
                let from = args.get(1).map(|v| v.to_number()).unwrap_or(s.len() as f64);
                let from = if from < 0.0 { 0 } else { (from as usize).min(s.len()) };
                if let Some(pos) = s[..from].rfind(&needle) {
                    Ok(Value::Number(pos as f64))
                } else {
                    Ok(Value::Number(-1.0))
                }
            }
            "String.prototype.charAt" => {
                let s = self.this_str(&this_value);
                let pos = args.first().map(|v| v.to_number()).unwrap_or(0.0);
                if pos < 0.0 || pos >= s.len() as f64 {
                    Ok(Value::String(String::new()))
                } else {
                    Ok(Value::String(s.chars().nth(pos as usize).unwrap_or(' ').to_string()))
                }
            }
            "String.prototype.charCodeAt" => {
                let s = self.this_str(&this_value);
                let pos = args.first().map(|v| v.to_number()).unwrap_or(0.0);
                if pos < 0.0 || pos >= s.len() as f64 {
                    Ok(Value::Number(f64::NAN))
                } else {
                    Ok(Value::Number(s.chars().nth(pos as usize).map_or(f64::NAN, |c| c as u32 as f64)))
                }
            }
            "String.prototype.trim" => {
                Ok(Value::String(self.this_str(&this_value).trim().to_string()))
            }
            "String.prototype.trimStart" => {
                Ok(Value::String(self.this_str(&this_value).trim_start().to_string()))
            }
            "String.prototype.trimEnd" => {
                Ok(Value::String(self.this_str(&this_value).trim_end().to_string()))
            }
            "String.prototype.toLowerCase" => {
                Ok(Value::String(self.this_str(&this_value).to_lowercase()))
            }
            "String.prototype.toUpperCase" => {
                Ok(Value::String(self.this_str(&this_value).to_uppercase()))
            }
            "String.prototype.concat" => {
                let mut result = self.this_str(&this_value);
                for arg in &args {
                    result.push_str(&arg.to_string());
                }
                Ok(Value::String(result))
            }
            "String.prototype.replace" => {
                let s = self.this_str(&this_value);
                let needle = args.first().cloned().unwrap_or(Value::Undefined).to_string();
                let replacement = args.get(1).cloned().unwrap_or(Value::Undefined).to_string();
                if let Some(pos) = s.find(&needle) {
                    let mut result = s[..pos].to_string();
                    result.push_str(&replacement);
                    result.push_str(&s[pos + needle.len()..]);
                    Ok(Value::String(result))
                } else {
                    Ok(Value::String(s))
                }
            }
            "String.prototype.split" => {
                let s = self.this_str(&this_value);
                let sep = args.first().cloned().unwrap_or(Value::Undefined).to_string();
                let limit = args.get(1).map(|v| v.to_number() as usize);
                if sep.is_empty() {
                    let mut chars: Vec<Option<Value>> = s.chars().map(|c| Some(Value::String(c.to_string()))).collect();
                    if let Some(lim) = limit {
                        chars.truncate(lim);
                    }
                    let obj = Object::with_internal(Internal::Array(chars));
                    obj.borrow_mut().proto = Some(self.array_proto.clone());
                    return Ok(Value::Object(obj));
                }
                let parts: Vec<&str> = s.split(&sep).collect();
                let mut results: Vec<Option<Value>> = parts.iter().map(|p| Some(Value::String(p.to_string()))).collect();
                if let Some(lim) = limit {
                    results.truncate(lim);
                }
                let obj = Object::with_internal(Internal::Array(results));
                obj.borrow_mut().proto = Some(self.array_proto.clone());
                Ok(Value::Object(obj))
            }
            "String.prototype.startsWith" => {
                let s = self.this_str(&this_value);
                let needle = args.first().cloned().unwrap_or(Value::Undefined).to_string();
                let pos = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
                if pos > s.len() {
                    Ok(Value::Bool(false))
                } else {
                    Ok(Value::Bool(s[pos..].starts_with(&needle)))
                }
            }
            "String.prototype.endsWith" => {
                let s = self.this_str(&this_value);
                let needle = args.first().cloned().unwrap_or(Value::Undefined).to_string();
                let end = args.get(1).map(|v| v.to_number() as usize).unwrap_or(s.len());
                let end = end.min(s.len());
                if needle.len() > end {
                    Ok(Value::Bool(false))
                } else {
                    Ok(Value::Bool(s[..end].ends_with(&needle)))
                }
            }
            "String.prototype.includes" => {
                let s = self.this_str(&this_value);
                let needle = args.first().cloned().unwrap_or(Value::Undefined).to_string();
                let pos = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
                if pos > s.len() {
                    Ok(Value::Bool(false))
                } else {
                    Ok(Value::Bool(s[pos..].contains(&needle)))
                }
            }
            "String.prototype.repeat" => {
                let s = self.this_str(&this_value);
                let count = args.first().map(|v| v.to_number()).unwrap_or(0.0);
                if count < 0.0 || count.is_infinite() {
                    return Err(JsError::range_error("repeat count must be non-negative finite"));
                }
                let count = count as usize;
                Ok(Value::String(s.repeat(count)))
            }
            "String.prototype.padStart" => {
                let s = self.this_str(&this_value);
                let max_len = args.first().map(|v| v.to_number()).unwrap_or(0.0) as usize;
                let pad = args.get(1).cloned().unwrap_or(Value::String(" ".into())).to_string();
                if s.len() >= max_len {
                    Ok(Value::String(s))
                } else {
                    let need = max_len - s.len();
                    let pad_repeat = pad.repeat((need / pad.len()) + 1);
                    Ok(Value::String(format!("{}{}", &pad_repeat[..need], s)))
                }
            }
            "String.prototype.padEnd" => {
                let s = self.this_str(&this_value);
                let max_len = args.first().map(|v| v.to_number()).unwrap_or(0.0) as usize;
                let pad = args.get(1).cloned().unwrap_or(Value::String(" ".into())).to_string();
                if s.len() >= max_len {
                    Ok(Value::String(s))
                } else {
                    let need = max_len - s.len();
                    let pad_repeat = pad.repeat((need / pad.len()) + 1);
                    Ok(Value::String(format!("{}{}", s, &pad_repeat[..need])))
                }
            }
            "String.fromCharCode" => {
                let mut result = String::new();
                for arg in &args {
                    if let Ok(c) = u32::try_from(arg.to_number() as i64) {
                        if let Some(ch) = char::from_u32(c) {
                            result.push(ch);
                        }
                    }
                }
                Ok(Value::String(result))
            }
            "Math.abs" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().abs())),
            "Math.acos" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().acos())),
            "Math.acosh" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().acosh())),
            "Math.asin" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().asin())),
            "Math.asinh" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().asinh())),
            "Math.atan" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().atan())),
            "Math.atanh" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().atanh())),
            "Math.atan2" => Ok(Value::Number(
                args.first().unwrap_or(&Value::Number(0.0)).to_number().atan2(
                    args.get(1).unwrap_or(&Value::Number(0.0)).to_number(),
                ),
            )),
            "Math.cbrt" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().cbrt())),
            "Math.ceil" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().ceil())),
            "Math.clz32" => Ok(Value::Number(
                (args.first().unwrap_or(&Value::Number(0.0)).to_number() as u32).leading_zeros() as f64,
            )),
            "Math.cos" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().cos())),
            "Math.cosh" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().cosh())),
            "Math.exp" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().exp())),
            "Math.expm1" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().exp_m1())),
            "Math.floor" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().floor())),
            "Math.fround" => {
                let v = args.first().unwrap_or(&Value::Number(0.0)).to_number() as f32;
                Ok(Value::Number(v as f64))
            }
            "Math.hypot" => {
                let sum: f64 = args.iter().map(|v| {
                    let n = v.to_number();
                    n * n
                }).sum();
                Ok(Value::Number(sum.sqrt()))
            }
            "Math.imul" => Ok(Value::Number(
                ((args.first().unwrap_or(&Value::Number(0.0)).to_number() as i32)
                    .wrapping_mul(args.get(1).unwrap_or(&Value::Number(0.0)).to_number() as i32)) as f64,
            )),
            "Math.log" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().ln())),
            "Math.log10" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().log10())),
            "Math.log1p" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().ln_1p())),
            "Math.log2" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().log2())),
            "Math.max" => {
                let mut max = f64::NEG_INFINITY;
                for arg in &args {
                    let n = arg.to_number();
                    if n.is_nan() { return Ok(Value::Number(f64::NAN)); }
                    if n > max || (n == 0.0 && max == 0.0 && n.signum() > max.signum()) {
                        max = n;
                    }
                }
                Ok(Value::Number(if args.is_empty() { f64::NEG_INFINITY } else { max }))
            }
            "Math.min" => {
                let mut min = f64::INFINITY;
                for arg in &args {
                    let n = arg.to_number();
                    if n.is_nan() { return Ok(Value::Number(f64::NAN)); }
                    if n < min || (n == 0.0 && min == 0.0 && n.signum() < min.signum()) {
                        min = n;
                    }
                }
                Ok(Value::Number(if args.is_empty() { f64::INFINITY } else { min }))
            }
            "Math.pow" => Ok(Value::Number(
                args.first().unwrap_or(&Value::Number(0.0)).to_number()
                    .powf(args.get(1).unwrap_or(&Value::Number(0.0)).to_number()),
            )),
            "Math.random" => {
                let seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .subsec_nanos();
                let next = (seed.wrapping_mul(1103515245).wrapping_add(12345)) & 0x7fffffff;
                Ok(Value::Number(next as f64 / 0x80000000u64 as f64))
            }
            "Math.round" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().round())),
            "Math.sign" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().signum())),
            "Math.sin" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().sin())),
            "Math.sinh" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().sinh())),
            "Math.sqrt" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().sqrt())),
            "Math.tan" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().tan())),
            "Math.tanh" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().tanh())),
            "Math.trunc" => Ok(Value::Number(args.first().unwrap_or(&Value::Number(0.0)).to_number().trunc())),
            "Object.assign" => {
                let Some(Value::Object(target)) = args.first().cloned() else {
                    return Err(JsError::type_error("Object.assign expects object"));
                };
                for source in args.iter().skip(1) {
                    if let Value::Object(src) = source {
                        let src = src.borrow();
                        for key in src.props.keys() {
                            if !src.non_enumerable_props.contains(key) {
                                target.borrow_mut().props.insert(key.clone(), src.props[key].clone());
                            }
                        }
                    }
                }
                Ok(Value::Object(target))
            }
            "Object.entries" => {
                let Some(Value::Object(obj)) = args.first().cloned() else {
                    return Err(JsError::type_error("Object.entries expects object"));
                };
                let obj = obj.borrow();
                let mut result: Vec<Option<Value>> = Vec::new();
                if let Internal::Array(items) = &obj.internal {
                    for (i, item) in items.iter().enumerate() {
                        if item.is_some() {
                            let pair = vec![Some(Value::String(i.to_string())), item.clone()];
                            let arr = Object::with_internal(Internal::Array(pair));
                            arr.borrow_mut().proto = Some(self.array_proto.clone());
                            result.push(Some(Value::Object(arr)));
                        }
                    }
                }
                let mut keys: Vec<String> = obj.props.keys().cloned().collect();
                keys.sort();
                for key in keys {
                    if !obj.non_enumerable_props.contains(&key) {
                        let pair = vec![Some(Value::String(key.clone())), Some(obj.props[&key].clone())];
                        let arr = Object::with_internal(Internal::Array(pair));
                        arr.borrow_mut().proto = Some(self.array_proto.clone());
                        result.push(Some(Value::Object(arr)));
                    }
                }
                let arr = Object::with_internal(Internal::Array(result));
                arr.borrow_mut().proto = Some(self.array_proto.clone());
                Ok(Value::Object(arr))
            }
            "Object.is" => Ok(Value::Bool(same_value(
                &args.first().cloned().unwrap_or(Value::Undefined),
                &args.get(1).cloned().unwrap_or(Value::Undefined),
            ))),
            "Object.getOwnPropertyNames" => {
                let Some(Value::Object(obj)) = args.first().cloned() else {
                    return Err(JsError::type_error("Object.getOwnPropertyNames expects object"));
                };
                let obj = obj.borrow();
                let mut names: Vec<Option<Value>> = Vec::new();
                if let Internal::Array(items) = &obj.internal {
                    names.push(Some(Value::String("length".into())));
                    for (i, item) in items.iter().enumerate() {
                        if item.is_some() { names.push(Some(Value::String(i.to_string()))); }
                    }
                }
                for key in obj.props.keys() {
                    names.push(Some(Value::String(key.clone())));
                }
                let arr = Object::with_internal(Internal::Array(names));
                arr.borrow_mut().proto = Some(self.array_proto.clone());
                Ok(Value::Object(arr))
            }
            "Object.setPrototypeOf" => {
                let Some(Value::Object(target)) = args.first().cloned() else {
                    return Err(JsError::type_error("Object.setPrototypeOf expects object"));
                };
                match args.get(1).cloned().unwrap_or(Value::Undefined) {
                    Value::Object(p) => { target.borrow_mut().proto = Some(p); }
                    Value::Null => { target.borrow_mut().proto = None; }
                    _ => return Err(JsError::type_error("prototype must be object or null")),
                }
                Ok(Value::Object(target))
            }
            "Object.preventExtensions" => {
                let Some(Value::Object(target)) = args.first().cloned() else {
                    return Err(JsError::type_error("Object.preventExtensions expects object"));
                };
                target.borrow_mut().non_enumerable_props.insert("__extensible_false".into());
                Ok(Value::Object(target))
            }
            "Object.isExtensible" => Ok(Value::Bool(
                if let Some(Value::Object(o)) = args.first().cloned() {
                    !o.borrow().non_enumerable_props.contains("__extensible_false")
                } else { true }
            )),
            "Object.freeze" => {
                let Some(Value::Object(target)) = args.first().cloned() else {
                    return Err(JsError::type_error("Object.freeze expects object"));
                };
                target.borrow_mut().non_enumerable_props.insert("__frozen".into());
                Ok(Value::Object(target))
            }
            "Object.isFrozen" => Ok(Value::Bool(
                if let Some(Value::Object(o)) = args.first().cloned() {
                    o.borrow().non_enumerable_props.contains("__frozen")
                } else { true }
            )),
            "Object.seal" => {
                let Some(Value::Object(target)) = args.first().cloned() else {
                    return Err(JsError::type_error("Object.seal expects object"));
                };
                target.borrow_mut().non_enumerable_props.insert("__sealed".into());
                Ok(Value::Object(target))
            }
            "Object.isSealed" => Ok(Value::Bool(
                if let Some(Value::Object(o)) = args.first().cloned() {
                    o.borrow().non_enumerable_props.contains("__sealed")
                } else { true }
            )),
            "Number.isNaN" => Ok(Value::Bool(
                args.first().map(|v| v.to_number().is_nan()).unwrap_or(false),
            )),
            "Number.isFinite" => Ok(Value::Bool(
                args.first().map(|v| v.to_number().is_finite()).unwrap_or(false),
            )),
            "Number.parseInt" => {
                let s = args.first().cloned().unwrap_or(Value::Undefined).to_string();
                let radix = args.get(1).map(|v| v.to_number() as u32).unwrap_or(10);
                Ok(Value::Number(i64::from_str_radix(s.trim(), radix).unwrap_or(0) as f64))
            }
            "Number.parseFloat" => {
                let s = args.first().cloned().unwrap_or(Value::Undefined).to_string();
                Ok(Value::Number(s.trim().parse::<f64>().unwrap_or(f64::NAN)))
            }
            "Number.isInteger" => Ok(Value::Bool(
                args.first().map(|v| { let n = v.to_number(); n.is_finite() && n.fract() == 0.0 }).unwrap_or(false),
            )),
            "Boolean.prototype.toString" => Ok(Value::String(this_value.is_truthy().to_string())),
            "Boolean.prototype.valueOf" => Ok(Value::Bool(this_value.is_truthy())),
            "Error.prototype.toString" => {
                let name = if let Value::Object(ref o) = this_value {
                    o.borrow().props.get("name").cloned().unwrap_or(Value::String("Error".into())).to_string()
                } else { "Error".to_string() };
                let msg = if let Value::Object(ref o) = this_value {
                    o.borrow().props.get("message").cloned().unwrap_or(Value::String(String::new())).to_string()
                } else { String::new() };
                if msg.is_empty() { Ok(Value::String(name)) }
                else { Ok(Value::String(format!("{name}: {msg}"))) }
            }
            "Symbol" => {
                let desc = args.first().cloned().map(|v| v.to_string()).unwrap_or_default();
                let sym = Object::plain();
                sym.borrow_mut().props.insert("description".into(), Value::String(desc));
                Ok(Value::Object(sym))
            }
            "Array.prototype.entries" => {
                if let Value::Object(ref o) = this_value
                    && let Internal::Array(items) = &o.borrow().internal
                {
                    let mut arr = Vec::new();
                    for (i, v) in items.iter().enumerate() {
                        let pair = vec![Value::Number(i as f64), v.clone().unwrap_or(Value::Undefined)];
                        arr.push(Value::array(pair));
                    }
                    return Ok(Value::Object(make_iterator(arr, 0)));
                }
                Ok(Value::Object(make_empty_iterator()))
            }
            "Array.prototype.keys" => {
                if let Value::Object(ref o) = this_value
                    && let Internal::Array(items) = &o.borrow().internal
                {
                    let arr: Vec<Value> = (0..items.len()).map(|i| Value::Number(i as f64)).collect();
                    return Ok(Value::Object(make_iterator(arr, 0)));
                }
                Ok(Value::Object(make_empty_iterator()))
            }
            "Array.prototype.values" => {
                if let Value::Object(ref o) = this_value
                    && let Internal::Array(items) = &o.borrow().internal
                {
                    let arr: Vec<Value> = items.iter().filter_map(|v| v.clone()).collect();
                    return Ok(Value::Object(make_iterator(arr, 0)));
                }
                Ok(Value::Object(make_empty_iterator()))
            }
            _ => {
                if let Some(func) = construct {
                    let obj = Object::plain();
                    if let Some(Value::Object(proto)) =
                        func.borrow().props.get("prototype").cloned()
                    {
                        obj.borrow_mut().proto = Some(proto);
                    }
                    Ok(Value::Object(obj))
                } else {
                    Ok(Value::Undefined)
                }
            }
        }
    }

    fn step(&mut self) -> JsResult<()> {
        self.steps = self.steps.saturating_add(1);
        if self.step_limit.is_some_and(|limit| self.steps > limit) {
            return Err(JsError::runtime("execution step limit exceeded"));
        }
        Ok(())
    }

    fn enter_call(&mut self) -> JsResult<()> {
        self.call_depth = self.call_depth.saturating_add(1);
        if self
            .max_call_depth
            .is_some_and(|limit| self.call_depth > limit)
        {
            self.leave_call();
            return Err(JsError::runtime("call depth limit exceeded"));
        }
        Ok(())
    }

    fn leave_call(&mut self) {
        self.call_depth = self.call_depth.saturating_sub(1);
    }

    fn push_output(&mut self, text: String) {
        if self
            .output_limit
            .is_some_and(|limit| self.output.len() >= limit)
        {
            self.output_truncated = true;
            return;
        }
        self.output.push(text);
    }

    fn json_stringify(&self, value: &Value) -> Result<String, JsError> {
        fn stringify_impl(value: &Value) -> Result<String, JsError> {
            match value {
                Value::Null => Ok("null".into()),
                Value::Bool(true) => Ok("true".into()),
                Value::Bool(false) => Ok("false".into()),
                Value::Number(n) => {
                    if n.is_nan() || n.is_infinite() { Ok("null".into()) }
                    else if n.fract() == 0.0 && n.is_finite() {
                        Ok(format!("{}", *n as i64))
                    } else {
                        Ok(n.to_string())
                    }
                }
                Value::String(s) => {
                    let escaped: String = s.chars().map(|c| match c {
                        '"' => "\\\"".to_string(),
                        '\\' => "\\\\".to_string(),
                        '\n' => "\\n".to_string(),
                        '\r' => "\\r".to_string(),
                        '\t' => "\\t".to_string(),
                        c if c.is_control() => format!("\\u{:04x}", c as u32),
                        c => c.to_string(),
                    }).collect();
                    Ok(format!("\"{}\"", escaped))
                }
                Value::Undefined => Ok("undefined".into()),
                Value::Object(o) => {
                    let obj = o.borrow();
                    match &obj.internal {
                        Internal::Array(items) => {
                            let parts: Vec<String> = items.iter()
                                .map(|v| match v {
                                    Some(v) => stringify_impl(v).unwrap_or_else(|_| "null".into()),
                                    None => "null".into(),
                                })
                                .collect();
                            Ok(format!("[{}]", parts.join(",")))
                        }
                        _ => {
                            let mut parts: Vec<String> = Vec::new();
                            let mut keys: Vec<String> = obj.props.keys().cloned().collect();
                            keys.sort();
                            for key in keys {
                                if obj.non_enumerable_props.contains(&key) { continue; }
                                let val = &obj.props[&key];
                                if matches!(val, Value::Undefined) || val.is_callable() { continue; }
                                if let Ok(s) = stringify_impl(val) {
                                    parts.push(format!("\"{}\":{}", key, s));
                                }
                            }
                            Ok(format!("{{{}}}", parts.join(",")))
                        }
                    }
                }
            }
        }
        let result = stringify_impl(value)?;
        if result == "undefined" { Ok(String::new()) }
        else { Ok(result) }
    }

    fn this_str(&self, this_value: &Value) -> String {
        match this_value {
            Value::String(s) => s.clone(),
            Value::Object(o) => {
                let obj = o.borrow();
                if let Some(Value::String(s)) = obj.props.get("value") {
                    s.clone()
                } else {
                    this_value.to_string()
                }
            }
            _ => this_value.to_string(),
        }
    }

    fn this_number(&self, this_value: &Value) -> f64 {
        match this_value {
            Value::Number(n) => *n,
            Value::Object(o) => {
                o.borrow().props.get("value").map(|v| v.to_number()).unwrap_or(f64::NAN)
            }
            _ => this_value.to_number(),
        }
    }
}

fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            if a.is_nan() && b.is_nan() { return true; }
            if *a == 0.0 && *b == 0.0 { return a.signum() == b.signum(); }
            a == b
        }
        _ => a == b,
    }
}

fn format_number_radix(n: f64, radix: u32) -> String {
    if n.is_nan() { return "NaN".into(); }
    if n.is_infinite() { return if n > 0.0 { "Infinity".into() } else { "-Infinity".into() }; }
    let abs = n.abs();
    let int_part = abs.trunc() as u64;
    let frac_part = abs.fract();
    let int_str = if radix == 16 {
        format!("{:x}", int_part)
    } else if radix == 8 {
        format!("{:o}", int_part)
    } else if radix == 2 {
        format!("{:b}", int_part)
    } else {
        int_part.to_string()
    };
    let sign = if n < 0.0 { "-" } else { "" };
    if frac_part == 0.0 {
        format!("{sign}{int_str}")
    } else {
        format!("{sign}{int_str}.{}", frac_part.to_string().trim_start_matches("0."))
    }
}

fn make_iterator(values: Vec<Value>, idx: usize) -> ObjectRef {
    let iter = Object::plain();
    let values = Rc::new(RefCell::new((values, idx)));
    let next_fn = Object::with_internal(Internal::Native("Iterator.next"));
    iter.borrow_mut().props.insert("next".into(), Value::Object(next_fn));
    // Store the values in a hidden property
    iter.borrow_mut().props.insert("__iter_values".into(), Value::Object(
        Object::with_internal(Internal::Array(values.borrow().0.clone().into_iter().map(Some).collect()))
    ));
    iter.borrow_mut().props.insert("__iter_idx".into(), Value::Number(idx as f64));
    iter
}

fn make_empty_iterator() -> ObjectRef {
    make_iterator(Vec::new(), 0)
}

fn this_str(this_value: &Value) -> String {
    match this_value {
        Value::String(s) => s.clone(),
        Value::Object(o) => {
            let obj = o.borrow();
            if let Some(Value::String(s)) = obj.props.get("value") {
                s.clone()
            } else {
                this_value.to_string()
            }
        }
        _ => this_value.to_string(),
    }
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::Value;
    use crate::run_source;

    #[test]
    fn if_else_works() {
        assert_eq!(
            run_source("let x=1; if (x) { 2; } else { 3; }").unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn closure_captures_lexical_environment() {
        let src = "function make(){ let x=1; return function(){ x=x+1; return x; }; } let f=make(); f()+f();";
        assert_eq!(run_source(src).unwrap(), Value::Number(5.0));
    }

    #[test]
    fn missing_function_args_become_undefined() {
        assert_eq!(
            run_source("function f(a,b){ return typeof b; } f(1);").unwrap(),
            Value::String("undefined".into())
        );
    }

    #[test]
    fn break_exits_nearest_loop() {
        assert_eq!(
            run_source(
                r#"
                    let i = 0;
                    while (i < 10) {
                        i = i + 1;
                        if (i == 4) {
                            break;
                        }
                    }
                    i;
                "#
            )
            .unwrap(),
            Value::Number(4.0)
        );
    }

    #[test]
    fn method_call_binds_this() {
        let src = "let o={x:3, f:function(){return this.x;}}; o.f();";
        assert_eq!(run_source(src).unwrap(), Value::Number(3.0));
    }

    #[test]
    fn new_uses_prototype_methods() {
        let src = "function C(x){ this.x=x; } C.prototype.get=function(){return this.x;}; let c=new C(7); c.get();";
        assert_eq!(run_source(src).unwrap(), Value::Number(7.0));
    }

    #[test]
    fn array_length_assignment_truncates() {
        let src = "let a=[1,2,3]; a.length=1; (a[1] === undefined) && (a.length === 1);";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn array_is_array_detects_array_literals_and_constructor_arrays() {
        let src = "Array.isArray([]) && Array.isArray(Array(1,2));";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn array_is_array_rejects_plain_objects_and_missing_values() {
        let src = "(!Array.isArray({})) && (!Array.isArray()) && (!Array.isArray('x'));";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn array_is_array_is_not_enumerable() {
        let src = "(!Array.propertyIsEnumerable('isArray')) && Object.keys(Array).length === 0;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn array_index_of_finds_first_matching_value() {
        let src = "[3,4,3].indexOf(3);";
        assert_eq!(run_source(src).unwrap(), Value::Number(0.0));
    }

    #[test]
    fn array_index_of_returns_minus_one_when_missing() {
        let src = "[1,2].indexOf(3);";
        assert_eq!(run_source(src).unwrap(), Value::Number(-1.0));
    }

    #[test]
    fn array_index_of_uses_positive_from_index() {
        let src = "[1,2,1].indexOf(1,1);";
        assert_eq!(run_source(src).unwrap(), Value::Number(2.0));
    }

    #[test]
    fn array_index_of_uses_negative_from_index() {
        let src = "[1,2,3,2].indexOf(2,-2);";
        assert_eq!(run_source(src).unwrap(), Value::Number(3.0));
    }

    #[test]
    fn array_index_of_skips_holes() {
        let src = "let a=[]; a.length=1; a.indexOf(undefined);";
        assert_eq!(run_source(src).unwrap(), Value::Number(-1.0));
    }

    #[test]
    fn array_index_of_is_not_enumerable() {
        let src = "Array.prototype.propertyIsEnumerable('indexOf');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    #[test]
    fn array_includes_finds_matching_value() {
        let src = "[1,2,3].includes(2);";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn array_includes_returns_false_when_missing() {
        let src = "[1,2,3].includes(4);";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    #[test]
    fn array_includes_uses_positive_from_index() {
        let src = "[1,2,1].includes(1,1);";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn array_includes_uses_negative_from_index() {
        let src = "[1,2,3].includes(1,-2);";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    #[test]
    fn array_includes_uses_same_value_zero_for_nan() {
        let src = "let n = 0 / 0; [n].includes(n);";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn array_includes_treats_holes_as_undefined() {
        let src = "let a=[]; a.length=1; a.includes(undefined);";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn array_includes_is_not_enumerable() {
        let src = "Array.prototype.propertyIsEnumerable('includes');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    #[test]
    fn array_slice_copies_all_items_without_arguments() {
        let src = "[1,2,3].slice().join(',');";
        assert_eq!(run_source(src).unwrap(), Value::String("1,2,3".into()));
    }

    #[test]
    fn array_slice_uses_start_and_end() {
        let src = "[1,2,3,4].slice(1,3).join(',');";
        assert_eq!(run_source(src).unwrap(), Value::String("2,3".into()));
    }

    #[test]
    fn array_slice_treats_explicit_undefined_end_as_length() {
        let src = "[1,2,3].slice(1, undefined).join(',');";
        assert_eq!(run_source(src).unwrap(), Value::String("2,3".into()));
    }

    #[test]
    fn array_slice_uses_negative_bounds() {
        let src = "[1,2,3,4].slice(-3,-1).join(',');";
        assert_eq!(run_source(src).unwrap(), Value::String("2,3".into()));
    }

    #[test]
    fn array_slice_returns_empty_array_for_reversed_range() {
        let src = "[1,2,3].slice(2,1).length;";
        assert_eq!(run_source(src).unwrap(), Value::Number(0.0));
    }

    #[test]
    fn array_slice_does_not_mutate_source_array() {
        let src = "let a=[1,2,3]; let b=a.slice(1); b[0]=9; a.join(',') + ':' + b.join(',');";
        assert_eq!(run_source(src).unwrap(), Value::String("1,2,3:9,3".into()));
    }

    #[test]
    fn array_slice_preserves_holes() {
        let src = "let a=[]; a.length=2; let b=a.slice(); b.hasOwnProperty('0') || b.hasOwnProperty('1');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    #[test]
    fn array_slice_is_not_enumerable() {
        let src = "Array.prototype.propertyIsEnumerable('slice');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    #[test]
    fn array_pop_returns_last_item_and_removes_it() {
        let src = "let a=[1,2,3]; let v=a.pop(); v + ':' + a.join(',');";
        assert_eq!(run_source(src).unwrap(), Value::String("3:1,2".into()));
    }

    #[test]
    fn array_pop_updates_length() {
        let src = "let a=[1,2]; a.pop(); a.length;";
        assert_eq!(run_source(src).unwrap(), Value::Number(1.0));
    }

    #[test]
    fn array_pop_empty_array_returns_undefined() {
        let src = "let a=[]; a.pop() === undefined && a.length === 0;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn array_pop_hole_returns_undefined_and_removes_slot() {
        let src = "let a=[]; a.length=1; a.pop() === undefined && a.length === 0;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn array_pop_is_not_enumerable() {
        let src = "Array.prototype.propertyIsEnumerable('pop');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    #[test]
    fn array_shift_returns_first_item_and_removes_it() {
        let src = "let a=[1,2,3]; let v=a.shift(); v + ':' + a.join(',');";
        assert_eq!(run_source(src).unwrap(), Value::String("1:2,3".into()));
    }

    #[test]
    fn array_shift_updates_length_and_indices() {
        let src = "let a=[1,2,3]; a.shift(); a.length + ':' + a[0];";
        assert_eq!(run_source(src).unwrap(), Value::String("2:2".into()));
    }

    #[test]
    fn array_shift_empty_array_returns_undefined() {
        let src = "let a=[]; a.shift() === undefined && a.length === 0;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn array_shift_hole_returns_undefined_and_moves_items() {
        let src = "let a=[]; a.length=2; a[1]=5; a.shift() === undefined && a.length === 1 && a[0] === 5;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn array_shift_is_not_enumerable() {
        let src = "Array.prototype.propertyIsEnumerable('shift');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    #[test]
    fn object_define_property_sets_value_descriptor() {
        let src = "let o={}; Object.defineProperty(o, 'a', {value: 3}); o.a;";
        assert_eq!(run_source(src).unwrap(), Value::Number(3.0));
    }

    #[test]
    fn object_define_property_returns_target_object() {
        let src = "let o={}; Object.defineProperty(o, 'a', {value: 1}) === o;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn object_define_property_sets_array_index() {
        let src = "let a=[]; Object.defineProperty(a, '1', {value: 7}); a.length + ':' + a[1];";
        assert_eq!(run_source(src).unwrap(), Value::String("2:7".into()));
    }

    #[test]
    fn object_define_property_allows_descriptor_to_alias_target() {
        let src = "let o={value:1}; Object.defineProperty(o, 'x', o); o.x;";
        assert_eq!(run_source(src).unwrap(), Value::Number(1.0));
    }

    #[test]
    fn object_get_own_property_descriptor_returns_data_descriptor() {
        let src = "let d=Object.getOwnPropertyDescriptor({a:3}, 'a'); d.value + ':' + d.writable + ':' + d.enumerable + ':' + d.configurable;";
        assert_eq!(
            run_source(src).unwrap(),
            Value::String("3:true:true:true".into())
        );
    }

    #[test]
    fn object_get_own_property_descriptor_reads_define_property_value() {
        let src = "let o={}; Object.defineProperty(o, 'a', {value: 3}); let d=Object.getOwnPropertyDescriptor(o, 'a'); d.value;";
        assert_eq!(run_source(src).unwrap(), Value::Number(3.0));
    }

    #[test]
    fn object_get_own_property_descriptor_ignores_prototype_properties() {
        let src = "let o={}; Object.getOwnPropertyDescriptor(o, 'toString') === undefined;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn object_get_own_property_descriptor_marks_builtin_properties_non_enumerable() {
        let src =
            "let d=Object.getOwnPropertyDescriptor(Object.prototype, 'toString'); d.enumerable;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    #[test]
    fn object_get_own_property_descriptor_describes_array_index() {
        let src =
            "let d=Object.getOwnPropertyDescriptor([10,20], '1'); d.value + ':' + d.enumerable;";
        assert_eq!(run_source(src).unwrap(), Value::String("20:true".into()));
    }

    #[test]
    fn object_get_own_property_descriptor_describes_array_length() {
        let src = "let d=Object.getOwnPropertyDescriptor([10,20], 'length'); d.value + ':' + d.enumerable + ':' + d.configurable;";
        assert_eq!(
            run_source(src).unwrap(),
            Value::String("2:false:false".into())
        );
    }

    #[test]
    fn object_get_own_property_descriptor_ignores_length_created_array_holes() {
        let src = "let a=[]; a.length=2; Object.getOwnPropertyDescriptor(a, '0') === undefined;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn object_get_own_property_descriptor_ignores_skipped_array_indices() {
        let src = "let a=[]; a[2]=1; Object.getOwnPropertyDescriptor(a, '1') === undefined;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn object_get_own_property_descriptor_describes_explicit_undefined_array_value() {
        let src = "let a=[]; a[0]=undefined; let d=Object.getOwnPropertyDescriptor(a, '0'); d !== undefined && d.value === undefined;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn object_create_uses_supplied_prototype() {
        let src = "let proto={x:7}; let o=Object.create(proto); o.x;";
        assert_eq!(run_source(src).unwrap(), Value::Number(7.0));
    }

    #[test]
    fn object_create_null_has_no_object_prototype() {
        let src = "let o=Object.create(null); Object.getPrototypeOf(o) === null;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn object_create_rejects_non_object_prototype() {
        let err = run_source("Object.create(1);").unwrap_err();
        assert!(
            err.to_string()
                .contains("Object.create expects object or null")
        );
    }

    #[test]
    fn object_get_prototype_of_returns_object_prototype() {
        let src = "Object.getPrototypeOf({}) === Object.prototype;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn object_get_prototype_of_returns_custom_prototype() {
        let src = "let proto={}; let o=Object.create(proto); Object.getPrototypeOf(o) === proto;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn array_push_appends_values_and_returns_length() {
        let src = "let a=[1,2]; let n=a.push(3,4); n + ':' + a.join(',');";
        assert_eq!(run_source(src).unwrap(), Value::String("4:1,2,3,4".into()));
    }

    #[test]
    fn array_push_updates_length_and_indices() {
        let src = "let a=[]; a.push(1); a.push(2); (a.length === 2) && (a[1] === 2);";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn has_own_property_detects_own_object_property() {
        let src = "let o={a:1}; o.hasOwnProperty('a');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn has_own_property_ignores_prototype_property() {
        let src = "let o={}; o.hasOwnProperty('toString');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    #[test]
    fn has_own_property_detects_array_index_and_length() {
        let src = "let a=[10]; a.hasOwnProperty('0') && a.hasOwnProperty('length');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn has_own_property_does_not_report_length_created_array_holes() {
        let src = "let a=[]; a.length=2; a.hasOwnProperty('0');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    #[test]
    fn has_own_property_does_not_report_skipped_array_indices() {
        let src = "let a=[]; a[2]=1; (!a.hasOwnProperty('1')) && a.hasOwnProperty('2');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn has_own_property_detects_array_named_property() {
        let src = "let a=[]; a.extra=1; a.hasOwnProperty('extra');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn property_is_enumerable_detects_own_object_property() {
        let src = "let o={a:1}; o.propertyIsEnumerable('a');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn property_is_enumerable_ignores_prototype_property() {
        let src = "let o={}; o.propertyIsEnumerable('toString');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    #[test]
    fn property_is_enumerable_detects_array_index_but_not_length() {
        let src = "let a=[10]; a.propertyIsEnumerable('0') && !a.propertyIsEnumerable('length');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn property_is_enumerable_detects_array_named_property() {
        let src = "let a=[]; a.extra=1; a.propertyIsEnumerable('extra');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn property_is_enumerable_ignores_length_created_array_holes() {
        let src = "let a=[]; a.length=1; a.propertyIsEnumerable('0');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    #[test]
    fn property_is_enumerable_ignores_runtime_installed_properties() {
        let src = "function C(){} (!Object.prototype.propertyIsEnumerable('toString')) && (!Array.prototype.propertyIsEnumerable('join')) && (!C.propertyIsEnumerable('prototype'));";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn object_keys_and_values_ignore_non_enumerable_builtin_properties() {
        let src =
            "Object.keys(Object.prototype).length + ':' + Object.values(Array.prototype).length;";
        assert_eq!(run_source(src).unwrap(), Value::String("0:0".into()));
    }

    #[test]
    fn array_foreach_runs_callback_for_each_item() {
        let src = "let total=0; [1,2,3].forEach(function(v){ total = total + v; }); total;";
        assert_eq!(run_source(src).unwrap(), Value::Number(6.0));
    }

    #[test]
    fn array_foreach_passes_index_and_source_array() {
        let src = "let out=''; let a=[10,20]; a.forEach(function(v,i,arr){ out = out + (v + i + arr.length); }); out;";
        assert_eq!(run_source(src).unwrap(), Value::String("1223".into()));
    }

    #[test]
    fn array_foreach_returns_undefined() {
        let src = "let result = [1].forEach(function(v){ return v + 1; }); result === undefined;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn array_foreach_reads_mutated_items_from_source_array() {
        let src = "let seen=''; let a=[1,2]; a.forEach(function(v,i,arr){ if (i === 0) { arr[1] = 9; } seen = seen + v; }); seen;";
        assert_eq!(run_source(src).unwrap(), Value::String("19".into()));
    }

    #[test]
    fn array_foreach_skips_items_removed_by_length_truncation() {
        let src = "let seen=''; let a=[1,2,3]; a.forEach(function(v,i,arr){ if (i === 0) { arr.length = 1; } seen = seen + v; }); seen;";
        assert_eq!(run_source(src).unwrap(), Value::String("1".into()));
    }

    #[test]
    fn array_filter_keeps_truthy_callback_results() {
        let src = "let a=[1,2,3,4]; a.filter(function(x){ return x > 2; }).join(',');";
        assert_eq!(run_source(src).unwrap(), Value::String("3,4".into()));
    }

    #[test]
    fn array_filter_returns_empty_array_for_no_matches() {
        let src = "let a=[1,2]; a.filter(function(x){ return x > 5; }).length;";
        assert_eq!(run_source(src).unwrap(), Value::Number(0.0));
    }

    #[test]
    fn array_filter_passes_index_to_callback() {
        let src = "let a=[10,20,30]; a.filter(function(_v,i){ return i > 0; }).join(',');";
        assert_eq!(run_source(src).unwrap(), Value::String("20,30".into()));
    }

    #[test]
    fn array_filter_passes_source_array_to_callback() {
        let src = "let a=[1,2,3]; a.filter(function(v,_i,arr){ return arr.length === 3 && v > 1; }).join(',');";
        assert_eq!(run_source(src).unwrap(), Value::String("2,3".into()));
    }

    #[test]
    fn array_map_passes_index_to_callback() {
        let src = "let a=[10,20,30]; a.map(function(v,i){ return v + i; }).join(',');";
        assert_eq!(run_source(src).unwrap(), Value::String("10,21,32".into()));
    }

    #[test]
    fn array_map_passes_source_array_to_callback() {
        let src = "let a=[1,2,3]; a.map(function(v,_i,arr){ return v + arr.length; }).join(',');";
        assert_eq!(run_source(src).unwrap(), Value::String("4,5,6".into()));
    }

    #[test]
    fn instanceof_walks_prototype_chain() {
        let src = "function C(){} let c=new C(); c instanceof C;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn continue_skips_to_next_loop_iteration() {
        assert_eq!(
            run_source(
                r#"
                    let i = 0;
                    let total = 0;
                    while (i < 5) {
                        i = i + 1;
                        if (i == 3) {
                            continue;
                        }
                        total = total + i;
                    }
                    total;
                "#
            )
            .unwrap(),
            Value::Number(12.0)
        );
    }

    #[test]
    fn break_only_exits_nearest_loop() {
        assert_eq!(
            run_source(
                r#"
                    let outer = 0;
                    let hits = 0;
                    while (outer < 3) {
                        outer = outer + 1;
                        let inner = 0;
                        while (inner < 3) {
                            inner = inner + 1;
                            if (inner == 2) {
                                break;
                            }
                            hits = hits + 1;
                        }
                    }
                    hits;
                "#
            )
            .unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn break_outside_loop_is_runtime_error() {
        let error = run_source("break;").unwrap_err();

        assert!(error.to_string().contains("break used outside loop"));
    }

    #[test]
    fn continue_outside_loop_is_runtime_error() {
        let error = run_source("continue;").unwrap_err();

        assert!(error.to_string().contains("continue used outside loop"));
    }

    #[test]
    fn for_loop_runs_init_condition_and_update() {
        let src = r#"
            let total = 0;
            for (let i = 0; i < 4; i = i + 1) {
                total = total + i;
            }
            total;
        "#;

        assert_eq!(run_source(src).unwrap(), Value::Number(6.0));
    }

    #[test]
    fn switch_matches_case_and_stops_at_break() {
        let src = r#"
            let value = 2;
            let label = "";
            switch (value) {
                case 1:
                    label = "one";
                    break;
                case 2:
                    label = "two";
                    break;
                default:
                    label = "other";
            }
            label;
        "#;

        assert_eq!(run_source(src).unwrap(), Value::String("two".into()));
    }

    #[test]
    fn try_catch_finally_handles_throw_and_runs_finally() {
        let src = r#"
            let message = "";
            let cleaned = false;
            try {
                throw Error("boom");
            } catch (err) {
                message = err.message;
            } finally {
                cleaned = true;
            }
            message + ":" + cleaned;
        "#;

        assert_eq!(run_source(src).unwrap(), Value::String("boom:true".into()));
    }

    #[test]
    fn object_and_array_builtins_cover_agent_style_data_flow() {
        let src = r#"
            let input = [1, 2, 3];
            let output = input.map(function (value) {
                return value * 3;
            });
            let summary = {
                tag: Object.prototype.toString(),
                values: output.join("|")
            };
            JSON.stringify(summary.values);
        "#;

        assert_eq!(run_source(src).unwrap(), Value::String("\"3|6|9\"".into()));
    }

    #[test]
    fn object_keys_returns_sorted_own_keys() {
        let src = "let o={b:2,a:1}; Object.keys(o).join(',');";
        assert_eq!(run_source(src).unwrap(), Value::String("a,b".into()));
    }

    #[test]
    fn object_keys_includes_array_indices() {
        let src = "let a=[10,20]; a.extra=30; Object.keys(a).join(',');";
        assert_eq!(run_source(src).unwrap(), Value::String("0,1,extra".into()));
    }

    #[test]
    fn object_keys_orders_array_indices_numerically() {
        let src = "let a=[0,1,2,3,4,5,6,7,8,9,10,11]; Object.keys(a).join(',');";
        assert_eq!(
            run_source(src).unwrap(),
            Value::String("0,1,2,3,4,5,6,7,8,9,10,11".into())
        );
    }

    #[test]
    fn object_values_returns_values_in_sorted_key_order() {
        let src = "let o={b:2,a:1}; Object.values(o).join(',');";
        assert_eq!(run_source(src).unwrap(), Value::String("1,2".into()));
    }

    #[test]
    fn object_values_includes_numeric_keys_on_plain_objects() {
        let src = "let o={0:'zero',a:'A'}; Object.values(o).join(',');";
        assert_eq!(run_source(src).unwrap(), Value::String("zero,A".into()));
    }

    #[test]
    fn object_values_includes_numeric_keys_assigned_to_plain_objects() {
        let src = "let o={a:'A'}; o[0]='zero'; Object.values(o).join(',');";
        assert_eq!(run_source(src).unwrap(), Value::String("zero,A".into()));
    }

    #[test]
    fn object_values_includes_array_indices_then_named_values() {
        let src = "let a=[10,20]; a.extra=30; Object.values(a).join(',');";
        assert_eq!(run_source(src).unwrap(), Value::String("10,20,30".into()));
    }

    #[test]
    fn object_values_orders_array_indices_numerically() {
        let src = "let a=[0,1,2,3,4,5,6,7,8,9,10,11]; Object.values(a).join(',');";
        assert_eq!(
            run_source(src).unwrap(),
            Value::String("0,1,2,3,4,5,6,7,8,9,10,11".into())
        );
    }
}
