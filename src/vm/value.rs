// Side-band tagged value system.
//
// Each register slot holds a raw u64. The type (scalar, function, native import,
// or closure) is stored in a separate 2-bit-per-register bitmap. This eliminates
// the VmValue enum entirely while preserving full 64-bit scalar precision.
//
// Layout per Frame:
//   registers: Box<[u64]>   — 8 bytes per slot, raw bits
//   reg_types: u128         — 2 bits per slot, up to 64 registers
//
// Type tags (2 bits):
//   00 = Scalar      — raw u64 (i64, f64, u64, ptr, bool, etc.)
//   01 = Function    — index into module.functions, stored as u32 in lower bits
//   10 = NativeImport — packed (module: u16 << 16) | function: u16
//   11 = Closure     — pointer to Box<ClosureData> on the heap

use std::rc::Rc;
use std::cell::UnsafeCell;
use crate::bytecode::Chunk;

pub mod tag {
    pub const SCALAR: u8 = 0b00;
    pub const FUNCTION: u8 = 0b01;
    pub const NATIVE_IMPORT: u8 = 0b10;
    pub const CLOSURE: u8 = 0b11;
}

#[derive(Clone, Copy, Default)]
#[repr(transparent)]
pub struct Value(pub u64);

impl Value {
    // ── Scalar constructors ──

    #[inline] pub fn scalar(bits: u64) -> Self { Value(bits) }
    #[inline] pub fn from_i8(v: i8) -> Self { Value(v as i64 as u64) }
    #[inline] pub fn from_i16(v: i16) -> Self { Value(v as i64 as u64) }
    #[inline] pub fn from_i32(v: i32) -> Self { Value(v as i64 as u64) }
    #[inline] pub fn from_i64(v: i64) -> Self { Value(v as u64) }
    #[inline] pub fn from_u8(v: u8) -> Self { Value(v as u64) }
    #[inline] pub fn from_u16(v: u16) -> Self { Value(v as u64) }
    #[inline] pub fn from_u32(v: u32) -> Self { Value(v as u64) }
    #[inline] pub fn from_u64(v: u64) -> Self { Value(v) }
    #[inline] pub fn from_f32(v: f32) -> Self { Value(v.to_bits() as u64) }
    #[inline] pub fn from_f64(v: f64) -> Self { Value(v.to_bits()) }
    #[inline] pub fn from_bool(v: bool) -> Self { Value(v as u64) }
    #[inline] pub fn from_ptr(v: usize) -> Self { Value(v as u64) }

    // ── Non-scalar constructors ──

    #[inline] pub fn function(idx: u32) -> Self { Value(idx as u64) }
    #[inline] pub fn native_import(module: u16, function: u16) -> Self {
        Value(((module as u64) << 16) | function as u64)
    }
    #[inline] pub fn closure(ptr: *const ClosureData) -> Self { Value(ptr as u64) }
    pub fn nil() -> Self { Value(u64::MAX) }

    // ── Scalar accessors (caller must know type) ──

    #[inline] pub fn bits(self) -> u64 { self.0 }
    #[inline] pub fn i8(self) -> i8 { self.0 as i8 }
    #[inline] pub fn i16(self) -> i16 { self.0 as i16 }
    #[inline] pub fn i32(self) -> i32 { self.0 as i32 }
    #[inline] pub fn i64(self) -> i64 { self.0 as i64 }
    #[inline] pub fn u8(self) -> u8 { self.0 as u8 }
    #[inline] pub fn u16(self) -> u16 { self.0 as u16 }
    #[inline] pub fn u32(self) -> u32 { self.0 as u32 }
    #[inline] pub fn u64(self) -> u64 { self.0 }
    #[inline] pub fn f32(self) -> f32 { f32::from_bits(self.0 as u32) }
    #[inline] pub fn f64(self) -> f64 { f64::from_bits(self.0) }
    #[inline] pub fn ptr(self) -> usize { self.0 as usize }
    #[inline] pub fn bool(self) -> bool { self.0 != 0 }

    // ── Non-scalar accessors ──

    #[inline] pub fn function_idx(self) -> u32 { self.0 as u32 }
    #[inline] pub fn import_module(self) -> u16 { (self.0 >> 16) as u16 }
    #[inline] pub fn import_function(self) -> u16 { self.0 as u16 }

    /// SAFETY: only call when tag == CLOSURE
    #[inline] pub unsafe fn closure_ref(self) -> &ClosureData {
        &*(self.0 as *const ClosureData)
    }
}

