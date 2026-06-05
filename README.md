# rjs_lite: Lightweight Agent JavaScript Execution Runtime

`rjs_lite` is a lightweight script execution runtime for AI Agent workflows. Its first execution language is a Rust-native JavaScript subset designed for short-lived, high-frequency tool calls.

The project is not a wrapper around QuickJS, Boa, V8, Node.js, or Deno. Current code implements its own hand-written lexer, parser, AST, runtime value model, lexical environment, tree-walking interpreter, and agent-facing tool result API.

## Current Status

This repository is an MVP engine scaffold, not a complete ECMAScript implementation yet. It is designed as a native foundation that can be expanded toward test262 coverage and benchmark work.

Currently supported:

- Number, string, boolean, `null`, and `undefined` literals
- `let` and `const` bindings
- Assignment to mutable bindings
- Arithmetic, comparison, equality, logical, and unary operators
- Blocks, `if`, `else`, and `while`
- Function declarations, calls, and `return`
- Minimal host builtin: `print(value)`
- CLI execution from inline source or a `.js` file
- Agent tool mode with structured JSON result output
- Unit tests for lexer, parser, and interpreter smoke behavior

Not yet supported:

- Object and array semantics
- Prototype chain and standard library objects
- Classes, modules, async, generators, promises, regexps, symbols, BigInt
- Full ECMAScript type coercion rules
- test262 harness integration
- JIT, bytecode VM, or advanced optimization pipeline

## Usage

Run inline JavaScript:

```bash
cargo run -- -e "let x = 1 + 2 * 3; print(x);"
```

Run a file:

```bash
cargo run -- examples/demo.js
```

Show help:

```bash
cargo run -- --help
```

Use as an AI Agent tool:

```bash
cargo run -- --agent-eval "let x = 1 + 2; print(x); x;"
```

Example agent result:

```json
{"ok":true,"request_id":"local","value":"3","value_type":"number","output":["3"],"output_truncated":false,"error":null}
```

Example program:

```javascript
function fib(n) {
  if (n < 2) {
    return n;
  }
  return fib(n - 1) + fib(n - 2);
}

print(fib(8));
```

## Architecture

Execution pipeline:

```text
source text -> lexer -> tokens -> parser -> AST -> interpreter -> Value
```

Core modules:

- `src/lexer.rs`: hand-written tokenizer with spans and diagnostics
- `src/parser.rs`: recursive-descent parser with expression precedence
- `src/ast.rs`: program, statement, and expression data model
- `src/value.rs`: runtime values and display behavior
- `src/interpreter.rs`: lexical scopes, control flow, functions, and builtin calls
- `src/agent.rs`: agent runtime context, limits, host-function metadata, and structured tool results
- `src/main.rs`: std-only CLI

## Agent Tool Mode

AI Agents can call `rjs_lite` as a local JavaScript execution tool through `--agent-eval`. The process prints one JSON object to stdout, making success, final value, captured `print` output, and errors easy to parse.

Agent mode is intentionally local and dependency-free. It is not an MCP server, network service, or sandbox guarantee. JavaScript parse/runtime failures are represented as `ok:false` JSON results so agents can handle them as ordinary tool output.

Rust callers can use:

```rust
let result = rjs_lite::run_agent_tool("let x = 1 + 2; x;");
println!("{}", result.to_json());
```

See `docs/agent-tool.md` for the full tool contract.

## Contest Alignment

Function completeness path:

1. Grow language surface from expressions/functions into objects, arrays, and standard builtins.
2. Add a test262 runner that can filter supported features, record pass rate, and track regressions.
3. Prioritize high-frequency syntax and APIs used by agent-generated scripts.

Performance benchmark path:

1. Add parser/interpreter microbenchmarks for arithmetic, loops, function calls, and object access.
2. Introduce bytecode compilation after AST interpreter correctness stabilizes.
3. Compare against small JavaScript workloads inspired by JetStream-style hot paths.

Innovation path:

1. Keep engine native and dependency-light.
2. Optimize for short-lived execution: fast startup, low memory, predictable teardown.
3. Explore agent-oriented host APIs and sandbox controls instead of copying browser-engine assumptions.

## Development

```bash
cargo fmt
cargo check
cargo test
```

Optional, if Clippy is installed:

```bash
cargo clippy -- -D warnings
```

## Upload Notes

The repository is ready for Gitee upload after verification. Keep `Cargo.lock` committed because this is a binary application. Do not upload `target/`, IDE metadata, logs, or local temp files.
