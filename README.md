# rjs_lite: Lightweight Agent JavaScript Execution Runtime

`rjs_lite` is a lightweight script execution runtime for AI agent workflows. Its first execution language is a Rust-native JavaScript subset designed for short-lived, high-frequency local tool calls.

The project is not a wrapper around QuickJS, Boa, V8, Node.js, or Deno. Current code implements its own hand-written lexer, parser, AST, runtime value model, lexical environment, tree-walking interpreter, and agent-facing result API.

## Current Status

This repository is a runnable MVP engine scaffold, not a complete ECMAScript implementation. It is designed as a native foundation that can be expanded toward test262 subset coverage and benchmark work.

Currently supported:

- Number, string, boolean, `null`, and `undefined` literals
- `let`, `var`, and `const` bindings, including multiple declarations
- Assignment to mutable bindings, member assignment, compound assignment, and update operators
- Arithmetic, comparison, equality, logical, unary, `typeof`, `instanceof`, and conditional operators
- Blocks, `if`, `else`, `while`, `for`, `switch`, and `break`
- Function declarations, function expressions, calls, closures, `this`, `new`, and `return`
- Basic object and array literals, member access, index access, array `length`, and prototype lookup
- Basic `throw`, `try`, `catch`, and `finally`
- Minimal builtins: `print(value)`, `Object`, `Object.values`, `Array`, `String`, `Number`, `Boolean`, `isNaN`, a placeholder `JSON.stringify`, and basic error constructors
- A small prototype surface, including `Object.prototype.toString`, `Array.prototype.join`, and `Array.prototype.map`
- CLI execution from inline source or a `.js` file
- Agent tool mode with structured JSON result output
- Unit and CLI tests for lexer, parser, interpreter, agent mode, and selected object/prototype behavior

Not yet supported:

- Complete object, array, property descriptor, and prototype semantics
- Complete ECMAScript standard library objects
- Classes, modules, async, generators, promises, regexps, symbols, BigInt
- Full ECMAScript type coercion rules
- Full strict-mode behavior
- DOM, browser APIs, npm packages, file APIs, network APIs, process APIs, and OS APIs
- Published test262 pass-rate tracking
- JIT, bytecode VM, or advanced optimization pipeline

## Usage

Run inline JavaScript:

```bash
cargo run -- -e "let x = 1 + 2 * 3; print(x);"
```

Run a JavaScript file:

```bash
cargo run -- path/to/file.js
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

AI agents can call `rjs_lite` as a local JavaScript execution tool through `--agent-eval`. The process prints one JSON object to stdout, making success, final value, captured `print` output, and errors easy to parse.

Agent mode is intentionally local and dependency-free. It is not an MCP server, a network service, or a security sandbox. JavaScript parse/runtime failures are represented as `ok:false` JSON results so agents can handle them as ordinary tool output.

Rust callers can use:

```rust
let result = rjs_lite::run_agent_tool("let x = 1 + 2; x;");
println!("{}", result.to_json());
```

See `docs/agent-tool.md` for the full tool contract.

## Contest Alignment

Function completeness path:

1. Harden object, array, constructor, prototype, and exception semantics against more edge cases.
2. Expand common builtins used by short agent-generated scripts.
3. Add a maintained test262 subset workflow that records pass rate and regression categories.
4. Prioritize high-frequency syntax and APIs used by agent-generated scripts.

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

Optional test262 exploration:

```bash
cargo build --release
python run_test262.py
```

The test262 runner expects an upstream `test262/` checkout in the repository root. It is intended for local compatibility exploration and result snapshots, not as a published pass-rate claim.

## Upload Notes

The repository is ready for Gitee upload after verification. Keep `Cargo.lock` committed because this is a binary application. Do not upload `target/`, IDE metadata, logs, or local temp files.
