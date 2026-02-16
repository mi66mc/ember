use std::rc::Rc;

use crate::bytecode::chunk::Chunk;
use crate::common::types::opcode::Opcode;
use crate::common::types::value::ValueType;
use crate::vm::memory::Memory;
use crate::vm::register::Register;
use crate::vm::stack::CallStack;

// ─────────────────────────────────────────────────────────────
// macros for arithmetic/comparison ops (reduce boilerplate)
// ─────────────────────────────────────────────────────────────

// Ra <- Rb op Rc (wrapping for integers)
macro_rules! binop_int {
    ($self:ident, $instr:ident, $field:ident, $from:ident, $method:ident) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        let vb = unsafe { $self.stack.get_reg(b).$field };
        let vc = unsafe { $self.stack.get_reg(c).$field };
        $self.stack.set_reg(a, Register::$from(vb.$method(vc)));
    }};
}

// Ra <- Rb op Rc (float, no wrapping)
macro_rules! binop_float {
    ($self:ident, $instr:ident, $field:ident, $from:ident, $op:tt) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        let vb = unsafe { $self.stack.get_reg(b).$field };
        let vc = unsafe { $self.stack.get_reg(c).$field };
        $self.stack.set_reg(a, Register::$from(vb $op vc));
    }};
}

// Ra <- Rb / Rc (with zero check)
macro_rules! divop_int {
    ($self:ident, $instr:ident, $field:ident, $from:ident, $method:ident) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        let vb = unsafe { $self.stack.get_reg(b).$field };
        let vc = unsafe { $self.stack.get_reg(c).$field };
        if vc == 0 {
            return Err(VMError::DivisionByZero);
        }
        $self.stack.set_reg(a, Register::$from(vb.$method(vc)));
    }};
}

// Ra <- -Rb (unary negation)
macro_rules! negop_int {
    ($self:ident, $instr:ident, $field:ident, $from:ident) => {{
        let (a, b) = ($instr.a(), $instr.b());
        let vb = unsafe { $self.stack.get_reg(b).$field };
        $self.stack.set_reg(a, Register::$from(vb.wrapping_neg()));
    }};
}

macro_rules! negop_float {
    ($self:ident, $instr:ident, $field:ident, $from:ident) => {{
        let (a, b) = ($instr.a(), $instr.b());
        let vb = unsafe { $self.stack.get_reg(b).$field };
        $self.stack.set_reg(a, Register::$from(-vb));
    }};
}

// Ra <- (Rb cmp Rc) ? 1 : 0
macro_rules! cmpop {
    ($self:ident, $instr:ident, $field:ident, $op:tt) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        let vb = unsafe { $self.stack.get_reg(b).$field };
        let vc = unsafe { $self.stack.get_reg(c).$field };
        $self.stack.set_reg(a, Register::from_bool(vb $op vc));
    }};
}

// Ra <- Rb op Rc (bitwise)
macro_rules! bitop {
    ($self:ident, $instr:ident, $field:ident, $from:ident, $op:tt) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        let vb = unsafe { $self.stack.get_reg(b).$field };
        let vc = unsafe { $self.stack.get_reg(c).$field };
        $self.stack.set_reg(a, Register::$from(vb $op vc));
    }};
}

// Ra <- !Rb (bitwise not)
macro_rules! notop {
    ($self:ident, $instr:ident, $field:ident, $from:ident) => {{
        let (a, b) = ($instr.a(), $instr.b());
        let vb = unsafe { $self.stack.get_reg(b).$field };
        $self.stack.set_reg(a, Register::$from(!vb));
    }};
}

// Ra <- Rb << Rc (shift left)
macro_rules! shlop {
    ($self:ident, $instr:ident, $field:ident, $from:ident) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        let vb = unsafe { $self.stack.get_reg(b).$field };
        let vc = unsafe { $self.stack.get_reg(c).$field };
        $self.stack.set_reg(a, Register::$from(vb.wrapping_shl(vc as u32)));
    }};
}

// Ra <- Rb >> Rc (arithmetic shift right, preserves sign)
macro_rules! shrop {
    ($self:ident, $instr:ident, $field:ident, $from:ident) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        let vb = unsafe { $self.stack.get_reg(b).$field };
        let vc = unsafe { $self.stack.get_reg(c).$field };
        $self.stack.set_reg(a, Register::$from(vb.wrapping_shr(vc as u32)));
    }};
}

// Ra <- mem[Rb + C] (load from memory)
macro_rules! loadop {
    ($self:ident, $instr:ident, $typ:ty, $from:ident) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        let base = unsafe { $self.stack.get_reg(b).ptr };
        let addr = base + c as usize;
        let val: $typ = unsafe { $self.memory.read(addr) };
        $self.stack.set_reg(a, Register::$from(val));
    }};
}

// mem[Ra + B] <- Rc (store to memory)
macro_rules! storeop {
    ($self:ident, $instr:ident, $field:ident) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        let base = unsafe { $self.stack.get_reg(a).ptr };
        let addr = base + b as usize;
        let val = unsafe { $self.stack.get_reg(c).$field };
        unsafe { $self.memory.write(addr, val) };
    }};
}

// vm execution engine
//  ┌────────────────────────────────────────────────────────────┐
//  │                          VM                                │
//  │  ┌─────────────┐  ┌─────────────────────────────────────┐  │
//  │  │   Memory    │  │            CallStack                │  │
//  │  │ ┌─────────┐ │  │  ┌───────────────────────────────┐  │  │
//  │  │ │ 0: ...  │ │  │  │ Frame 0                       │  │  │
//  │  │ │ 8: ...  │ │  │  │  chunk: Rc<Chunk>             │  │  │
//  │  │ │16: ...  │ │  │  │  pc: 2                        │  │  │
//  │  │ │  ...    │ │  │  │  regs: [R0, R1, R2, ...]      │  │  │
//  │  │ └─────────┘ │  │  └───────────────────────────────┘  │  │
//  │  └─────────────┘  └─────────────────────────────────────┘  │
//  └────────────────────────────────────────────────────────────┘

