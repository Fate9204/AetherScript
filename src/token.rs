#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token<'a> {
    Ident(&'a str),
    Int(i64),
    Plus,
    Minus,
    Star,
    Slash,
    Assign,
    EqEq,
    Gt,
    Lt,
    If,
    Else,
    While,
    Print,
    Newline,
    Colon,
    LParen,
    RParen,
    Illegal,
    Eof,
}

impl<'a> Token<'a> {
    pub fn from_word(word: &'a str) -> Self {
        match word {
            "if" => Token::If,
            "else" => Token::Else,
            "while" => Token::While,
            "print" => Token::Print,
            _ => Token::Ident(word),
        }
    }
}
