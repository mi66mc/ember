use std::rc::Rc;

use crate::bytecode::Chunk;
use crate::vm::register::{Register, VmValue};

pub struct Frame {
    pub chunk: Rc<Chunk>,
    pub pc: usize,
    pub registers: Box<[VmValue]>,
    pub return_base: Option<u8>,
    pub expected_returns: u8,
    pub function_name: String,
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
        }
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

    pub fn get_scalar(&self, idx: u8) -> Option<Register> {
        self.get(idx).and_then(VmValue::as_scalar)
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

