#![allow(non_camel_case_types)]

macro_rules! opcodes {
    ($($name:ident = $val:expr),* $(,)?) => {
        #[repr(u8)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Opcode {
            $($name = $val),*
        }

        impl Opcode {
            pub fn from_byte(byte: u8) -> Option<Self> {
                match byte {
                    $($val => Some(Self::$name),)*
                    _ => None,
                }
            }

            pub fn to_byte(self) -> u8 {
                self as u8
            }
        }
    }
}

opcodes! {
    LOADK = 0x00,
    MOVE = 0x01,

    LOAD_I8 = 0x02,
    LOAD_I16 = 0x03,
    LOAD_I32 = 0x04,
    LOAD_I64 = 0x05,
    LOAD_U8 = 0x06,
    LOAD_U16 = 0x07,
    LOAD_U32 = 0x08,
    LOAD_U64 = 0x09,
    LOAD_F32 = 0x0A,
    LOAD_F64 = 0x0B,

    STORE_I8 = 0x0C,
    STORE_I16 = 0x0D,
    STORE_I32 = 0x0E,
    STORE_I64 = 0x0F,
    STORE_U8 = 0x10,
    STORE_U16 = 0x11,
    STORE_U32 = 0x12,
    STORE_U64 = 0x13,
    STORE_F32 = 0x14,
    STORE_F64 = 0x15,

    ADD_I64 = 0x23,
    SUB_I64 = 0x27,
    MUL_I64 = 0x2B,
    DIV_I64 = 0x2F,
    MOD_I64 = 0x33,
    NEG_I64 = 0x37,

    DIV_U64 = 0x4F,
    MOD_U64 = 0x53,

    ADD_F64 = 0x59,
    SUB_F64 = 0x5B,
    MUL_F64 = 0x5D,
    DIV_F64 = 0x5F,
    NEG_F64 = 0x61,

    AND_I64 = 0x6B,
    OR_I64 = 0x6F,
    XOR_I64 = 0x73,
    NOT_I64 = 0x77,
    SHL_I64 = 0x7B,
    SHR_I64 = 0x7F,
    USHR_I64 = 0x83,

    EQ_I64 = 0x93,
    NE_I64 = 0x97,
    LT_I64 = 0x9B,
    LE_I64 = 0x9F,
    GT_I64 = 0xA3,
    GE_I64 = 0xA7,

    LT_U64 = 0xAB,
    LE_U64 = 0xAF,
    GT_U64 = 0xB3,
    GE_U64 = 0xB7,

    EQ_F64 = 0xB9,
    NE_F64 = 0xBB,
    LT_F64 = 0xBD,
    LE_F64 = 0xBF,
    GT_F64 = 0xC1,
    GE_F64 = 0xC3,

    CONV = 0xC8,

    JMP = 0xD0,
    JMPIF = 0xD1,
    JMPIFNOT = 0xD2,

    TRY = 0xD3,
    ENDTRY = 0xD4,
    THROW = 0xD5,

    GETUPVAL = 0xD6,

    CLOSURE = 0xD7,
    CALL = 0xD8,
    RET = 0xD9,

    SETUPVAL = 0xDA,

    CALLTAIL = 0xDB,

    GETG = 0xE0,
    SETG = 0xE1,

    EXT = 0xFD,
    NOP = 0xFE,
    HALT = 0xFF,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opcode_round_trip() {
        let original = Opcode::ADD_I64;
        assert_eq!(Opcode::from_byte(original.to_byte()), Some(original));
    }

    #[test]
    fn invalid_opcode_bytes_return_none() {
        assert!(Opcode::from_byte(0x16).is_none());
        assert!(Opcode::from_byte(0x17).is_none());
        assert!(Opcode::from_byte(0x1F).is_none());
    }
}
