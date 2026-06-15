use crate::{JsError, Value, run_source_with_output_and_step_limit};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLimits {
    pub max_source_bytes: usize,
    pub max_output_lines: usize,
    pub max_execution_steps: usize,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 64 * 1024,
            max_output_lines: 256,
            max_execution_steps: 100_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFunction {
    pub name: String,
    pub arity: usize,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionContext {
    pub request_id: String,
    pub limits: RuntimeLimits,
    pub host_functions: Vec<HostFunction>,
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self {
            request_id: "local".to_string(),
            limits: RuntimeLimits::default(),
            host_functions: vec![HostFunction {
                name: "print".to_string(),
                arity: 1,
                description: "Capture one value into the agent tool output array".to_string(),
            }],
        }
    }
}

pub struct AgentRuntime {
    context: ExecutionContext,
}

impl AgentRuntime {
    pub fn new(context: ExecutionContext) -> Self {
        Self { context }
    }

    pub fn run(&self, source: &str) -> AgentToolResult {
        if source.len() > self.context.limits.max_source_bytes {
            return AgentToolResult::failure(
                self.context.request_id.clone(),
                JsError::runtime(format!(
                    "source is {} bytes, limit is {} bytes",
                    source.len(),
                    self.context.limits.max_source_bytes
                )),
            );
        }

        match run_source_with_output_and_step_limit(source, self.context.limits.max_execution_steps)
        {
            Ok((value, mut output)) => {
                let output_truncated = output.len() > self.context.limits.max_output_lines;
                output.truncate(self.context.limits.max_output_lines);
                AgentToolResult {
                    ok: true,
                    request_id: self.context.request_id.clone(),
                    value_type: Some(value.type_name()),
                    value: Some(value),
                    output,
                    output_truncated,
                    error: None,
                }
            }
            Err(error) => AgentToolResult::failure(self.context.request_id.clone(), error),
        }
    }
}

impl Default for AgentRuntime {
    fn default() -> Self {
        Self::new(ExecutionContext::default())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentToolResult {
    pub ok: bool,
    pub request_id: String,
    pub value: Option<Value>,
    pub value_type: Option<&'static str>,
    pub output: Vec<String>,
    pub output_truncated: bool,
    pub error: Option<String>,
}

impl AgentToolResult {
    fn failure(request_id: String, error: JsError) -> Self {
        Self {
            ok: false,
            request_id,
            value: None,
            value_type: None,
            output: Vec::new(),
            output_truncated: false,
            error: Some(error.to_string()),
        }
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\"ok\":{},\"request_id\":{},\"value\":{},\"value_type\":{},\"output\":{},\"output_truncated\":{},\"error\":{}}}",
            self.ok,
            json_string(&self.request_id),
            json_optional_string(self.value.as_ref().map(ToString::to_string).as_deref()),
            json_optional_string(self.value_type),
            json_string_array(&self.output),
            self.output_truncated,
            json_optional_string(self.error.as_deref())
        )
    }
}

pub fn run_agent_tool(source: &str) -> AgentToolResult {
    AgentRuntime::default().run(source)
}

fn json_optional_string(value: Option<&str>) -> String {
    value.map(json_string).unwrap_or_else(|| "null".to_string())
}

fn json_string_array(values: &[String]) -> String {
    let items = values
        .iter()
        .map(|value| json_string(value))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{items}]")
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('\"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_tool_returns_structured_success() {
        let result = run_agent_tool("let x = 1 + 2; print(x); x;");
        assert!(result.ok);
        assert_eq!(result.value, Some(Value::Number(3.0)));
        assert_eq!(result.value_type, Some("number"));
        assert_eq!(result.output, vec!["3".to_string()]);
        assert_eq!(result.error, None);
    }

    #[test]
    fn agent_tool_converts_parse_error_to_result() {
        let result = run_agent_tool("let = ;");
        assert!(!result.ok);
        assert_eq!(result.value, None);
        assert!(result.output.is_empty());
        assert!(result.error.unwrap().contains("parse error"));
    }

    #[test]
    fn agent_tool_json_is_stable_and_escaped() {
        let result = AgentToolResult {
            ok: false,
            request_id: "req\n1".to_string(),
            value: None,
            value_type: None,
            output: vec!["a\"b".to_string()],
            output_truncated: false,
            error: Some("bad\\news".to_string()),
        };

        assert_eq!(
            result.to_json(),
            "{\"ok\":false,\"request_id\":\"req\\n1\",\"value\":null,\"value_type\":null,\"output\":[\"a\\\"b\"],\"output_truncated\":false,\"error\":\"bad\\\\news\"}"
        );
    }

    #[test]
    fn agent_runtime_applies_source_limit() {
        let runtime = AgentRuntime::new(ExecutionContext {
            limits: RuntimeLimits {
                max_source_bytes: 3,
                max_output_lines: 1,
                max_execution_steps: 100,
            },
            ..ExecutionContext::default()
        });

        let result = runtime.run("print(1);");
        assert!(!result.ok);
        assert!(result.error.unwrap().contains("limit"));
    }

    #[test]
    fn agent_runtime_applies_execution_step_limit() {
        let runtime = AgentRuntime::new(ExecutionContext {
            limits: RuntimeLimits {
                max_source_bytes: 64 * 1024,
                max_output_lines: 256,
                max_execution_steps: 10,
            },
            ..ExecutionContext::default()
        });

        let result = runtime.run("while (true) {}");

        assert!(!result.ok);
        assert!(result.output.is_empty());
        assert!(
            result
                .error
                .unwrap()
                .contains("execution step limit exceeded")
        );
    }
}
