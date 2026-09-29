---
id: CHANGES
title: Recent changes (architecture-impact deltas)
state: active
---

# Recent Changes

> Architecture-impact deltas recorded by reviewers during main batches.
> A periodic compact merges these into the stable `codebase/` docs and clears
> this log. One cache invalidation per compact — keep stable periods long.

## Deltas

> One bullet per delta, tagged with the target stable doc:
> `- [ARCHITECTURE] contract X moved`. Tags: STACK / STRUCTURE /
> ARCHITECTURE / INTEGRATIONS / CONVENTIONS. Untagged → unassigned.
> `w1mer sync` shows a grouped preview; `w1mer sync --apply` writes + clears.

- [ARCHITECTURE] Task 01 (commit 34b806a): `non_enumerable_props` now dual-purpose — stores non-enumerable keys AND `__writable_<key>`/`__configurable_<key>` marker strings for descriptor flag reporting. `set_property` can now shrink `non_enumerable_props` when shadowing a prototype property. `Object.assign` walks source prototype chains (copies inherited enumerable props — deliberate spec deviation). Three new `Object.prototype` methods registered: `valueOf`, `toLocaleString`, `isPrototypeOf`.
- [ARCHITECTURE] Task 02 (commit b451ede): 10 new native methods added via existing `define_non_enumerable` + `call_native` pattern — `Array.prototype.at`, `String.prototype.at`, `String.prototype.codePointAt`, `String.prototype.replaceAll`, `String.prototype.localeCompare`, `String.prototype.search`, `String.prototype.match`, `Object.fromEntries`, `Object.defineProperties`, `Number.isSafeInteger`. Change surface confined to `src/interpreter.rs` (`install_builtins` + `call_native`); no new helpers, no `value.rs` changes. New internal call path: `Object.defineProperties` calls `call_native("Object.defineProperty", ...)` per descriptor (native-calling-native). **Contract tension**: `match` and `search` read only `source` from RegExp args, while `test`/`exec` read both `source` and `flags` — the `i` flag is ignored by `match`/`search` (spec deviation, R_02 findings M1/M2).
- [ARCHITECTURE] Task 03 (commit 9b4b9cf): `Stmt::Try.catch_param` changed from `String` to `Option<String>` (parameterless `catch {}` support, ES2019). `finally` block now always runs and its non-`Value` flow (throw/return/break/continue) overrides the pending result per ES2024 §15.14.2. Change surface: `parser.rs` (`try_stmt`), `ast.rs` (`Stmt::Try`), `interpreter.rs` (`Stmt::Try` arm + 15 tests). No changes to `value.rs`, `env`, or other modules.
- [INTEGRATIONS] Task 04 (commit da8f4a8): New `run_test262.py` (377 lines, stdlib-only Python) bridges the engine binary (`target/release/rjs_lite.exe`) and the upstream test262 checkout (`./test262`). Depends on the engine's stderr error format (`SyntaxError:` prefix for parse/lex, `{type}:` for runtime — contract with `src/error.rs` Display impl) and the test262 harness files (`sta.js`, `assert.js`, `doneprintHandle.js`) + frontmatter format. 8 mutually-exclusive failure categories; per-category breakdown in `test262_report.json` (gitignored). `src/` untouched — purely additive. R_04: docstring stale on type verification (code does verify it), `read-error` category never populated (read errors classified as `skip` not `fail`).
