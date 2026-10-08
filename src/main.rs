use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt::Display;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::panic;
use std::process::{Command, ExitCode};
use std::thread;

use aetherscript::compiler::Compiler;
use aetherscript::evaluator::{Environment, Scope, execute};
use aetherscript::highlight;
use aetherscript::lexer::Lexer;
use aetherscript::parser::Parser;
use aetherscript::token::Token;
use aetherscript::vm::VirtualMachine;

const INTERPRETER_STACK_BYTES: usize = 256 * 1024 * 1024;
const REDRAW_COLUMNS: usize = 80;

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

#[derive(Clone, Copy)]
struct Presentation {
    color_errors: bool,
    redraw_input: bool,
}

impl Presentation {
    const PLAIN: Self = Self {
        color_errors: false,
        redraw_input: false,
    };

    fn detect(arguments: &[String]) -> Self {
        let no_color = env::var_os("NO_COLOR");
        if !color_allowed(arguments, no_color.as_deref()) {
            return Self::PLAIN;
        }
        let on_console = io::stderr().is_terminal() || io::stdout().is_terminal();
        if cfg!(windows) && on_console {
            enable_ansi_escapes();
        }
        Self {
            color_errors: io::stderr().is_terminal(),
            // Unix ttys echo on arrival; redrawing would misplace pasted lines.
            redraw_input: cfg!(windows) && io::stdin().is_terminal() && io::stdout().is_terminal(),
        }
    }

    fn diagnostic(self, error: &impl Display) -> String {
        let text = format!("error: {error}");
        if self.color_errors {
            highlight::error(&text)
        } else {
            text
        }
    }

    fn redraw(self, prompt: &str, line: &str) -> Option<String> {
        let text = line.strip_suffix('\n')?.trim_end_matches('\r');
        let fits = prompt.len() + text.len() < REDRAW_COLUMNS;
        let plain = text.bytes().all(|byte| matches!(byte, b' '..=b'~'));
        (self.redraw_input && fits && plain).then(|| highlight::echo(prompt, text))
    }
}

fn color_allowed(arguments: &[String], no_color: Option<&OsStr>) -> bool {
    let disabled_by_environment = no_color.is_some_and(|value| !value.is_empty());
    !disabled_by_environment && !arguments.iter().any(|argument| argument == "--no-color")
}

fn enable_ansi_escapes() {
    // Starting cmd switches the console into escape-sequence mode.
    let _ = Command::new("cmd").args(["/C", ""]).status();
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
    let presentation = Presentation::detect(&arguments);
    let session = thread::Builder::new()
        .stack_size(INTERPRETER_STACK_BYTES)
        .spawn(move || launch(&arguments, presentation).map_err(|error| error.to_string()));
    let outcome = match session {
        Ok(handle) => handle
            .join()
            .unwrap_or_else(|payload| panic::resume_unwind(payload)),
        Err(error) => Err(error.to_string()),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{}", presentation.diagnostic(&message));
            ExitCode::FAILURE
        }
    }
}

fn launch(arguments: &[String], presentation: Presentation) -> Result<(), Box<dyn Error>> {
    let engine = select_engine(arguments);
    match script_path(arguments) {
        Some(path) => run_script(path, engine, &mut io::stdout().lock()),
        None => Ok(repl(
            io::stdin().lock(),
            io::stdout(),
            io::stderr(),
            engine,
            presentation,
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
    presentation: Presentation,
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
                report(engine.run(&source, &mut out), &mut errors, presentation)?;
            }
            return writeln!(out);
        }
        let line = String::from_utf8_lossy(&raw_line);
        if let Some(redrawn) = presentation.redraw(prompt, &line) {
            out.write_all(redrawn.as_bytes())?;
        }
        open_blocks = (open_blocks + block_delta(&line)).max(0);
        source.push_str(&line);
        if open_blocks > 0 {
            continue;
        }
        report(engine.run(&source, &mut out), &mut errors, presentation)?;
        source.clear();
    }
}

fn report(
    result: Result<(), Box<dyn Error>>,
    errors: &mut impl Write,
    presentation: Presentation,
) -> io::Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(error) => writeln!(errors, "{}", presentation.diagnostic(&error)),
    }
}