#[derive(Clone, Copy)]
pub struct TypeMask(pub(crate) u128);

impl TypeMask {
    pub fn new() -> Self { Self(0) }

    #[inline]
    pub fn get(&self, idx: u8) -> u8 {
        ((self.0 >> (idx as u32 * 2)) & 0b11) as u8
    }

    #[inline]
    pub fn set(&mut self, idx: u8, tag: u8) {
        let shift = idx as u32 * 2;
        self.0 = (self.0 & !(0b11u128 << shift)) | ((tag as u128 & 0b11) << shift);
    }
}

// ── Keep ClosureData for heap-allocated closures ──

#[derive(Debug, Clone)]
pub struct ClosureData {
    pub chunk: Rc<Chunk>,
    pub upvalues: Rc<UnsafeCell<Vec<u64>>>,  // raw u64 values
    pub upvalue_types: u128,                  // type info for upvalues
}

// ── Keep Register for backward compat with memory.rs ──

use std::fmt;

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
    pub fn zero() -> Self { Register { bits: 0 } }
    pub fn from_i8(v: i8) -> Self { Register { i8: v } }
    pub fn from_i16(v: i16) -> Self { Register { i16: v } }
    pub fn from_i32(v: i32) -> Self { Register { i32: v } }
    pub fn from_i64(v: i64) -> Self { Register { i64: v } }
    pub fn from_u8(v: u8) -> Self { Register { u8: v } }
    pub fn from_u16(v: u16) -> Self { Register { u16: v } }
    pub fn from_u32(v: u32) -> Self { Register { u32: v } }
    pub fn from_u64(v: u64) -> Self { Register { u64: v } }
    pub fn from_f32(v: f32) -> Self { Register { f32: v } }
    pub fn from_f64(v: f64) -> Self { Register { f64: v } }
    pub fn from_ptr(v: usize) -> Self { Register { ptr: v } }
    pub fn from_bool(v: bool) -> Self { Register { u64: v as u64 } }
}

impl Default for Register {
    fn default() -> Self { Self::zero() }
}

impl fmt::Debug for Register {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Register(0x{:016x})", unsafe { self.bits })
    }
}

// ── Keep VmValue for gradual migration (will be removed) ──

use crate::vm::native::ImportIndex;

#[derive(Clone, Debug)]
pub enum VmValue {
    Scalar(Register),
    Function(Rc<Chunk>),
    NativeImport(ImportIndex),
    ClosureLegacy(Box<ClosureData>),
}

impl VmValue {
    pub fn scalar(register: Register) -> Self { VmValue::Scalar(register) }
    pub fn function(chunk: Rc<Chunk>) -> Self { VmValue::Function(chunk) }
    pub fn native_import(index: ImportIndex) -> Self { VmValue::NativeImport(index) }
    pub fn closure(chunk: Rc<Chunk>, upvalues: Vec<VmValue>) -> Self {
        VmValue::ClosureLegacy(Box::new(ClosureData {
            chunk,
            upvalues: Rc::new(UnsafeCell::new(Vec::new())),
            upvalue_types: 0,
        }))
    }
    pub fn zero() -> Self { VmValue::Scalar(Register::zero()) }
    pub fn as_scalar(&self) -> Option<Register> {
        match self { VmValue::Scalar(r) => Some(*r), _ => None }
    }
    pub fn as_function(&self) -> Option<Rc<Chunk>> {
        match self { VmValue::Function(c) => Some(c.clone()), _ => None }
    }
    pub fn as_native_import(&self) -> Option<ImportIndex> {
        match self { VmValue::NativeImport(i) => Some(*i), _ => None }
    }
    pub fn as_closure(&self) -> Option<&ClosureData> {
        match self { VmValue::ClosureLegacy(d) => Some(d), _ => None }
    }
    pub fn scalar_bits_unchecked(&self) -> u64 {
        match self { VmValue::Scalar(r) => unsafe { r.bits }, _ => unsafe { std::hint::unreachable_unchecked() } }
    }
}

impl Default for VmValue {
    fn default() -> Self { Self::zero() }
}
