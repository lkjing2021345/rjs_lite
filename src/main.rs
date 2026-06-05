use std::env;
use std::fs;
use std::process;

fn main() {
    if let Err(message) = run() {
        eprintln!("{message}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.as_slice() {
        [] => Err(usage()),
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{}", usage());
            Ok(())
        }
        [flag, source] if flag == "--agent-eval" => execute_agent_tool(source),
        [flag, source] if flag == "-e" || flag == "--eval" => execute(source),
        [path] => {
            let source = fs::read_to_string(path)
                .map_err(|err| format!("failed to read `{path}`: {err}"))?;
            execute(&source)
        }
        _ => Err(usage()),
    }
}

fn execute_agent_tool(source: &str) -> Result<(), String> {
    println!("{}", rjs_lite::run_agent_tool(source).to_json());
    Ok(())
}

fn execute(source: &str) -> Result<(), String> {
    let (value, output) =
        rjs_lite::run_source_with_output(source).map_err(|err| err.to_string())?;
    for line in output {
        println!("{line}");
    }
    if !matches!(value, rjs_lite::Value::Undefined) {
        println!("{value}");
    }
    Ok(())
}

fn usage() -> String {
    "Usage:\n  rjs_lite -e \"let x = 1 + 2; print(x);\"\n  rjs_lite --agent-eval \"let x = 1 + 2; x;\"\n  rjs_lite path/to/file.js\n\nA lightweight JavaScript execution runtime for AI Agent tool calls.".to_string()
}
