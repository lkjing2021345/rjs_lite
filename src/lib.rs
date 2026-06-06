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
    run_source_with_output_inner(source, interpreter::Interpreter::new())
}

pub fn run_source_with_output_and_call_depth_limit(
    source: &str,
    max_call_depth: usize,
) -> JsResult<(Value, Vec<String>)> {
    run_source_with_output_inner(
        source,
        interpreter::Interpreter::with_call_depth_limit(max_call_depth),
    )
}

fn run_source_with_output_inner(
    source: &str,
    mut interpreter: interpreter::Interpreter,
) -> JsResult<(Value, Vec<String>)> {
    let tokens = lexer::lex(source)?;
    let program = parser::parse(tokens)?;
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
    fn call_depth_limited_execution_allows_shallow_recursion() {
        let source = r#"
            function count(n) {
                if (n < 1) {
                    return 0;
                }
                return count(n - 1) + 1;
            }
            count(3);
        "#;
        let (value, output) = run_source_with_output_and_call_depth_limit(source, 8).unwrap();

        assert_eq!(value, Value::Number(3.0));
        assert!(output.is_empty());
    }

    #[test]
    fn call_depth_limited_execution_stops_deep_recursion() {
        let source = "function loop() { return loop(); } loop();";
        let error = run_source_with_output_and_call_depth_limit(source, 4).unwrap_err();

        assert!(error.to_string().contains("call depth limit exceeded"));
    }
}
