use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token<'a> {
    Ident(&'a str),
    Int(i64),
    String(&'a str),
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
    End,
    Def,
    Return,
    Print,
    True,
    False,
    Newline,
    Colon,
    Comma,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Illegal,
    Eof,
}

impl<'a> Token<'a> {
    pub fn from_word(word: &'a str) -> Self {
        match word {
            "if" => Token::If,
            "else" => Token::Else,
            "while" => Token::While,
            "end" => Token::End,
            "def" => Token::Def,
            "return" => Token::Return,
            "true" => Token::True,
            "false" => Token::False,
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
            Token::String(text) => write!(f, "string \"{text}\""),
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
            Token::End => f.write_str("`end`"),
            Token::Def => f.write_str("`def`"),
            Token::Return => f.write_str("`return`"),
            Token::Print => f.write_str("`print`"),
            Token::True => f.write_str("`true`"),
            Token::False => f.write_str("`false`"),
            Token::Newline => f.write_str("newline"),
            Token::Colon => f.write_str("`:`"),
            Token::Comma => f.write_str("`,`"),
            Token::LParen => f.write_str("`(`"),
            Token::RParen => f.write_str("`)`"),
            Token::LBracket => f.write_str("`[`"),
            Token::RBracket => f.write_str("`]`"),
            Token::Illegal => f.write_str("illegal token"),
            Token::Eof => f.write_str("end of input"),
        }
    }
}
