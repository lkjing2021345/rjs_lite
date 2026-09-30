use crate::ast::{BinaryOp, ClassElement, Expr, Pattern, Program, Stmt, UnaryOp};
use crate::error::{JsError, JsErrorType, JsResult};
use crate::value::{Internal, Object, ObjectRef, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone)]
pub(crate) struct Binding {
    value: Value,
    mutable: bool,
}

#[derive(Clone)]
pub(crate) struct Env {
    values: HashMap<String, Binding>,
    parent: Option<Rc<RefCell<Env>>>,
}

impl Env {
    pub(crate) fn new() -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            values: HashMap::new(),
            parent: None,
        }))
    }

    pub(crate) fn child(parent: Rc<RefCell<Env>>) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            values: HashMap::new(),
            parent: Some(parent),
        }))
    }

    pub(crate) fn define(&mut self, name: String, value: Value, mutable: bool) {
        self.values.insert(name, Binding { value, mutable });
    }

    pub(crate) fn get(&self, name: &str) -> Option<Value> {
        self.values
            .get(name)
            .map(|b| b.value.clone())
            .or_else(|| self.parent.as_ref().and_then(|p| p.borrow().get(name)))
    }

    pub(crate) fn assign(&mut self, name: &str, value: Value) -> JsResult<()> {
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Flow {
    Value(Value),
    Return(Value),
    Throw(Value),
    Break,
    Continue,
    /// Produced when a `yield` expression reaches the statement boundary.
    /// Propagates out of the generator body like `Return`.
    Yield(Value),
}

/// Captured state of a suspended generator. The body is re-run from the top
/// on each `.next()` call; `yields_seen` records how many yields have already
/// been delivered so the matching one can be captured and execution aborted.
///
/// This is a pragmatic tree-walking approximation: side effects before the
/// next un-delivered `yield` are recomputed on each resume. It produces the
/// correct `{value, done}` sequence for deterministic generator bodies.
pub(crate) struct GeneratorState {
    params: Vec<Pattern>,
    body: Vec<Stmt>,
    closure_env: Rc<RefCell<Env>>,
    args: Vec<Value>,
    this_value: Value,
    /// Number of yields already delivered to callers.
    yields_seen: usize,
    /// True for generators created by the VM: no tree-walking body to run,
    /// so `.next()` immediately reports completion.
    noop: bool,
}

struct RefTarget {
    object: ObjectRef,
    property: String,
}

pub struct Interpreter {
    pub(crate) env: Rc<RefCell<Env>>,
    pub(crate) closures: HashMap<usize, Rc<RefCell<Env>>>,
    /// Live generator states keyed by generator-object pointer.
    pub(crate) generators: HashMap<usize, GeneratorState>,
    /// During a generator resume, the index of the yield to capture.
    pub(crate) yield_target: Option<usize>,
    /// Number of yields encountered during the current generator resume.
    pub(crate) yield_counter: usize,
    /// Set while evaluating a `yield` expression; consumed at the statement
    /// boundary to produce `Flow::Yield`.
    pub(crate) pending_yield: Option<Value>,
    pub(crate) output: Vec<String>,
    pub(crate) global: ObjectRef,
    pub(crate) object_proto: ObjectRef,
    pub(crate) function_proto: ObjectRef,
    pub(crate) array_proto: ObjectRef,
    pub(crate) error_proto: ObjectRef,
    pub(crate) string_proto: ObjectRef,
    pub(crate) number_proto: ObjectRef,
    pub(crate) boolean_proto: ObjectRef,
    pub(crate) step_limit: Option<usize>,
    pub(crate) steps: usize,
    pub(crate) max_call_depth: Option<usize>,
    pub(crate) call_depth: usize,
    pub(crate) output_limit: Option<usize>,
    pub(crate) output_truncated: bool,
    pub(crate) strict: bool,
    /// Optional bridge: when the VM owns the interpreter, native methods
    /// that call JS callbacks are routed through this hook so the VM can
    /// execute VM-compiled functions.
    pub(crate) call_host: Option<
        Box<dyn FnMut(&mut Interpreter, Value, Vec<Value>, Value, bool) -> JsResult<Value>>,
    >,
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
            generators: HashMap::new(),
            yield_target: None,
            yield_counter: 0,
            pending_yield: None,
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
            call_host: None,
        };
        this.install_builtins();
        this
    }

    pub(crate) fn install_builtins(&mut self) {
        self.define_native("print");
        self.define_native("Error");
        self.define_native("TypeError");
        self.define_native("SyntaxError");
        self.define_native("ReferenceError");
        self.define_native("RangeError");
        self.define_native("EvalError");
        self.define_native("URIError");
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
        self.define_native("RegExp");
        self.define_native("Map");
        self.define_native("Reflect");

        self.define_global("Infinity", Value::Number(f64::INFINITY), false);
        self.define_global("NaN", Value::Number(f64::NAN), false);
        self.define_global("undefined", Value::Undefined, false);
        // `globalThis` — the global object itself.
        self.define_global("globalThis", Value::Object(self.global.clone()), false);

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
        if let Some(Value::Object(function_ctor)) = self.env.borrow().get("Function") {
            Self::define_non_enumerable(
                &function_ctor,
                "prototype",
                Value::Object(self.function_proto.clone()),
            );
        }
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
                &object_ctor,
                "fromEntries",
                self.native_method("Object.fromEntries"),
            );
            Self::define_non_enumerable(
                &object_ctor,
                "defineProperties",
                self.native_method("Object.defineProperties"),
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
            Self::define_non_enumerable(
                &self.object_proto,
                "valueOf",
                self.native_method("Object.prototype.valueOf"),
            );
            Self::define_non_enumerable(
                &self.object_proto,
                "toLocaleString",
                self.native_method("Object.prototype.toLocaleString"),
            );
            Self::define_non_enumerable(
                &self.object_proto,
                "isPrototypeOf",
                self.native_method("Object.prototype.isPrototypeOf"),
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
                "toString",
                self.native_method("Array.prototype.toString"),
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
            Self::define_non_enumerable(
                &self.array_proto,
                "at",
                self.native_method("Array.prototype.at"),
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
            Self::define_non_enumerable(
                &self.string_proto,
                "at",
                self.native_method("String.prototype.at"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "codePointAt",
                self.native_method("String.prototype.codePointAt"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "replaceAll",
                self.native_method("String.prototype.replaceAll"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "localeCompare",
                self.native_method("String.prototype.localeCompare"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "search",
                self.native_method("String.prototype.search"),
            );
            Self::define_non_enumerable(
                &self.string_proto,
                "match",
                self.native_method("String.prototype.match"),
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
            "EvalError",
            "URIError",
        ] {
            if let Some(Value::Object(ctor)) = self.env.borrow().get(name) {
                Self::define_non_enumerable(
                    &ctor,
                    "prototype",
                    Value::Object(self.error_proto.clone()),
                );
            }
        }
        for name in [
            "Number.isNaN",
            "Number.isFinite",
            "Number.parseInt",
            "Number.parseFloat",
            "Number.isInteger",
        ] {
            self.define_native(name);
        }
        if let Some(Value::Object(number_ctor)) = self.env.borrow().get("Number") {
            Self::define_non_enumerable(&number_ctor, "isSafeInteger", self.native_method("Number.isSafeInteger"));
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
        self.error_proto
            .borrow_mut()
            .props
            .insert("name".into(), Value::String("Error".into()));
        self.error_proto
            .borrow_mut()
            .props
            .insert("message".into(), Value::String(String::new()));
        self.define_native("Symbol");
        if let Some(Value::Object(sym_ctor)) = self.env.borrow().get("Symbol") {
            let toStringTag = Value::String("Symbol.toStringTag".into());
            Self::define_non_enumerable(&sym_ctor, "toStringTag", toStringTag.clone());
            Self::define_non_enumerable(&sym_ctor, "iterator", Value::String("Symbol.iterator".into()));
            Self::define_non_enumerable(&sym_ctor, "species", Value::String("Symbol.species".into()));
            Self::define_non_enumerable(&sym_ctor, "toPrimitive", Value::String("Symbol.toPrimitive".into()));
        }
        if let Some(Value::Object(map_ctor)) = self.env.borrow().get("Map") {
            let map_proto = Object::plain();
            map_proto.borrow_mut().proto = Some(self.object_proto.clone());
            Self::define_non_enumerable(&map_ctor, "prototype", Value::Object(map_proto.clone()));
            Self::define_non_enumerable(&map_proto, "get", self.native_method("Map.prototype.get"));
            Self::define_non_enumerable(&map_proto, "set", self.native_method("Map.prototype.set"));
            Self::define_non_enumerable(&map_proto, "has", self.native_method("Map.prototype.has"));
        }
        if let Some(Value::Object(reflect_obj)) = self.env.borrow().get("Reflect") {
            Self::define_non_enumerable(&reflect_obj, "ownKeys", self.native_method("Reflect.ownKeys"));
        }
        Self::define_non_enumerable(&self.array_proto, "entries", self.native_method("Array.prototype.entries"));
        Self::define_non_enumerable(&self.array_proto, "keys", self.native_method("Array.prototype.keys"));
        Self::define_non_enumerable(&self.array_proto, "values", self.native_method("Array.prototype.values"));
    }

    pub(crate) fn define_non_enumerable(object: &ObjectRef, property: &str, value: Value) {
        let mut object = object.borrow_mut();
        object.props.insert(property.to_string(), value);
        object.non_enumerable_props.insert(property.to_string());
    }

    fn define_native(&mut self, name: &'static str) {
        let value = self.native_method(name);
        self.define_global(name, value, false);
    }

    pub(crate) fn native_method(&self, name: &'static str) -> Value {
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

    pub(crate) fn same_value_zero(left: &Value, right: &Value) -> bool {
        match (left, right) {
            (Value::Number(a), Value::Number(b)) => a == b || (a.is_nan() && b.is_nan()),
            _ => left == right,
        }
    }

    pub(crate) fn array_slice_bound(value: f64, len: isize) -> isize {
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

    /// Render a thrown value as a human-readable message: honours a JS
    /// `toString`/`name`+`message`, falling back to the default rendering.
    fn throw_to_string(&mut self, v: &Value) -> String {
        if let Value::Object(_) = v {
            let rendered = self.to_string_value(v).unwrap_or_else(|_| v.to_string());
            if rendered != "[object Object]" {
                return rendered;
            }
            let Value::Object(o) = v else { unreachable!() };
            let obj = o.borrow();
            let name = obj.props.get("name").map(|n| n.to_string());
            let message = obj.props.get("message").map(|m| m.to_string());
            return match (name, message) {
                (Some(n), Some(m)) if !m.is_empty() => format!("{n}: {m}"),
                (Some(n), _) => n,
                (None, Some(m)) => m,
                _ => "[object Object]".into(),
            };
        }
        v.to_string()
    }

    pub fn run(&mut self, program: &Program) -> JsResult<Value> {
        self.detect_strict_mode(&program.statements);
        // A `throw` may surface either as `Flow::Throw` or, when it crosses a
        // function boundary, as `JsError::Flow(Flow::Throw(_))`.
        let (flow, propagated) = match self.eval_statements(&program.statements) {
            Ok(flow) => (flow, None),
            Err(JsError::Flow(flow)) => (flow, None),
            Err(e) => (Flow::Value(Value::Undefined), Some(e)),
        };
        if let Some(e) = propagated {
            return Err(e);
        }
        match flow {
            Flow::Value(v) | Flow::Return(v) => Ok(v),
            Flow::Throw(v) => {
                let msg = self.throw_to_string(&v);
                Err(JsError::runtime(msg))
            }
            Flow::Break => Err(JsError::syntax_error("break used outside loop")),
            Flow::Continue => Err(JsError::syntax_error("continue used outside loop")),
            Flow::Yield(_) => Err(JsError::syntax_error("yield used outside generator")),
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
                flow @ (Flow::Return(_)
                | Flow::Throw(_)
                | Flow::Break
                | Flow::Continue
                | Flow::Yield(_)) => {
                    return Ok(flow);
                }
            }
        }
        Ok(Flow::Value(last))
    }

    /// Evaluate a statement, converting a `yield` encountered anywhere inside
    /// its expression evaluation into a `Flow::Yield` unwinding signal.
    fn eval_stmt(&mut self, stmt: &Stmt) -> JsResult<Flow> {
        let flow = self.eval_stmt_inner(stmt)?;
        if let Some(value) = self.pending_yield.take() {
            return Ok(Flow::Yield(value));
        }
        Ok(flow)
    }

    fn eval_stmt_inner(&mut self, stmt: &Stmt) -> JsResult<Flow> {
        self.step()?;
        match stmt {
            Stmt::VarDecl {
                name,
                value,
                mutable,
            } => {
                let value = self.eval_expr(value)?;
                self.bind_pattern(name, value, *mutable)?;
                Ok(Flow::Value(Value::Undefined))
            }
            Stmt::VarDecls {
                declarations,
                mutable,
            } => {
                for (name, expr) in declarations {
                    let value = self.eval_expr(expr)?;
                    self.bind_pattern(name, value, *mutable)?;
                }
                Ok(Flow::Value(Value::Undefined))
            }
            Stmt::FunctionDecl {
                name,
                params,
                body,
                generator,
            } => {
                let value = self.make_function_async(params.clone(), body.clone(), false, *generator);
                Self::set_function_name(&value, name);
                self.env
                    .borrow_mut()
                    .define(name.clone(), value.clone(), false);
                if self.env.borrow().parent.is_none() {
                    self.global.borrow_mut().props.insert(name.clone(), value);
                }
                Ok(Flow::Value(Value::Undefined))
            }
            Stmt::ClassDecl {
                name,
                extends,
                body,
            } => {
                let class = self.make_class(Some(name.clone()), extends, body)?;
                self.env
                    .borrow_mut()
                    .define(name.clone(), class.clone(), false);
                if self.env.borrow().parent.is_none() {
                    self.global.borrow_mut().props.insert(name.clone(), class);
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
                let mut result = match self.catchable(|s| s.with_child(block))? {
                    Flow::Throw(v) => {
                        if let Some(catch) = catch_block {
                            let previous = self.env.clone();
                            self.env = Env::child(previous.clone());
                            if let Some(param) = catch_param {
                                self.env.borrow_mut().define(param.clone(), v, true);
                            }
                            let r = self.catchable(|s| s.eval_statements(catch));
                            self.env = previous;
                            r?
                        } else {
                            Flow::Throw(v)
                        }
                    }
                    other => other,
                };
                if let Some(finally) = finally_block {
                    // finally always runs; its throw/return/break/continue
                    // override the pending result per ECMAScript.
                    if let Ok(finally_result) = self.catchable(|s| s.with_child(finally))
                        && !matches!(finally_result, Flow::Value(_))
                    {
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
                        r @ (Flow::Return(_) | Flow::Throw(_) | Flow::Yield(_)) => return Ok(r),
                    }
                }
                Ok(Flow::Value(last))
            }
            Stmt::DoWhile { body, condition } => {
                let mut last = Value::Undefined;
                loop {
                    match self.with_child(body)? {
                        Flow::Value(v) => last = v,
                        Flow::Break => break,
                        Flow::Continue => {}
                        r @ (Flow::Return(_) | Flow::Throw(_) | Flow::Yield(_)) => return Ok(r),
                    }
                    if !self.eval_expr(condition)?.is_truthy() {
                        break;
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
                        r @ (Flow::Return(_) | Flow::Throw(_) | Flow::Break | Flow::Continue | Flow::Yield(_)) => {
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
                        r @ (Flow::Return(_) | Flow::Throw(_) | Flow::Yield(_)) => return Ok(r),
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
                        r @ (Flow::Return(_) | Flow::Throw(_) | Flow::Yield(_)) => return Ok(r),
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
                            r @ (Flow::Return(_) | Flow::Throw(_) | Flow::Continue | Flow::Yield(_)) => {
                                return Ok(r);
                            }
                        }
                    }
                }
                if !matched {
                    match self.with_child(default)? {
                        Flow::Value(v) => last = v,
                        Flow::Break => return Ok(Flow::Value(last)),
                        r @ (Flow::Return(_) | Flow::Throw(_) | Flow::Continue | Flow::Yield(_)) => return Ok(r),
                    }
                }
                Ok(Flow::Value(last))
            }
            Stmt::Break => Ok(Flow::Break),
            Stmt::Continue | Stmt::LabeledContinue { .. } => Ok(Flow::Continue),
            Stmt::LabeledBreak { .. } => Ok(Flow::Break),
            // A labeled statement catches a `break` targeting its own label
            // (e.g. `label: { break label; }`). Loops catch their own breaks
            // first, so this only consumes breaks that would otherwise leak.
            Stmt::Labeled { body, .. } => match self.eval_stmt(body)? {
                Flow::Break => Ok(Flow::Value(Value::Undefined)),
                other => Ok(other),
            },
            Stmt::Block(stmts) => self.with_child(stmts),
            Stmt::Expr(expr) => Ok(Flow::Value(self.eval_expr(expr)?)),
            Stmt::With { expression, body } => {
                let obj = self.eval_expr(expression)?;
                // Create a child env whose properties are the with-object's
                // own + inherited properties.
                let previous = self.env.clone();
                let mut child_env = Env::child(previous.clone());
                if let Value::Object(o) = &obj {
                    for (k, v) in o.borrow().props.clone() {
                        child_env.borrow_mut().define(k, v, true);
                    }
                }
                self.env = child_env;
                let result = self.eval_statements(body);
                self.env = previous;
                result
            }
        }
    }

    fn with_child(&mut self, statements: &[Stmt]) -> JsResult<Flow> {
        let previous = self.env.clone();
        self.env = Env::child(previous.clone());
        let result = self.eval_statements(statements);
        self.env = previous;
        result
    }

    fn make_function(&mut self, params: Vec<Pattern>, body: Vec<Stmt>) -> Value {
        self.make_function_async(params, body, false, false)
    }

    /// Set a non-enumerable `name` own property on a function value.
    pub(crate) fn set_function_name(value: &Value, name: &str) {
        if let Value::Object(obj) = value {
            Self::define_non_enumerable(obj, "name", Value::String(name.to_string()));
        }
    }

    fn make_function_async(
        &mut self,
        params: Vec<Pattern>,
        body: Vec<Stmt>,
        is_async: bool,
        generator: bool,
    ) -> Value {
        let arity = params
            .iter()
            .take_while(|p| !matches!(p, Pattern::Rest(_)))
            .count();
        let obj = Object::with_internal(Internal::Function {
            params,
            body,
            func_index: 0,
            is_async,
            generator,
        });
        obj.borrow_mut().proto = Some(self.function_proto.clone());
        let proto = Object::plain();
        proto.borrow_mut().proto = Some(self.object_proto.clone());
        Self::define_non_enumerable(&proto, "constructor", Value::Object(obj.clone()));
        Self::define_non_enumerable(&obj, "prototype", Value::Object(proto));
        Self::define_non_enumerable(&obj, "length", Value::Number(arity as f64));
        Self::define_non_enumerable(&obj, "name", Value::String(String::new()));
        let value = Value::Object(obj.clone());
        self.remember_closure(&value);
        value
    }

    /// Build a class value: a callable object (the constructor) whose
    /// `prototype` holds the instance methods and whose own properties hold
    /// the static members. Instance field initializers are prepended to the
    /// constructor body so they run on `new`.
    fn make_class(
        &mut self,
        name: Option<String>,
        extends: &Option<Box<Expr>>,
        body: &[ClassElement],
    ) -> JsResult<Value> {
        let parent = match extends {
            Some(e) => Some(self.eval_expr(e)?),
            None => None,
        };
        let class_obj = Object::with_internal(Internal::Function {
            params: Vec::new(),
            body: Vec::new(),
            func_index: 0,
            is_async: false,
            generator: false,
        });
        class_obj.borrow_mut().proto = Some(self.function_proto.clone());

        let proto_obj = Object::plain();
        let parent_proto = match &parent {
            Some(Value::Object(p)) => p.borrow().props.get("prototype").cloned(),
            _ => None,
        };
        proto_obj.borrow_mut().proto = Some(match parent_proto {
            Some(Value::Object(pp)) => pp,
            _ => self.object_proto.clone(),
        });
        Self::define_non_enumerable(&proto_obj, "constructor", Value::Object(class_obj.clone()));
        Self::define_non_enumerable(&class_obj, "prototype", Value::Object(proto_obj.clone()));
        Self::define_non_enumerable(
            &class_obj,
            "name",
            Value::String(name.clone().unwrap_or_default()),
        );
        Self::define_non_enumerable(
            &class_obj,
            "__super__",
            parent.clone().unwrap_or(Value::Undefined),
        );

        // Assemble the constructor: instance field initializers followed by the
        // explicit `constructor` body (if any).
        let mut ctor_params: Vec<Pattern> = Vec::new();
        let mut ctor_body: Vec<Stmt> = Vec::new();
        for element in body {
            if let ClassElement::Field {
                name,
                init,
                is_static: false,
                ..
            } = element
            {
                ctor_body.push(Stmt::Expr(Expr::Assign {
                    target: Box::new(Expr::Member {
                        object: Box::new(Expr::This),
                        property: name.clone(),
                    }),
                    value: Box::new(init.as_deref().cloned().unwrap_or(Expr::Undefined)),
                }));
            }
        }
        for element in body {
            match element {
                ClassElement::Constructor { params, body } => {
                    ctor_params = params.clone();
                    ctor_body.extend(body.iter().cloned());
                }
                ClassElement::Method {
                    name,
                    params,
                    body,
                    is_static,
                    is_generator,
                    is_async,
                } => {
                    let function = self.make_function_async(
                        params.clone(),
                        body.clone(),
                        *is_async,
                        *is_generator,
                    );
                    let target = if *is_static { &class_obj } else { &proto_obj };
                    Self::tag_super(&function, &parent);
                    Self::define_non_enumerable(target, name, function);
                }
                ClassElement::Getter {
                    name,
                    body,
                    is_static,
                } => {
                    let function = self.make_function(Vec::new(), body.clone());
                    let target = if *is_static { &class_obj } else { &proto_obj };
                    Self::tag_super(&function, &parent);
                    let key = format!("__get_{name}");
                    Self::define_non_enumerable(target, &key, function);
                }
                ClassElement::Setter {
                    name,
                    param,
                    body,
                    is_static,
                } => {
                    let function = self.make_function(vec![param.clone()], body.clone());
                    let target = if *is_static { &class_obj } else { &proto_obj };
                    Self::tag_super(&function, &parent);
                    let key = format!("__set_{name}");
                    Self::define_non_enumerable(target, &key, function);
                }
                ClassElement::Field {
                    name,
                    init,
                    is_static: true,
                    ..
                } => {
                    let value = match init {
                        Some(expr) => self.eval_expr(expr)?,
                        None => Value::Undefined,
                    };
                    Self::define_non_enumerable(&class_obj, name, value);
                }
                ClassElement::Field { .. } => {}
            }
        }

        if let Internal::Function { params, body, .. } = &mut class_obj.borrow_mut().internal {
            *params = ctor_params;
            *body = ctor_body;
        }
        // The caller creates the binding; the declared name is not needed here.
        let _ = name;
        let value = Value::Object(class_obj.clone());
        self.remember_closure(&value);
        Ok(value)
    }

    /// Create a generator object wrapping a suspended generator function. The
    /// returned object exposes a `.next()` method that drives the body.
    fn make_generator(
        &mut self,
        params: Vec<Pattern>,
        body: Vec<Stmt>,
        closure_env: Rc<RefCell<Env>>,
        args: Vec<Value>,
        this_value: Value,
    ) -> Value {
        let obj = Object::plain();
        obj.borrow_mut().proto = Some(self.object_proto.clone());
        Self::define_non_enumerable(
            &obj,
            "next",
            self.native_method("Generator.prototype.next"),
        );
        let value = Value::Object(obj.clone());
        self.generators.insert(
            Rc::as_ptr(&obj) as usize,
            GeneratorState {
                params,
                body,
                closure_env,
                args,
                this_value,
                yields_seen: 0,
                noop: false,
            },
        );
        value
    }

    /// Register a generator object created by the VM. It has no tree-walking
    /// body, so `.next()` immediately reports completion.
    pub(crate) fn register_noop_generator(&mut self, obj: &ObjectRef) {
        self.generators.insert(
            Rc::as_ptr(obj) as usize,
            GeneratorState {
                params: Vec::new(),
                body: Vec::new(),
                closure_env: self.env.clone(),
                args: Vec::new(),
                this_value: Value::Undefined,
                yields_seen: 0,
                noop: true,
            },
        );
    }

    /// Advance the generator identified by `key` by one `.next()` call,
    /// returning the `{ value, done }` iterator result object.
    fn resume_generator(&mut self, key: usize) -> JsResult<Value> {
        let Some(state) = self.generators.remove(&key) else {
            return Ok(Self::iterator_result(Value::Undefined, true));
        };
        if state.noop {
            // VM-created generators have no runnable tree-walking body.
            return Ok(Self::iterator_result(Value::Undefined, true));
        }
        let mut state = state;

        let previous_env = self.env.clone();
        self.env = Env::child(state.closure_env.clone());
        self.env
            .borrow_mut()
            .define("this".into(), state.this_value.clone(), true);
        // Bind parameters for this resume. Default-value expressions are
        // re-evaluated on each resume (an accepted simplification).
        let bind_result = (|| -> JsResult<()> {
            for (index, pattern) in state.params.iter().enumerate() {
                if let Pattern::Rest(inner) = pattern {
                    let rest: Vec<Option<Value>> = state
                        .args
                        .iter()
                        .skip(index)
                        .map(|v| Some(v.clone()))
                        .collect();
                    let arr = Object::with_internal(Internal::Array(rest));
                    arr.borrow_mut().proto = Some(self.array_proto.clone());
                    self.bind_pattern(inner, Value::Object(arr), true)?;
                    break;
                }
                let value = state
                    .args
                    .get(index)
                    .cloned()
                    .unwrap_or(Value::Undefined);
                self.bind_pattern(pattern, value, true)?;
            }
            Ok(())
        })();

        if let Err(e) = bind_result {
            self.env = previous_env;
            return Err(e);
        }

        // Capture the (yields_seen+1)-th yield encountered during this run.
        self.yield_target = Some(state.yields_seen);
        self.yield_counter = 0;
        self.pending_yield = None;
        let result = self.eval_statements(&state.body);
        self.yield_target = None;
        self.pending_yield = None;
        self.env = previous_env;

        match result {
            Ok(Flow::Yield(value)) => {
                state.yields_seen += 1;
                self.generators.insert(key, state);
                Ok(Self::iterator_result(value, false))
            }
            Ok(Flow::Value(_)) | Ok(Flow::Return(_)) => {
                // Completed generators are dropped; a later `.next()` on the
                // stale object reports `{ value: undefined, done: true }`.
                Ok(Self::iterator_result(Value::Undefined, true))
            }
            Ok(Flow::Throw(value)) => Err(JsError::runtime(value.to_string())),
            Ok(Flow::Break) => Err(JsError::syntax_error("break used outside loop")),
            Ok(Flow::Continue) => Err(JsError::syntax_error("continue used outside loop")),
            Err(e) => Err(e),
        }
    }

    /// Build an `{ value, done }` iterator-result object.
    fn iterator_result(value: Value, done: bool) -> Value {
        let obj = Object::plain();
        obj.borrow_mut().props.insert("value".into(), value);
        obj.borrow_mut().props.insert("done".into(), Value::Bool(done));
        Value::Object(obj)
    }

    fn remember_closure(&mut self, function: &Value) {
        if let Value::Object(obj) = function {
            self.closures
                .insert(Rc::as_ptr(obj) as usize, self.env.clone());
        }
    }

    /// ECMAScript ToPrimitive: for objects, try `valueOf` then `toString`
    /// (calling user-defined methods), falling back to the object itself.
    pub(crate) fn to_primitive_value(&mut self, value: &Value) -> JsResult<Value> {
        if !matches!(value, Value::Object(_)) {
            return Ok(value.clone());
        }
        for name in ["valueOf", "toString"] {
            let method = self.get_property_on_value(value, name);
            if method.is_callable() {
                let r = self.call(method, Vec::new(), value.clone(), false)?;
                if !matches!(r, Value::Object(_)) {
                    return Ok(r);
                }
            }
        }
        // No usable primitive; fall back to implementation-defined string.
        Ok(Value::String(value.to_string()))
    }

    /// ECMAScript abstract equality (`==`) including object→primitive coercion.
    fn abstract_eq_js(&mut self, a: &Value, b: &Value) -> JsResult<bool> {
        match (a, b) {
            (Value::Object(_), Value::Object(_)) => Ok(a == b),
            (Value::Object(_), Value::Null | Value::Undefined) => Ok(false),
            (Value::Null | Value::Undefined, Value::Object(_)) => Ok(false),
            (Value::Object(_), _) => {
                let p = self.to_primitive_value(a)?;
                self.abstract_eq_js(&p, b)
            }
            (_, Value::Object(_)) => {
                let p = self.to_primitive_value(b)?;
                self.abstract_eq_js(a, &p)
            }
            _ => Ok(a.abstract_eq(b)),
        }
    }

    /// ECMAScript ToString, honouring user-defined `toString`/`valueOf`.
    pub(crate) fn to_string_value(&mut self, value: &Value) -> JsResult<String> {
        match value {
            Value::Object(_) => {
                let prim = self.to_primitive_value(value)?;
                Ok(if matches!(prim, Value::Object(_)) {
                    value.to_string()
                } else {
                    prim.to_string()
                })
            }
            other => Ok(other.to_string()),
        }
    }

    /// Convert a host-level `JsError` into a JS Error object so it can be
    /// caught by `try`/`catch` (runtime errors are catchable in ECMAScript).
    fn error_to_value(&self, error: &JsError) -> Value {
        let (name, message) = match error {
            JsError::Lex { message, .. } | JsError::Parse { message, .. } => {
                ("SyntaxError", message.clone())
            }
            JsError::Runtime { message, error_type } => (error_type.as_str(), message.clone()),
            JsError::Flow(_) => ("Error", error.to_string()),
        };
        let obj = Object::plain();
        obj.borrow_mut().proto = Some(self.error_proto.clone());
        obj.borrow_mut()
            .props
            .insert("name".into(), Value::String(name.into()));
        obj.borrow_mut()
            .props
            .insert("message".into(), Value::String(message));
        // Own `constructor` so `err.constructor === SyntaxError` holds.
        if let Some(ctor) = self.env.borrow().get(name) {
            obj.borrow_mut().props.insert("constructor".into(), ctor);
        }
        Value::Object(obj)
    }

    /// Evaluate `f`, converting a host-level `JsError` into a thrown JS value.
    fn catchable<F>(&mut self, f: F) -> JsResult<Flow>
    where
        F: FnOnce(&mut Self) -> JsResult<Flow>,
    {
        match f(self) {
            Ok(flow) => Ok(flow),
            Err(JsError::Flow(flow)) => Ok(flow),
            Err(e) => {
                let v = self.error_to_value(&e);
                Ok(Flow::Throw(v))
            }
        }
    }

    /// Bind `value` against a destructuring `pattern`, defining each leaf
    /// identifier in the current environment. Used by variable declarations
    /// and function parameter binding.
    pub(crate) fn bind_pattern(
        &mut self,
        pattern: &Pattern,
        value: Value,
        mutable: bool,
    ) -> JsResult<()> {
        match pattern {
            Pattern::Identifier(name) => {
                self.env
                    .borrow_mut()
                    .define(name.clone(), value.clone(), mutable);
                if self.env.borrow().parent.is_none() {
                    self.global.borrow_mut().props.insert(name.clone(), value);
                }
                Ok(())
            }
            Pattern::Default(inner, default) => {
                let value = if matches!(value, Value::Undefined) {
                    self.eval_expr(default)?
                } else {
                    value
                };
                self.bind_pattern(inner, value, mutable)
            }
            // A bare `Rest` only appears inside a container; binding it
            // directly treats the incoming value as a single element.
            Pattern::Rest(inner) => self.bind_pattern(inner, value, mutable),
            // `a.b = v`, `a[0] = v` — non-identifier assignment targets.
            Pattern::AssignTarget(expr) => {
                match expr {
                    Expr::Index { object, index } => {
                        let obj = self.eval_expr(object)?;
                        let key = self.eval_expr(index)?;
                        if let Value::Object(obj_ref) = &obj {
                            let key_str = key.to_string();
                            self.set_property(obj_ref, &key_str, value);
                        }
                        Ok(())
                    }
                    Expr::Member { object, property, .. } => {
                        let obj = self.eval_expr(object)?;
                        if let Value::Object(obj_ref) = &obj {
                            self.set_property(obj_ref, property, value);
                        }
                        Ok(())
                    }
                    _ => Ok(()),
                }
            }
            Pattern::ArrayPattern(elements) => {
                let mut index = 0usize;
                for element in elements {
                    if let Pattern::Rest(inner) = element {
                        let rest = self.collect_rest_array(&value, index);
                        self.bind_pattern(inner, rest, mutable)?;
                        break;
                    }
                    let item = self.element_at(&value, index);
                    self.bind_pattern(element, item, mutable)?;
                    index += 1;
                }
                Ok(())
            }
            Pattern::ObjectPattern(entries) => {
                let mut consumed: Vec<String> = Vec::new();
                for entry in entries {
                    if entry.key == "..." {
                        if let Pattern::Rest(inner) = &entry.value {
                            let rest = self.collect_object_rest(&value, &consumed);
                            self.bind_pattern(inner, rest, mutable)?;
                        }
                        continue;
                    }
                    consumed.push(entry.key.clone());
                    let item = self.get_property_on_value(&value, &entry.key);
                    self.bind_pattern(&entry.value, item, mutable)?;
                }
                Ok(())
            }
        }
    }

    /// Length used for array destructuring: character count for strings,
    /// otherwise the value's `length` property.
    fn value_length(&self, value: &Value) -> usize {
        if let Value::String(s) = value {
            return s.chars().count();
        }
        let n = self.get_property_on_value(value, "length").to_number();
        if n.is_finite() && n > 0.0 {
            n as usize
        } else {
            0
        }
    }

    /// Extract element `index` for array destructuring.
    fn element_at(&self, value: &Value, index: usize) -> Value {
        if let Value::String(s) = value {
            return s
                .chars()
                .nth(index)
                .map(|c| Value::String(c.to_string()))
                .unwrap_or(Value::Undefined);
        }
        self.get_property_on_value(value, &index.to_string())
    }

    /// Collect the remaining elements of an array-like value into a new array.
    fn collect_rest_array(&self, value: &Value, start: usize) -> Value {
        let length = self.value_length(value);
        let mut items = Vec::new();
        for i in start..length {
            items.push(Some(self.element_at(value, i)));
        }
        let arr = Object::with_internal(Internal::Array(items));
        arr.borrow_mut().proto = Some(self.array_proto.clone());
        Value::Object(arr)
    }

    /// Collect the not-yet-consumed own enumerable properties into a new
    /// plain object (object rest).
    fn collect_object_rest(&self, value: &Value, consumed: &[String]) -> Value {
        let result = Object::plain();
        result.borrow_mut().proto = Some(self.object_proto.clone());
        if let Value::Object(source) = value {
            let source = source.borrow();
            for (key, val) in &source.props {
                if !consumed.contains(key) && !source.non_enumerable_props.contains(key) {
                    result.borrow_mut().props.insert(key.clone(), val.clone());
                }
            }
            if let Internal::Array(items) = &source.internal {
                for (i, item) in items.iter().enumerate() {
                    let key = i.to_string();
                    if !consumed.contains(&key)
                        && let Some(val) = item
                    {
                        result.borrow_mut().props.insert(key, val.clone());
                    }
                }
            }
        }
        Value::Object(result)
    }

    /// Evaluate a call argument list, flattening `...spread` arguments.
    fn eval_call_args(&mut self, args: &[Expr]) -> JsResult<Vec<Value>> {
        let mut out = Vec::new();
        for arg in args {
            match arg {
                Expr::Spread(inner) => {
                    let v = self.eval_expr(inner)?;
                    if let Value::Object(o) = &v {
                        if let Internal::Array(items) = &o.borrow().internal {
                            for item in items {
                                out.push(item.clone().unwrap_or(Value::Undefined));
                            }
                        } else {
                            out.push(v.clone());
                        }
                    } else {
                        out.push(v);
                    }
                }
                other => out.push(self.eval_expr(other)?),
            }
        }
        Ok(out)
    }

    fn eval_expr(&mut self, expr: &Expr) -> JsResult<Value> {
        self.step()?;
        match expr {
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::BigInt(s) => Ok(Value::BigInt(s.clone())),
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
                let mut values: Vec<Option<Value>> = Vec::with_capacity(items.len());
                for e in items {
                    match e {
                        None => values.push(None),
                        // Spread element: `[...iterable]` — flatten arrays.
                        Some(Expr::Spread(inner)) => {
                            let v = self.eval_expr(inner)?;
                            if let Value::Object(o) = &v {
                                if let Internal::Array(items) = &o.borrow().internal {
                                    for item in items {
                                        values.push(Some(item.clone().unwrap_or(Value::Undefined)));
                                    }
                                    continue;
                                }
                            }
                            values.push(Some(v));
                        }
                        Some(expr) => values.push(Some(self.eval_expr(expr)?)),
                    }
                }
                let obj = Object::with_internal(Internal::Array(values));
                obj.borrow_mut().proto = Some(self.array_proto.clone());
                Ok(Value::Object(obj))
            }
            Expr::Object(props) => {
                let obj = Object::plain();
                obj.borrow_mut().proto = Some(self.object_proto.clone());
                for (k, e) in props {
                    match k {
                        // Spread: `...expr` — copy own enumerable properties.
                        None => {
                            let v = self.eval_expr(e)?;
                            if let Value::Object(src) = &v {
                                for (pk, pv) in src.borrow().props.clone() {
                                    obj.borrow_mut().props.insert(pk, pv);
                                }
                            }
                        }
                        // Computed key: `computed:<expr>` — evaluate the key.
                        Some(key) if key.starts_with("computed:") => {
                            let v = self.eval_expr(e)?;
                            obj.borrow_mut().props.insert(key.clone(), v);
                        }
                        // Accessor: `__accessor__<name>` — install as getter/setter.
                        Some(key) if key.starts_with("__accessor__") => {
                            let name = key.strip_prefix("__accessor__").unwrap_or("");
                            let v = self.eval_expr(e)?;
                            // Determine if it's a getter (no params) or setter (1 param).
                            let is_getter = if let Value::Object(fobj) = &v {
                                if let Internal::Function { params, .. } = &fobj.borrow().internal {
                                    params.is_empty()
                                } else {
                                    false
                                }
                            } else {
                                false
                            };
                            let accessor_key = if is_getter {
                                format!("__get_{}", name)
                            } else {
                                format!("__set_{}", name)
                            };
                            Interpreter::define_non_enumerable(&obj, &accessor_key, v);
                        }
                        // Normal key.
                        Some(key) => {
                            let v = self.eval_expr(e)?;
                            obj.borrow_mut().props.insert(key.clone(), v);
                        }
                    }
                }
                Ok(Value::Object(obj))
            }
            Expr::Function {
                name,
                params,
                body,
                generator,
            } => {
                let value = self.make_function_async(params.clone(), body.clone(), false, *generator);
                Self::set_function_name(&value, name.as_deref().unwrap_or(""));
                Ok(value)
            }
            Expr::ArrowFunction { params, body } => {
                let obj = Object::with_internal(Internal::Function {
                    params: params.clone(),
                    body: body.clone(),
                    func_index: 0,
                    is_async: false,
                    generator: false,
                });
                obj.borrow_mut().proto = Some(self.function_proto.clone());
                let proto = Object::plain();
                proto.borrow_mut().proto = Some(self.object_proto.clone());
                Self::define_non_enumerable(&proto, "constructor", Value::Object(obj.clone()));
                Self::define_non_enumerable(&obj, "prototype", Value::Object(proto));
                let arity = params
                    .iter()
                    .take_while(|p| !matches!(p, Pattern::Rest(_)))
                    .count();
                Self::define_non_enumerable(&obj, "length", Value::Number(arity as f64));
                Self::define_non_enumerable(&obj, "name", Value::String(String::new()));
                let value = Value::Object(obj.clone());
                self.remember_closure(&value);
                Ok(value)
            }
            Expr::AsyncFunction {
                name,
                params,
                body,
                generator,
            } => {
                let value = self.make_function_async(params.clone(), body.clone(), true, *generator);
                Self::set_function_name(&value, name.as_deref().unwrap_or(""));
                Ok(value)
            }
            Expr::Yield(argument) => {
                // Evaluate the yielded operand. If we are resuming a
                // generator and this is the yield being awaited, record it in
                // `pending_yield`; the enclosing statement then unwinds with
                // `Flow::Yield`. Otherwise the value is discarded and
                // execution continues to the next yield.
                let value = self.eval_expr(argument)?;
                if let Some(target) = self.yield_target {
                    let index = self.yield_counter;
                    self.yield_counter += 1;
                    if index == target {
                        self.pending_yield = Some(value.clone());
                    }
                }
                Ok(value)
            }
            Expr::Await(expr) => {
                // In a synchronous interpreter, `await` just evaluates the
                // expression. If the result is a Promise (native), we return
                // it as-is for now — full async scheduling is out of scope.
                let v = self.eval_expr(expr)?;
                Ok(v)
            }
            Expr::Assign { target, value } => {
                let value = self.eval_expr(value)?;
                self.assign_target(target, value.clone())?;
                Ok(value)
            }
            Expr::DestructuringAssign { pattern, value } => {
                // Evaluate the right-hand side, then bind each pattern leaf.
                // The assignment expression itself evaluates to the RHS so it
                // chains like `result = [a, b] = [1, 2]`.
                let rhs = self.eval_expr(value)?;
                self.bind_pattern(pattern, rhs.clone(), true)?;
                Ok(rhs)
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
            Expr::TemplateLiteral { parts } => {
                let mut result = String::new();
                for part in parts {
                    result.push_str(&self.eval_expr(part)?.to_string());
                }
                Ok(Value::String(result))
            }
            Expr::RegExp { pattern, flags } => {
                let obj = Object::plain();
                obj.borrow_mut().proto = Some(self.object_proto.clone());
                let mut p = obj.borrow_mut();
                p.props.insert("source".into(), Value::String(pattern.clone()));
                p.props.insert("flags".into(), Value::String(flags.clone()));
                p.props.insert("lastIndex".into(), Value::Number(0.0));
                drop(p);
                for method in ["test", "exec"] {
                    Self::define_non_enumerable(&obj, method, self.native_method("RegExp.prototype."));
                }
                Ok(Value::Object(obj))
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
                    UnaryOp::Plus => Ok(Value::Number(value.to_number())),
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
                let args = self.eval_call_args(args)?;
                self.call(callee_value, args, this_value, false)
            }
            Expr::New { callee, args } => {
                let callee = self.eval_expr(callee)?;
                let args = self.eval_call_args(args)?;
                self.call(callee, args, Value::Undefined, true)
            }
            Expr::Class {
                name,
                extends,
                body,
            } => self.make_class(name.clone(), extends, body),
            Expr::NewTarget => Ok(Value::Undefined),
            Expr::Super => Err(JsError::syntax_error("'super' keyword unexpected here")),
            // Spread is only valid in call arguments / array literals; as a
            // standalone expression it evaluates to its operand.
            Expr::Spread(inner) => self.eval_expr(inner),
            // Comma operator: evaluate all, return the last.
            Expr::Sequence(exprs) => {
                let mut last = Value::Undefined;
                for e in exprs {
                    last = self.eval_expr(e)?;
                }
                Ok(last)
            }
            Expr::Member { .. } | Expr::Index { .. } => self.get_target(expr),
        }
    }

    fn eval_callee(&mut self, expr: &Expr) -> JsResult<(Value, Value)> {
        match expr {
            Expr::Super => {
                let parent = self
                    .env
                    .borrow()
                    .get("__super__")
                    .unwrap_or(Value::Undefined);
                let this = self
                    .env
                    .borrow()
                    .get("this")
                    .unwrap_or(Value::Undefined);
                Ok((parent, this))
            }
            Expr::Member { object, property } if matches!(object.as_ref(), Expr::Super) => {
                let parent = self
                    .env
                    .borrow()
                    .get("__super__")
                    .unwrap_or(Value::Undefined);
                let proto = self.get_property_on_value(&parent, "prototype");
                let method = self.get_property_on_value(&proto, property);
                let this = self
                    .env
                    .borrow()
                    .get("this")
                    .unwrap_or(Value::Undefined);
                Ok((method, this))
            }
            Expr::Member { object, property } => {
                let object_value = self.eval_expr(object)?;
                self.deny_strict_arguments_callee_value(&object_value, property)?;
                let method = self.get_property_on_value(&object_value, property);
                Ok((method, object_value))
            }
            Expr::Index { object, index } => {
                let object_value = self.eval_expr(object)?;
                let key = self.eval_expr(index)?.to_string();
                self.deny_strict_arguments_callee_value(&object_value, &key)?;
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
                    self.deny_strict_arguments_callee(&o, property)?;
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
                    self.deny_strict_arguments_callee(&o, &property)?;
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
                self.deny_strict_arguments_callee_value(&obj, property)?;
                self.read_property(&obj, property)
            }
            Expr::Index { object, index } => {
                let obj = self.eval_expr(object)?;
                let key = self.eval_expr(index)?.to_string();
                self.deny_strict_arguments_callee_value(&obj, &key)?;
                self.read_property(&obj, &key)
            }
            _ => self.eval_expr(target),
        }
    }

    /// Read a property, invoking a class-style getter (`__get_<name>`) when one
    /// exists along the prototype chain.
    fn read_property(&mut self, object: &Value, key: &str) -> JsResult<Value> {
        if let Value::Object(o) = object {
            let getter_key = format!("__get_{key}");
            if let Some(getter) = Self::lookup_proto(o, &getter_key) {
                return self.call(getter, Vec::new(), object.clone(), false);
            }
        }
        Ok(self.get_property_on_value(object, key))
    }

    /// Walk an object's prototype chain looking for `key`.
    pub(crate) fn lookup_proto(object: &ObjectRef, key: &str) -> Option<Value> {
        let mut current = Some(object.clone());
        while let Some(o) = current {
            if let Some(value) = o.borrow().props.get(key).cloned() {
                return Some(value);
            }
            current = o.borrow().proto.clone();
        }
        None
    }

    /// Record the parent class on a method function so that `super` resolves
    /// correctly when the method runs.
    pub(crate) fn tag_super(function: &Value, parent: &Option<Value>) {
        if let Value::Object(func) = function {
            Self::define_non_enumerable(
                func,
                "__super__",
                parent.clone().unwrap_or(Value::Undefined),
            );
        }
    }

    fn assign_target(&mut self, target: &Expr, value: Value) -> JsResult<()> {
        match target {
            Expr::Identifier(name) => {
                // Strict mode: `arguments` is immutable.
                if self.strict && name == "arguments" {
                    return Err(JsError::syntax_error(
                        "in strict mode code, functions may not be invoked with 'arguments' reassigned",
                    ));
                }
                self.env.borrow_mut().assign(name, value)
            }
            Expr::Member { .. } | Expr::Index { .. } => {
                let r = self.get_ref(target)?;
                self.write_property(&r.object, &r.property, value)
            }
            _ => Err(JsError::type_error("target is not assignable")),
        }
    }

    /// Assign a property, invoking a class-style setter (`__set_<name>`) when
    /// one exists along the prototype chain.
    fn write_property(&mut self, object: &ObjectRef, key: &str, value: Value) -> JsResult<()> {
        let setter_key = format!("__set_{key}");
        if let Some(setter) = Self::lookup_proto(object, &setter_key) {
            self.call(setter, vec![value], Value::Object(object.clone()), false)?;
            return Ok(());
        }
        self.set_property(object, key, value);
        Ok(())
    }

    pub(crate) fn get_property(&self, object: &ObjectRef, property: &str) -> Value {
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

    pub(crate) fn get_property_on_value(&self, value: &Value, property: &str) -> Value {
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

    /// A strict-mode `arguments` object exposes `callee` as a poison-pill
    /// accessor: any read or write throws a TypeError rather than exposing
    /// the enclosing function. Reject such access for an extracted object.
    fn deny_strict_arguments_callee(&self, object: &ObjectRef, property: &str) -> JsResult<()> {
        if property == "callee" && object.borrow().is_strict_arguments {
            return Err(JsError::type_error(
                "'callee' is not allowed in strict mode",
            ));
        }
        Ok(())
    }

    /// [`deny_strict_arguments_callee`] for call sites that only hold a value.
    fn deny_strict_arguments_callee_value(&self, value: &Value, property: &str) -> JsResult<()> {
        if let Value::Object(object) = value {
            self.deny_strict_arguments_callee(object, property)?;
        }
        Ok(())
    }

    pub(crate) fn set_property(&self, object: &ObjectRef, property: &str, value: Value) {
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
        let mut object = object.borrow_mut();
        object.props.insert(property.to_string(), value);
        // A plain-object assignment shadows a prototype property; the property
        // is now own+enumerable, so drop any inherited non-enumerable marker.
        object.non_enumerable_props.remove(property);
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
        let writable = !object
            .non_enumerable_props
            .contains(&("__writable_".to_string() + property));
        let configurable = !object
            .non_enumerable_props
            .contains(&("__configurable_".to_string() + property));
        object
            .props
            .get(property)
            .cloned()
            .map(|value| {
                let enumerable = !object.non_enumerable_props.contains(property);
                self.data_descriptor(value, writable, enumerable, configurable)
            })
            .unwrap_or(Value::Undefined)
    }

    fn binary(&mut self, left: Value, op: BinaryOp, right: Value) -> JsResult<Value> {
        match op {
            BinaryOp::Add => {
                // ToPrimitive both sides first (string concat vs numeric add).
                let lp = self.to_primitive_value(&left)?;
                let rp = self.to_primitive_value(&right)?;
                match (&lp, &rp) {
                    (Value::String(a), b) => Ok(Value::String(a.clone() + &b.to_string())),
                    (a, Value::String(b)) => Ok(Value::String(a.to_string() + b)),
                    (a, b) => Ok(Value::Number(a.to_number() + b.to_number())),
                }
            }
            BinaryOp::Subtract => Ok(Value::Number(left.to_number() - right.to_number())),
            BinaryOp::Multiply => Ok(Value::Number(left.to_number() * right.to_number())),
            BinaryOp::Divide => Ok(Value::Number(left.to_number() / right.to_number())),
            BinaryOp::Remainder => Ok(Value::Number(left.to_number() % right.to_number())),
            BinaryOp::Equal => Ok(Value::Bool(self.abstract_eq_js(&left, &right)?)),
            BinaryOp::NotEqual => Ok(Value::Bool(!self.abstract_eq_js(&left, &right)?)),
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

    pub(crate) fn instanceof(&self, left: Value, right: Value) -> bool {
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
        // Bridge: when the VM owns this interpreter, route the call through
        // the VM so VM-compiled functions can be invoked from native methods.
        if self.call_host.is_some() {
            let mut host = self.call_host.take().unwrap();
            let result = host(self, callee, args, this_value, construct);
            self.call_host = Some(host);
            return result;
        }
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
            Internal::Function {
                params,
                body,
                generator,
                ..
            } => {
                if generator {
                    // Calling a generator does not run its body; instead it
                    // returns a fresh suspended generator object.
                    let closure_env = self
                        .closures
                        .get(&(Rc::as_ptr(&func) as usize))
                        .cloned()
                        .unwrap_or_else(|| self.env.clone());
                    return Ok(self.make_generator(params, body, closure_env, args, this_value));
                }
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
                let super_value = func
                    .borrow()
                    .props
                    .get("__super__")
                    .cloned()
                    .unwrap_or(Value::Undefined);
                self.env
                    .borrow_mut()
                    .define("__super__".into(), super_value, false);
                for (index, pattern) in params.iter().enumerate() {
                    if let Pattern::Rest(inner) = pattern {
                        let rest: Vec<Option<Value>> = args
                            .iter()
                            .skip(index)
                            .map(|v| Some(v.clone()))
                            .collect();
                        let arr = Object::with_internal(Internal::Array(rest));
                        arr.borrow_mut().proto = Some(self.array_proto.clone());
                        self.bind_pattern(inner, Value::Object(arr), true)?;
                        break;
                    }
                    let value = args.get(index).cloned().unwrap_or(Value::Undefined);
                    self.bind_pattern(pattern, value, true)?;
                }
                // Build the `arguments` object (constructors have one too).
                {
                    let slots: Vec<Option<Value>> = args.iter().map(|v| Some(v.clone())).collect();
                    let args_obj = Object::with_internal(Internal::Array(slots));
                    args_obj.borrow_mut().proto = Some(self.object_proto.clone());
                    if self.strict {
                        // Strict-mode `arguments.callee` is a poison-pill
                        // accessor: any read or write throws a TypeError.
                        args_obj.borrow_mut().is_strict_arguments = true;
                    } else {
                        Interpreter::define_non_enumerable(
                            &args_obj,
                            "callee",
                            Value::Object(func.clone()),
                        );
                    }
                    self.env
                        .borrow_mut()
                        .define("arguments".into(), Value::Object(args_obj), true);
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
                    Flow::Throw(v) => Err(JsError::Flow(Flow::Throw(v))),
                    Flow::Break => Err(JsError::syntax_error("break used outside loop")),
                    Flow::Continue => Err(JsError::syntax_error("continue used outside loop")),
                    Flow::Yield(_) => Err(JsError::syntax_error("yield used outside generator")),
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

    pub(crate) fn call_native(
        &mut self,
        name: &'static str,
        args: Vec<Value>,
        this_value: Value,
        construct: Option<ObjectRef>,
    ) -> JsResult<Value> {
        match name {
            "print" => {
                let v = args.first().cloned().unwrap_or(Value::Undefined);
                let s = self.to_string_value(&v)?;
                self.push_output(s);
                Ok(Value::Undefined)
            }
            "eval" => {
                let code = args
                    .first()
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                let tokens = crate::lexer::lex(&code)
                    .map_err(|e| JsError::syntax_error(e.to_string()))?;
                let program = crate::parser::parse(tokens)
                    .map_err(|e| JsError::syntax_error(e.to_string()))?;
                // Save state, execute, restore.
                let saved_env = self.env.clone();
                let saved_strict = self.strict;
                let saved_output = std::mem::take(&mut self.output);
                let result = self.eval_statements(&program.statements);
                self.env = saved_env;
                self.strict = saved_strict;
                self.output = saved_output;
                match result {
                    Ok(Flow::Value(v)) => Ok(v),
                    Ok(Flow::Return(v)) => Ok(v),
                    Ok(Flow::Throw(v)) => Err(JsError::Flow(Flow::Throw(v))),
                Ok(Flow::Break) => Err(JsError::syntax_error("break used outside loop")),
                Ok(Flow::Continue) => Err(JsError::syntax_error("continue used outside loop")),
                Ok(Flow::Yield(_)) => Err(JsError::syntax_error("yield used outside generator")),
                Err(e) => Err(e),
                }
            }
            "Generator.prototype.next" => {
                // Drives a generator object created by `make_generator`.
                if let Value::Object(generator) = &this_value {
                    let key = Rc::as_ptr(generator) as usize;
                    return self.resume_generator(key);
                }
                Ok(Self::iterator_result(Value::Undefined, true))
            }
            "Error" | "TypeError" | "SyntaxError" | "ReferenceError" | "RangeError" | "EvalError" | "URIError" => {
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
                // Own `constructor` so `err.constructor === TypeError` holds.
                if let Some(ctor) = self.env.borrow().get(name) {
                    obj.borrow_mut().props.insert("constructor".into(), ctor);
                }
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
                let desc = descriptor.borrow();
                let value = desc.props.get("value").cloned();
                let writable = desc
                    .props
                    .get("writable")
                    .map_or(true, |v| v.is_truthy());
                let enumerable = desc
                    .props
                    .get("enumerable")
                    .map_or(false, |v| v.is_truthy());
                let configurable = desc
                    .props
                    .get("configurable")
                    .map_or(true, |v| v.is_truthy());
                drop(desc);

                if let Some(value) = value {
                    // Reject changing a non-configurable own property.
                    if target.borrow().props.contains_key(&key)
                        && target.borrow().non_enumerable_props.contains(&("__configurable_".to_string() + &key))
                    {
                        return Err(JsError::type_error(format!(
                            "cannot redefine non-configurable property `{key}`"
                        )));
                    }
                    self.set_property(&target, &key, value);
                    let mut target = target.borrow_mut();
                    if !writable {
                        target
                            .non_enumerable_props
                            .insert("__writable_".to_string() + &key);
                    }
                    if !enumerable {
                        target.non_enumerable_props.insert(key.clone());
                    }
                    if !configurable {
                        target
                            .non_enumerable_props
                            .insert("__configurable_".to_string() + &key);
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
                    }
                    Value::Null => {}
                    _ => return Err(JsError::type_error("Object.create expects object or null")),
                }
                // Second argument: a properties object whose own
                // enumerable data descriptors are defined on the result.
                if let Some(Value::Object(props)) = args.get(1).cloned() {
                    let desc = props.borrow();
                    for (k, v) in desc.props.iter() {
                        if desc.non_enumerable_props.contains(k) {
                            continue;
                        }
                        if let Value::Object(d) = v {
                            let d = d.borrow();
                            let value = d.props.get("value").cloned();
                            let writable = d.props.get("writable")
                                .map_or(true, |vv| vv.is_truthy());
                            let enumerable = d.props.get("enumerable")
                                .map_or(false, |vv| vv.is_truthy());
                            let configurable = d.props.get("configurable")
                                .map_or(true, |vv| vv.is_truthy());
                            drop(d);
                            if let Some(value) = value {
                                obj.borrow_mut().props.insert(k.clone(), value);
                                let mut o = obj.borrow_mut();
                                if !writable {
                                    o.non_enumerable_props.insert("__writable_".to_string() + k);
                                }
                                if !enumerable {
                                    o.non_enumerable_props.insert(k.clone());
                                }
                                if !configurable {
                                    o.non_enumerable_props.insert("__configurable_".to_string() + k);
                                }
                            }
                        }
                    }
                }
                Ok(Value::Object(obj))
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
            "String" => {
                let v = args.first().cloned().unwrap_or(Value::Undefined);
                Ok(Value::String(self.to_string_value(&v)?))
            }
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
                    // Re-throw the original value so an enclosing try/catch
                    // receives it unchanged.
                    Flow::Throw(v) => Err(JsError::Flow(Flow::Throw(v))),
                    Flow::Break => Err(JsError::syntax_error("break used outside loop")),
                    Flow::Continue => Err(JsError::syntax_error("continue used outside loop")),
                    Flow::Yield(_) => Err(JsError::syntax_error("yield used outside generator")),
                }
            },
            "Function" => {
                let body = args.last().cloned().unwrap_or(Value::Undefined).to_string();
                let params: Vec<String> = args.iter().take(args.len().saturating_sub(1))
                    .map(|v| v.to_string())
                    .collect();
                // Wrap the body in an anonymous function expression and parse it.
                let code = if params.is_empty() {
                    format!("(function(){{ {} }})", body)
                } else {
                    format!("(function({}){{ {} }})", params.join(","), body)
                };
                let tokens = crate::lexer::lex(&code)
                    .map_err(|e| JsError::syntax_error(e.to_string()))?;
                let program = crate::parser::parse(tokens)
                    .map_err(|e| JsError::syntax_error(e.to_string()))?;
                for stmt in &program.statements {
                    if let Stmt::Expr(Expr::Function { params, body, .. }) = stmt {
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
            "Object.prototype.valueOf" => Ok(this_value.clone()),
            "Object.prototype.toLocaleString" => {
                // Delegate to the receiver's own/inherited `toString`.
                let method = self.get_property_on_value(&this_value, "toString");
                self.call(method, Vec::new(), this_value.clone(), false)
            }
            "Object.prototype.isPrototypeOf" => {
                let receiver = args.first().cloned().unwrap_or(Value::Undefined);
                let Value::Object(proto_obj) = this_value else {
                    return Ok(Value::Bool(false));
                };
                let receiver_obj = match &receiver {
                    Value::Object(o) => o.clone(),
                    _ => return Ok(Value::Bool(false)),
                };
                // Walk the prototype chain of receiver_obj to check if proto_obj is in it
                let mut current = Some(receiver_obj.clone());
                while let Some(obj) = current {
                    if Rc::ptr_eq(&obj, &proto_obj) {
                        return Ok(Value::Bool(true));
                    }
                    current = obj.borrow().proto.clone();
                }
                Ok(Value::Bool(false))
            }
            "Array.prototype.toString" => {
                // Array.prototype.toString === join with "," separator.
                if let Value::Object(o) = &this_value
                    && let Internal::Array(items) = &o.borrow().internal
                {
                    let mut parts = Vec::with_capacity(items.len());
                    for v in items {
                        let s = match v {
                            None | Some(Value::Null) | Some(Value::Undefined) => String::new(),
                            Some(other) => self.to_string_value(other)?,
                        };
                        parts.push(s);
                    }
                    return Ok(Value::String(parts.join(",")));
                }
                Ok(Value::String(String::new()))
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
            "Array.prototype.at" => {
                let index = args.first().map(|v| v.to_number()).unwrap_or(f64::NAN);
                if let Value::Object(o) = this_value
                    && let Internal::Array(items) = &o.borrow().internal
                {
                    let len = items.len() as isize;
                    let pos = if index.is_nan() {
                        0
                    } else if index < 0.0 {
                        len + index as isize
                    } else {
                        index as isize
                    };
                    if pos >= 0 && pos < len {
                        return Ok(items[pos as usize].clone().unwrap_or(Value::Undefined));
                    }
                }
                Ok(Value::Undefined)
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
            "String.prototype.codePointAt" => {
                let s = self.this_str(&this_value);
                let pos = args.first().map(|v| v.to_number()).unwrap_or(f64::NAN);
                if pos.is_nan() || pos < 0.0 || pos >= s.len() as f64 {
                    return Ok(Value::Undefined);
                }
                let pos = pos as usize;
                let mut chars = s.chars().skip(pos);
                let Some(first) = chars.next() else {
                    return Ok(Value::Undefined);
                };
                let code_point = if (0xD800..0xDC00).contains(&(first as u32))
                    && let Some(second) = chars.next()
                    && (0xDC00..0xE000).contains(&(second as u32))
                {
                    0x10000
                        + ((first as u32 - 0xD800) << 10)
                        + (second as u32 - 0xDC00)
                } else {
                    first as u32
                };
                Ok(Value::Number(code_point as f64))
            }
            "String.prototype.at" => {
                let s = self.this_str(&this_value);
                let len = s.len() as isize;
                let index = args.first().map(|v| v.to_number()).unwrap_or(f64::NAN);
                let pos = if index.is_nan() {
                    0
                } else if index < 0.0 {
                    len + index as isize
                } else {
                    index as isize
                };
                if pos >= 0 && pos < len {
                    Ok(Value::String(s.chars().nth(pos as usize).unwrap_or(' ').to_string()))
                } else {
                    Ok(Value::String(String::new()))
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
            "String.prototype.replaceAll" => {
                let s = self.this_str(&this_value);
                let needle = args.first().cloned().unwrap_or(Value::Undefined).to_string();
                let replacement = args.get(1).cloned().unwrap_or(Value::Undefined).to_string();
                if needle.is_empty() {
                    return Ok(Value::String(s));
                }
                let mut result = String::new();
                let mut rest = s.as_str();
                while let Some(pos) = rest.find(&needle) {
                    result.push_str(&rest[..pos]);
                    result.push_str(&replacement);
                    rest = &rest[pos + needle.len()..];
                }
                result.push_str(rest);
                Ok(Value::String(result))
            }
            "String.prototype.localeCompare" => {
                let s = self.this_str(&this_value);
                let other = args
                    .first()
                    .cloned()
                    .unwrap_or(Value::Undefined)
                    .to_string();
                let result = s.cmp(&other);
                Ok(Value::Number(if result == std::cmp::Ordering::Equal {
                    0.0
                } else if result == std::cmp::Ordering::Less {
                    -1.0
                } else {
                    1.0
                }))
            }
            "String.prototype.search" => {
                let s = self.this_str(&this_value);
                let pattern = if let Some(Value::Object(o)) = args.first() {
                    o.borrow()
                        .props
                        .get("source")
                        .cloned()
                        .unwrap_or(Value::String(String::new()))
                        .to_string()
                } else {
                    args.first().cloned().unwrap_or(Value::Undefined).to_string()
                };
                if pattern.is_empty() {
                    return Ok(Value::Number(0.0));
                }
                match s.find(&pattern) {
                    Some(pos) => Ok(Value::Number(pos as f64)),
                    None => Ok(Value::Number(-1.0)),
                }
            }
            "String.prototype.match" => {
                let s = self.this_str(&this_value);
                let pattern = if let Some(Value::Object(o)) = args.first() {
                    o.borrow()
                        .props
                        .get("source")
                        .cloned()
                        .unwrap_or(Value::String(String::new()))
                        .to_string()
                } else {
                    args.first().cloned().unwrap_or(Value::Undefined).to_string()
                };
                if pattern.is_empty() {
                    return Ok(Value::Null);
                }
                match s.find(&pattern) {
                    Some(pos) => {
                        let end = pos + pattern.len();
                        let matched = s[pos..end].to_string();
                        let obj = Object::with_internal(Internal::Array(vec![
                            Some(Value::String(matched.clone())),
                        ]));
                        obj.borrow_mut().proto = Some(self.array_proto.clone());
                        obj.borrow_mut().props.insert("index".into(), Value::Number(pos as f64));
                        obj.borrow_mut().props.insert("input".into(), Value::String(s.clone()));
                        Ok(Value::Object(obj))
                    }
                    None => Ok(Value::Null),
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
                        // Walk the prototype chain so inherited enumerable
                        // own properties are copied (spec: ownKeys of the source
                        // object, which includes inherited ones for assign).
                        let mut current = Some(src.clone());
                        while let Some(s) = current {
                            let s = s.borrow();
                            for key in s.props.keys() {
                                if !s.non_enumerable_props.contains(key) {
                                    target
                                        .borrow_mut()
                                        .props
                                        .insert(key.clone(), s.props[key].clone());
                                }
                            }
                            current = s.proto.clone();
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
            "Object.fromEntries" => {
                let obj = Object::plain();
                obj.borrow_mut().proto = Some(self.object_proto.clone());
                if let Value::Object(entries) = args.first().cloned().unwrap_or(Value::Undefined) {
                    let items = if let Internal::Array(items) = &entries.borrow().internal {
                        items.clone()
                    } else {
                        Vec::new()
                    };
                    for pair in items {
                        let Some(Value::Object(pair_obj)) = pair else {
                            continue;
                        };
                        let pair = pair_obj.borrow();
                        if let Internal::Array(pair_items) = &pair.internal {
                            if let (Some(key_v), Some(value_v)) = (pair_items.first(), pair_items.get(1)) {
                                if let (Some(key), Some(value)) = (key_v.as_ref(), value_v.as_ref()) {
                                    let key = key.to_string();
                                    let mut target = obj.borrow_mut();
                                    if !target.non_enumerable_props.contains(&key) {
                                        target.props.insert(key, value.clone());
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(Value::Object(obj))
            }
            "Object.defineProperties" => {
                let Some(Value::Object(target)) = args.first().cloned() else {
                    return Err(JsError::type_error("Object.defineProperties expects object"));
                };
                let Some(Value::Object(properties)) = args.get(1).cloned() else {
                    return Err(JsError::type_error(
                        "Object.defineProperties expects properties object",
                    ));
                };
                let descriptors: Vec<(String, Value)> = {
                    let props = properties.borrow();
                    props
                        .props
                        .iter()
                        .filter_map(|(key, value)| {
                            if let Value::Object(descriptor) = value {
                                Some((key.clone(), Value::Object(descriptor.clone())))
                            } else {
                                None
                            }
                        })
                        .collect()
                };
                for (key, descriptor) in descriptors {
                    self.call_native(
                        "Object.defineProperty",
                        vec![
                            Value::Object(target.clone()),
                            Value::String(key),
                            descriptor,
                        ],
                        Value::Undefined,
                        None,
                    )?;
                }
                Ok(Value::Object(target))
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
            "Number.isSafeInteger" => {
                let max_safe = 9007199254740991.0;
                Ok(Value::Bool(
                    args.first().map(|v| {
                        let n = v.to_number();
                        n.is_finite() && n.fract() == 0.0 && n.abs() <= max_safe
                    }).unwrap_or(false),
                ))
            }
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
            "RegExp.prototype.test" => {
                let input = args.first().cloned().unwrap_or(Value::Undefined).to_string();
                let pattern = if let Value::Object(ref o) = this_value {
                    o.borrow().props.get("source").cloned().unwrap_or(Value::String(String::new())).to_string()
                } else { String::new() };
                let flags = if let Value::Object(ref o) = this_value {
                    o.borrow().props.get("flags").cloned().unwrap_or(Value::String(String::new())).to_string()
                } else { String::new() };
                let case_insensitive = flags.contains('i');
                let found = if case_insensitive {
                    input.to_lowercase().contains(&pattern.to_lowercase())
                } else {
                    input.contains(&pattern)
                };
                Ok(Value::Bool(found))
            }
            "RegExp.prototype.exec" => {
                let input = args.first().cloned().unwrap_or(Value::Undefined).to_string();
                let pattern = if let Value::Object(ref o) = this_value {
                    o.borrow().props.get("source").cloned().unwrap_or(Value::String(String::new())).to_string()
                } else { String::new() };
                let flags = if let Value::Object(ref o) = this_value {
                    o.borrow().props.get("flags").cloned().unwrap_or(Value::String(String::new())).to_string()
                } else { String::new() };
                let case_insensitive = flags.contains('i');
                let found = if case_insensitive {
                    input.to_lowercase().contains(&pattern.to_lowercase())
                } else {
                    input.contains(&pattern)
                };
                if found {
                    let pos = if case_insensitive {
                        input.to_lowercase().find(&pattern.to_lowercase()).unwrap_or(0)
                    } else {
                        input.find(&pattern).unwrap_or(0)
                    };
                    let result = vec![
                        Some(Value::String(input[pos..pos + pattern.len()].to_string())),
                        Some(Value::Number(pos as f64)),
                        Some(Value::String(input.clone())),
                    ];
                    let obj = Object::with_internal(Internal::Array(result));
                    obj.borrow_mut().proto = Some(self.array_proto.clone());
                    obj.borrow_mut().props.insert("index".into(), Value::Number(pos as f64));
                    obj.borrow_mut().props.insert("input".into(), Value::String(input));
                    Ok(Value::Object(obj))
                } else {
                    Ok(Value::Null)
                }
            }
            "Map.prototype.get" => {
                let key = args.first().cloned().unwrap_or(Value::Undefined);
                let key_str = key.to_string();
                if let Value::Object(ref o) = this_value {
                    let val = o.borrow().props.get(&key_str).cloned().unwrap_or(Value::Undefined);
                    return Ok(val);
                }
                Ok(Value::Undefined)
            }
            "Map.prototype.set" => {
                let key = args.first().cloned().unwrap_or(Value::Undefined);
                let value = args.get(1).cloned().unwrap_or(Value::Undefined);
                let key_str = key.to_string();
                if let Value::Object(ref o) = this_value {
                    o.borrow_mut().props.insert(key_str, value.clone());
                }
                Ok(this_value)
            }
            "Map.prototype.has" => {
                let key = args.first().cloned().unwrap_or(Value::Undefined);
                let key_str = key.to_string();
                Ok(Value::Bool(if let Value::Object(ref o) = this_value {
                    o.borrow().props.contains_key(&key_str)
                } else { false }))
            }
            "Reflect.ownKeys" => {
                let Some(Value::Object(target)) = args.first().cloned() else {
                    return Err(JsError::type_error("Reflect.ownKeys expects object"));
                };
                let obj = target.borrow();
                let mut keys: Vec<Option<Value>> = Vec::new();
                if let Internal::Array(items) = &obj.internal {
                    for (i, item) in items.iter().enumerate() {
                        if item.is_some() {
                            keys.push(Some(Value::String(i.to_string())));
                        }
                    }
                }
                for key in obj.props.keys() {
                    keys.push(Some(Value::String(key.clone())));
                }
                let arr = Object::with_internal(Internal::Array(keys));
                arr.borrow_mut().proto = Some(self.array_proto.clone());
                Ok(Value::Object(arr))
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
                Value::BigInt(s) => Ok(format!("\"{}\"", s)),
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

    pub(crate) fn this_str(&self, this_value: &Value) -> String {
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
    fn strict_mode_arguments_callee_access_throws_type_error() {
        let err = run_source("\"use strict\"; (function(){ return arguments.callee; })();")
            .unwrap_err();
        assert!(
            err.to_string().starts_with("TypeError"),
            "expected TypeError, got: {err}"
        );
    }

    #[test]
    fn strict_mode_arguments_callee_call_throws_type_error() {
        let err = run_source("\"use strict\"; (function(){ return arguments.callee(); })();")
            .unwrap_err();
        assert!(
            err.to_string().starts_with("TypeError"),
            "expected TypeError, got: {err}"
        );
    }

    #[test]
    fn strict_mode_arguments_callee_assignment_throws_type_error() {
        let err = run_source("\"use strict\"; (function(){ arguments.callee = 1; })();")
            .unwrap_err();
        assert!(
            err.to_string().starts_with("TypeError"),
            "expected TypeError, got: {err}"
        );
    }

    #[test]
    fn non_strict_arguments_callee_returns_enclosing_function() {
        assert_eq!(
            run_source("function f(){ return typeof arguments.callee; } f();").unwrap(),
            Value::String("function".into())
        );
    }

    #[test]
    fn non_strict_arguments_callee_is_the_enclosing_function() {
        assert_eq!(
            run_source("function f(){ return arguments.callee === f; } f();").unwrap(),
            Value::Bool(true)
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
    fn catch_without_parameter_still_runs_catch_block() {
        let src = r#"
            let out = "";
            try {
                throw 1;
            } catch {
                out = "caught";
            }
            out;
        "#;
        assert_eq!(run_source(src).unwrap(), Value::String("caught".into()));
    }

    #[test]
    fn throw_in_try_without_catch_propagates() {
        let error = run_source("try { throw 1; } finally { }").unwrap_err();
        assert!(error.to_string().contains("1"));
    }

    #[test]
    fn finally_runs_when_try_throws_without_catch() {
        let src = r#"
            let out = "";
            try {
                throw 1;
            } finally {
                out = "finally";
            }
            "after";
        "#;
        let error = run_source(src).unwrap_err();
        assert!(error.to_string().contains("1"));
    }

    #[test]
    fn finally_runs_when_catch_throws_and_replaces_throw() {
        let src = r#"
            let out = "";
            try {
                throw 1;
            } catch (e) {
                throw 2;
            } finally {
                out = "finally";
            }
            out;
        "#;
        let error = run_source(src).unwrap_err();
        assert!(error.to_string().contains("2"));
    }

    #[test]
    fn finally_throw_replaces_catch_throw() {
        let src = r#"
            try {
                throw 1;
            } catch (e) {
                throw 2;
            } finally {
                throw 3;
            }
        "#;
        let error = run_source(src).unwrap_err();
        assert!(error.to_string().contains("3"));
    }

    #[test]
    fn finally_throw_replaces_uncaught_throw() {
        let src = r#"
            try {
                throw 1;
            } finally {
                throw 3;
            }
        "#;
        let error = run_source(src).unwrap_err();
        assert!(error.to_string().contains("3"));
    }

    #[test]
    fn finally_runs_before_return_from_try() {
        let src = r#"
            function f() {
                try {
                    return 1;
                } finally {
                    print("fin");
                }
            }
            f();
        "#;
        let (value, output) = crate::run_source_with_output(src).unwrap();
        assert_eq!(value, Value::Number(1.0));
        assert_eq!(output, vec!["fin".to_string()]);
    }

    #[test]
    fn finally_return_replaces_try_return() {
        let src = r#"
            function f() {
                try {
                    return 1;
                } finally {
                    return 2;
                }
            }
            f();
        "#;
        assert_eq!(run_source(src).unwrap(), Value::Number(2.0));
    }

    #[test]
    fn finally_return_replaces_catch_return() {
        let src = r#"
            function f() {
                try {
                    throw 1;
                } catch (e) {
                    return 2;
                } finally {
                    return 3;
                }
            }
            f();
        "#;
        assert_eq!(run_source(src).unwrap(), Value::Number(3.0));
    }

    #[test]
    fn finally_throw_replaces_try_return() {
        let src = r#"
            function f() {
                try {
                    return 1;
                } finally {
                    throw 3;
                }
            }
            f();
        "#;
        let error = run_source(src).unwrap_err();
        assert!(error.to_string().contains("3"));
    }

    #[test]
    fn nested_try_finally_runs_then_inner_throw_wins() {
        let src = r#"
            let out = "";
            try {
                try {
                    throw 1;
                } catch (e) {
                    throw 2;
                } finally {
                    out = "inner";
                }
            } catch (e) {
                out = out + ":" + e;
            }
            out;
        "#;
        assert_eq!(run_source(src).unwrap(), Value::String("inner:2".into()));
    }

    #[test]
    fn throw_undefined_is_caught_and_prints_typeof() {
        let src = r#"
            let out = "";
            try {
                throw undefined;
            } catch (e) {
                out = typeof e;
            }
            out;
        "#;
        assert_eq!(run_source(src).unwrap(), Value::String("undefined".into()));
    }

    #[test]
    fn try_without_throw_runs_finally_only() {
        let src = r#"
            let out = "";
            try {
                out = "a";
            } catch (e) {
                out = "b";
            } finally {
                out = out + "c";
            }
            out;
        "#;
        assert_eq!(run_source(src).unwrap(), Value::String("ac".into()));
    }

    #[test]
    fn break_in_finally_wins_over_try_value() {
        let src = r#"
            let out = "";
            let i = 0;
            while (i < 3) {
                i = i + 1;
                try {
                    out = "tried";
                } finally {
                    break;
                }
            }
            out + ":" + i;
        "#;
        assert_eq!(run_source(src).unwrap(), Value::String("tried:1".into()));
    }

    #[test]
    fn continue_in_finally_wins_over_try_value() {
        let src = r#"
            let out = "";
            let i = 0;
            while (i < 3) {
                i = i + 1;
                try {
                    out = out + "x";
                } finally {
                    continue;
                }
            }
            out + ":" + i;
        "#;
        assert_eq!(run_source(src).unwrap(), Value::String("xxx:3".into()));
    }

    #[test]
    fn finally_value_does_not_override_pending_result() {
        let src = r#"
            let out = "";
            try {
                out = "try";
            } catch (e) {
                out = "catch";
            } finally {
                42;
            }
            out;
        "#;
        assert_eq!(run_source(src).unwrap(), Value::String("try".into()));
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

    // ---- Task 01: harden object/array/property-descriptor/prototype semantics ----

    // set_property: shadowing a prototype property must make it own+enumerable.
    #[test]
    fn set_property_shadows_prototype_property() {
        let src = "let proto={x:1}; let o=Object.create(proto); o.x=2; o.hasOwnProperty('x') && o.x === 2 && Object.keys(o).length === 1;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn set_property_shadowed_property_is_enumerable() {
        let src = "let proto={x:1}; let o=Object.create(proto); o.x=2; o.propertyIsEnumerable('x');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    // instanceof: walk starts at the left object's [[Prototype]] (spec §13.5.3.1).
    #[test]
    fn instanceof_matches_when_left_object_is_the_right_prototype() {
        let src = "function C(){} let c=new C(); c instanceof C;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn instanceof_matches_regular_instance() {
        let src = "function C(){} let c=new C(); c instanceof C;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn instanceof_rejects_unrelated_ctor() {
        let src = "function A(){} function B(){} let a=new A(); a instanceof B;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    // Object.defineProperty: descriptor flags must round-trip.
    #[test]
    fn define_property_honors_writable_flag() {
        let src = "let o={}; Object.defineProperty(o, 'a', {value: 1, writable: false}); let d=Object.getOwnPropertyDescriptor(o, 'a'); d.writable;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    #[test]
    fn define_property_honors_configurable_flag() {
        let src = "let o={}; Object.defineProperty(o, 'a', {value: 1, configurable: false}); let d=Object.getOwnPropertyDescriptor(o, 'a'); d.configurable;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    #[test]
    fn define_property_defaults_writable_configurable_to_true() {
        let src = "let o={}; Object.defineProperty(o, 'a', {value: 1}); let d=Object.getOwnPropertyDescriptor(o, 'a'); d.writable + ':' + d.configurable;";
        assert_eq!(run_source(src).unwrap(), Value::String("true:true".into()));
    }

    #[test]
    fn define_property_rejects_redefining_non_configurable() {
        let src = "let o={}; Object.defineProperty(o, 'a', {value: 1, configurable: false}); Object.defineProperty(o, 'a', {value: 2});";
        let err = run_source(src).unwrap_err();
        assert!(err.to_string().contains("non-configurable"));
    }

    #[test]
    fn define_property_non_enumerable_is_not_in_keys() {
        let src = "let o={}; Object.defineProperty(o, 'a', {value: 1, enumerable: false}); Object.keys(o).length;";
        assert_eq!(run_source(src).unwrap(), Value::Number(0.0));
    }

    // Object.assign: must copy inherited enumerable own properties.
    #[test]
    fn object_assign_copies_inherited_enumerable_properties() {
        let src = "let proto={x:1}; let o=Object.create(proto); let t={}; Object.assign(t, o); t.x;";
        assert_eq!(run_source(src).unwrap(), Value::Number(1.0));
    }

    #[test]
    fn object_assign_skips_non_enumerable_source_properties() {
        let src = "let o={}; Object.defineProperty(o, 'a', {value: 1, enumerable: false}); let t={}; Object.assign(t, o); ('a' in t);";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    // Object.create: second-arg properties object must be applied.
    #[test]
    fn object_create_applies_properties_object() {
        let src = "let o=Object.create(null, {x: {value: 7}}); o.x;";
        assert_eq!(run_source(src).unwrap(), Value::Number(7.0));
    }

    #[test]
    fn object_create_properties_object_is_own() {
        let src = "let o=Object.create(null, {x: {value: 7}}); Object.getOwnPropertyNames(o).length;";
        assert_eq!(run_source(src).unwrap(), Value::Number(1.0));
    }

    // getOwnPropertyDescriptor: array length stays non-enumerable/non-configurable.
    #[test]
    fn get_own_property_descriptor_array_length_flags() {
        let src = "let d=Object.getOwnPropertyDescriptor([1,2], 'length'); d.enumerable + ':' + d.configurable;";
        assert_eq!(run_source(src).unwrap(), Value::String("false:false".into()));
    }

    // Property assignment vs prototype chain: deletion does not expose inherited.
    #[test]
    fn delete_own_property_exposes_inherited() {
        let src = "let proto={x:1}; let o=Object.create(proto); o.x=2; delete o.x; o.x;";
        assert_eq!(run_source(src).unwrap(), Value::Number(1.0));
    }

    // Object.create: second-arg non-enumerable descriptor flag respected.
    #[test]
    fn object_create_non_enumerable_descriptor_not_in_keys() {
        let src = "let o=Object.create(null, {x: {value: 7, enumerable: false}}); Object.keys(o).length;";
        assert_eq!(run_source(src).unwrap(), Value::Number(0.0));
    }

    // Object.defineProperty: non-configurable prevents redefining value.
    #[test]
    fn define_property_non_configurable_prevents_flag_change() {
        let src = "let o={}; Object.defineProperty(o, 'a', {value: 1, configurable: false}); Object.defineProperty(o, 'a', {value: 2});";
        let err = run_source(src).unwrap_err();
        assert!(err.to_string().contains("non-configurable"));
    }

    // instanceof: rejects non-object right-hand side.
    #[test]
    fn instanceof_rejects_non_object_right() {
        let src = "(1) instanceof 1;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    // Object.defineProperty: accessor descriptors not supported (data-only).
    // Verify data descriptor round-trips with all flags.
    #[test]
    fn define_property_all_flags_roundtrip() {
        let src = "let o={}; Object.defineProperty(o, 'a', {value: 42, writable: false, enumerable: false, configurable: false}); let d=Object.getOwnPropertyDescriptor(o, 'a'); d.value + ':' + d.writable + ':' + d.enumerable + ':' + d.configurable;";
        assert_eq!(run_source(src).unwrap(), Value::String("42:false:false:false".into()));
    }

    // Object.prototype.valueOf: returns this for objects.
    #[test]
    fn object_prototype_value_of_returns_this() {
        let src = "let o={x:1}; let v=o.valueOf(); v === o;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn object_prototype_value_of_returns_this_for_array() {
        let src = "let a=[1,2,3]; let v=a.valueOf(); v === a;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    // Object.prototype.toLocaleString: delegates to toString for plain objects.
    #[test]
    fn object_prototype_to_locale_string_delegates_to_to_string() {
        let src = "let o={}; o.toLocaleString() === '[object Object]';";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn object_prototype_to_locale_string_returns_same_as_to_string_for_array() {
        let src = "let a=[1,2,3]; a.toLocaleString() === a.toString();";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    // Object.prototype.isPrototypeOf: checks if receiver is in prototype chain.
    #[test]
    fn object_prototype_is_prototype_of_returns_true_for_own() {
        let src = "let proto={}; let o=Object.create(proto); proto.isPrototypeOf(o);";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn object_prototype_is_prototype_of_returns_false_for_unrelated() {
        let src = "let a={}; let b={}; a.isPrototypeOf(b);";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    #[test]
    fn object_prototype_is_prototype_of_returns_false_for_non_object() {
        let src = "let proto={}; proto.isPrototypeOf(1);";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    #[test]
    fn object_prototype_is_prototype_of_checks_full_chain() {
        let src = "let proto={}; let o=Object.create(proto); let o2=Object.create(o); proto.isPrototypeOf(o2);";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn object_prototype_is_prototype_of_returns_false_for_null() {
        let src = "let proto={}; proto.isPrototypeOf(null);";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    // ---- Task 02: expand standard library (remaining Array/Object/String methods) ----

    // Array.prototype.at
    #[test]
    fn array_at_positive_index() {
        let src = "[10,20,30].at(1);";
        assert_eq!(run_source(src).unwrap(), Value::Number(20.0));
    }

    #[test]
    fn array_at_negative_index_counts_from_end() {
        let src = "[10,20,30].at(-1);";
        assert_eq!(run_source(src).unwrap(), Value::Number(30.0));
    }

    #[test]
    fn array_at_out_of_bounds_returns_undefined() {
        let src = "let a=[1]; a.at(5) === undefined && a.at(-5) === undefined;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn array_at_is_not_enumerable() {
        let src = "Array.prototype.propertyIsEnumerable('at');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    // String.prototype.at
    #[test]
    fn string_at_positive_index() {
        let src = "'hello'.at(1);";
        assert_eq!(run_source(src).unwrap(), Value::String("e".into()));
    }

    #[test]
    fn string_at_negative_index_counts_from_end() {
        let src = "'hello'.at(-1);";
        assert_eq!(run_source(src).unwrap(), Value::String("o".into()));
    }

    #[test]
    fn string_at_out_of_bounds_returns_empty_string() {
        let src = "let s='ab'; s.at(5) === '' && s.at(-5) === '';";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn string_at_is_not_enumerable() {
        let src = "String.prototype.propertyIsEnumerable('at');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    // String.prototype.codePointAt
    #[test]
    fn string_code_point_at_ascii() {
        let src = "'abc'.codePointAt(1);";
        assert_eq!(run_source(src).unwrap(), Value::Number(98.0));
    }

    #[test]
    fn string_code_point_at_surrogate_pair() {
        let src = "'😀'.codePointAt(0);";
        assert_eq!(run_source(src).unwrap(), Value::Number(128512.0));
    }

    #[test]
    fn string_code_point_at_out_of_bounds_returns_undefined() {
        let src = "'ab'.codePointAt(5) === undefined;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn string_code_point_at_is_not_enumerable() {
        let src = "String.prototype.propertyIsEnumerable('codePointAt');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    // String.prototype.replaceAll
    #[test]
    fn string_replace_all_replaces_every_occurrence() {
        let src = "'a-b-a'.replaceAll('a', 'x');";
        assert_eq!(run_source(src).unwrap(), Value::String("x-b-x".into()));
    }

    #[test]
    fn string_replace_all_no_match_returns_original() {
        let src = "'abc'.replaceAll('z', 'x');";
        assert_eq!(run_source(src).unwrap(), Value::String("abc".into()));
    }

    #[test]
    fn string_replace_all_replaces_multi_char_needle() {
        let src = "'ababab'.replaceAll('ab', 'xy');";
        assert_eq!(run_source(src).unwrap(), Value::String("xyxyxy".into()));
    }

    #[test]
    fn string_replace_all_is_not_enumerable() {
        let src = "String.prototype.propertyIsEnumerable('replaceAll');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    // String.prototype.localeCompare
    #[test]
    fn string_locale_compare_equal_strings() {
        let src = "'abc'.localeCompare('abc');";
        assert_eq!(run_source(src).unwrap(), Value::Number(0.0));
    }

    #[test]
    fn string_locale_compare_less_and_greater() {
        let src = "'abc'.localeCompare('abd') < 0 && 'abd'.localeCompare('abc') > 0;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn string_locale_compare_is_not_enumerable() {
        let src = "String.prototype.propertyIsEnumerable('localeCompare');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    // String.prototype.search
    #[test]
    fn string_search_finds_first_match() {
        let src = "'hello world'.search('world');";
        assert_eq!(run_source(src).unwrap(), Value::Number(6.0));
    }

    #[test]
    fn string_search_returns_minus_one_when_missing() {
        let src = "'hello'.search('xyz');";
        assert_eq!(run_source(src).unwrap(), Value::Number(-1.0));
    }

    #[test]
    fn string_search_is_not_enumerable() {
        let src = "String.prototype.propertyIsEnumerable('search');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    // String.prototype.match
    #[test]
    fn string_match_returns_null_when_no_match() {
        let src = "'abc'.match('xyz') === null;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn string_match_returns_first_match_with_index_and_input() {
        let src = "let m='hello world'.match('world'); m[0] + ':' + m.index + ':' + m.input;";
        assert_eq!(
            run_source(src).unwrap(),
            Value::String("world:6:hello world".into())
        );
    }

    #[test]
    fn string_match_is_not_enumerable() {
        let src = "String.prototype.propertyIsEnumerable('match');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    // Object.fromEntries
    #[test]
    fn object_from_entries_builds_object_from_pairs() {
        let src = "let o=Object.fromEntries([['a',1],['b',2]]); o.a + ':' + o.b;";
        assert_eq!(run_source(src).unwrap(), Value::String("1:2".into()));
    }

    #[test]
    fn object_from_entries_round_trips_object_entries() {
        let src = "let o={a:1,b:2}; Object.fromEntries(Object.entries(o)).a;";
        assert_eq!(run_source(src).unwrap(), Value::Number(1.0));
    }

    #[test]
    fn object_from_entries_ignores_non_array_pairs() {
        let src = "let o=Object.fromEntries([1,'x']); Object.keys(o).length;";
        assert_eq!(run_source(src).unwrap(), Value::Number(0.0));
    }

    #[test]
    fn object_from_entries_is_not_enumerable() {
        let src = "Object.propertyIsEnumerable('fromEntries');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    // Object.defineProperties
    #[test]
    fn object_define_properties_sets_multiple_properties() {
        let src = "let o={}; Object.defineProperties(o, {a: {value: 1}, b: {value: 2}}); o.a + ':' + o.b;";
        assert_eq!(run_source(src).unwrap(), Value::String("1:2".into()));
    }

    #[test]
    fn object_define_properties_returns_target_object() {
        let src = "let o={}; Object.defineProperties(o, {a: {value: 1}}) === o;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn object_define_properties_honors_descriptor_flags() {
        let src = "let o={}; Object.defineProperties(o, {a: {value: 1, enumerable: false}}); Object.keys(o).length;";
        assert_eq!(run_source(src).unwrap(), Value::Number(0.0));
    }

    #[test]
    fn object_define_properties_is_not_enumerable() {
        let src = "Object.propertyIsEnumerable('defineProperties');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }

    // Number.isSafeInteger
    #[test]
    fn number_is_safe_integer_accepts_safe_integers() {
        let src = "Number.isSafeInteger(0) && Number.isSafeInteger(42) && Number.isSafeInteger(9007199254740991) && Number.isSafeInteger(-9007199254740991);";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn number_is_safe_integer_rejects_unsafe_integers() {
        let src = "let big = 9007199254740991 + 2; (!Number.isSafeInteger(big)) && (!Number.isSafeInteger(-big));";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn number_is_safe_integer_rejects_non_integers_and_non_numbers() {
        let src = "(!Number.isSafeInteger(1.5)) && (!Number.isSafeInteger('x')) && (!Number.isSafeInteger());";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }

    #[test]
    fn number_is_safe_integer_is_not_enumerable() {
        let src = "Number.propertyIsEnumerable('isSafeInteger');";
        assert_eq!(run_source(src).unwrap(), Value::Bool(false));
    }
}
