use crate::common::types::opcode::Opcode;

// ┌─────────┬─────────┬─────────┬─────────┐
// │ opcode  │    A    │    B    │    C    │
// │  8 bits │  8 bits │  8 bits │  8 bits │
// └─────────┴─────────┴─────────┴─────────┘
//
// formats:
//   abc  -> Ra, Rb, Rc     (3 registers)
//   abx  -> Ra, Bx         (1 register + 16-bit unsigned)
//   asbx -> Ra, sBx        (1 register + 16-bit signed)
//   jmp  -> sBx            (16-bit signed offset only)

#[derive(Debug, Clone, Copy)]
pub struct Instruction {
    opcode: Opcode,
    operands: [u8; 3],
}

impl Instruction {
    pub fn new(opcode: Opcode, operands: [u8; 3]) -> Self {
        Instruction { opcode, operands }
    }

    // abc: Ra <- Rb op Rc
    pub fn abc(opcode: Opcode, a: u8, b: u8, c: u8) -> Self {
        Self::new(opcode, [a, b, c])
    }

    // abx: Ra <- constants[Bx]
    pub fn abx(opcode: Opcode, a: u8, bx: u16) -> Self {
        let [b, c] = bx.to_le_bytes();
        Self::new(opcode, [a, b, c])
    }

    // asbx: if Ra then pc <- pc + sBx
    pub fn asbx(opcode: Opcode, a: u8, sbx: i16) -> Self {
        let [b, c] = sbx.to_le_bytes();
        Self::new(opcode, [a, b, c])
    }

    // jmp: pc <- pc + offset
    pub fn jmp(opcode: Opcode, offset: i16) -> Self {
        let bytes = offset.to_le_bytes();
        Self::new(opcode, [bytes[0], bytes[1], 0])
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

    // B + C as u16
    pub fn bx(&self) -> u16 {
        u16::from_le_bytes([self.operands[1], self.operands[2]])
    }

    // B + C as i16
    pub fn sbx(&self) -> i16 {
        i16::from_le_bytes([self.operands[1], self.operands[2]])
    }

    // A + B as i16 (for jmp format)
    pub fn sbx_ab(&self) -> i16 {
        i16::from_le_bytes([self.operands[0], self.operands[1]])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instruction_32_bits() {
        assert_eq!(size_of::<Instruction>(), 4);
    }

    #[test]
    fn test_abc_format() {
        let instr = Instruction::abc(Opcode::ADD_I64, 0, 1, 2);
        assert_eq!(instr.opcode(), Opcode::ADD_I64);
        assert_eq!(instr.a(), 0);
        assert_eq!(instr.b(), 1);
        assert_eq!(instr.c(), 2);
    }

    #[test]
    fn test_abx_format() {
        let instr = Instruction::abx(Opcode::LOADK, 5, 1000);
        assert_eq!(instr.opcode(), Opcode::LOADK);
        assert_eq!(instr.a(), 5);
        assert_eq!(instr.bx(), 1000);
    }

    #[test]
    fn test_abx_max_value() {
        let instr = Instruction::abx(Opcode::LOADK, 0, 65535);
        assert_eq!(instr.bx(), 65535);
    }

    #[test]
    fn test_asbx_positive() {
        let instr = Instruction::asbx(Opcode::JMPIF, 3, 100);
        assert_eq!(instr.opcode(), Opcode::JMPIF);
        assert_eq!(instr.a(), 3);
        assert_eq!(instr.sbx(), 100);
    }

    #[test]
    fn test_asbx_negative() {
        let instr = Instruction::asbx(Opcode::JMPIF, 3, -50);
        assert_eq!(instr.sbx(), -50);
    }

    #[test]
    fn test_jmp_offset() {
        let instr = Instruction::jmp(Opcode::JMP, -10);
        assert_eq!(instr.sbx_ab(), -10);
    }
}
