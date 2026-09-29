# rjs_lite — Roadmap

> Lower number = higher priority. Priorities: performance > bug fixes > features.
> Each completed task: mark `[x]` + date + commit, add a one-line measured
> effect summary and `pending-review`; full data goes to the results archive.
> Insert new items with derived sub-ids (`05.1`, `05.1.2`); never batch-renumber.

## ID rules

- Unbounded rolling hierarchical IDs: `01`, `05`, `05.1`, `05.1.1`, ...
- Children branch deeper on demand (sub-batch fixes, review follow-ups).
- Ordering: pre-order traversal (parent first, children right behind).
- Cross-references stay stable; derived ids link review/results/bug docs.

## Status states

`todo` `doing` `done` `pending-review` `reviewed` `reviewed-issues` `fixed` `re-reviewed`

## 一、Performance

| # | Task | Doc | Status | Effect |
|---|------|-----|--------|--------|
| 05 | Add microbenchmarks (arithmetic, loops, function calls, object access) | 05.md | done | 4 benchmark functions (arithmetic, loops, function calls, object access); 2026-09-29 0ce9cab |
<!-- w1mer:task:perf -->

## 二、Bug fixes

| # | Task | Doc | Status | Effect |
|---|------|-----|--------|--------|
<!-- w1mer:task:bug -->

## 三、Features

| # | Task | Doc | Status | Effect |
|---|------|-----|--------|--------|
| 01 | Harden object/array/property-descriptor/prototype semantics | 01.md | done | Added Object.prototype.valueOf, toLocaleString, isPrototypeOf; 2026-09-29 |
| 02 | Expand standard library (remaining Array/Object/String methods) | 02.md | done | Added Array.prototype.at, String.prototype.at/codePointAt/replaceAll/localeCompare/search/match, Object.fromEntries/defineProperties, Number.isSafeInteger; 2026-09-29 |
| 03 | Harden exception semantics (try/catch/finally edge cases) | 03.md | done | Fixed 8 edge cases: parameterless catch, finally always runs, finally throw/return/break/continue override pending result, nested try; 15 new tests; 2026-09-29 |
<!-- w1mer:task:feature -->

## 四、Infrastructure

| # | Task | Doc | Status | Effect |
|---|------|-----|--------|--------|
| 04 | Set up test262 subset runner with pass-rate tracking | 04.md | done | Runner: CLI subset filtering, 8 failure categories, per-category breakdown, test262_report.json; 2026-09-29 da8f4a8 |
<!-- w1mer:task:infra -->

## 七、test262 Optimization

| # | Task | Doc | Status | Effect |
|---|------|-----|--------|--------|
| 09 | Baseline test262 pass rate + initial fixes | 09.md | done | language 26.3%→29.35% (2000 sample); added arguments object, eval(), destructuring, strict mode guards, trailing semicolon fix; 2026-09-29 |
| 10 | Improve test262 pass rate (parse errors, runtime errors) | 10.md | doing | 31.7% (2000 sample); added async function + await, BigInt literals, strict-mode arguments.callee TypeError, strict-mode fn-decl-in-statement-position SyntaxError, named function expressions, ASI edge cases, array elision, computed property names, object spread, globalThis, method definitions; 901 parse + 380 runtime + 83 negative-phase-mismatch remaining; 2026-09-29 17d9729 |
<!-- w1mer:task:test262 -->

(Items awaiting re-prioritization)
- class, module, async, generator, promise, symbol, BigInt
- Full ECMAScript type coercion rules
- JIT / advanced optimization pipeline
<!-- w1mer:task:backlog -->

## 六、Bytecode VM

| # | Task | Doc | Status | Effect |
|---|------|-----|--------|--------|
| 06 | Design opcode set + bytecode format (compile AST → bytecode) | 06.md | done | src/bytecode.rs + src/compiler.rs; 2026-09-29 03091da |
| 07 | Implement register-based VM (operand stack, execution loop, call frames) | 07.md | done | src/vm.rs: register-based VM (operand stack, pc, call frames, handler stack) executing all compiler opcodes; closures, this/construct, try/catch/finally, for-in, limits; 35 tests; 2026-09-29 b5e1b19 |
| 08 | Integrate VM: CLI flag --vm, run same test suite, parity check | 08.md | done | --vm flag routes -e/file through bytecode VM; 15 parity tests verify interpreter vs VM output match; 2026-09-29 129f4b6 |
<!-- w1mer:task:vm -->
