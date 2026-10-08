use std::collections::HashMap;
use std::fmt;

use crate::ast::{BinaryOp, Expression, Statement};
use crate::evaluator::Value;
use crate::opcodes::{OpCode, STACK_SIZE};

#[derive(Debug, Default)]
pub struct Chunk {
    code: Vec<u8>,
    constants: Vec<Value>,
}

impl Chunk {
    pub fn code(&self) -> &[u8] {
        &self.code
    }

    pub fn constants(&self) -> &[Value] {
        &self.constants
    }
}

#[derive(Debug, Default)]
pub struct SymbolTable {
    slots: HashMap<String, u16>,
    names: Vec<String>,
}

impl SymbolTable {
    pub fn name(&self, slot: usize) -> &str {
        &self.names[slot]
    }

    pub fn slot_count(&self) -> usize {
        self.names.len()
    }

    fn slot(&mut self, name: &str) -> Result<u16, CompileError> {
        if let Some(&slot) = self.slots.get(name) {
            return Ok(slot);
        }
        let slot = u16::try_from(self.names.len()).map_err(|_| CompileError::TooManyGlobals)?;
        self.slots.insert(name.to_owned(), slot);
        self.names.push(name.to_owned());
        Ok(slot)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CompileError {
    Unsupported(&'static str),
    TooManyConstants,
    TooManyGlobals,
    ChunkTooLarge,
    ExpressionTooDeep,
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::Unsupported(feature) => {
                write!(f, "the bytecode compiler does not support {feature} yet")
            }
            CompileError::TooManyConstants => f.write_str("too many constants in one chunk"),
            CompileError::TooManyGlobals => f.write_str("too many distinct global variables"),
            CompileError::ChunkTooLarge => f.write_str("jump target beyond the 64 KiB chunk limit"),
            CompileError::ExpressionTooDeep => {
                f.write_str("expression needs more stack than the virtual machine has")
            }
        }
    }
}

impl std::error::Error for CompileError {}

pub struct Compiler<'a> {
    chunk: Chunk,
    symbols: &'a mut SymbolTable,
}

impl<'a> Compiler<'a> {
    pub fn new(symbols: &'a mut SymbolTable) -> Self {
        Self {
            chunk: Chunk::default(),
            symbols,
        }
    }

    pub fn compile(mut self, statements: &[Statement]) -> Result<Chunk, CompileError> {
        for statement in statements {
            self.compile_statement(statement)?;
        }
        Ok(self.chunk)
    }

    fn compile_statement(&mut self, statement: &Statement) -> Result<(), CompileError> {
        match statement {
            Statement::Assign { name, value } => {
                self.compile_expression(value, 0)?;
                let slot = self.symbols.slot(name)?;
                self.emit_operand(OpCode::SetGlobal, slot);
            }
            Statement::Print(expression) => {
                self.compile_expression(expression, 0)?;
                self.emit(OpCode::Print);
            }
            Statement::While { condition, body } => self.compile_while(condition, body)?,
            Statement::IndexAssign { .. } => {
                return Err(CompileError::Unsupported("index assignment"));
            }
            Statement::FunctionDef { .. } => {
                return Err(CompileError::Unsupported("function definitions"));
            }
            Statement::Return(_) => return Err(CompileError::Unsupported("return statements")),
            Statement::Expression(_) => {
                return Err(CompileError::Unsupported("expression statements"));
            }
        }
        Ok(())
    }

    fn compile_while(
        &mut self,
        condition: &Expression,
        body: &[Statement],
    ) -> Result<(), CompileError> {
        let loop_start = self.address()?;
        self.compile_expression(condition, 0)?;
        let exit = self.emit_jump(OpCode::JumpIfFalse);
        for statement in body {
            self.compile_statement(statement)?;
        }
        self.emit_operand(OpCode::Jump, loop_start);
        self.patch_jump(exit)
    }

