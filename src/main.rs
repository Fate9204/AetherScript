use std::error::Error;
use std::io::{self, BufRead, Write};
use std::panic;
use std::thread;

use aetherscript::evaluator::{Environment, Scope, execute};
use aetherscript::lexer::Lexer;
use aetherscript::parser::Parser;
use aetherscript::token::Token;

const INTERPRETER_STACK_BYTES: usize = 256 * 1024 * 1024;

fn main() -> io::Result<()> {
    let session = thread::Builder::new()
        .stack_size(INTERPRETER_STACK_BYTES)
        .spawn(|| repl(io::stdin().lock(), io::stdout(), io::stderr()))?;
    session
        .join()
        .unwrap_or_else(|payload| panic::resume_unwind(payload))
}

fn repl(mut input: impl BufRead, mut out: impl Write, mut errors: impl Write) -> io::Result<()> {
    let globals = Environment::global();
    let mut source = String::new();
    let mut open_blocks = 0;

    loop {
        let prompt = if source.is_empty() { ">> " } else { ".. " };
        write!(out, "{prompt}")?;
        out.flush()?;

        let mut raw_line = Vec::new();
        if input.read_until(b'\n', &mut raw_line)? == 0 {
            if !source.is_empty() {
                report(run(&globals, &source, &mut out), &mut errors)?;
            }
            return writeln!(out);
        }
        let line = String::from_utf8_lossy(&raw_line);
        open_blocks = (open_blocks + block_delta(&line)).max(0);
        source.push_str(&line);
        if open_blocks > 0 {
            continue;
        }
        report(run(&globals, &source, &mut out), &mut errors)?;
        source.clear();
    }
}

fn run(scope: &Scope, source: &str, out: &mut impl Write) -> Result<(), Box<dyn Error>> {
    let statements = Parser::new(Lexer::new(source)).parse_program()?;
    execute(scope, &statements, out)?;
    Ok(())
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

    fn session(input: &[u8]) -> (String, String) {
        let (mut out, mut errors) = (Vec::new(), Vec::new());
        repl(input, &mut out, &mut errors).unwrap();
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
}
