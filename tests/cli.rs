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

// --- Function.prototype method tests ---

#[test]
fn function_call_calls_with_this_arg() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "function add(x){return this.base+x} add.call({base:10}, 5)"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "15");
}

#[test]
fn function_apply_calls_with_this_arg() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "function add(x,y){return this.base+x+y} add.apply({base:10}, [1,2])"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "13");
}

#[test]
fn function_bind_returns_bound_function() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "function add(x){return this.base+x} var b=add.bind({base:10}); b(5)"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "15");
}

#[test]
fn function_bind_prepends_args() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "function add(x,y){return this.base+x+y} var b=add.bind({base:10}, 1); b(2)"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "13");
}

// --- end Function method tests ---

// --- Math tests ---

#[test]
fn math_abs_returns_absolute() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "Math.abs(-5)"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "5");
}

#[test]
fn math_floor_rounds_down() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "Math.floor(3.7)"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "3");
}

#[test]
fn math_max_returns_largest() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "Math.max(1, 5, 3)"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "5");
}

#[test]
fn math_pow_raises_to_power() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "Math.pow(2, 3)"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "8");
}

#[test]
fn math_constants_are_defined() {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(["-e", "Math.PI > 3.14 && Math.E > 2.71"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "true");
}

// --- end Math tests ---

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

// --- VM parity tests: --vm flag should produce same output as interpreter ---

fn run_cli(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_rjs_lite"))
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "CLI failed: {args:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn run_both(source: &str) -> (String, String) {
    let interp = run_cli(&["-e", source]);
    let vm = run_cli(&["--vm", "-e", source]);
    (interp, vm)
}

#[test]
fn vm_parity_arithmetic() {
    let (i, v) = run_both("let x = 1 + 2 * 3; x;");
    assert_eq!(i, "7");
    assert_eq!(v, "7");
}

#[test]
fn vm_parity_functions_and_loops() {
    let src = "function add(a,b){return a+b;} let t=0; for(let i=0;i<4;i++){t=t+add(i,1);} t;";
    let (i, v) = run_both(src);
    assert_eq!(i, "10");
    assert_eq!(v, "10");
}

#[test]
fn vm_parity_closures() {
    let src = "function make(){let x=1;return function(){x=x+1;return x;};} let f=make();f()+f();";
    let (i, v) = run_both(src);
    assert_eq!(i, "5");
    assert_eq!(v, "5");
}

#[test]
fn vm_parity_string_ops() {
    let (i, v) = run_both("'hello'.toUpperCase();");
    assert_eq!(i, "HELLO");
    assert_eq!(v, "HELLO");
}

#[test]
fn vm_parity_object_literal() {
    let (i, v) = run_both("let o={a:1,b:2}; o.a + o.b;");
    assert_eq!(i, "3");
    assert_eq!(v, "3");
}

#[test]
fn vm_parity_array_literal() {
    let (i, v) = run_both("let a=[1,2,3]; a.length;");
    assert_eq!(i, "3");
    assert_eq!(v, "3");
}

#[test]
fn vm_parity_try_catch() {
    let src = "try { throw 1; } catch(e) { e; }";
    let (i, v) = run_both(src);
    assert_eq!(i, "1");
    assert_eq!(v, "1");
}

#[test]
fn vm_parity_while_loop() {
    let src = "let x=0; while(x<5){x=x+1;} x;";
    let (i, v) = run_both(src);
    assert_eq!(i, "5");
    assert_eq!(v, "5");
}

#[test]
fn vm_parity_conditional() {
    let src = "let x = 1 > 0 ? 'yes' : 'no'; x;";
    let (i, v) = run_both(src);
    assert_eq!(i, "yes");
    assert_eq!(v, "yes");
}

#[test]
fn vm_parity_template_literal() {
    let src = "let x=42; `value is ${x}`;";
    let (i, v) = run_both(src);
    assert_eq!(i, "value is 42");
    assert_eq!(v, "value is 42");
}

#[test]
fn vm_parity_typeof() {
    let (i, v) = run_both("typeof 42;");
    assert_eq!(i, "number");
    assert_eq!(v, "number");
}

#[test]
fn vm_parity_short_circuit() {
    let src = "let x = null || 'default'; x;";
    let (i, v) = run_both(src);
    assert_eq!(i, "default");
    assert_eq!(v, "default");
}

#[test]
fn vm_parity_for_in_object() {
    let src = "let o={a:1,b:2}; let keys=[]; for(let k in o){keys.push(k);} keys.join(',');";
    let (i, v) = run_both(src);
    // Both must contain both keys; order may differ between engines.
    for expected in ["a", "b"] {
        assert!(i.contains(expected), "interpreter missing {expected}: {i}");
        assert!(v.contains(expected), "vm missing {expected}: {v}");
    }
    assert_eq!(i.len(), v.len());
}

#[test]
fn vm_parity_method_call() {
    let src = "let o={x:3,f:function(){return this.x;}}; o.f();";
    let (i, v) = run_both(src);
    assert_eq!(i, "3");
    assert_eq!(v, "3");
}

#[test]
fn vm_parity_new_constructor() {
    let src = "function C(x){this.x=x;} C.prototype.get=function(){return this.x;}; let c=new C(7); c.get();";
    let (i, v) = run_both(src);
    assert_eq!(i, "7");
    assert_eq!(v, "7");
}

// --- Generator functions / methods ---

#[test]
fn generator_function_next_yields_values_in_order() {
    let src = "function* g(){ yield 1; yield 2; } var it = g(); \
               print(it.next().value); print(it.next().value); \
               print(it.next().done); print(it.next().done);";
    let out = run_cli(&["-e", src]);
    assert_eq!(out, "1\n2\ntrue\ntrue");
}

#[test]
fn named_generator_expression_runs() {
    let src = "var f = function* named(){ yield 42; }; var it = f(); print(it.next().value);";
    assert_eq!(run_cli(&["-e", src]), "42");
}

#[test]
fn generator_method_in_object_literal_runs() {
    let src = "var o = { *m(){ yield 7; yield 8; } }; var it = o.m(); \
               print(it.next().value); print(it.next().value);";
    assert_eq!(run_cli(&["-e", src]), "7\n8");
}

#[test]
fn async_generator_parses_and_runs_without_error() {
    let src = "async function* ag(){ yield 1; } var it = ag(); \
               print(it.next().value); print(typeof ag);";
    assert_eq!(run_cli(&["-e", src]), "1\nfunction");
}

#[test]
fn vm_generator_does_not_crash() {
    // The VM does not resume generator bodies yet; it must still parse and
    // return a valid iterator-result object instead of panicking.
    let src = "function* g(){ yield 1; } var it = g(); var r = it.next(); print(r.done);";
    assert_eq!(run_cli(&["--vm", "-e", src]), "true");
}

// --- Class declarations / expressions ---

#[test]
fn class_with_constructor_sets_instance_field() {
    let src = "class C { constructor(x){ this.x = x; } } let c = new C(5); c.x;";
    assert_eq!(run_cli(&["-e", src]), "5");
}

#[test]
fn class_with_method_runs() {
    let src = "class C { m(){ return 1; } } new C().m();";
    assert_eq!(run_cli(&["-e", src]), "1");
}

#[test]
fn class_extends_super_calls_parent_constructor() {
    let src = "class C { constructor(x){ this.x = x; } } \
               class D extends C { constructor(){ super(1); } } new D().x;";
    assert_eq!(run_cli(&["-e", src]), "1");
}

#[test]
fn class_expression_runs() {
    let src = "var E = class { f(){ return 2; } }; new E().f();";
    assert_eq!(run_cli(&["-e", src]), "2");
}

#[test]
fn class_static_method_and_getter_and_private_field() {
    let src = "class C { static s(){ return 3; } get g(){ return this.x; } \
               #p = 7; m(){ return this.#p; } } \
               let c = new C(); c.x = 9; \
               C.s() + c.g + c.m();";
    assert_eq!(run_cli(&["-e", src]), "19");
}

#[test]
fn vm_parity_class_constructor_and_method() {
    let src = "class C { constructor(x){ this.x = x; } m(){ return this.x + 1; } } \
               new C(5).m();";
    let (i, v) = run_both(src);
    assert_eq!(i, "6");
    assert_eq!(v, "6");
}
