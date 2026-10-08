use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::io::{self, Write};
use std::ops::Range;
use std::rc::Rc;

use crate::ast::{BinaryOp, Expression, Statement};

pub const MAX_CALL_DEPTH: usize = 1000;

pub type Scope = Rc<RefCell<Environment>>;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Bool(bool),
    String(String),
    Array(Rc<RefCell<Vec<Value>>>),
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

impl Value {
    fn new_array(items: Vec<Value>) -> Self {
        Value::Array(Rc::new(RefCell::new(items)))
    }

    fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "integer",
            Value::Bool(_) => "boolean",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Unit => "unit",
            Value::Function(_) => "function",
        }
    }

    fn write_to(
        &self,
        f: &mut fmt::Formatter<'_>,
        quoted: bool,
        open: &mut Vec<*const RefCell<Vec<Value>>>,
    ) -> fmt::Result {
        match self {
            Value::Int(value) => write!(f, "{value}"),
            Value::Bool(value) => write!(f, "{value}"),
            Value::String(text) if quoted => write!(f, "\"{text}\""),
            Value::String(text) => f.write_str(text),
            Value::Unit => f.write_str("unit"),
            Value::Function(function) => write!(f, "<function {}>", function.name),
            Value::Array(items) => {
                let identity = Rc::as_ptr(items);
                if open.contains(&identity) {
                    return f.write_str("[...]");
                }
                open.push(identity);
                f.write_str("[")?;
                for (position, item) in items.borrow().iter().enumerate() {
                    if position > 0 {
                        f.write_str(", ")?;
                    }
                    item.write_to(f, true, open)?;
                }
                open.pop();
                f.write_str("]")
            }
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_to(f, false, &mut Vec::new())
    }
}

