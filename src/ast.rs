#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub statements: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    VarDecl {
        name: String,
        value: Expr,
        mutable: bool,
    },
    VarDecls {
        declarations: Vec<(String, Expr)>,
        mutable: bool,
    },
    FunctionDecl {
        name: String,
        params: Vec<String>,
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
    String(String),
    Bool(bool),
    Null,
    Undefined,
    This,
    Identifier(String),
    Array(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    Function {
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    ArrowFunction {
        params: Vec<String>,
        body: Vec<Stmt>,
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
