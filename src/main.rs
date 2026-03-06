use std::env;
use std::fs;
use std::io::{self, BufRead, Write};

use viper::interpreter::Interpreter;
use viper::lexer::Lexer;
use viper::parser::Parser;
use viper::symbol::Interner;

fn run_code(code: &str, interp: &mut Interpreter) -> Result<(), String> {
    let mut lexer = Lexer::new(code);
    let tokens = lexer.tokenize()?;
    let stmts = {
        let mut parser = Parser::new(tokens, interp.interner_mut());
        parser.parse()?
    };
    interp.run(&stmts)
}

fn run_file(path: &str) -> Result<(), String> {
    let code = fs::read_to_string(path).map_err(|e| format!("Cannot read file: {}", e))?;
    let mut interp = Interpreter::new(Interner::new());
    run_code(&code, &mut interp)
}

fn run_repl() {
    println!("Viper v0.1.0 — Python interpreter");
    println!("Type Python code. Press Ctrl+D to exit.\n");

    let stdin = io::stdin();
    let mut interp = Interpreter::new(Interner::new());

    loop {
        print!(">>> ");
        io::stdout().flush().unwrap();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => {
                println!();
                break;
            }
            Ok(_) => {
                if line.trim().is_empty() {
                    continue;
                }

                // Collect multi-line input for blocks
                if line.trim_end().ends_with(':') {
                    let mut block = line.clone();
                    loop {
                        print!("... ");
                        io::stdout().flush().unwrap();
                        let mut next_line = String::new();
                        match stdin.lock().read_line(&mut next_line) {
                            Ok(0) => break,
                            Ok(_) => {
                                if next_line.trim().is_empty() {
                                    break;
                                }
                                block.push_str(&next_line);
                            }
                            Err(e) => {
                                eprintln!("Error: {}", e);
                                break;
                            }
                        }
                    }
                    line = block;
                }

                match run_code(&line, &mut interp) {
                    Ok(()) => {}
                    Err(e) => eprintln!("Error: {}", e),
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                break;
            }
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 {
        if let Err(e) = run_file(&args[1]) {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    } else {
        run_repl();
    }
}
