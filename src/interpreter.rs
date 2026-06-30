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
                return Err(JsError::runtime(format!("cannot assign to const `{name}`")));
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
    step_limit: Option<usize>,
    steps: usize,
    max_call_depth: Option<usize>,
    call_depth: usize,
    output_limit: Option<usize>,
    output_truncated: bool,
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
        let mut this = Self {
            env,
            closures: HashMap::new(),
            output: Vec::new(),
            global,
            object_proto,
            function_proto,
            array_proto,
            error_proto,
            step_limit,
            steps: 0,
            max_call_depth,
            call_depth: 0,
            output_limit,
            output_truncated: false,
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

        let json = Object::plain();
        json.borrow_mut().proto = Some(self.object_proto.clone());
        Self::define_non_enumerable(&json, "stringify", self.native_method("JSON.stringify"));
        self.define_global("JSON", Value::Object(json), false);

        self.function_proto.borrow_mut().proto = Some(self.object_proto.clone());
        self.array_proto.borrow_mut().proto = Some(self.object_proto.clone());
        self.error_proto.borrow_mut().proto = Some(self.object_proto.clone());

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
        }
        if let Some(Value::Object(string_ctor)) = self.env.borrow().get("String") {
            Self::define_non_enumerable(&string_ctor, "prototype", Value::Object(Object::plain()));
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

    pub fn run(&mut self, program: &Program) -> JsResult<Value> {
        match self.eval_statements(&program.statements)? {
            Flow::Value(v) | Flow::Return(v) => Ok(v),
            Flow::Throw(v) => Err(JsError::runtime(v.to_string())),
            Flow::Break => Err(JsError::runtime("break used outside loop")),
            Flow::Continue => Err(JsError::runtime("continue used outside loop")),
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
                .ok_or_else(|| JsError::runtime(format!("undefined variable `{name}`"))),
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
                if let Value::Object(o) = object_value.clone() {
                    Ok((
                        Object::lookup(&o, property).unwrap_or(Value::Undefined),
                        object_value,
                    ))
                } else {
                    Ok((Value::Undefined, object_value))
                }
            }
            Expr::Index { object, index } => {
                let object_value = self.eval_expr(object)?;
                let key = self.eval_expr(index)?.to_string();
                if let Value::Object(o) = object_value.clone() {
                    Ok((
                        Object::lookup(&o, &key).unwrap_or(Value::Undefined),
                        object_value,
                    ))
                } else {
                    Ok((Value::Undefined, object_value))
                }
            }
            _ => Ok((self.eval_expr(expr)?, Value::Object(self.global.clone()))),
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
                    Err(JsError::runtime("member access on non-object"))
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
                    Err(JsError::runtime("index access on non-object"))
                }
            }
            _ => Err(JsError::runtime("target is not a reference")),
        }
    }

    fn get_target(&mut self, target: &Expr) -> JsResult<Value> {
        match target {
            Expr::Identifier(name) => self
                .env
                .borrow()
                .get(name)
                .ok_or_else(|| JsError::runtime(format!("undefined variable `{name}`"))),
            Expr::Member { .. } | Expr::Index { .. } => {
                let r = self.get_ref(target)?;
                Ok(self.get_property(&r.object, &r.property))
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
            _ => Err(JsError::runtime("target is not assignable")),
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
            BinaryOp::Equal | BinaryOp::StrictEqual => Ok(Value::Bool(left == right)),
            BinaryOp::NotEqual | BinaryOp::StrictNotEqual => Ok(Value::Bool(left != right)),
            BinaryOp::Less => Ok(Value::Bool(left.to_number() < right.to_number())),
            BinaryOp::LessEqual => Ok(Value::Bool(left.to_number() <= right.to_number())),
            BinaryOp::Greater => Ok(Value::Bool(left.to_number() > right.to_number())),
            BinaryOp::GreaterEqual => Ok(Value::Bool(left.to_number() >= right.to_number())),
            BinaryOp::Instanceof => Ok(Value::Bool(self.instanceof(left, right))),
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
            return Err(JsError::runtime(format!(
                "{} is not callable",
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
                    Flow::Break => Err(JsError::runtime("break used outside loop")),
                    Flow::Continue => Err(JsError::runtime("continue used outside loop")),
                }
            }
            _ => Err(JsError::runtime("object is not callable")),
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
                    return Err(JsError::runtime("Object.keys expects object"));
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
                    return Err(JsError::runtime("Object.defineProperty expects object"));
                };
                let key = args.get(1).cloned().unwrap_or(Value::Undefined).to_string();
                let Some(Value::Object(descriptor)) = args.get(2).cloned() else {
                    return Err(JsError::runtime(
                        "Object.defineProperty expects descriptor object",
                    ));
                };
                let value = descriptor.borrow().props.get("value").cloned();
                if let Some(value) = value {
                    self.set_property(&target, &key, value);
                }
                Ok(Value::Object(target))
            }
            "Object.getOwnPropertyDescriptor" => {
                let Some(Value::Object(object)) = args.first().cloned() else {
                    return Err(JsError::runtime(
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
                    _ => Err(JsError::runtime("Object.create expects object or null")),
                }
            }
            "Object.getPrototypeOf" => {
                let Some(Value::Object(object)) = args.first().cloned() else {
                    return Err(JsError::runtime("Object.getPrototypeOf expects object"));
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
            "JSON.stringify" => Ok(Value::String(
                args.first()
                    .cloned()
                    .unwrap_or(Value::Undefined)
                    .to_string(),
            )),
            "Object.values" => {
                let Some(Value::Object(object)) = args.first().cloned() else {
                    return Err(JsError::runtime("Object.values expects object"));
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

        assert_eq!(run_source(src).unwrap(), Value::String("3|6|9".into()));
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
