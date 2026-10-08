pub const STACK_SIZE: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OpCode {
    Constant,
    Add,
    Subtract,
    Multiply,
    Divide,
    SetGlobal,
    GetGlobal,
    Print,
    Jump,
    JumpIfFalse,
    Negate,
    Equal,
    Greater,
    Less,
}

impl OpCode {
    const ALL: [OpCode; 14] = [
        OpCode::Constant,
        OpCode::Add,
        OpCode::Subtract,
        OpCode::Multiply,
        OpCode::Divide,
        OpCode::SetGlobal,
        OpCode::GetGlobal,
        OpCode::Print,
        OpCode::Jump,
        OpCode::JumpIfFalse,
        OpCode::Negate,
        OpCode::Equal,
        OpCode::Greater,
        OpCode::Less,
    ];

    pub fn from_byte(byte: u8) -> Option<Self> {
        Self::ALL.get(usize::from(byte)).copied()
    }
}

impl From<OpCode> for u8 {
    fn from(opcode: OpCode) -> Self {
        opcode as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_opcode_round_trips_through_its_raw_byte() {
        for (position, opcode) in OpCode::ALL.into_iter().enumerate() {
            assert_eq!(usize::from(u8::from(opcode)), position);
            assert_eq!(OpCode::from_byte(u8::from(opcode)), Some(opcode));
        }
    }

    #[test]
    fn the_requested_instructions_keep_their_order_at_the_start() {
        let requested = [
            OpCode::Constant,
            OpCode::Add,
            OpCode::Subtract,
            OpCode::Multiply,
            OpCode::Divide,
            OpCode::SetGlobal,
            OpCode::GetGlobal,
            OpCode::Print,
            OpCode::Jump,
            OpCode::JumpIfFalse,
        ];
        assert_eq!(OpCode::ALL[..requested.len()], requested);
    }

    #[test]
    fn bytes_past_the_last_opcode_do_not_decode() {
        let past_the_end = u8::try_from(OpCode::ALL.len()).unwrap();
        assert_eq!(OpCode::from_byte(past_the_end), None);
        assert_eq!(OpCode::from_byte(u8::MAX), None);
    }
}
