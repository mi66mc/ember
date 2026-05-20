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
use crate::vm::native::NativeFunction;

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

    // signed integers
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

    // unsigned integers
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

    // floats
    pub fn from_f32(v: f32) -> Self {
        Register { f32: v }
    }
    pub fn from_f64(v: f64) -> Self {
        Register { f64: v }
    }

    // other
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
    NativeFunction(NativeFunction),
    String(Rc<str>),
}

impl VmValue {
    pub fn scalar(register: Register) -> Self {
        VmValue::Scalar(register)
    }

    pub fn function(chunk: Rc<Chunk>) -> Self {
        VmValue::Function(chunk)
    }

    pub fn native_function(function: NativeFunction) -> Self {
        VmValue::NativeFunction(function)
    }

    pub fn string(value: impl Into<Rc<str>>) -> Self {
        VmValue::String(value.into())
    }

    pub fn zero() -> Self {
        VmValue::Scalar(Register::zero())
    }

    pub fn as_scalar(&self) -> Option<Register> {
        match self {
            VmValue::Scalar(register) => Some(*register),
            VmValue::Function(_) | VmValue::NativeFunction(_) | VmValue::String(_) => None,
        }
    }

    pub fn as_function(&self) -> Option<Rc<Chunk>> {
        match self {
            VmValue::Scalar(_) | VmValue::NativeFunction(_) | VmValue::String(_) => None,
            VmValue::Function(chunk) => Some(chunk.clone()),
        }
    }

    pub fn as_native_function(&self) -> Option<NativeFunction> {
        match self {
            VmValue::NativeFunction(function) => Some(function.clone()),
            VmValue::Scalar(_) | VmValue::Function(_) | VmValue::String(_) => None,
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
        // i8 lives in the lower byte of i64
        let r = Register::from_i64(0x0102030405060708);
        unsafe {
            assert_eq!(r.i8, 0x08);
            assert_eq!(r.i16, 0x0708);
            assert_eq!(r.i32, 0x05060708);
        }
    }
}
