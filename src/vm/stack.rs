use std::rc::Rc;

use crate::bytecode::{Chunk, Instruction};
use crate::vm::value::{self, ClosureData, TypeMask, Value};

pub const MAX_REGISTERS: u8 = 64;
pub const MAX_STACK_DEPTH: usize = 256;

pub struct Frame {
    pub(crate) chunk: Rc<Chunk>,
    pub(crate) code_ptr: *const Instruction,
    pub(crate) code_len: usize,
    pub(crate) pc: usize,
    pub(crate) registers: Box<[u64]>,     // raw u64 values
    pub(crate) reg_types: TypeMask,        // 2 bits per register
    pub(crate) return_base: Option<u8>,
    pub(crate) expected_returns: u8,
    pub(crate) function_name: String,
    pub(crate) handlers: Vec<u32>,
}

impl Frame {
    pub fn entry(chunk: Rc<Chunk>, registers: Box<[u64]>, name: impl Into<String>) -> Self {
        Self::new(chunk, registers, None, 0, name)
    }

    pub fn call(chunk: Rc<Chunk>, registers: Box<[u64]>, return_base: u8, expected_returns: u8, name: impl Into<String>) -> Self {
        Self::new(chunk, registers, Some(return_base), expected_returns, name)
    }

    pub fn set_chunk(&mut self, chunk: Rc<Chunk>) {
        self.code_ptr = chunk.code.as_ptr();
        self.code_len = chunk.code.len();
        self.chunk = chunk;
    }

    fn new(chunk: Rc<Chunk>, registers: Box<[u64]>, return_base: Option<u8>, expected_returns: u8, name: impl Into<String>) -> Self {
        let code_ptr = chunk.code.as_ptr();
        let code_len = chunk.code.len();
        Frame {
            chunk,
            code_ptr,
            code_len,
            pc: 0,
            registers,
            reg_types: TypeMask::new(),
            return_base,
            expected_returns,
            function_name: name.into(),
            handlers: Vec::new(),
        }
    }

    pub fn pc(&self) -> usize { self.pc }
    pub fn function_name(&self) -> &str { &self.function_name }
    pub fn chunk(&self) -> &Rc<Chunk> { &self.chunk }

    #[inline(always)]
    pub fn get(&self, idx: u8) -> Option<u64> {
        self.registers.get(idx as usize).copied()
    }

    #[inline(always)]
    pub unsafe fn get_unchecked(&self, idx: u8) -> u64 {
        *self.registers.get_unchecked(idx as usize)
    }

    #[inline(always)]
    pub fn get_tag(&self, idx: u8) -> u8 {
        self.reg_types.get(idx)
    }

    #[inline]
    pub fn set(&mut self, idx: u8, value: u64, tag: u8) -> bool {
        if let Some(slot) = self.registers.get_mut(idx as usize) {
            *slot = value;
            self.reg_types.set(idx, tag);
            true
        } else { false }
    }

    #[inline(always)]
    pub unsafe fn set_unchecked(&mut self, idx: u8, value: u64, tag: u8) {
        *self.registers.get_unchecked_mut(idx as usize) = value;
        self.reg_types.set(idx, tag);
    }

    #[inline]
    pub fn get_mut(&mut self, idx: u8) -> Option<&mut u64> {
        self.registers.get_mut(idx as usize)
    }

    #[inline(always)]
    pub unsafe fn get_mut_unchecked(&mut self, idx: u8) -> &mut u64 {
        self.registers.get_unchecked_mut(idx as usize)
    }

    pub fn get_scalar(&self, idx: u8) -> Option<Register> {
        if self.reg_types.get(idx) == value::tag::SCALAR {
            Some(Register { bits: self.registers[idx as usize] })
        } else {
            None
        }
    }

    pub fn collect_roots(&self) -> Vec<usize> {
        let mut roots = Vec::new();
        for i in 0..self.registers.len() {
            let tag = self.reg_types.get(i as u8);
            let raw = self.registers[i];
            match tag {
                value::tag::SCALAR => {
                    if raw != 0 && raw != u64::MAX {
                        roots.push(raw as usize);
                    }
                }
                value::tag::CLOSURE => {
                    let data = unsafe { &*(raw as *const ClosureData) };
                    let upvalues = unsafe { &*data.upvalues.get() };
                    for &uv in upvalues.iter() {
                        if uv != 0 && uv != u64::MAX {
                            roots.push(uv as usize);
                        }
                    }
                }
                _ => {}
            }
        }
        roots
    }

    pub fn register_count(&self) -> usize { self.registers.len() }
    pub fn register_value(&self, idx: u8) -> Option<u64> { self.get(idx) }
    pub fn push_handler(&mut self, handler_pc: u32) { self.handlers.push(handler_pc); }
    pub fn pop_handler(&mut self) -> Option<u32> { self.handlers.pop() }
    pub fn current_handler(&self) -> Option<u32> { self.handlers.last().copied() }
}

// Re-export Register for backward compat
use crate::vm::value::Register;

pub struct CallStack {
    frames: Vec<Frame>,
    register_pool: Vec<Box<[u64]>>,
}

impl CallStack {
    pub fn new() -> Self {
        CallStack { frames: Vec::new(), register_pool: Vec::new() }
    }

    pub fn push_entry(&mut self, chunk: Rc<Chunk>, name: impl Into<String>) {
        let regs = self.acquire_registers(chunk.max_registers as usize);
        self.frames.push(Frame::entry(chunk, regs, name));
    }

    pub fn push_call(&mut self, chunk: Rc<Chunk>, return_base: u8, expected_returns: u8, name: impl Into<String>) {
        let regs = self.acquire_registers(chunk.max_registers as usize);
        self.frames.push(Frame::call(chunk, regs, return_base, expected_returns, name));
    }

    pub fn pop_frame(&mut self) -> Option<Frame> {
        let mut frame = self.frames.pop()?;
        let old_regs = std::mem::replace(&mut frame.registers, Box::new([]));
        self.register_pool.push(old_regs);
        Some(frame)
    }

    pub fn current(&self) -> Option<&Frame> { self.frames.last() }
    pub fn current_mut(&mut self) -> Option<&mut Frame> { self.frames.last_mut() }

    pub unsafe fn current_unchecked(&self) -> &Frame {
        self.frames.get_unchecked(self.frames.len().wrapping_sub(1))
    }
    pub unsafe fn current_mut_unchecked(&mut self) -> &mut Frame {
        let len = self.frames.len();
        self.frames.get_unchecked_mut(len.wrapping_sub(1))
    }

    pub fn depth(&self) -> usize { self.frames.len() }
    pub fn frames(&self) -> &[Frame] { &self.frames }
    pub fn is_empty(&self) -> bool { self.frames.is_empty() }

    pub fn advance_pc(&mut self) {
        unsafe { self.frames.last_mut().unwrap_unchecked() }.pc += 1;
    }

    pub fn jump(&mut self, offset: i16) {
        if let Some(frame) = self.current_mut() {
            frame.pc = (frame.pc as isize + offset as isize) as usize;
        }
    }

    fn acquire_registers(&mut self, count: usize) -> Box<[u64]> {
        if let Some(pos) = self.register_pool.iter().position(|r| r.len() == count) {
            let mut regs = self.register_pool.swap_remove(pos);
            for r in regs.iter_mut() { *r = 0; }
            regs
        } else {
            vec![0u64; count].into_boxed_slice()
        }
    }
}

impl Default for CallStack {
    fn default() -> Self { Self::new() }
}
