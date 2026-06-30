# rjs_lite: Lightweight Agent JavaScript Execution Runtime

`rjs_lite` is a lightweight script execution runtime for AI agent workflows. Its first execution language is a Rust-native JavaScript subset designed for short-lived, high-frequency local tool calls.

The project is not a wrapper around QuickJS, Boa, V8, Node.js, or Deno. Current code implements its own hand-written lexer, parser, AST, runtime value model, lexical environment, tree-walking interpreter, and agent-facing result API.

## Current Status

This repository is a runnable MVP engine scaffold, not a complete ECMAScript implementation. It is designed as a native foundation that can be expanded toward test262 subset coverage and benchmark work.

Currently supported:

- Number, string, boolean, `null`, and `undefined` literals
- `let`, `var`, and `const` bindings, including uninitialized `let` declarations
- Assignment to mutable bindings, member assignment, compound assignment, and update operators
- Arithmetic, comparison, equality, logical, unary, `typeof`, `instanceof`, and conditional operators
- Blocks, `if`, `else`, `while`, `for`, `switch`, `break`, and `continue`
- Function declarations, function expressions, calls, closures, `this`, `new`, and `return`
- Object and array literals, member/index access, assignment, array `length`, and prototype lookup
- `this` binding, `new`, constructor prototypes, and `instanceof`
- `throw`, `try`, `catch`, and `finally`
- Minimal host builtin: `print(value)`
- Small standard surface: `Object`, `Object.create`, `Object.defineProperty`, `Object.getOwnPropertyDescriptor`, `Object.getPrototypeOf`, `Object.keys`, `Object.values`, `Array`, `Array.isArray`, `String`, `Number`, `Boolean`, `Error` constructors, `isNaN`, placeholder `JSON.stringify`, `Object.prototype.toString`, `Object.prototype.hasOwnProperty`, `Object.prototype.propertyIsEnumerable`, `Array.prototype.map`, `Array.prototype.filter`, `Array.prototype.forEach`, `Array.prototype.join`, and `Array.prototype.push`
- Interactive REPL with state preservation, multi-line input, and commands
- CLI execution from inline source or a `.js` file
- Agent tool mode with structured JSON result output, stable `error_kind` values, source-size limit, output-line limit, execution-step limit, and call-depth limit
- Unit tests for lexer, parser, interpreter, Agent mode, CLI, and REPL behavior
- A local `run_test262.py` runner scaffold for use with an external test262 checkout

Not yet supported:

- Full object, array, property descriptor metadata, prototype, and standard-library semantics
- Classes, modules, async, generators, promises, regexps, symbols, BigInt
- Full ECMAScript type coercion rules
- Full strict-mode behavior
- DOM, browser APIs, npm packages, file APIs, network APIs, process APIs, and OS APIs
- Bundled test262 checkout or published test262 pass-rate tracking
- JIT, bytecode VM, or advanced optimization pipeline

## Usage

Start interactive REPL:

```bash
cargo run
```

Or explicitly:

```bash
cargo run -- --repl
```

In the REPL, type JavaScript line by line. Variables persist across lines. Multi-line input is supported: the prompt changes to `...` while braces or parentheses are unbalanced. Use `.exit` or `.quit` to leave, or press Ctrl+D.

```text
rjs_lite REPL. Type .exit or .quit to exit. Type .help for commands.
>>> function add(a, b) {
...   return a + b;
... }
>>> add(3, 4);
7
>>> .exit
```

REPL commands:

| Command | Description |
|---------|-------------|
| `.exit`, `.quit` | Exit the REPL |
| `.help` | Show available commands |
| `.clear` | Clear the screen |
| `.reset` | Reset interpreter state (clear all variables) |
| `.vars` | List defined variables with their declaration kind |

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
{"ok":true,"request_id":"local","value":"3","value_type":"number","output":["3"],"output_truncated":false,"error_kind":null,"error":null}
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

See `docs/language-subset.md` for the supported JavaScript subset.

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

## Upload Notes

The repository is ready for Gitee upload after verification. Keep `Cargo.lock` committed because this is a binary application. Do not upload `target/`, IDE metadata, logs, or local temp files.
