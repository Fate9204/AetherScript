use crate::token::Token;

pub struct Lexer<'a> {
    input: &'a str,
    pos: usize,
    line: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            line: 1,
        }
    }

    pub fn line(&self) -> usize {
        self.line
    }

    pub fn next_token(&mut self) -> Token<'a> {
        self.advance_while(|b| matches!(b, b' ' | b'\t' | b'\r'));

        let Some(byte) = self.peek() else {
            return Token::Eof;
        };

        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                Token::from_word(self.take_while(|b| b.is_ascii_alphanumeric() || b == b'_'))
            }
            b'0'..=b'9' => self
                .take_while(|b| b.is_ascii_digit())
                .parse()
                .map_or(Token::Illegal, Token::Int),
            _ => {
                self.pos += 1;
                self.punctuation(byte)
            }
        }
    }

    fn punctuation(&mut self, byte: u8) -> Token<'a> {
        match byte {
            b'+' => Token::Plus,
            b'-' => Token::Minus,
            b'*' => Token::Star,
            b'/' => Token::Slash,
            b'>' => Token::Gt,
            b'<' => Token::Lt,
            b':' => Token::Colon,
            b'(' => Token::LParen,
            b')' => Token::RParen,
            b'=' if self.peek() == Some(b'=') => {
                self.pos += 1;
                Token::EqEq
            }
            b'=' => Token::Assign,
            b'\n' => {
                self.line += 1;
                Token::Newline
            }
            _ => {
                // skip continuation bytes to stay on a char boundary
                self.advance_while(|b| b & 0xC0 == 0x80);
                Token::Illegal
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }

    fn advance_while(&mut self, predicate: impl Fn(u8) -> bool) {
        while self.peek().is_some_and(&predicate) {
            self.pos += 1;
        }
    }

    fn take_while(&mut self, predicate: impl Fn(u8) -> bool) -> &'a str {
        let start = self.pos;
        self.advance_while(predicate);
        &self.input[start..self.pos]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenize(input: &str) -> Vec<Token<'_>> {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token();
            tokens.push(token);
            if token == Token::Eof {
                return tokens;
            }
        }
    }

    #[test]
    fn tokenizes_assignment_and_print() {
        assert_eq!(
            tokenize("x = 10 \n print(x)"),
            [
                Token::Ident("x"),
                Token::Assign,
                Token::Int(10),
                Token::Newline,
                Token::Print,
                Token::LParen,
                Token::Ident("x"),
                Token::RParen,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn distinguishes_assignment_from_comparison() {
        assert_eq!(
            tokenize("a == b = c"),
            [
                Token::Ident("a"),
                Token::EqEq,
                Token::Ident("b"),
                Token::Assign,
                Token::Ident("c"),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenizes_arithmetic_and_relational_operators() {
        assert_eq!(
            tokenize("+-*/><"),
            [
                Token::Plus,
                Token::Minus,
                Token::Star,
                Token::Slash,
                Token::Gt,
                Token::Lt,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn matches_keywords_only_on_whole_words() {
        assert_eq!(
            tokenize("if else while print iffy _print"),
            [
                Token::If,
                Token::Else,
                Token::While,
                Token::Print,
                Token::Ident("iffy"),
                Token::Ident("_print"),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenizes_block_header_with_colon() {
        assert_eq!(
            tokenize("while n < 3:\n"),
            [
                Token::While,
                Token::Ident("n"),
                Token::Lt,
                Token::Int(3),
                Token::Colon,
                Token::Newline,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn flags_unknown_and_non_ascii_characters_as_illegal() {
        assert_eq!(
            tokenize("x @ é 1"),
            [
                Token::Ident("x"),
                Token::Illegal,
                Token::Illegal,
                Token::Int(1),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn flags_overflowing_integer_as_illegal() {
        assert_eq!(
            tokenize("9223372036854775807 9223372036854775808"),
            [Token::Int(i64::MAX), Token::Illegal, Token::Eof]
        );
    }

    #[test]
    fn treats_carriage_return_as_whitespace() {
        assert_eq!(
            tokenize("a\r\nb"),
            [
                Token::Ident("a"),
                Token::Newline,
                Token::Ident("b"),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tracks_line_numbers() {
        let mut lexer = Lexer::new("a\n\nb");
        assert_eq!(lexer.line(), 1);
        while lexer.next_token() != Token::Eof {}
        assert_eq!(lexer.line(), 3);
    }

    #[test]
    fn keeps_returning_eof_after_exhaustion() {
        let mut lexer = Lexer::new("");
        assert_eq!(lexer.next_token(), Token::Eof);
        assert_eq!(lexer.next_token(), Token::Eof);
    }
}
