use crate::bytecode::opcode::Opcode;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instruction {
    pub opcode_byte: u8,
    pub a: u8,
    pub b: u8,
    pub c: u8,
}

impl Instruction {
    pub fn new(opcode: Opcode, operands: [u8; 3]) -> Self {
        Instruction {
            opcode_byte: opcode.to_byte(),
            a: operands[0],
            b: operands[1],
            c: operands[2],
        }
    }

    pub fn abc(opcode: Opcode, a: u8, b: u8, c: u8) -> Self {
        Instruction {
            opcode_byte: opcode.to_byte(),
            a,
            b,
            c,
        }
    }

    pub fn abx(opcode: Opcode, a: u8, bx: u16) -> Self {
        let [b, c] = bx.to_le_bytes();
        Instruction {
            opcode_byte: opcode.to_byte(),
            a,
            b,
            c,
        }
    }

    pub fn asbx(opcode: Opcode, a: u8, sbx: i16) -> Self {
        let [b, c] = sbx.to_le_bytes();
        Instruction {
            opcode_byte: opcode.to_byte(),
            a,
            b,
            c,
        }
    }

    pub fn jmp(opcode: Opcode, offset: i16) -> Self {
        let [a, b] = offset.to_le_bytes();
        Instruction {
            opcode_byte: opcode.to_byte(),
            a,
            b,
            c: 0,
        }
    }

    pub fn opcode(&self) -> Opcode {
        Opcode::from_byte(self.opcode_byte).unwrap_or(Opcode::NOP)
    }

    pub fn a(&self) -> u8 {
        self.a
    }

    pub fn b(&self) -> u8 {
        self.b
    }

    pub fn c(&self) -> u8 {
        self.c
    }

    pub fn bx(&self) -> u16 {
        u16::from_le_bytes([self.b, self.c])
    }

    pub fn sbx(&self) -> i16 {
        i16::from_le_bytes([self.b, self.c])
    }

    pub fn sbx_ab(&self) -> i16 {
        i16::from_le_bytes([self.a, self.b])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instruction_stays_32_bits() {
        assert_eq!(size_of::<Instruction>(), 4);
    }

    #[test]
    fn formats_round_trip_operands() {
        let abc = Instruction::abc(Opcode::ADD_I64, 0, 1, 2);
        assert_eq!(abc.a(), 0);
        assert_eq!(abc.b(), 1);
        assert_eq!(abc.c(), 2);

        let abx = Instruction::abx(Opcode::LOADK, 5, 1000);
        assert_eq!(abx.a(), 5);
        assert_eq!(abx.bx(), 1000);

        let asbx = Instruction::asbx(Opcode::JMPIF, 3, -50);
        assert_eq!(asbx.a(), 3);
        assert_eq!(asbx.sbx(), -50);

        let jmp = Instruction::jmp(Opcode::JMP, -10);
        assert_eq!(jmp.sbx_ab(), -10);
    }
}
