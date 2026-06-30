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
}

impl Interpreter {
    pub fn new() -> Self {
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
        json.borrow_mut()
            .props
            .insert("stringify".into(), self.native_method("JSON.stringify"));
        self.define_global("JSON", Value::Object(json), false);

        self.function_proto.borrow_mut().proto = Some(self.object_proto.clone());
        self.array_proto.borrow_mut().proto = Some(self.object_proto.clone());
        self.error_proto.borrow_mut().proto = Some(self.object_proto.clone());

        if let Some(Value::Object(object_ctor)) = self.env.borrow().get("Object") {
            object_ctor
                .borrow_mut()
                .props
                .insert("prototype".into(), Value::Object(self.object_proto.clone()));
            self.object_proto.borrow_mut().props.insert(
                "toString".into(),
                self.native_method("Object.prototype.toString"),
            );
        }
        if let Some(Value::Object(array_ctor)) = self.env.borrow().get("Array") {
            array_ctor
                .borrow_mut()
                .props
                .insert("prototype".into(), Value::Object(self.array_proto.clone()));
            self.array_proto
                .borrow_mut()
                .props
                .insert("map".into(), self.native_method("Array.prototype.map"));
            self.array_proto
                .borrow_mut()
                .props
                .insert("join".into(), self.native_method("Array.prototype.join"));
            self.array_proto
                .borrow_mut()
                .props
                .insert("slice".into(), self.native_method("Array.prototype.slice"));
        }
        if let Some(Value::Object(string_ctor)) = self.env.borrow().get("String") {
            string_ctor
                .borrow_mut()
                .props
                .insert("prototype".into(), Value::Object(Object::plain()));
        }
        for name in [
            "Error",
            "TypeError",
            "SyntaxError",
            "ReferenceError",
            "RangeError",
        ] {
            if let Some(Value::Object(ctor)) = self.env.borrow().get(name) {
                ctor.borrow_mut()
                    .props
                    .insert("prototype".into(), Value::Object(self.error_proto.clone()));
            }
        }
    }

    fn define_native(&mut self, name: &'static str) {
        let value = self.native_method(name);
        self.define_global(name, value, false);
    }

    fn native_method(&self, name: &'static str) -> Value {
        let obj = Object::with_internal(Internal::Native(name));
        obj.borrow_mut().proto = Some(self.function_proto.clone());
        obj.borrow_mut()
            .props
            .insert("prototype".into(), Value::Object(Object::plain()));
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
        match self.eval_statements(&program.statements)? {
            Flow::Value(v) | Flow::Return(v) => Ok(v),
            Flow::Throw(v) => Err(JsError::runtime(v.to_string())),
            Flow::Break => Err(JsError::runtime("break outside loop")),
        }
    }

    pub fn take_output(self) -> Vec<String> {
        self.output
    }

    fn eval_statements(&mut self, statements: &[Stmt]) -> JsResult<Flow> {
        let mut last = Value::Undefined;
        for stmt in statements {
            match self.eval_stmt(stmt)? {
                Flow::Value(v) => last = v,
                r @ (Flow::Return(_) | Flow::Throw(_) | Flow::Break) => return Ok(r),
            }
        }
        Ok(Flow::Value(last))
    }

    fn eval_stmt(&mut self, stmt: &Stmt) -> JsResult<Flow> {
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
                        r @ (Flow::Return(_) | Flow::Throw(_) | Flow::Break) => return Ok(r),
                    }
                }
                let mut last = Value::Undefined;
                loop {
                    if let Some(condition) = condition
                        && !self.eval_expr(condition)?.is_truthy()
                    {
                        break;
                    }
                    match self.with_child(body)? {
                        Flow::Value(v) => last = v,
                        Flow::Break => break,
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
                            r @ (Flow::Return(_) | Flow::Throw(_)) => return Ok(r),
                        }
                    }
                }
                if !matched {
                    match self.with_child(default)? {
                        Flow::Value(v) => last = v,
                        Flow::Break => return Ok(Flow::Value(last)),
                        r @ (Flow::Return(_) | Flow::Throw(_)) => return Ok(r),
                    }
                }
                Ok(Flow::Value(last))
            }
            Stmt::Block(stmts) => self.with_child(stmts),
            Stmt::Break => Ok(Flow::Break),
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
        proto
            .borrow_mut()
            .props
            .insert("constructor".into(), Value::Object(obj.clone()));
        obj.borrow_mut()
            .props
            .insert("prototype".into(), Value::Object(proto));
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
                let obj = Object::with_internal(Internal::Array(values));
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
                return items.get(i).cloned().unwrap_or(Value::Undefined);
            }
        }
        Object::lookup(object, property).unwrap_or(Value::Undefined)
    }

    fn set_property(&self, object: &ObjectRef, property: &str, value: Value) {
        if let Internal::Array(items) = &mut object.borrow_mut().internal {
            if property == "length" {
                let n = value.to_number();
                if n.is_finite() && n >= 0.0 && n.fract() == 0.0 {
                    items.resize(n as usize, Value::Undefined);
                }
                return;
            }
            if let Ok(i) = property.parse::<usize>() {
                if i >= items.len() {
                    items.resize(i + 1, Value::Undefined);
                }
                items[i] = value;
                return;
            }
        }
        object
            .borrow_mut()
            .props
            .insert(property.to_string(), value);
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
                    Flow::Break => Err(JsError::runtime("break outside loop")),
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
                self.output.push(
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
            "Array" => {
                let obj = Object::with_internal(Internal::Array(args));
                obj.borrow_mut().proto = Some(self.array_proto.clone());
                Ok(Value::Object(obj))
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
                            .map(|v| v.to_string())
                            .collect::<Vec<_>>()
                            .join(&sep),
                    ));
                }
                Ok(Value::String(String::new()))
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
            "Array.prototype.map" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                if let Value::Object(o) = this_value {
                    let items = if let Internal::Array(items) = &o.borrow().internal {
                        items.clone()
                    } else {
                        Vec::new()
                    };
                    let mapped = items
                        .into_iter()
                        .map(|v| {
                            self.call(
                                callback.clone(),
                                vec![v],
                                Value::Object(self.global.clone()),
                                false,
                            )
                        })
                        .collect::<JsResult<Vec<_>>>()?;
                    let obj = Object::with_internal(Internal::Array(mapped));
                    obj.borrow_mut().proto = Some(self.array_proto.clone());
                    return Ok(Value::Object(obj));
                }
                let obj = Object::with_internal(Internal::Array(Vec::new()));
                obj.borrow_mut().proto = Some(self.array_proto.clone());
                Ok(Value::Object(obj))
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
    fn instanceof_walks_prototype_chain() {
        let src = "function C(){} let c=new C(); c instanceof C;";
        assert_eq!(run_source(src).unwrap(), Value::Bool(true));
    }
}
