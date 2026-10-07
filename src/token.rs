use std::fmt;

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

impl fmt::Display for Token<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Ident(name) => write!(f, "identifier `{name}`"),
            Token::Int(value) => write!(f, "integer `{value}`"),
            Token::Plus => f.write_str("`+`"),
            Token::Minus => f.write_str("`-`"),
            Token::Star => f.write_str("`*`"),
            Token::Slash => f.write_str("`/`"),
            Token::Assign => f.write_str("`=`"),
            Token::EqEq => f.write_str("`==`"),
            Token::Gt => f.write_str("`>`"),
            Token::Lt => f.write_str("`<`"),
            Token::If => f.write_str("`if`"),
            Token::Else => f.write_str("`else`"),
            Token::While => f.write_str("`while`"),
            Token::Print => f.write_str("`print`"),
            Token::Newline => f.write_str("newline"),
            Token::Colon => f.write_str("`:`"),
            Token::LParen => f.write_str("`(`"),
            Token::RParen => f.write_str("`)`"),
            Token::Illegal => f.write_str("illegal token"),
            Token::Eof => f.write_str("end of input"),
        }
    }
}
