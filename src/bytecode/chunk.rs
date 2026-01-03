use crate::common::types::instr::Instruction;

pub struct Chunk {
    pub code: Vec<Instruction>,
    pub constants: Vec<usize>,
    pub max_registers: u8,
}
