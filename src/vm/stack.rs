use crate::bytecode::chunk::Chunk;

pub struct Frame {
    pub chunk: Chunk,
    pub pc: usize,
    pub registers: Vec<usize>,
}

pub struct CallStack {
    frames: Vec<Frame>,
}
