use std::cell::UnsafeCell;
use std::rc::Rc;
use crate::bytecode::Chunk;
use crate::vm::register::VmValue;

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
    #[inline] pub fn scalar(bits: u64) -> Self { Value(bits) }
    #[inline] pub fn from_i64(v: i64) -> Self { Value(v as u64) }
    #[inline] pub fn from_f64(v: f64) -> Self { Value(v.to_bits()) }
    #[inline] pub fn from_bool(v: bool) -> Self { Value(v as u64) }
    #[inline] pub fn from_ptr(v: usize) -> Self { Value(v as u64) }
    #[inline] pub fn from_u64(v: u64) -> Self { Value(v) }
    #[inline] pub fn nil() -> Self { Value(u64::MAX) }
    #[inline] pub fn bits(self) -> u64 { self.0 }
    #[inline] pub fn i64(self) -> i64 { self.0 as i64 }
    #[inline] pub fn u64(self) -> u64 { self.0 }
    #[inline] pub fn f64(self) -> f64 { f64::from_bits(self.0) }
    #[inline] pub fn ptr(self) -> usize { self.0 as usize }
    #[inline] pub fn bool(self) -> bool { self.0 != 0 }
}

#[derive(Clone, Copy)]
pub struct TypeMask(pub(crate) u128);

impl TypeMask {
    pub fn new() -> Self { Self(0) }
    #[inline] pub fn get(&self, idx: u8) -> u8 {
        ((self.0 >> (idx as u32 * 2)) & 0b11) as u8
    }
    #[inline] pub fn set(&mut self, idx: u8, tag: u8) {
        let shift = idx as u32 * 2;
        self.0 = (self.0 & !(0b11u128 << shift)) | ((tag as u128 & 0b11) << shift);
    }
}

#[derive(Debug, Clone)]
pub struct ClosureData {
    pub chunk: Rc<Chunk>,
    pub upvalues: Rc<UnsafeCell<Vec<VmValue>>>,
}
