use std::io::Write;
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
    assert!(stdout.contains("\"error_kind\":\"parse\""));
    assert!(stdout.contains("parse error"));
}

#[test]
fn agent_eval_emits_typeof_result() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["--agent-eval", "typeof 42;"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("\"ok\":true"));
    assert!(stdout.contains("\"value\":\"number\""));
    assert!(stdout.contains("\"value_type\":\"string\""));
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
fn agent_eval_marks_output_truncated() {
    let source = (0..257)
        .map(|i| format!("print({i});"))
        .collect::<Vec<_>>()
        .join(" ");
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["--agent-eval", &source])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("\"ok\":true"));
    assert!(stdout.contains("\"output_truncated\":true"));
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
    assert!(stdout.contains("\"error_kind\":\"step_limit\""));
    assert!(stdout.contains("execution step limit exceeded"));
}

#[test]
fn agent_eval_wraps_call_depth_error_in_result() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["--agent-eval", "function loop() { return loop(); } loop();"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("\"ok\":false"));
    assert!(stdout.contains("\"error_kind\":\"call_depth_limit\""));
    assert!(stdout.contains("call depth limit exceeded"));
}

#[test]
fn repl_runs_and_exits() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let stdin = child.stdin.as_mut().unwrap();
    writeln!(stdin, ".exit").unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("rjs_lite REPL"),
        "should show banner: {stdout}"
    );
}

#[test]
fn repl_preserves_state_across_lines() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let stdin = child.stdin.as_mut().unwrap();
    writeln!(stdin, "let x = 1;").unwrap();
    writeln!(stdin, "let y = 2;").unwrap();
    writeln!(stdin, "x + y;").unwrap();
    writeln!(stdin, ".exit").unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains('3'), "should contain 3, got: {stdout}");
}

#[test]
fn repl_errors_do_not_exit() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let stdin = child.stdin.as_mut().unwrap();
    writeln!(stdin, "let = ;").unwrap();
    writeln!(stdin, "let x = 42;").unwrap();
    writeln!(stdin, "x;").unwrap();
    writeln!(stdin, ".exit").unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("parse error"),
        "should show parse error on stderr: {stderr}"
    );
    assert!(
        stdout.contains("42"),
        "should continue after error and print 42: {stdout}"
    );
}

#[test]
fn repl_quit_command_exits() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let stdin = child.stdin.as_mut().unwrap();
    writeln!(stdin, ".quit").unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
}

#[test]
fn repl_flag_starts_repl() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .arg("--repl")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let stdin = child.stdin.as_mut().unwrap();
    writeln!(stdin, ".exit").unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("rjs_lite REPL"),
        "should show banner: {stdout}"
    );
}

#[test]
fn repl_multiline_function() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let stdin = child.stdin.as_mut().unwrap();
    writeln!(stdin, "function add(a, b) {{").unwrap();
    writeln!(stdin, "  return a + b;").unwrap();
    writeln!(stdin, "}}").unwrap();
    writeln!(stdin, "add(3, 4);").unwrap();
    writeln!(stdin, ".exit").unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains('7'), "should contain 7, got: {stdout}");
}

#[test]
fn repl_multiline_if() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let stdin = child.stdin.as_mut().unwrap();
    writeln!(stdin, "let x = 10;").unwrap();
    writeln!(stdin, "if (x > 5) {{").unwrap();
    writeln!(stdin, "  1;").unwrap();
    writeln!(stdin, "}} else {{").unwrap();
    writeln!(stdin, "  2;").unwrap();
    writeln!(stdin, "}}").unwrap();
    writeln!(stdin, ".exit").unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains('1'), "should contain 1, got: {stdout}");
}

#[test]
fn repl_multiline_error_clears_buffer() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let stdin = child.stdin.as_mut().unwrap();
    writeln!(stdin, "function bad() {{").unwrap();
    writeln!(stdin, "  let = ;").unwrap();
    writeln!(stdin, "}}").unwrap();
    writeln!(stdin, "let ok = 1;").unwrap();
    writeln!(stdin, "ok;").unwrap();
    writeln!(stdin, ".exit").unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("parse error"),
        "should show parse error on stderr: {stderr}"
    );
    assert!(
        stdout.contains('1'),
        "should continue and print 1: {stdout}"
    );
}

#[test]
fn repl_dot_vars_lists_variables() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let stdin = child.stdin.as_mut().unwrap();
    writeln!(stdin, "let x = 1;").unwrap();
    writeln!(stdin, "const y = 2;").unwrap();
    writeln!(stdin, ".vars").unwrap();
    writeln!(stdin, ".exit").unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("let x"), "should show let x: {stdout}");
    assert!(stdout.contains("const y"), "should show const y: {stdout}");
}

#[test]
fn repl_dot_reset_clears_state() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let stdin = child.stdin.as_mut().unwrap();
    writeln!(stdin, "let x = 42;").unwrap();
    writeln!(stdin, ".reset").unwrap();
    writeln!(stdin, "x;").unwrap();
    writeln!(stdin, ".exit").unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("undefined variable"),
        "should show undefined variable error: {stderr}"
    );
}

#[test]
fn repl_dot_help_shows_commands() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let stdin = child.stdin.as_mut().unwrap();
    writeln!(stdin, ".help").unwrap();
    writeln!(stdin, ".exit").unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("REPL commands"),
        "should show commands: {stdout}"
    );
    assert!(stdout.contains(".exit"), "should mention .exit: {stdout}");
    assert!(stdout.contains(".vars"), "should mention .vars: {stdout}");
    assert!(stdout.contains(".reset"), "should mention .reset: {stdout}");
}
