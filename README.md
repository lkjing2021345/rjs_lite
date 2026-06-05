# rjs_lite

`rjs_lite` is a lightweight JavaScript execution engine written in Rust for the 2026 operating-system contest topic around short-lived, high-frequency JavaScript execution in AI-agent scenarios.

The project is not a wrapper around QuickJS, Boa, V8, Node.js, or Deno. Current code implements its own hand-written lexer, parser, AST, runtime value model, lexical environment, and tree-walking interpreter.

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
- `src/main.rs`: std-only CLI

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
