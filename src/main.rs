use std::env;
use std::error::Error;
use std::fs;
use std::io::{self, BufRead, Write};
use std::panic;
use std::process::ExitCode;
use std::thread;

use aetherscript::compiler::Compiler;
use aetherscript::evaluator::{Environment, Scope, execute};
use aetherscript::lexer::Lexer;
use aetherscript::parser::Parser;
use aetherscript::token::Token;
use aetherscript::vm::VirtualMachine;

const INTERPRETER_STACK_BYTES: usize = 256 * 1024 * 1024;

enum Engine {
    TreeWalker(Scope),
    Bytecode(VirtualMachine),
}

impl Engine {
    fn run(&mut self, source: &str, out: &mut impl Write) -> Result<(), Box<dyn Error>> {
        let statements = Parser::new(Lexer::new(source)).parse_program()?;
        match self {
            Engine::TreeWalker(globals) => execute(globals, &statements, out)?,
            Engine::Bytecode(machine) => {
                let chunk = Compiler::new(machine.symbols_mut()).compile(&statements)?;
                machine.run(&chunk, out)?;
            }
        }
        Ok(())
    }
}

fn select_engine(arguments: &[String]) -> Engine {
    if arguments.iter().any(|argument| argument == "--tree") {
        Engine::TreeWalker(Environment::global())
    } else {
        Engine::Bytecode(VirtualMachine::default())
    }
}

fn script_path(arguments: &[String]) -> Option<&str> {
    arguments
        .iter()
        .map(String::as_str)
        .find(|argument| !argument.starts_with("--"))
}

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let session = thread::Builder::new()
        .stack_size(INTERPRETER_STACK_BYTES)
        .spawn(move || launch(&arguments).map_err(|error| error.to_string()));
    let outcome = match session {
        Ok(handle) => handle
            .join()
            .unwrap_or_else(|payload| panic::resume_unwind(payload)),
        Err(error) => Err(error.to_string()),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn launch(arguments: &[String]) -> Result<(), Box<dyn Error>> {
    let engine = select_engine(arguments);
    match script_path(arguments) {
        Some(path) => run_script(path, engine, &mut io::stdout().lock()),
        None => Ok(repl(
            io::stdin().lock(),
            io::stdout(),
            io::stderr(),
            engine,
        )?),
    }
}

fn run_script(path: &str, mut engine: Engine, out: &mut impl Write) -> Result<(), Box<dyn Error>> {
    let source =
        fs::read_to_string(path).map_err(|error| format!("cannot read {path}: {error}"))?;
    engine.run(source.strip_prefix('\u{feff}').unwrap_or(&source), out)
}

fn repl(
    mut input: impl BufRead,
    mut out: impl Write,
    mut errors: impl Write,
    mut engine: Engine,
) -> io::Result<()> {
    let mut source = String::new();
    let mut open_blocks = 0;

    loop {
        let prompt = if source.is_empty() { ">> " } else { ".. " };
        write!(out, "{prompt}")?;
        out.flush()?;

        let mut raw_line = Vec::new();
        if input.read_until(b'\n', &mut raw_line)? == 0 {
            if !source.is_empty() {
                report(engine.run(&source, &mut out), &mut errors)?;
            }
            return writeln!(out);
        }
        let line = String::from_utf8_lossy(&raw_line);
        open_blocks = (open_blocks + block_delta(&line)).max(0);
        source.push_str(&line);
        if open_blocks > 0 {
            continue;
        }
        report(engine.run(&source, &mut out), &mut errors)?;
        source.clear();
    }
}

fn report(result: Result<(), Box<dyn Error>>, errors: &mut impl Write) -> io::Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(error) => writeln!(errors, "error: {error}"),
    }
}

