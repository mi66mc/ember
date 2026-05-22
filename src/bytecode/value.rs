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
    Ptr,
}

#[derive(Debug, Clone, PartialEq)]
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
    Bytes(Vec<u8>),
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
            Constant::Bytes(_) => ValueType::Ptr,
        }
    }

    pub fn to_bits(&self) -> Option<u64> {
        Some(match *self {
            Constant::I8(v) => v as i64 as u64,
            Constant::I16(v) => v as i64 as u64,
            Constant::I32(v) => v as i64 as u64,
            Constant::I64(v) => v as u64,
            Constant::U8(v) => v as u64,
            Constant::U16(v) => v as u64,
            Constant::U32(v) => v as u64,
            Constant::U64(v) => v,
            Constant::F32(v) => v.to_bits() as u64,
            Constant::F64(v) => v.to_bits(),
            Constant::Bool(v) => v as u64,
            Constant::Bytes(_) => return None,
        })
    }
}

impl ValueType {
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(ValueType::I8),
            1 => Some(ValueType::I16),
            2 => Some(ValueType::I32),
            3 => Some(ValueType::I64),
            4 => Some(ValueType::U8),
            5 => Some(ValueType::U16),
            6 => Some(ValueType::U32),
            7 => Some(ValueType::U64),
            8 => Some(ValueType::F32),
            9 => Some(ValueType::F64),
            10 => Some(ValueType::Bool),
            11 => Some(ValueType::Ptr),
            _ => None,
        }
    }

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

