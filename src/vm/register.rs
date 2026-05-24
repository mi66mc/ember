// ┌────────────────────────────────────────────────────────────┐
// │                      register (64 bits)                    │
// ├────────────────────────────────────────────────────────────┤
// │  i8 │ i16 │ i32 │       i64       │ <- signed integers     │
// │  u8 │ u16 │ u32 │       u64       │ <- unsigned integers   │
// │          f32    │       f64       │ <- floats              │
// │                 │       ptr       │ <- pointer             │
// └────────────────────────────────────────────────────────────┘
//
// # Safety
//
// `Register` is a union whose fields all occupy the same 64-bit storage.
// The safe-usage contract is:
//
// 1. A `Register` can be safely read via any named field whose type matches
//    the last write. The bytecode compiler guarantees this by emitting typed
//    opcodes (e.g. ADD_I64 produces operands that were written by from_i64).
//
// 2. Every `from_*` constructor produces a valid bit pattern for its
//    respective type, so reading the same field back is always sound.
//
// 3. Reading via `bits: u64` is always safe regardless of which field was
//    last written, because the union is exactly 64 bits wide and `u64` is
//    valid for every possible bit pattern.

use std::cell::UnsafeCell;
use std::rc::Rc;

use crate::bytecode::Chunk;
use crate::vm::native::ImportIndex;

#[derive(Clone, Debug)]
pub struct ClosureData {
    pub chunk: Rc<Chunk>,
    pub upvalues: Rc<UnsafeCell<Vec<VmValue>>>,
}


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

impl PartialEq for Register {
    fn eq(&self, other: &Self) -> bool {
        unsafe { self.bits == other.bits }
    }
}

impl Eq for Register {}

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
    Closure(Box<ClosureData>),
}

impl PartialEq for VmValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (VmValue::Scalar(a), VmValue::Scalar(b)) => a == b,
            (VmValue::Function(a), VmValue::Function(b)) => Rc::ptr_eq(a, b),
            (VmValue::NativeImport(a), VmValue::NativeImport(b)) => a == b,
            (VmValue::Closure(a), VmValue::Closure(b)) => {
                Rc::ptr_eq(&a.chunk, &b.chunk) && unsafe { &*a.upvalues.get() == &*b.upvalues.get() }
            }
            _ => false,
        }
    }
}

impl Eq for VmValue {}

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

    pub fn closure(chunk: Rc<Chunk>, upvalues: Vec<VmValue>) -> Self {
        VmValue::Closure(Box::new(ClosureData {
            chunk,
            upvalues: Rc::new(UnsafeCell::new(upvalues)),
        }))
    }

    pub fn zero() -> Self {
        VmValue::Scalar(Register::zero())
    }

    pub fn as_scalar(&self) -> Option<Register> {
        match self {
            VmValue::Scalar(register) => Some(*register),
            VmValue::Function(_) | VmValue::NativeImport(_) | VmValue::Closure(_) => None,
        }
    }

    pub fn as_function(&self) -> Option<Rc<Chunk>> {
        match self {
            VmValue::Scalar(_) | VmValue::NativeImport(_) | VmValue::Closure(_) => None,
            VmValue::Function(chunk) => Some(chunk.clone()),
        }
    }

    pub fn as_native_import(&self) -> Option<ImportIndex> {
        match self {
            VmValue::NativeImport(index) => Some(*index),
            VmValue::Scalar(_) | VmValue::Function(_) | VmValue::Closure(_) => None,
        }
    }

    pub fn as_closure(&self) -> Option<&ClosureData> {
        match self {
            VmValue::Closure(data) => Some(data),
            _ => None,
        }
    }

    /// SAFETY: caller must guarantee this VmValue is the Scalar variant.
    /// Returns the raw u64 bits of the contained Register without branching.
    #[inline(always)]
    pub unsafe fn scalar_bits_unchecked(&self) -> u64 {
        match self {
            VmValue::Scalar(r) => unsafe { r.bits },
            _ => unsafe { std::hint::unreachable_unchecked() },
        }
    }
}

impl Default for VmValue {
    fn default() -> Self {
        Self::zero()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_size() {
        assert_eq!(size_of::<Register>(), 8);
    }

    #[test]
    fn test_register_zero() {
        let r = Register::zero();
        unsafe {
            assert_eq!(r.bits, 0);
            assert_eq!(r.i64, 0);
            assert_eq!(r.f64, 0.0);
        }
    }

    #[test]
    fn test_register_i64() {
        let r = Register::from_i64(-42);
        unsafe {
            assert_eq!(r.i64, -42);
        }
    }

    #[test]
    fn test_register_f64() {
        let r = Register::from_f64(1.25);
        unsafe {
            assert_eq!(r.f64, 1.25);
        }
    }

    #[test]
    fn test_register_bool() {
        let t = Register::from_bool(true);
        let f = Register::from_bool(false);
        unsafe {
            assert_eq!(t.u64, 1);
            assert_eq!(f.u64, 0);
        }
    }

    #[test]
    fn test_register_overlap() {
        let r = Register::from_i64(0x0102030405060708);
        unsafe {
            assert_eq!(r.i8, 0x08);
            assert_eq!(r.i16, 0x0708);
            assert_eq!(r.i32, 0x05060708);
        }
    }
}
