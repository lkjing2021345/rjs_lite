# Supported JavaScript Subset

This document describes the JavaScript subset currently targeted by `rjs_lite`.
The project is a lightweight runtime for short-lived AI agent scripts, not a full
ECMAScript engine.

## Supported Syntax

- Literals: number, string, boolean, `null`, and `undefined`
- Bindings: `let`, uninitialized `let`, `var`, and `const`
- Expressions: arithmetic, comparison, equality, logical, unary, conditional,
  `typeof`, and `instanceof`
- Assignment: mutable bindings, member assignment, index assignment, compound
  assignment, prefix update, and postfix update
- Control flow: blocks, `if`, `else`, `while`, `for`, `switch`, `break`, and
  `continue`
- Functions: declarations, expressions, calls, closures, `this`, `new`, and
  `return`
- Exceptions: `throw`, `try`, `catch`, and `finally`
- Data structures: object literals, array literals, member access, index access,
  array `length`, and prototype lookup

## Supported Runtime Surface

- Minimal host function: `print(value)`
- Basic constructors: `Object`, `Array`, `String`, `Number`, `Boolean`, and
  common error constructors
- Basic utilities: `isNaN`
- Object utilities: `Object.keys` and `Object.values`
- Placeholder JSON support: `JSON.stringify`
- Small prototype surface: `Object.prototype.toString`,
  `Array.prototype.join`, `Array.prototype.map`, `Array.prototype.filter`,
  `Array.prototype.forEach`, and `Array.prototype.push`
- `Array.prototype.map`, `Array.prototype.filter`, and
  `Array.prototype.forEach` callbacks receive the current value, zero-based
  index, and source array
- Interactive REPL with state preservation, multi-line input, and dot commands

## Agent Tool Contract

Agent mode returns one JSON object containing:

- `ok`
- `request_id`
- `value`
- `value_type`
- `output`
- `output_truncated`
- `error_kind`
- `error`

JavaScript parse and runtime failures are encoded as ordinary JSON results with
`ok:false`, so agents can handle them without interpreting process failures.

Current `error_kind` values are:

- `lex`
- `parse`
- `runtime`
- `source_limit`
- `step_limit`
- `call_depth_limit`

Agent mode applies these default runtime limits:

- Source text: 64 KiB
- Captured output: 256 lines
- Interpreter steps: 100,000
- Nested user-defined function calls: 16 frames

## Partial Semantics

These features exist but are intentionally incomplete:

- Object, array, constructor, and prototype semantics
- JavaScript coercion rules
- Standard library behavior
- Error object behavior and typed exception matching
- `JSON.stringify`, which currently behaves as a minimal placeholder
- REPL line editing and history

## Not Supported

- Classes
- Modules
- Async functions and promises
- Generators
- Regular expressions
- Symbols
- BigInt
- Full strict-mode behavior
- Browser APIs, DOM APIs, npm packages, file APIs, network APIs, process APIs,
  and OS APIs
- JIT, bytecode VM, or advanced optimization pipeline

## Test262 Direction

`run_test262.py` is an optional local exploration runner. It expects an upstream
`test262/` checkout in the repository root and should be used for subset
tracking, not for claiming full ECMAScript compatibility.

Future test262 work should use a maintained subset list, record pass/fail
categories, and clearly distinguish supported-syntax failures from unsupported
feature failures.
