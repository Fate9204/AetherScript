use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Equal,
    Greater,
    Less,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expression<'a> {
    Int(i64),
    Ident(&'a str),
    Binary {
        op: BinaryOp,
        left: Box<Expression<'a>>,
        right: Box<Expression<'a>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Statement<'a> {
    Assign {
        name: &'a str,
        value: Expression<'a>,
    },
    Print(Expression<'a>),
    While {
        condition: Expression<'a>,
        body: Vec<Statement<'a>>,
    },
}

impl fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            BinaryOp::Add => "+",
            BinaryOp::Subtract => "-",
            BinaryOp::Multiply => "*",
            BinaryOp::Divide => "/",
            BinaryOp::Equal => "==",
            BinaryOp::Greater => ">",
            BinaryOp::Less => "<",
        })
    }
}
