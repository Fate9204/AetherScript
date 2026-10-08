use crate::lexer::Lexer;
use crate::token::Token;

const RESET: &str = "\x1b[0m";
const CONTROL: &str = "\x1b[38;2;197;134;192m";
const KEYWORD: &str = "\x1b[38;2;86;156;214m";
const FUNCTION: &str = "\x1b[38;2;220;220;170m";
const VARIABLE: &str = "\x1b[38;2;156;220;254m";
const NUMBER: &str = "\x1b[38;2;181;206;168m";
const STRING: &str = "\x1b[38;2;206;145;120m";
const COMMENT: &str = "\x1b[38;2;106;153;85m";
const BOLD_RED: &str = "\x1b[31;1m";

pub fn highlight(source: &str) -> String {
    let mut lexer = Lexer::new(source);
    let mut rendered = String::with_capacity(source.len() * 2);
    loop {
        let start = lexer.offset();
        let token = lexer.next_token();
        let (gap, text) = split_token(&source[start..lexer.offset()], token);
        push_gap(&mut rendered, gap);
        push_styled(
            &mut rendered,
            style_of(token, &source[lexer.offset()..]),
            text,
        );
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
            push_styled(rendered, Some(COMMENT), &gap[comment_start..]);
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

fn style_of(token: Token<'_>, rest: &str) -> Option<&'static str> {
    match token {
        Token::If | Token::Else | Token::While | Token::End | Token::Return => Some(CONTROL),
        Token::Def | Token::True | Token::False => Some(KEYWORD),
        Token::Print => Some(FUNCTION),
        Token::Ident(_) if rest.trim_start_matches([' ', '\t', '\r']).starts_with('(') => {
            Some(FUNCTION)
        }
        Token::Ident(_) => Some(VARIABLE),
        Token::Int(_) => Some(NUMBER),
        Token::String(_) => Some(STRING),
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
            format!("{CONTROL}while{RESET} {VARIABLE}x{RESET} < {NUMBER}10{RESET}:")
        );
        assert_eq!(
            highlight("print(\"hi\") == true"),
            format!("{FUNCTION}print{RESET}({STRING}\"hi\"{RESET}) == {KEYWORD}true{RESET}")
        );
        assert_eq!(
            highlight("def f(a, b): end"),
            format!(
                "{KEYWORD}def{RESET} {FUNCTION}f{RESET}({VARIABLE}a{RESET}, {VARIABLE}b{RESET}): {CONTROL}end{RESET}"
            )
        );
    }

    #[test]
    fn two_character_comparisons_stay_plain_and_a_lone_bang_is_invalid() {
        assert_eq!(
            highlight("a <= b != c >= d"),
            format!(
                "{VARIABLE}a{RESET} <= {VARIABLE}b{RESET} != {VARIABLE}c{RESET} >= {VARIABLE}d{RESET}"
            )
        );
        assert_eq!(
            highlight("!x"),
            format!("{BOLD_RED}!{RESET}{VARIABLE}x{RESET}")
        );
    }

    #[test]
    fn a_name_followed_by_a_parenthesis_is_a_call_and_other_names_are_variables() {
        assert_eq!(
            highlight("f (1) + xs.push(2) + g\nh\n(3)"),
            format!(
                "{FUNCTION}f{RESET} ({NUMBER}1{RESET}) + {VARIABLE}xs{RESET}.{FUNCTION}push{RESET}({NUMBER}2{RESET}) + {VARIABLE}g{RESET}\n{VARIABLE}h{RESET}\n({NUMBER}3{RESET})"
            )
        );
    }

    #[test]
    fn control_flow_keywords_differ_from_declarations_and_literals() {
        assert_eq!(
            highlight("if else return end def false"),
            format!(
                "{CONTROL}if{RESET} {CONTROL}else{RESET} {CONTROL}return{RESET} {CONTROL}end{RESET} {KEYWORD}def{RESET} {KEYWORD}false{RESET}"
            )
        );
    }

    #[test]
    fn invalid_syntax_is_bold_red() {
        assert_eq!(
            highlight("x @ 99999999999999999999"),
            format!("{VARIABLE}x{RESET} {BOLD_RED}@{RESET} {BOLD_RED}99999999999999999999{RESET}")
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
            format!("{VARIABLE}x{RESET} = {NUMBER}1{RESET}  {COMMENT}# note{RESET}")
        );
        assert_eq!(highlight("\"a # b\""), format!("{STRING}\"a # b\"{RESET}"));
        assert_eq!(
            highlight("# whole line"),
            format!("{COMMENT}# whole line{RESET}")
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
            format!("\x1b[1A\r\x1b[2K>> {VARIABLE}x{RESET} = {NUMBER}5{RESET}\n")
        );
    }
}
