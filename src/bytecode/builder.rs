use crate::bytecode::{Chunk, Constant, Instruction, Opcode};

#[derive(Debug, Default)]
pub struct Builder {
    chunk: Chunk,
}

impl Builder {
    pub fn new(max_registers: u8) -> Self {
        let mut chunk = Chunk::new();
        chunk.max_registers = max_registers;
        Self { chunk }
    }

    pub fn constant(&mut self, value: Constant) -> u16 {
        self.chunk.add_constant(value)
    }

    pub fn proto(&mut self, chunk: Chunk) -> u16 {
        self.chunk.add_proto(chunk)
    }

    pub fn emit(&mut self, instruction: Instruction) -> usize {
        self.chunk.emit(instruction)
    }

    pub fn label(&self) -> usize {
        self.chunk.len()
    }

    pub fn jump_placeholder(&mut self, opcode: Opcode, register: u8) -> usize {
        self.emit(Instruction::asbx(opcode, register, 0))
    }

    pub fn patch_jump(&mut self, at: usize, target: usize) {
        let instruction = self.chunk.code[at];
        self.chunk.code[at] = Instruction::asbx(
            instruction.opcode(),
            instruction.a(),
            (target as isize - at as isize) as i16,
        );
    }

    pub fn finish(self) -> Chunk {
        self.chunk
    }
}
