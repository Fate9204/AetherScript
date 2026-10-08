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

    pub fn parse_program(&mut self) -> Result<Vec<Statement>, ParseError> {
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

    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        let statement = match self.current_token {
            Token::Ident(_)
                if matches!(
                    self.peek_token,
                    Token::LParen | Token::LBracket | Token::Dot
                ) =>
            {
                match self.parse_expression(LOWEST_BINDING)? {
                    Expression::Index { target, index } if self.peek_token == Token::Assign => {
                        self.advance();
                        self.advance();
                        Statement::IndexAssign {
                            target: *target,
                            index: *index,
                            value: self.parse_expression(LOWEST_BINDING)?,
                        }
                    }
                    expression => Statement::Expression(expression),
                }
            }
            Token::Ident(name) => {
                self.expect_peek(Token::Assign)?;
                self.advance();
                Statement::Assign {
                    name: name.to_owned(),
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
            Token::If => {
                self.advance();
                let condition = self.parse_expression(LOWEST_BINDING)?;
                self.expect_block_opening()?;
                let then_body = self.parse_block(&[Token::Else, Token::End])?;
                let else_body = if self.current_token == Token::Else {
                    self.expect_block_opening()?;
                    self.parse_block(&[Token::End])?
                } else {
                    Vec::new()
                };
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                }
            }
            Token::While => {
                self.advance();
                let condition = self.parse_expression(LOWEST_BINDING)?;
                self.expect_block_opening()?;
                Statement::While {
                    condition,
                    body: self.parse_block(&[Token::End])?,
                }
            }
            Token::Def => {
                let name = self.expect_ident("function name")?;
                self.expect_peek(Token::LParen)?;
                let params = self.parse_list(Token::RParen, Self::parse_parameter)?;
                self.expect_block_opening()?;
                Statement::FunctionDef {
                    name,
                    params,
                    body: self.parse_block(&[Token::End])?.into(),
                }
            }
            Token::Return => {
                let value = if matches!(self.peek_token, Token::Newline | Token::Eof) {
                    None
                } else {
                    self.advance();
                    Some(self.parse_expression(LOWEST_BINDING)?)
                };
                Statement::Return(value)
            }
            _ => return Err(self.error_at_current("statement")),
        };
        match self.peek_token {
            Token::Newline | Token::Eof => Ok(statement),
            _ => Err(self.error_at_peek("end of statement")),
        }
    }

    fn expect_block_opening(&mut self) -> Result<(), ParseError> {
        self.expect_peek(Token::Colon)?;
        self.expect_peek(Token::Newline)
    }

    fn parse_block(&mut self, closers: &[Token<'_>]) -> Result<Vec<Statement>, ParseError> {
        let mut body = Vec::new();
        loop {
            self.advance();
            while self.current_token == Token::Newline {
                self.advance();
            }
            if closers.contains(&self.current_token) {
                return Ok(body);
            }
            if self.current_token == Token::Eof {
                return Err(self.error_at_current("`end`"));
            }
            body.push(self.parse_statement()?);
        }
    }

    fn parse_expression(&mut self, min_binding: u8) -> Result<Expression, ParseError> {
        let mut left = self.parse_unary()?;
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

    fn parse_unary(&mut self) -> Result<Expression, ParseError> {
        if self.current_token != Token::Minus {
            return self.parse_postfix();
        }
        self.advance();
        Ok(Expression::Negate(Box::new(self.parse_unary()?)))
    }

    fn parse_postfix(&mut self) -> Result<Expression, ParseError> {
        let mut target = self.parse_primary()?;
        loop {
            target = match self.peek_token {
                Token::LBracket => self.parse_subscript(target)?,
                Token::Dot => self.parse_method_call(target)?,
                _ => return Ok(target),
            };
        }
    }

    fn parse_subscript(&mut self, target: Expression) -> Result<Expression, ParseError> {
        self.advance();
        let start = if self.peek_token == Token::Colon {
            None
        } else {
            self.advance();
            Some(self.parse_expression(LOWEST_BINDING)?)
        };
        let target = Box::new(target);
        match start {
            Some(index) if self.peek_token != Token::Colon => {
                self.expect_peek(Token::RBracket)?;
                Ok(Expression::Index {
                    target,
                    index: Box::new(index),
                })
            }
            start => {
                self.expect_peek(Token::Colon)?;
                let end = if self.peek_token == Token::RBracket {
                    None
                } else {
                    self.advance();
                    Some(Box::new(self.parse_expression(LOWEST_BINDING)?))
                };
                self.expect_peek(Token::RBracket)?;
                Ok(Expression::Slice {
                    target,
                    start: start.map(Box::new),
                    end,
                })
            }
        }
    }

    fn parse_method_call(&mut self, target: Expression) -> Result<Expression, ParseError> {
        self.advance();
        let method = self.expect_ident("method name")?;
        self.expect_peek(Token::LParen)?;
        let arguments = self.parse_list(Token::RParen, Self::parse_full_expression)?;
        Ok(Expression::MethodCall {
            target: Box::new(target),
            method,
            arguments,
        })
    }

    fn parse_full_expression(&mut self) -> Result<Expression, ParseError> {
        self.parse_expression(LOWEST_BINDING)
    }

    fn parse_primary(&mut self) -> Result<Expression, ParseError> {
        match self.current_token {
            Token::Int(value) => Ok(Expression::Int(value)),
            Token::True => Ok(Expression::Bool(true)),
            Token::False => Ok(Expression::Bool(false)),
            Token::String(text) => Ok(Expression::String(text.to_owned())),
            Token::LBracket => {
                let items = self.parse_list(Token::RBracket, Self::parse_full_expression)?;
                Ok(Expression::Array(items))
            }
            Token::Ident(name) if self.peek_token == Token::LParen => {
                self.advance();
                let arguments = self.parse_list(Token::RParen, Self::parse_full_expression)?;
                Ok(Expression::Call {
                    name: name.to_owned(),
                    arguments,
                })
            }
            Token::Ident(name) => Ok(Expression::Ident(name.to_owned())),
            Token::LParen => {
                self.advance();
                let inner = self.parse_expression(LOWEST_BINDING)?;
                self.expect_peek(Token::RParen)?;
                Ok(inner)
            }
            _ => Err(self.error_at_current("expression")),
        }
    }

    fn parse_list<T>(
        &mut self,
        closing: Token<'static>,
        parse_item: impl Fn(&mut Self) -> Result<T, ParseError>,
    ) -> Result<Vec<T>, ParseError> {
        let mut items = Vec::new();
        if self.peek_token == closing {
            self.advance();
            return Ok(items);
        }
        self.advance();
        items.push(parse_item(self)?);
        while self.peek_token == Token::Comma {
            self.advance();
            self.advance();
            items.push(parse_item(self)?);
        }
        self.expect_peek(closing)?;
        Ok(items)
    }

    fn parse_parameter(&mut self) -> Result<String, ParseError> {
        match self.current_token {
            Token::Ident(name) => Ok(name.to_owned()),
            _ => Err(self.error_at_current("parameter name")),
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

    fn expect_ident(&mut self, expected: &str) -> Result<String, ParseError> {
        let Token::Ident(name) = self.peek_token else {
            return Err(self.error_at_peek(expected));
        };
        self.advance();
        Ok(name.to_owned())
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
        Token::NotEq => Some((BinaryOp::NotEqual, 1)),
        Token::Lt => Some((BinaryOp::Less, 2)),
        Token::LtEq => Some((BinaryOp::LessEqual, 2)),
        Token::Gt => Some((BinaryOp::Greater, 2)),
        Token::GtEq => Some((BinaryOp::GreaterEqual, 2)),
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

    fn parse(source: &str) -> Result<Vec<Statement>, ParseError> {
        Parser::new(Lexer::new(source)).parse_program()
    }

    fn ident(name: &str) -> Expression {
        Expression::Ident(name.to_owned())
    }

    fn assign(name: &str, value: Expression) -> Statement {
        Statement::Assign {
            name: name.to_owned(),
            value,
        }
    }

    fn index(target: Expression, position: Expression) -> Expression {
        Expression::Index {
            target: Box::new(target),
            index: Box::new(position),
        }
    }

    fn binary(op: BinaryOp, left: Expression, right: Expression) -> Expression {
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
            [assign(
                "x",
                binary(
                    BinaryOp::Add,
                    Expression::Int(5),
                    binary(BinaryOp::Multiply, Expression::Int(2), Expression::Int(3)),
                )
            )]
        );
    }

    #[test]
    fn multiplication_on_the_left_groups_first() {
        assert_eq!(
            parse("x = 2 * 3 + 5").unwrap(),
            [assign(
                "x",
                binary(
                    BinaryOp::Add,
                    binary(BinaryOp::Multiply, Expression::Int(2), Expression::Int(3)),
                    Expression::Int(5),
                )
            )]
        );
    }

    #[test]
    fn operators_of_equal_precedence_associate_left() {
        assert_eq!(
            parse("x = 8 / 4 / 2 - 1").unwrap(),
            [assign(
                "x",
                binary(
                    BinaryOp::Subtract,
                    binary(
                        BinaryOp::Divide,
                        binary(BinaryOp::Divide, Expression::Int(8), Expression::Int(4)),
                        Expression::Int(2),
                    ),
                    Expression::Int(1),
                )
            )]
        );
    }

    #[test]
    fn parentheses_override_precedence() {
        assert_eq!(
            parse("x = (5 + 2) * 3").unwrap(),
            [assign(
                "x",
                binary(
                    BinaryOp::Multiply,
                    binary(BinaryOp::Add, Expression::Int(5), Expression::Int(2)),
                    Expression::Int(3),
                )
            )]
        );
    }

    #[test]
    fn comparisons_bind_looser_than_arithmetic() {
        assert_eq!(
            parse("x = a + 1 == b * 2").unwrap(),
            [assign(
                "x",
                binary(
                    BinaryOp::Equal,
                    binary(BinaryOp::Add, ident("a"), Expression::Int(1)),
                    binary(BinaryOp::Multiply, ident("b"), Expression::Int(2)),
                )
            )]
        );
    }

    #[test]
    fn relational_operators_bind_tighter_than_equality() {
        assert_eq!(
            parse("x = a == b < c").unwrap(),
            [assign(
                "x",
                binary(
                    BinaryOp::Equal,
                    ident("a"),
                    binary(BinaryOp::Less, ident("b"), ident("c")),
                )
            )]
        );
    }

    #[test]
    fn greater_than_binds_looser_than_subtraction_and_division() {
        assert_eq!(
            parse("x = a == b > c - d / e").unwrap(),
            [assign(
                "x",
                binary(
                    BinaryOp::Equal,
                    ident("a"),
                    binary(
                        BinaryOp::Greater,
                        ident("b"),
                        binary(
                            BinaryOp::Subtract,
                            ident("c"),
                            binary(BinaryOp::Divide, ident("d"), ident("e")),
                        ),
                    ),
                )
            )]
        );
    }

    #[test]
    fn less_than_binds_looser_than_addition() {
        assert_eq!(
            parse("x = a == b < c + d").unwrap(),
            [assign(
                "x",
                binary(
                    BinaryOp::Equal,
                    ident("a"),
                    binary(
                        BinaryOp::Less,
                        ident("b"),
                        binary(BinaryOp::Add, ident("c"), ident("d")),
                    ),
                )
            )]
        );
    }

    #[test]
    fn parses_print_statement_over_multiple_lines() {
        assert_eq!(
            parse("x = 10 \n\n print(x > 3)\r\n").unwrap(),
            [
                assign("x", Expression::Int(10)),
                Statement::Print(binary(BinaryOp::Greater, ident("x"), Expression::Int(3),)),
            ]
        );
    }

    #[test]
    fn parses_while_loop_with_multi_statement_body() {
        assert_eq!(
            parse("while x < 3:\n print(x)\n\n x = x + 1\nend\n").unwrap(),
            [Statement::While {
                condition: binary(BinaryOp::Less, ident("x"), Expression::Int(3)),
                body: vec![
                    Statement::Print(ident("x")),
                    assign("x", binary(BinaryOp::Add, ident("x"), Expression::Int(1))),
                ],
            }]
        );
    }

    #[test]
    fn nested_loops_close_innermost_end_first() {
        assert_eq!(
            parse("while a < 1:\nwhile b < 2:\nb = 2\nend\na = 1\nend\nprint(a)").unwrap(),
            [
                Statement::While {
                    condition: binary(BinaryOp::Less, ident("a"), Expression::Int(1)),
                    body: vec![
                        Statement::While {
                            condition: binary(BinaryOp::Less, ident("b"), Expression::Int(2),),
                            body: vec![assign("b", Expression::Int(2))],
                        },
                        assign("a", Expression::Int(1)),
                    ],
                },
                Statement::Print(ident("a")),
            ]
        );
    }

    #[test]
    fn parses_function_definition_with_parameters() {
        assert_eq!(
            parse("def add(a, b):\n return a + b\nend\n").unwrap(),
            [Statement::FunctionDef {
                name: "add".to_owned(),
                params: vec!["a".to_owned(), "b".to_owned()],
                body: [Statement::Return(Some(binary(
                    BinaryOp::Add,
                    ident("a"),
                    ident("b"),
                )))]
                .into(),
            }]
        );
    }

    #[test]
    fn parses_parameterless_function_with_bare_return() {
        assert_eq!(
            parse("def f():\nreturn\nend").unwrap(),
            [Statement::FunctionDef {
                name: "f".to_owned(),
                params: vec![],
                body: [Statement::Return(None)].into(),
            }]
        );
    }

    #[test]
    fn parses_calls_as_expressions_and_statements() {
        assert_eq!(
            parse("x = add(1, 2 * 3) + f()\ngreet(add(x, 1))").unwrap(),
            [
                assign(
                    "x",
                    binary(
                        BinaryOp::Add,
                        Expression::Call {
                            name: "add".to_owned(),
                            arguments: vec![
                                Expression::Int(1),
                                binary(BinaryOp::Multiply, Expression::Int(2), Expression::Int(3)),
                            ],
                        },
                        Expression::Call {
                            name: "f".to_owned(),
                            arguments: vec![],
                        },
                    ),
                ),
                Statement::Expression(Expression::Call {
                    name: "greet".to_owned(),
                    arguments: vec![Expression::Call {
                        name: "add".to_owned(),
                        arguments: vec![ident("x"), Expression::Int(1)],
                    }],
                }),
            ]
        );
    }

    #[test]
    fn parses_string_boolean_and_array_literals() {
        assert_eq!(
            parse("x = [1, \"two\", true, f(3), [false]]\ny = []\nz = \"hi there\"").unwrap(),
            [
                assign(
                    "x",
                    Expression::Array(vec![
                        Expression::Int(1),
                        Expression::String("two".to_owned()),
                        Expression::Bool(true),
                        Expression::Call {
                            name: "f".to_owned(),
                            arguments: vec![Expression::Int(3)],
                        },
                        Expression::Array(vec![Expression::Bool(false)]),
                    ]),
                ),
                assign("y", Expression::Array(vec![])),
                assign("z", Expression::String("hi there".to_owned())),
            ]
        );
    }

    #[test]
    fn index_binds_tighter_than_any_binary_operator() {
        assert_eq!(
            parse("x = 2 * xs[1] + 1").unwrap(),
            [assign(
                "x",
                binary(
                    BinaryOp::Add,
                    binary(
                        BinaryOp::Multiply,
                        Expression::Int(2),
                        index(ident("xs"), Expression::Int(1))
                    ),
                    Expression::Int(1),
                )
            )]
        );
    }

    #[test]
    fn indexes_chain_and_apply_to_any_primary() {
        assert_eq!(
            parse("x = m[0][1 + 1]\ny = \"abc\"[0]\nz = [1, 2][0]\nw = f(1)[2]").unwrap(),
            [
                assign(
                    "x",
                    index(
                        index(ident("m"), Expression::Int(0)),
                        binary(BinaryOp::Add, Expression::Int(1), Expression::Int(1)),
                    ),
                ),
                assign(
                    "y",
                    index(Expression::String("abc".to_owned()), Expression::Int(0)),
                ),
                assign(
                    "z",
                    index(
                        Expression::Array(vec![Expression::Int(1), Expression::Int(2)]),
                        Expression::Int(0),
                    ),
                ),
                assign(
                    "w",
                    index(
                        Expression::Call {
                            name: "f".to_owned(),
                            arguments: vec![Expression::Int(1)],
                        },
                        Expression::Int(2),
                    ),
                ),
            ]
        );
    }

    #[test]
    fn parses_index_assignment() {
        assert_eq!(
            parse("xs[i + 1] = 2 * 3").unwrap(),
            [Statement::IndexAssign {
                target: ident("xs"),
                index: binary(BinaryOp::Add, ident("i"), Expression::Int(1)),
                value: binary(BinaryOp::Multiply, Expression::Int(2), Expression::Int(3)),
            }]
        );
    }

    #[test]
    fn parses_multi_dimensional_index_assignment() {
        assert_eq!(
            parse("m[0][1] = 99").unwrap(),
            [Statement::IndexAssign {
                target: index(ident("m"), Expression::Int(0)),
                index: Expression::Int(1),
                value: Expression::Int(99),
            }]
        );
    }

    #[test]
    fn unary_minus_binds_tighter_than_binary_operators() {
        assert_eq!(
            parse("x = -5 * -y\nz = 2 - -3\nw = -xs[0]").unwrap(),
            [
                assign(
                    "x",
                    binary(
                        BinaryOp::Multiply,
                        Expression::Negate(Box::new(Expression::Int(5))),
                        Expression::Negate(Box::new(ident("y"))),
                    ),
                ),
                assign(
                    "z",
                    binary(
                        BinaryOp::Subtract,
                        Expression::Int(2),
                        Expression::Negate(Box::new(Expression::Int(3))),
                    ),
                ),
                assign(
                    "w",
                    Expression::Negate(Box::new(index(ident("xs"), Expression::Int(0)))),
                ),
            ]
        );
    }

    #[test]
    fn parses_slices_with_optional_and_negative_bounds() {
        let slice = |start: Option<Expression>, end: Option<Expression>| Expression::Slice {
            target: Box::new(ident("xs")),
            start: start.map(Box::new),
            end: end.map(Box::new),
        };
        assert_eq!(
            parse("a = xs[1:3]\nb = xs[:2]\nc = xs[1:]\nd = xs[:]\ne = xs[-2:4]").unwrap(),
            [
                assign(
                    "a",
                    slice(Some(Expression::Int(1)), Some(Expression::Int(3)))
                ),
                assign("b", slice(None, Some(Expression::Int(2)))),
                assign("c", slice(Some(Expression::Int(1)), None)),
                assign("d", slice(None, None)),
                assign(
                    "e",
                    slice(
                        Some(Expression::Negate(Box::new(Expression::Int(2)))),
                        Some(Expression::Int(4)),
                    ),
                ),
            ]
        );
    }

    #[test]
    fn parses_chained_method_calls_as_expressions_and_statements() {
        let method =
            |target: Expression, name: &str, arguments: Vec<Expression>| Expression::MethodCall {
                target: Box::new(target),
                method: name.to_owned(),
                arguments,
            };
        assert_eq!(
            parse("n = xs.push(1 + 2).len()\nxs.push(3)").unwrap(),
            [
                assign(
                    "n",
                    method(
                        method(
                            ident("xs"),
                            "push",
                            vec![binary(
                                BinaryOp::Add,
                                Expression::Int(1),
                                Expression::Int(2)
                            )],
                        ),
                        "len",
                        vec![],
                    ),
                ),
                Statement::Expression(method(ident("xs"), "push", vec![Expression::Int(3)])),
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
            ("else:", "line 1: expected statement, found `else`"),
            ("if x:", "line 1: expected newline, found end of input"),
            ("if x\nend", "line 1: expected `:`, found newline"),
            ("if:\nend", "line 1: expected expression, found `:`"),
            (
                "if x:\nprint(1)",
                "line 2: expected `end`, found end of input",
            ),
            (
                "if x:\nelse:\nprint(1)",
                "line 3: expected `end`, found end of input",
            ),
            ("if x:\nelse\nend", "line 2: expected `:`, found newline"),
            (
                "if x:\nelse:\nelse:\nend",
                "line 3: expected statement, found `else`",
            ),
            (
                "while x:\nelse:\nend",
                "line 2: expected statement, found `else`",
            ),
            (
                "def f():\nelse:\nend",
                "line 2: expected statement, found `else`",
            ),
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
            (
                "while x < 3:",
                "line 1: expected newline, found end of input",
            ),
            (
                "while x < 3:\nx = 1",
                "line 2: expected `end`, found end of input",
            ),
            (
                "while x < 3\nx = 1\nend",
                "line 1: expected `:`, found newline",
            ),
            (
                "while x < 3: x = 1\nend",
                "line 1: expected newline, found identifier `x`",
            ),
            ("while:\nend", "line 1: expected expression, found `:`"),
            (
                "while x < 3:\nx =\nend",
                "line 2: expected expression, found newline",
            ),
            (
                "while x < 3:\nend 1",
                "line 2: expected end of statement, found integer `1`",
            ),
            ("end", "line 1: expected statement, found `end`"),
            ("x = [1, 2", "line 1: expected `]`, found end of input"),
            ("x = [1,]", "line 1: expected expression, found `]`"),
            ("x = [1 2]", "line 1: expected `]`, found integer `2`"),
            ("x = [1, 2)", "line 1: expected `]`, found `)`"),
            ("f(1, 2]", "line 1: expected `)`, found `]`"),
            ("x = xs[]", "line 1: expected expression, found `]`"),
            ("x = xs[1", "line 1: expected `]`, found end of input"),
            ("xs[0] =", "line 1: expected expression, found end of input"),
            ("xs.", "line 1: expected method name, found end of input"),
            ("xs.5()", "line 1: expected method name, found integer `5`"),
            ("xs.len", "line 1: expected `(`, found end of input"),
            ("xs.push(1", "line 1: expected `)`, found end of input"),
            (
                "x = xs[1:",
                "line 1: expected expression, found end of input",
            ),
            ("x = xs[1:2", "line 1: expected `]`, found end of input"),
            ("x = xs[1:2:3]", "line 1: expected `]`, found `:`"),
            ("x = -", "line 1: expected expression, found end of input"),
            (
                "xs[1:2] = 3",
                "line 1: expected end of statement, found `=`",
            ),
            ("[1, 2][0] = 3", "line 1: expected statement, found `[`"),
            (
                "xs[0] = 1 2",
                "line 1: expected end of statement, found integer `2`",
            ),
            (
                "x = \"abc",
                "line 1: expected expression, found illegal token",
            ),
            (
                "x = 1 \"a\"",
                "line 1: expected end of statement, found string \"a\"",
            ),
            (
                "x = true false",
                "line 1: expected end of statement, found `false`",
            ),
            (
                "while x < 3:\nx = 1 end",
                "line 2: expected end of statement, found `end`",
            ),
            ("def", "line 1: expected function name, found end of input"),
            (
                "def 5():\nend",
                "line 1: expected function name, found integer `5`",
            ),
            ("def f:\nend", "line 1: expected `(`, found `:`"),
            (
                "def f(a,):\nend",
                "line 1: expected parameter name, found `)`",
            ),
            (
                "def f(1):\nend",
                "line 1: expected parameter name, found integer `1`",
            ),
            (
                "def f(a b):\nend",
                "line 1: expected `)`, found identifier `b`",
            ),
            ("def f(a)\nend", "line 1: expected `:`, found newline"),
            (
                "def f(a): return a\nend",
                "line 1: expected newline, found `return`",
            ),
            (
                "def f():\nreturn 1",
                "line 2: expected `end`, found end of input",
            ),
            ("x = f(1,)", "line 1: expected expression, found `)`"),
            ("x = f(1 2)", "line 1: expected `)`, found integer `2`"),
            ("f(1", "line 1: expected `)`, found end of input"),
            ("f(1) = 2", "line 1: expected end of statement, found `=`"),
            (
                "return 1 2",
                "line 1: expected end of statement, found integer `2`",
            ),
        ];
        for (source, message) in cases {
            assert_eq!(
                parse(source).unwrap_err().to_string(),
                message,
                "{source:?}"
            );
        }
    }

    fn if_statement(
        condition: Expression,
        then_body: Vec<Statement>,
        else_body: Vec<Statement>,
    ) -> Statement {
        Statement::If {
            condition,
            then_body,
            else_body,
        }
    }

    #[test]
    fn parses_if_with_and_without_else() {
        assert_eq!(
            parse("if x > 1:\nprint(x)\nend\n").unwrap(),
            [if_statement(
                binary(BinaryOp::Greater, ident("x"), Expression::Int(1)),
                vec![Statement::Print(ident("x"))],
                vec![],
            )]
        );
        assert_eq!(
            parse("if x == 1:\nprint(1)\nelse:\nprint(2)\n\nprint(3)\nend\nprint(4)").unwrap(),
            [
                if_statement(
                    binary(BinaryOp::Equal, ident("x"), Expression::Int(1)),
                    vec![Statement::Print(Expression::Int(1))],
                    vec![
                        Statement::Print(Expression::Int(2)),
                        Statement::Print(Expression::Int(3)),
                    ],
                ),
                Statement::Print(Expression::Int(4)),
            ]
        );
    }

    #[test]
    fn an_else_belongs_to_the_nearest_open_if() {
        assert_eq!(
            parse("if a == 1:\nif b == 2:\nx = 1\nelse:\nx = 2\nend\nelse:\nx = 3\nend").unwrap(),
            [if_statement(
                binary(BinaryOp::Equal, ident("a"), Expression::Int(1)),
                vec![if_statement(
                    binary(BinaryOp::Equal, ident("b"), Expression::Int(2)),
                    vec![assign("x", Expression::Int(1))],
                    vec![assign("x", Expression::Int(2))],
                )],
                vec![assign("x", Expression::Int(3))],
            )]
        );
    }

    #[test]
    fn ifs_nest_inside_loops_and_functions() {
        assert_eq!(
            parse(
                "while a < 3:\nif a == 1:\nprint(a)\nend\nend\ndef f():\nif true:\nreturn\nend\nend"
            )
            .unwrap(),
            [
                Statement::While {
                    condition: binary(BinaryOp::Less, ident("a"), Expression::Int(3)),
                    body: vec![if_statement(
                        binary(BinaryOp::Equal, ident("a"), Expression::Int(1)),
                        vec![Statement::Print(ident("a"))],
                        vec![],
                    )],
                },
                Statement::FunctionDef {
                    name: "f".to_owned(),
                    params: vec![],
                    body: [if_statement(
                        Expression::Bool(true),
                        vec![Statement::Return(None)],
                        vec![],
                    )]
                    .into(),
                },
            ]
        );
    }

    #[test]
    fn the_new_comparisons_share_the_precedence_of_their_siblings() {
        assert_eq!(
            parse("x = a + 1 <= b * 2 != c >= d").unwrap(),
            [assign(
                "x",
                binary(
                    BinaryOp::NotEqual,
                    binary(
                        BinaryOp::LessEqual,
                        binary(BinaryOp::Add, ident("a"), Expression::Int(1)),
                        binary(BinaryOp::Multiply, ident("b"), Expression::Int(2)),
                    ),
                    binary(BinaryOp::GreaterEqual, ident("c"), ident("d")),
                )
            )]
        );
    }

    #[test]
    fn postfix_chains_parse_as_assignment_targets_and_expression_statements() {
        let method = |target: &str, name: &str| Expression::MethodCall {
            target: Box::new(ident(target)),
            method: name.to_owned(),
            arguments: vec![],
        };
        assert_eq!(
            parse("a.b()[2] = 3").unwrap(),
            [Statement::IndexAssign {
                target: method("a", "b"),
                index: Expression::Int(2),
                value: Expression::Int(3),
            }]
        );
        let slice = Expression::Slice {
            target: Box::new(ident("xs")),
            start: Some(Box::new(Expression::Int(1))),
            end: Some(Box::new(Expression::Int(2))),
        };
        assert_eq!(
            parse("xs[1:2][0] = 5").unwrap(),
            [Statement::IndexAssign {
                target: slice,
                index: Expression::Int(0),
                value: Expression::Int(5),
            }]
        );
        assert_eq!(
            parse("xs.pop()[0]").unwrap(),
            [Statement::Expression(index(
                method("xs", "pop"),
                Expression::Int(0)
            ))]
        );
        assert_eq!(
            parse("xs[1:3].len()").unwrap(),
            [Statement::Expression(Expression::MethodCall {
                target: Box::new(Expression::Slice {
                    target: Box::new(ident("xs")),
                    start: Some(Box::new(Expression::Int(1))),
                    end: Some(Box::new(Expression::Int(3))),
                }),
                method: "len".to_owned(),
                arguments: vec![],
            })]
        );
    }

    #[test]
    fn the_lowest_precedence_operator_is_accepted_in_every_nested_position() {
        let sources = [
            "x = f(a == b)",
            "xs.push(a == b)",
            "x = [a == b]",
            "x = (a == b) + 1",
            "x = xs[a == b]",
            "x = xs[a == b : c == d]",
            "def f():\nreturn a == b\nend",
            "xs[0] = a == b",
            "print(a != b)",
        ];
        for source in sources {
            assert!(parse(source).is_ok(), "{source:?}");
        }
    }

    #[test]
    fn bare_index_statements_and_returns_parse_and_slice_bounds_are_full_expressions() {
        assert_eq!(
            parse("xs[0]\nxs[-1]").unwrap(),
            [
                Statement::Expression(index(ident("xs"), Expression::Int(0))),
                Statement::Expression(index(
                    ident("xs"),
                    Expression::Negate(Box::new(Expression::Int(1)))
                )),
            ]
        );
        assert_eq!(parse("return").unwrap(), [Statement::Return(None)]);
        assert_eq!(
            parse("a = xs[i + 1:n * 2 < m]").unwrap(),
            [assign(
                "a",
                Expression::Slice {
                    target: Box::new(ident("xs")),
                    start: Some(Box::new(binary(
                        BinaryOp::Add,
                        ident("i"),
                        Expression::Int(1)
                    ))),
                    end: Some(Box::new(binary(
                        BinaryOp::Less,
                        binary(BinaryOp::Multiply, ident("n"), Expression::Int(2)),
                        ident("m"),
                    ))),
                }
            )]
        );
    }

    #[test]
    fn multiplication_and_division_share_one_left_associative_level() {
        assert_eq!(
            parse("x = 8 * 2 / 4").unwrap(),
            [assign(
                "x",
                binary(
                    BinaryOp::Divide,
                    binary(BinaryOp::Multiply, Expression::Int(8), Expression::Int(2)),
                    Expression::Int(4),
                )
            )]
        );
        assert_eq!(
            parse("x = 8 / 2 * 4").unwrap(),
            [assign(
                "x",
                binary(
                    BinaryOp::Multiply,
                    binary(BinaryOp::Divide, Expression::Int(8), Expression::Int(2)),
                    Expression::Int(4),
                )
            )]
        );
    }

    #[test]
    fn every_token_kind_is_named_in_error_messages() {
        let statement_starts = [
            ("+ 1", "`+`"),
            ("- 1", "`-`"),
            ("* 1", "`*`"),
            ("/ 1", "`/`"),
            ("== 1", "`==`"),
            ("!= 1", "`!=`"),
            ("> 1", "`>`"),
            (">= 1", "`>=`"),
            ("< 1", "`<`"),
            ("<= 1", "`<=`"),
            ("else", "`else`"),
            ("end", "`end`"),
            ("true", "`true`"),
            ("false", "`false`"),
            (", 1", "`,`"),
            (". 1", "`.`"),
            (": 1", "`:`"),
            ("( 1", "`(`"),
            (") 1", "`)`"),
            ("[ 1", "`[`"),
            ("] 1", "`]`"),
            ("5", "integer `5`"),
            ("\"s\"", "string \"s\""),
            ("@", "illegal token"),
        ];
        for (source, found) in statement_starts {
            assert_eq!(
                parse(source).unwrap_err().to_string(),
                format!("line 1: expected statement, found {found}"),
                "{source:?}"
            );
        }
        for keyword in ["while", "def", "print", "else", "if", "return"] {
            assert_eq!(
                parse(&format!("x = {keyword}")).unwrap_err().to_string(),
                format!("line 1: expected expression, found `{keyword}`"),
                "{keyword}"
            );
        }
    }

    #[test]
    fn carriage_returns_do_not_count_as_lines() {
        assert_eq!(
            parse("x = 1\r\n\r\ny = )").unwrap_err().to_string(),
            "line 3: expected expression, found `)`"
        );
    }
}
