use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::io::{self, Write};
use std::rc::Rc;

use crate::ast::{BinaryOp, Expression, Statement};

pub const MAX_CALL_DEPTH: usize = 1000;

pub type Scope = Rc<RefCell<Environment>>;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Bool(bool),
    Unit,
    Function(Rc<Function>),
}

pub struct Function {
    name: String,
    params: Vec<String>,
    body: Vec<Statement>,
    closure: Scope,
}

impl PartialEq for Function {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl fmt::Debug for Function {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Function")
            .field("name", &self.name)
            .field("params", &self.params)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(value) => write!(f, "{value}"),
            Value::Bool(value) => write!(f, "{value}"),
            Value::Unit => f.write_str("unit"),
            Value::Function(function) => write!(f, "<function {}>", function.name),
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
    NotCallable(String),
    ArityMismatch {
        name: String,
        expected: usize,
        found: usize,
    },
    CallDepthExceeded,
    ReturnOutsideFunction,
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
            RuntimeError::NotCallable(name) => write!(f, "`{name}` is not a function"),
            RuntimeError::ArityMismatch {
                name,
                expected,
                found,
            } => write!(
                f,
                "wrong number of arguments to `{name}`: expected {expected}, found {found}"
            ),
            RuntimeError::CallDepthExceeded => {
                write!(f, "call depth exceeded (limit {MAX_CALL_DEPTH})")
            }
            RuntimeError::ReturnOutsideFunction => f.write_str("`return` outside of a function"),
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

#[derive(Debug)]
pub struct Environment {
    variables: HashMap<String, Value>,
    parent: Option<Scope>,
    call_depth: usize,
}

impl Environment {
    pub fn global() -> Scope {
        Rc::new(RefCell::new(Self {
            variables: HashMap::new(),
            parent: None,
            call_depth: 0,
        }))
    }

    fn enclosed(parent: &Scope, call_depth: usize) -> Scope {
        Rc::new(RefCell::new(Self {
            variables: HashMap::new(),
            parent: Some(Rc::clone(parent)),
            call_depth,
        }))
    }

    fn get(&self, name: &str) -> Option<Value> {
        match self.variables.get(name) {
            Some(value) => Some(value.clone()),
            None => self.parent.as_ref()?.borrow().get(name),
        }
    }

    fn define(&mut self, name: &str, value: Value) {
        self.variables.insert(name.to_owned(), value);
    }

    fn assign(&mut self, name: &str, value: Value) {
        if let Err(value) = self.replace_existing(name, value) {
            self.define(name, value);
        }
    }

