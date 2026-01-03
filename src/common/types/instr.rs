use crate::common::types::opcode::Opcode;

type Operands = [u8; 3];

#[derive(Debug)]
pub struct Instruction {
    opcode: Opcode,
    operands: Operands, // really don't know the best option for this
}

impl Instruction {
    pub fn new(opcode: Opcode, operands: Operands) -> Self {
        Instruction { opcode, operands }
    }

    pub fn get_opcode(self) -> Opcode {
        return self.opcode;
    }

    pub fn get_operands(self) -> Operands {
        return self.operands;
    }
}
