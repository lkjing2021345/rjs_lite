---
name: rjs-lite-js-execution
description: Use when an agent needs to execute small JavaScript snippets through the project-local rjs_lite engine or integrate rjs_lite as a local AI Agent JavaScript execution tool.
---

# rjs_lite JS Execution

Use `rjs_lite` as a local, Rust-native JavaScript execution tool for AI Agent workflows.

## When To Use

Use this skill when an agent needs to:

- execute a small JavaScript snippet using the supported MVP subset
- call the project runtime through CLI and read structured JSON
- integrate `rjs_lite::run_agent_tool(source)` from Rust
- explain how this project acts as an AI Agent JS execution tool

## CLI Tool Contract

Run:

```bash
cargo run -- --agent-eval "let x = 1 + 2; print(x); x;"
```

Read one JSON object from stdout:

```json
{"ok":true,"request_id":"local","value":"3","value_type":"number","output":["3"],"output_truncated":false,"error":null}
```

Agent mode encodes JS parse/runtime failure as `ok:false` JSON. Do not treat nonzero exit as the only failure signal for `--agent-eval`.

## Result Fields

- `ok`: JavaScript execution success flag.
- `request_id`: execution context identifier.
- `value`: final value rendered as string, or `null`.
- `value_type`: interpreter type name, or `null`.
- `output`: captured `print(value)` lines.
- `output_truncated`: output limit indicator.
- `error`: diagnostic text on failure, or `null`.

## Rust API

```rust
let result = rjs_lite::run_agent_tool("let x = 1 + 2; x;");
println!("{}", result.to_json());
```

Use `AgentRuntime::new(ExecutionContext { ... })` for request metadata and basic source/output limits.

## Supported MVP Subset

Current engine supports literals, `let`/`const`, assignment, arithmetic, comparison, logical operators, blocks, `if`/`else`, `while`, function declarations/calls, `return`, and `print(value)`.

## Must Not Claim

- No full ECMAScript compatibility claim.
- No test262 pass-rate claim yet.
- No sandbox/security guarantee.
- No MCP server or network service.
- No Node.js, Deno, QuickJS, Boa, V8, or browser-engine wrapper.

## Good Agent Use Cases

- short deterministic calculations
- small control-flow snippets
- local glue-code checks
- structured output capture through `print`

Avoid untrusted code isolation, npm/browser APIs, filesystem/network access, large scripts, or compatibility claims beyond the documented subset.
