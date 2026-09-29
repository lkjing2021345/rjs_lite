#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub statements: Vec<Stmt>,
}

/// A binding pattern used by variable declarations and function parameters.
///
/// `Identifier` is the common case. The container variants hold nested
/// patterns; `Rest` captures the remaining elements/values; `Default` falls
/// back to an expression when the extracted value is `undefined`.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Identifier(String),
    ArrayPattern(Vec<Pattern>),
    ObjectPattern(Vec<ObjectPatternEntry>),
    Rest(Box<Pattern>),
    Default(Box<Pattern>, Expr),
}

/// One `key: pattern` entry of an object destructuring pattern. The shorthand
/// form `{ a }` is stored as key `"a"` with an `Identifier("a")` value.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectPatternEntry {
    pub key: String,
    pub value: Pattern,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    VarDecl {        name: Pattern,
        value: Expr,
        mutable: bool,
    },
    VarDecls {
        declarations: Vec<(Pattern, Expr)>,
        mutable: bool,
    },
    FunctionDecl {
        name: String,
        params: Vec<Pattern>,
        body: Vec<Stmt>,
        /// True for `function* name(...) { ... }` generator declarations.
        generator: bool,
    },
    /// `class Name [extends Super] { ... }` declaration.
    ClassDecl {
        name: String,
        extends: Option<Box<Expr>>,
        body: Vec<ClassElement>,
    },
    Return(Option<Expr>),
    Throw(Expr),
    Try {
        block: Vec<Stmt>,
        catch_param: Option<String>,
        catch_block: Option<Vec<Stmt>>,
        finally_block: Option<Vec<Stmt>>,
    },
    If {
        condition: Expr,
        then_branch: Vec<Stmt>,
        else_branch: Vec<Stmt>,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
    },
    For {
        init: Option<Box<Stmt>>,
        condition: Option<Expr>,
        update: Option<Expr>,
        body: Vec<Stmt>,
    },
    ForIn {
        left: Box<Expr>,
        right: Expr,
        body: Vec<Stmt>,
    },
    Switch {
        discriminant: Expr,
        cases: Vec<(Expr, Vec<Stmt>)>,
        default: Vec<Stmt>,
    },
    Block(Vec<Stmt>),
    Break,
    Continue,
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    BigInt(String),
    String(String),
    Bool(bool),
    Null,
    Undefined,
    This,
    Identifier(String),
    Array(Vec<Option<Expr>>),
    Object(Vec<(Option<String>, Expr)>),
    Function {
        name: Option<String>,
        params: Vec<Pattern>,
        body: Vec<Stmt>,
        /// True for `function* (...) { ... }` generator functions.
        generator: bool,
    },
    ArrowFunction {
        params: Vec<Pattern>,
        body: Vec<Stmt>,
    },
    /// `async function` expression / `async (params) =>` arrow.
    AsyncFunction {
        name: Option<String>,
        params: Vec<Pattern>,
        body: Vec<Stmt>,
        /// True for `async function* (...) { ... }` async generators.
        generator: bool,
    },
    /// `yield expr` — only valid inside generator function bodies. A bare
    /// `yield` is represented as `Yield(Undefined)`.
    Yield(Box<Expr>),    /// `await expr` — only valid inside async function bodies.
    Await(Box<Expr>),
    TemplateLiteral {
        parts: Vec<Expr>,
    },
    RegExp {
        pattern: String,
        flags: String,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Typeof(Box<Expr>),
    Conditional {
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
    },
    /// A destructuring assignment such as `[a, b] = rhs` or `{ x, y } = rhs`.
    /// Evaluates to the right-hand side value so it chains like a normal
    /// assignment (`result = [a, b] = [1, 2]`).
    DestructuringAssign {
        pattern: Box<Pattern>,
        value: Box<Expr>,
    },
    CompoundAssign {
        target: Box<Expr>,
        op: BinaryOp,
        value: Box<Expr>,
    },
    Update {
        target: Box<Expr>,
        delta: f64,
        prefix: bool,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    New {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Member {
        object: Box<Expr>,
        property: String,
    },
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
    },
    /// `class [Name] [extends Super] { ... }` expression.
    Class {
        name: Option<String>,
        extends: Option<Box<Expr>>,
        body: Vec<ClassElement>,
    },
    /// The `super` keyword. Used as a call callee (`super(...)`) or as the
    /// object of a member call (`super.method(...)`).
    Super,
}

/// One member of a class body.
#[derive(Debug, Clone, PartialEq)]
pub enum ClassElement {
    /// A method (including `static`, `async`, and generator methods).
    Method {
        name: String,
        params: Vec<Pattern>,
        body: Vec<Stmt>,
        is_static: bool,
        is_generator: bool,
        is_async: bool,
    },
    /// A public or private field with an optional initializer.
    Field {
        name: String,
        init: Option<Box<Expr>>,
        is_static: bool,
        is_private: bool,
    },
    /// A `get name() { ... }` accessor.
    Getter {
        name: String,
        body: Vec<Stmt>,
        is_static: bool,
    },
    /// A `set name(param) { ... }` accessor.
    Setter {
        name: String,
        param: Pattern,
        body: Vec<Stmt>,
        is_static: bool,
    },
    /// The `constructor(...) { ... }` method.
    Constructor {
        params: Vec<Pattern>,
        body: Vec<Stmt>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,
    Not,
    Delete,
    Void,
    BitwiseNot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Equal,
    NotEqual,
    StrictEqual,
    StrictNotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
    In,
    Instanceof,
    BitwiseAnd,
    BitwiseOr,
    BitwiseXor,
    LeftShift,
    RightShift,
    UnsignedRightShift,
}
