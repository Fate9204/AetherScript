use std::collections::HashMap;
use std::fmt;
use std::io::{self, Write};

use crate::ast::{BinaryOp, Expression, Statement};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Bool(bool),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(value) => write!(f, "{value}"),
            Value::Bool(value) => write!(f, "{value}"),
        }
    }
}

#[derive(Debug)]
pub enum RuntimeError {
    UndefinedVariable(String),
    TypeMismatch {
        op: BinaryOp,
        left: Value,
        right: Value,
    },
    NonBooleanCondition(Value),
    DivisionByZero,
    Overflow,
    Output(io::Error),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeError::UndefinedVariable(name) => write!(f, "undefined variable `{name}`"),
            RuntimeError::TypeMismatch { op, left, right } => {
                write!(f, "cannot apply `{op}` to {left} and {right}")
            }
            RuntimeError::NonBooleanCondition(value) => {
                write!(f, "while condition must be a boolean, found {value}")
            }
            RuntimeError::DivisionByZero => f.write_str("division by zero"),
            RuntimeError::Overflow => f.write_str("integer overflow"),
            RuntimeError::Output(error) => write!(f, "output error: {error}"),
        }
    }
}

impl std::error::Error for RuntimeError {}

impl From<io::Error> for RuntimeError {
    fn from(error: io::Error) -> Self {
        RuntimeError::Output(error)
    }
}

#[derive(Debug, Default)]
pub struct Environment {
    variables: HashMap<String, Value>,
}

impl Environment {
    pub fn execute(
        &mut self,
        statements: &[Statement<'_>],
        out: &mut impl Write,
    ) -> Result<(), RuntimeError> {
        for statement in statements {
            self.execute_statement(statement, out)?;
        }
        Ok(())
    }

    fn execute_statement(
        &mut self,
        statement: &Statement<'_>,
        out: &mut impl Write,
    ) -> Result<(), RuntimeError> {
        match statement {
            Statement::Assign { name, value } => {
                let value = self.eval(value)?;
                self.assign(name, value);
            }
            Statement::Print(expression) => writeln!(out, "{}", self.eval(expression)?)?,
            Statement::While { condition, body } => {
                while self.condition_holds(condition)? {
                    self.execute(body, out)?;
                }
            }
        }
        Ok(())
    }

    fn eval(&self, expression: &Expression<'_>) -> Result<Value, RuntimeError> {
        match expression {
            Expression::Int(value) => Ok(Value::Int(*value)),
            Expression::Ident(name) => self
                .variables
                .get(*name)
                .copied()
                .ok_or_else(|| RuntimeError::UndefinedVariable((*name).to_owned())),
            Expression::Binary { op, left, right } => {
                apply(*op, self.eval(left)?, self.eval(right)?)
            }
        }
    }

    fn condition_holds(&self, condition: &Expression<'_>) -> Result<bool, RuntimeError> {
        match self.eval(condition)? {
            Value::Bool(holds) => Ok(holds),
            other => Err(RuntimeError::NonBooleanCondition(other)),
        }
    }

    fn assign(&mut self, name: &str, value: Value) {
        // reassignment must not allocate a new key
        match self.variables.get_mut(name) {
            Some(slot) => *slot = value,
            None => {
                self.variables.insert(name.to_owned(), value);
            }
        }
    }
}

fn apply(op: BinaryOp, left: Value, right: Value) -> Result<Value, RuntimeError> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => apply_int(op, left, right),
        (Value::Bool(left), Value::Bool(right)) if op == BinaryOp::Equal => {
            Ok(Value::Bool(left == right))
        }
        _ => Err(RuntimeError::TypeMismatch { op, left, right }),
    }
}

