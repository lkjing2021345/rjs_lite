use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

use rjs_lite::Value;
use rjs_lite::interpreter::Interpreter;
use rjs_lite::lexer::lex;
use rjs_lite::parser::parse;

fn main() {
    if let Err(message) = run() {
        eprintln!("{message}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.as_slice() {
        [] => repl(),
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{}", usage());
            Ok(())
        }
        [flag] if flag == "--repl" => repl(),
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

fn repl() -> Result<(), String> {
    let mut interpreter = Interpreter::new();
    let stdin = io::stdin();
    let mut line = String::new();

    println!("rjs_lite REPL. 输入 .exit 或 .quit 以退出");

    loop {
        print!("rjs> ");
        io::stdout().flush().map_err(|e| e.to_string())?;

        line.clear();
        let bytes = stdin.read_line(&mut line).map_err(|e| e.to_string())?;
        if bytes == 0 {
            println!();
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == ".exit" || trimmed == ".quit" {
            break;
        }

        match lex(trimmed) {
            Ok(tokens) => match parse(tokens) {
                Ok(program) => match interpreter.run(&program) {
                    Ok(value) => {
                        let output = interpreter.take_output();
                        for line in &output {
                            println!("{line}");
                        }
                        if !matches!(value, Value::Undefined) {
                            println!("{value}");
                        }
                    }
                    Err(e) => eprintln!("{e}"),
                },
                Err(e) => eprintln!("{e}"),
            },
            Err(e) => eprintln!("{e}"),
        }
    }
    Ok(())
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
    "Usage:\n  rjs_lite                  start REPL\n  rjs_lite --repl           start REPL\n  rjs_lite -e \"...\"          evaluate inline source\n  rjs_lite --agent-eval \"...\" agent tool mode\n  rjs_lite path/to/file.js   evaluate file\n\nA lightweight JavaScript execution runtime for AI Agent tool calls.".to_string()
}
