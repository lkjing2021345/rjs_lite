//! Register-based bytecode VM.
//!
//! Executes the [`crate::bytecode::Program`] produced by [`crate::compiler`].
//! The machine is a classic fetch-decode-execute loop over an operand stack
//! (`Vec<Value>`), a program counter, a call frame stack, and a stack of
//! exception handlers. Lexical scope is the interpreter's `Env` chain,
//! carried per frame so closures keep their defining environment.
//!
//! Design notes:
//! - The VM reuses the existing `Value` / `Object` model and the
//!   interpreter's built-in dispatch (`call_native`), so semantics match the
//!   tree-walking interpreter exactly.
//! - `break` / `continue` are compiled to plain `Jump`s by the compiler; the
//!   `Break` / `Continue` instructions are defensive fallbacks (the compiler
//!   panics before emitting them outside a loop).
//! - `try/catch/finally` is compiled to `TryBegin`/`CatchParam`/`EndTry`
//!   with a handler stack. A jump into a handler region is intercepted by
//!   `run_to` and the pending exception is bound to the catch parameter.
//! - `for-in` is compiled to `ForIn`/`ForInEnd` with a per-frame iterator
//!   state; the target binding is assigned by the compiler-emitted code.
//! - Function bodies are separate instruction streams; entering a call
//!   pushes a frame and switches the program counter into the body stream.

