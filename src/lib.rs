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
    let (value, output, _) = run_source_with_output_inner(source, interpreter::Interpreter::new())?;
    Ok((value, output))
}

pub fn run_source_with_limited_output(
    source: &str,
    max_output_lines: usize,
) -> JsResult<(Value, Vec<String>, bool)> {
    run_source_with_output_inner(
        source,
        interpreter::Interpreter::with_output_limit(max_output_lines),
    )
}

fn run_source_with_output_inner(
    source: &str,
    mut interpreter: interpreter::Interpreter,
) -> JsResult<(Value, Vec<String>, bool)> {
    let tokens = lexer::lex(source)?;
    let program = parser::parse(tokens)?;
    let value = interpreter.run(&program)?;
    let (output, output_truncated) = interpreter.take_output_with_truncation();
    Ok((value, output, output_truncated))
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
    fn limited_output_stops_collecting_extra_lines() {
        let source = r#"
            print(1);
            print(2);
            print(3);
        "#;
        let (value, output, output_truncated) = run_source_with_limited_output(source, 2).unwrap();

        assert_eq!(value, Value::Undefined);
        assert_eq!(output, vec!["1".to_string(), "2".to_string()]);
        assert!(output_truncated);
    }

    #[test]
    fn limited_output_is_not_truncated_at_limit() {
        let source = r#"
            print(1);
            print(2);
        "#;
        let (_, output, output_truncated) = run_source_with_limited_output(source, 2).unwrap();

        assert_eq!(output, vec!["1".to_string(), "2".to_string()]);
        assert!(!output_truncated);
    }
}