    fn compile_expression(
        &mut self,
        expression: &Expression,
        depth: usize,
    ) -> Result<(), CompileError> {
        match expression {
            Expression::Int(value) => self.emit_constant(Value::Int(*value), depth),
            Expression::Bool(value) => self.emit_constant(Value::Bool(*value), depth),
            Expression::String(text) => self.emit_constant(Value::String(text.clone()), depth),
            Expression::Ident(name) => {
                ensure_stack_slot(depth)?;
                let slot = self.symbols.slot(name)?;
                self.emit_operand(OpCode::GetGlobal, slot);
                Ok(())
            }
            Expression::Negate(operand) => {
                self.compile_expression(operand, depth)?;
                self.emit(OpCode::Negate);
                Ok(())
            }
            Expression::Binary { op, left, right } => {
                self.compile_expression(left, depth)?;
                self.compile_expression(right, depth + 1)?;
                self.emit(binary_opcode(*op));
                Ok(())
            }
            Expression::Array(_) => Err(CompileError::Unsupported("arrays")),
            Expression::Index { .. } => Err(CompileError::Unsupported("indexing")),
            Expression::Slice { .. } => Err(CompileError::Unsupported("slicing")),
            Expression::MethodCall { .. } => Err(CompileError::Unsupported("method calls")),
            Expression::Call { .. } => Err(CompileError::Unsupported("function calls")),
        }
    }

    fn emit_constant(&mut self, value: Value, depth: usize) -> Result<(), CompileError> {
        ensure_stack_slot(depth)?;
        let index = self.add_constant(value)?;
        self.emit_operand(OpCode::Constant, index);
        Ok(())
    }

    fn add_constant(&mut self, value: Value) -> Result<u16, CompileError> {
        let index = u16::try_from(self.chunk.constants.len())
            .map_err(|_| CompileError::TooManyConstants)?;
        self.chunk.constants.push(value);
        Ok(index)
    }

    fn address(&self) -> Result<u16, CompileError> {
        u16::try_from(self.chunk.code.len()).map_err(|_| CompileError::ChunkTooLarge)
    }

    fn emit(&mut self, opcode: OpCode) {
        self.chunk.code.push(u8::from(opcode));
    }

    fn emit_operand(&mut self, opcode: OpCode, operand: u16) {
        self.emit(opcode);
        self.chunk.code.extend_from_slice(&operand.to_be_bytes());
    }

    fn emit_jump(&mut self, opcode: OpCode) -> usize {
        self.emit_operand(opcode, 0);
        self.chunk.code.len() - 2
    }

    fn patch_jump(&mut self, operand_position: usize) -> Result<(), CompileError> {
        let target = self.address()?;
        self.chunk.code[operand_position..operand_position + 2]
            .copy_from_slice(&target.to_be_bytes());
        Ok(())
    }
}

fn ensure_stack_slot(depth: usize) -> Result<(), CompileError> {
    if depth < STACK_SIZE {
        Ok(())
    } else {
        Err(CompileError::ExpressionTooDeep)
    }
}

