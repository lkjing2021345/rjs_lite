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
    VarDecl {
        name: Pattern,
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
    Array(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    Function {
        name: Option<String>,
        params: Vec<Pattern>,
        body: Vec<Stmt>,
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
    },
    /// `await expr` — only valid inside async function bodies.
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
