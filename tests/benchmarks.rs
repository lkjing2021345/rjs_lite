//! Microbenchmarks for the rjs_lite interpreter.
//!
//! Each benchmark function measures a single interpreter workload using
//! `std::time::Instant` (no external dependencies). The four categories
//! track the roadmap perf goal:
//!
//! - **arithmetic**   — repeated numeric operator evaluation
//! - **loops**        — a `for` loop running a fixed iteration count
//! - **function calls** — repeated direct + recursive calls
//! - **object access** — repeated property reads on a plain object
//!
//! Run with `cargo test --test benchmarks -- --nocapture`.

use std::time::Instant;

use rjs_lite::{run_source, Value};

const ITERS: usize = 200_000;

/// Run `source` and assert the final value is the expected number.
/// Returns the elapsed duration.
fn bench(source: &str, expected: f64) -> std::time::Duration {
    let start = Instant::now();
    let value = run_source(source).unwrap_or_else(|e| panic!("benchmark source failed: {e}"));
    let elapsed = start.elapsed();
    assert_eq!(
        value,
        Value::Number(expected),
        "benchmark produced wrong result"
    );
    elapsed
}

/// Arithmetic: 200_000 iterations of `a + b * c - d` with mixed operators.
fn bench_arithmetic() -> std::time::Duration {
    let source = format!(
        "let a = 1; let b = 2; let c = 3; let d = 4;\
         let r = 0;\
         for (let i = 0; i < {ITERS}; i = i + 1) {{\
           r = r + (a + b * c - d) + i;\
         }}\
         r;"
    );
    bench(&source, 200_000.0_f64 * (1.0 + 2.0 * 3.0 - 4.0) + (ITERS as f64 - 1.0) * ITERS as f64 / 2.0)
}

/// Loops: a `for` loop with a simple body (increment + branch).
fn bench_loops() -> std::time::Duration {
    let source = format!(
        "let x = 0;\
         for (let i = 0; i < {ITERS}; i = i + 1) {{\
           if (i % 2 == 0) {{ x = x + 1; }}\
         }}\
         x;"
    );
    bench(&source, (ITERS / 2) as f64)
}

/// Function calls: 200_000 direct calls to a two-arg function,
/// plus 50_000 recursive calls (depth 1).
fn bench_function_calls() -> std::time::Duration {
    let source = format!(
        "function add(a, b) {{ return a + b; }}\
         function double(x) {{ return add(x, x); }}\
         let r = 0;\
         for (let i = 0; i < {ITERS}; i = i + 1) {{\
           r = r + add(i, 1);\
         }}\
         for (let i = 0; i < 50000; i = i + 1) {{\
           r = r + double(i);\
         }}\
         r;"
    );
    // add(i,1) = i+1, sum i=0..ITERS-1 of (i+1) = ITERS*(ITERS+1)/2
    // double(i) = 2i, sum i=0..49999 of 2i = 2 * 49999*50000/2 = 49999*50000
    let expected = (ITERS as f64) * (ITERS as f64 + 1.0) / 2.0 + 49999.0 * 50000.0;
    bench(&source, expected)
}

/// Object access: 200_000 reads of `.x` on a plain object with 3 properties.
fn bench_object_access() -> std::time::Duration {
    let source = format!(
        "let o = {{ x: 1, y: 2, z: 3 }};\
         let r = 0;\
         for (let i = 0; i < {ITERS}; i = i + 1) {{\
           r = r + o.x + o.y + o.z;\
         }}\
         r;"
    );
    bench(&source, 6.0 * ITERS as f64)
}

#[test]
fn microbenchmarks() {
    let art = bench_arithmetic();
    let loops = bench_loops();
    let calls = bench_function_calls();
    let obj = bench_object_access();

    println!("=== rjs_lite microbenchmarks ===");
    println!("arithmetic:      {art:?}");
    println!("loops:           {loops:?}");
    println!("function calls:  {calls:?}");
    println!("object access:   {obj:?}");
    println!("===============================");
}