// ─────────────────────────────────────────────────────────────
// type conversion helper
// ─────────────────────────────────────────────────────────────

// convert register value from one type to another
// safety: caller must ensure src was written with from_type
unsafe fn convert_register(src: Register, from: ValueType, to: ValueType) -> Register {
    // first read as i64/u64/f64 depending on source type
    let as_i64: i64 = unsafe {
        match from {
            ValueType::I8 => src.i8 as i64,
            ValueType::I16 => src.i16 as i64,
            ValueType::I32 => src.i32 as i64,
            ValueType::I64 => src.i64,
            ValueType::U8 => src.u8 as i64,
            ValueType::U16 => src.u16 as i64,
            ValueType::U32 => src.u32 as i64,
            ValueType::U64 => src.u64 as i64,
            ValueType::F32 => src.f32 as i64,
            ValueType::F64 => src.f64 as i64,
            ValueType::Bool => src.u64 as i64,
            ValueType::Ptr => src.ptr as i64,
        }
    };

    let as_f64: f64 = unsafe {
        match from {
            ValueType::I8 => src.i8 as f64,
            ValueType::I16 => src.i16 as f64,
            ValueType::I32 => src.i32 as f64,
            ValueType::I64 => src.i64 as f64,
            ValueType::U8 => src.u8 as f64,
            ValueType::U16 => src.u16 as f64,
            ValueType::U32 => src.u32 as f64,
            ValueType::U64 => src.u64 as f64,
            ValueType::F32 => src.f32 as f64,
            ValueType::F64 => src.f64,
            ValueType::Bool => src.u64 as f64,
            ValueType::Ptr => src.ptr as f64,
        }
    };

    // then convert to target type
    match to {
        ValueType::I8 => Register::from_i8(as_i64 as i8),
        ValueType::I16 => Register::from_i16(as_i64 as i16),
        ValueType::I32 => Register::from_i32(as_i64 as i32),
        ValueType::I64 => Register::from_i64(as_i64),
        ValueType::U8 => Register::from_u8(as_i64 as u8),
        ValueType::U16 => Register::from_u16(as_i64 as u16),
        ValueType::U32 => Register::from_u32(as_i64 as u32),
        ValueType::U64 => Register::from_u64(as_i64 as u64),
        ValueType::F32 => Register::from_f32(as_f64 as f32),
        ValueType::F64 => Register::from_f64(as_f64),
        ValueType::Bool => Register::from_bool(as_i64 != 0),
        ValueType::Ptr => Register::from_ptr(as_i64 as usize),
    }
}

#[derive(Debug)]
pub enum VMError {
    StackUnderflow,
    InvalidOpcode(u8),
    DivisionByZero,
    InvalidConstantIndex(u16),
    InvalidProtoIndex(u16),
    InvalidConversionType(u8),
    Halted,
}

pub struct VM {
    pub stack: CallStack,
    pub memory: Memory,
    pub globals: Vec<Register>,
}

impl VM {
    pub fn new(memory_size: usize) -> Self {
        VM {
            stack: CallStack::new(),
            memory: Memory::new(memory_size),
            globals: Vec::new(),
        }
    }

    // run chunk until HALT or error
    pub fn run(&mut self, chunk: Chunk) -> Result<(), VMError> {
        self.stack.push_frame(Rc::new(chunk));

        loop {
            let result = self.step();
            match result {
                Ok(()) => continue,
                Err(VMError::Halted) => {
                    self.stack.pop_frame();
                    return Ok(());
                }
                Err(e) => return Err(e),
            }
        }
    }

