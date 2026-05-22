use std::collections::BTreeMap;

use crate::bytecode::instruction::Instruction;
use crate::bytecode::module::SourceLocation;

#[derive(Debug, Clone)]
pub struct Chunk {
    pub code: Vec<Instruction>,
    pub max_registers: u8,
    pub source_map: BTreeMap<u32, SourceLocation>,
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
}

impl Default for Chunk {
    fn default() -> Self {
        Self::new()
    }
}

