pub mod agent;
pub mod ast;
pub mod error;
pub mod interpreter;
pub mod lexer;
pub mod parser;
pub mod token;
pub mod value;

pub use agent::{
    AgentRuntime, AgentToolResult, ExecutionContext, HostFunction, RuntimeLimits, run_agent_tool,
};
pub use error::{JsError, JsResult, Span};
pub use value::Value;

pub fn run_source(source: &str) -> JsResult<Value> {
    let tokens = lexer::lex(source)?;
    let program = parser::parse(tokens)?;
    interpreter::Interpreter::new().run(&program)
}

pub fn run_source_with_output(source: &str) -> JsResult<(Value, Vec<String>)> {
    let tokens = lexer::lex(source)?;
    let program = parser::parse(tokens)?;
    let mut interpreter = interpreter::Interpreter::new();
    let value = interpreter.run(&program)?;
    Ok((value, interpreter.take_output()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_arithmetic_and_variables() {
        let value = run_source("let x = 1 + 2 * 3; x;").unwrap();
        assert_eq!(value, Value::Number(7.0));
    }

    #[test]
    fn evaluates_functions_and_loops() {
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
        assert_eq!(run_source(source).unwrap(), Value::Number(10.0));
    }

    #[test]
    fn evaluates_uninitialized_let_as_undefined() {
        assert_eq!(run_source("let x; x;").unwrap(), Value::Undefined);
    }

    #[test]
    fn uninitialized_let_can_be_assigned_later() {
        assert_eq!(run_source("let x; x = 4; x;").unwrap(), Value::Number(4.0));
    }
}
