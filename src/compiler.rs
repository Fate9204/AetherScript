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
        self.compile_block(statements)?;
        Ok(self.chunk)
    }

    fn compile_block(&mut self, statements: &[Statement]) -> Result<(), CompileError> {
        statements
            .iter()
            .try_for_each(|statement| self.compile_statement(statement))
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
            Statement::If {
                condition,
                then_body,
                else_body,
            } => self.compile_if(condition, then_body, else_body)?,
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

    fn compile_if(
        &mut self,
        condition: &Expression,
        then_body: &[Statement],
        else_body: &[Statement],
    ) -> Result<(), CompileError> {
        self.compile_expression(condition, 0)?;
        let skip_then = self.emit_jump(OpCode::JumpIfFalse);
        self.compile_block(then_body)?;
        if else_body.is_empty() {
            return self.patch_jump(skip_then);
        }
        let skip_else = self.emit_jump(OpCode::Jump);
        self.patch_jump(skip_then)?;
        self.compile_block(else_body)?;
        self.patch_jump(skip_else)
    }

    fn compile_while(
        &mut self,
        condition: &Expression,
        body: &[Statement],
    ) -> Result<(), CompileError> {
        let loop_start = self.address()?;
        self.compile_expression(condition, 0)?;
        let exit = self.emit_jump(OpCode::JumpIfFalse);
        self.compile_block(body)?;
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
        BinaryOp::NotEqual => OpCode::NotEqual,
        BinaryOp::Greater => OpCode::Greater,
        BinaryOp::GreaterEqual => OpCode::GreaterEqual,
        BinaryOp::Less => OpCode::Less,
        BinaryOp::LessEqual => OpCode::LessEqual,
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

    fn right_nested(leaf: &str, levels: usize) -> String {
        format!("{leaf} + (").repeat(levels) + leaf + &")".repeat(levels)
    }

    #[test]
    fn expressions_deeper_than_the_stack_are_rejected_at_the_exact_boundary() {
        for leaf in ["1", "x"] {
            let fits = format!("print({})", right_nested(leaf, STACK_SIZE - 1));
            assert!(compile(&fits).is_ok(), "{leaf}");
            let overflows = format!("print({})", right_nested(leaf, STACK_SIZE));
            assert_eq!(
                compile(&overflows).unwrap_err(),
                CompileError::ExpressionTooDeep,
                "{leaf}"
            );
        }
    }

    #[test]
    fn only_right_operands_consume_stack_depth() {
        let flat_sum = ["1"; STACK_SIZE * 2].join(" + ");
        assert!(compile(&format!("x = {flat_sum}")).is_ok());
        let negations = "-".repeat(STACK_SIZE * 2);
        assert!(compile(&format!("x = {negations}1")).is_ok());
        let condition = right_nested("1", STACK_SIZE - 1);
        assert!(compile(&format!("while {condition}:\nend")).is_ok());
        assert!(compile(&format!("if {condition}:\nend")).is_ok());
    }

    fn print_globals(count: usize) -> String {
        (0..count).map(|n| format!("print(v{n})\n")).collect()
    }

    fn print_in_loop(body_statements: usize) -> String {
        let body = "print(x)\n".repeat(body_statements);
        format!("x = 1\nwhile x < 2:\n{body}end")
    }

    #[test]
    fn a_program_can_name_exactly_as_many_globals_as_a_u16_slot_addresses() {
        let limit = usize::from(u16::MAX) + 1;
        assert!(compile(&print_globals(limit)).is_ok());
        assert_eq!(
            compile(&print_globals(limit + 1)).unwrap_err(),
            CompileError::TooManyGlobals
        );
    }

    #[test]
    fn a_chunk_can_hold_exactly_as_many_constants_as_a_u16_operand_addresses() {
        let limit = usize::from(u16::MAX) + 1;
        assert!(compile(&"x = 1\n".repeat(limit)).is_ok());
        assert_eq!(
            compile(&"x = 1\n".repeat(limit + 1)).unwrap_err(),
            CompileError::TooManyConstants
        );
    }

    #[test]
    fn a_jump_may_target_the_last_address_a_u16_holds() {
        assert!(compile(&print_in_loop(16379)).is_ok());
        assert_eq!(
            compile(&print_in_loop(16380)).unwrap_err(),
            CompileError::ChunkTooLarge
        );
    }

    #[test]
    fn compile_errors_describe_the_limit_that_was_hit() {
        let cases = [
            (
                CompileError::TooManyConstants,
                "too many constants in one chunk",
            ),
            (
                CompileError::TooManyGlobals,
                "too many distinct global variables",
            ),
            (
                CompileError::ChunkTooLarge,
                "jump target beyond the 64 KiB chunk limit",
            ),
            (
                CompileError::ExpressionTooDeep,
                "expression needs more stack than the virtual machine has",
            ),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
        }
    }

    #[test]
    fn if_without_else_jumps_past_the_body_when_false() {
        let chunk = compile("if 1 < 2:\nprint(3)\nend").unwrap();
        let [constant, less, jump_if_false, print] = [
            OpCode::Constant,
            OpCode::Less,
            OpCode::JumpIfFalse,
            OpCode::Print,
        ]
        .map(u8::from);
        assert_eq!(
            chunk.code(),
            [
                constant,
                0,
                0,
                constant,
                0,
                1,
                less,
                jump_if_false,
                0,
                14,
                constant,
                0,
                2,
                print,
            ]
        );
    }

    #[test]
    fn if_with_else_skips_the_else_after_the_then_branch() {
        let chunk = compile("if 1 < 2:\nprint(3)\nelse:\nprint(4)\nend").unwrap();
        let [constant, less, jump_if_false, jump, print] = [
            OpCode::Constant,
            OpCode::Less,
            OpCode::JumpIfFalse,
            OpCode::Jump,
            OpCode::Print,
        ]
        .map(u8::from);
        assert_eq!(
            chunk.code(),
            [
                constant,
                0,
                0,
                constant,
                0,
                1,
                less,
                jump_if_false,
                0,
                17,
                constant,
                0,
                2,
                print,
                jump,
                0,
                21,
                constant,
                0,
                3,
                print,
            ]
        );
    }

    #[test]
    fn an_empty_else_compiles_like_no_else() {
        let chunk = compile("if true:\nelse:\nend").unwrap();
        let [constant, jump_if_false] = [OpCode::Constant, OpCode::JumpIfFalse].map(u8::from);
        assert_eq!(chunk.code(), [constant, 0, 0, jump_if_false, 0, 6]);
    }

    #[test]
    fn an_empty_then_branch_still_gets_a_jump_over_the_else() {
        let chunk = compile("if true:\nelse:\nprint(1)\nend").unwrap();
        let [constant, jump_if_false, jump, print] = [
            OpCode::Constant,
            OpCode::JumpIfFalse,
            OpCode::Jump,
            OpCode::Print,
        ]
        .map(u8::from);
        assert_eq!(
            chunk.code(),
            [
                constant,
                0,
                0,
                jump_if_false,
                0,
                9,
                jump,
                0,
                13,
                constant,
                0,
                1,
                print,
            ]
        );
    }

    #[test]
    fn each_new_comparison_compiles_to_its_own_instruction() {
        let cases = [
            ("<=", OpCode::LessEqual),
            (">=", OpCode::GreaterEqual),
            ("!=", OpCode::NotEqual),
        ];
        for (operator, opcode) in cases {
            let chunk = compile(&format!("x = 1 {operator} 2")).unwrap();
            assert_eq!(chunk.code()[6], u8::from(opcode), "{operator}");
        }
    }

    #[test]
    fn an_if_inside_a_loop_patches_independent_targets() {
        let chunk = compile("while 1 < 2:\nif 3 < 4:\nprint(5)\nend\nend").unwrap();
        let code = chunk.code();
        let operand =
            |position: usize| usize::from(u16::from_be_bytes([code[position], code[position + 1]]));
        let loop_exit = operand(8);
        let if_exit = operand(18);
        let loop_back = operand(code.len() - 2);
        assert_eq!(loop_exit, code.len());
        assert_eq!(if_exit, code.len() - 3);
        assert_eq!(loop_back, 0);
    }
}
