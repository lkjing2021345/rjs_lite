//! AST -> bytecode compiler.
//!
//! Walks a parsed [`crate::ast::Program`] and emits [`crate::bytecode::Program`].
//! Control flow (if/else, loops, switch, for-in, try/catch/finally) is compiled
//! with two passes: jumps are emitted with a placeholder (`usize::MAX`) and the
//! real target is patched in once the branch is fully laid out (`backpatch`).
//!
//! Operand-stack conventions (bottom to top):
//! - `Call`/`New` expect `[this, callee, arg0, .., argN]`.
//! - `SetMember`/`SetIndex`/`SetLocal` expect `[value, object, (key)]`.
//! - `GetMember`/`GetIndex` expect `[object, (key)]` and leave the value on top.

use crate::ast::{BinaryOp, Expr, Stmt, UnaryOp};
use crate::bytecode::{FunctionBytecode, Instruction, Program};

/// Context for `break` / `continue` while compiling a function body.
#[derive(Clone, Copy)]
struct LoopContext {
    break_target: usize,
    continue_target: usize,
}

struct Compiler<'a> {
    program: &'a mut Program,
    /// Stack of loop contexts for `break` / `continue` resolution.
    loops: Vec<LoopContext>,
}

impl<'a> Compiler<'a> {
    /// Compile a full program into bytecode.
    pub fn compile(program: &crate::ast::Program) -> Program {
        let mut out = Program::new();
        let mut compiler = Compiler {
            program: &mut out,
            loops: Vec::new(),
        };
        compiler.emit_statements(&program.statements);
        out
    }

    // --- Instruction stream helpers ---

    fn stream(&self) -> &Vec<Instruction> {
        &self.program.instructions
    }

    fn emit(&mut self, instr: Instruction) -> usize {
        let idx = self.program.instructions.len();
        self.program.instructions.push(instr);
        idx
    }

    /// Emit a jump with a placeholder target; returns the index to backpatch.
    fn emit_jump(&mut self, make: fn(usize) -> Instruction) -> usize {
        self.emit(make(usize::MAX))
    }

    fn backpatch(&mut self, idx: usize, target: usize) {
        let instr = std::mem::replace(&mut self.program.instructions[idx], Instruction::Pop);
        self.program.instructions[idx] = match instr {
            Instruction::Jump(_) => Instruction::Jump(target),
            Instruction::JumpIfFalse(_) => Instruction::JumpIfFalse(target),
            Instruction::JumpIfTrue(_) => Instruction::JumpIfTrue(target),
            Instruction::Loop(_) => Instruction::Loop(target),
            other => panic!("backpatch: index {idx} is not a jump instruction: {other:?}"),
        };
    }

    /// Compile a function body into a new `FunctionBytecode` and return its
    /// index in the program's function table.
    fn compile_function(&mut self, params: &[String], body: &[Stmt]) -> usize {
        let mut func = FunctionBytecode {
            params: params.to_vec(),
            instructions: Vec::new(),
        };
        // Compile the body into a separate instruction stream, then move it
        // into the function.
        let mut sub_program = Program::new();
        let mut sub = Compiler {
            program: &mut sub_program,
            loops: Vec::new(),
        };
        sub.emit_statements(body);
        func.instructions = std::mem::take(&mut sub_program.instructions);
        self.program.functions.push(func);
        self.program.functions.len() - 1
    }

    // --- Statements ---

