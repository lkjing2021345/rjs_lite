pub mod agent;
pub mod ast;
pub mod bytecode;
pub mod compiler;
pub mod error;
pub mod interpreter;
pub mod lexer;
pub mod parser;
pub mod token;
pub mod value;
pub mod vm;

pub use agent::{
    AgentErrorKind, AgentRuntime, AgentToolResult, ExecutionContext, HostFunction, RuntimeLimits,
    run_agent_tool,
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

pub fn run_source_with_output_and_step_limit(
    source: &str,
    max_execution_steps: usize,
) -> JsResult<(Value, Vec<String>)> {
    let (value, output, _) = run_source_with_output_inner(
        source,
        interpreter::Interpreter::with_step_limit(max_execution_steps),
    )?;
    Ok((value, output))
}

pub fn run_source_with_output_and_call_depth_limit(
    source: &str,
    max_call_depth: usize,
) -> JsResult<(Value, Vec<String>)> {
    let (value, output, _) = run_source_with_output_inner(
        source,
        interpreter::Interpreter::with_call_depth_limit(max_call_depth),
    )?;
    Ok((value, output))
}

pub fn run_source_with_output_and_limits(
    source: &str,
    max_execution_steps: usize,
    max_call_depth: usize,
) -> JsResult<(Value, Vec<String>)> {
    let (value, output, _) = run_source_with_output_inner(
        source,
        interpreter::Interpreter::with_limits(max_execution_steps, max_call_depth),
    )?;
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

pub fn run_source_with_output_and_all_limits(
    source: &str,
    max_execution_steps: usize,
    max_call_depth: usize,
    max_output_lines: usize,
) -> JsResult<(Value, Vec<String>, bool)> {
    run_source_with_output_inner(
        source,
        interpreter::Interpreter::with_limits_and_output_limit(
            max_execution_steps,
            max_call_depth,
            max_output_lines,
        ),
    )
}

pub fn run_vm_source(source: &str) -> JsResult<Value> {
    vm::run_vm_source(source)
}

pub fn run_vm_source_with_output(source: &str) -> JsResult<(Value, Vec<String>)> {
    vm::run_vm_source_with_output(source)
}

pub fn run_vm_source_with_output_and_limits(
    source: &str,
    max_execution_steps: usize,
    max_call_depth: usize,
    max_output_lines: usize,
) -> JsResult<(Value, Vec<String>, bool)> {
    vm::run_vm_source_with_output_and_limits(
        source,
        max_execution_steps,
        max_call_depth,
        max_output_lines,
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
    fn step_limited_execution_allows_finite_loops() {
        let source = "let x = 0; while (x < 3) { x = x + 1; } x;";
        let (value, output) = run_source_with_output_and_step_limit(source, 100).unwrap();

        assert_eq!(value, Value::Number(3.0));
        assert!(output.is_empty());
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
    fn step_limited_execution_stops_infinite_loops() {
        let error = run_source_with_output_and_step_limit("while (true) {}", 10).unwrap_err();

        assert!(error.to_string().contains("execution step limit exceeded"));
    }

    #[test]
    fn call_depth_limited_execution_stops_deep_recursion() {
        let source = "function loop() { return loop(); } loop();";
        let error = run_source_with_output_and_call_depth_limit(source, 4).unwrap_err();

        assert!(error.to_string().contains("call depth limit exceeded"));
    }

    #[test]
    fn evaluates_typeof_operator() {
        assert_eq!(
            run_source("typeof 1;").unwrap(),
            Value::String("number".to_string())
        );
        assert_eq!(
            run_source("typeof 'x';").unwrap(),
            Value::String("string".to_string())
        );
        assert_eq!(
            run_source("typeof true;").unwrap(),
            Value::String("boolean".to_string())
        );
        assert_eq!(
            run_source("typeof null;").unwrap(),
            Value::String("object".to_string())
        );
        assert_eq!(
            run_source("typeof undefined;").unwrap(),
            Value::String("undefined".to_string())
        );
        assert_eq!(
            run_source("function f() {} typeof f;").unwrap(),
            Value::String("function".to_string())
        );
        assert_eq!(
            run_source("typeof missing;").unwrap(),
            Value::String("undefined".to_string())
        );
    }

    #[test]
    fn evaluates_uninitialized_let_as_undefined() {
        assert_eq!(run_source("let x; x;").unwrap(), Value::Undefined);
    }

    #[test]
    fn uninitialized_let_can_be_assigned_later() {
        assert_eq!(run_source("let x; x = 4; x;").unwrap(), Value::Number(4.0));
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

    #[test]
    fn destructures_array_with_let() {
        assert_eq!(
            run_source("let [a, b] = [1, 2]; a + b;").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn destructures_object_with_let() {
        assert_eq!(
            run_source("let {a, b} = {a: 1, b: 2}; a + b;").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn destructures_array_rest() {
        assert_eq!(
            run_source("let [a, ...rest] = [1, 2, 3]; a + rest.length;").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn destructures_object_renaming() {
        assert_eq!(
            run_source("let {a: x, b} = {a: 1, b: 2}; x + b;").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn destructures_function_params() {
        assert_eq!(
            run_source("function foo([a, b]) { return a + b; } foo([3, 4]);").unwrap(),
            Value::Number(7.0)
        );
    }

    #[test]
    fn destructures_array_defaults() {
        assert_eq!(
            run_source("let [a = 10] = [1]; a;").unwrap(),
            Value::Number(1.0)
        );
        assert_eq!(
            run_source("let [a = 10] = []; a;").unwrap(),
            Value::Number(10.0)
        );
    }

    #[test]
    fn destructures_nested_patterns() {
        assert_eq!(
            run_source("let [[a], {b}] = [[1], {b: 2}]; a + b;").unwrap(),
            Value::Number(3.0)
        );
        assert_eq!(
            run_source("function f({a, b: y}) { return a + y; } f({a: 1, b: 2});").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn destructures_declaration_lists() {
        assert_eq!(
            run_source("let [a, b] = [1, 2], c = 3; a + b + c;").unwrap(),
            Value::Number(6.0)
        );
    }
}
