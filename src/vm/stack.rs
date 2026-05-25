use std::rc::Rc;

use crate::bytecode::{Chunk, Instruction};
use crate::vm::register::{Register, VmValue};
use crate::vm::value::{self, ClosureData, TypeMask, Value};

pub const MAX_REGISTERS: u8 = 64;
pub const MAX_STACK_DEPTH: usize = 256;

pub struct Frame {
    pub(crate) chunk: Rc<Chunk>,
    pub(crate) code_ptr: *const Instruction,
    pub(crate) code_len: usize,
    pub(crate) pc: usize,

    // OLD SYSTEM (gradually being migrated away)
    pub(crate) registers: Box<[VmValue]>,
    pub(crate) scalar_regs: Box<[u64]>,

    // NEW SYSTEM (side-band tagged)
    pub(crate) raw_regs: Box<[u64]>,
    pub(crate) reg_types: TypeMask,

    pub(crate) return_base: Option<u8>,
    pub(crate) expected_returns: u8,
    pub(crate) function_name: String,
    pub(crate) handlers: Vec<u32>,
}

impl Frame {
    pub fn entry(chunk: Rc<Chunk>, registers: Box<[VmValue]>, name: impl Into<String>) -> Self {
        Self::new(chunk, registers, None, 0, name)
    }

    pub fn call(chunk: Rc<Chunk>, registers: Box<[VmValue]>, return_base: u8, expected_returns: u8, name: impl Into<String>) -> Self {
        Self::new(chunk, registers, Some(return_base), expected_returns, name)
    }

    pub fn set_chunk(&mut self, chunk: Rc<Chunk>) {
        self.code_ptr = chunk.code.as_ptr();
        self.code_len = chunk.code.len();
        self.chunk = chunk;
    }

    fn new(chunk: Rc<Chunk>, registers: Box<[VmValue]>, return_base: Option<u8>, expected_returns: u8, name: impl Into<String>) -> Self {
        let code_ptr = chunk.code.as_ptr();
        let code_len = chunk.code.len();
        let reg_count = registers.len();
        Frame {
            chunk,
            code_ptr,
            code_len,
            pc: 0,
            registers,
            scalar_regs: vec![0u64; reg_count].into_boxed_slice(),
            raw_regs: vec![0u64; reg_count].into_boxed_slice(),
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

    // ── Old system accessors (VmValue) ──

    pub fn get(&self, idx: u8) -> Option<&VmValue> {
        self.registers.get(idx as usize)
    }

    #[inline(always)]
    pub unsafe fn get_unchecked(&self, idx: u8) -> &VmValue {
        unsafe { self.registers.get_unchecked(idx as usize) }
    }

    pub fn set(&mut self, idx: u8, value: VmValue) -> bool {
        if let Some(slot) = self.registers.get_mut(idx as usize) {
            // Sync old scalar_regs
            if let VmValue::Scalar(r) = &value {
                if let Some(s) = self.scalar_regs.get_mut(idx as usize) {
                    *s = unsafe { r.bits };
                }
                // Also sync new raw_regs
                if let Some(s) = self.raw_regs.get_mut(idx as usize) {
                    *s = unsafe { r.bits };
                }
            } else {
                if let Some(s) = self.scalar_regs.get_mut(idx as usize) { *s = 0; }
                if let Some(s) = self.raw_regs.get_mut(idx as usize) { *s = 0; }
            }
            *slot = value;
            true
        } else {
            false
        }
    }

    #[inline(always)]
    pub unsafe fn set_unchecked(&mut self, idx: u8, value: VmValue) {
        if let VmValue::Scalar(r) = &value {
            unsafe { *self.scalar_regs.get_unchecked_mut(idx as usize) = r.bits; }
            unsafe { *self.raw_regs.get_unchecked_mut(idx as usize) = r.bits; }
        } else {
            unsafe { *self.scalar_regs.get_unchecked_mut(idx as usize) = 0; }
            unsafe { *self.raw_regs.get_unchecked_mut(idx as usize) = 0; }
        }
        unsafe { *self.registers.get_unchecked_mut(idx as usize) = value; }
    }

    pub fn get_mut(&mut self, idx: u8) -> Option<&mut VmValue> {
        self.registers.get_mut(idx as usize)
    }

    #[inline(always)]
    pub unsafe fn get_mut_unchecked(&mut self, idx: u8) -> &mut VmValue {
        unsafe { self.registers.get_unchecked_mut(idx as usize) }
    }

    pub fn get_scalar(&self, idx: u8) -> Option<Register> {
        self.registers.get(idx as usize).and_then(|v| v.as_scalar())
    }

    // ── New system accessors (tagged u64) ──

    #[inline]
    pub fn raw_get(&self, idx: u8) -> u64 {
        unsafe { *self.raw_regs.get_unchecked(idx as usize) }
    }

    #[inline]
    pub fn raw_set(&mut self, idx: u8, value: u64, tag: u8) {
        unsafe {
            *self.raw_regs.get_unchecked_mut(idx as usize) = value;
        }
        self.reg_types.set(idx, tag);
    }

    #[inline]
    pub fn raw_tag(&self, idx: u8) -> u8 {
        self.reg_types.get(idx)
    }

    // ── Roots ──

    pub fn collect_roots(&self) -> Vec<usize> {
        let mut roots = Vec::new();
        for reg in self.registers.iter() {
            if let VmValue::Scalar(r) = reg {
                let ptr = unsafe { r.ptr };
                if ptr != 0 { roots.push(ptr); }
            }
            if let VmValue::Closure(data) = reg {
                let upvalues = unsafe { &*data.upvalues.get() };
                for uv in upvalues.iter() {
                    if let VmValue::Scalar(r) = uv {
                        let ptr = unsafe { r.ptr };
                        if ptr != 0 { roots.push(ptr); }
                    }
                }
            }
        }
        roots
    }

    pub fn register_count(&self) -> usize { self.registers.len() }
    pub fn register_value(&self, idx: u8) -> Option<&VmValue> { self.get(idx) }
    pub fn push_handler(&mut self, handler_pc: u32) { self.handlers.push(handler_pc); }
    pub fn pop_handler(&mut self) -> Option<u32> { self.handlers.pop() }
    pub fn current_handler(&self) -> Option<u32> { self.handlers.last().copied() }
}

pub struct CallStack {
    frames: Vec<Frame>,
    register_pool: Vec<Box<[VmValue]>>,
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

    fn acquire_registers(&mut self, count: usize) -> Box<[VmValue]> {
        if let Some(pos) = self.register_pool.iter().position(|r| r.len() == count) {
            let mut regs = self.register_pool.swap_remove(pos);
            for r in regs.iter_mut() { *r = VmValue::zero(); }
            regs
        } else {
            vec![VmValue::zero(); count].into_boxed_slice()
        }
    }
}

impl Default for CallStack {
    fn default() -> Self { Self::new() }
}