#[derive(Debug)]
pub enum RuntimeError {
    UndefinedVariable(String),
    TypeMismatch {
        op: BinaryOp,
        left: &'static str,
        right: &'static str,
    },
    NonBooleanCondition(&'static str),
    NotIndexable(&'static str),
    NonIntegerIndex(&'static str),
    IndexOutOfRange {
        index: i64,
        length: usize,
    },
    ElementAssignment(&'static str),
    InvalidNegation(&'static str),
    UnknownMethod {
        receiver: &'static str,
        method: String,
    },
    PopFromEmpty,
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
            RuntimeError::NonBooleanCondition(found) => {
                write!(f, "while condition must be a boolean, found {found}")
            }
            RuntimeError::NotIndexable(found) => write!(f, "cannot index {found}"),
            RuntimeError::NonIntegerIndex(found) => {
                write!(f, "index must be an integer, found {found}")
            }
            RuntimeError::IndexOutOfRange { index, length } => {
                write!(f, "index {index} out of range for length {length}")
            }
            RuntimeError::ElementAssignment(found) => {
                write!(f, "cannot assign to an element of {found}")
            }
            RuntimeError::InvalidNegation(found) => write!(f, "cannot negate {found}"),
            RuntimeError::UnknownMethod { receiver, method } => {
                write!(f, "{receiver} has no method `{method}`")
            }
            RuntimeError::PopFromEmpty => f.write_str("pop from empty array"),
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
        Statement::IndexAssign {
            target,
            index,
            value,
        } => {
            let target = eval(scope, target, out)?;
            let index = eval(scope, index, out)?;
            let value = eval(scope, value, out)?;
            store_element(&target, &index, value)?;
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
        Expression::Bool(value) => Ok(Value::Bool(*value)),
        Expression::String(text) => Ok(Value::String(text.clone())),
        Expression::Array(items) => items
            .iter()
            .map(|item| eval(scope, item, out))
            .collect::<Result<_, _>>()
            .map(Value::new_array),
        Expression::Negate(operand) => match eval(scope, operand, out)? {
            Value::Int(value) => value
                .checked_neg()
                .map(Value::Int)
                .ok_or(RuntimeError::Overflow),
            other => Err(RuntimeError::InvalidNegation(other.type_name())),
        },
        Expression::Index { target, index } => {
            let target = eval(scope, target, out)?;
            let index = eval(scope, index, out)?;
            element(&target, &index)
        }
        Expression::Slice { target, start, end } => {
            let target = eval(scope, target, out)?;
            let start = start
                .as_ref()
                .map(|bound| eval(scope, bound, out))
                .transpose()?;
            let end = end
                .as_ref()
                .map(|bound| eval(scope, bound, out))
                .transpose()?;
            slice(&target, start.as_ref(), end.as_ref())
        }
        Expression::MethodCall {
            target,
            method,
            arguments,
        } => call_method(scope, target, method, arguments, out),
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
        other => Err(RuntimeError::NonBooleanCondition(other.type_name())),
    }
}

fn apply(op: BinaryOp, left: Value, right: Value) -> Result<Value, RuntimeError> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => apply_int(op, left, right),
        (Value::Bool(left), Value::Bool(right)) if op == BinaryOp::Equal => {
            Ok(Value::Bool(left == right))
        }
        (Value::String(left), Value::String(right)) if op == BinaryOp::Equal => {
            Ok(Value::Bool(left == right))
        }
        (Value::String(mut left), Value::String(right)) if op == BinaryOp::Add => {
            left.push_str(&right);
            Ok(Value::String(left))
        }
        (Value::String(left), Value::Int(right)) if op == BinaryOp::Add => {
            Ok(Value::String(format!("{left}{right}")))
        }
        (Value::Int(left), Value::String(right)) if op == BinaryOp::Add => {
            Ok(Value::String(format!("{left}{right}")))
        }
        (left, right) => Err(RuntimeError::TypeMismatch {
            op,
            left: left.type_name(),
            right: right.type_name(),
        }),
    }
}

fn call_method(
    scope: &Scope,
    target: &Expression,
    method: &str,
    arguments: &[Expression],
    out: &mut impl Write,
) -> Result<Value, RuntimeError> {
    let receiver = eval(scope, target, out)?;
    match (&receiver, method) {
        (Value::Array(items), "len") => {
            expect_arguments(method, arguments, 0)?;
            Ok(length_value(items.borrow().len()))
        }
        (Value::Array(items), "pop") => {
            expect_arguments(method, arguments, 0)?;
            items.borrow_mut().pop().ok_or(RuntimeError::PopFromEmpty)
        }
        (Value::Array(items), "push") => {
            expect_arguments(method, arguments, 1)?;
            let value = eval(scope, &arguments[0], out)?;
            items.borrow_mut().push(value);
            Ok(Value::Unit)
        }
        (Value::String(text), "len") => {
            expect_arguments(method, arguments, 0)?;
            Ok(length_value(text.chars().count()))
        }
        _ => Err(RuntimeError::UnknownMethod {
            receiver: receiver.type_name(),
            method: method.to_owned(),
        }),
    }
}

fn expect_arguments(
    method: &str,
    arguments: &[Expression],
    expected: usize,
) -> Result<(), RuntimeError> {
    if arguments.len() == expected {
        return Ok(());
    }
    Err(RuntimeError::ArityMismatch {
        name: method.to_owned(),
        expected,
        found: arguments.len(),
    })
}

fn length_value(length: usize) -> Value {
    Value::Int(i64::try_from(length).unwrap_or(i64::MAX))
}

fn element(value: &Value, index: &Value) -> Result<Value, RuntimeError> {
    match value {
        Value::Array(items) => {
            let items = items.borrow();
            let position = resolve_index(index, items.len())?;
            Ok(items[position].clone())
        }
        Value::String(text) => {
            let position = resolve_index(index, text.chars().count())?;
            Ok(Value::String(text.chars().skip(position).take(1).collect()))
        }
        other => Err(RuntimeError::NotIndexable(other.type_name())),
    }
}

fn store_element(target: &Value, index: &Value, value: Value) -> Result<(), RuntimeError> {
    let Value::Array(items) = target else {
        return Err(RuntimeError::ElementAssignment(target.type_name()));
    };
    let mut items = items.borrow_mut();
    let position = resolve_index(index, items.len())?;
    items[position] = value;
    Ok(())
}

fn slice(value: &Value, start: Option<&Value>, end: Option<&Value>) -> Result<Value, RuntimeError> {
    match value {
        Value::Array(items) => {
            let items = items.borrow();
            let window = slice_window(start, end, items.len())?;
            Ok(Value::new_array(items[window].to_vec()))
        }
        Value::String(text) => {
            let window = slice_window(start, end, text.chars().count())?;
            let window_length = window.end - window.start;
            let sliced = text.chars().skip(window.start).take(window_length);
            Ok(Value::String(sliced.collect()))
        }
        other => Err(RuntimeError::NotIndexable(other.type_name())),
    }
}

fn resolve_index(index: &Value, length: usize) -> Result<usize, RuntimeError> {
    let Value::Int(index) = *index else {
        return Err(RuntimeError::NonIntegerIndex(index.type_name()));
    };
    let position = match usize::try_from(index) {
        Ok(forward) => Some(forward),
        Err(_) => usize::try_from(index.unsigned_abs())
            .ok()
            .and_then(|back| length.checked_sub(back)),
    };
    position
        .filter(|&position| position < length)
        .ok_or(RuntimeError::IndexOutOfRange { index, length })
}

fn slice_window(
    start: Option<&Value>,
    end: Option<&Value>,
    length: usize,
) -> Result<Range<usize>, RuntimeError> {
    let start = clamp_bound(start, 0, length)?;
    let end = clamp_bound(end, length, length)?;
    Ok(start..end.max(start))
}

fn clamp_bound(
    bound: Option<&Value>,
    default: usize,
    length: usize,
) -> Result<usize, RuntimeError> {
    let Some(bound) = bound else {
        return Ok(default);
    };
    let Value::Int(bound) = *bound else {
        return Err(RuntimeError::NonIntegerIndex(bound.type_name()));
    };
    Ok(match usize::try_from(bound) {
        Ok(forward) => forward.min(length),
        Err(_) => {
            usize::try_from(bound.unsigned_abs()).map_or(0, |back| length.saturating_sub(back))
        }
    })
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
    fn strings_and_arrays_work_end_to_end() {
        let script = "arr = [10, 20, 30] \n msg = \"Value is \" \n print(msg + arr[1])";
        assert_eq!(output_of(script), "Value is 20\n");
    }

    #[test]
    fn array_literals_evaluate_their_elements_left_to_right() {
        let script = "def show(n):\nprint(n)\nreturn n\nend\n\
                      xs = [show(1), show(2) + 1, show(3)]\nprint(xs)";
        assert_eq!(output_of(script), "1\n2\n3\n[1, 3, 3]\n");
    }

    #[test]
    fn prints_nested_arrays_with_quoted_strings() {
        let script = "print([1, 2, \"three\", true])\nprint([])\n\
                      print([[1, 2], [\"a\"]])\nprint(\"plain\")\nprint(\"\")";
        assert_eq!(
            output_of(script),
            "[1, 2, \"three\", true]\n[]\n[[1, 2], [\"a\"]]\nplain\n\n"
        );
    }

    #[test]
    fn indexes_arrays_strings_and_nested_arrays() {
        let script = "xs = [10, 20, 30]\nprint(xs[0])\nprint(xs[1 + 1])\n\
                      m = [[1, 2], [3, 4]]\nprint(m[1][0])\nprint([5, 6][1])\n\
                      print(\"hello\"[1])\ns = \"h\u{e9}llo\"\nprint(s[1])";
        assert_eq!(output_of(script), "10\n30\n3\n6\ne\n\u{e9}\n");
    }

    #[test]
    fn index_expressions_may_call_functions_and_nest() {
        let script = "def one():\nreturn 1\nend\nxs = [7, 8, 9]\n\
                      print(xs[one()])\nprint(xs[xs[0] - 6])";
        assert_eq!(output_of(script), "8\n8\n");
    }

    #[test]
    fn assigns_into_arrays_in_place() {
        let script = "xs = [1, 2, 3]\nxs[0] = 10\nxs[1 + 1] = xs[0] + 5\nprint(xs)";
        assert_eq!(output_of(script), "[10, 2, 15]\n");
    }

    #[test]
    fn index_assignment_evaluates_target_then_index_then_value() {
        let script = "def show(n):\nprint(n)\nreturn n\nend\n\
                      def pick(xs):\nprint(100)\nreturn xs\nend\n\
                      xs = [1]\npick(xs)[show(0)] = show(7)\nprint(xs)";
        assert_eq!(output_of(script), "100\n0\n7\n[7]\n");
    }

    #[test]
    fn assigning_an_array_shares_the_same_heap_vector() {
        assert_eq!(
            output_of("a = [1, 2] \n b = a \n b[0] = 99 \n print(a[0])"),
            "99\n"
        );
    }

    #[test]
    fn functions_receive_and_return_arrays_by_reference() {
        let script = "def add_one(xs):\nxs.push(1)\nreturn xs\nend\n\
                      a = []\nb = add_one(a)\nadd_one(a)\nb[0] = 7\nprint(a)\nprint(b)";
        assert_eq!(output_of(script), "[7, 1]\n[7, 1]\n");
    }

    #[test]
    fn strings_stay_immutable_values() {
        let script = "s = \"ab\"\nt = s\nt = t + \"c\"\nprint(s)\nprint(t)";
        assert_eq!(output_of(script), "ab\nabc\n");
    }

    #[test]
    fn nested_index_assignment_mutates_the_base_vector_in_place() {
        let script = "m = [[1, 2], [3, 4]]\nm[0][1] = 99\nprint(m)\nm[1][-1] = 7\nprint(m[1])\n\
                      row = m[0]\nrow[0] = 5\nprint(m)\nt = [[[0]]]\nt[0][0][0] = 1\nprint(t)";
        assert_eq!(
            output_of(script),
            "[[1, 99], [3, 4]]\n[3, 7]\n[[5, 99], [3, 7]]\n[[[1]]]\n"
        );
    }

    #[test]
    fn negative_indexes_count_from_the_end() {
        let script = "xs = [10, 20, 30]\nprint(xs[-1])\nprint(xs[-3])\nxs[-1] = 0\nprint(xs)\n\
                      s = \"h\u{e9}llo\"\nprint(s[-1])\nprint(s[-4])";
        assert_eq!(output_of(script), "30\n10\n[10, 20, 0]\no\n\u{e9}\n");
    }

    #[test]
    fn slices_cap_at_the_boundaries_and_support_negative_bounds() {
        let script = "arr = [10, 20, 30, 40] \n print(arr[-2:4])\n\
                      xs = [1, 2, 3, 4, 5]\nprint(xs[1:3])\nprint(xs[:2])\nprint(xs[3:])\n\
                      print(xs[:])\nprint(xs[-2:])\nprint(xs[1:100])\nprint(xs[-100:2])\n\
                      print(xs[4:1])\nprint(xs[10:20])\n\
                      s = \"h\u{e9}llo w\u{f6}rld\"\nprint(s[0:2])\nprint(s[6:])\nprint(s[-3:])\nprint(\"abc\"[1:1])";
        assert_eq!(
            output_of(script),
            "[30, 40]\n[2, 3]\n[1, 2]\n[4, 5]\n[1, 2, 3, 4, 5]\n[4, 5]\n[2, 3, 4, 5]\n[1, 2]\n[]\n[]\n\
             h\u{e9}\nw\u{f6}rld\nrld\n\n"
        );
    }

    #[test]
    fn slices_are_independent_but_shallow_copies() {
        let script = "xs = [1, 2, 3]\nys = xs[0:2]\nys[0] = 99\nprint(xs)\nprint(ys)\n\
                      zs = xs[:]\nzs.push(4)\nprint(xs)\nprint(zs)\n\
                      m = [[1], [2]]\nc = m[:]\nc[0][0] = 9\nprint(m)";
        assert_eq!(
            output_of(script),
            "[1, 2, 3]\n[99, 2]\n[1, 2, 3]\n[1, 2, 3, 4]\n[[9], [2]]\n"
        );
    }

    #[test]
    fn push_appends_in_place_and_len_counts_characters() {
        let script = "arr = [10]\narr.push(50)\nprint(arr)\nprint(arr.len())\n\
                      print([1, 2, 3].len())\nprint([].len())\nprint(\"h\u{e9}llo\".len())";
        assert_eq!(output_of(script), "[10, 50]\n2\n3\n0\n5\n");
    }

    #[test]
    fn push_and_pop_return_values_and_share_pushed_arrays() {
        let script = "xs = []\nprint(xs.push(1))\nxs.push(\"a\")\nprint(xs)\nprint(xs.pop())\nprint(xs)\n\
                      inner = [1]\nouter = []\nouter.push(inner)\ninner.push(2)\nprint(outer)";
        assert_eq!(output_of(script), "unit\n[1, \"a\"]\na\n[1]\n[[1, 2]]\n");
    }

    #[test]
    fn loops_can_grow_and_walk_arrays_with_len() {
        let script = "xs = []\ni = 0\nwhile i < 5:\nxs.push(i * i)\ni = i + 1\nend\n\
                      total = 0\nj = 0\nwhile j < xs.len():\ntotal = total + xs[j]\nj = j + 1\nend\n\
                      print(xs)\nprint(total)";
        assert_eq!(output_of(script), "[0, 1, 4, 9, 16]\n30\n");
    }

    #[test]
    fn unary_minus_negates_integers() {
        let script = "print(-5)\nprint(2 - -3)\nprint(-2 * 3)\nprint(-(1 + 2))\n\
                      xs = [4, 5]\nprint(-xs[1])\nprint(--3)";
        assert_eq!(output_of(script), "-5\n5\n-6\n-3\n-5\n3\n");
    }

    #[test]
    fn self_referencing_arrays_print_without_recursing_forever() {
        let script = "a = [1]\na.push(a)\nprint(a)\nb = [0]\nb[0] = b\nprint(b)\n\
                      c = []\nd = [c]\nc.push(d)\nprint(c)";
        assert_eq!(output_of(script), "[1, [...]]\n[[...]]\n[[[...]]]\n");
    }

    #[test]
    fn functions_update_enclosing_arrays_through_the_scope_chain() {
        let script = "log = [0, 0]\ndef bump(i):\nlog[i] = log[i] + 1\nend\n\
                      bump(1)\nbump(1)\nprint(log)";
        assert_eq!(output_of(script), "[0, 2]\n");
    }

    #[test]
    fn loops_read_and_write_array_elements() {
        let script = "xs = [3, 1, 2]\ni = 0\ntotal = 0\n\
                      while i < 3:\ntotal = total + xs[i]\nxs[i] = xs[i] * 10\ni = i + 1\nend\n\
                      print(total)\nprint(xs)";
        assert_eq!(output_of(script), "6\n[30, 10, 20]\n");
    }

    #[test]
    fn concatenates_strings_with_strings_and_integers() {
        let script = "print(\"a\" + \"b\")\nprint(\"n=\" + 5)\nprint(5 + \"x\")\n\
                      print(\"a\" + \"b\" + 1 + 2)\nprint(1 + 2 + \"x\")\nprint(\"\" + \"\")";
        assert_eq!(output_of(script), "ab\nn=5\n5x\nab12\n3x\n\n");
    }

    #[test]
    fn strings_compare_for_equality() {
        let script = "print(\"a\" == \"a\")\nprint(\"a\" == \"b\")\nprint(\"\" == \"\")";
        assert_eq!(output_of(script), "true\nfalse\ntrue\n");
    }

    #[test]
    fn arrays_persist_between_runs() {
        let globals = Environment::global();
        run(&globals, "xs = [1]").unwrap();
        assert_eq!(run(&globals, "xs[0] = 5\nprint(xs)").unwrap(), "[5]\n");
    }

    #[test]
    fn failed_index_assignment_leaves_the_array_unchanged() {
        let globals = Environment::global();
        assert!(run(&globals, "xs = [1, 2]\nxs[5] = 0").is_err());
        assert_eq!(
            globals.borrow().get("xs"),
            Some(Value::new_array(vec![Value::Int(1), Value::Int(2)]))
        );
    }

    #[test]
    fn reports_runtime_errors() {
        let cases = [
            ("print(y)", "undefined variable `y`"),
            ("x = x + 1", "undefined variable `x`"),
            ("print(1 / 0)", "division by zero"),
            ("print(0 / 0)", "division by zero"),
            (
                "print(1 + (2 < 3))",
                "cannot apply `+` to integer and boolean",
            ),
            (
                "print(1 == (1 < 2))",
                "cannot apply `==` to integer and boolean",
            ),
            (
                "print((1 < 2) > (2 < 3))",
                "cannot apply `>` to boolean and boolean",
            ),
            (
                "while 1:\nend",
                "while condition must be a boolean, found integer",
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
            (
                "xs = [1, 2, 3]\nprint(xs[3])",
                "index 3 out of range for length 3",
            ),
            ("xs = []\nprint(xs[0])", "index 0 out of range for length 0"),
            (
                "xs = [1, 2, 3]\nprint(xs[-4])",
                "index -4 out of range for length 3",
            ),
            ("print(\"abc\"[-4])", "index -4 out of range for length 3"),
            ("print([][-1])", "index -1 out of range for length 0"),
            ("xs = [1]\nxs[-2] = 0", "index -2 out of range for length 1"),
            (
                "xs = [1]\nprint(xs[0:true])",
                "index must be an integer, found boolean",
            ),
            (
                "xs = [1]\nprint(xs[\"a\":])",
                "index must be an integer, found string",
            ),
            ("print(5[0:1])", "cannot index integer"),
            ("print(-true)", "cannot negate boolean"),
            ("print(-\"a\")", "cannot negate string"),
            ("print(-(0 - 9223372036854775807 - 1))", "integer overflow"),
            ("print([1].nope())", "array has no method `nope`"),
            ("print(\"a\".push(1))", "string has no method `push`"),
            ("print(5.len())", "integer has no method `len`"),
            (
                "xs = [1]\nxs.push()",
                "wrong number of arguments to `push`: expected 1, found 0",
            ),
            (
                "xs = [1]\nxs.len(1)",
                "wrong number of arguments to `len`: expected 0, found 1",
            ),
            ("xs = []\nxs.pop()", "pop from empty array"),
            (
                "m = [[1]]\nm[0][1] = 2",
                "index 1 out of range for length 1",
            ),
            (
                "m = [1]\nm[0][0] = 2",
                "cannot assign to an element of integer",
            ),
            ("nope.push(1)", "undefined variable `nope`"),
            (
                "xs = [1]\nxs[1:2] = [3]",
                "line 2: expected end of statement, found `=`",
            ),
            ("print(\"abc\"[3])", "index 3 out of range for length 3"),
            (
                "print(\"h\u{e9}llo\"[5])",
                "index 5 out of range for length 5",
            ),
            (
                "xs = [1]\nprint(xs[true])",
                "index must be an integer, found boolean",
            ),
            (
                "xs = [1]\nprint(xs[\"a\"])",
                "index must be an integer, found string",
            ),
            ("x = 5\nprint(x[0])", "cannot index integer"),
            ("print(true[0])", "cannot index boolean"),
            ("def f():\nend\nprint(f[0])", "cannot index function"),
            ("def f():\nreturn\nend\nprint(f()[0])", "cannot index unit"),
            ("print(nope[0])", "undefined variable `nope`"),
            ("nope[0] = 1", "undefined variable `nope`"),
            (
                "s = \"abc\"\ns[0] = \"x\"",
                "cannot assign to an element of string",
            ),
            ("x = 1\nx[0] = 2", "cannot assign to an element of integer"),
            ("xs = [1]\nxs[1] = 2", "index 1 out of range for length 1"),
            (
                "xs = [1]\nxs[true] = 2",
                "index must be an integer, found boolean",
            ),
            (
                "print(\"a\" + true)",
                "cannot apply `+` to string and boolean",
            ),
            ("print(\"a\" - 1)", "cannot apply `-` to string and integer"),
            ("print([1] + [2])", "cannot apply `+` to array and array"),
            (
                "print(\"a\" == 1)",
                "cannot apply `==` to string and integer",
            ),
            ("print([1] == [1])", "cannot apply `==` to array and array"),
            (
                "while \"x\":\nend",
                "while condition must be a boolean, found string",
            ),
            (
                "print(\"abc",
                "line 1: expected expression, found illegal token",
            ),
            (
                "xs = [1, 2\nprint(xs)",
                "line 1: expected `]`, found newline",
            ),
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
                "cannot apply `+` to function and integer",
            ),
            (
                "def f():\nreturn\nend\nprint(f() + 1)",
                "cannot apply `+` to unit and integer",
            ),
            (
                "def f():\nreturn 1\nend\nprint(f == f)",
                "cannot apply `==` to function and function",
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