    fn replace_existing(&mut self, name: &str, value: Value) -> Result<(), Value> {
        if let Some(slot) = self.variables.get_mut(name) {
            *slot = value;
            return Ok(());
        }
        match &self.parent {
            Some(parent) => parent.borrow_mut().replace_existing(name, value),
            None => Err(value),
        }
    }
}

enum Flow {
    Next,
    Return(Value),
}

pub fn execute(
    scope: &Scope,
    statements: &[Statement],
    out: &mut impl Write,
) -> Result<(), RuntimeError> {
    match run_block(scope, statements, out)? {
        Flow::Next => Ok(()),
        Flow::Return(_) => Err(RuntimeError::ReturnOutsideFunction),
    }
}

fn run_block(
    scope: &Scope,
    statements: &[Statement],
    out: &mut impl Write,
) -> Result<Flow, RuntimeError> {
    for statement in statements {
        if let Flow::Return(value) = run_statement(scope, statement, out)? {
            return Ok(Flow::Return(value));
        }
    }
    Ok(Flow::Next)
}

fn run_statement(
    scope: &Scope,
    statement: &Statement,
    out: &mut impl Write,
) -> Result<Flow, RuntimeError> {
    match statement {
        Statement::Assign { name, value } => {
            let value = eval(scope, value, out)?;
            scope.borrow_mut().assign(name, value);
        }
        Statement::Print(expression) => {
            let value = eval(scope, expression, out)?;
            writeln!(out, "{value}")?;
        }
        Statement::While { condition, body } => {
            while condition_holds(scope, condition, out)? {
                if let Flow::Return(value) = run_block(scope, body, out)? {
                    return Ok(Flow::Return(value));
                }
            }
        }
        Statement::FunctionDef { name, params, body } => {
            let function = Function {
                name: name.clone(),
                params: params.clone(),
                body: body.clone(),
                closure: Rc::clone(scope),
            };
            scope
                .borrow_mut()
                .define(name, Value::Function(Rc::new(function)));
        }
        Statement::Return(expression) => {
            let value = match expression {
                Some(expression) => eval(scope, expression, out)?,
                None => Value::Unit,
            };
            return Ok(Flow::Return(value));
        }
        Statement::Expression(expression) => {
            eval(scope, expression, out)?;
        }
    }
    Ok(Flow::Next)
}

fn eval(
    scope: &Scope,
    expression: &Expression,
    out: &mut impl Write,
) -> Result<Value, RuntimeError> {
    match expression {
        Expression::Int(value) => Ok(Value::Int(*value)),
        Expression::Ident(name) => scope
            .borrow()
            .get(name)
            .ok_or_else(|| RuntimeError::UndefinedVariable(name.clone())),
        Expression::Binary { op, left, right } => {
            let left = eval(scope, left, out)?;
            let right = eval(scope, right, out)?;
            apply(*op, left, right)
        }
        Expression::Call { name, arguments } => call(scope, name, arguments, out),
    }
}

fn call(
    scope: &Scope,
    name: &str,
    arguments: &[Expression],
    out: &mut impl Write,
) -> Result<Value, RuntimeError> {
    let callee = scope
        .borrow()
        .get(name)
        .ok_or_else(|| RuntimeError::UndefinedVariable(name.to_owned()))?;
    let Value::Function(function) = callee else {
        return Err(RuntimeError::NotCallable(name.to_owned()));
    };
    if arguments.len() != function.params.len() {
        return Err(RuntimeError::ArityMismatch {
            name: name.to_owned(),
            expected: function.params.len(),
            found: arguments.len(),
        });
    }
    let call_depth = scope.borrow().call_depth + 1;
    if call_depth > MAX_CALL_DEPTH {
        return Err(RuntimeError::CallDepthExceeded);
    }

    let local = Environment::enclosed(&function.closure, call_depth);
    for (param, argument) in function.params.iter().zip(arguments) {
        let value = eval(scope, argument, out)?;
        local.borrow_mut().define(param, value);
    }
    match run_block(&local, &function.body, out)? {
        Flow::Return(value) => Ok(value),
        Flow::Next => Ok(Value::Unit),
    }
}

fn condition_holds(
    scope: &Scope,
    condition: &Expression,
    out: &mut impl Write,
) -> Result<bool, RuntimeError> {
    match eval(scope, condition, out)? {
        Value::Bool(holds) => Ok(holds),
        other => Err(RuntimeError::NonBooleanCondition(other)),
    }
}

fn apply(op: BinaryOp, left: Value, right: Value) -> Result<Value, RuntimeError> {
    match (&left, &right) {
        (Value::Int(left), Value::Int(right)) => apply_int(op, *left, *right),
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
    use std::thread;

    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn run(scope: &Scope, source: &str) -> Result<String, Box<dyn Error>> {
        let statements = Parser::new(Lexer::new(source)).parse_program()?;
        let mut output = Vec::new();
        execute(scope, &statements, &mut output)?;
        Ok(String::from_utf8(output)?)
    }

    fn output_of(source: &str) -> String {
        run(&Environment::global(), source).unwrap()
    }

    fn error_of(source: &str) -> String {
        run(&Environment::global(), source).unwrap_err().to_string()
    }

    #[test]
    fn while_loop_counts_up_to_the_bound() {
        let globals = Environment::global();
        run(
            &globals,
            "x = 5 \n y = 10 \n while x < y: \n x = x + 1 \n end",
        )
        .unwrap();
        assert_eq!(globals.borrow().get("x"), Some(Value::Int(10)));
        assert_eq!(globals.borrow().get("y"), Some(Value::Int(10)));
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
            output_of(
                "print(5 + 2 * 3)\nprint((5 + 2) * 3)\nprint(7 / 2)\nprint(10 - 4 - 3)\n\
                 print((0 - 7) / 2)\nprint(7 / (0 - 2))\nprint((0 - 7) / (0 - 2))"
            ),
            "11\n21\n3\n3\n-3\n-3\n3\n"
        );
    }

    #[test]
    fn comparisons_produce_booleans() {
        assert_eq!(
            output_of(
                "print(1 < 2)\nprint(2 < 1)\nprint(2 > 1)\nprint(2 > 2)\nprint(2 < 2)\nprint(3 == 3)\nb = 1 < 2\nprint(b == (2 > 1))"
            ),
            "true\nfalse\ntrue\nfalse\nfalse\ntrue\ntrue\n"
        );
    }

    #[test]
    fn reassignment_overwrites_the_previous_value() {
        assert_eq!(output_of("x = 1\nx = x + 1\nx = x * 10\nprint(x)"), "20\n");
    }

    #[test]
    fn global_scope_persists_between_runs() {
        let globals = Environment::global();
        run(&globals, "x = 6\ndef square(n):\nreturn n * n\nend").unwrap();
        assert_eq!(run(&globals, "print(square(x))").unwrap(), "36\n");
    }

    #[test]
    fn calls_a_function_with_arguments() {
        assert_eq!(
            output_of("def add(a, b):\nreturn a + b\nend\nprint(add(10, 15))"),
            "25\n"
        );
        assert_eq!(
            output_of("def add(a, b):\nreturn a + b\nend\nprint(add(add(1, 2), add(3, 4)) * 2)"),
            "20\n"
        );
    }

    #[test]
    fn locals_do_not_leak_out_of_a_call() {
        let script = "def f():\ny = 1\nreturn y\nend\nprint(f())\nprint(y)";
        assert_eq!(error_of(script), "undefined variable `y`");
    }

    #[test]
    fn parameters_shadow_globals_without_changing_them() {
        let script = "x = 1\ndef f(x):\nx = x + 1\nreturn x\nend\nprint(f(10))\nprint(x)";
        assert_eq!(output_of(script), "11\n1\n");
    }

    #[test]
    fn functions_read_enclosing_variables_through_the_scope_chain() {
        let script = "base = 100\ndef add_base(n):\nreturn base + n\nend\nprint(add_base(5))";
        assert_eq!(output_of(script), "105\n");
    }

    #[test]
    fn assignment_updates_the_nearest_enclosing_variable() {
        let script = "count = 0\ndef bump():\ncount = count + 1\nend\nbump()\nbump()\nprint(count)";
        assert_eq!(output_of(script), "2\n");
    }

    #[test]
    fn scoping_is_lexical_rather_than_dynamic() {
        let script = "x = 1\ndef get_x():\nreturn x\nend\n\
                      def caller(x):\nreturn get_x()\nend\nprint(caller(99))";
        assert_eq!(output_of(script), "1\n");
    }

    #[test]
    fn closures_keep_independent_mutable_state() {
        let script = "def make_counter():\ncount = 0\n\
                      def next():\ncount = count + 1\nreturn count\nend\n\
                      return next\nend\n\
                      a = make_counter()\nb = make_counter()\n\
                      print(a())\nprint(a())\nprint(b())";
        assert_eq!(output_of(script), "1\n2\n1\n");
    }

    #[test]
    fn nested_function_definitions_stay_local() {
        let globals = Environment::global();
        let script =
            "def outer():\ndef inner():\nreturn 5\nend\nreturn inner()\nend\nprint(outer())";
        assert_eq!(run(&globals, script).unwrap(), "5\n");
        assert_eq!(
            run(&globals, "inner()").unwrap_err().to_string(),
            "undefined variable `inner`"
        );
    }

    #[test]
    fn recursion_terminates_through_return_inside_a_loop() {
        let script = "def fib(n):\nwhile n < 2:\nreturn n\nend\n\
                      return fib(n - 1) + fib(n - 2)\nend\nprint(fib(10))";
        assert_eq!(output_of(script), "55\n");
    }

    #[test]
    fn return_unwinds_through_nested_loops() {
        let script = "def find(n):\ni = 0\nwhile i < 100:\ni = i + 1\n\
                      while i == n:\nreturn i * 10\nend\nend\nreturn 0\nend\n\
                      print(find(3))\nprint(find(500))";
        assert_eq!(output_of(script), "30\n0\n");
    }

    #[test]
    fn bare_return_and_falling_off_the_end_yield_unit() {
        let script = "def a():\nreturn\nend\ndef b():\nx = 1\nend\nprint(a())\nprint(b())";
        assert_eq!(output_of(script), "unit\nunit\n");
    }

    #[test]
    fn call_statements_run_for_their_side_effects() {
        assert_eq!(
            output_of("def hello():\nprint(7)\nend\nhello()\nhello()"),
            "7\n7\n"
        );
    }

    #[test]
    fn arguments_evaluate_left_to_right_in_the_callers_scope() {
        let script = "def show(n):\nprint(n)\nreturn n\nend\n\
                      def add(a, b):\nreturn a + b\nend\nprint(add(show(1), show(2)))";
        assert_eq!(output_of(script), "1\n2\n3\n");
    }

    #[test]
    fn functions_are_first_class_values() {
        let script =
            "def add(a, b):\nreturn a + b\nend\nplus = add\nprint(plus(2, 3))\nprint(plus)";
        assert_eq!(output_of(script), "5\n<function add>\n");
    }

    #[test]
    fn redefining_a_function_replaces_it() {
        let script = "def f():\nreturn 1\nend\ndef f():\nreturn 2\nend\nprint(f())";
        assert_eq!(output_of(script), "2\n");
    }

    #[test]
    fn debug_output_of_a_function_does_not_recurse_into_its_scope() {
        let globals = Environment::global();
        run(&globals, "def f():\nend").unwrap();
        assert!(format!("{globals:?}").contains("Function"));
    }

    #[test]
    fn runaway_recursion_reports_depth_instead_of_crashing() {
        let message = thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(|| error_of("def f(n):\nreturn f(n + 1)\nend\nf(0)"))
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(
            message,
            format!("call depth exceeded (limit {MAX_CALL_DEPTH})")
        );
    }

    #[test]
    fn reports_runtime_errors() {
        let cases = [
            ("print(y)", "undefined variable `y`"),
            ("x = x + 1", "undefined variable `x`"),
            ("print(1 / 0)", "division by zero"),
            ("print(0 / 0)", "division by zero"),
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
            ("nope(1)", "undefined variable `nope`"),
            ("x = 1\nx(2)", "`x` is not a function"),
            ("return 1", "`return` outside of a function"),
            (
                "def f(a):\nreturn a\nend\nf()",
                "wrong number of arguments to `f`: expected 1, found 0",
            ),
            (
                "def f():\nreturn 1\nend\nf(1, 2)",
                "wrong number of arguments to `f`: expected 0, found 2",
            ),
            (
                "def f():\nreturn 1\nend\nprint(f + 1)",
                "cannot apply `+` to <function f> and 1",
            ),
            (
                "def f():\nreturn\nend\nprint(f() + 1)",
                "cannot apply `+` to unit and 1",
            ),
            (
                "def f():\nreturn 1\nend\nprint(f == f)",
                "cannot apply `==` to <function f> and <function f>",
            ),
        ];
        for (source, message) in cases {
            assert_eq!(error_of(source), message, "{source:?}");
        }
    }

    #[test]
    fn output_before_a_runtime_error_is_kept() {
        let statements = Parser::new(Lexer::new("print(1)\nprint(2 / 0)"))
            .parse_program()
            .unwrap();
        let mut output = Vec::new();
        let result = execute(&Environment::global(), &statements, &mut output);
        assert!(matches!(result, Err(RuntimeError::DivisionByZero)));
        assert_eq!(output, b"1\n");
    }

    #[test]
    fn failed_assignment_leaves_the_variable_unchanged() {
        let globals = Environment::global();
        let script = "def f(a):\nreturn a / 0\nend\nx = 1\nx = f(5)";
        assert!(run(&globals, script).is_err());
        assert_eq!(globals.borrow().get("x"), Some(Value::Int(1)));
    }
}
