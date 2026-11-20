//! `nous-js arquivo.js` roda um arquivo; sem argumentos abre um REPL simples.

use std::io::{BufRead, Write};
use std::process::ExitCode;

use nous_js::{JsError, Vm};

fn main() -> ExitCode {
    let mut vm = Vm::new();
    if let Some(path) = std::env::args().nth(1) {
        let source = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("não consegui ler {path}: {e}");
                return ExitCode::FAILURE;
            }
        };
        return match vm.eval(&source) {
            Ok(_) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }

    println!("nous-js (esqueleto) — digite JavaScript; linha vazia ou Ctrl+Z/Ctrl+D para sair");
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        print!("> ");
        let _ = std::io::stdout().flush();
        let Some(Ok(line)) = lines.next() else { break };
        if line.trim().is_empty() {
            break;
        }
        match vm.eval(&line) {
            Ok(v) => {
                let s = vm.display(v);
                println!("{s}");
            }
            Err(e @ (JsError::Syntax(_) | JsError::Unsupported(_) | JsError::Thrown(_))) => {
                println!("{e}")
            }
        }
    }
    ExitCode::SUCCESS
}
