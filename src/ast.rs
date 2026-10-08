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
pub enum Expression {
    Int(i64),
    Ident(String),
    Binary {
        op: BinaryOp,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    Call {
        name: String,
        arguments: Vec<Expression>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Statement {
    Assign {
        name: String,
        value: Expression,
    },
    Print(Expression),
    While {
        condition: Expression,
        body: Vec<Statement>,
    },
    FunctionDef {
        name: String,
        params: Vec<String>,
        body: Vec<Statement>,
    },
    Return(Option<Expression>),
    Expression(Expression),
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
