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

    pub fn offset(&self) -> usize {
        self.pos
    }

    pub fn next_token(&mut self) -> Token<'a> {
        self.skip_blanks_and_comment();

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
            b'"' => self.string_literal(),
            _ => {
                self.pos += 1;
                self.punctuation(byte)
            }
        }
    }

    fn string_literal(&mut self) -> Token<'a> {
        self.pos += 1;
        let start = self.pos;
        self.advance_while(|b| b != b'"' && b != b'\n');
        if self.peek() != Some(b'"') {
            return Token::Illegal;
        }
        let text = &self.input[start..self.pos];
        self.pos += 1;
        Token::String(text)
    }

    fn punctuation(&mut self, byte: u8) -> Token<'a> {
        match byte {
            b'+' => Token::Plus,
            b'-' => Token::Minus,
            b'*' => Token::Star,
            b'/' => Token::Slash,
            b'>' if self.eat(b'=') => Token::GtEq,
            b'>' => Token::Gt,
            b'<' if self.eat(b'=') => Token::LtEq,
            b'<' => Token::Lt,
            b'!' if self.eat(b'=') => Token::NotEq,
            b':' => Token::Colon,
            b',' => Token::Comma,
            b'.' => Token::Dot,
            b'(' => Token::LParen,
            b')' => Token::RParen,
            b'[' => Token::LBracket,
            b']' => Token::RBracket,
            b'=' if self.eat(b'=') => Token::EqEq,
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

    fn eat(&mut self, byte: u8) -> bool {
        let found = self.peek() == Some(byte);
        if found {
            self.pos += 1;
        }
        found
    }

    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }

    fn skip_blanks_and_comment(&mut self) {
        self.advance_while(|b| matches!(b, b' ' | b'\t' | b'\r'));
        if self.peek() == Some(b'#') {
            self.advance_while(|b| b != b'\n');
        }
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
            tokenize(
                "if else while end def return print true false iffy _print end_x endless _end End END truest"
            ),
            [
                Token::If,
                Token::Else,
                Token::While,
                Token::End,
                Token::Def,
                Token::Return,
                Token::Print,
                Token::True,
                Token::False,
                Token::Ident("iffy"),
                Token::Ident("_print"),
                Token::Ident("end_x"),
                Token::Ident("endless"),
                Token::Ident("_end"),
                Token::Ident("End"),
                Token::Ident("END"),
                Token::Ident("truest"),
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
    fn tokenizes_function_header_with_comma_separated_parameters() {
        assert_eq!(
            tokenize("def add(a, b):\n"),
            [
                Token::Def,
                Token::Ident("add"),
                Token::LParen,
                Token::Ident("a"),
                Token::Comma,
                Token::Ident("b"),
                Token::RParen,
                Token::Colon,
                Token::Newline,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenizes_string_literals_without_their_quotes() {
        assert_eq!(
            tokenize("x = \"hello world\" \"\" \"h\u{e9}llo\""),
            [
                Token::Ident("x"),
                Token::Assign,
                Token::String("hello world"),
                Token::String(""),
                Token::String("h\u{e9}llo"),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn unterminated_string_is_illegal_and_stops_at_the_line_end() {
        assert_eq!(
            tokenize("x = \"abc\ny"),
            [
                Token::Ident("x"),
                Token::Assign,
                Token::Illegal,
                Token::Newline,
                Token::Ident("y"),
                Token::Eof,
            ]
        );
        assert_eq!(tokenize("\"abc"), [Token::Illegal, Token::Eof]);
    }

    #[test]
    fn tokenizes_array_literal_and_index_brackets() {
        assert_eq!(
            tokenize("[1, \"a\", true][0]"),
            [
                Token::LBracket,
                Token::Int(1),
                Token::Comma,
                Token::String("a"),
                Token::Comma,
                Token::True,
                Token::RBracket,
                Token::LBracket,
                Token::Int(0),
                Token::RBracket,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenizes_method_call_dot_and_slice_colon() {
        assert_eq!(
            tokenize("xs.len()[-1:2]"),
            [
                Token::Ident("xs"),
                Token::Dot,
                Token::Ident("len"),
                Token::LParen,
                Token::RParen,
                Token::LBracket,
                Token::Minus,
                Token::Int(1),
                Token::Colon,
                Token::Int(2),
                Token::RBracket,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn comments_run_to_the_end_of_the_line_and_keep_the_newline() {
        assert_eq!(
            tokenize("x = 1 # note\n# whole line\ny # tail"),
            [
                Token::Ident("x"),
                Token::Assign,
                Token::Int(1),
                Token::Newline,
                Token::Newline,
                Token::Ident("y"),
                Token::Eof,
            ]
        );
        assert_eq!(tokenize("# only a comment"), [Token::Eof]);
        assert_eq!(
            tokenize("# crlf\r\nx"),
            [Token::Newline, Token::Ident("x"), Token::Eof]
        );
    }

    #[test]
    fn a_hash_inside_a_string_is_text_not_a_comment() {
        assert_eq!(
            tokenize("\"a # b\" # real comment"),
            [Token::String("a # b"), Token::Eof]
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

    #[test]
    fn two_character_comparisons_are_single_tokens() {
        assert_eq!(
            tokenize("<= >= != == < > = !"),
            [
                Token::LtEq,
                Token::GtEq,
                Token::NotEq,
                Token::EqEq,
                Token::Lt,
                Token::Gt,
                Token::Assign,
                Token::Illegal,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn comparisons_split_greedily_without_spaces() {
        assert_eq!(
            tokenize("a<=b>=c!=d"),
            [
                Token::Ident("a"),
                Token::LtEq,
                Token::Ident("b"),
                Token::GtEq,
                Token::Ident("c"),
                Token::NotEq,
                Token::Ident("d"),
                Token::Eof,
            ]
        );
        assert_eq!(tokenize("<=="), [Token::LtEq, Token::Assign, Token::Eof]);
        assert_eq!(tokenize("!!="), [Token::Illegal, Token::NotEq, Token::Eof]);
        assert_eq!(tokenize("! ="), [Token::Illegal, Token::Assign, Token::Eof]);
        assert_eq!(tokenize("< ="), [Token::Lt, Token::Assign, Token::Eof]);
    }

    #[test]
    fn identifiers_may_contain_digits_and_underscores_after_the_first_character() {
        assert_eq!(
            tokenize("x1 _2 a_b3 y10"),
            [
                Token::Ident("x1"),
                Token::Ident("_2"),
                Token::Ident("a_b3"),
                Token::Ident("y10"),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn crlf_line_endings_advance_the_line_counter_once() {
        let mut lexer = Lexer::new("a\r\nb\r\n\r\nc");
        while lexer.next_token() != Token::Eof {}
        assert_eq!(lexer.line(), 4);
    }
}
