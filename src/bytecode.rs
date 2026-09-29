//! Bytecode representation for the rjs_lite VM.
//!
//! This module defines the instruction set (`Instruction`), the constant pool
//! (`Constant`), and the container types (`Program` / `FunctionBytecode`) that
//! the AST -> bytecode compiler (`crate::compiler`) emits and the VM
//! (task 07) executes.
//!
//! Design notes:
//! - Literals and identifiers are stored in a per-`Program` constant pool and
//!   referenced by index, so hot instructions stay small.
//! - Jump targets and loop targets are absolute instruction offsets, resolved
//!   by the compiler's two-pass backpatching.
//! - `FunctionBytecode` is a separate instruction stream per function, so the
//!   VM can jump into a function body by switching streams.

use crate::ast::{BinaryOp, Pattern, UnaryOp};

/// A value stored in the constant pool. The compiler interns literals and
/// identifier names here; instructions reference them by index.
#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    Number(f64),
    BigInt(String),
    String(String),
    Bool(bool),
    Null,
    Undefined,
    /// Identifier name (used by `GetLocal` / `SetLocal` / `DefineLocal`).
    Name(String),
    /// Object property key (used by `SetObjectProperty`).
    Key(String),
    /// RegExp pattern.
    RegExpPattern(String),
    /// RegExp flags.
    RegExpFlags(String),
}

/// A single bytecode instruction.
#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    // --- Literals (constants) ---
    PushNumber(f64),
    PushBigInt(String),
    PushString(String),
    PushBool(bool),
    PushNull,
    PushUndefined,
    PushThis,

    // --- Variable access ---
    GetLocal(String),
    SetLocal(String),
    DefineLocal(String, bool),
    /// Pop a value and bind it against a destructuring pattern.
    DestructureDefine(Pattern, bool),

    // --- Property access ---
    GetMember(String),
    SetMember(String),
    GetIndex,
    SetIndex,
    DeleteMember(String),
    DeleteIndex,

    // --- Stack manipulation ---
    Dup,
    Pop,

    // --- Arithmetic / logical ---
    Binary(BinaryOp),
    Unary(UnaryOp),
    Typeof,
    In,
    Instanceof,
    Update {
        delta: f64,
        prefix: bool,
    },

    // --- Calls ---
    Call(usize),
    New(usize),

    // --- Object / array construction ---
    PushArray(usize),
    PushObject,
    SetObjectProperty(String),
    PushRegExp(String, String),

    // --- Functions ---
    PushFunction(usize),

    // --- Control flow ---
    Jump(usize),
    JumpIfFalse(usize),
    JumpIfTrue(usize),
    Loop(usize),
    Break,
    Continue,
    Return,
    Throw,

    // --- Exception handling ---
    TryBegin {
        block_end: usize,
        catch: Option<usize>,
        finally: Option<usize>,
    },
    CatchParam(Option<String>),
    EndTry,

    // --- For-in ---
    ForIn(usize),
    ForInEnd,

    // --- Template literal ---
    ConcatTemplate,
}

/// A compiled function body (function declarations, function expressions,
/// arrow functions).
#[derive(Debug, Clone)]
pub struct FunctionBytecode {
    pub params: Vec<Pattern>,
    pub instructions: Vec<Instruction>,
}

/// The compiled program: the top-level instruction stream plus the constant
/// pool and all nested function bodies.
#[derive(Debug, Clone)]
pub struct Program {
    pub instructions: Vec<Instruction>,
    pub constants: Vec<Constant>,
    pub functions: Vec<FunctionBytecode>,
}

impl Program {
    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
            constants: Vec::new(),
            functions: Vec::new(),
        }
    }
}