fn apply_int(op: BinaryOp, left: i64, right: i64) -> Result<Value, RuntimeError> {
    let arithmetic = match op {
        BinaryOp::Add => left.checked_add(right),
        BinaryOp::Subtract => left.checked_sub(right),
        BinaryOp::Multiply => left.checked_mul(right),
        BinaryOp::Divide if right == 0 => return Err(RuntimeError::DivisionByZero),
        BinaryOp::Divide => left.checked_div(right),
        BinaryOp::Equal => return Ok(Value::Bool(left == right)),
        BinaryOp::Greater => return Ok(Value::Bool(left > right)),
        BinaryOp::Less => return Ok(Value::Bool(left < right)),
    };
    arithmetic.map(Value::Int).ok_or(RuntimeError::Overflow)
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn run(environment: &mut Environment, source: &str) -> Result<String, Box<dyn Error>> {
        let statements = Parser::new(Lexer::new(source)).parse_program()?;
        let mut output = Vec::new();
        environment.execute(&statements, &mut output)?;
        Ok(String::from_utf8(output)?)
    }

    fn output_of(source: &str) -> String {
        run(&mut Environment::default(), source).unwrap()
    }

    #[test]
    fn while_loop_counts_up_to_the_bound() {
        let mut environment = Environment::default();
        run(
            &mut environment,
            "x = 5 \n y = 10 \n while x < y: \n x = x + 1 \n end",
        )
        .unwrap();
        assert_eq!(environment.variables["x"], Value::Int(10));
        assert_eq!(environment.variables["y"], Value::Int(10));
    }

    #[test]
    fn while_loop_runs_every_body_statement_each_iteration() {
        assert_eq!(
            output_of("x = 1\nwhile x < 4:\nprint(x)\nx = x + 1\nend\nprint(x)"),
            "1\n2\n3\n4\n"
        );
    }

    #[test]
    fn while_loop_with_false_condition_never_runs() {
        assert_eq!(
            output_of("x = 5\nwhile x < 3:\nx = 0\nend\nprint(x)"),
            "5\n"
        );
    }

    #[test]
    fn nested_loops_run_to_completion() {
        let script = "i = 0\ntotal = 0\n\
                      while i < 3:\nj = 0\n\
                      while j < 3:\ntotal = total + 1\nj = j + 1\nend\n\
                      i = i + 1\nend\nprint(total)";
        assert_eq!(output_of(script), "9\n");
    }

    #[test]
    fn arithmetic_respects_precedence_and_truncates_division() {
        assert_eq!(
            output_of("print(5 + 2 * 3)\nprint((5 + 2) * 3)\nprint(7 / 2)\nprint(10 - 4 - 3)"),
            "11\n21\n3\n3\n"
        );
    }

    #[test]
    fn comparisons_produce_booleans() {
        assert_eq!(
            output_of(
                "print(1 < 2)\nprint(2 < 1)\nprint(2 > 1)\nprint(3 == 3)\nb = 1 < 2\nprint(b == (2 > 1))"
            ),
            "true\nfalse\ntrue\ntrue\ntrue\n"
        );
    }

    #[test]
    fn reassignment_overwrites_the_previous_value() {
        assert_eq!(output_of("x = 1\nx = x + 1\nx = x * 10\nprint(x)"), "20\n");
    }

    #[test]
    fn environment_persists_between_runs() {
        let mut environment = Environment::default();
        run(&mut environment, "x = 6").unwrap();
        assert_eq!(run(&mut environment, "print(x * x)").unwrap(), "36\n");
    }

    #[test]
    fn reports_runtime_errors() {
        let cases = [
            ("print(y)", "undefined variable `y`"),
            ("x = x + 1", "undefined variable `x`"),
            ("print(1 / 0)", "division by zero"),
            ("print(1 + (2 < 3))", "cannot apply `+` to 1 and true"),
            ("print(1 == (1 < 2))", "cannot apply `==` to 1 and true"),
            (
                "print((1 < 2) > (2 < 3))",
                "cannot apply `>` to true and true",
            ),
            (
                "while 1:\nend",
                "while condition must be a boolean, found 1",
            ),
            ("print(9223372036854775807 + 1)", "integer overflow"),
            ("print(0 - 9223372036854775807 - 2)", "integer overflow"),
            ("print(4611686018427387904 * 2)", "integer overflow"),
            (
                "print((0 - 9223372036854775807 - 1) / (0 - 1))",
                "integer overflow",
            ),
            (
                "print(1 +",
                "line 1: expected expression, found end of input",
            ),
        ];
        for (source, message) in cases {
            let error = run(&mut Environment::default(), source).unwrap_err();
            assert_eq!(error.to_string(), message, "{source:?}");
        }
    }

    #[test]
    fn output_before_a_runtime_error_is_kept() {
        let statements = Parser::new(Lexer::new("print(1)\nprint(2 / 0)"))
            .parse_program()
            .unwrap();
        let mut output = Vec::new();
        let result = Environment::default().execute(&statements, &mut output);
        assert!(matches!(result, Err(RuntimeError::DivisionByZero)));
        assert_eq!(output, b"1\n");
    }
}
