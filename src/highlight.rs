use crate::lexer::Lexer;
use crate::token::Token;

const RESET: &str = "\x1b[0m";
const MAGENTA: &str = "\x1b[35m";
const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const GREEN: &str = "\x1b[32m";
const GRAY: &str = "\x1b[90m";
const BOLD_RED: &str = "\x1b[31;1m";

pub fn highlight(source: &str) -> String {
    let mut lexer = Lexer::new(source);
    let mut rendered = String::with_capacity(source.len() * 2);
    loop {
        let start = lexer.offset();
        let token = lexer.next_token();
        let (gap, text) = split_token(&source[start..lexer.offset()], token);
        push_gap(&mut rendered, gap);
        push_styled(&mut rendered, style_of(token), text);
        if token == Token::Eof {
            return rendered;
        }
    }
}

pub fn error(message: &str) -> String {
    format!("{BOLD_RED}{message}{RESET}")
}

pub fn echo(prompt: &str, line: &str) -> String {
    format!("\x1b[1A\r\x1b[2K{prompt}{}\n", highlight(line))
}

fn split_token<'a>(consumed: &'a str, token: Token<'_>) -> (&'a str, &'a str) {
    match token {
        Token::Eof => (consumed, ""),
        Token::Newline => consumed.split_at(consumed.len() - 1),
        _ => {
            let blanks = consumed.len() - consumed.trim_start_matches([' ', '\t', '\r']).len();
            consumed.split_at(blanks)
        }
    }
}

fn push_gap(rendered: &mut String, gap: &str) {
    match gap.find('#') {
        Some(comment_start) => {
            rendered.push_str(&gap[..comment_start]);
            push_styled(rendered, Some(GRAY), &gap[comment_start..]);
        }
        None => rendered.push_str(gap),
    }
}

fn push_styled(rendered: &mut String, style: Option<&str>, text: &str) {
    match style {
        Some(style) => {
            rendered.push_str(style);
            rendered.push_str(text);
            rendered.push_str(RESET);
        }
        None => rendered.push_str(text),
    }
}

fn style_of(token: Token<'_>) -> Option<&'static str> {
    match token {
        Token::If
        | Token::Else
        | Token::While
        | Token::End
        | Token::Def
        | Token::Return
        | Token::Print
        | Token::True
        | Token::False => Some(MAGENTA),
        Token::Int(_) => Some(CYAN),
        Token::Ident(_) => Some(YELLOW),
        Token::String(_) => Some(GREEN),
        Token::Illegal => Some(BOLD_RED),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visible_text(styled: &str) -> String {
        let mut text = String::new();
        let mut characters = styled.chars();
        while let Some(character) = characters.next() {
            if character == '\x1b' {
                characters.by_ref().find(|&terminator| terminator == 'm');
            } else {
                text.push(character);
            }
        }
        text
    }

    #[test]
    fn each_token_class_gets_its_color_and_operators_stay_plain() {
        assert_eq!(
            highlight("while x < 10:"),
            format!("{MAGENTA}while{RESET} {YELLOW}x{RESET} < {CYAN}10{RESET}:")
        );
        assert_eq!(
            highlight("print(\"hi\") == true"),
            format!("{MAGENTA}print{RESET}({GREEN}\"hi\"{RESET}) == {MAGENTA}true{RESET}")
        );
        assert_eq!(
            highlight("def f(a, b): end"),
            format!(
                "{MAGENTA}def{RESET} {YELLOW}f{RESET}({YELLOW}a{RESET}, {YELLOW}b{RESET}): {MAGENTA}end{RESET}"
            )
        );
    }

    #[test]
    fn two_character_comparisons_stay_plain_and_a_lone_bang_is_invalid() {
        assert_eq!(
            highlight("a <= b != c >= d"),
            format!("{YELLOW}a{RESET} <= {YELLOW}b{RESET} != {YELLOW}c{RESET} >= {YELLOW}d{RESET}")
        );
        assert_eq!(
            highlight("!x"),
            format!("{BOLD_RED}!{RESET}{YELLOW}x{RESET}")
        );
    }

    #[test]
    fn invalid_syntax_is_bold_red() {
        assert_eq!(
            highlight("x @ 99999999999999999999"),
            format!("{YELLOW}x{RESET} {BOLD_RED}@{RESET} {BOLD_RED}99999999999999999999{RESET}")
        );
        assert_eq!(
            highlight("\"unterminated"),
            format!("{BOLD_RED}\"unterminated{RESET}")
        );
    }

    #[test]
    fn comments_are_gray_and_a_hash_in_a_string_is_text() {
        assert_eq!(
            highlight("x = 1  # note"),
            format!("{YELLOW}x{RESET} = {CYAN}1{RESET}  {GRAY}# note{RESET}")
        );
        assert_eq!(highlight("\"a # b\""), format!("{GREEN}\"a # b\"{RESET}"));
        assert_eq!(
            highlight("# whole line"),
            format!("{GRAY}# whole line{RESET}")
        );
    }

    #[test]
    fn highlighting_never_changes_the_visible_characters() {
        let sources = [
            "",
            "   ",
            "x = 5",
            "  x   =\t5  ",
            "while counter < 3:\r\n    counter = counter + 1\r\nend\r\n",
            "print(\"h\u{e9}llo w\u{f6}rld\") # \u{1f600} comment\n",
            "x @ $ ` ~ \u{e9} \u{1f600}",
            "\"unterminated\nx = 1",
            "# comment only\n\n\n",
            "a=1;b=2",
            "99999999999999999999 007 0",
            "end # trailing",
        ];
        for source in sources {
            assert_eq!(visible_text(&highlight(source)), source, "{source:?}");
        }
    }

    #[test]
    fn every_color_start_is_closed_by_a_reset() {
        let styled = highlight("while x < 10: # go\n  print(\"a\", @)\nend");
        let resets = styled.matches(RESET).count();
        let escapes = styled.matches('\x1b').count();
        assert_eq!(escapes, resets * 2);
        assert!(styled.ends_with(RESET) || !styled.contains('\x1b'));
    }

    #[test]
    fn diagnostics_are_wrapped_in_bold_red() {
        assert_eq!(error("boom"), "\x1b[31;1mboom\x1b[0m");
    }

    #[test]
    fn echo_replaces_the_previous_row_with_the_prompt_and_highlighted_line() {
        assert_eq!(
            echo(">> ", "x = 5"),
            format!("\x1b[1A\r\x1b[2K>> {YELLOW}x{RESET} = {CYAN}5{RESET}\n")
        );
    }
}
