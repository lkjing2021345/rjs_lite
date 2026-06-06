use crate::ast::{BinaryOp, Expr, Program, Stmt, UnaryOp};
use crate::error::{JsError, JsResult};
use crate::value::{Function, Value};
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
        Err(JsError::runtime(format!("undefined variable `{name}`")))
    }
}

enum Flow {
    Value(Value),
    Return(Value),
}

pub struct Interpreter {
    env: Rc<RefCell<Env>>,
    output: Vec<String>,
    step_limit: Option<usize>,
    steps: usize,
}

impl Interpreter {
    pub fn new() -> Self {
        Self::new_with_step_limit(None)
    }

    pub fn with_step_limit(step_limit: usize) -> Self {
        Self::new_with_step_limit(Some(step_limit))
    }

    fn new_with_step_limit(step_limit: Option<usize>) -> Self {
        let env = Env::new();
        env.borrow_mut()
            .define("print".into(), Value::NativeFunction("print", 1), false);
        Self {
            env,
            output: Vec::new(),
            step_limit,
            steps: 0,
        }
    }

    pub fn run(&mut self, program: &Program) -> JsResult<Value> {
        match self.eval_statements(&program.statements)? {
            Flow::Value(v) | Flow::Return(v) => Ok(v),
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
                r @ Flow::Return(_) => return Ok(r),
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
                self.env.borrow_mut().define(name.clone(), value, *mutable);
                Ok(Flow::Value(Value::Undefined))
            }
            Stmt::FunctionDecl { name, params, body } => {
                self.env.borrow_mut().define(
                    name.clone(),
                    Value::Function(Function {
                        params: params.clone(),
                        body: body.clone(),
                    }),
                    false,
                );
                Ok(Flow::Value(Value::Undefined))
            }
            Stmt::Return(value) => Ok(Flow::Return(
                value
                    .as_ref()
                    .map(|v| self.eval_expr(v))
                    .transpose()?
                    .unwrap_or(Value::Undefined),
            )),
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
                        r @ Flow::Return(_) => return Ok(r),
                    }
                }
                Ok(Flow::Value(last))
            }
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

    fn eval_expr(&mut self, expr: &Expr) -> JsResult<Value> {
        self.step()?;
        match expr {
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::String(s) => Ok(Value::String(s.clone())),
            Expr::Bool(v) => Ok(Value::Bool(*v)),
            Expr::Null => Ok(Value::Null),
            Expr::Undefined => Ok(Value::Undefined),
            Expr::Identifier(name) => self
                .env
                .borrow()
                .get(name)
                .ok_or_else(|| JsError::runtime(format!("undefined variable `{name}`"))),
            Expr::Assign { name, value } => {
                let value = self.eval_expr(value)?;
                self.env.borrow_mut().assign(name, value.clone())?;
                Ok(value)
            }
            Expr::Unary { op, expr } => {
                let value = self.eval_expr(expr)?;
                match op {
                    UnaryOp::Not => Ok(Value::Bool(!value.is_truthy())),
                    UnaryOp::Negate => Ok(Value::Number(-number(value)?)),
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
                let callee = self.eval_expr(callee)?;
                let args = args
                    .iter()
                    .map(|a| self.eval_expr(a))
                    .collect::<JsResult<Vec<_>>>()?;
                self.call(callee, args)
            }
        }
    }

    fn binary(&self, left: Value, op: BinaryOp, right: Value) -> JsResult<Value> {
        match op {
            BinaryOp::Add => match (left, right) {
                (Value::String(a), b) => Ok(Value::String(a + &b.to_string())),
                (a, Value::String(b)) => Ok(Value::String(a.to_string() + &b)),
                (a, b) => Ok(Value::Number(number(a)? + number(b)?)),
            },
            BinaryOp::Subtract => Ok(Value::Number(number(left)? - number(right)?)),
            BinaryOp::Multiply => Ok(Value::Number(number(left)? * number(right)?)),
            BinaryOp::Divide => Ok(Value::Number(number(left)? / number(right)?)),
            BinaryOp::Remainder => Ok(Value::Number(number(left)? % number(right)?)),
            BinaryOp::Equal | BinaryOp::StrictEqual => Ok(Value::Bool(left == right)),
            BinaryOp::NotEqual | BinaryOp::StrictNotEqual => Ok(Value::Bool(left != right)),
            BinaryOp::Less => Ok(Value::Bool(number(left)? < number(right)?)),
            BinaryOp::LessEqual => Ok(Value::Bool(number(left)? <= number(right)?)),
            BinaryOp::Greater => Ok(Value::Bool(number(left)? > number(right)?)),
            BinaryOp::GreaterEqual => Ok(Value::Bool(number(left)? >= number(right)?)),
            BinaryOp::And | BinaryOp::Or => unreachable!("short-circuited before binary eval"),
        }
    }

    fn call(&mut self, callee: Value, args: Vec<Value>) -> JsResult<Value> {
        match callee {
            Value::NativeFunction("print", 1) => {
                let text = args
                    .first()
                    .cloned()
                    .unwrap_or(Value::Undefined)
                    .to_string();
                self.output.push(text);
                Ok(Value::Undefined)
            }
            Value::Function(function) => {
                if args.len() != function.params.len() {
                    return Err(JsError::runtime(format!(
                        "expected {} arguments, got {}",
                        function.params.len(),
                        args.len()
                    )));
                }
                let previous = self.env.clone();
                self.env = Env::child(previous.clone());
                for (name, value) in function.params.into_iter().zip(args) {
                    self.env.borrow_mut().define(name, value, true);
                }
                let result = self.eval_statements(&function.body);
                self.env = previous;
                match result? {
                    Flow::Value(v) | Flow::Return(v) => Ok(v),
                }
            }
            other => Err(JsError::runtime(format!(
                "{} is not callable",
                other.type_name()
            ))),
        }
    }

    fn step(&mut self) -> JsResult<()> {
        self.steps = self.steps.saturating_add(1);
        if self.step_limit.is_some_and(|limit| self.steps > limit) {
            return Err(JsError::runtime("execution step limit exceeded"));
        }
        Ok(())
    }
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

fn number(value: Value) -> JsResult<f64> {
    if let Value::Number(n) = value {
        Ok(n)
    } else {
        Err(JsError::runtime(format!(
            "expected number, got {}",
            value.type_name()
        )))
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
}
