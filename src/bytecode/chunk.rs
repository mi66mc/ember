use crate::common::types::instr::Instruction;
use crate::common::types::value::Constant;

// chunk = compiled function/block
//
// ┌─────────────────────────────────────┐
// │ code: [instr, instr, instr, ...]    │  <- bytecode
// │ constants: [const, const, ...]      │  <- literal values
// │ max_registers: u8                   │  <- register count
// └─────────────────────────────────────┘

#[derive(Debug, Clone)]
pub struct Chunk {
    pub code: Vec<Instruction>,
    pub constants: Vec<Constant>,
    pub max_registers: u8,
}

impl Chunk {
    pub fn new() -> Self {
        Chunk {
            code: Vec::new(),
            constants: Vec::new(),
            max_registers: 0,
        }
    }

    // append instruction, return its index
    pub fn emit(&mut self, instr: Instruction) -> usize {
        let idx = self.code.len();
        self.code.push(instr);
        idx
    }

    // add constant, return its index
    pub fn add_constant(&mut self, val: Constant) -> u16 {
        let idx = self.constants.len();
        self.constants.push(val);
        idx as u16
    }

    pub fn len(&self) -> usize {
        self.code.len()
    }

    pub fn is_empty(&self) -> bool {
        self.code.is_empty()
    }
}

impl Default for Chunk {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::types::opcode::Opcode;

    #[test]
    fn test_chunk_new() {
        let chunk = Chunk::new();
        assert!(chunk.is_empty());
        assert_eq!(chunk.max_registers, 0);
    }

    #[test]
    fn test_chunk_emit() {
        let mut chunk = Chunk::new();
        let idx = chunk.emit(Instruction::abc(Opcode::ADD_I64, 0, 1, 2));
        assert_eq!(idx, 0);
        assert_eq!(chunk.len(), 1);
    }

    #[test]
    fn test_chunk_add_constant() {
        let mut chunk = Chunk::new();
        let idx = chunk.add_constant(Constant::I64(42));
        assert_eq!(idx, 0);
        assert_eq!(chunk.constants.len(), 1);
    }
}
