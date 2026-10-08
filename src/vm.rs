use std::io::Write;
use std::mem;

use crate::ast::BinaryOp;
use crate::compiler::{Chunk, SymbolTable};
use crate::evaluator::{RuntimeError, Value, apply, apply_int, negate};
use crate::opcodes::{OpCode, STACK_SIZE};

pub struct VirtualMachine {
    ip: usize,
    stack: Box<[Value; STACK_SIZE]>,
    sp: usize,
    symbols: SymbolTable,
    globals: Vec<Option<Value>>,
}

impl Default for VirtualMachine {
    fn default() -> Self {
        Self {
            ip: 0,
            stack: Box::new(std::array::from_fn(|_| Value::Unit)),
            sp: 0,
            symbols: SymbolTable::default(),
            globals: Vec::new(),
        }
    }
}

impl VirtualMachine {
    pub fn symbols_mut(&mut self) -> &mut SymbolTable {
        &mut self.symbols
    }

    pub fn run(&mut self, chunk: &Chunk, out: &mut impl Write) -> Result<(), RuntimeError> {
        self.globals.resize(self.symbols.slot_count(), None);
        let result = self.execute(chunk, out);
        self.clear_stack();
        result
    }

    fn execute(&mut self, chunk: &Chunk, out: &mut impl Write) -> Result<(), RuntimeError> {
        let code = chunk.code();
        self.ip = 0;
        while self.ip < code.len() {
            let opcode = OpCode::from_byte(code[self.ip]).expect("chunks hold only valid opcodes");
            self.ip += 1;
            match opcode {
                OpCode::Constant => {
                    let index = self.read_operand(code);
                    self.push(chunk.constants()[index].clone());
                }
                OpCode::Add => self.binary(BinaryOp::Add)?,
                OpCode::Subtract => self.binary(BinaryOp::Subtract)?,
                OpCode::Multiply => self.binary(BinaryOp::Multiply)?,
                OpCode::Divide => self.binary(BinaryOp::Divide)?,
                OpCode::Equal => self.binary(BinaryOp::Equal)?,
                OpCode::Greater => self.binary(BinaryOp::Greater)?,
                OpCode::Less => self.binary(BinaryOp::Less)?,
                OpCode::Negate => {
                    let operand = self.pop();
                    self.push(negate(operand)?);
                }
                OpCode::SetGlobal => {
                    let slot = self.read_operand(code);
                    self.globals[slot] = Some(self.pop());
                }
                OpCode::GetGlobal => {
                    let slot = self.read_operand(code);
                    let value = self.globals[slot].clone().ok_or_else(|| {
                        RuntimeError::UndefinedVariable(self.symbols.name(slot).to_owned())
                    })?;
                    self.push(value);
                }
                OpCode::Print => {
                    let value = self.pop();
                    writeln!(out, "{value}")?;
                }
                OpCode::Jump => self.ip = self.read_operand(code),
                OpCode::JumpIfFalse => {
                    let target = self.read_operand(code);
                    match self.pop() {
                        Value::Bool(true) => {}
                        Value::Bool(false) => self.ip = target,
                        other => return Err(RuntimeError::NonBooleanCondition(other.type_name())),
                    }
                }
            }
        }
        Ok(())
    }

    fn read_operand(&mut self, code: &[u8]) -> usize {
        let operand = u16::from_be_bytes([code[self.ip], code[self.ip + 1]]);
        self.ip += 2;
        usize::from(operand)
    }

    fn push(&mut self, value: Value) {
        self.stack[self.sp] = value;
        self.sp += 1;
    }

    fn pop(&mut self) -> Value {
        self.sp -= 1;
        mem::replace(&mut self.stack[self.sp], Value::Unit)
    }

    fn clear_stack(&mut self) {
        self.stack[..self.sp].fill_with(|| Value::Unit);
        self.sp = 0;
    }