fn block_delta(line: &str) -> isize {
    match Lexer::new(line).next_token() {
        Token::If | Token::While | Token::Def => 1,
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
        styled_session(input, engine, Presentation::PLAIN)
    }

    fn styled_session(
        input: &[u8],
        engine: Engine,
        presentation: Presentation,
    ) -> (String, String) {
        let (mut out, mut errors) = (Vec::new(), Vec::new());
        repl(input, &mut out, &mut errors, engine, presentation).unwrap();
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

    const REDRAWING: Presentation = Presentation {
        color_errors: false,
        redraw_input: true,
    };

    #[test]
    fn a_redrawing_session_repaints_each_echoed_line_in_colour() {
        let (out, errors) = styled_session(
            b"x = 5\nprint(x)\n",
            Engine::Bytecode(VirtualMachine::default()),
            REDRAWING,
        );
        let repaint = "\x1b[1A\r\x1b[2K";
        assert_eq!(
            out,
            format!(
                ">> {repaint}>> \x1b[33mx\x1b[0m = \x1b[36m5\x1b[0m\n\
                 >> {repaint}>> \x1b[35mprint\x1b[0m(\x1b[33mx\x1b[0m)\n5\n>> \n"
            )
        );
        assert_eq!(errors, "");
    }

    #[test]
    fn lines_that_cannot_be_repainted_in_place_are_left_as_typed() {
        let long_comment = format!("# {}\n", "a".repeat(REDRAW_COLUMNS));
        let input = [
            b"# \xc3\xa9\n".as_slice(),
            long_comment.as_bytes(),
            b"x = 1",
        ]
        .concat();
        let (out, errors) = styled_session(
            &input,
            Engine::Bytecode(VirtualMachine::default()),
            REDRAWING,
        );
        assert_eq!(out, ">> >> >> >> \n");
        assert_eq!(errors, "");
    }

    #[test]
    fn a_windows_line_ending_is_repainted_without_its_carriage_return() {
        let (out, _) = styled_session(
            b"7\r\n",
            Engine::Bytecode(VirtualMachine::default()),
            REDRAWING,
        );
        assert_eq!(out, ">> \x1b[1A\r\x1b[2K>> \x1b[36m7\x1b[0m\n>> \n");
    }

    #[test]
    fn coloured_diagnostics_are_bold_red_and_plain_ones_are_untouched() {
        let coloured = Presentation {
            color_errors: true,
            redraw_input: false,
        };
        let (out, errors) = styled_session(
            b"print(y)\n",
            Engine::TreeWalker(Environment::global()),
            coloured,
        );
        assert_eq!(out, ">> >> \n");
        assert_eq!(errors, "\x1b[31;1merror: undefined variable `y`\x1b[0m\n");
        assert_eq!(Presentation::PLAIN.diagnostic(&"boom"), "error: boom");
        assert_eq!(coloured.diagnostic(&"boom"), "\x1b[31;1merror: boom\x1b[0m");
    }

    #[test]
    fn colour_is_disabled_by_the_flag_and_by_a_non_empty_no_color_variable() {
        assert!(color_allowed(&[], None));
        assert!(color_allowed(&[], Some(OsStr::new(""))));
        assert!(!color_allowed(&[], Some(OsStr::new("1"))));
        assert!(!color_allowed(&arguments(&["--no-color"]), None));
        assert!(!color_allowed(&arguments(&["a.ae", "--no-color"]), None));
    }

    const PRIMES: &str = r"limit = 20
num = 2

while num <= limit:
    is_prime = 1
    divisor = 2
    
    # Nested check loop
    while divisor * divisor <= num:
        # Check remainder using a subtraction loop
        temp = num
        while temp >= divisor:
            temp = temp - divisor
        end
        
        # If temp is 0, divisor divides num evenly (not prime)
        if temp == 0:
            is_prime = 0
        end
        
        divisor = divisor + 1
    end
    
    if is_prime == 1:
        print(num)
    end
    
    num = num + 1
end
";
    const PRIMES_BELOW_TWENTY: &str = "2\n3\n5\n7\n11\n13\n17\n19\n";

    #[test]
    fn an_if_else_block_keeps_the_continuation_prompt_until_its_end() {
        let (out, errors) = session_with(
            b"x = 2\nif x == 2:\nprint(1)\nelse:\nprint(2)\nend\nprint(3)\n",
            Engine::Bytecode(VirtualMachine::default()),
        );
        assert_eq!(out, ">> >> .. .. .. .. 1\n>> 3\n>> \n");
        assert_eq!(errors, "");
    }

    #[test]
    fn an_else_without_an_if_is_reported_and_the_session_continues() {
        let (out, errors) = session_with(
            b"else:\nprint(1)\n",
            Engine::Bytecode(VirtualMachine::default()),
        );
        assert_eq!(out, ">> >> 1\n>> \n");
        assert_eq!(errors, "error: line 1: expected statement, found `else`\n");
    }

    #[test]
    fn the_prime_program_runs_as_a_script_on_both_engines() {
        for engine in [
            Engine::Bytecode(VirtualMachine::default()),
            Engine::TreeWalker(Environment::global()),
        ] {
            let (result, out) = run_file("primes", PRIMES.as_bytes(), engine);
            assert_eq!(result, Ok(()));
            assert_eq!(out, PRIMES_BELOW_TWENTY);
        }
    }

    #[test]
    fn the_prime_program_can_be_pasted_into_the_repl() {
        let (out, errors) = session_with(
            PRIMES.as_bytes(),
            Engine::Bytecode(VirtualMachine::default()),
        );
        assert_eq!(errors, "");
        assert_eq!(
            out.replace(">> ", "").replace(".. ", ""),
            format!("{PRIMES_BELOW_TWENTY}\n")
        );
    }
}
