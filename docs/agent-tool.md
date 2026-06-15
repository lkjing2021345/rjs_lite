# Agent Tool Mode

`rjs_lite` can be used as a local JavaScript execution tool for AI Agent workflows. The agent submits a small JavaScript snippet, the runtime executes the supported subset, and the process returns one structured JSON result.

This is a local CLI/library contract. It is not an MCP server, network service, or sandbox guarantee.

## CLI Contract

```bash
cargo run -- --agent-eval "let x = 1 + 2; print(x); x;"
```

Example output:

```json
{"ok":true,"request_id":"local","value":"3","value_type":"number","output":["3"],"output_truncated":false,"error":null}
```

Agent mode always exits successfully when the tool process itself runs. JavaScript parse/runtime failures are encoded in the JSON result with `ok:false`.

Agent mode applies the default runtime limits: source text is capped at 64 KiB, captured output is capped at 256 lines, execution is stopped after 100,000 interpreter steps, and nested user-defined function calls are stopped after 16 frames.

## Result Fields

- `ok`: whether JavaScript execution succeeded.
- `request_id`: execution context identifier. Default CLI value is `local`.
- `value`: final evaluated value rendered as a string, or `null` on failure.
- `value_type`: runtime type name such as `number`, `string`, `boolean`, `null`, `undefined`, or `function`.
- `output`: captured `print(value)` lines.
- `output_truncated`: whether output exceeded the configured line limit.
- `error`: diagnostic string on failure, or `null` on success.

## Rust API

```rust
let result = rjs_lite::run_agent_tool("let x = 1 + 2; x;");
println!("{}", result.to_json());
```

For custom execution metadata and limits:

```rust
let runtime = rjs_lite::AgentRuntime::new(rjs_lite::ExecutionContext {
    request_id: "agent-call-1".to_string(),
    limits: rjs_lite::RuntimeLimits {
        max_source_bytes: 64 * 1024,
        max_output_lines: 256,
        max_execution_steps: 100_000,
        max_call_depth: 16,
    },
    host_functions: vec![rjs_lite::HostFunction {
        name: "print".to_string(),
        arity: 1,
        description: "Capture one value into output".to_string(),
    }],
});

let result = runtime.run("print(42);");
```

`host_functions` currently documents the intended host surface. The MVP interpreter exposes `print(value)` only.

## Limitations

- Supports only the documented MVP JavaScript subset.
- Stops agent-mode execution after the configured interpreter step limit.
- Stops agent-mode execution after the configured call depth limit.
- Stops agent-mode execution after the configured interpreter step limit.
- Stops agent-mode execution after the configured call depth limit.
- Does not claim full ECMAScript compatibility.
- Does not claim test262 pass-rate coverage yet.
- Does not provide a security sandbox guarantee.
- Does not expose file, network, process, or OS APIs to JavaScript.
- Does not wrap Node.js, Deno, QuickJS, Boa, V8, or another JS engine.

## Agent Usage Guidance

Use this tool for short-lived snippets such as:

- arithmetic and control-flow checks
- small data transformation logic supported by current syntax
- glue-code experiments before host tool calls
- deterministic local calculations

Avoid using it for untrusted code isolation, browser APIs, network access, npm packages, large scripts, or full ECMAScript compatibility testing.
