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
    // ─────────────────────────────────────────
    // load/store
    // ─────────────────────────────────────────
    LOADK = 0x00,       // Ra <- constants[Bx]
    MOVE = 0x01,        // Ra <- Rb

    LOAD_I8 = 0x02,     // Ra <- mem[Rb + C] as i8
    LOAD_I16 = 0x03,
    LOAD_I32 = 0x04,
    LOAD_I64 = 0x05,
    LOAD_U8 = 0x06,
    LOAD_U16 = 0x07,
    LOAD_U32 = 0x08,
    LOAD_U64 = 0x09,
    LOAD_F32 = 0x0A,
    LOAD_F64 = 0x0B,

    STORE_I8 = 0x0C,    // mem[Ra + B] <- Rc as i8
    STORE_I16 = 0x0D,
    STORE_I32 = 0x0E,
    STORE_I64 = 0x0F,
    STORE_U8 = 0x10,
    STORE_U16 = 0x11,
    STORE_U32 = 0x12,
    STORE_U64 = 0x13,
    STORE_F32 = 0x14,
    STORE_F64 = 0x15,

    // ─────────────────────────────────────────
    // arithmetic (signed)
    // ─────────────────────────────────────────
    ADD_I8 = 0x20,      // Ra <- Rb + Rc
    ADD_I16 = 0x21,
    ADD_I32 = 0x22,
    ADD_I64 = 0x23,

    SUB_I8 = 0x24,      // Ra <- Rb - Rc
    SUB_I16 = 0x25,
    SUB_I32 = 0x26,
    SUB_I64 = 0x27,

    MUL_I8 = 0x28,      // Ra <- Rb * Rc
    MUL_I16 = 0x29,
    MUL_I32 = 0x2A,
    MUL_I64 = 0x2B,

    DIV_I8 = 0x2C,      // Ra <- Rb / Rc
    DIV_I16 = 0x2D,
    DIV_I32 = 0x2E,
    DIV_I64 = 0x2F,

    MOD_I8 = 0x30,      // Ra <- Rb % Rc
    MOD_I16 = 0x31,
    MOD_I32 = 0x32,
    MOD_I64 = 0x33,

    NEG_I8 = 0x34,      // Ra <- -Rb
    NEG_I16 = 0x35,
    NEG_I32 = 0x36,
    NEG_I64 = 0x37,

    // ─────────────────────────────────────────
    // arithmetic (unsigned)
    // ─────────────────────────────────────────
    ADD_U8 = 0x40,
    ADD_U16 = 0x41,
    ADD_U32 = 0x42,
    ADD_U64 = 0x43,

    SUB_U8 = 0x44,
    SUB_U16 = 0x45,
    SUB_U32 = 0x46,
    SUB_U64 = 0x47,

    MUL_U8 = 0x48,
    MUL_U16 = 0x49,
    MUL_U32 = 0x4A,
    MUL_U64 = 0x4B,

    DIV_U8 = 0x4C,
    DIV_U16 = 0x4D,
    DIV_U32 = 0x4E,
    DIV_U64 = 0x4F,

    MOD_U8 = 0x50,
    MOD_U16 = 0x51,
    MOD_U32 = 0x52,
    MOD_U64 = 0x53,

    // ─────────────────────────────────────────
    // arithmetic (float)
    // ─────────────────────────────────────────
    ADD_F32 = 0x58,
    ADD_F64 = 0x59,

    SUB_F32 = 0x5A,
    SUB_F64 = 0x5B,

    MUL_F32 = 0x5C,
    MUL_F64 = 0x5D,

    DIV_F32 = 0x5E,
    DIV_F64 = 0x5F,

    NEG_F32 = 0x60,
    NEG_F64 = 0x61,

    // ─────────────────────────────────────────
    // bitwise
    // ─────────────────────────────────────────
    AND_I8 = 0x68,      // Ra <- Rb & Rc
    AND_I16 = 0x69,
    AND_I32 = 0x6A,
    AND_I64 = 0x6B,

    OR_I8 = 0x6C,       // Ra <- Rb | Rc
    OR_I16 = 0x6D,
    OR_I32 = 0x6E,
    OR_I64 = 0x6F,

    XOR_I8 = 0x70,      // Ra <- Rb ^ Rc
    XOR_I16 = 0x71,
    XOR_I32 = 0x72,
    XOR_I64 = 0x73,

    NOT_I8 = 0x74,      // Ra <- !Rb
    NOT_I16 = 0x75,
    NOT_I32 = 0x76,
    NOT_I64 = 0x77,

    SHL_I8 = 0x78,      // Ra <- Rb << Rc
    SHL_I16 = 0x79,
    SHL_I32 = 0x7A,
    SHL_I64 = 0x7B,

    SHR_I8 = 0x7C,      // Ra <- Rb >> Rc (arithmetic)
    SHR_I16 = 0x7D,
    SHR_I32 = 0x7E,
    SHR_I64 = 0x7F,

    USHR_I8 = 0x80,     // Ra <- Rb >>> Rc (logical)
    USHR_I16 = 0x81,
    USHR_I32 = 0x82,
    USHR_I64 = 0x83,

    // ─────────────────────────────────────────
    // comparison (signed) -> Ra <- 0 | 1
    // ─────────────────────────────────────────
    EQ_I8 = 0x90,       // Ra <- Rb == Rc
    EQ_I16 = 0x91,
    EQ_I32 = 0x92,
    EQ_I64 = 0x93,

    NE_I8 = 0x94,       // Ra <- Rb != Rc
    NE_I16 = 0x95,
    NE_I32 = 0x96,
    NE_I64 = 0x97,

    LT_I8 = 0x98,       // Ra <- Rb < Rc
    LT_I16 = 0x99,
    LT_I32 = 0x9A,
    LT_I64 = 0x9B,

    LE_I8 = 0x9C,       // Ra <- Rb <= Rc
    LE_I16 = 0x9D,
    LE_I32 = 0x9E,
    LE_I64 = 0x9F,

    GT_I8 = 0xA0,       // Ra <- Rb > Rc
    GT_I16 = 0xA1,
    GT_I32 = 0xA2,
    GT_I64 = 0xA3,

    GE_I8 = 0xA4,       // Ra <- Rb >= Rc
    GE_I16 = 0xA5,
    GE_I32 = 0xA6,
    GE_I64 = 0xA7,

    // ─────────────────────────────────────────
    // comparison (unsigned)
    // ─────────────────────────────────────────
    LT_U8 = 0xA8,
    LT_U16 = 0xA9,
    LT_U32 = 0xAA,
    LT_U64 = 0xAB,

    LE_U8 = 0xAC,
    LE_U16 = 0xAD,
    LE_U32 = 0xAE,
    LE_U64 = 0xAF,

    GT_U8 = 0xB0,
    GT_U16 = 0xB1,
    GT_U32 = 0xB2,
    GT_U64 = 0xB3,

    GE_U8 = 0xB4,
    GE_U16 = 0xB5,
    GE_U32 = 0xB6,
    GE_U64 = 0xB7,

    // ─────────────────────────────────────────
    // comparison (float)
    // ─────────────────────────────────────────
    EQ_F32 = 0xB8,
    EQ_F64 = 0xB9,

    NE_F32 = 0xBA,
    NE_F64 = 0xBB,

    LT_F32 = 0xBC,
    LT_F64 = 0xBD,

    LE_F32 = 0xBE,
    LE_F64 = 0xBF,

    GT_F32 = 0xC0,
    GT_F64 = 0xC1,

    GE_F32 = 0xC2,
    GE_F64 = 0xC3,

    // ─────────────────────────────────────────
    // conversion
    // ─────────────────────────────────────────
    CONV = 0xC8,        // Ra <- convert(Rb, B, C) where B=from C=to

    // ─────────────────────────────────────────
    // control flow
    // ─────────────────────────────────────────
    JMP = 0xD0,         // pc <- pc + sBx
    JMPIF = 0xD1,       // if Ra != 0 then pc <- pc + sBx
    JMPIFNOT = 0xD2,    // if Ra == 0 then pc <- pc + sBx

    // ─────────────────────────────────────────
    // functions
    // ─────────────────────────────────────────
    CLOSURE = 0xD7,     // Ra <- closure(protos[Bx])
    CALL = 0xD8,        // call Ra with B args, C returns
    RET = 0xD9,         // return from current frame

    // ─────────────────────────────────────────
    // globals
    // ─────────────────────────────────────────
    GETG = 0xE0,        // Ra <- globals[Bx]
    SETG = 0xE1,        // globals[Bx] <- Ra

    // ─────────────────────────────────────────
    // system
    // ─────────────────────────────────────────
    NOP = 0xFE,         // no operation
    HALT = 0xFF,        // stop vm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opcode_round_trip() {
        let original = Opcode::ADD_I64;
        let byte = original.to_byte();
        let recovered = Opcode::from_byte(byte);
        assert_eq!(recovered, Some(original));
    }

    #[test]
    fn test_opcode_invalid_byte() {
        assert!(Opcode::from_byte(0x16).is_none());
        assert!(Opcode::from_byte(0x17).is_none());
        assert!(Opcode::from_byte(0x1F).is_none());
    }

    #[test]
    fn test_opcode_categories() {
        assert!(Opcode::LOADK.to_byte() < 0x20);
        assert!(Opcode::ADD_I8.to_byte() >= 0x20 && Opcode::ADD_I8.to_byte() < 0x40);
        assert!(Opcode::ADD_U8.to_byte() >= 0x40 && Opcode::ADD_U8.to_byte() < 0x60);
        assert!(Opcode::AND_I8.to_byte() >= 0x68 && Opcode::AND_I8.to_byte() < 0x90);
        assert!(Opcode::EQ_I8.to_byte() >= 0x90);
        assert!(Opcode::JMP.to_byte() >= 0xD0);
        assert_eq!(Opcode::HALT.to_byte(), 0xFF);
    }

    #[test]
    fn test_all_opcodes_valid() {
        let opcodes = [
            Opcode::LOADK,
            Opcode::MOVE,
            Opcode::ADD_I64,
            Opcode::SUB_I64,
            Opcode::JMP,
            Opcode::CALL,
            Opcode::HALT,
        ];
        for op in opcodes {
            assert_eq!(Opcode::from_byte(op.to_byte()), Some(op));
        }
    }
}
