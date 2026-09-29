# CONVENTIONS — Conventions & Naming

> Stable layer, fifth in fixed read order. Edit rarely.

- Code conventions: stdlib-only Python for tooling (no third-party deps);
  single-file modules; no new `src/` helpers for builtin work — use the
  existing `define_non_enumerable` + `call_native` pattern in
  `src/interpreter.rs`.
- Naming rules: roadmap ids are rolling hierarchical (`01`, `05.1`); result
  archives go to `.w1mer/results/`, roadmap rows carry a one-line summary only.
