use std::rc::Rc;

use crate::bytecode::chunk::Chunk;
use crate::vm::register::Register;

// call stack (isolated registers per frame)
//
// ┌─────────────────────────────────────────────────────────┐
// │ frame 0 (main)     │ regs: [R0, R1, R2, ...]            │
// ├────────────────────┴────────────────────────────────────┤
// │ frame 1 (func a)   │ regs: [R0, R1, ...]   (isolated)   │
// ├────────────────────┴────────────────────────────────────┤
// │ frame 2 (func b)   │ regs: [R0, R1, R2, R3] (isolated)  │
// └─────────────────────────────────────────────────────────┘
//                      ↑ current frame (top)

pub struct Frame {
    pub chunk: Rc<Chunk>,
    pub pc: usize,
    pub registers: Vec<Register>, // each frame owns its registers
}

impl Frame {
    pub fn new(chunk: Rc<Chunk>) -> Self {
        let num_regs = chunk.max_registers as usize;
        Frame {
            chunk,
            pc: 0,
            registers: vec![Register::zero(); num_regs],
        }
    }

    #[inline]
    pub fn get_reg(&self, idx: u8) -> Register {
        self.registers[idx as usize]
    }

    #[inline]
    pub fn set_reg(&mut self, idx: u8, val: Register) {
        self.registers[idx as usize] = val;
    }
}

pub struct CallStack {
    frames: Vec<Frame>,
}

impl CallStack {
    pub fn new() -> Self {
        CallStack { frames: Vec::new() }
    }

    pub fn push_frame(&mut self, chunk: Rc<Chunk>) {
        self.frames.push(Frame::new(chunk));
    }

    pub fn pop_frame(&mut self) -> Option<Frame> {
        self.frames.pop()
    }

    pub fn current(&self) -> Option<&Frame> {
        self.frames.last()
    }

    pub fn current_mut(&mut self) -> Option<&mut Frame> {
        self.frames.last_mut()
    }

    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    // ─────────────────────────────────────────
    // shortcuts to current frame
    // ─────────────────────────────────────────

    #[inline]
    pub fn get_reg(&self, idx: u8) -> Register {
        self.current().map(|f| f.get_reg(idx)).unwrap_or(Register::zero())
    }

    #[inline]
    pub fn set_reg(&mut self, idx: u8, val: Register) {
        if let Some(frame) = self.current_mut() {
            frame.set_reg(idx, val);
        }
    }

    pub fn pc(&self) -> usize {
        self.current().map(|f| f.pc).unwrap_or(0)
    }

    pub fn set_pc(&mut self, pc: usize) {
        if let Some(frame) = self.current_mut() {
            frame.pc = pc;
        }
    }

    pub fn advance_pc(&mut self) {
        if let Some(frame) = self.current_mut() {
            frame.pc += 1;
        }
    }

    pub fn jump(&mut self, offset: i16) {
        if let Some(frame) = self.current_mut() {
            frame.pc = (frame.pc as isize + offset as isize) as usize;
        }
    }
}

impl Default for CallStack {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_chunk(max_regs: u8) -> Rc<Chunk> {
        Rc::new(Chunk {
            code: Vec::new(),
            constants: Vec::new(),
            max_registers: max_regs,
        })
    }

    #[test]
    fn test_push_pop_frame() {
        let mut stack = CallStack::new();
        assert!(stack.is_empty());

        stack.push_frame(make_chunk(4));
        assert_eq!(stack.depth(), 1);

        stack.push_frame(make_chunk(2));
        assert_eq!(stack.depth(), 2);

        stack.pop_frame();
        assert_eq!(stack.depth(), 1);

        stack.pop_frame();
        assert!(stack.is_empty());
    }

    #[test]
    fn test_register_access() {
        let mut stack = CallStack::new();
        stack.push_frame(make_chunk(4));

        stack.set_reg(0, Register::from_i64(42));
        stack.set_reg(1, Register::from_i64(100));

        unsafe {
            assert_eq!(stack.get_reg(0).i64, 42);
            assert_eq!(stack.get_reg(1).i64, 100);
        }
    }

    #[test]
    fn test_frame_isolation() {
        let mut stack = CallStack::new();

        // frame 0: set R0 = 111
        stack.push_frame(make_chunk(2));
        stack.set_reg(0, Register::from_i64(111));

        // frame 1: set R0 = 222 (different register!)
        stack.push_frame(make_chunk(2));
        stack.set_reg(0, Register::from_i64(222));

        unsafe {
            assert_eq!(stack.get_reg(0).i64, 222);
        }

        // pop frame 1
        stack.pop_frame();

        // back to frame 0, R0 still 111 (isolated!)
        unsafe {
            assert_eq!(stack.get_reg(0).i64, 111);
        }
    }

    #[test]
    fn test_pc_manipulation() {
        let mut stack = CallStack::new();
        stack.push_frame(make_chunk(2));

        assert_eq!(stack.pc(), 0);

        stack.advance_pc();
        assert_eq!(stack.pc(), 1);

        stack.jump(-1);
        assert_eq!(stack.pc(), 0);

        stack.jump(5);
        assert_eq!(stack.pc(), 5);
    }
}
