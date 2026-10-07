use std::fmt;

use crate::ast::{BinaryOp, Expression, Statement};
use crate::lexer::Lexer;
use crate::token::Token;

const LOWEST_BINDING: u8 = 0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub expected: String,
    pub found: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "line {}: expected {}, found {}",
            self.line, self.expected, self.found
        )
    }
}

impl std::error::Error for ParseError {}

pub struct Parser<'a> {
    lexer: Lexer<'a>,
    current_token: Token<'a>,
    peek_token: Token<'a>,
    current_line: usize,
    peek_line: usize,
}

impl<'a> Parser<'a> {
    pub fn new(lexer: Lexer<'a>) -> Self {
        let mut parser = Self {
            lexer,
            current_token: Token::Eof,
            peek_token: Token::Eof,
            current_line: 1,
            peek_line: 1,
        };
        parser.advance();
        parser.advance();
        parser
    }

    pub fn parse_program(&mut self) -> Result<Vec<Statement<'a>>, ParseError> {
        let mut statements = Vec::new();
        loop {
            while self.current_token == Token::Newline {
                self.advance();
            }
            if self.current_token == Token::Eof {
                return Ok(statements);
            }
            statements.push(self.parse_statement()?);
            self.advance();
        }
    }

    fn parse_statement(&mut self) -> Result<Statement<'a>, ParseError> {
        let statement = match self.current_token {
            Token::Ident(name) => {
                self.expect_peek(Token::Assign)?;
                self.advance();
                Statement::Assign {
                    name,
                    value: self.parse_expression(LOWEST_BINDING)?,
                }
            }
            Token::Print => {
                self.expect_peek(Token::LParen)?;
                self.advance();
                let value = self.parse_expression(LOWEST_BINDING)?;
                self.expect_peek(Token::RParen)?;
                Statement::Print(value)
            }
            _ => return Err(self.error_at_current("statement")),
        };
        match self.peek_token {
            Token::Newline | Token::Eof => Ok(statement),
            _ => Err(self.error_at_peek("end of statement")),
        }
    }

    fn parse_expression(&mut self, min_binding: u8) -> Result<Expression<'a>, ParseError> {
        let mut left = self.parse_primary()?;
        while let Some((op, binding)) = infix_binding(self.peek_token)
            && binding >= min_binding
        {
            self.advance();
            self.advance();
            let right = self.parse_expression(binding + 1)?;
            left = Expression::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_primary(&mut self) -> Result<Expression<'a>, ParseError> {
        match self.current_token {
            Token::Int(value) => Ok(Expression::Int(value)),
            Token::Ident(name) => Ok(Expression::Ident(name)),
            Token::LParen => {
                self.advance();
                let inner = self.parse_expression(LOWEST_BINDING)?;
                self.expect_peek(Token::RParen)?;
                Ok(inner)
            }
            _ => Err(self.error_at_current("expression")),
        }
    }

    fn advance(&mut self) {
        self.current_token = self.peek_token;
        self.current_line = self.peek_line;
        self.peek_line = self.lexer.line();
        self.peek_token = self.lexer.next_token();
    }

    fn expect_peek(&mut self, expected: Token<'a>) -> Result<(), ParseError> {
        if self.peek_token != expected {
            return Err(self.error_at_peek(&expected.to_string()));
        }
        self.advance();
        Ok(())
    }

    fn error_at_current(&self, expected: &str) -> ParseError {
        ParseError {
            line: self.current_line,
            expected: expected.to_owned(),
            found: self.current_token.to_string(),
        }
    }

    fn error_at_peek(&self, expected: &str) -> ParseError {
        ParseError {
            line: self.peek_line,
            expected: expected.to_owned(),
            found: self.peek_token.to_string(),
        }
    }
}

