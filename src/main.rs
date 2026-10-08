use std::error::Error;
use std::io::{self, BufRead, Write};

use aetherscript::evaluator::Environment;
use aetherscript::lexer::Lexer;
use aetherscript::parser::Parser;
use aetherscript::token::Token;

fn main() -> io::Result<()> {
    let mut input = io::stdin().lock();
    let mut stdout = io::stdout();
    let mut environment = Environment::default();
    let mut source = String::new();

    loop {
        let prompt = if source.is_empty() { ">> " } else { ".. " };
        write!(stdout, "{prompt}")?;
        stdout.flush()?;

        if input.read_line(&mut source)? == 0 {
            return Ok(());
        }
        if open_blocks(&source) > 0 {
            continue;
        }
        if let Err(error) = run(&mut environment, &source, &mut stdout) {
            eprintln!("error: {error}");
        }
        source.clear();
    }
}

fn run(
    environment: &mut Environment,
    source: &str,
    out: &mut impl Write,
) -> Result<(), Box<dyn Error>> {
    let statements = Parser::new(Lexer::new(source)).parse_program()?;
    environment.execute(&statements, out)?;
    Ok(())
}

fn open_blocks(source: &str) -> usize {
    let mut lexer = Lexer::new(source);
    let mut depth = 0usize;
    loop {
        match lexer.next_token() {
            Token::While => depth += 1,
            Token::End => depth = depth.saturating_sub(1),
            Token::Eof => return depth,
            _ => {}
        }
    }
}
