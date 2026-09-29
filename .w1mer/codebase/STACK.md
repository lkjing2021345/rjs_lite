# STACK — Toolchain & Environment

> Stable layer, first in fixed read order. Edit rarely.

- Toolchain: Rust (stable, Windows MSVC), Python 3 stdlib-only for tooling
- Build: `cargo build --release` → `target/release/rjs_lite.exe`
- Test runner: `cargo test` (unit tests in `src/interpreter.rs`) + `run_test262.py`
  (test262 subset runner, stdlib-only; writes `test262_report.json` +
  `test262_result.json`; supports `--category`/`--skip-category`/`--limit`)
- Benchmark tooling: _(none yet; see ROADMAP 05)_
