use std::rc::Rc;

use crate::bytecode::Chunk;
use crate::vm::register::{Register, VmValue};

pub const MAX_REGISTERS: u8 = 64;

pub struct Frame {
    pub(crate) chunk: Rc<Chunk>,
    pub(crate) pc: usize,
    pub(crate) registers: Box<[VmValue]>,
    pub(crate) return_base: Option<u8>,
    pub(crate) expected_returns: u8,
    pub(crate) function_name: String,
    pub(crate) handlers: Vec<u32>,
}

impl Frame {
    pub fn entry(chunk: Rc<Chunk>, name: impl Into<String>) -> Self {
        Self::new(chunk, None, 0, name)
    }

    pub fn call(chunk: Rc<Chunk>, return_base: u8, expected_returns: u8, name: impl Into<String>) -> Self {
        Self::new(chunk, Some(return_base), expected_returns, name)
    }

    fn new(chunk: Rc<Chunk>, return_base: Option<u8>, expected_returns: u8, name: impl Into<String>) -> Self {
        let num_regs = chunk.max_registers as usize;
        Frame {
            chunk,
            pc: 0,
            registers: vec![VmValue::default(); num_regs].into_boxed_slice(),
            return_base,
            expected_returns,
            function_name: name.into(),
            handlers: Vec::new(),
        }
    }

    pub fn pc(&self) -> usize {
        self.pc
    }

    pub fn function_name(&self) -> &str {
        &self.function_name
    }

    pub fn chunk(&self) -> &Rc<Chunk> {
        &self.chunk
    }

    pub fn get(&self, idx: u8) -> Option<&VmValue> {
        self.registers.get(idx as usize)
    }

    pub fn set(&mut self, idx: u8, value: VmValue) -> bool {
        if let Some(slot) = self.registers.get_mut(idx as usize) {
            *slot = value;
            true
        } else {
            false
        }
    }

    pub fn get_mut(&mut self, idx: u8) -> Option<&mut VmValue> {
        self.registers.get_mut(idx as usize)
    }

    pub fn get_scalar(&self, idx: u8) -> Option<Register> {
        self.get(idx).and_then(VmValue::as_scalar)
    }

    pub fn push_handler(&mut self, handler_pc: u32) {
        self.handlers.push(handler_pc);
    }

    pub fn pop_handler(&mut self) -> Option<u32> {
        self.handlers.pop()
    }

    pub fn current_handler(&self) -> Option<u32> {
        self.handlers.last().copied()
    }

    pub fn collect_roots(&self) -> Vec<usize> {
        let mut roots = Vec::new();
        for value in self.registers.iter() {
            if let VmValue::Scalar(register) = value {
                let ptr = unsafe { register.ptr };
                if ptr != 0 {
                    roots.push(ptr);
                }
            }
        }
        roots
    }
}

pub struct CallStack {
    frames: Vec<Frame>,
}

impl CallStack {
    pub fn new() -> Self {
        CallStack { frames: Vec::new() }
    }

    pub fn push_entry(&mut self, chunk: Rc<Chunk>, name: impl Into<String>) {
        self.frames.push(Frame::entry(chunk, name));
    }

    pub fn push_call(&mut self, chunk: Rc<Chunk>, return_base: u8, expected_returns: u8, name: impl Into<String>) {
        self.frames
            .push(Frame::call(chunk, return_base, expected_returns, name));
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

    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
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
            max_registers: max_regs,
            source_map: std::collections::BTreeMap::new(),
            exception_table: Vec::new(),
        })
    }

    #[test]
    fn push_pop_frame() {
        let mut stack = CallStack::new();
        stack.push_entry(make_chunk(4), "entry");
        stack.push_call(make_chunk(2), 0, 1, "child");
        assert_eq!(stack.depth(), 2);
        assert_eq!(stack.pop_frame().unwrap().return_base, Some(0));
        assert_eq!(stack.depth(), 1);
    }

    #[test]
    fn frame_register_access_is_checked() {
        let mut frame = Frame::entry(make_chunk(1), "test");
        assert!(frame.set(0, VmValue::scalar(Register::from_i64(42))));
        assert!(!frame.set(1, VmValue::scalar(Register::from_i64(100))));
        unsafe {
            assert_eq!(frame.get_scalar(0).unwrap().i64, 42);
        }
        assert!(frame.get_scalar(1).is_none());
    }
}