    // execute single instruction
    pub fn step(&mut self) -> Result<(), VMError> {
        let frame = self.stack.current().ok_or(VMError::StackUnderflow)?;
        let pc = frame.pc;
        let instr = frame.chunk.code[pc];
        let op = instr.opcode();

        // advance pc before execution (jumps will override)
        self.stack.advance_pc();

        match op {
            // ═══════════════════════════════════════════════════════
            // system
            // ═══════════════════════════════════════════════════════
            Opcode::HALT => return Err(VMError::Halted),
            Opcode::NOP => {}

            // ═══════════════════════════════════════════════════════
            // load/move
            // ═══════════════════════════════════════════════════════

            // Ra <- constants[Bx]
            Opcode::LOADK => {
                let a = instr.a();
                let bx = instr.bx();
                let frame = self.stack.current().ok_or(VMError::StackUnderflow)?;
                let constant = frame
                    .chunk
                    .constants
                    .get(bx as usize)
                    .ok_or(VMError::InvalidConstantIndex(bx))?;
                let val = Register {
                    bits: constant.to_bits(),
                };
                self.stack.set_reg(a, val);
            }

            // Ra <- Rb
            Opcode::MOVE => {
                let a = instr.a();
                let b = instr.b();
                let val = self.stack.get_reg(b);
                self.stack.set_reg(a, val);
            }

            // ═══════════════════════════════════════════════════════
            // memory: load (Ra <- mem[Rb + C])
            // ═══════════════════════════════════════════════════════

            Opcode::LOAD_I8 => loadop!(self, instr, i8, from_i8),
            Opcode::LOAD_I16 => loadop!(self, instr, i16, from_i16),
            Opcode::LOAD_I32 => loadop!(self, instr, i32, from_i32),
            Opcode::LOAD_I64 => loadop!(self, instr, i64, from_i64),
            Opcode::LOAD_U8 => loadop!(self, instr, u8, from_u8),
            Opcode::LOAD_U16 => loadop!(self, instr, u16, from_u16),
            Opcode::LOAD_U32 => loadop!(self, instr, u32, from_u32),
            Opcode::LOAD_U64 => loadop!(self, instr, u64, from_u64),
            Opcode::LOAD_F32 => loadop!(self, instr, f32, from_f32),
            Opcode::LOAD_F64 => loadop!(self, instr, f64, from_f64),

            // ═══════════════════════════════════════════════════════
            // memory: store (mem[Ra + B] <- Rc)
            // ═══════════════════════════════════════════════════════

            Opcode::STORE_I8 => storeop!(self, instr, i8),
            Opcode::STORE_I16 => storeop!(self, instr, i16),
            Opcode::STORE_I32 => storeop!(self, instr, i32),
            Opcode::STORE_I64 => storeop!(self, instr, i64),
            Opcode::STORE_U8 => storeop!(self, instr, u8),
            Opcode::STORE_U16 => storeop!(self, instr, u16),
            Opcode::STORE_U32 => storeop!(self, instr, u32),
            Opcode::STORE_U64 => storeop!(self, instr, u64),
            Opcode::STORE_F32 => storeop!(self, instr, f32),
            Opcode::STORE_F64 => storeop!(self, instr, f64),

            // ═══════════════════════════════════════════════════════
            // arithmetic: signed integers (i8, i16, i32, i64)
            // ═══════════════════════════════════════════════════════

            // ADD: Ra <- Rb + Rc
            Opcode::ADD_I8 => binop_int!(self, instr, i8, from_i8, wrapping_add),
            Opcode::ADD_I16 => binop_int!(self, instr, i16, from_i16, wrapping_add),
            Opcode::ADD_I32 => binop_int!(self, instr, i32, from_i32, wrapping_add),
            Opcode::ADD_I64 => binop_int!(self, instr, i64, from_i64, wrapping_add),

            // SUB: Ra <- Rb - Rc
            Opcode::SUB_I8 => binop_int!(self, instr, i8, from_i8, wrapping_sub),
            Opcode::SUB_I16 => binop_int!(self, instr, i16, from_i16, wrapping_sub),
            Opcode::SUB_I32 => binop_int!(self, instr, i32, from_i32, wrapping_sub),
            Opcode::SUB_I64 => binop_int!(self, instr, i64, from_i64, wrapping_sub),

            // MUL: Ra <- Rb * Rc
            Opcode::MUL_I8 => binop_int!(self, instr, i8, from_i8, wrapping_mul),
            Opcode::MUL_I16 => binop_int!(self, instr, i16, from_i16, wrapping_mul),
            Opcode::MUL_I32 => binop_int!(self, instr, i32, from_i32, wrapping_mul),
            Opcode::MUL_I64 => binop_int!(self, instr, i64, from_i64, wrapping_mul),

            // DIV: Ra <- Rb / Rc
            Opcode::DIV_I8 => divop_int!(self, instr, i8, from_i8, wrapping_div),
            Opcode::DIV_I16 => divop_int!(self, instr, i16, from_i16, wrapping_div),
            Opcode::DIV_I32 => divop_int!(self, instr, i32, from_i32, wrapping_div),
            Opcode::DIV_I64 => divop_int!(self, instr, i64, from_i64, wrapping_div),

            // MOD: Ra <- Rb % Rc
            Opcode::MOD_I8 => divop_int!(self, instr, i8, from_i8, wrapping_rem),
            Opcode::MOD_I16 => divop_int!(self, instr, i16, from_i16, wrapping_rem),
            Opcode::MOD_I32 => divop_int!(self, instr, i32, from_i32, wrapping_rem),
            Opcode::MOD_I64 => divop_int!(self, instr, i64, from_i64, wrapping_rem),

            // NEG: Ra <- -Rb
            Opcode::NEG_I8 => negop_int!(self, instr, i8, from_i8),
            Opcode::NEG_I16 => negop_int!(self, instr, i16, from_i16),
            Opcode::NEG_I32 => negop_int!(self, instr, i32, from_i32),
            Opcode::NEG_I64 => negop_int!(self, instr, i64, from_i64),

            // ═══════════════════════════════════════════════════════
            // arithmetic: unsigned integers (u8, u16, u32, u64)
            // ═══════════════════════════════════════════════════════

            Opcode::ADD_U8 => binop_int!(self, instr, u8, from_u8, wrapping_add),
            Opcode::ADD_U16 => binop_int!(self, instr, u16, from_u16, wrapping_add),
            Opcode::ADD_U32 => binop_int!(self, instr, u32, from_u32, wrapping_add),
            Opcode::ADD_U64 => binop_int!(self, instr, u64, from_u64, wrapping_add),

            Opcode::SUB_U8 => binop_int!(self, instr, u8, from_u8, wrapping_sub),
            Opcode::SUB_U16 => binop_int!(self, instr, u16, from_u16, wrapping_sub),
            Opcode::SUB_U32 => binop_int!(self, instr, u32, from_u32, wrapping_sub),
            Opcode::SUB_U64 => binop_int!(self, instr, u64, from_u64, wrapping_sub),

            Opcode::MUL_U8 => binop_int!(self, instr, u8, from_u8, wrapping_mul),
            Opcode::MUL_U16 => binop_int!(self, instr, u16, from_u16, wrapping_mul),
            Opcode::MUL_U32 => binop_int!(self, instr, u32, from_u32, wrapping_mul),
            Opcode::MUL_U64 => binop_int!(self, instr, u64, from_u64, wrapping_mul),

            Opcode::DIV_U8 => divop_int!(self, instr, u8, from_u8, wrapping_div),
            Opcode::DIV_U16 => divop_int!(self, instr, u16, from_u16, wrapping_div),
            Opcode::DIV_U32 => divop_int!(self, instr, u32, from_u32, wrapping_div),
            Opcode::DIV_U64 => divop_int!(self, instr, u64, from_u64, wrapping_div),

            Opcode::MOD_U8 => divop_int!(self, instr, u8, from_u8, wrapping_rem),
            Opcode::MOD_U16 => divop_int!(self, instr, u16, from_u16, wrapping_rem),
            Opcode::MOD_U32 => divop_int!(self, instr, u32, from_u32, wrapping_rem),
            Opcode::MOD_U64 => divop_int!(self, instr, u64, from_u64, wrapping_rem),

            // ═══════════════════════════════════════════════════════
            // arithmetic: floats (f32, f64)
            // ═══════════════════════════════════════════════════════

            Opcode::ADD_F32 => binop_float!(self, instr, f32, from_f32, +),
            Opcode::ADD_F64 => binop_float!(self, instr, f64, from_f64, +),

            Opcode::SUB_F32 => binop_float!(self, instr, f32, from_f32, -),
            Opcode::SUB_F64 => binop_float!(self, instr, f64, from_f64, -),

            Opcode::MUL_F32 => binop_float!(self, instr, f32, from_f32, *),
            Opcode::MUL_F64 => binop_float!(self, instr, f64, from_f64, *),

            Opcode::DIV_F32 => binop_float!(self, instr, f32, from_f32, /),
            Opcode::DIV_F64 => binop_float!(self, instr, f64, from_f64, /),

            Opcode::NEG_F32 => negop_float!(self, instr, f32, from_f32),
            Opcode::NEG_F64 => negop_float!(self, instr, f64, from_f64),

            // ═══════════════════════════════════════════════════════
            // comparisons: signed integers -> Ra <- 0 | 1
            // ═══════════════════════════════════════════════════════

            Opcode::EQ_I8 => cmpop!(self, instr, i8, ==),
            Opcode::EQ_I16 => cmpop!(self, instr, i16, ==),
            Opcode::EQ_I32 => cmpop!(self, instr, i32, ==),
            Opcode::EQ_I64 => cmpop!(self, instr, i64, ==),

            Opcode::NE_I8 => cmpop!(self, instr, i8, !=),
            Opcode::NE_I16 => cmpop!(self, instr, i16, !=),
            Opcode::NE_I32 => cmpop!(self, instr, i32, !=),
            Opcode::NE_I64 => cmpop!(self, instr, i64, !=),

            Opcode::LT_I8 => cmpop!(self, instr, i8, <),
            Opcode::LT_I16 => cmpop!(self, instr, i16, <),
            Opcode::LT_I32 => cmpop!(self, instr, i32, <),
            Opcode::LT_I64 => cmpop!(self, instr, i64, <),

            Opcode::LE_I8 => cmpop!(self, instr, i8, <=),
            Opcode::LE_I16 => cmpop!(self, instr, i16, <=),
            Opcode::LE_I32 => cmpop!(self, instr, i32, <=),
            Opcode::LE_I64 => cmpop!(self, instr, i64, <=),

            Opcode::GT_I8 => cmpop!(self, instr, i8, >),
            Opcode::GT_I16 => cmpop!(self, instr, i16, >),
            Opcode::GT_I32 => cmpop!(self, instr, i32, >),
            Opcode::GT_I64 => cmpop!(self, instr, i64, >),

            Opcode::GE_I8 => cmpop!(self, instr, i8, >=),
            Opcode::GE_I16 => cmpop!(self, instr, i16, >=),
            Opcode::GE_I32 => cmpop!(self, instr, i32, >=),
            Opcode::GE_I64 => cmpop!(self, instr, i64, >=),

            // ═══════════════════════════════════════════════════════
            // comparisons: unsigned integers
            // ═══════════════════════════════════════════════════════

            Opcode::LT_U8 => cmpop!(self, instr, u8, <),
            Opcode::LT_U16 => cmpop!(self, instr, u16, <),
            Opcode::LT_U32 => cmpop!(self, instr, u32, <),
            Opcode::LT_U64 => cmpop!(self, instr, u64, <),

            Opcode::LE_U8 => cmpop!(self, instr, u8, <=),
            Opcode::LE_U16 => cmpop!(self, instr, u16, <=),
            Opcode::LE_U32 => cmpop!(self, instr, u32, <=),
            Opcode::LE_U64 => cmpop!(self, instr, u64, <=),

            Opcode::GT_U8 => cmpop!(self, instr, u8, >),
            Opcode::GT_U16 => cmpop!(self, instr, u16, >),
            Opcode::GT_U32 => cmpop!(self, instr, u32, >),
            Opcode::GT_U64 => cmpop!(self, instr, u64, >),

            Opcode::GE_U8 => cmpop!(self, instr, u8, >=),
            Opcode::GE_U16 => cmpop!(self, instr, u16, >=),
            Opcode::GE_U32 => cmpop!(self, instr, u32, >=),
            Opcode::GE_U64 => cmpop!(self, instr, u64, >=),

            // ═══════════════════════════════════════════════════════
            // comparisons: floats
            // ═══════════════════════════════════════════════════════

            Opcode::EQ_F32 => cmpop!(self, instr, f32, ==),
            Opcode::EQ_F64 => cmpop!(self, instr, f64, ==),

            Opcode::NE_F32 => cmpop!(self, instr, f32, !=),
            Opcode::NE_F64 => cmpop!(self, instr, f64, !=),

            Opcode::LT_F32 => cmpop!(self, instr, f32, <),
            Opcode::LT_F64 => cmpop!(self, instr, f64, <),

            Opcode::LE_F32 => cmpop!(self, instr, f32, <=),
            Opcode::LE_F64 => cmpop!(self, instr, f64, <=),

            Opcode::GT_F32 => cmpop!(self, instr, f32, >),
            Opcode::GT_F64 => cmpop!(self, instr, f64, >),

            Opcode::GE_F32 => cmpop!(self, instr, f32, >=),
            Opcode::GE_F64 => cmpop!(self, instr, f64, >=),

            // ═══════════════════════════════════════════════════════
            // bitwise: AND, OR, XOR
            // ═══════════════════════════════════════════════════════

            Opcode::AND_I8 => bitop!(self, instr, i8, from_i8, &),
            Opcode::AND_I16 => bitop!(self, instr, i16, from_i16, &),
            Opcode::AND_I32 => bitop!(self, instr, i32, from_i32, &),
            Opcode::AND_I64 => bitop!(self, instr, i64, from_i64, &),

            Opcode::OR_I8 => bitop!(self, instr, i8, from_i8, |),
            Opcode::OR_I16 => bitop!(self, instr, i16, from_i16, |),
            Opcode::OR_I32 => bitop!(self, instr, i32, from_i32, |),
            Opcode::OR_I64 => bitop!(self, instr, i64, from_i64, |),

            Opcode::XOR_I8 => bitop!(self, instr, i8, from_i8, ^),
            Opcode::XOR_I16 => bitop!(self, instr, i16, from_i16, ^),
            Opcode::XOR_I32 => bitop!(self, instr, i32, from_i32, ^),
            Opcode::XOR_I64 => bitop!(self, instr, i64, from_i64, ^),

            // ═══════════════════════════════════════════════════════
            // bitwise: NOT
            // ═══════════════════════════════════════════════════════

            Opcode::NOT_I8 => notop!(self, instr, i8, from_i8),
            Opcode::NOT_I16 => notop!(self, instr, i16, from_i16),
            Opcode::NOT_I32 => notop!(self, instr, i32, from_i32),
            Opcode::NOT_I64 => notop!(self, instr, i64, from_i64),

            // ═══════════════════════════════════════════════════════
            // bitwise: shifts
            // ═══════════════════════════════════════════════════════

            Opcode::SHL_I8 => shlop!(self, instr, i8, from_i8),
            Opcode::SHL_I16 => shlop!(self, instr, i16, from_i16),
            Opcode::SHL_I32 => shlop!(self, instr, i32, from_i32),
            Opcode::SHL_I64 => shlop!(self, instr, i64, from_i64),

            Opcode::SHR_I8 => shrop!(self, instr, i8, from_i8),
            Opcode::SHR_I16 => shrop!(self, instr, i16, from_i16),
            Opcode::SHR_I32 => shrop!(self, instr, i32, from_i32),
            Opcode::SHR_I64 => shrop!(self, instr, i64, from_i64),

            Opcode::USHR_I8 => shrop!(self, instr, u8, from_u8),
            Opcode::USHR_I16 => shrop!(self, instr, u16, from_u16),
            Opcode::USHR_I32 => shrop!(self, instr, u32, from_u32),
            Opcode::USHR_I64 => shrop!(self, instr, u64, from_u64),

            // ═══════════════════════════════════════════════════════
            // control flow
            // ═══════════════════════════════════════════════════════

            // pc <- pc + sBx
            Opcode::JMP => {
                let offset = instr.sbx_ab();
                // offset is relative to current instruction
                self.stack.jump(offset - 1);
            }

            // if Ra != 0 then pc <- pc + sBx
            Opcode::JMPIF => {
                let a = instr.a();
                let offset = instr.sbx();
                let val = unsafe { self.stack.get_reg(a).u64 };
                if val != 0 {
                    self.stack.jump(offset - 1);
                }
            }

            // if Ra == 0 then pc <- pc + sBx
            Opcode::JMPIFNOT => {
                let a = instr.a();
                let offset = instr.sbx();
                let val = unsafe { self.stack.get_reg(a).u64 };
                if val == 0 {
                    self.stack.jump(offset - 1);
                }
            }

            // ═══════════════════════════════════════════════════════
            // functions
            // ═══════════════════════════════════════════════════════

            // Ra <- closure from proto[Bx]
            // (for now, just stores proto index - real closures would capture upvalues)
            Opcode::CLOSURE => {
                let a = instr.a();
                let bx = instr.bx();
                let frame = self.stack.current().ok_or(VMError::StackUnderflow)?;
                if bx as usize >= frame.chunk.protos.len() {
                    return Err(VMError::InvalidProtoIndex(bx));
                }
                // store proto index in register (simplified - real closure would be object)
                self.stack.set_reg(a, Register::from_u64(bx as u64));
            }

            // call function in Ra with B args, C returns
            Opcode::CALL => {
                let a = instr.a();
                let proto_idx = unsafe { self.stack.get_reg(a).u64 } as usize;
                let frame = self.stack.current().ok_or(VMError::StackUnderflow)?;
                if proto_idx >= frame.chunk.protos.len() {
                    return Err(VMError::InvalidProtoIndex(proto_idx as u16));
                }
                let proto = frame.chunk.protos[proto_idx].clone();
                self.stack.push_frame(proto);
            }

            // return from current frame
            Opcode::RET => {
                self.stack.pop_frame();
                if self.stack.is_empty() {
                    return Err(VMError::Halted);
                }
            }

            // ═══════════════════════════════════════════════════════
            // globals
            // ═══════════════════════════════════════════════════════

            // Ra <- globals[Bx]
            Opcode::GETG => {
                let a = instr.a();
                let gx = instr.bx() as usize;
                let val = if gx < self.globals.len() {
                    self.globals[gx]
                } else {
                    Register::zero()
                };
                self.stack.set_reg(a, val);
            }

            // globals[Bx] <- Ra
            Opcode::SETG => {
                let a = instr.a();
                let gx = instr.bx() as usize;
                let val = self.stack.get_reg(a);
                if gx >= self.globals.len() {
                    self.globals.resize(gx + 1, Register::zero());
                }
                self.globals[gx] = val;
            }

            // ═══════════════════════════════════════════════════════
            // conversion: Ra <- convert(Rb) where B=src, C=types
            // C encodes: (from_type << 4) | to_type
            // ═══════════════════════════════════════════════════════

            Opcode::CONV => {
                let a = instr.a();
                let b = instr.b();
                let c = instr.c();
                let from_type = c >> 4;
                let to_type = c & 0x0F;

                let from =
                    ValueType::from_byte(from_type).ok_or(VMError::InvalidConversionType(from_type))?;
                let to =
                    ValueType::from_byte(to_type).ok_or(VMError::InvalidConversionType(to_type))?;

                let src = self.stack.get_reg(b);
                let result = unsafe { convert_register(src, from, to) };
                self.stack.set_reg(a, result);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::types::instr::Instruction;
    use crate::common::types::value::Constant;

    fn run_chunk(chunk: Chunk) -> Result<VM, VMError> {
        let mut vm = VM::new(1024);
        vm.run(chunk)?;
        Ok(vm)
    }

    #[test]
    fn test_halt() {
        let mut chunk = Chunk::new();
        chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));
        assert!(run_chunk(chunk).is_ok());
    }

    #[test]
    fn test_loadk() {
        let mut chunk = Chunk::new();
        chunk.max_registers = 1;
        let idx = chunk.add_constant(Constant::I64(42));
        chunk.emit(Instruction::abx(Opcode::LOADK, 0, idx));
        chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));