    fn emit_statements(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.emit_stmt(s);
        }
    }

    fn emit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::VarDecl {
                name,
                value,
                mutable,
            } => {
                self.emit_expr(value);
                self.emit(Instruction::DefineLocal(name.clone(), *mutable));
            }
            Stmt::VarDecls {
                declarations,
                mutable,
            } => {
                for (name, value) in declarations {
                    self.emit_expr(value);
                    self.emit(Instruction::DefineLocal(name.clone(), *mutable));
                }
            }
            Stmt::FunctionDecl { name, params, body } => {
                let func_idx = self.compile_function(params, body);
                self.emit(Instruction::PushFunction(func_idx));
                self.emit(Instruction::DefineLocal(name.clone(), false));
            }
            Stmt::Return(value) => {
                match value {
                    Some(v) => self.emit_expr(v),
                    None => { self.emit(Instruction::PushUndefined); }
                }
                self.emit(Instruction::Return);
            }
            Stmt::Throw(e) => {
                self.emit_expr(e);
                self.emit(Instruction::Throw);
            }
            Stmt::Try {
                block,
                catch_param,
                catch_block,
                finally_block,
            } => {
                let try_idx = self.emit(Instruction::TryBegin {
                    block_end: usize::MAX,
                    catch: None,
                    finally: None,
                });
                self.emit_statements(block);
                let block_end = self.stream().len();

                // Catch block: the normal path jumps over it.
                let mut catch_idx = None;
                if let Some(catch) = catch_block {
                    catch_idx = Some(self.stream().len());
                    self.emit(Instruction::CatchParam(catch_param.clone()));
                    self.emit_statements(catch);
                }

                // Finally block.
                let mut finally_idx = None;
                if let Some(finally) = finally_block {
                    finally_idx = Some(self.stream().len());
                    self.emit_statements(finally);
                }

                self.program.instructions[try_idx] = Instruction::TryBegin {
                    block_end,
                    catch: catch_idx,
                    finally: finally_idx,
                };
                // Normal path: skip past the catch/finally handlers.
                if catch_idx.is_some() || finally_idx.is_some() {
                    self.emit(Instruction::Jump(self.stream().len() + 1));
                }
                self.emit(Instruction::EndTry);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.emit_expr(condition);
                let jump_false = self.emit_jump(|t| Instruction::JumpIfFalse(t));
                self.emit_statements(then_branch);
                let end = self.stream().len();
                if else_branch.is_empty() {
                    self.backpatch(jump_false, end);
                } else {
                    let jump_over = self.emit_jump(|t| Instruction::Jump(t));
                    self.backpatch(jump_false, self.stream().len());
                    self.emit_statements(else_branch);
                    self.backpatch(jump_over, self.stream().len());
                }
            }
            Stmt::While { condition, body } => {
                let loop_start = self.stream().len();
                let break_target = self.stream().len();
                self.emit_expr(condition);
                let exit_jump = self.emit_jump(|t| Instruction::JumpIfFalse(t));
                self.loops.push(LoopContext {
                    break_target,
                    continue_target: loop_start,
                });
                self.emit_statements(body);
                self.loops.pop();
                self.emit(Instruction::Loop(loop_start));
                self.backpatch(exit_jump, self.stream().len());
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.emit_stmt(init);
                }
                let loop_start = self.stream().len();
                let continue_target = self.stream().len();
                if let Some(cond) = condition {
                    self.emit_expr(cond);
                    let exit_jump = self.emit_jump(|t| Instruction::JumpIfFalse(t));
                    self.loops.push(LoopContext {
                        break_target: self.stream().len(),
                        continue_target,
                    });
                    self.emit_statements(body);
                    self.loops.pop();
                    if let Some(update) = update {
                        self.emit_expr(update);
                    }
                    self.emit(Instruction::Loop(loop_start));
                    self.backpatch(exit_jump, self.stream().len());
                } else {
                    // No condition: infinite loop.
                    self.loops.push(LoopContext {
                        break_target: self.stream().len(),
                        continue_target,
                    });
                    self.emit_statements(body);
                    self.loops.pop();
                    if let Some(update) = update {
                        self.emit_expr(update);
                    }
                    self.emit(Instruction::Loop(loop_start));
                }
            }
            Stmt::ForIn { left, right, body } => {
                self.emit_expr(right);
                let loop_start = self.stream().len();
                let header_idx = self.emit(Instruction::ForIn(usize::MAX));
                self.loops.push(LoopContext {
                    break_target: self.stream().len(),
                    continue_target: loop_start,
                });
                // Assign the current key to `left`.
                self.emit(Instruction::GetLocal("__forin_key__".to_string()));
                self.emit_target(left);
                self.emit_statements(body);
                self.loops.pop();
                self.emit(Instruction::ForInEnd);
                self.program.instructions[header_idx] = Instruction::ForIn(loop_start);
            }
            Stmt::Switch {
                discriminant,
                cases,
                default,
            } => {
                self.emit_expr(discriminant);
                let mut case_jumps: Vec<usize> = Vec::new();
                for (test, body) in cases {
                    self.emit(Instruction::Dup);
                    self.emit_expr(test);
                    self.emit(Instruction::Binary(BinaryOp::Equal));
                    case_jumps.push(self.emit_jump(|t| Instruction::JumpIfFalse(t)));
                    self.emit_statements(body);
                }
                let default_idx = self.stream().len();
                for jump in case_jumps {
                    self.backpatch(jump, default_idx);
                }
                self.emit_statements(default);
            }
            Stmt::Block(stmts) => self.emit_statements(stmts),
            Stmt::Break => {
                let target = self
                    .loops
                    .last()
                    .map(|l| l.break_target)
                    .expect("break outside of loop");
                self.emit(Instruction::Jump(target));
            }
            Stmt::Continue => {
                let target = self
                    .loops
                    .last()
                    .map(|l| l.continue_target)
                    .expect("continue outside of loop");
                self.emit(Instruction::Jump(target));
            }
            Stmt::Expr(e) => {
                self.emit_expr(e);
                self.emit(Instruction::Pop);
            }
        }
    }

    // --- Expressions ---

    fn emit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Number(n) => { self.emit(Instruction::PushNumber(*n)); }
            Expr::String(s) => { self.emit(Instruction::PushString(s.clone())); }
            Expr::Bool(b) => { self.emit(Instruction::PushBool(*b)); }
            Expr::Null => { self.emit(Instruction::PushNull); }
            Expr::Undefined => { self.emit(Instruction::PushUndefined); }
            Expr::This => { self.emit(Instruction::PushThis); }
            Expr::Identifier(name) => { self.emit(Instruction::GetLocal(name.clone())); }
            Expr::Array(items) => {
                for item in items {
                    self.emit_expr(item);
                }
                self.emit(Instruction::PushArray(items.len()));
            }
            Expr::Object(props) => {
                self.emit(Instruction::PushObject);
                for (key, value) in props {
                    self.emit_expr(value);
                    self.emit(Instruction::SetObjectProperty(key.clone()));
                }
            }
            Expr::Function { params, body } => {
                let idx = self.compile_function(params, body);
                self.emit(Instruction::PushFunction(idx));
            }
            Expr::ArrowFunction { params, body } => {
                let idx = self.compile_function(params, body);
                self.emit(Instruction::PushFunction(idx));
            }
            Expr::TemplateLiteral { parts } => {
                self.emit(Instruction::PushString(String::new()));
                for part in parts {
                    self.emit_expr(part);
                    self.emit(Instruction::ConcatTemplate);
                }
            }
            Expr::RegExp { pattern, flags } => {
                self.emit(Instruction::PushRegExp(pattern.clone(), flags.clone()));
            }
            Expr::Unary { op, expr } => {
                self.emit_expr(expr);
                self.emit(Instruction::Unary(*op));
            }
            Expr::Typeof(expr) => {
                self.emit_expr(expr);
                self.emit(Instruction::Typeof);
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                self.emit_expr(condition);
                let jump_false = self.emit_jump(|t| Instruction::JumpIfFalse(t));
                self.emit_expr(then_expr);
                let jump_over = self.emit_jump(|t| Instruction::Jump(t));
                self.backpatch(jump_false, self.stream().len());
                self.emit_expr(else_expr);
                self.backpatch(jump_over, self.stream().len());
            }
            Expr::Binary { left, op, right } => match op {
                BinaryOp::And => {
                    self.emit_expr(left);
                    let jump_true = self.emit_jump(|t| Instruction::JumpIfTrue(t));
                    self.emit(Instruction::Pop);
                    self.emit_expr(right);
                    self.backpatch(jump_true, self.stream().len());
                }
                BinaryOp::Or => {
                    self.emit_expr(left);
                    let jump_false = self.emit_jump(|t| Instruction::JumpIfFalse(t));
                    self.emit(Instruction::Pop);
                    self.emit_expr(right);
                    self.backpatch(jump_false, self.stream().len());
                }
                _ => {
                    self.emit_expr(left);
                    self.emit_expr(right);
                    self.emit(Instruction::Binary(*op));
                }
            },
            Expr::Assign { target, value } => {
                self.emit_expr(value);
                self.emit_target(target);
                self.emit(Instruction::Dup);
            }
            Expr::CompoundAssign { target, op, value } => {
                self.emit_target_ref(target);
                self.emit_expr(value);
                self.emit(Instruction::Binary(*op));
                self.emit_target(target);
                self.emit(Instruction::Dup);
            }
            Expr::Update {
                target,
                delta,
                prefix,
            } => {
                self.emit_target_ref(target);
                self.emit(Instruction::Dup);
                self.emit(Instruction::Update {
                    delta: *delta,
                    prefix: *prefix,
                });
                self.emit_target(target);
            }
            Expr::Call { callee, args } => {
                self.emit_callee(callee);
                for arg in args {
                    self.emit_expr(arg);
                }
                self.emit(Instruction::Call(args.len()));
            }
            Expr::New { callee, args } => {
                self.emit(Instruction::PushUndefined);
                self.emit_expr(callee);
                for arg in args {
                    self.emit_expr(arg);
                }
                self.emit(Instruction::New(args.len()));
            }
            Expr::Member { object, property } => {
                self.emit_expr(object);
                self.emit(Instruction::GetMember(property.clone()));
            }
            Expr::Index { object, index } => {
                self.emit_expr(object);
                self.emit_expr(index);
                self.emit(Instruction::GetIndex);
            }
        }
    }

    /// Emit the callee plus its `this` binding. Stack result (bottom to top):
    /// `[this, callee]`.
    fn emit_callee(&mut self, callee: &Expr) {
        match callee {
            Expr::Member { object, property } => {
                self.emit_expr(object);
                self.emit(Instruction::Dup);
                self.emit(Instruction::GetMember(property.clone()));
            }
            Expr::Index { object, index } => {
                self.emit_expr(object);
                self.emit(Instruction::Dup);
                self.emit_expr(index);
                self.emit(Instruction::GetIndex);
            }
            _ => {
                self.emit(Instruction::PushUndefined);
                self.emit_expr(callee);
            }
        }
    }

    /// Emit an assignment target: expects the value on top, stores it.
    fn emit_target(&mut self, target: &Expr) {
        match target {
            Expr::Identifier(name) => { self.emit(Instruction::SetLocal(name.clone())); }
            Expr::Member { object, property } => {
                self.emit_expr(object);
                self.emit(Instruction::SetMember(property.clone()));
            }
            Expr::Index { object, index } => {
                self.emit_expr(object);
                self.emit_expr(index);
                self.emit(Instruction::SetIndex);
            }
            _ => panic!("invalid assignment target: {target:?}"),
        }
    }

    /// Emit a read of the target's current value (for compound assign / update).
    fn emit_target_ref(&mut self, target: &Expr) {
        match target {
            Expr::Identifier(name) => { self.emit(Instruction::GetLocal(name.clone())); }
            Expr::Member { object, property } => {
                self.emit_expr(object);
                self.emit(Instruction::GetMember(property.clone()));
            }
            Expr::Index { object, index } => {
                self.emit_expr(object);
                self.emit_expr(index);
                self.emit(Instruction::GetIndex);
            }
            _ => panic!("invalid update target: {target:?}"),
        }
    }
}

/// Convenience: compile a parsed program into bytecode.
pub fn compile(program: &crate::ast::Program) -> Program {
    Compiler::compile(program)
}
