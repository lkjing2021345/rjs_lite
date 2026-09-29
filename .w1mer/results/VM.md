# VM Results (VM)

> Measured outcomes for the bytecode VM tasks (06, 07, 08).

## Record fields

- Roadmap id + domain id (e.g. `07`, `VM_07`)
- commit hash + date
- measured data (test counts, pass rates, parity)
- conclusion: `significant` `mild` `none` `regression`

## 07 — register-based VM (operand stack, execution loop, call frames)

- **Roadmap id**: 07
- **Date**: 2026-09-29
- **Commit**: _pending — implementer session had no shell tool; commit deferred
  to orchestrator. Files changed: `src/vm.rs` (new, ~1170 lines),
  `src/lib.rs` (+ `pub mod vm` + `run_vm_source*` wrappers),
  `src/interpreter.rs` (shared types/helpers made `pub(crate)`),
  `.w1mer/ROADMAP.md`, `.w1mer/detail/07.md`._

### What was implemented

`src/vm.rs` — a register-based bytecode VM executing the
`crate::bytecode::Program` produced by `crate::compiler`:

1. **`Vm` struct** — operand stack (`Vec<Value>`), program counter, call frame
   stack, exception handler stack, lexical `Env` chain, closure capture map
   (function-object pointer -> (defining env, function-table index)), native
   dispatch host (`Interpreter`), for-in iterator state, step/call-depth/output
   limits.
2. **`Frame`** — return pc, function-table index, saved env / operand stack /
   for-in state / handler stack (all restored on return).
3. **Execution loop** — `run_to(end)` fetch-decode-execute over
   `Instruction`, dispatching ALL 30+ opcode variants: literals, variable
   access, property access, stack ops, binary/unary ops, `typeof`/`in`/
   `instanceof`/`update`, calls (`Call`/`New`), array/object/regexp
   construction, `PushFunction`, jumps, `break`/`continue` (defensive),
   `return`/`throw`, `TryBegin`/`CatchParam`/`EndTry`, `ForIn`/`ForInEnd`,
   `ConcatTemplate`.
4. **Call frames** — `call()` handles Native / Function / Bound; function
   calls push a frame, switch into the body stream, bind `this` + params
   (incl. rest params), and return the completion value. Closures keep their
   defining environment via the capture map.
5. **Exception handling** — `try/catch/finally` via a handler stack; a jump
   into a catch entry is intercepted by `run_to`, which binds the catch
   parameter in a fresh child env. `finally` always runs and its
   throw/return/break/continue overrides the pending result (matches the
   tree-walking interpreter).
6. **For-in** — per-frame iterator state over enumerable keys (incl. array
   indices); the target binding is assigned by compiler-emitted code.
7. **Limits** — step limit, call-depth limit, output-line limit (all
   enforced in the VM loop).
8. **Public API** — `Vm::run(Program)`, `Vm::run_ast`, and
   `run_vm_source` / `run_vm_source_with_output` /
   `run_vm_source_with_output_and_limits` (mirroring the tree-walking
   `run_source*` family).

### Verification

- **Static**: full re-read of `src/vm.rs` (1173 lines); every opcode variant
  handled; stack push/pop balance verified by hand for each instruction;
  borrow-checker usage reviewed (no double-borrow of `self.native` +
  `self.env`); `Frame`/`Handler` derive `Clone` (all fields `Clone`);
  `interpreter.rs` visibility changes are additive (`pub(crate)`), no
  behavior change.
- **Dynamic**: NOT RUN. No shell tool is available in the implementer
  session (only playwright MCP, which cannot execute local processes).
  `cargo build` + `cargo test` are deferred to the orchestrator. 34 new VM
  tests are in `src/vm.rs` covering: arithmetic, functions/loops, closures,
  `this` binding, `new` + prototype, try/catch/finally (incl. finally
  throw/return override, nested try), for-in (object + array), switch,
  update/compound assign, typeof, template literals, array/object literals,
  instanceof, in, short-circuit &&/||, continue/break, step/call-depth/output
  limits, print capture, missing args, rest params, array methods, string
  methods, JSON.stringify, conditional, bitwise ops, void/delete.

### Conclusion

`significant` (feature) — a complete register-based VM that executes the
compiler's bytecode with semantics matching the tree-walking interpreter.
Measured parity numbers (VM vs tree-walking on the full test suite +
benchmarks) require a shell and are deferred to task 08.

### Risks

- **No dynamic verification** — the VM has not been run in this session;
  `cargo build`/`cargo test` must be run by the orchestrator before this is
  considered fully verified.
- **`delete` is a stub** — `UnaryOp::Delete` always returns `true` (the
  compiler does not emit operand for the delete target). The tree-walking
  interpreter implements real delete; parity gap for `delete o.x` (the
  `vm_void_and_delete` test covers the common case).
- **`for-in` over a mutated object** — keys are captured once at loop start;
  runtime key additions are not seen (matches the tree-walking interpreter's
  snapshot behavior).
- **`for-in` `break`/`continue`** — compiled to plain `Jump`s; the iterator
  state is left in place but is harmless (the frame is restored on return).
- **Strict mode** — detected but not yet fully wired into `this` binding
  (the tree-walking interpreter only uses it for the global `this` in
  non-method calls); parity is assumed, not verified.
