use std::rc::Rc;

use crate::bytecode::{Chunk, Instruction};
use crate::vm::register::{Register, VmValue};

pub const MAX_REGISTERS: u8 = 64;

pub struct Frame {
    pub(crate) chunk: Rc<Chunk>,
    pub(crate) code_ptr: *const Instruction,
    pub(crate) code_len: usize,
    pub(crate) pc: usize,
    pub(crate) registers: Box<[VmValue]>,
    pub(crate) return_base: Option<u8>,
    pub(crate) expected_returns: u8,
    pub(crate) function_name: String,
    pub(crate) handlers: Vec<u32>,
    pub(crate) root_mask: u64,
}

impl Frame {
    pub fn entry(chunk: Rc<Chunk>, name: impl Into<String>) -> Self {
        Self::new(chunk, None, 0, name)
    }

    pub fn call(chunk: Rc<Chunk>, return_base: u8, expected_returns: u8, name: impl Into<String>) -> Self {
        Self::new(chunk, Some(return_base), expected_returns, name)
    }

    pub fn set_chunk(&mut self, chunk: Rc<Chunk>) {
        self.code_ptr = chunk.code.as_ptr();
        self.code_len = chunk.code.len();
        self.chunk = chunk;
    }

    fn new(chunk: Rc<Chunk>, return_base: Option<u8>, expected_returns: u8, name: impl Into<String>) -> Self {
        let num_regs = chunk.max_registers as usize;
        let code_ptr = chunk.code.as_ptr();
        let code_len = chunk.code.len();
        Frame {
            chunk,
            code_ptr,
            code_len,
            pc: 0,
            registers: vec![VmValue::default(); num_regs].into_boxed_slice(),
            return_base,
            expected_returns,
            function_name: name.into(),
            handlers: Vec::new(),
            root_mask: 0,
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

    /// SAFETY: caller must guarantee idx < self.registers.len().
    /// Guaranteed by the bytecode validator at compile time.
    #[inline(always)]
    pub unsafe fn get_unchecked(&self, idx: u8) -> &VmValue {
        self.registers.get_unchecked(idx as usize)
    }

    pub fn set(&mut self, idx: u8, value: VmValue) -> bool {
        if let Some(slot) = self.registers.get_mut(idx as usize) {
            *slot = value;
            true
        } else {
            false
        }
    }

    /// SAFETY: caller must guarantee idx < self.registers.len().
    /// Guaranteed by the bytecode validator at compile time.
    #[inline(always)]
    pub unsafe fn set_unchecked(&mut self, idx: u8, value: VmValue) {
        *self.registers.get_unchecked_mut(idx as usize) = value;
    }

    pub fn get_mut(&mut self, idx: u8) -> Option<&mut VmValue> {
        self.registers.get_mut(idx as usize)
    }

    /// SAFETY: caller must guarantee idx < self.registers.len().
    /// Guaranteed by the bytecode validator at compile time.
    #[inline(always)]
    pub unsafe fn get_mut_unchecked(&mut self, idx: u8) -> &mut VmValue {
        self.registers.get_unchecked_mut(idx as usize)
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

    pub fn register_count(&self) -> usize {
        self.registers.len()
    }

    pub fn register_value(&self, idx: u8) -> Option<&VmValue> {
        self.registers.get(idx as usize)
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

    /// SAFETY: caller must guarantee the stack is non-empty.
    #[inline(always)]
    pub unsafe fn current_unchecked(&self) -> &Frame {
        self.frames.get_unchecked(self.frames.len() - 1)
    }

    pub fn current_mut(&mut self) -> Option<&mut Frame> {
        self.frames.last_mut()
    }

    /// SAFETY: caller must guarantee the stack is non-empty.
    #[inline(always)]
    pub unsafe fn current_mut_unchecked(&mut self) -> &mut Frame {
        let len = self.frames.len();
        self.frames.get_unchecked_mut(len - 1)
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
