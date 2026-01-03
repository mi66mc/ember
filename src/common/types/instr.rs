use crate::common::types::opcode::Opcode;

#[derive(Debug)]
pub struct Instruction {
    opcode: Opcode,
    operands: [u8; 3], // really don't know the best option for this
}
