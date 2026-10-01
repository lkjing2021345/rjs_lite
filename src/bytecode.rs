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

/// The kind of a class member function stored by [`ClassInfo`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClassMethodKind {
    Method,
    Getter,
    Setter,
}

/// One method compiled for a class body.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassMethodInfo {
    pub name: String,
    pub kind: ClassMethodKind,
    pub is_static: bool,
    /// Index into `Program::functions` of the compiled method body.
    pub func: usize,
}

/// Everything the VM needs to build a class object. The constructor body is
/// compiled as an entry in `Program::functions` (with instance field
/// initializers already prepended); the superclass value is on the stack.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassInfo {
    /// Index into `Program::functions` of the constructor, if any.
    pub constructor: Option<usize>,
    pub methods: Vec<ClassMethodInfo>,
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
    /// Marks the top of the stack as a spread argument; the VM's `Call`/`New`
    /// handler flattens it before invoking.
    Spread,

    // --- Object / array construction ---
    PushArray(usize),
    PushObject,
    SetObjectProperty(String),
    PushRegExp(String, String),

    // --- Functions ---
    PushFunction(usize),
    /// Build a class object. The superclass value (or `undefined`) is popped
    /// from the stack and the class value is pushed. See [`ClassInfo`].
    MakeClass(ClassInfo),

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
    // --- For-of ---
    ForOf(usize),
    ForOfEnd,

    // --- Template literal ---
    ConcatTemplate,
}

/// A compiled function body (function declarations, function expressions,
/// arrow functions).
#[derive(Debug, Clone)]
pub struct FunctionBytecode {
    pub params: Vec<Pattern>,
    pub instructions: Vec<Instruction>,
    /// True for generator functions (`function*`, `async function*`). Calling
    /// one produces a generator object rather than running the body.
    pub is_generator: bool,
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
