# Infrastructure Results (INFRA)

> Measured outcomes for infrastructure tasks (runner, tooling, CI).

## Record fields

- Roadmap id + domain id (e.g. `04`, `INFRA_04`)
- commit hash + date
- measured data (counts, pass rates, timings)
- conclusion: `significant` `mild` `none` `regression`

## 04 — test262 subset runner with pass-rate tracking

- **Roadmap id**: 04
- **Date**: 2026-09-29
- **Commit**: _pending — implementer session had no shell tool; commit deferred
  to orchestrator. Files changed: `run_test262.py` (rewritten), `.gitignore`
  (+ `test262_report.json`), `.w1mer/detail/04.md` (new), `.w1mer/ROADMAP.md`,
  `.w1mer/codebase/STACK.md`, `.w1mer/codebase/CONVENTIONS.md`._

### What was implemented

`run_test262.py` rewritten (stdlib-only, no new deps):

1. **CLI** (argparse): `-d DIR` (test262 root), `-b BIN` (engine binary),
   `--category` / `--skip-category` (top-level subset filter),
   `--list-categories`, `--limit N` (smoke runs), `--workers N`,
   `--timeout S`, `--out FILE`, `--quiet`.
2. **Failure categorization** — 8 categories: `timeout`, `parse-error`,
   `runtime-error`, `negative-phase-mismatch`, `negative-type-mismatch`,
   `negative-but-passed`, `async-not-complete`, `read-error`.
3. **Pass-rate tracking** — `test262_report.json` (gitignored) records
   date, git commit, total/pass/fail/skip, `pass_rate` (pass/(pass+fail)) and
   `pass_rate_total` (pass/total), per-category failure counts, sample paths
   per category (≤10), per-top-level-category pass/fail breakdown, elapsed,
   workers, active filters. `test262_result.json` kept for backward compat.
4. **Graceful errors** — missing test262 dir / missing binary → clear stderr
   message + exit 1 (previously: unhandled `FileNotFoundError` at import).

### Verification

- **Static**: full re-read of `run_test262.py` (377 lines); all names defined
  before use; `run_one` returns 3-tuple consistently; `cat_counts`/`cat_samples`
  keyed by `FAIL_CATEGORIES` (all 8 categories always present); `cat_stats`
  keyed by top-level dir; `work()` closure captures `args`/`test_dir`/locks
  correctly; thread-safe accumulation under `lock`.
- **Dynamic**: NOT RUN. No shell tool is available in the implementer session
  (only playwright MCP, which cannot execute local processes). `cargo build
  --release`, `python -m py_compile run_test262.py`, and a smoke run
  (`python run_test262.py --limit 20`) are deferred to the orchestrator.

### Conclusion

`mild` (infrastructure) — runner capability complete; measured pass-rate
numbers require a test262 checkout + release build, not present in this
workspace.

### Risks

- No test262 checkout in repo (gitignored; upstream clone required) — full
  run numbers can only be measured where the checkout exists.
- Negative-test `type` verification remains a documented limitation (phase
  only).
- Engine binary not built in this workspace (`target/release/` absent) — smoke
  run needs `cargo build --release` first.
