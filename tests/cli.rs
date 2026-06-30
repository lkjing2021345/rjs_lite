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
fn agent_eval_emits_object_keys_result() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["--agent-eval", "let o={b:2,a:1}; Object.keys(o).join(',');"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("\"ok\":true"));
    assert!(stdout.contains("\"value\":\"a,b\""));
    assert!(stdout.contains("\"value_type\":\"string\""));
}

#[test]
fn agent_eval_emits_array_push_result() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["--agent-eval", "let a=[1,2]; a.push(3); a.join(',');"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("\"ok\":true"));
    assert!(stdout.contains("\"value\":\"1,2,3\""));
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
fn agent_eval_emits_array_filter_result() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args([
            "--agent-eval",
            "let a=[1,2,3,4]; a.filter(function(x){return x>2;}).join(',');",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("\"ok\":true"));
    assert!(stdout.contains("\"value\":\"3,4\""));
    assert!(stdout.contains("\"value_type\":\"string\""));
}

// --- Array.prototype method tests ---

#[test]
fn array_splice_removes_and_inserts() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "let a=[1,2,3,4]; a.splice(1,2,9,10); a.join(',') + ':' + [1,2,3,4].splice(1,2,9,10).join(',');"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "1,9,10,4:2,3");
}

#[test]
fn array_unshift_prepends_items() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "let a=[3,4]; a.unshift(1,2); a.join(',');"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "1,2,3,4");
}

#[test]
fn array_sort_default_string_order() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "let a=[3,1,2]; a.sort(); a.join(',');"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "1,2,3");
}

#[test]
fn array_sort_with_comparator() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "let a=[3,1,2]; a.sort(function(x,y){return x-y;}); a.join(',');"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "1,2,3");
}

#[test]
fn array_reverse_reverses_in_place() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "let a=[1,2,3]; a.reverse(); a.join(',');"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "3,2,1");
}

#[test]
fn array_concat_joins_arrays() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "[1,2].concat([3,4]).join(',');"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "1,2,3,4");
}

#[test]
fn array_reduce_sums_values() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "[1,2,3,4].reduce(function(a,b){return a+b;},0);"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "10");
}

#[test]
fn array_reduce_without_initial() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "[1,2,3].reduce(function(a,b){return a+b;});"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "6");
}

#[test]
fn array_reduce_right_subtracts_right_to_left() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "[1,2,3].reduceRight(function(a,b){return a-b;});"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "0");
}

#[test]
fn array_some_returns_true_when_any_matches() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "[1,2,3].some(function(x){return x>2;});"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "true");
}

#[test]
fn array_some_returns_false_when_none_matches() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "[1,2,3].some(function(x){return x>5;});"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "false");
}

#[test]
fn array_every_returns_true_when_all_match() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "[1,2,3].every(function(x){return x>0;});"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "true");
}

#[test]
fn array_every_returns_false_when_any_fails() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "[1,2,3].every(function(x){return x>1;});"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "false");
}

#[test]
fn array_find_returns_first_matching_value() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "[1,2,3,4].find(function(x){return x>2;});"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "3");
}

#[test]
fn array_find_returns_undefined_when_no_match() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "[1,2,3].find(function(x){return x>5;});"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
}

#[test]
fn array_find_index_returns_index_of_first_match() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "[1,2,3,4].findIndex(function(x){return x>2;});"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "2");
}

#[test]
fn array_fill_fills_range() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "let a=[1,2,3,4]; a.fill(0,1,3); a.join(',');"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "1,0,0,4");
}

#[test]
fn array_flat_flattens_nested_arrays() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "[1,[2,3],[4,[5,6]]].flat().join(',');"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "1,2,3,4,5,6");
}

#[test]
fn array_last_index_of_finds_last_index() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "[1,2,3,2,1].lastIndexOf(2);"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "3");
}

#[test]
fn array_from_creates_array_from_array_like() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "Array.from([1,2,3]).join(',');"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "1,2,3");
}

// --- end Array method tests ---

// --- String.prototype method tests ---

#[test]
fn string_slice_returns_substring() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "'hello world'.slice(1,5);"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "ello");
}

#[test]
fn string_substring_returns_substring() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "'hello'.substring(1,4);"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "ell");
}

#[test]
fn string_index_of_finds_position() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "'hello'.indexOf('l');"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "2");
}

#[test]
fn string_index_of_not_found_returns_minus_one() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "'hello'.indexOf('z');"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "-1");
}

#[test]
fn string_char_at_returns_character() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "'hello'.charAt(1);"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "e");
}

#[test]
fn string_trim_removes_whitespace() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "'  hello  '.trim();"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
}

#[test]
fn string_to_upper_case_works() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "'hello'.toUpperCase();"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "HELLO");
}

#[test]
fn string_to_lower_case_works() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "'HELLO'.toLowerCase();"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
}

#[test]
fn string_concat_joins_strings() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "'hello'.concat(' ', 'world');"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello world");
}

#[test]
fn string_replace_first_occurrence() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "'hello world'.replace('hello', 'hi');"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hi world");
}

#[test]
fn string_split_returns_array() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "'a,b,c'.split(',').join('-');"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "a-b-c");
}

#[test]
fn string_starts_with_checks_prefix() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "'hello'.startsWith('he');"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "true");
}

#[test]
fn string_ends_with_checks_suffix() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "'hello'.endsWith('lo');"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "true");
}

#[test]
fn string_includes_checks_substring() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "'hello'.includes('ell');"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "true");
}

#[test]
fn string_repeat_repeats() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "'ab'.repeat(3);"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "ababab");
}

#[test]
fn string_pad_start_pads_left() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "'5'.padStart(3, '0');"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "005");
}

#[test]
fn string_pad_end_pads_right() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "'5'.padEnd(3, '0');"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "500");
}

#[test]
fn string_length_property_works() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "'hello'.length;"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "5");
}

// --- end String method tests ---

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
        stderr.contains("is not defined"),
        "should show reference error: {stderr}"
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