fn binary_opcode(op: BinaryOp) -> OpCode {
    match op {
        BinaryOp::Add => OpCode::Add,
        BinaryOp::Subtract => OpCode::Subtract,
        BinaryOp::Multiply => OpCode::Multiply,
        BinaryOp::Divide => OpCode::Divide,
        BinaryOp::Equal => OpCode::Equal,
        BinaryOp::Greater => OpCode::Greater,
        BinaryOp::Less => OpCode::Less,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn compile(source: &str) -> Result<Chunk, CompileError> {
        compile_with(&mut SymbolTable::default(), source)
    }

    fn compile_with(symbols: &mut SymbolTable, source: &str) -> Result<Chunk, CompileError> {
        let statements = Parser::new(Lexer::new(source)).parse_program().unwrap();
        Compiler::new(symbols).compile(&statements)
    }

    #[test]
    fn arithmetic_compiles_operands_before_their_operator() {
        let chunk = compile("x = 1 + 2 * 3").unwrap();
        let [constant, multiply, add, set_global] = [
            OpCode::Constant,
            OpCode::Multiply,
            OpCode::Add,
            OpCode::SetGlobal,
        ]
        .map(u8::from);
        assert_eq!(
            chunk.code(),
            [
                constant, 0, 0, constant, 0, 1, constant, 0, 2, multiply, add, set_global, 0, 0
            ]
        );
        assert_eq!(
            chunk.constants(),
            [Value::Int(1), Value::Int(2), Value::Int(3)]
        );
    }

    #[test]
    fn while_loop_jumps_forward_past_the_body_and_back_to_the_condition() {
        let chunk = compile("x = 1\nwhile x < 3:\nx = x + 1\nend").unwrap();
        let [
            constant,
            get_global,
            set_global,
            less,
            add,
            jump,
            jump_if_false,
        ] = [
            OpCode::Constant,
            OpCode::GetGlobal,
            OpCode::SetGlobal,
            OpCode::Less,
            OpCode::Add,
            OpCode::Jump,
            OpCode::JumpIfFalse,
        ]
        .map(u8::from);
        assert_eq!(
            chunk.code(),
            [
                constant,
                0,
                0,
                set_global,
                0,
                0,
                get_global,
                0,
                0,
                constant,
                0,
                1,
                less,
                jump_if_false,
                0,
                29,
                get_global,
                0,
                0,
                constant,
                0,
                2,
                add,
                set_global,
                0,
                0,
                jump,
                0,
                6,
            ]
        );
    }

    #[test]
    fn nested_loops_patch_their_own_exits() {
        let chunk = compile("while 1 < 2:\nwhile 3 < 4:\nend\nend").unwrap();
        let code = chunk.code();
        let operand =
            |position: usize| usize::from(u16::from_be_bytes([code[position], code[position + 1]]));
        let outer_exit = operand(8);
        let inner_exit = operand(18);
        let inner_back = operand(code.len() - 5);
        let outer_back = operand(code.len() - 2);
        assert_eq!(outer_exit, code.len());
        assert_eq!(outer_back, 0);
        assert_eq!(inner_back, 10);
        assert_eq!(inner_exit, code.len() - 3);
    }

    #[test]
    fn names_resolve_to_stable_global_slots_across_chunks() {
        let mut symbols = SymbolTable::default();
        let first = compile_with(&mut symbols, "x = 1\ny = 2\nx = 3").unwrap();
        let second = compile_with(&mut symbols, "print(y)\nz = x").unwrap();
        let [constant, set_global, get_global, print] = [
            OpCode::Constant,
            OpCode::SetGlobal,
            OpCode::GetGlobal,
            OpCode::Print,
        ]
        .map(u8::from);
        assert_eq!(
            first.code(),
            [
                constant, 0, 0, set_global, 0, 0, constant, 0, 1, set_global, 0, 1, constant, 0, 2,
                set_global, 0, 0
            ]
        );
        assert_eq!(
            second.code(),
            [get_global, 0, 1, print, get_global, 0, 0, set_global, 0, 2]
        );
        assert_eq!(symbols.slot_count(), 3);
        assert_eq!(symbols.name(2), "z");
    }

    #[test]
    fn unsupported_constructs_report_what_is_missing() {
        let cases = [
            ("def f():\nend", "function definitions"),
            ("return 1", "return statements"),
            ("f(1)", "expression statements"),
            ("xs[0] = 1", "index assignment"),
            ("x = f(1)", "function calls"),
            ("x = [1]", "arrays"),
            ("x = y[0]", "indexing"),
            ("x = y[0:1]", "slicing"),
            ("x = y.len()", "method calls"),
        ];
        for (source, feature) in cases {
            assert_eq!(
                compile(source).unwrap_err().to_string(),
                format!("the bytecode compiler does not support {feature} yet"),
                "{source:?}"
            );
        }
    }

    #[test]
    fn expressions_deeper_than_the_stack_are_rejected() {
        let nesting = "1 + (".repeat(STACK_SIZE + 1) + "1" + &")".repeat(STACK_SIZE + 1);
        assert_eq!(
            compile(&format!("x = {nesting}")).unwrap_err(),
            CompileError::ExpressionTooDeep
        );
        let fits = "1 + (".repeat(STACK_SIZE - 1) + "1" + &")".repeat(STACK_SIZE - 1);
        assert!(compile(&format!("x = {fits}")).is_ok());
    }

    #[test]
    fn a_program_cannot_name_more_globals_than_a_u16_slot_addresses() {
        let source = (0..=usize::from(u16::MAX) + 1)
            .map(|number| format!("print(v{number})\n"))
            .collect::<String>();
        assert_eq!(compile(&source).unwrap_err(), CompileError::TooManyGlobals);
    }

    #[test]
    fn a_chunk_cannot_hold_more_constants_than_a_u16_operand_addresses() {
        let source = "x = 1\n".repeat(usize::from(u16::MAX) + 2);
        assert_eq!(
            compile(&source).unwrap_err(),
            CompileError::TooManyConstants
        );
    }

    #[test]
    fn jump_targets_beyond_a_u16_are_rejected() {
        let body = "print(x)\n".repeat(usize::from(u16::MAX) / 4 + 1);
        let source = format!("x = 1\nwhile x < 2:\n{body}end");
        assert_eq!(compile(&source).unwrap_err(), CompileError::ChunkTooLarge);
    }
}