use crate::ast::{BinaryOp, UnaryOp};
use crate::bytecode::{FunctionBytecode, Instruction, Program};
use crate::error::{JsError, JsResult};
use crate::interpreter::{Env, Flow, Interpreter};
use crate::value::{Internal, Object, ObjectRef, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// A pending exception: the thrown value plus the innermost exception
/// handler that is responsible for it.
#[derive(Clone)]
struct PendingException {
    value: Value,
    handler: usize,
}

/// One frame of the call stack.
#[derive(Clone)]
struct Frame {
    /// Return address in the caller's stream.
    return_pc: usize,
    /// Index into the program's function table for the current stream.
    function_index: usize,
    /// Saved outer environment (restored on return).
    saved_env: Rc<RefCell<Env>>,
    /// Saved operand stack (restored on return).
    saved_stack: Vec<Value>,
    /// Saved for-in iterator state (restored on return).
    saved_forin: Option<(Value, Vec<String>, usize)>,
    /// Saved exception handler stack (restored on return).
    saved_handlers: Vec<Handler>,
}

/// An active `try` handler.
#[derive(Clone)]
struct Handler {
    /// End of the try block.
    block_end: usize,
    /// Entry of the catch block (immediately followed by `CatchParam`).
    catch: Option<usize>,
    /// Entry of the finally block.
    finally: Option<usize>,
}

pub struct Vm {
    /// Operand stack (register file).
    stack: Vec<Value>,
    /// Program counter into the current instruction stream.
    pc: usize,
    /// Call frame stack.
    frames: Vec<Frame>,
    /// Exception handler stack.
    handlers: Vec<Handler>,
    /// Current lexical environment.
    env: Rc<RefCell<Env>>,
    /// Closure capture map: function object pointer -> (defining env,
    /// function-table index).
    closures: HashMap<usize, (Rc<RefCell<Env>>, usize)>,
    /// The compiled program.
    program: Program,
    /// Native dispatch host (owns the built-in prototypes).
    native: Interpreter,
    /// For-in iterator state: (object, keys, index).
    forin: Option<(Value, Vec<String>, usize)>,
    /// Pending exception, if any.
    #[allow(dead_code)]
    pending: Option<PendingException>,
    /// Step accounting.
    step_limit: Option<usize>,
    steps: usize,
    /// Call depth accounting.
    max_call_depth: Option<usize>,
    /// Output accounting.
    output_limit: Option<usize>,
    output_truncated: bool,
    /// Strict mode.
    strict: bool,
}

impl Vm {
    pub fn new() -> Self {
        Self::new_with_limits(None, None, None)
    }

    pub fn new_with_limits(
        step_limit: Option<usize>,
        max_call_depth: Option<usize>,
        output_limit: Option<usize>,
    ) -> Self {
        let native = Interpreter::new();
        Self {
            stack: Vec::new(),
            pc: 0,
            frames: Vec::new(),
            handlers: Vec::new(),
            env: Env::new(),
            closures: HashMap::new(),
            program: Program::new(),
            native,
            forin: None,
            pending: None,
            step_limit,
            steps: 0,
            max_call_depth,
            output_limit,
            output_truncated: false,
            strict: false,
        }
    }

    pub fn take_output(&mut self) -> (Vec<String>, bool) {
        (self.native.take_output(), self.output_truncated)
    }

    /// Execute a compiled program, returning the completion value.
    pub fn run(&mut self, program: Program) -> JsResult<Value> {
        self.program = program;
        self.detect_strict_mode();
        self.run_to(self.program.instructions.len())
    }

    /// Compile and run a parsed program.
    pub fn run_ast(&mut self, program: &crate::ast::Program) -> JsResult<Value> {
        self.run(crate::compiler::compile(program))
    }

    fn detect_strict_mode(&mut self) {
        if let Some(Instruction::PushString(s)) = self.program.instructions.first() {
            if s == "use strict" {
                self.strict = true;
            }
        }
    }

    // --- Operand stack ---

    fn push(&mut self, value: Value) {
        self.stack.push(value);
    }

    fn pop(&mut self) -> Value {
        self.stack
            .pop()
            .unwrap_or_else(|| panic!("vm: operand stack underflow"))
    }

    fn peek(&self) -> &Value {
        self.stack
            .last()
            .expect("vm: operand stack underflow")
    }

    // --- Instruction streams ---

    fn stream(&self, function_index: usize) -> &[Instruction] {
        if function_index == 0 {
            &self.program.instructions
        } else {
            &self.program.functions[function_index - 1].instructions
        }
    }

    fn current_stream(&self) -> &[Instruction] {
        self.stream(self.function_index())
    }

    fn function_index(&self) -> usize {
        if self.frames.is_empty() {
            0
        } else {
            self.frames.last().unwrap().function_index
        }
    }

    // --- Step / depth / output accounting ---

    fn step(&mut self) -> JsResult<()> {
        self.steps = self.steps.saturating_add(1);
        if self.step_limit.is_some_and(|limit| self.steps > limit) {
            return Err(JsError::runtime("execution step limit exceeded"));
        }
        Ok(())
    }

    fn enter_call(&mut self) -> JsResult<()> {
        if self
            .max_call_depth
            .is_some_and(|limit| self.frames.len() + 1 > limit)
        {
            return Err(JsError::runtime("call depth limit exceeded"));
        }
        Ok(())
    }

    // --- Core execution ---

    /// Execute the current stream until `pc` reaches `end`, the current
    /// function returns, an exception escapes every active handler, or a
    /// hard runtime error occurs.
    fn run_to(&mut self, end: usize) -> JsResult<Value> {
        loop {
            self.step()?;
            if self.pc >= end {
                return Ok(self.pop());
            }
            let instr = self.current_stream()[self.pc].clone();
            self.pc += 1;
            match self.execute(instr) {
                Ok(()) => {}
                Err(e) => {
                    if let Some(flow) = e.as_flow() {
                        match flow {
                            Flow::Return(v) => {
                                let v = v.clone();
                                if self.frames.is_empty() {
                                    return Ok(v);
                                }
                                return Ok(self.finish_frame());
                            }
                            Flow::Throw(v) => {
                                let v = v.clone();
                                self.pending = Some(PendingException {
                                    value: v,
                                    handler: 0,
                                });
                                self.pc = end;
                                match self.find_handler() {
                                    Some(catch) => {
                                        self.pc = catch;
                                    }
                                    None => {
                                        return Err(JsError::runtime(
                                            self.pending.take().unwrap().value.to_string(),
                                        ));
                                    }
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }
                    return Err(e);
                }
            }

            // Interceptor: a jump landed exactly on a catch entry.
            if let Some((param, after, value)) = self.catch_entry_at(self.pc) {
                let env = Env::child(self.env.clone());
                self.env = env.clone();
                if let Some(param) = param {
                    env.borrow_mut().define(param, value, true);
                }
                self.pc = after;
            }
        }
    }

    /// Restore the caller's state after a `return` and return the value.
    fn finish_frame(&mut self) -> Value {
        let frame = self.frames.pop().unwrap();
        self.pc = frame.return_pc;
        self.env = frame.saved_env;
        self.stack = frame.saved_stack;
        self.forin = frame.saved_forin;
        self.handlers = frame.saved_handlers;
        self.pop()
    }

    /// Walk the handler stack (and frame stack) to find the innermost
    /// handler that can catch the pending exception. Returns the catch
    /// entry, or `None` when the exception escapes the whole program.
    fn find_handler(&mut self) -> Option<usize> {
        loop {
            let mut handler = None;
            while let Some(h) = self.handlers.pop() {
                if self.pc > h.block_end {
                    // The jump already passed the try block: this handler is
                    // done (e.g. we are inside its finally block).
                    continue;
                }
                handler = h.catch;
                self.handlers.push(h);
                break;
            }
            if let Some(catch) = handler {
                return Some(catch);
            }
            if self.frames.is_empty() {
                return None;
            }
            // Unwind the frame and keep searching outer handlers.
            self.finish_frame();
        }
    }

    /// The catch entry at `pc`, if a handler's catch block starts there.
    fn catch_entry_at(&mut self, pc: usize) -> Option<(Option<String>, usize, Value)> {
        for h in self.handlers.iter().rev() {
            if let Some(catch) = h.catch {
                if catch == pc {
                    let param = match &self.current_stream()[catch] {
                        Instruction::CatchParam(param) => param.clone(),
                        _ => None,
                    };
                    let value = self.pending.take().unwrap().value;
                    return Some((param, catch + 1, value));
                }
            }
        }
        None
    }

    fn execute(&mut self, instr: Instruction) -> JsResult<()> {
        match instr {
            // --- Literals ---
            Instruction::PushNumber(n) => self.push(Value::Number(n)),
            Instruction::PushString(s) => self.push(Value::String(s)),
            Instruction::PushBool(b) => self.push(Value::Bool(b)),
            Instruction::PushNull => self.push(Value::Null),
            Instruction::PushUndefined => self.push(Value::Undefined),
            Instruction::PushThis => {
                let this = self
                    .env
                    .borrow()
                    .get("this")
                    .unwrap_or(Value::Object(self.native.global.clone()));
                self.push(this);
            }

            // --- Variable access ---
            Instruction::GetLocal(name) => {
                let value = self
                    .env
                    .borrow()
                    .get(&name)
                    .ok_or_else(|| JsError::reference_error(format!("{name} is not defined")))?;
                self.push(value);
            }
            Instruction::SetLocal(name) => {
                let value = self.pop();
                self.env.borrow_mut().assign(&name, value)?;
            }
            Instruction::DefineLocal(name, mutable) => {
                let value = self.pop();
                self.env.borrow_mut().define(name, value, mutable);
            }

            // --- Property access ---
            Instruction::GetMember(property) => {
                let object = self.pop();
                let value = self.native.get_property_on_value(&object, &property);
                self.push(value);
            }
            Instruction::SetMember(property) => {
                let object = self.pop();
                let value = self.pop();
                match object {
                    Value::Object(o) => self.native.set_property(&o, &property, value),
                    _ => {
                        // JS: assignment to a primitive member target is a
                        // silent no-op.
                    }
                }
            }
            Instruction::GetIndex => {
                let index = self.pop();
                let object = self.pop();
                let value = self
                    .native
                    .get_property_on_value(&object, &index.to_string());
                self.push(value);
            }
            Instruction::SetIndex => {
                let index = self.pop();
                let object = self.pop();
                let value = self.pop();
                match object {
                    Value::Object(o) => {
                        self.native.set_property(&o, &index.to_string(), value)
                    }
                    _ => {}
                }
            }

            // --- Stack manipulation ---
            Instruction::Dup => {
                let value = self.peek().clone();
                self.push(value);
            }
            Instruction::Pop => {
                self.pop();
            }

            // --- Arithmetic / logical ---
            Instruction::Binary(op) => {
                let right = self.pop();
                let left = self.pop();
                let result = self.eval_binary(left, op, right)?;
                self.push(result);
            }
            Instruction::Unary(op) => {
                let operand = self.pop();
                let result = self.eval_unary(op, operand);
                self.push(result);
            }
            Instruction::Typeof => {
                let value = self.pop();
                self.push(Value::String(value.type_name().to_string()));
            }
            Instruction::In => {
                let right = self.pop();
                let left = self.pop();
                let Value::Object(o) = right else {
                    return Err(JsError::type_error(
                        "right-hand side of in must be an object",
                    ));
                };
                let key = left.to_string();
                let has = Object::lookup(&o, &key).is_some();
                self.push(Value::Bool(has));
            }
            Instruction::Instanceof => {
                let right = self.pop();
                let left = self.pop();
                self.push(Value::Bool(self.native.instanceof(left, right)));
            }
            Instruction::Update { delta, prefix } => {
                let old = self.pop();
                let new_value = Value::Number(old.to_number() + delta);
                self.push(if prefix { new_value } else { old });
            }

            // --- Calls ---
            Instruction::Call(arg_count) => self.call_value(false, arg_count)?,
            Instruction::New(arg_count) => self.call_value(true, arg_count)?,

            // --- Object / array construction ---
            Instruction::PushArray(count) => {
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push(Some(self.pop()));
                }
                items.reverse();
                let obj = Object::with_internal(Internal::Array(items));
                obj.borrow_mut().proto = Some(self.native.array_proto.clone());
                self.push(Value::Object(obj));
            }
            Instruction::PushObject => {
                let obj = Object::plain();
                obj.borrow_mut().proto = Some(self.native.object_proto.clone());
                self.push(Value::Object(obj));
            }
            Instruction::SetObjectProperty(key) => {
                let value = self.pop();
                let Value::Object(obj) = self.pop() else {
                    return Err(JsError::type_error(
                        "object literal target is not an object",
                    ));
                };
                obj.borrow_mut().props.insert(key, value);
                self.push(Value::Object(obj));
            }
            Instruction::PushRegExp(pattern, flags) => {
                let obj = Object::plain();
                obj.borrow_mut().proto = Some(self.native.object_proto.clone());
                let mut p = obj.borrow_mut();
                p.props.insert("source".into(), Value::String(pattern));
                p.props.insert("flags".into(), Value::String(flags));
                p.props.insert("lastIndex".into(), Value::Number(0.0));
                drop(p);
                for method in ["test", "exec"] {
                    Interpreter::define_non_enumerable(
                        &obj,
                        method,
                        self.native.native_method("RegExp.prototype."),
                    );
                }
                self.push(Value::Object(obj));
            }

            // --- Functions ---
            Instruction::PushFunction(index) => {
                let value = self.make_function(index);
                self.push(value);
            }

            // --- Control flow ---
            Instruction::Jump(target) => {
                self.pc = target;
            }
            Instruction::JumpIfFalse(target) => {
                let value = self.pop();
                if !value.is_truthy() {
                    self.pc = target;
                }
            }
            Instruction::JumpIfTrue(target) => {
                let value = self.pop();
                if value.is_truthy() {
                    self.pc = target;
                }
            }
            Instruction::Loop(target) => {
                self.pc = target;
            }
            Instruction::Break => {
                return Err(JsError::syntax_error("break used outside loop"));
            }
            Instruction::Continue => {
                return Err(JsError::syntax_error("continue used outside loop"));
            }
            Instruction::Return => {
                let value = self.pop();
                return Err(JsError::flow(Flow::Return(value)));
            }
            Instruction::Throw => {
                let value = self.pop();
                return Err(JsError::flow(Flow::Throw(value)));
            }

            // --- Exception handling ---
            Instruction::TryBegin {
                block_end,
                catch,
                finally,
            } => {
                self.handlers.push(Handler {
                    block_end,
                    catch,
                    finally,
                });
            }
            Instruction::CatchParam(_) => {
                // The interceptor binds the parameter; the stream continues
                // into the catch block.
            }
            Instruction::EndTry => {
                // Normal completion of the try block.
                if let Some(h) = self.handlers.last() {
                    if self.pc <= h.block_end {
                        if let Some(finally) = h.finally {
                            self.handlers.pop();
                            self.run_finally(finally)?;
                        }
                        // No finally: the normal path jumps over the
                        // handlers via the compiler-emitted `Jump`.
                    }
                }
            }

            // --- For-in ---
            Instruction::ForIn(target) => {
                let Some((_, keys, index)) = &mut self.forin else {
                    return Err(JsError::runtime("for-in iterator is not active"));
                };
                if *index >= keys.len() {
                    self.forin = None;
                    self.pc = target;
                } else {
                    let key = keys[*index].clone();
                    *index += 1;
                    self.push(Value::String(key));
                }
            }
            Instruction::ForInEnd => {
                // The body ran for one key; loop back to the ForIn header
                // (the previous instruction).
                self.pc -= 1;
            }

            // --- Template literal ---
            Instruction::ConcatTemplate => {
                let part = self.pop();
                let Value::String(base) = self.pop() else {
                    return Err(JsError::runtime(
                        "template literal accumulator is not a string",
                    ));
                };
                self.push(Value::String(format!("{base}{}", part.to_string())));
            }
        }
        Ok(())
    }

    // --- Calls ---

    fn call_value(&mut self, construct: bool, arg_count: usize) -> JsResult<()> {
        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(self.pop());
        }
        args.reverse();
        let callee = self.pop();
        let this_value = self.pop();
        let result = self.call(callee, args, this_value, construct)?;
        self.push(result);
        Ok(())
    }

    fn call(
        &mut self,
        callee: Value,
        args: Vec<Value>,
        this_value: Value,
        construct: bool,
    ) -> JsResult<Value> {
        let Value::Object(func) = callee else {
            return Err(JsError::flow(Flow::Throw(Value::String(format!(
                "{} is not a function",
                callee.type_name()
            )))));
        };
        let internal = func.borrow().internal.clone();
        match internal {
            Internal::Native(name) => self
                .native
                .call_native(name, args, this_value, if construct { Some(func) } else { None })
                .map_err(|e| JsError::flow(Flow::Throw(Value::String(e.to_string())))),
            Internal::Function { params, .. } => {
                self.enter_call()?;
                let key = Rc::as_ptr(&func) as usize;
                let (closure_env, function_index) = self
                    .closures
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| (self.env.clone(), 0));
                let previous_env = self.env.clone();
                let env = Env::child(closure_env);
                let this_obj = if construct {
                    let obj = Object::plain();
                    if let Some(Value::Object(proto)) =
                        func.borrow().props.get("prototype").cloned()
                    {
                        obj.borrow_mut().proto = Some(proto);
                    } else {
                        obj.borrow_mut().proto = Some(self.native.object_proto.clone());
                    }
                    Value::Object(obj)
                } else {
                    this_value
                };
                env.borrow_mut().define("this".into(), this_obj.clone(), true);
                for (index, name) in params.into_iter().enumerate() {
                    if name.starts_with("...") {
                        let rest_name = name[3..].to_string();
                        let rest: Vec<Option<Value>> = args
                            .iter()
                            .skip(index)
                            .map(|v| Some(v.clone()))
                            .collect();
                        let arr = Object::with_internal(Internal::Array(rest));
                        arr.borrow_mut().proto = Some(self.native.array_proto.clone());
                        env.borrow_mut().define(rest_name, Value::Object(arr), true);
                        break;
                    }
                    let value = args.get(index).cloned().unwrap_or(Value::Undefined);
                    env.borrow_mut().define(name, value, true);
                }
                // Save the caller's state and switch into the body stream.
                let frame = Frame {
                    return_pc: self.pc,
                    function_index,
                    saved_env: previous_env,
                    saved_stack: self.stack.clone(),
                    saved_forin: self.forin.take(),
                    saved_handlers: std::mem::take(&mut self.handlers),
                };
                self.frames.push(frame);
                self.env = env;
                self.stack.clear();
                self.stack.push(Value::Undefined); // slot for the return value
                self.pc = 0;
                let result = self.run_to(self.current_stream().len())?;
                if construct && !matches!(result, Value::Object(_)) {
                    Ok(this_obj)
                } else {
                    Ok(result)
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
            _ => Err(JsError::flow(Flow::Throw(Value::String(
                "object is not a function".into(),
            )))),
        }
    }

    fn make_function(&mut self, index: usize) -> Value {
        let FunctionBytecode { params, .. } = &self.program.functions[index];
        let obj = Object::with_internal(Internal::Function {
            params: params.clone(),
            body: Vec::new(),
        });
        obj.borrow_mut().proto = Some(self.native.function_proto.clone());
        let proto = Object::plain();
        proto.borrow_mut().proto = Some(self.native.object_proto.clone());
        Interpreter::define_non_enumerable(&proto, "constructor", Value::Object(obj.clone()));
        Interpreter::define_non_enumerable(&obj, "prototype", Value::Object(proto));
        let value = Value::Object(obj.clone());
        self.closures
            .insert(Rc::as_ptr(&obj) as usize, (self.env.clone(), index + 1));
        value
    }

    // --- Property / coercion helpers ---

    fn eval_binary(&mut self, left: Value, op: BinaryOp, right: Value) -> JsResult<Value> {
        Ok(match op {
            BinaryOp::Add => match (left, right) {
                (Value::String(a), b) => Value::String(a + &b.to_string()),
                (a, Value::String(b)) => Value::String(a.to_string() + &b),
                (a, b) => Value::Number(a.to_number() + b.to_number()),
            },
            BinaryOp::Subtract => Value::Number(left.to_number() - right.to_number()),
            BinaryOp::Multiply => Value::Number(left.to_number() * right.to_number()),
            BinaryOp::Divide => Value::Number(left.to_number() / right.to_number()),
            BinaryOp::Remainder => Value::Number(left.to_number() % right.to_number()),
            BinaryOp::Equal => Value::Bool(left.abstract_eq(&right)),
            BinaryOp::NotEqual => Value::Bool(!left.abstract_eq(&right)),
            BinaryOp::StrictEqual => Value::Bool(left == right),
            BinaryOp::StrictNotEqual => Value::Bool(left != right),
            BinaryOp::Less => Value::Bool(left.to_number() < right.to_number()),
            BinaryOp::LessEqual => Value::Bool(left.to_number() <= right.to_number()),
            BinaryOp::Greater => Value::Bool(left.to_number() > right.to_number()),
            BinaryOp::GreaterEqual => Value::Bool(left.to_number() >= right.to_number()),
            BinaryOp::Instanceof => Value::Bool(self.native.instanceof(left, right)),
            BinaryOp::In => {
                let Value::Object(o) = right else {
                    return Err(JsError::type_error(
                        "right-hand side of in must be an object",
                    ));
                };
                let key = left.to_string();
                Value::Bool(Object::lookup(&o, &key).is_some())
            }
            BinaryOp::BitwiseAnd => {
                Value::Number((left.to_number() as i32 & right.to_number() as i32) as f64)
            }
            BinaryOp::BitwiseOr => {
                Value::Number((left.to_number() as i32 | right.to_number() as i32) as f64)
            }
            BinaryOp::BitwiseXor => {
                Value::Number((left.to_number() as i32 ^ right.to_number() as i32) as f64)
            }
            BinaryOp::LeftShift => {
                Value::Number(((left.to_number() as i32) << (right.to_number() as u32)) as f64)
            }
            BinaryOp::RightShift => {
                Value::Number(((left.to_number() as i32) >> (right.to_number() as u32)) as f64)
            }
            BinaryOp::UnsignedRightShift => {
                Value::Number(((left.to_number() as u32) >> (right.to_number() as u32)) as f64)
            }
            BinaryOp::And | BinaryOp::Or => unreachable!("short-circuited before binary eval"),
        })
    }

    fn eval_unary(&mut self, op: UnaryOp, operand: Value) -> Value {
        match op {
            UnaryOp::Not => Value::Bool(!operand.is_truthy()),
            UnaryOp::Negate => Value::Number(-operand.to_number()),
            UnaryOp::Delete => Value::Bool(true),
            UnaryOp::Void => Value::Undefined,
            UnaryOp::BitwiseNot => Value::Number(!(operand.to_number() as i32) as f64),
        }
    }

    // --- Try helpers ---

    fn run_finally(&mut self, entry: usize) -> JsResult<()> {
        let end = self.current_stream().len();
        self.pc = entry;
        self.run_to(end)?;
        Ok(())
    }
}

/// Convenience: compile and run a source string through the VM.
pub fn run_vm_source(source: &str) -> JsResult<Value> {
    let tokens = crate::lexer::lex(source)?;
    let program = crate::parser::parse(tokens)?;
    let compiled = crate::compiler::compile(&program);
    Vm::new().run(compiled)
}

/// Run a source string through the VM, returning the value and captured
/// `print()` output.
pub fn run_vm_source_with_output(source: &str) -> JsResult<(Value, Vec<String>)> {
    let tokens = crate::lexer::lex(source)?;
    let program = crate::parser::parse(tokens)?;
    let compiled = crate::compiler::compile(&program);
    let mut vm = Vm::new();
    let value = vm.run(compiled)?;
    let (output, _) = vm.take_output();
    Ok((value, output))
}

/// Run a source string through the VM with execution-step, call-depth,
/// and output-line limits.
pub fn run_vm_source_with_output_and_limits(
    source: &str,
    max_execution_steps: usize,
    max_call_depth: usize,
    max_output_lines: usize,
) -> JsResult<(Value, Vec<String>, bool)> {
    let tokens = crate::lexer::lex(source)?;
    let program = crate::parser::parse(tokens)?;
    let compiled = crate::compiler::compile(&program);
    let mut vm = Vm::new_with_limits(
        Some(max_execution_steps),
        Some(max_call_depth),
        Some(max_output_lines),
    );
    let value = vm.run(compiled)?;
    let (output, truncated) = vm.take_output();
    Ok((value, output, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Value;

    fn run(source: &str) -> Value {
        run_vm_source(source).unwrap()
    }

    #[test]
    fn vm_evaluates_arithmetic_and_variables() {
        assert_eq!(run("let x = 1 + 2 * 3; x;"), Value::Number(7.0));
    }

    #[test]
    fn vm_evaluates_functions_and_loops() {
        let source = r#"
            function add(a, b) { return a + b; }
            let total = 0;
            let i = 0;
            while (i < 4) {
                total = total + add(i, 1);
                i = i + 1;
            }
            total;
        "#;
        assert_eq!(run(source), Value::Number(10.0));
    }

    #[test]
    fn vm_closures_capture_lexical_environment() {
        let src = "function make(){ let x=1; return function(){ x=x+1; return x; }; } let f=make(); f()+f();";
        assert_eq!(run(src), Value::Number(5.0));
    }

    #[test]
    fn vm_method_call_binds_this() {
        let src = "let o={x:3, f:function(){return this.x;}}; o.f();";
        assert_eq!(run(src), Value::Number(3.0));
    }

    #[test]
    fn vm_new_uses_prototype_methods() {
        let src = "function C(x){ this.x=x; } C.prototype.get=function(){return this.x;}; let c=new C(7); c.get();";
        assert_eq!(run(src), Value::Number(7.0));
    }

    #[test]
    fn vm_try_catch_finally() {
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
        assert_eq!(run(src), Value::String("boom:true".into()));
    }

    #[test]
    fn vm_finally_runs_when_try_throws_without_catch() {
        let src = r#"
            let out = "";
            try {
                throw 1;
            } finally {
                out = "finally";
            }
            "after";
        "#;
        let err = run_vm_source(src).unwrap_err();
        assert!(err.to_string().contains("1"));
    }

    #[test]
    fn vm_finally_throw_replaces_catch_throw() {
        let src = r#"
            try {
                throw 1;
            } catch (e) {
                throw 2;
            } finally {
                throw 3;
            }
        "#;
        let err = run_vm_source(src).unwrap_err();
        assert!(err.to_string().contains("3"));
    }

    #[test]
    fn vm_finally_return_replaces_try_return() {
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
        assert_eq!(run(src), Value::Number(2.0));
    }

    #[test]
    fn vm_for_in_over_object() {
        let src = "let o={a:1,b:2}; let out=''; for (let k in o) { out = out + k; } out;";
        assert_eq!(run(src), Value::String("ab".into()));
    }

    #[test]
    fn vm_for_in_over_array() {
        let src = "let a=[10,20,30]; let s=0; for (let k in a) { s = s + a[k]; } s;";
        assert_eq!(run(src), Value::Number(60.0));
    }

    #[test]
    fn vm_switch_matches_case_and_stops_at_break() {
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
        assert_eq!(run(src), Value::String("two".into()));
    }

    #[test]
    fn vm_update_and_compound_assign() {
        let src = "let x = 5; x += 3; x++; x;";
        assert_eq!(run(src), Value::Number(9.0));
    }

    #[test]
    fn vm_typeof_operator() {
        assert_eq!(run("typeof 1;"), Value::String("number".into()));
        assert_eq!(run("typeof missing;"), Value::String("undefined".into()));
        assert_eq!(
            run("function f() {} typeof f;"),
            Value::String("function".into())
        );
    }

    #[test]
    fn vm_template_literal() {
        let src = "let name = 'world'; `hello ${name}!`;";
        assert_eq!(run(src), Value::String("hello world!".into()));
    }

    #[test]
    fn vm_array_and_object_literals() {
        let src = "let a = [1, 2, 3]; let o = {x: 10}; a.length + o.x;";
        assert_eq!(run(src), Value::Number(13.0));
    }

    #[test]
    fn vm_instanceof() {
        let src = "function C(){} let c = new C(); c instanceof C;";
        assert_eq!(run(src), Value::Bool(true));
    }

    #[test]
    fn vm_in_operator() {
        let src = "let o = {a: 1}; 'a' in o;";
        assert_eq!(run(src), Value::Bool(true));
    }

    #[test]
    fn vm_short_circuit_and_or() {
        assert_eq!(run("1 && 2;"), Value::Number(2.0));
        assert_eq!(run("0 && 2;"), Value::Number(0.0));
        assert_eq!(run("0 || 5;"), Value::Number(5.0));
        assert_eq!(run("1 || 5;"), Value::Number(1.0));
    }

    #[test]
    fn vm_continue_skips_to_next_iteration() {
        let src = r#"
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
        "#;
        assert_eq!(run(src), Value::Number(12.0));
    }

    #[test]
    fn vm_break_exits_nearest_loop() {
        let src = r#"
            let i = 0;
            while (i < 10) {
                i = i + 1;
                if (i == 4) {
                    break;
                }
            }
            i;
        "#;
        assert_eq!(run(src), Value::Number(4.0));
    }

    #[test]
    fn vm_step_limited_execution_stops_infinite_loops() {
        let error =
            run_vm_source_with_output_and_limits("while (true) {}", 10, 16, 256).unwrap_err();
        assert!(error.to_string().contains("execution step limit exceeded"));
    }

    #[test]
    fn vm_call_depth_limited_execution_stops_deep_recursion() {
        let source = "function loop() { return loop(); } loop();";
        let error =
            run_vm_source_with_output_and_limits(source, 100_000, 4, 256).unwrap_err();
        assert!(error.to_string().contains("call depth limit exceeded"));
    }

    #[test]
    fn vm_output_limit_truncates_extra_lines() {
        let source = "print(1); print(2); print(3);";
        let (_, output, truncated) =
            run_vm_source_with_output_and_limits(source, 100_000, 16, 2).unwrap();
        assert_eq!(output, vec!["1".to_string(), "2".to_string()]);
        assert!(truncated);
    }

    #[test]
    fn vm_print_output_is_captured() {
        let (value, output) = run_vm_source_with_output("print(1); print(2); 42;").unwrap();
        assert_eq!(value, Value::Number(42.0));
        assert_eq!(output, vec!["1".to_string(), "2".to_string()]);
    }

    #[test]
    fn vm_missing_function_args_become_undefined() {
        assert_eq!(
            run("function f(a,b){ return typeof b; } f(1);"),
            Value::String("undefined".into())
        );
    }

    #[test]
    fn vm_rest_params() {
        let src = "function f(a, ...rest) { return rest.length; } f(1, 2, 3);";
        assert_eq!(run(src), Value::Number(2.0));
    }

    #[test]
    fn vm_nested_try_finally_runs_then_inner_throw_wins() {
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
        assert_eq!(run(src), Value::String("inner:2".into()));
    }

    #[test]
    fn vm_break_in_finally_wins_over_try_value() {
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
        assert_eq!(run(src), Value::String("tried:1".into()));
    }

    #[test]
    fn vm_array_method_calls() {
        let src = "let a = [1, 2, 3]; a.map(function(v){ return v * 3; }).join('|');";
        assert_eq!(run(src), Value::String("3|6|9".into()));
    }

    #[test]
    fn vm_string_methods() {
        assert_eq!(run("'hello'.slice(1, 3);"), Value::String("el".into()));
        assert_eq!(run("'ab'.toUpperCase();"), Value::String("AB".into()));
    }

    #[test]
    fn vm_json_stringify() {
        let src = "let o = {a: 1, b: [2, 3]}; JSON.stringify(o);";
        assert_eq!(run(src), Value::String("{\"a\":1,\"b\":[2,3]}".into()));
    }

    #[test]
    fn vm_conditional_expression() {
        assert_eq!(run("let x = 1 > 0 ? 'yes' : 'no'; x;"), Value::String("yes".into()));
    }

    #[test]
    fn vm_bitwise_ops() {
        assert_eq!(run("5 & 3;"), Value::Number(1.0));
        assert_eq!(run("5 | 3;"), Value::Number(7.0));
        assert_eq!(run("5 ^ 3;"), Value::Number(6.0));
        assert_eq!(run("1 << 3;"), Value::Number(8.0));
        assert_eq!(run("8 >> 2;"), Value::Number(2.0));
        assert_eq!(run("16 >>> 2;"), Value::Number(4.0));
    }

    #[test]
    fn vm_void_and_delete() {
        assert_eq!(run("void 0;"), Value::Undefined);
        let src = "let o = {a: 1}; delete o.a; o.a === undefined;";
        assert_eq!(run(src), Value::Bool(true));
    }
}