fn infix_binding(token: Token<'_>) -> Option<(BinaryOp, u8)> {
    match token {
        Token::EqEq => Some((BinaryOp::Equal, 1)),
        Token::Lt => Some((BinaryOp::Less, 2)),
        Token::Gt => Some((BinaryOp::Greater, 2)),
        Token::Plus => Some((BinaryOp::Add, 3)),
        Token::Minus => Some((BinaryOp::Subtract, 3)),
        Token::Star => Some((BinaryOp::Multiply, 4)),
        Token::Slash => Some((BinaryOp::Divide, 4)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Result<Vec<Statement<'_>>, ParseError> {
        Parser::new(Lexer::new(source)).parse_program()
    }

    fn binary<'a>(op: BinaryOp, left: Expression<'a>, right: Expression<'a>) -> Expression<'a> {
        Expression::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn multiplication_binds_tighter_than_addition() {
        assert_eq!(
            parse("x = 5 + 2 * 3").unwrap(),
            [Statement::Assign {
                name: "x",
                value: binary(
                    BinaryOp::Add,
                    Expression::Int(5),
                    binary(BinaryOp::Multiply, Expression::Int(2), Expression::Int(3)),
                ),
            }]
        );
    }

    #[test]
    fn multiplication_on_the_left_groups_first() {
        assert_eq!(
            parse("x = 2 * 3 + 5").unwrap(),
            [Statement::Assign {
                name: "x",
                value: binary(
                    BinaryOp::Add,
                    binary(BinaryOp::Multiply, Expression::Int(2), Expression::Int(3)),
                    Expression::Int(5),
                ),
            }]
        );
    }

    #[test]
    fn operators_of_equal_precedence_associate_left() {
        assert_eq!(
            parse("x = 8 / 4 / 2 - 1").unwrap(),
            [Statement::Assign {
                name: "x",
                value: binary(
                    BinaryOp::Subtract,
                    binary(
                        BinaryOp::Divide,
                        binary(BinaryOp::Divide, Expression::Int(8), Expression::Int(4)),
                        Expression::Int(2),
                    ),
                    Expression::Int(1),
                ),
            }]
        );
    }

    #[test]
    fn parentheses_override_precedence() {
        assert_eq!(
            parse("x = (5 + 2) * 3").unwrap(),
            [Statement::Assign {
                name: "x",
                value: binary(
                    BinaryOp::Multiply,
                    binary(BinaryOp::Add, Expression::Int(5), Expression::Int(2)),
                    Expression::Int(3),
                ),
            }]
        );
    }

    #[test]
    fn comparisons_bind_looser_than_arithmetic() {
        assert_eq!(
            parse("x = a + 1 == b * 2").unwrap(),
            [Statement::Assign {
                name: "x",
                value: binary(
                    BinaryOp::Equal,
                    binary(BinaryOp::Add, Expression::Ident("a"), Expression::Int(1)),
                    binary(
                        BinaryOp::Multiply,
                        Expression::Ident("b"),
                        Expression::Int(2)
                    ),
                ),
            }]
        );
    }

    #[test]
    fn relational_operators_bind_tighter_than_equality() {
        assert_eq!(
            parse("x = a == b < c").unwrap(),
            [Statement::Assign {
                name: "x",
                value: binary(
                    BinaryOp::Equal,
                    Expression::Ident("a"),
                    binary(
                        BinaryOp::Less,
                        Expression::Ident("b"),
                        Expression::Ident("c")
                    ),
                ),
            }]
        );
    }

    #[test]
    fn parses_print_statement_over_multiple_lines() {
        assert_eq!(
            parse("x = 10 \n\n print(x > 3)\r\n").unwrap(),
            [
                Statement::Assign {
                    name: "x",
                    value: Expression::Int(10),
                },
                Statement::Print(binary(
                    BinaryOp::Greater,
                    Expression::Ident("x"),
                    Expression::Int(3),
                )),
            ]
        );
    }

    #[test]
    fn accepts_empty_and_blank_input() {
        assert_eq!(parse("").unwrap(), []);
        assert_eq!(parse("\n \n\t\n").unwrap(), []);
    }

    #[test]
    fn reports_descriptive_errors_with_line_numbers() {
        let cases = [
            ("x =", "line 1: expected expression, found end of input"),
            ("x = 1 +\n", "line 1: expected expression, found newline"),
            ("x 5", "line 1: expected `=`, found integer `5`"),
            ("= 5", "line 1: expected statement, found `=`"),
            ("if x:", "line 1: expected statement, found `if`"),
            ("print x", "line 1: expected `(`, found identifier `x`"),
            ("print()", "line 1: expected expression, found `)`"),
            ("print(x", "line 1: expected `)`, found end of input"),
            ("x = (1 + 2", "line 1: expected `)`, found end of input"),
            (
                "x = 1 2",
                "line 1: expected end of statement, found integer `2`",
            ),
            ("x = 1 + 2)", "line 1: expected end of statement, found `)`"),
            (
                "x = 1 @",
                "line 1: expected end of statement, found illegal token",
            ),
            (
                "x = 99999999999999999999",
                "line 1: expected expression, found illegal token",
            ),
            ("x = 1\n\ny = )", "line 3: expected expression, found `)`"),
        ];
        for (source, message) in cases {
            assert_eq!(
                parse(source).unwrap_err().to_string(),
                message,
                "{source:?}"
            );
        }
    }
}
