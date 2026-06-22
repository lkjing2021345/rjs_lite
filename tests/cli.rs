use std::process::Command;

#[test]
fn eval_executes_inline_source() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "let x = 1 + 2 * 3; print(x);"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "7");
}

#[test]
fn invalid_source_exits_nonzero() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "let = ;"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("parse error"));
}

#[test]
fn agent_eval_emits_structured_success() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["--agent-eval", "let x = 1 + 2; print(x); x;"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("\"ok\":true"));
    assert!(stdout.contains("\"value\":\"3\""));
    assert!(stdout.contains("\"value_type\":\"number\""));
    assert!(stdout.contains("\"output\":[\"3\"]"));
}

#[test]
fn agent_eval_wraps_parse_error_in_result() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["--agent-eval", "let = ;"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("\"ok\":false"));
    assert!(stdout.contains("parse error"));
}

#[test]
fn eval_executes_file_with_objects_arrays_and_prototypes() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .arg("examples/demo.js")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "21\n2,4,6\n7"
    );
}

#[test]
fn agent_eval_emits_structured_array_result() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args([
            "--agent-eval",
            "let values=[1,2,3]; values.map(function(x){return x+1;}).join(',');",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("\"ok\":true"));
    assert!(stdout.contains("\"value\":\"2,3,4\""));
    assert!(stdout.contains("\"value_type\":\"string\""));
}

#[test]
fn agent_eval_wraps_step_limit_error_in_result() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["--agent-eval", "while (true) {}"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("\"ok\":false"));
    assert!(stdout.contains("execution step limit exceeded"));
}

#[test]
fn agent_eval_wraps_call_depth_error_in_result() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["--agent-eval", "function loop(){ return loop(); } loop();"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("\"ok\":false"));
    assert!(stdout.contains("call depth limit exceeded"));
}
