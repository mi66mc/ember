#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
    Ptr, // pointer
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Constant {
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    Bool(bool),
}

impl Constant {
    pub fn value_type(&self) -> ValueType {
        match self {
            Constant::I8(_) => ValueType::I8,
            Constant::I16(_) => ValueType::I16,
            Constant::I32(_) => ValueType::I32,
            Constant::I64(_) => ValueType::I64,
            Constant::U8(_) => ValueType::U8,
            Constant::U16(_) => ValueType::U16,
            Constant::U32(_) => ValueType::U32,
            Constant::U64(_) => ValueType::U64,
            Constant::F32(_) => ValueType::F32,
            Constant::F64(_) => ValueType::F64,
            Constant::Bool(_) => ValueType::Bool,
        }
    }

    pub fn to_bits(&self) -> u64 {
        match *self {
            Constant::I8(v) => v as i64 as u64,
            Constant::I16(v) => v as i64 as u64,
            Constant::I32(v) => v as i64 as u64,
            Constant::I64(v) => v as u64,
            Constant::U8(v) => v as u64,
            Constant::U16(v) => v as u64,
            Constant::U32(v) => v as u64,
            Constant::U64(v) => v,
            Constant::F32(v) => (v as f64).to_bits(),
            Constant::F64(v) => v.to_bits(),
            Constant::Bool(v) => v as u64,
        }
    }
}

impl ValueType {
    pub fn size_bytes(&self) -> usize {
        match self {
            ValueType::I8 | ValueType::U8 | ValueType::Bool => 1,
            ValueType::I16 | ValueType::U16 => 2,
            ValueType::I32 | ValueType::U32 | ValueType::F32 => 4,
            ValueType::I64 | ValueType::U64 | ValueType::F64 | ValueType::Ptr => 8,
        }
    }

    pub fn is_signed(&self) -> bool {
        matches!(
            self,
            ValueType::I8 | ValueType::I16 | ValueType::I32 | ValueType::I64
        )
    }

    pub fn is_float(&self) -> bool {
        matches!(self, ValueType::F32 | ValueType::F64)
    }

    pub fn is_integer(&self) -> bool {
        !self.is_float() && *self != ValueType::Bool && *self != ValueType::Ptr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_type_size() {
        assert_eq!(ValueType::I8.size_bytes(), 1);
        assert_eq!(ValueType::I16.size_bytes(), 2);
        assert_eq!(ValueType::I32.size_bytes(), 4);
        assert_eq!(ValueType::I64.size_bytes(), 8);
        assert_eq!(ValueType::F32.size_bytes(), 4);
        assert_eq!(ValueType::F64.size_bytes(), 8);
        assert_eq!(ValueType::Ptr.size_bytes(), 8);
    }

    #[test]
    fn test_constant_value_type() {
        assert_eq!(Constant::I32(42).value_type(), ValueType::I32);
        assert_eq!(Constant::F64(3.14).value_type(), ValueType::F64);
        assert_eq!(Constant::Bool(true).value_type(), ValueType::Bool);
    }

    #[test]
    fn test_constant_to_bits() {
        assert_eq!(Constant::I64(42).to_bits(), 42);
        assert_eq!(Constant::I64(-1).to_bits(), u64::MAX);
        assert_eq!(Constant::Bool(true).to_bits(), 1);
        assert_eq!(Constant::Bool(false).to_bits(), 0);
    }

    #[test]
    fn test_value_type_is_signed() {
        assert!(ValueType::I8.is_signed());
        assert!(ValueType::I64.is_signed());
        assert!(!ValueType::U8.is_signed());
        assert!(!ValueType::F64.is_signed());
    }

    #[test]
    fn test_value_type_is_float() {
        assert!(ValueType::F32.is_float());
        assert!(ValueType::F64.is_float());
        assert!(!ValueType::I64.is_float());
    }

    #[test]
    fn test_value_type_is_integer() {
        assert!(ValueType::I32.is_integer());
        assert!(ValueType::U64.is_integer());
        assert!(!ValueType::F64.is_integer());
        assert!(!ValueType::Bool.is_integer());
        assert!(!ValueType::Ptr.is_integer());
    }
}