        let mut vm = VM::new(1024);
        vm.run(chunk).unwrap();
    }

    #[test]
    fn test_add_i64() {
        let mut chunk = Chunk::new();
        chunk.max_registers = 3;

        // R0 = 10, R1 = 20, R2 = R0 + R1
        let c0 = chunk.add_constant(Constant::I64(10));
        let c1 = chunk.add_constant(Constant::I64(20));
        chunk.emit(Instruction::abx(Opcode::LOADK, 0, c0));
        chunk.emit(Instruction::abx(Opcode::LOADK, 1, c1));
        chunk.emit(Instruction::abc(Opcode::ADD_I64, 2, 0, 1));
        chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));

        let mut vm = VM::new(1024);
        vm.stack.push_frame(Rc::new(chunk));
        vm.step().unwrap(); // LOADK R0
        vm.step().unwrap(); // LOADK R1
        vm.step().unwrap(); // ADD_I64

        unsafe {
            assert_eq!(vm.stack.get_reg(2).i64, 30);
        }
    }

    #[test]
    fn test_add_f64() {
        let mut chunk = Chunk::new();
        chunk.max_registers = 3;

        let c0 = chunk.add_constant(Constant::F64(1.5));
        let c1 = chunk.add_constant(Constant::F64(2.5));
        chunk.emit(Instruction::abx(Opcode::LOADK, 0, c0));
        chunk.emit(Instruction::abx(Opcode::LOADK, 1, c1));
        chunk.emit(Instruction::abc(Opcode::ADD_F64, 2, 0, 1));
        chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));

        let mut vm = VM::new(1024);
        vm.stack.push_frame(Rc::new(chunk));
        vm.step().unwrap();
        vm.step().unwrap();
        vm.step().unwrap();

        unsafe {
            assert_eq!(vm.stack.get_reg(2).f64, 4.0);
        }
    }

    #[test]
    fn test_comparison() {
        let mut chunk = Chunk::new();
        chunk.max_registers = 3;

        // R0 = 5, R1 = 10, R2 = (R0 < R1)
        let c0 = chunk.add_constant(Constant::I64(5));
        let c1 = chunk.add_constant(Constant::I64(10));
        chunk.emit(Instruction::abx(Opcode::LOADK, 0, c0));
        chunk.emit(Instruction::abx(Opcode::LOADK, 1, c1));
        chunk.emit(Instruction::abc(Opcode::LT_I64, 2, 0, 1));
        chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));

        let mut vm = VM::new(1024);
        vm.stack.push_frame(Rc::new(chunk));
        vm.step().unwrap();
        vm.step().unwrap();
        vm.step().unwrap();

        unsafe {
            assert_eq!(vm.stack.get_reg(2).u64, 1); // true
        }
    }

    #[test]
    fn test_jmpif() {
        let mut chunk = Chunk::new();
        chunk.max_registers = 2;

        let c1 = chunk.add_constant(Constant::I64(1));
        let c100 = chunk.add_constant(Constant::I64(100));
        let c200 = chunk.add_constant(Constant::I64(200));

        chunk.emit(Instruction::abx(Opcode::LOADK, 0, c1)); // 0: R0 = 1
        chunk.emit(Instruction::asbx(Opcode::JMPIF, 0, 2)); // 1: if R0 jump +2
        chunk.emit(Instruction::abx(Opcode::LOADK, 1, c100)); // 2: R1 = 100 (skipped)
        chunk.emit(Instruction::abx(Opcode::LOADK, 1, c200)); // 3: R1 = 200
        chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0)); // 4: HALT

        let mut vm = VM::new(1024);
        vm.stack.push_frame(Rc::new(chunk));
        vm.step().unwrap(); // LOADK R0 = 1
        vm.step().unwrap(); // JMPIF -> jumps to 3
        vm.step().unwrap(); // LOADK R1 = 200

        unsafe {
            assert_eq!(vm.stack.get_reg(1).i64, 200);
        }
    }

    #[test]
    fn test_loop() {
        let mut chunk = Chunk::new();
        chunk.max_registers = 3;

        let c5 = chunk.add_constant(Constant::I64(5));
        let c0 = chunk.add_constant(Constant::I64(0));
        let c1 = chunk.add_constant(Constant::I64(1));

        chunk.emit(Instruction::abx(Opcode::LOADK, 0, c5)); // 0: R0 = 5
        chunk.emit(Instruction::abx(Opcode::LOADK, 1, c0)); // 1: R1 = 0
        chunk.emit(Instruction::asbx(Opcode::JMPIFNOT, 0, 5)); // 2: if R0 == 0 jump +5
        chunk.emit(Instruction::abc(Opcode::ADD_I64, 1, 1, 0)); // 3: R1 = R1 + R0
        chunk.emit(Instruction::abx(Opcode::LOADK, 2, c1)); // 4: R2 = 1
        chunk.emit(Instruction::abc(Opcode::SUB_I64, 0, 0, 2)); // 5: R0 = R0 - 1
        chunk.emit(Instruction::jmp(Opcode::JMP, -4)); // 6: jump -4 (to 2)
        chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0)); // 7: HALT

        let mut vm = VM::new(1024);
        vm.run(chunk).unwrap();
    }

    #[test]
    fn test_loop_result() {
        let mut chunk = Chunk::new();
        chunk.max_registers = 3;

        let c5 = chunk.add_constant(Constant::I64(5));
        let c0 = chunk.add_constant(Constant::I64(0));
        let c1 = chunk.add_constant(Constant::I64(1));

        chunk.emit(Instruction::abx(Opcode::LOADK, 0, c5));
        chunk.emit(Instruction::abx(Opcode::LOADK, 1, c0));
        chunk.emit(Instruction::asbx(Opcode::JMPIFNOT, 0, 5));
        chunk.emit(Instruction::abc(Opcode::ADD_I64, 1, 1, 0));
        chunk.emit(Instruction::abx(Opcode::LOADK, 2, c1));
        chunk.emit(Instruction::abc(Opcode::SUB_I64, 0, 0, 2));
        chunk.emit(Instruction::jmp(Opcode::JMP, -4));
        chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));

        let mut vm = VM::new(1024);
        vm.stack.push_frame(Rc::new(chunk));

        loop {
            match vm.step() {
                Ok(()) => continue,
                Err(VMError::Halted) => break,
                Err(e) => panic!("unexpected error: {:?}", e),
            }
        }

        // R1 should be 5+4+3+2+1 = 15
        unsafe {
            assert_eq!(vm.stack.get_reg(1).i64, 15);
        }
    }

    #[test]
    fn test_memory_load_store() {
        let mut chunk = Chunk::new();
        chunk.max_registers = 3;

        // R0 = 0 (base address)
        // R1 = 42
        // store R1 to mem[R0 + 0]
        // R2 = load from mem[R0 + 0]
        // verify R2 == 42

        let c0 = chunk.add_constant(Constant::I64(0));
        let c42 = chunk.add_constant(Constant::I64(42));

        chunk.emit(Instruction::abx(Opcode::LOADK, 0, c0)); // R0 = 0 (base addr)
        chunk.emit(Instruction::abx(Opcode::LOADK, 1, c42)); // R1 = 42
        chunk.emit(Instruction::abc(Opcode::STORE_I64, 0, 0, 1)); // mem[R0+0] = R1
        chunk.emit(Instruction::abc(Opcode::LOAD_I64, 2, 0, 0)); // R2 = mem[R0+0]
        chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));

        let mut vm = VM::new(1024);
        vm.stack.push_frame(Rc::new(chunk));
        vm.step().unwrap(); // LOADK R0
        vm.step().unwrap(); // LOADK R1
        vm.step().unwrap(); // STORE
        vm.step().unwrap(); // LOAD

        unsafe {
            assert_eq!(vm.stack.get_reg(2).i64, 42);
        }
    }

    #[test]
    fn test_globals() {
        let mut chunk = Chunk::new();
        chunk.max_registers = 2;

        // R0 = 42
        // globals[0] = R0
        // R1 = globals[0]
        // verify R1 == 42

        let c42 = chunk.add_constant(Constant::I64(42));

        chunk.emit(Instruction::abx(Opcode::LOADK, 0, c42)); // R0 = 42
        chunk.emit(Instruction::abx(Opcode::SETG, 0, 0)); // globals[0] = R0
        chunk.emit(Instruction::abx(Opcode::GETG, 1, 0)); // R1 = globals[0]
        chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));

        let mut vm = VM::new(1024);
        vm.stack.push_frame(Rc::new(chunk));
        vm.step().unwrap(); // LOADK
        vm.step().unwrap(); // SETG
        vm.step().unwrap(); // GETG

        unsafe {
            assert_eq!(vm.stack.get_reg(1).i64, 42);
        }
    }

    #[test]
    fn test_call_ret() {
        // main chunk calls inner function that sets R0 = 100 then returns
        // after return, main continues and halts

        // inner function (proto 0)
        let mut inner = Chunk::new();
        inner.max_registers = 1;
        let c100 = inner.add_constant(Constant::I64(100));
        inner.emit(Instruction::abx(Opcode::LOADK, 0, c100)); // R0 = 100
        inner.emit(Instruction::abc(Opcode::RET, 0, 0, 0)); // return

        // main chunk
        let mut main_chunk = Chunk::new();
        main_chunk.max_registers = 2;
        let proto_idx = main_chunk.add_proto(inner); // add inner as proto[0]

        main_chunk.emit(Instruction::abx(Opcode::CLOSURE, 0, proto_idx)); // R0 = closure(proto[0])
        main_chunk.emit(Instruction::abc(Opcode::CALL, 0, 0, 0)); // call R0
        main_chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0)); // halt

        let mut vm = VM::new(1024);
        vm.stack.push_frame(Rc::new(main_chunk));

        vm.step().unwrap(); // CLOSURE
        assert_eq!(vm.stack.depth(), 1);

        vm.step().unwrap(); // CALL -> pushes new frame
        assert_eq!(vm.stack.depth(), 2);

        vm.step().unwrap(); // LOADK in inner
        vm.step().unwrap(); // RET -> pops frame
        assert_eq!(vm.stack.depth(), 1);

        // should be at HALT now
        match vm.step() {
            Err(VMError::Halted) => {}
            other => panic!("expected Halted, got {:?}", other),
        }
    }

    #[test]
    fn test_nested_calls() {
        // main -> foo -> bar -> ret -> ret -> halt
        // tests multiple nested function calls

        // bar: just returns
        let mut bar = Chunk::new();
        bar.max_registers = 1;
        bar.emit(Instruction::abc(Opcode::RET, 0, 0, 0));

        // foo: calls bar then returns
        let mut foo = Chunk::new();
        foo.max_registers = 1;
        let bar_idx = foo.add_proto(bar);
        foo.emit(Instruction::abx(Opcode::CLOSURE, 0, bar_idx)); // R0 = bar
        foo.emit(Instruction::abc(Opcode::CALL, 0, 0, 0)); // call bar
        foo.emit(Instruction::abc(Opcode::RET, 0, 0, 0)); // return

        // main: calls foo then halts
        let mut main_chunk = Chunk::new();
        main_chunk.max_registers = 1;
        let foo_idx = main_chunk.add_proto(foo);
        main_chunk.emit(Instruction::abx(Opcode::CLOSURE, 0, foo_idx)); // R0 = foo
        main_chunk.emit(Instruction::abc(Opcode::CALL, 0, 0, 0)); // call foo
        main_chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0)); // halt

        let mut vm = VM::new(1024);
        vm.stack.push_frame(Rc::new(main_chunk));

        // main: CLOSURE
        vm.step().unwrap();
        assert_eq!(vm.stack.depth(), 1);

        // main: CALL -> enters foo
        vm.step().unwrap();
        assert_eq!(vm.stack.depth(), 2);

        // foo: CLOSURE
        vm.step().unwrap();
        assert_eq!(vm.stack.depth(), 2);

        // foo: CALL -> enters bar
        vm.step().unwrap();
        assert_eq!(vm.stack.depth(), 3);

        // bar: RET -> back to foo
        vm.step().unwrap();
        assert_eq!(vm.stack.depth(), 2);

        // foo: RET -> back to main
        vm.step().unwrap();
        assert_eq!(vm.stack.depth(), 1);

        // main: HALT
        match vm.step() {
            Err(VMError::Halted) => {}
            other => panic!("expected Halted, got {:?}", other),
        }
    }

    #[test]
    fn test_conv_i32_to_i64() {
        let mut chunk = Chunk::new();
        chunk.max_registers = 2;

        // R0 = 42 (as i32)
        // R1 = conv(R0, I32 -> I64)
        // verify R1 == 42 (as i64)

        let c42 = chunk.add_constant(Constant::I32(42));

        // conv type encoding: (from << 4) | to
        // I32 = 2, I64 = 3 -> (2 << 4) | 3 = 35
        let conv_i32_to_i64 = (2 << 4) | 3;

        chunk.emit(Instruction::abx(Opcode::LOADK, 0, c42)); // R0 = 42 (i32)
        chunk.emit(Instruction::abc(Opcode::CONV, 1, 0, conv_i32_to_i64)); // R1 = conv(R0, i32->i64)
        chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));

        let mut vm = VM::new(1024);
        vm.stack.push_frame(Rc::new(chunk));
        vm.step().unwrap(); // LOADK
        vm.step().unwrap(); // CONV

        unsafe {
            assert_eq!(vm.stack.get_reg(1).i64, 42);
        }
    }

    #[test]
    fn test_conv_f64_to_i64() {
        let mut chunk = Chunk::new();
        chunk.max_registers = 2;

        let c_pi = chunk.add_constant(Constant::F64(3.14159));

        // F64 = 9, I64 = 3 -> (9 << 4) | 3 = 147
        let conv_f64_to_i64 = (9 << 4) | 3;

        chunk.emit(Instruction::abx(Opcode::LOADK, 0, c_pi)); // R0 = 3.14159
        chunk.emit(Instruction::abc(Opcode::CONV, 1, 0, conv_f64_to_i64)); // R1 = conv(R0, f64->i64)
        chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));

        let mut vm = VM::new(1024);
        vm.stack.push_frame(Rc::new(chunk));
        vm.step().unwrap(); // LOADK
        vm.step().unwrap(); // CONV

        unsafe {
            assert_eq!(vm.stack.get_reg(1).i64, 3); // truncated
        }
    }

    #[test]
    fn test_conv_i64_to_f64() {
        let mut chunk = Chunk::new();
        chunk.max_registers = 2;

        let c42 = chunk.add_constant(Constant::I64(42));

        // I64 = 3, F64 = 9 -> (3 << 4) | 9 = 57
        let conv_i64_to_f64 = (3 << 4) | 9;

        chunk.emit(Instruction::abx(Opcode::LOADK, 0, c42)); // R0 = 42
        chunk.emit(Instruction::abc(Opcode::CONV, 1, 0, conv_i64_to_f64)); // R1 = conv(R0, i64->f64)
        chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));

        let mut vm = VM::new(1024);
        vm.stack.push_frame(Rc::new(chunk));
        vm.step().unwrap(); // LOADK
        vm.step().unwrap(); // CONV

        unsafe {
            assert_eq!(vm.stack.get_reg(1).f64, 42.0);
        }
    }
}
