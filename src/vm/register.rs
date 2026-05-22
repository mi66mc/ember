// ┌────────────────────────────────────────────────────────────┐
// │                      register (64 bits)                    │
// ├────────────────────────────────────────────────────────────┤
// │  i8 │ i16 │ i32 │       i64       │ <- signed integers     │
// │  u8 │ u16 │ u32 │       u64       │ <- unsigned integers   │
// │          f32    │       f64       │ <- floats              │
// │                 │       ptr       │ <- pointer             │
// └────────────────────────────────────────────────────────────┘

use std::rc::Rc;

use crate::bytecode::Chunk;
use crate::vm::native::ImportIndex;

#[derive(Clone, Copy)]
#[repr(C)]
pub union Register {
    pub i8: i8,
    pub i16: i16,
    pub i32: i32,
    pub i64: i64,
    pub u8: u8,
    pub u16: u16,
    pub u32: u32,
    pub u64: u64,
    pub f32: f32,
    pub f64: f64,
    pub ptr: usize,
    pub bits: u64,
}

impl Register {
    pub fn zero() -> Self {
        Register { bits: 0 }
    }

    pub fn from_i8(v: i8) -> Self {
        Register { i8: v }
    }
    pub fn from_i16(v: i16) -> Self {
        Register { i16: v }
    }
    pub fn from_i32(v: i32) -> Self {
        Register { i32: v }
    }
    pub fn from_i64(v: i64) -> Self {
        Register { i64: v }
    }

    pub fn from_u8(v: u8) -> Self {
        Register { u8: v }
    }
    pub fn from_u16(v: u16) -> Self {
        Register { u16: v }
    }
    pub fn from_u32(v: u32) -> Self {
        Register { u32: v }
    }
    pub fn from_u64(v: u64) -> Self {
        Register { u64: v }
    }

    pub fn from_f32(v: f32) -> Self {
        Register { f32: v }
    }
    pub fn from_f64(v: f64) -> Self {
        Register { f64: v }
    }

    pub fn from_ptr(v: usize) -> Self {
        Register { ptr: v }
    }
    pub fn from_bool(v: bool) -> Self {
        Register { u64: v as u64 }
    }
}

impl Default for Register {
    fn default() -> Self {
        Self::zero()
    }
}

impl std::fmt::Debug for Register {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Register(0x{:016x})", unsafe { self.bits })
    }
}

#[derive(Clone, Debug)]
pub enum VmValue {
    Scalar(Register),
    Function(Rc<Chunk>),
    NativeImport(ImportIndex),
}

impl VmValue {
    pub fn scalar(register: Register) -> Self {
        VmValue::Scalar(register)
    }

    pub fn function(chunk: Rc<Chunk>) -> Self {
        VmValue::Function(chunk)
    }

    pub fn native_import(index: ImportIndex) -> Self {
        VmValue::NativeImport(index)
    }

    pub fn zero() -> Self {
        VmValue::Scalar(Register::zero())
    }

    pub fn as_scalar(&self) -> Option<Register> {
        match self {
            VmValue::Scalar(register) => Some(*register),
            VmValue::Function(_) | VmValue::NativeImport(_) => None,
        }
    }

    pub fn as_function(&self) -> Option<Rc<Chunk>> {
        match self {
            VmValue::Scalar(_) | VmValue::NativeImport(_) => None,
            VmValue::Function(chunk) => Some(chunk.clone()),
        }
    }

    pub fn as_native_import(&self) -> Option<ImportIndex> {
        match self {
            VmValue::NativeImport(index) => Some(*index),
            VmValue::Scalar(_) | VmValue::Function(_) => None,
        }
    }
}

impl Default for VmValue {
    fn default() -> Self {
        Self::zero()
    }
}

