use std::collections::BTreeMap;

use crate::bytecode::instruction::Instruction;
use crate::bytecode::module::SourceLocation;

#[derive(Debug, Clone)]
pub struct Chunk {
    pub(crate) code: Vec<Instruction>,
    pub(crate) max_registers: u8,
    pub(crate) source_map: BTreeMap<u32, SourceLocation>,
}

impl Chunk {
    pub fn new() -> Self {
        Chunk {
            code: Vec::new(),
            max_registers: 0,
            source_map: BTreeMap::new(),
        }
    }

    pub fn emit(&mut self, instr: Instruction) -> usize {
        let idx = self.code.len();
        self.code.push(instr);
        idx
    }

    pub fn len(&self) -> usize {
        self.code.len()
    }

    pub fn is_empty(&self) -> bool {
        self.code.is_empty()
    }

    pub fn source_location(&self, pc: usize) -> Option<&SourceLocation> {
        self.source_map.get(&(pc as u32))
    }

    pub fn code(&self) -> &[Instruction] {
        &self.code
    }

    pub fn code_mut(&mut self) -> &mut Vec<Instruction> {
        &mut self.code
    }

    pub fn max_registers(&self) -> u8 {
        self.max_registers
    }

    pub fn set_max_registers(&mut self, n: u8) {
        self.max_registers = n;
    }

    pub fn source_map(&self) -> &BTreeMap<u32, SourceLocation> {
        &self.source_map
    }

    pub fn source_map_mut(&mut self) -> &mut BTreeMap<u32, SourceLocation> {
        &mut self.source_map
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
    use crate::bytecode::opcode::Opcode;

    #[test]
    fn chunk_starts_empty() {
        let chunk = Chunk::new();
        assert!(chunk.is_empty());
        assert_eq!(chunk.max_registers, 0);
    }

    #[test]
    fn emit_appends_instruction() {
        let mut chunk = Chunk::new();
        let idx = chunk.emit(Instruction::abc(Opcode::ADD_I64, 0, 1, 2));
        assert_eq!(idx, 0);
        assert_eq!(chunk.len(), 1);
    }
}