    fn binary(&mut self, op: BinaryOp) -> Result<(), RuntimeError> {
        if let [.., Value::Int(left), Value::Int(right)] = &self.stack[..self.sp] {
            let result = apply_int(op, *left, *right)?;
            self.sp -= 1;
            self.stack[self.sp - 1] = result;
            return Ok(());
        }
        let right = self.pop();
        let left = self.pop();
        let result = apply(op, left, right)?;
        self.push(result);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;
    use crate::compiler::Compiler;
    use crate::evaluator::{Environment, execute};
    use crate::lexer::Lexer;
    use crate::opcodes::STACK_SIZE;
    use crate::parser::Parser;

    fn run(machine: &mut VirtualMachine, source: &str) -> Result<String, Box<dyn Error>> {
        let statements = Parser::new(Lexer::new(source)).parse_program()?;
        let chunk = Compiler::new(machine.symbols_mut()).compile(&statements)?;
        let mut output = Vec::new();
        machine.run(&chunk, &mut output)?;
        Ok(String::from_utf8(output)?)
    }

    fn output_of(source: &str) -> String {
        run(&mut VirtualMachine::default(), source).unwrap()
    }

    fn error_of(source: &str) -> String {
        run(&mut VirtualMachine::default(), source)
            .unwrap_err()
            .to_string()
    }

    fn outcome_of_tree_walker(source: &str) -> (String, Option<String>) {
        let statements = Parser::new(Lexer::new(source)).parse_program().unwrap();
        let mut output = Vec::new();
        let error = execute(&Environment::global(), &statements, &mut output).err();
        (
            String::from_utf8(output).unwrap(),
            error.map(|error| error.to_string()),
        )
    }

    fn outcome_of_bytecode(source: &str) -> (String, Option<String>) {
        let statements = Parser::new(Lexer::new(source)).parse_program().unwrap();
        let mut machine = VirtualMachine::default();
        let chunk = Compiler::new(machine.symbols_mut())
            .compile(&statements)
            .unwrap();
        let mut output = Vec::new();
        let error = machine.run(&chunk, &mut output).err();
        (
            String::from_utf8(output).unwrap(),
            error.map(|error| error.to_string()),
        )
    }

    #[test]
    fn arithmetic_follows_precedence_and_associativity() {
        let script = "print(5 + 2 * 3)\nprint((5 + 2) * 3)\nprint(7 / 2)\nprint(10 - 4 - 3)\n\
                      print(0 - 7 / 2)\nprint(-5 + 2)\nprint(-(2 + 3))\nprint(2 - -3)";
        assert_eq!(output_of(script), "11\n21\n3\n3\n-3\n-3\n-5\n5\n");
    }

    #[test]
    fn globals_hold_state_across_statements_and_runs() {
        assert_eq!(output_of("x = 5\ny = x * 2\nx = x + y\nprint(x)"), "15\n");
        let mut machine = VirtualMachine::default();
        run(&mut machine, "z = 4").unwrap();
        assert_eq!(run(&mut machine, "print(z * z)").unwrap(), "16\n");
    }

    #[test]
    fn while_loop_counts_up_to_the_bound() {
        let script = "x = 5 \n y = 10 \n while x < y: \n x = x + 1 \n end \n print(x)";
        assert_eq!(output_of(script), "10\n");
    }

    #[test]
    fn loops_run_every_body_statement_each_iteration() {
        let script = "i = 1\nwhile i < 4:\nprint(i)\ni = i + 1\nend\nprint(i)";
        assert_eq!(output_of(script), "1\n2\n3\n4\n");
    }

    #[test]
    fn loop_with_a_false_condition_never_runs() {
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
    fn comparisons_booleans_and_strings_evaluate() {
        let script = "print(1 < 2)\nprint(2 < 2)\nprint(3 > 2)\nprint(3 == 3)\nprint(true == (1 < 2))\n\
                      print(\"a\" + \"b\")\nprint(\"n=\" + 5)\nprint(5 + \"x\")\nprint(\"a\" == \"a\")";
        assert_eq!(
            output_of(script),
            "true\nfalse\ntrue\ntrue\ntrue\nab\nn=5\n5x\ntrue\n"
        );
    }

    #[test]
    fn an_expression_filling_the_stack_exactly_still_runs() {
        let nesting = "1 + (".repeat(STACK_SIZE - 1) + "1" + &")".repeat(STACK_SIZE - 1);
        assert_eq!(output_of(&format!("print({nesting})")), "256\n");
    }

    #[test]
    fn runtime_errors_match_the_language_rules() {
        let cases = [
            ("print(y)", "undefined variable `y`"),
            ("x = x + 1", "undefined variable `x`"),
            ("print(1 / 0)", "division by zero"),
            ("print(9223372036854775807 + 1)", "integer overflow"),
            ("print(-(0 - 9223372036854775807 - 1))", "integer overflow"),
            (
                "print(1 + (2 < 3))",
                "cannot apply `+` to integer and boolean",
            ),
            ("print(\"a\" - 1)", "cannot apply `-` to string and integer"),
            ("print(-true)", "cannot negate boolean"),
            (
                "while 1:\nend",
                "while condition must be a boolean, found integer",
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
        let mut machine = VirtualMachine::default();
        let chunk = Compiler::new(machine.symbols_mut())
            .compile(&statements)
            .unwrap();
        let mut output = Vec::new();
        let result = machine.run(&chunk, &mut output);
        assert!(matches!(result, Err(RuntimeError::DivisionByZero)));
        assert_eq!(output, b"1\n");
    }

    #[test]
    fn a_name_read_before_it_is_assigned_is_undefined_until_assigned() {
        let mut machine = VirtualMachine::default();
        assert_eq!(
            run(&mut machine, "print(y)").unwrap_err().to_string(),
            "undefined variable `y`"
        );
        run(&mut machine, "y = 3").unwrap();
        assert_eq!(run(&mut machine, "print(y)").unwrap(), "3\n");
    }

    #[test]
    fn a_failed_run_leaves_globals_intact_and_the_stack_clean() {
        let mut machine = VirtualMachine::default();
        run(&mut machine, "x = 1").unwrap();
        assert!(run(&mut machine, "x = x + (2 + (3 / 0))").is_err());
        assert_eq!(machine.sp, 0);
        assert_eq!(run(&mut machine, "print(x)").unwrap(), "1\n");
    }

    #[test]
    fn bytecode_matches_the_tree_walker_on_the_shared_language() {
        let scripts = [
            "x = 5\ny = 10\nwhile x < y:\nx = x + 1\nend\nprint(x)",
            "i = 0\ntotal = 0\nwhile i < 10:\ntotal = total + i * i\ni = i + 1\nend\nprint(total)",
            "print(1 + 2 * 3 - 4 / 2)",
            "print(-3 * -4)\nprint(--5)",
            "a = \"x\" + 1 + \"y\"\nprint(a)\nprint(a == \"x1y\")",
            "print(true == false)\nprint(1 > 2)\nprint(2 > 1)",
            "print(9223372036854775807 + 1)",
            "print(1)\nprint(1 / 0)",
            "print(1)\nx = x + 1",
            "x = 1\nx = x + (1 / 0)",
            "print(\"a\" - 1)",
            "print(-true)",
            "print(1 == true)",
            "while 1:\nend",
            "n = 3\nwhile n > 0:\nprint(n)\nn = n - 1\nend\nprint(n)",
        ];
        for script in scripts {
            assert_eq!(
                outcome_of_bytecode(script),
                outcome_of_tree_walker(script),
                "{script:?}"
            );
        }
    }
}