fn block_delta(line: &str) -> isize {
    match Lexer::new(line).next_token() {
        Token::While | Token::Def => 1,
        Token::End => -1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn run_file(name: &str, contents: &[u8], engine: Engine) -> (Result<(), String>, String) {
        let path = env::temp_dir().join(format!("aetherscript-{}-{name}.ae", std::process::id()));
        fs::write(&path, contents).unwrap();
        let mut out = Vec::new();
        let result =
            run_script(path.to_str().unwrap(), engine, &mut out).map_err(|error| error.to_string());
        fs::remove_file(&path).unwrap();
        (result, String::from_utf8(out).unwrap())
    }

    fn session(input: &[u8]) -> (String, String) {
        session_with(input, Engine::TreeWalker(Environment::global()))
    }

    fn session_with(input: &[u8], engine: Engine) -> (String, String) {
        let (mut out, mut errors) = (Vec::new(), Vec::new());
        repl(input, &mut out, &mut errors, engine).unwrap();
        (
            String::from_utf8(out).unwrap(),
            String::from_utf8(errors).unwrap(),
        )
    }

    #[test]
    fn continuation_prompt_shows_until_every_block_is_closed() {
        let (out, errors) = session(b"def inc(n):\nreturn n + 1\nend\nprint(inc(1))\n");
        assert_eq!(out, ">> .. .. >> 2\n>> \n");
        assert_eq!(errors, "");
    }

    #[test]
    fn errors_go_to_the_error_stream_and_the_session_continues() {
        let (out, errors) = session(b"x = 1\nprint(y)\nprint(x)\n");
        assert_eq!(out, ">> >> >> 1\n>> \n");
        assert_eq!(errors, "error: undefined variable `y`\n");
    }

    #[test]
    fn end_inside_an_expression_does_not_close_a_block() {
        let (out, errors) = session(b"while 1 < end:\nprint(1)\nend\n");
        assert_eq!(out, ">> .. .. >> \n");
        assert_eq!(errors, "error: line 1: expected expression, found `end`\n");
    }

    #[test]
    fn end_of_input_inside_a_block_reports_the_missing_end() {
        let (out, errors) = session(b"while 1 < 2:\nprint(1)\n");
        assert_eq!(out, ">> .. .. \n");
        assert_eq!(
            errors,
            "error: line 3: expected `end`, found end of input\n"
        );
    }

    #[test]
    fn invalid_utf8_is_reported_without_ending_the_session() {
        let (out, errors) = session(b"x = \xe9\nprint(7)\n");
        assert_eq!(out, ">> >> 7\n>> \n");
        assert_eq!(errors.lines().count(), 1);
    }

    #[test]
    fn bytecode_engine_runs_loops_end_to_end_through_the_repl() {
        let engine = Engine::Bytecode(VirtualMachine::default());
        let (out, errors) =
            session_with(b"x = 5\nwhile x < 7:\nprint(x)\nx = x + 1\nend\n", engine);
        assert_eq!(out, ">> >> .. .. .. 5\n6\n>> \n");
        assert_eq!(errors, "");
    }

    #[test]
    fn bytecode_engine_keeps_globals_and_survives_unsupported_constructs() {
        let engine = Engine::Bytecode(VirtualMachine::default());
        let (out, errors) =
            session_with(b"x = 2\nprint(x * 21)\ndef f():\nend\nprint(x)\n", engine);
        assert_eq!(out, ">> >> 42\n>> .. >> 2\n>> \n");
        assert_eq!(
            errors,
            "error: the bytecode compiler does not support function definitions yet\n"
        );
    }

    #[test]
    fn the_bytecode_engine_is_the_default_and_tree_selects_the_tree_walker() {
        assert!(matches!(select_engine(&[]), Engine::Bytecode(_)));
        assert!(matches!(
            select_engine(&arguments(&["--tree"])),
            Engine::TreeWalker(_)
        ));
        assert!(matches!(
            select_engine(&arguments(&["--vm"])),
            Engine::Bytecode(_)
        ));
    }

    #[test]
    fn the_script_path_is_the_first_argument_that_is_not_a_flag() {
        assert_eq!(script_path(&arguments(&["a.ae"])), Some("a.ae"));
        assert_eq!(script_path(&arguments(&["--tree", "a.ae"])), Some("a.ae"));
        assert_eq!(script_path(&arguments(&["a.ae", "--tree"])), Some("a.ae"));
        assert_eq!(script_path(&arguments(&["--vm"])), None);
        assert_eq!(script_path(&[]), None);
    }

    #[test]
    fn a_script_file_runs_through_the_bytecode_engine_and_prints_its_output() {
        let script = b"x = 5\ny = 10\nwhile x < y:\nx = x + 1\nprint(x)\nend\nprint(x * 2)\n";
        let (result, out) = run_file("loop", script, Engine::Bytecode(VirtualMachine::default()));
        assert_eq!(result, Ok(()));
        assert_eq!(out, "6\n7\n8\n9\n10\n20\n");
    }

    #[test]
    fn a_script_file_can_use_the_full_language_with_the_tree_walker() {
        let script =
            b"def double(n):\nreturn n * 2\nend\nxs = [1, 2]\nxs.push(double(21))\nprint(xs)\n";
        let (result, out) = run_file("tree", script, Engine::TreeWalker(Environment::global()));
        assert_eq!(result, Ok(()));
        assert_eq!(out, "[1, 2, 42]\n");
    }

    #[test]
    fn a_comment_line_before_a_block_does_not_confuse_the_repl_depth() {
        let (out, errors) = session_with(
            b"# loop\nx = 0\nwhile x < 2: # header\nx = x + 1 # step\nend # done\nprint(x)\n",
            Engine::Bytecode(VirtualMachine::default()),
        );
        assert_eq!(out, ">> >> >> .. .. >> 2\n>> \n");
        assert_eq!(errors, "");
    }

    #[test]
    fn a_script_with_windows_line_endings_and_a_byte_order_mark_runs() {
        let script = b"\xEF\xBB\xBFx = 2\r\nprint(x * 21)\r\n";
        let (result, out) = run_file(
            "windows",
            script,
            Engine::Bytecode(VirtualMachine::default()),
        );
        assert_eq!(result, Ok(()));
        assert_eq!(out, "42\n");
    }

    #[test]
    fn a_failing_script_keeps_the_output_written_before_the_error() {
        let (result, out) = run_file(
            "runtime-error",
            b"print(1)\nprint(2 / 0)\nprint(3)\n",
            Engine::Bytecode(VirtualMachine::default()),
        );
        assert_eq!(result, Err("division by zero".to_owned()));
        assert_eq!(out, "1\n");
    }

    #[test]
    fn a_script_with_a_syntax_error_reports_the_line() {
        let (result, out) = run_file(
            "syntax-error",
            b"x = 1\ny = )\n",
            Engine::Bytecode(VirtualMachine::default()),
        );
        assert_eq!(
            result,
            Err("line 2: expected expression, found `)`".to_owned())
        );
        assert_eq!(out, "");
    }

    #[test]
    fn a_missing_script_is_reported_with_its_path() {
        let error = run_script(
            "does-not-exist.ae",
            Engine::Bytecode(VirtualMachine::default()),
            &mut Vec::new(),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.starts_with("cannot read does-not-exist.ae: "),
            "{error}"
        );
    }
}
