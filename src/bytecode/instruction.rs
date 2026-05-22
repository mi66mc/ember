use crate::bytecode::opcode::Opcode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instruction {
    opcode: Opcode,
    operands: [u8; 3],
}

impl Instruction {
    pub fn new(opcode: Opcode, operands: [u8; 3]) -> Self {
        Instruction { opcode, operands }
    }

    pub fn abc(opcode: Opcode, a: u8, b: u8, c: u8) -> Self {
        Self::new(opcode, [a, b, c])
    }

    pub fn abx(opcode: Opcode, a: u8, bx: u16) -> Self {
        let [b, c] = bx.to_le_bytes();
        Self::new(opcode, [a, b, c])
    }

    pub fn asbx(opcode: Opcode, a: u8, sbx: i16) -> Self {
        let [b, c] = sbx.to_le_bytes();
        Self::new(opcode, [a, b, c])
    }

    pub fn jmp(opcode: Opcode, offset: i16) -> Self {
        let [a, b] = offset.to_le_bytes();
        Self::new(opcode, [a, b, 0])
    }

    pub fn opcode(&self) -> Opcode {
        self.opcode
    }

    pub fn a(&self) -> u8 {
        self.operands[0]
    }

    pub fn b(&self) -> u8 {
        self.operands[1]
    }

    pub fn c(&self) -> u8 {
        self.operands[2]
    }

    pub fn bx(&self) -> u16 {
        u16::from_le_bytes([self.operands[1], self.operands[2]])
    }

    pub fn sbx(&self) -> i16 {
        i16::from_le_bytes([self.operands[1], self.operands[2]])
    }

    pub fn sbx_ab(&self) -> i16 {
        i16::from_le_bytes([self.operands[0], self.operands[1]])
    }
}

