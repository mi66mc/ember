use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

use crate::bytecode::{
    Callable, Chunk, Constant, Function, Instruction, Module, Opcode, ValueType,
};
use crate::vm::memory::Memory;
use crate::vm::native::{NativeError, NativeLinker};
use crate::vm::register::{Register, VmValue};
use crate::vm::stack::{CallStack, Frame};

// Token-threaded dispatch: each opcode handler is a standalone function.
// The main loop fetches the opcode byte, indexes the table, and calls the handler.
type OpHandler = fn(&mut Vm, Instruction, u32) -> Result<(), VMError>;

static DISPATCH: OnceLock<[OpHandler; 256]> = OnceLock::new();

fn dispatch_table() -> &'static [OpHandler; 256] {
    DISPATCH.get_or_init(|| {
        let mut table: [OpHandler; 256] = [op_halt as OpHandler; 256];
        table[Opcode::LOADK as usize] = op_loadk;
        table[Opcode::MOVE as usize] = op_move;
        table[Opcode::LOAD_I8 as usize] = op_load_i8;
        table[Opcode::LOAD_I16 as usize] = op_load_i16;
        table[Opcode::LOAD_I32 as usize] = op_load_i32;
        table[Opcode::LOAD_I64 as usize] = op_load_i64;
        table[Opcode::LOAD_U8 as usize] = op_load_u8;
        table[Opcode::LOAD_U16 as usize] = op_load_u16;
        table[Opcode::LOAD_U32 as usize] = op_load_u32;
        table[Opcode::LOAD_U64 as usize] = op_load_u64;
        table[Opcode::LOAD_F32 as usize] = op_load_f32;
        table[Opcode::LOAD_F64 as usize] = op_load_f64;
        table[Opcode::STORE_I8 as usize] = op_store_i8;
        table[Opcode::STORE_I16 as usize] = op_store_i16;
        table[Opcode::STORE_I32 as usize] = op_store_i32;
        table[Opcode::STORE_I64 as usize] = op_store_i64;
        table[Opcode::STORE_U8 as usize] = op_store_u8;
        table[Opcode::STORE_U16 as usize] = op_store_u16;
        table[Opcode::STORE_U32 as usize] = op_store_u32;
        table[Opcode::STORE_U64 as usize] = op_store_u64;
        table[Opcode::STORE_F32 as usize] = op_store_f32;
        table[Opcode::STORE_F64 as usize] = op_store_f64;
        table[Opcode::ADD_I8 as usize] = op_add_i8;
        table[Opcode::ADD_I16 as usize] = op_add_i16;
        table[Opcode::ADD_I32 as usize] = op_add_i32;
        table[Opcode::ADD_I64 as usize] = op_add_i64;
        table[Opcode::SUB_I8 as usize] = op_sub_i8;
        table[Opcode::SUB_I16 as usize] = op_sub_i16;
        table[Opcode::SUB_I32 as usize] = op_sub_i32;
        table[Opcode::SUB_I64 as usize] = op_sub_i64;
        table[Opcode::MUL_I8 as usize] = op_mul_i8;
        table[Opcode::MUL_I16 as usize] = op_mul_i16;
        table[Opcode::MUL_I32 as usize] = op_mul_i32;
        table[Opcode::MUL_I64 as usize] = op_mul_i64;
        table[Opcode::DIV_I8 as usize] = op_div_i8;
        table[Opcode::DIV_I16 as usize] = op_div_i16;
        table[Opcode::DIV_I32 as usize] = op_div_i32;
        table[Opcode::DIV_I64 as usize] = op_div_i64;
        table[Opcode::MOD_I8 as usize] = op_mod_i8;
        table[Opcode::MOD_I16 as usize] = op_mod_i16;
        table[Opcode::MOD_I32 as usize] = op_mod_i32;
        table[Opcode::MOD_I64 as usize] = op_mod_i64;
        table[Opcode::NEG_I8 as usize] = op_neg_i8;
        table[Opcode::NEG_I16 as usize] = op_neg_i16;
        table[Opcode::NEG_I32 as usize] = op_neg_i32;
        table[Opcode::NEG_I64 as usize] = op_neg_i64;
        table[Opcode::ADD_U8 as usize] = op_add_u8;
        table[Opcode::ADD_U16 as usize] = op_add_u16;
        table[Opcode::ADD_U32 as usize] = op_add_u32;
        table[Opcode::ADD_U64 as usize] = op_add_u64;
        table[Opcode::SUB_U8 as usize] = op_sub_u8;
        table[Opcode::SUB_U16 as usize] = op_sub_u16;
        table[Opcode::SUB_U32 as usize] = op_sub_u32;
        table[Opcode::SUB_U64 as usize] = op_sub_u64;
        table[Opcode::MUL_U8 as usize] = op_mul_u8;
        table[Opcode::MUL_U16 as usize] = op_mul_u16;
        table[Opcode::MUL_U32 as usize] = op_mul_u32;
        table[Opcode::MUL_U64 as usize] = op_mul_u64;
        table[Opcode::DIV_U8 as usize] = op_div_u8;
        table[Opcode::DIV_U16 as usize] = op_div_u16;
        table[Opcode::DIV_U32 as usize] = op_div_u32;
        table[Opcode::DIV_U64 as usize] = op_div_u64;
        table[Opcode::MOD_U8 as usize] = op_mod_u8;
        table[Opcode::MOD_U16 as usize] = op_mod_u16;
        table[Opcode::MOD_U32 as usize] = op_mod_u32;
        table[Opcode::MOD_U64 as usize] = op_mod_u64;
        table[Opcode::ADD_F32 as usize] = op_add_f32;
        table[Opcode::ADD_F64 as usize] = op_add_f64;
        table[Opcode::SUB_F32 as usize] = op_sub_f32;
        table[Opcode::SUB_F64 as usize] = op_sub_f64;
        table[Opcode::MUL_F32 as usize] = op_mul_f32;
        table[Opcode::MUL_F64 as usize] = op_mul_f64;
        table[Opcode::DIV_F32 as usize] = op_div_f32;
        table[Opcode::DIV_F64 as usize] = op_div_f64;
        table[Opcode::NEG_F32 as usize] = op_neg_f32;
        table[Opcode::NEG_F64 as usize] = op_neg_f64;
        table[Opcode::AND_I8 as usize] = op_and_i8;
        table[Opcode::AND_I16 as usize] = op_and_i16;
        table[Opcode::AND_I32 as usize] = op_and_i32;
        table[Opcode::AND_I64 as usize] = op_and_i64;
        table[Opcode::OR_I8 as usize] = op_or_i8;
        table[Opcode::OR_I16 as usize] = op_or_i16;
        table[Opcode::OR_I32 as usize] = op_or_i32;
        table[Opcode::OR_I64 as usize] = op_or_i64;
        table[Opcode::XOR_I8 as usize] = op_xor_i8;
        table[Opcode::XOR_I16 as usize] = op_xor_i16;
        table[Opcode::XOR_I32 as usize] = op_xor_i32;
        table[Opcode::XOR_I64 as usize] = op_xor_i64;
        table[Opcode::NOT_I8 as usize] = op_not_i8;
        table[Opcode::NOT_I16 as usize] = op_not_i16;
        table[Opcode::NOT_I32 as usize] = op_not_i32;
        table[Opcode::NOT_I64 as usize] = op_not_i64;
        table[Opcode::SHL_I8 as usize] = op_shl_i8;
        table[Opcode::SHL_I16 as usize] = op_shl_i16;
        table[Opcode::SHL_I32 as usize] = op_shl_i32;
        table[Opcode::SHL_I64 as usize] = op_shl_i64;
        table[Opcode::SHR_I8 as usize] = op_shr_i8;
        table[Opcode::SHR_I16 as usize] = op_shr_i16;
        table[Opcode::SHR_I32 as usize] = op_shr_i32;
        table[Opcode::SHR_I64 as usize] = op_shr_i64;
        table[Opcode::USHR_I8 as usize] = op_ushr_i8;
        table[Opcode::USHR_I16 as usize] = op_ushr_i16;
        table[Opcode::USHR_I32 as usize] = op_ushr_i32;
        table[Opcode::USHR_I64 as usize] = op_ushr_i64;
        table[Opcode::EQ_I8 as usize] = op_eq_i8;
        table[Opcode::EQ_I16 as usize] = op_eq_i16;
        table[Opcode::EQ_I32 as usize] = op_eq_i32;
        table[Opcode::EQ_I64 as usize] = op_eq_i64;
        table[Opcode::NE_I8 as usize] = op_ne_i8;
        table[Opcode::NE_I16 as usize] = op_ne_i16;
        table[Opcode::NE_I32 as usize] = op_ne_i32;
        table[Opcode::NE_I64 as usize] = op_ne_i64;
        table[Opcode::LT_I8 as usize] = op_lt_i8;
        table[Opcode::LT_I16 as usize] = op_lt_i16;
        table[Opcode::LT_I32 as usize] = op_lt_i32;
        table[Opcode::LT_I64 as usize] = op_lt_i64;
        table[Opcode::LE_I8 as usize] = op_le_i8;
        table[Opcode::LE_I16 as usize] = op_le_i16;
        table[Opcode::LE_I32 as usize] = op_le_i32;
        table[Opcode::LE_I64 as usize] = op_le_i64;
        table[Opcode::GT_I8 as usize] = op_gt_i8;
        table[Opcode::GT_I16 as usize] = op_gt_i16;
        table[Opcode::GT_I32 as usize] = op_gt_i32;
        table[Opcode::GT_I64 as usize] = op_gt_i64;
        table[Opcode::GE_I8 as usize] = op_ge_i8;
        table[Opcode::GE_I16 as usize] = op_ge_i16;
        table[Opcode::GE_I32 as usize] = op_ge_i32;
        table[Opcode::GE_I64 as usize] = op_ge_i64;
        table[Opcode::LT_U8 as usize] = op_lt_u8;
        table[Opcode::LT_U16 as usize] = op_lt_u16;
        table[Opcode::LT_U32 as usize] = op_lt_u32;
        table[Opcode::LT_U64 as usize] = op_lt_u64;
        table[Opcode::LE_U8 as usize] = op_le_u8;
        table[Opcode::LE_U16 as usize] = op_le_u16;
        table[Opcode::LE_U32 as usize] = op_le_u32;
        table[Opcode::LE_U64 as usize] = op_le_u64;
        table[Opcode::GT_U8 as usize] = op_gt_u8;
        table[Opcode::GT_U16 as usize] = op_gt_u16;
        table[Opcode::GT_U32 as usize] = op_gt_u32;
        table[Opcode::GT_U64 as usize] = op_gt_u64;
        table[Opcode::GE_U8 as usize] = op_ge_u8;
        table[Opcode::GE_U16 as usize] = op_ge_u16;
        table[Opcode::GE_U32 as usize] = op_ge_u32;
        table[Opcode::GE_U64 as usize] = op_ge_u64;
        table[Opcode::EQ_F32 as usize] = op_eq_f32;
        table[Opcode::EQ_F64 as usize] = op_eq_f64;
        table[Opcode::NE_F32 as usize] = op_ne_f32;
        table[Opcode::NE_F64 as usize] = op_ne_f64;
        table[Opcode::LT_F32 as usize] = op_lt_f32;
        table[Opcode::LT_F64 as usize] = op_lt_f64;
        table[Opcode::LE_F32 as usize] = op_le_f32;
        table[Opcode::LE_F64 as usize] = op_le_f64;
        table[Opcode::GT_F32 as usize] = op_gt_f32;
        table[Opcode::GT_F64 as usize] = op_gt_f64;
        table[Opcode::GE_F32 as usize] = op_ge_f32;
        table[Opcode::GE_F64 as usize] = op_ge_f64;
        table[Opcode::JMP as usize] = op_jmp;
        table[Opcode::JMPIF as usize] = op_jmpif;
        table[Opcode::JMPIFNOT as usize] = op_jmpifnot;
        table[Opcode::TRY as usize] = op_try;
        table[Opcode::ENDTRY as usize] = op_endtry;
        table[Opcode::THROW as usize] = op_throw;
        table[Opcode::GETUPVAL as usize] = op_getupval;
        table[Opcode::CLOSURE as usize] = op_closure;
        table[Opcode::CALL as usize] = op_call;
        table[Opcode::RET as usize] = op_ret;
        table[Opcode::SETUPVAL as usize] = op_setupval;
        table[Opcode::CALLTAIL as usize] = op_calltail;
        table[Opcode::GETG as usize] = op_getg;
        table[Opcode::SETG as usize] = op_setg;
        table[Opcode::CONV as usize] = op_conv;
        table[Opcode::HALT as usize] = op_halt;
        table[Opcode::NOP as usize] = op_nop;
        table[Opcode::EXT as usize] = op_ext;
        table
    })
}

// ── Safety contract for union field access in macros ──────────────
//
// All macros below use `unsafe { $vm.scalar(reg)?.$field }` to read a
// named field from a Register union.  This is sound because:
//
//  * The bytecode compiler emits typed opcodes.  For example, the
//    ADD_I64 opcode is only generated when both operands were stored
//    via Register::from_i64, so reading the `i64` field is valid.
//
//  * The LOAD/STORE macros read `ptr` for address operands.  The
//    compiler guarantees address registers were written via from_ptr.
//
//  * The cmpop macros read a specific field and then write a bool
//    (u64 field), which is always a valid reinterpretation since bool
//    only distinguishes zero / non-zero.

macro_rules! scalar_binop {
    ($vm:ident, $instr:ident, $field:ident, $from:ident, $method:ident) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        // SAFETY: typed opcodes guarantee registers b,c were written by from_<type>;
        // register indices and frame existence validated at compile/load time
        let vb = unsafe { $vm.scalar_unchecked(b).$field };
        let vc = unsafe { $vm.scalar_unchecked(c).$field };
        unsafe { $vm.set_scalar_unchecked(a, Register::$from(vb.$method(vc))); }
    }};
}

macro_rules! float_binop {
    ($vm:ident, $instr:ident, $field:ident, $from:ident, $op:tt) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        // SAFETY: typed opcodes guarantee registers b,c were written by from_<type>;
        // register indices and frame existence validated at compile/load time
        let vb = unsafe { $vm.scalar_unchecked(b).$field };
        let vc = unsafe { $vm.scalar_unchecked(c).$field };
        unsafe { $vm.set_scalar_unchecked(a, Register::$from(vb $op vc)); }
    }};
}

macro_rules! int_divop {
    ($vm:ident, $instr:ident, $field:ident, $from:ident, $method:ident) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        // SAFETY: typed opcodes guarantee registers b,c were written by from_<type>;
        // register indices and frame existence validated at compile/load time
        let vb = unsafe { $vm.scalar_unchecked(b).$field };
        let vc = unsafe { $vm.scalar_unchecked(c).$field };
        if vc == 0 {
            return Err(VMError::DivisionByZero);
        }
        unsafe { $vm.set_scalar_unchecked(a, Register::$from(vb.$method(vc))); }
    }};
}

macro_rules! int_negop {
    ($vm:ident, $instr:ident, $field:ident, $from:ident) => {{
        let (a, b) = ($instr.a(), $instr.b());
        // SAFETY: typed opcodes guarantee register b was written by from_<type>;
        // register indices and frame existence validated at compile/load time
        let vb = unsafe { $vm.scalar_unchecked(b).$field };
        unsafe { $vm.set_scalar_unchecked(a, Register::$from(vb.wrapping_neg())); }
    }};
}

macro_rules! float_negop {
    ($vm:ident, $instr:ident, $field:ident, $from:ident) => {{
        let (a, b) = ($instr.a(), $instr.b());
        // SAFETY: typed opcodes guarantee register b was written by from_<type>;
        // register indices and frame existence validated at compile/load time
        let vb = unsafe { $vm.scalar_unchecked(b).$field };
        unsafe { $vm.set_scalar_unchecked(a, Register::$from(-vb)); }
    }};
}

macro_rules! cmpop {
    ($vm:ident, $instr:ident, $field:ident, $op:tt) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        // SAFETY: typed opcodes guarantee registers b,c were written by from_<type>;
        // register indices and frame existence validated at compile/load time
        let vb = unsafe { $vm.scalar_unchecked(b).$field };
        let vc = unsafe { $vm.scalar_unchecked(c).$field };
        unsafe { $vm.set_scalar_unchecked(a, Register::from_bool(vb $op vc)); }
    }};
}

macro_rules! bitop {
    ($vm:ident, $instr:ident, $field:ident, $from:ident, $op:tt) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        // SAFETY: typed opcodes guarantee registers b,c were written by from_<type>;
        // register indices and frame existence validated at compile/load time
        let vb = unsafe { $vm.scalar_unchecked(b).$field };
        let vc = unsafe { $vm.scalar_unchecked(c).$field };
        unsafe { $vm.set_scalar_unchecked(a, Register::$from(vb $op vc)); }
    }};
}

macro_rules! notop {
    ($vm:ident, $instr:ident, $field:ident, $from:ident) => {{
        let (a, b) = ($instr.a(), $instr.b());
        // SAFETY: typed opcodes guarantee register b was written by from_<type>;
        // register indices and frame existence validated at compile/load time
        let vb = unsafe { $vm.scalar_unchecked(b).$field };
        unsafe { $vm.set_scalar_unchecked(a, Register::$from(!vb)); }
    }};
}

macro_rules! shiftop {
    ($vm:ident, $instr:ident, $field:ident, $from:ident, $method:ident) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        // SAFETY: typed opcodes guarantee registers b,c were written by from_<type>;
        // register indices and frame existence validated at compile/load time
        let vb = unsafe { $vm.scalar_unchecked(b).$field };
        let vc = unsafe { $vm.scalar_unchecked(c).$field };
        unsafe { $vm.set_scalar_unchecked(a, Register::$from(vb.$method(vc as u32))); }
    }};
}

macro_rules! load_macro {
    ($vm:ident, $instr:ident, $typ:ty, $from:ident) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        let pc = unsafe { $vm.stack.current_unchecked() }.pc.wrapping_sub(1);
        let base = unsafe { $vm.scalar_unchecked(b).ptr };
        let addr = base
            .checked_add(c as usize)
                .ok_or(VMError::MemoryOutOfBounds {
                    pc,
                    addr: base,
                    size: size_of::<$typ>(),
                })?;
            let value = $vm
                .memory
                .read_checked::<$typ>(addr)
                .ok_or(VMError::MemoryOutOfBounds {
                    pc,
                    addr,
                    size: size_of::<$typ>(),
                })?;
        unsafe { $vm.set_scalar_unchecked(a, Register::$from(value)); }
    }};
}

macro_rules! store_macro {
    ($vm:ident, $instr:ident, $field:ident, $typ:ty) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        let pc = unsafe { $vm.stack.current_unchecked() }.pc.wrapping_sub(1);
        let base = unsafe { $vm.scalar_unchecked(a).ptr };
        let addr = base
            .checked_add(b as usize)
            .ok_or(VMError::MemoryOutOfBounds {
                pc,
                addr: base,
                size: size_of::<$typ>(),
            })?;
        let value = unsafe { $vm.scalar_unchecked(c).$field };
        if !$vm.memory.write_checked::<$typ>(addr, value) {
            return Err(VMError::MemoryOutOfBounds {
                pc,
                addr,
                size: size_of::<$typ>(),
            });
        }
    }};
}

fn convert_register(src: Register, from: ValueType, to: ValueType) -> Register {
    // SAFETY: Each match arm reads the union field that corresponds to the
    // ValueType variant. The bytecode compiler ensures that from matches the
    // last field written to src. Reading any valid field and casting to i64
    // is sound because all fields occupy the same 64-bit storage.
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

// ── Opcode handler functions ──────────────────────────────────────

fn op_halt(_vm: &mut Vm, _instr: Instruction, _ext: u32) -> Result<(), VMError> {
    Err(VMError::Halted)
}

fn op_nop(_vm: &mut Vm, _instr: Instruction, _ext: u32) -> Result<(), VMError> {
    Ok(())
}

fn op_ext(_vm: &mut Vm, _instr: Instruction, _ext: u32) -> Result<(), VMError> {
    Err(VMError::NativeError(
        "unexpected EXT in execute (should have been handled by fetch)".to_string(),
    ))
}

fn op_loadk(vm: &mut Vm, instr: Instruction, ext: u32) -> Result<(), VMError> {
    let bx = if ext != 0 {
        (instr.bx() as u32 | ext) as usize
    } else {
        instr.bx() as usize
    };
    let constant = vm.module()?
        .constants
        .get(bx)
        .ok_or(VMError::InvalidConstantIndex(instr.bx()))?
        .clone();
    match constant {
        Constant::Bytes(_) => {
            let offset = vm.constant_section.get(&bx)
                .copied()
                .ok_or(VMError::InvalidConstantIndex(instr.bx()))?;
            vm.set_scalar(instr.a(), Register::from_ptr(offset))?;
        }
        constant => vm.set_scalar(
            instr.a(),
            Register {
                bits: constant
                    .to_bits()
                    .expect("non-bytes constants always have scalar bits"),
            },
        )?,
    }
    Ok(())
}

fn op_move(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    let a = instr.a();
    let b = instr.b();
    let src_val;
    {
        let frame = unsafe { vm.stack.current_unchecked() };
        let val = unsafe { frame.get_unchecked(b) };
        if let VmValue::Scalar(r) = val {
            src_val = VmValue::scalar(*r);
        } else {
            src_val = val.clone();
        }
    }
    let frame = unsafe { vm.stack.current_mut_unchecked() };
    unsafe { frame.set_unchecked(a, src_val); }
    Ok(())
}

fn op_load_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    load_macro!(vm, instr, i8, from_i8);
    Ok(())
}
fn op_load_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    load_macro!(vm, instr, i16, from_i16);
    Ok(())
}
fn op_load_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    load_macro!(vm, instr, i32, from_i32);
    Ok(())
}
fn op_load_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    load_macro!(vm, instr, i64, from_i64);
    Ok(())
}
fn op_load_u8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    load_macro!(vm, instr, u8, from_u8);
    Ok(())
}
fn op_load_u16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    load_macro!(vm, instr, u16, from_u16);
    Ok(())
}
fn op_load_u32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    load_macro!(vm, instr, u32, from_u32);
    Ok(())
}
fn op_load_u64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    load_macro!(vm, instr, u64, from_u64);
    Ok(())
}
fn op_load_f32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    load_macro!(vm, instr, f32, from_f32);
    Ok(())
}
fn op_load_f64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    load_macro!(vm, instr, f64, from_f64);
    Ok(())
}

fn op_store_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    store_macro!(vm, instr, i8, i8);
    Ok(())
}
fn op_store_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    store_macro!(vm, instr, i16, i16);
    Ok(())
}
fn op_store_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    store_macro!(vm, instr, i32, i32);
    Ok(())
}
fn op_store_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    store_macro!(vm, instr, i64, i64);
    Ok(())
}
fn op_store_u8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    store_macro!(vm, instr, u8, u8);
    Ok(())
}
fn op_store_u16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    store_macro!(vm, instr, u16, u16);
    Ok(())
}
fn op_store_u32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    store_macro!(vm, instr, u32, u32);
    Ok(())
}
fn op_store_u64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    store_macro!(vm, instr, u64, u64);
    Ok(())
}
fn op_store_f32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    store_macro!(vm, instr, f32, f32);
    Ok(())
}
fn op_store_f64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    store_macro!(vm, instr, f64, f64);
    Ok(())
}

// Signed integer arithmetic
fn op_add_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, i8, from_i8, wrapping_add); Ok(()) }
fn op_add_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, i16, from_i16, wrapping_add); Ok(()) }
fn op_add_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, i32, from_i32, wrapping_add); Ok(()) }
fn op_add_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, i64, from_i64, wrapping_add); Ok(()) }
fn op_sub_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, i8, from_i8, wrapping_sub); Ok(()) }
fn op_sub_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, i16, from_i16, wrapping_sub); Ok(()) }
fn op_sub_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, i32, from_i32, wrapping_sub); Ok(()) }
fn op_sub_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, i64, from_i64, wrapping_sub); Ok(()) }
fn op_mul_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, i8, from_i8, wrapping_mul); Ok(()) }
fn op_mul_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, i16, from_i16, wrapping_mul); Ok(()) }
fn op_mul_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, i32, from_i32, wrapping_mul); Ok(()) }
fn op_mul_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, i64, from_i64, wrapping_mul); Ok(()) }
fn op_div_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_divop!(vm, instr, i8, from_i8, wrapping_div); Ok(()) }
fn op_div_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_divop!(vm, instr, i16, from_i16, wrapping_div); Ok(()) }
fn op_div_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_divop!(vm, instr, i32, from_i32, wrapping_div); Ok(()) }
fn op_div_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_divop!(vm, instr, i64, from_i64, wrapping_div); Ok(()) }
fn op_mod_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_divop!(vm, instr, i8, from_i8, wrapping_rem); Ok(()) }
fn op_mod_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_divop!(vm, instr, i16, from_i16, wrapping_rem); Ok(()) }
fn op_mod_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_divop!(vm, instr, i32, from_i32, wrapping_rem); Ok(()) }
fn op_mod_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_divop!(vm, instr, i64, from_i64, wrapping_rem); Ok(()) }
fn op_neg_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_negop!(vm, instr, i8, from_i8); Ok(()) }
fn op_neg_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_negop!(vm, instr, i16, from_i16); Ok(()) }
fn op_neg_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_negop!(vm, instr, i32, from_i32); Ok(()) }
fn op_neg_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_negop!(vm, instr, i64, from_i64); Ok(()) }

// Unsigned integer arithmetic
fn op_add_u8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, u8, from_u8, wrapping_add); Ok(()) }
fn op_add_u16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, u16, from_u16, wrapping_add); Ok(()) }
fn op_add_u32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, u32, from_u32, wrapping_add); Ok(()) }
fn op_add_u64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, u64, from_u64, wrapping_add); Ok(()) }
fn op_sub_u8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, u8, from_u8, wrapping_sub); Ok(()) }
fn op_sub_u16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, u16, from_u16, wrapping_sub); Ok(()) }
fn op_sub_u32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, u32, from_u32, wrapping_sub); Ok(()) }
fn op_sub_u64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, u64, from_u64, wrapping_sub); Ok(()) }
fn op_mul_u8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, u8, from_u8, wrapping_mul); Ok(()) }
fn op_mul_u16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, u16, from_u16, wrapping_mul); Ok(()) }
fn op_mul_u32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, u32, from_u32, wrapping_mul); Ok(()) }
fn op_mul_u64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { scalar_binop!(vm, instr, u64, from_u64, wrapping_mul); Ok(()) }
fn op_div_u8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_divop!(vm, instr, u8, from_u8, wrapping_div); Ok(()) }
fn op_div_u16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_divop!(vm, instr, u16, from_u16, wrapping_div); Ok(()) }
fn op_div_u32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_divop!(vm, instr, u32, from_u32, wrapping_div); Ok(()) }
fn op_div_u64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_divop!(vm, instr, u64, from_u64, wrapping_div); Ok(()) }
fn op_mod_u8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_divop!(vm, instr, u8, from_u8, wrapping_rem); Ok(()) }
fn op_mod_u16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_divop!(vm, instr, u16, from_u16, wrapping_rem); Ok(()) }
fn op_mod_u32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_divop!(vm, instr, u32, from_u32, wrapping_rem); Ok(()) }
fn op_mod_u64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { int_divop!(vm, instr, u64, from_u64, wrapping_rem); Ok(()) }

// Float arithmetic
fn op_add_f32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { float_binop!(vm, instr, f32, from_f32, +); Ok(()) }
fn op_add_f64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { float_binop!(vm, instr, f64, from_f64, +); Ok(()) }
fn op_sub_f32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { float_binop!(vm, instr, f32, from_f32, -); Ok(()) }
fn op_sub_f64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { float_binop!(vm, instr, f64, from_f64, -); Ok(()) }
fn op_mul_f32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { float_binop!(vm, instr, f32, from_f32, *); Ok(()) }
fn op_mul_f64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { float_binop!(vm, instr, f64, from_f64, *); Ok(()) }
fn op_div_f32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { float_binop!(vm, instr, f32, from_f32, /); Ok(()) }
fn op_div_f64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { float_binop!(vm, instr, f64, from_f64, /); Ok(()) }
fn op_neg_f32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { float_negop!(vm, instr, f32, from_f32); Ok(()) }
fn op_neg_f64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { float_negop!(vm, instr, f64, from_f64); Ok(()) }

// Bitwise ops
fn op_and_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { bitop!(vm, instr, i8, from_i8, &); Ok(()) }
fn op_and_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { bitop!(vm, instr, i16, from_i16, &); Ok(()) }
fn op_and_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { bitop!(vm, instr, i32, from_i32, &); Ok(()) }
fn op_and_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { bitop!(vm, instr, i64, from_i64, &); Ok(()) }
fn op_or_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { bitop!(vm, instr, i8, from_i8, |); Ok(()) }
fn op_or_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { bitop!(vm, instr, i16, from_i16, |); Ok(()) }
fn op_or_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { bitop!(vm, instr, i32, from_i32, |); Ok(()) }
fn op_or_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { bitop!(vm, instr, i64, from_i64, |); Ok(()) }
fn op_xor_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { bitop!(vm, instr, i8, from_i8, ^); Ok(()) }
fn op_xor_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { bitop!(vm, instr, i16, from_i16, ^); Ok(()) }
fn op_xor_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { bitop!(vm, instr, i32, from_i32, ^); Ok(()) }
fn op_xor_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { bitop!(vm, instr, i64, from_i64, ^); Ok(()) }
fn op_not_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { notop!(vm, instr, i8, from_i8); Ok(()) }
fn op_not_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { notop!(vm, instr, i16, from_i16); Ok(()) }
fn op_not_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { notop!(vm, instr, i32, from_i32); Ok(()) }
fn op_not_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { notop!(vm, instr, i64, from_i64); Ok(()) }
fn op_shl_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { shiftop!(vm, instr, i8, from_i8, wrapping_shl); Ok(()) }
fn op_shl_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { shiftop!(vm, instr, i16, from_i16, wrapping_shl); Ok(()) }
fn op_shl_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { shiftop!(vm, instr, i32, from_i32, wrapping_shl); Ok(()) }
fn op_shl_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { shiftop!(vm, instr, i64, from_i64, wrapping_shl); Ok(()) }
fn op_shr_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { shiftop!(vm, instr, i8, from_i8, wrapping_shr); Ok(()) }
fn op_shr_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { shiftop!(vm, instr, i16, from_i16, wrapping_shr); Ok(()) }
fn op_shr_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { shiftop!(vm, instr, i32, from_i32, wrapping_shr); Ok(()) }
fn op_shr_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { shiftop!(vm, instr, i64, from_i64, wrapping_shr); Ok(()) }
fn op_ushr_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { shiftop!(vm, instr, u8, from_u8, wrapping_shr); Ok(()) }
fn op_ushr_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { shiftop!(vm, instr, u16, from_u16, wrapping_shr); Ok(()) }
fn op_ushr_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { shiftop!(vm, instr, u32, from_u32, wrapping_shr); Ok(()) }
fn op_ushr_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { shiftop!(vm, instr, u64, from_u64, wrapping_shr); Ok(()) }

// Signed comparisons
fn op_eq_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i8, ==); Ok(()) }
fn op_eq_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i16, ==); Ok(()) }
fn op_eq_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i32, ==); Ok(()) }
fn op_eq_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i64, ==); Ok(()) }
fn op_ne_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i8, !=); Ok(()) }
fn op_ne_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i16, !=); Ok(()) }
fn op_ne_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i32, !=); Ok(()) }
fn op_ne_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i64, !=); Ok(()) }
fn op_lt_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i8, <); Ok(()) }
fn op_lt_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i16, <); Ok(()) }
fn op_lt_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i32, <); Ok(()) }
fn op_lt_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i64, <); Ok(()) }
fn op_le_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i8, <=); Ok(()) }
fn op_le_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i16, <=); Ok(()) }
fn op_le_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i32, <=); Ok(()) }
fn op_le_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i64, <=); Ok(()) }
fn op_gt_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i8, >); Ok(()) }
fn op_gt_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i16, >); Ok(()) }
fn op_gt_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i32, >); Ok(()) }
fn op_gt_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i64, >); Ok(()) }
fn op_ge_i8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i8, >=); Ok(()) }
fn op_ge_i16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i16, >=); Ok(()) }
fn op_ge_i32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i32, >=); Ok(()) }
fn op_ge_i64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, i64, >=); Ok(()) }

// Unsigned comparisons
fn op_lt_u8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, u8, <); Ok(()) }
fn op_lt_u16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, u16, <); Ok(()) }
fn op_lt_u32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, u32, <); Ok(()) }
fn op_lt_u64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, u64, <); Ok(()) }
fn op_le_u8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, u8, <=); Ok(()) }
fn op_le_u16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, u16, <=); Ok(()) }
fn op_le_u32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, u32, <=); Ok(()) }
fn op_le_u64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, u64, <=); Ok(()) }
fn op_gt_u8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, u8, >); Ok(()) }
fn op_gt_u16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, u16, >); Ok(()) }
fn op_gt_u32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, u32, >); Ok(()) }
fn op_gt_u64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, u64, >); Ok(()) }
fn op_ge_u8(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, u8, >=); Ok(()) }
fn op_ge_u16(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, u16, >=); Ok(()) }
fn op_ge_u32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, u32, >=); Ok(()) }
fn op_ge_u64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, u64, >=); Ok(()) }

// Float comparisons
fn op_eq_f32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, f32, ==); Ok(()) }
fn op_eq_f64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, f64, ==); Ok(()) }
fn op_ne_f32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, f32, !=); Ok(()) }
fn op_ne_f64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, f64, !=); Ok(()) }
fn op_lt_f32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, f32, <); Ok(()) }
fn op_lt_f64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, f64, <); Ok(()) }
fn op_le_f32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, f32, <=); Ok(()) }
fn op_le_f64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, f64, <=); Ok(()) }
fn op_gt_f32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, f32, >); Ok(()) }
fn op_gt_f64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, f64, >); Ok(()) }
fn op_ge_f32(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, f32, >=); Ok(()) }
fn op_ge_f64(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> { cmpop!(vm, instr, f64, >=); Ok(()) }

// Jumps
fn op_jmp(vm: &mut Vm, instr: Instruction, ext: u32) -> Result<(), VMError> {
    let offset = if ext != 0 {
        let effective_offset = ((instr.a() as u16 as i16) | ((instr.b() as i16) << 8)) as i32 | (ext as i32);
        effective_offset as i16
    } else {
        instr.sbx_ab()
    };
    vm.jump(offset - 1)?;
    Ok(())
}

fn op_jmpif(vm: &mut Vm, instr: Instruction, ext: u32) -> Result<(), VMError> {
    let offset = if ext != 0 {
        let effective_offset = ((instr.a() as u16 as i16) | ((instr.b() as i16) << 8)) as i32 | (ext as i32);
        effective_offset as i16
    } else {
        instr.sbx()
    };
    // SAFETY: scalar_unchecked returns a Register; reading u64 from any
    // Register is sound since all fields occupy the same 64 bits
    if unsafe { vm.scalar_unchecked(instr.a()).u64 } != 0 {
        vm.jump(offset - 1)?;
    }
    Ok(())
}

fn op_jmpifnot(vm: &mut Vm, instr: Instruction, ext: u32) -> Result<(), VMError> {
    let offset = if ext != 0 {
        let effective_offset = ((instr.a() as u16 as i16) | ((instr.b() as i16) << 8)) as i32 | (ext as i32);
        effective_offset as i16
    } else {
        instr.sbx()
    };
    // SAFETY: scalar_unchecked returns a Register; reading u64 from any
    // Register is sound since all fields occupy the same 64 bits
    if unsafe { vm.scalar_unchecked(instr.a()).u64 } == 0 {
        vm.jump(offset - 1)?;
    }
    Ok(())
}

// Exception handling
fn op_try(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    let offset = instr.bx() as i16 as isize;
    let handler_pc = unsafe { vm.stack.current_unchecked()
        .pc.wrapping_sub(1).wrapping_add(offset as usize) };
    unsafe { vm.stack
        .current_mut_unchecked()
        .push_handler(handler_pc as u32); }
    Ok(())
}

fn op_endtry(vm: &mut Vm, _instr: Instruction, _ext: u32) -> Result<(), VMError> {
    unsafe { vm.stack
        .current_mut_unchecked()
        .pop_handler(); }
    Ok(())
}

fn op_throw(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    let value = unsafe { vm.value_unchecked(instr.a()) };
    if let Some(handler_pc) = unsafe { vm.stack.current_unchecked()
        .current_handler() }
    {
        unsafe {
            vm.stack
                .current_mut_unchecked()
                .pc = handler_pc as usize;
            vm.set_value_unchecked(0, value);
        }
        unsafe {
            vm.stack
                .current_mut_unchecked()
                .pop_handler();
        }
    } else {
        vm.stack.pop_frame();
        if vm.stack.is_empty() {
            return Err(VMError::Runtime {
                message: "uncaught exception".to_string(),
                backtrace: vec![],
            });
        }
        return Err(VMError::Thrown(value));
    }
    Ok(())
}

// Upvalues
fn op_getupval(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    let dest = instr.a();
    let idx = instr.b() as usize;
    let closure_reg = instr.c();
    let value = match unsafe { vm.value_unchecked(closure_reg) } {
        VmValue::Closure { upvalues, .. } => {
            let upvalues = unsafe { &*upvalues.get() };
            upvalues.get(idx).cloned()
        }
        _ => return Err(VMError::ExpectedFunction(closure_reg)),
    }
    .ok_or(VMError::InvalidRegister(idx as u8))?;
    unsafe { vm.set_value_unchecked(dest, value); }
    Ok(())
}

fn op_setupval(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    let src = instr.a();
    let idx = instr.b() as usize;
    let closure_reg = instr.c();
    let value = unsafe { vm.value_unchecked(src) };
    let frame = unsafe { vm
        .stack
        .current_mut_unchecked() };
    let slot = unsafe { frame
        .get_mut_unchecked(closure_reg) };
    match slot {
        VmValue::Closure { upvalues, .. } => {
            let upvalues = unsafe { &mut *upvalues.get() };
            if idx >= upvalues.len() {
                return Err(VMError::InvalidRegister(idx as u8));
            }
            upvalues[idx] = value;
        }
        _ => return Err(VMError::ExpectedFunction(closure_reg)),
    }
    Ok(())
}

// Closure
fn op_closure(vm: &mut Vm, instr: Instruction, ext: u32) -> Result<(), VMError> {
    let upvalue_count = instr.c() as usize;
    let callable_idx = if ext != 0 {
        (instr.b() as u32 | (ext >> 16)) as usize
    } else {
        instr.b() as usize
    };
    let closure = {
        let module = vm.module()?;
        match module
            .callables
            .get(callable_idx)
            .ok_or(VMError::InvalidCallableIndex(callable_idx as u16))?
        {
            Callable::Function(function_id) => {
                let function = module
                    .functions
                    .get(*function_id as usize)
                    .ok_or(VMError::InvalidFunctionIndex(*function_id))?;
                let mut upvalues = Vec::with_capacity(upvalue_count);
                let frame = unsafe { vm.stack.current_unchecked() };
                let reg_count = frame.chunk.max_registers as usize;
                for i in 0..upvalue_count {
                    let reg_idx = reg_count - upvalue_count + i;
                    upvalues.push(
                        unsafe { frame
                            .get_unchecked(reg_idx as u8) }
                            .clone(),
                    );
                }
                VmValue::closure(Rc::new(function.chunk.clone()), upvalues)
            }
            Callable::Import(import_idx) => {
                let import_decl = module
                    .imports
                    .get(*import_idx as usize)
                    .ok_or(VMError::InvalidCallableIndex(callable_idx as u16))?;
                let resolved = vm
                    .linker
                    .resolve(import_decl)
                    .ok_or_else(|| {
                        VMError::UnresolvedNativeImport(import_decl.to_string())
                    })?;
                VmValue::native_import(resolved)
            }
        }
    };
    vm.set_value(instr.a(), closure)?;
    Ok(())
}

// Call / Return
fn op_call(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    vm.call(instr.a(), instr.b(), instr.c())
}

fn op_ret(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    vm.ret(instr.a(), instr.b())
}

// Tail call
fn op_calltail(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    let base = instr.a();
    let arg_count = instr.b();

    let mut args = Vec::with_capacity(arg_count as usize);
    for index in 0..arg_count {
        let src = base
            .checked_add(1)
            .and_then(|v| v.checked_add(index))
            .ok_or(VMError::InvalidRegister(base))?;
        args.push(unsafe { vm.value_unchecked(src) });
    }

    let callable = unsafe { vm.value_unchecked(base) };

    if let Some(function) = callable.as_function() {
        let frame = unsafe { vm.stack.current_mut_unchecked() };
        frame.set_chunk(function);
        frame.pc = 0;
        for (i, arg) in args.into_iter().enumerate() {
            unsafe { frame.set_unchecked(i as u8, arg); }
        }
        // Store function reference for nested CALLTAIL
        unsafe { frame.set_unchecked(arg_count, callable); }
    } else if let Some(idx) = callable.as_native_import() {
        let returns = vm
            .linker
            .call(idx, &args, &mut vm.memory)
            .map_err(|e| VMError::NativeError(e.message))?;
        for (i, val) in returns.into_iter().enumerate() {
            let tgt = base
                .checked_add(i as u8)
                .ok_or(VMError::InvalidRegister(base))?;
            unsafe { vm.set_value_unchecked(tgt, val); }
        }
        return vm.ret(base, 0);
    } else if let Some(closure) = callable.as_closure() {
        let (chunk, _upvalues) = closure;
        let frame = unsafe { vm.stack.current_mut_unchecked() };
        frame.set_chunk(chunk.clone());
        frame.pc = 0;
        for (i, arg) in args.into_iter().enumerate() {
            unsafe { frame.set_unchecked(i as u8, arg); }
        }
    } else {
        return Err(VMError::ExpectedFunction(base));
    }
    Ok(())
}

// Globals
fn op_getg(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    let value = vm
        .globals
        .get(instr.bx() as usize)
        .cloned()
        .unwrap_or_default();
    vm.set_value(instr.a(), value)?;
    Ok(())
}

fn op_setg(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    const MAX_GLOBALS: usize = 256;
    let gx = instr.bx() as usize;
    if gx >= MAX_GLOBALS {
        return Err(VMError::NativeError("global index out of range".to_string()));
    }
    let value = unsafe { vm.value_unchecked(instr.a()) };
    if gx >= vm.globals.len() {
        vm.globals.resize(gx + 1, VmValue::default());
    }
    vm.globals[gx] = value;
    Ok(())
}

// Conversion
fn op_conv(vm: &mut Vm, instr: Instruction, _ext: u32) -> Result<(), VMError> {
    let from_type = instr.c() >> 4;
    let to_type = instr.c() & 0x0F;
    let from = ValueType::from_byte(from_type)
        .ok_or(VMError::InvalidConversionType(from_type))?;
    let to =
        ValueType::from_byte(to_type).ok_or(VMError::InvalidConversionType(to_type))?;
    let result = convert_register(unsafe { vm.scalar_unchecked(instr.b()) }, from, to);
    vm.set_scalar(instr.a(), result)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugAction {
    Continue,
    Step,
    Break,
}

pub type DebugHook = Box<dyn Fn(&Frame, usize, Option<u32>) -> DebugAction + Send>;

#[derive(Debug, PartialEq, Eq)]
pub enum VMError {
    StackUnderflow,
    DivisionByZero,
    InvalidConstantIndex(u16),
    InvalidCallableIndex(u16),
    InvalidFunctionIndex(u32),
    InvalidConversionType(u8),
    InvalidProgramCounter { pc: usize, len: usize },
    InvalidRegister(u8),
    ExpectedScalar(u8),
    ExpectedFunction(u8),
    UnresolvedNativeImport(String),
    NativeError(String),
    InvalidJump { from: usize, offset: i16 },
    MemoryOutOfBounds { pc: usize, addr: usize, size: usize },
    Runtime {
        message: String,
        backtrace: Vec<FrameInfo>,
    },
    Thrown(VmValue),
    Halted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameInfo {
    pub function_name: String,
    pub pc: usize,
    pub source_line: Option<u32>,
}

pub struct Vm {
    pub(crate) stack: CallStack,
    pub(crate) memory: Memory,
    pub(crate) globals: Vec<VmValue>,
    pub(crate) linker: NativeLinker,
    module: Option<Rc<Module>>,
    constant_section: HashMap<usize, usize>,
    pub(crate) debug_hook: Option<DebugHook>,
}

pub type VM = Vm;

impl Vm {
    pub fn new(memory_size: usize) -> Self {
        Vm {
            stack: CallStack::new(),
            memory: Memory::new(memory_size),
            globals: Vec::new(),
            linker: NativeLinker::default(),
            module: None,
            constant_section: HashMap::new(),
            debug_hook: None,
        }
    }

    pub fn with_linker(memory_size: usize, linker: NativeLinker) -> Self {
        Vm {
            stack: CallStack::new(),
            memory: Memory::new(memory_size),
            globals: Vec::new(),
            linker,
            module: None,
            constant_section: HashMap::new(),
            debug_hook: None,
        }
    }

    pub fn run(&mut self, chunk: Chunk) -> Result<(), VMError> {
        let mut module = Module::new("<chunk>");
        module.entry = Some(0);
        module.functions.push(Function { 
            name: "main".to_string(),
            chunk,
        });
        self.run_module(module)
    }

    pub fn run_module(&mut self, module: Module) -> Result<(), VMError> {
        let entry_idx = module
            .entry
            .ok_or(VMError::NativeError("module has no entry point".to_string()))?;
        let entry = module
            .functions
            .get(entry_idx as usize)
            .ok_or(VMError::InvalidFunctionIndex(entry_idx))?
            .chunk
            .clone();
        let entry_name = module
            .functions
            .get(entry_idx as usize)
            .map(|f| f.name.clone())
            .unwrap_or_else(|| "main".to_string());
        if entry.max_registers > crate::vm::stack::MAX_REGISTERS {
            return Err(VMError::NativeError(format!(
                "entry function has {} registers, max is {}",
                entry.max_registers,
                crate::vm::stack::MAX_REGISTERS
            )));
        }
        // Pre-load Bytes constants into VM memory at startup.
        // Offsets are stored (not raw pointers) to survive Vec reallocations.
        self.constant_section.clear();
        for (idx, constant) in module.constants.iter().enumerate() {
            if let Constant::Bytes(bytes) = constant {
                let offset = self.memory.alloc(bytes.len());
                unsafe {
                    let dst = self.memory.as_mut_ptr().add(offset);
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
                }
                self.constant_section.insert(idx, offset);
            }
        }
        self.module = Some(Rc::new(module));
        self.stack.push_entry(Rc::new(entry), entry_name);

        let dispatch = dispatch_table();

        loop {
            let frame = unsafe { self.stack.current_unchecked() };
            if frame.pc >= frame.code_len {
                return Ok(());
            }

            // Fetch instruction, handle EXT inline
            let mut instr = unsafe { *frame.code_ptr.add(frame.pc) };
            let mut extended_bits: u32 = 0;

            if instr.opcode_byte == Opcode::EXT as u8 {
                let extra = ((instr.c as u16) << 8) | instr.b as u16;
                unsafe { self.stack.current_mut_unchecked() }.pc += 1;
                let frame = unsafe { self.stack.current_unchecked() };
                if frame.pc >= frame.code_len {
                    return Err(VMError::InvalidProgramCounter { pc: frame.pc, len: frame.code_len });
                }
                instr = unsafe { *frame.code_ptr.add(frame.pc) };
                extended_bits = (extra as u32) << 16;
            }

            unsafe { self.stack.current_mut_unchecked() }.pc += 1;

            let handler = dispatch[instr.opcode_byte as usize];
            match handler(self, instr, extended_bits) {
                Ok(()) => {}
                Err(VMError::Halted) => {
                    self.stack.pop_frame();
                    self.module = None;
                    return Ok(());
                }
                Err(VMError::Thrown(value)) => {
                    if let Some(handler_pc) = unsafe {
                        self.stack
                            .current_unchecked()
                            .current_handler()
                    } {
                        unsafe {
                            self.stack
                                .current_mut_unchecked()
                                .pc = handler_pc as usize;
                            self.set_value_unchecked(0, value);
                        }
                        unsafe {
                            self.stack
                                .current_mut_unchecked()
                                .pop_handler();
                        }
                        continue;
                    } else {
                        self.stack.pop_frame();
                        if self.stack.is_empty() {
                            return Err(VMError::Runtime {
                                message: "uncaught exception".to_string(),
                                backtrace: vec![],
                            });
                        }
                        return Err(VMError::Thrown(value));
                    }
                }
                Err(mut error) => {
                    if !matches!(&error, VMError::Runtime { .. }) {
                        let backtrace: Vec<FrameInfo> = self.stack.frames().iter().rev().map(|f| {
                            FrameInfo {
                                function_name: f.function_name.clone(),
                                pc: f.pc,
                                source_line: f.chunk.source_location(f.pc).map(|l| l.line),
                            }
                        }).collect();
                        error = VMError::Runtime {
                            message: format!("{error:?}"),
                            backtrace,
                        };
                    }
                    self.module = None;
                    return Err(error);
                }
            }
        }
    }

    pub fn set_debug_hook(&mut self, hook: DebugHook) {
        self.debug_hook = Some(hook);
    }

    pub fn clear_debug_hook(&mut self) {
        self.debug_hook = None;
    }

    pub fn step(&mut self) -> Result<(), VMError> {
        let instr = self.fetch()?;
        if let Some(ref hook) = self.debug_hook {
            if let Some(frame) = self.stack.current() {
                let source_line = frame.chunk.source_location(frame.pc).map(|l| l.line);
                match hook(frame, frame.pc, source_line) {
                    DebugAction::Continue => {}
                    DebugAction::Step => {}
                    DebugAction::Break => {
                        self.clear_debug_hook();
                        return Ok(());
                    }
                }
            }
        }
        self.stack.advance_pc();

        let mut extended_bits: u32 = 0;
        let instr = if instr.opcode() == Opcode::EXT {
            let extra = ((instr.c() as u16) << 8) | instr.b() as u16;
            let next = self.fetch()?;
            self.stack.advance_pc();
            extended_bits = (extra as u32) << 16;
            next
        } else {
            instr
        };

        let dispatch = dispatch_table();
        dispatch[instr.opcode_byte as usize](self, instr, extended_bits)
    }

    pub fn scalar(&self, register: u8) -> Result<Register, VMError> {
        self.stack
            .current()
            .ok_or(VMError::StackUnderflow)?
            .get(register)
            .ok_or(VMError::InvalidRegister(register))?
            .as_scalar()
            .ok_or(VMError::ExpectedScalar(register))
    }

    #[inline(always)]
    pub unsafe fn scalar_unchecked(&self, register: u8) -> Register {
        let frame = unsafe { self.stack.current_unchecked() };
        let val = unsafe { frame.get_unchecked(register) };
        Register { bits: unsafe { val.scalar_bits_unchecked() } }
    }

    pub fn value(&self, register: u8) -> Result<VmValue, VMError> {
        self.stack
            .current()
            .ok_or(VMError::StackUnderflow)?
            .get(register)
            .cloned()
            .ok_or(VMError::InvalidRegister(register))
    }

    #[inline(always)]
    pub unsafe fn value_unchecked(&self, register: u8) -> VmValue {
        unsafe { self.stack.current_unchecked().get_unchecked(register).clone() }
    }

    pub fn set_scalar(&mut self, register: u8, value: Register) -> Result<(), VMError> {
        self.set_value(register, VmValue::scalar(value))
    }

    #[inline(always)]
    pub unsafe fn set_scalar_unchecked(&mut self, register: u8, value: Register) {
        unsafe { self.set_value_unchecked(register, VmValue::scalar(value)); }
    }

    pub fn set_value(&mut self, register: u8, value: VmValue) -> Result<(), VMError> {
        let frame = self.stack.current_mut().ok_or(VMError::StackUnderflow)?;
        if frame.set(register, value) {
            Ok(())
        } else {
            Err(VMError::InvalidRegister(register))
        }
    }

    #[inline(always)]
    pub unsafe fn set_value_unchecked(&mut self, register: u8, value: VmValue) {
        unsafe { self.stack.current_mut_unchecked().set_unchecked(register, value); }
    }

    pub fn collect_roots(&self) -> Vec<usize> {
        let mut roots = Vec::new();
        for frame in self.stack.frames() {
            roots.extend(frame.collect_roots());
        }
        roots
    }

    pub fn alloc_managed(&mut self, type_tag: u8, size: usize) -> usize {
        let roots = self.collect_roots();
        self.memory.alloc_managed(type_tag, size, &roots)
    }

    fn fetch(&self) -> Result<Instruction, VMError> {
        let frame = unsafe { self.stack.current_unchecked() };
        if frame.pc >= frame.code_len {
            return Err(VMError::InvalidProgramCounter {
                pc: frame.pc,
                len: frame.code_len,
            });
        }
        // SAFETY: bounds checked above
        Ok(unsafe { *frame.code_ptr.add(frame.pc) })
    }

    fn module(&self) -> Result<&Module, VMError> {
        self.module.as_deref().ok_or(VMError::StackUnderflow)
    }

    fn jump(&mut self, offset: i16) -> Result<(), VMError> {
        let frame = unsafe { self.stack.current_unchecked() };
        let from = frame.pc;
        let target = from as isize + offset as isize;
        if target < 0 {
            return Err(VMError::InvalidJump { from, offset });
        }
        self.stack.jump(offset);
        Ok(())
    }

    fn call(&mut self, base: u8, arg_count: u8, expected_returns: u8) -> Result<(), VMError> {
        let callable = unsafe { self.value_unchecked(base) };

        let mut args = Vec::with_capacity(arg_count as usize);
        for index in 0..arg_count {
            let source = base
                .checked_add(1)
                .and_then(|value| value.checked_add(index))
                .ok_or(VMError::InvalidRegister(base))?;
            args.push(unsafe { self.value_unchecked(source) });
        }

        if let Some(function) = callable.as_function() {
            if arg_count as usize > function.max_registers as usize {
                return Err(VMError::InvalidRegister(arg_count));
            }

            self.stack.push_call(function.clone(), base, expected_returns, "anon");
            for (index, value) in args.into_iter().enumerate() {
                unsafe { self.set_value_unchecked(index as u8, value); }
            }
            unsafe { self.set_value_unchecked(arg_count, callable); }
            return Ok(());
        }

        if let Some((chunk, _)) = callable.as_closure() {
            if arg_count as usize > chunk.max_registers as usize {
                return Err(VMError::InvalidRegister(arg_count));
            }

            let chunk_clone = chunk.clone();
            self.stack.push_call(chunk_clone, base, expected_returns, "anon");
            unsafe { self.set_value_unchecked(arg_count, callable); }
            for (index, value) in args.into_iter().enumerate() {
                unsafe { self.set_value_unchecked(index as u8, value); }
            }
            return Ok(());
        }

        if let Some(function) = callable.as_native_import() {
            let returns = self
                .linker
                .call(function, &args, &mut self.memory)
                .map_err(|NativeError { message }| VMError::NativeError(message))?;
            let copy_count = usize::min(expected_returns as usize, returns.len());
            for (index, value) in returns.into_iter().take(copy_count).enumerate() {
                let target = base
                    .checked_add(index as u8)
                    .ok_or(VMError::InvalidRegister(base))?;
                unsafe { self.set_value_unchecked(target, value); }
            }
            for index in copy_count..expected_returns as usize {
                let target = base
                    .checked_add(index as u8)
                    .ok_or(VMError::InvalidRegister(base))?;
                unsafe { self.set_value_unchecked(target, VmValue::default()); }
            }
            return Ok(());
        }

        Err(VMError::ExpectedFunction(base))
    }

    fn ret(&mut self, base: u8, returned_count: u8) -> Result<(), VMError> {
        let mut returns = Vec::with_capacity(returned_count as usize);
        for index in 0..returned_count {
            let source = base
                .checked_add(index)
                .ok_or(VMError::InvalidRegister(base))?;
            returns.push(unsafe { self.value_unchecked(source) });
        }

        let frame = self.stack.pop_frame().ok_or(VMError::StackUnderflow)?;
        let Some(return_base) = frame.return_base else {
            return Err(VMError::Halted);
        };

        let copy_count = usize::min(frame.expected_returns as usize, returns.len());
        for (index, value) in returns.into_iter().take(copy_count).enumerate() {
            let target = return_base
                .checked_add(index as u8)
                .ok_or(VMError::InvalidRegister(return_base))?;
            unsafe { self.set_value_unchecked(target, value); }
        }
        for index in copy_count..frame.expected_returns as usize {
            let target = return_base
                .checked_add(index as u8)
                .ok_or(VMError::InvalidRegister(return_base))?;
            unsafe { self.set_value_unchecked(target, VmValue::default()); }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{Constant, Instruction};

    fn run_module(module: Module) -> Result<Vm, VMError> {
        let mut vm = Vm::new(1024);
        vm.run_module(module)?;
        Ok(vm)
    }

    fn module_for(chunk: Chunk) -> Module {
        let mut module = Module::new("test");
        module.entry = Some(0);
        module.functions.push(Function { 
            name: "main".to_string(),
            chunk,
        });
        module
    }

    #[test]
    fn halt_finishes_run() {
        let mut chunk = Chunk::new();
        chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));
        assert!(run_module(module_for(chunk)).is_ok());
    }

    #[test]
    fn arithmetic_and_comparison_work() {
        let mut chunk = Chunk::new();
        chunk.max_registers = 4;
        chunk.emit(Instruction::abx(Opcode::LOADK, 0, 0));
        chunk.emit(Instruction::abx(Opcode::LOADK, 1, 1));
        chunk.emit(Instruction::abc(Opcode::ADD_I64, 2, 0, 1));
        chunk.emit(Instruction::abc(Opcode::LT_I64, 3, 0, 1));

        let mut module = module_for(chunk);
        module.constants.push(Constant::I64(10));
        module.constants.push(Constant::I64(20));
        let mut vm = Vm::new(1024);
        vm.module = Some(Rc::new(module));
        let entry = vm.module.as_ref().unwrap().functions[0].chunk.clone();
        vm.stack.push_entry(Rc::new(entry), "test");
        vm.step().unwrap();
        vm.step().unwrap();
        vm.step().unwrap();
        vm.step().unwrap();

        // SAFETY: tests verify exact register values produced by the bytecode;
        // reading i64/u64 fields is sound because LOADK writes the matching type
        unsafe {
            assert_eq!(vm.scalar(2).unwrap().i64, 30);
            assert_eq!(vm.scalar(3).unwrap().u64, 1);
        }
    }

    #[test]
    fn call_with_args_and_one_return() {
        let mut add = Chunk::new();
        add.max_registers = 3;
        add.emit(Instruction::abc(Opcode::ADD_I64, 2, 0, 1));
        add.emit(Instruction::abc(Opcode::RET, 2, 1, 0));

        let mut main = Chunk::new();
        main.max_registers = 4;
        main.emit(Instruction::abc(Opcode::CLOSURE, 0, 0, 0));
        main.emit(Instruction::abx(Opcode::LOADK, 1, 0));
        main.emit(Instruction::abx(Opcode::LOADK, 2, 1));
        main.emit(Instruction::abc(Opcode::CALL, 0, 2, 1));
        main.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));

        let mut module = Module::new("test");
        module.entry = Some(0);
        module.constants = vec![Constant::I64(10), Constant::I64(20)];
        module.callables = vec![Callable::Function(1)];
        module.functions.push(Function { 
            name: "main".to_string(),
            chunk: main,
        });
        module.functions.push(Function { 
            name: "add".to_string(),
            chunk: add,
        });

        let mut vm = Vm::new(1024);
        vm.module = Some(Rc::new(module));
        let entry = vm.module.as_ref().unwrap().functions[0].chunk.clone();
        vm.stack.push_entry(Rc::new(entry), "test");
        loop {
            match vm.step() {
                Ok(()) => {}
                Err(VMError::Halted) => break,
                Err(error) => panic!("unexpected error: {error:?}"),
            }
        }

        // SAFETY: tests verify exact register values produced by the bytecode;
        // reading i64 is sound because LOADK_I64 writes the i64 field
        unsafe {
            assert_eq!(vm.scalar(0).unwrap().i64, 30);
        }
    }

    #[test]
    fn call_with_multiple_returns() {
        let mut pair = Chunk::new();
        pair.max_registers = 2;
        pair.emit(Instruction::abx(Opcode::LOADK, 0, 0));
        pair.emit(Instruction::abx(Opcode::LOADK, 1, 1));
        pair.emit(Instruction::abc(Opcode::RET, 0, 2, 0));

        let mut main = Chunk::new();
        main.max_registers = 3;
        main.emit(Instruction::abc(Opcode::CLOSURE, 0, 0, 0));
        main.emit(Instruction::abc(Opcode::CALL, 0, 0, 2));
        main.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));

        let mut module = Module::new("test");
        module.entry = Some(0);
        module.constants = vec![Constant::I64(1), Constant::I64(2)];
        module.callables = vec![Callable::Function(1)];
        module.functions.push(Function { 
            name: "main".to_string(),
            chunk: main,
        });
        module.functions.push(Function { 
            name: "pair".to_string(),
            chunk: pair,
        });
        let mut vm = Vm::new(1024);
        vm.module = Some(Rc::new(module));
        let entry = vm.module.as_ref().unwrap().functions[0].chunk.clone();
        vm.stack.push_entry(Rc::new(entry), "test");
        while !matches!(vm.step(), Err(VMError::Halted)) {}

        // SAFETY: tests verify exact register values produced by the bytecode;
        // reading i64 is sound because LOADK writes the i64 field
        unsafe {
            assert_eq!(vm.scalar(0).unwrap().i64, 1);
            assert_eq!(vm.scalar(1).unwrap().i64, 2);
        }
    }

    #[test]
    fn nested_calls_return_to_callers() {
        let mut leaf = Chunk::new();
        leaf.max_registers = 1;
        leaf.emit(Instruction::abx(Opcode::LOADK, 0, 0));
        leaf.emit(Instruction::abc(Opcode::RET, 0, 1, 0));

        let mut middle = Chunk::new();
        middle.max_registers = 1;
        middle.emit(Instruction::abc(Opcode::CLOSURE, 0, 1, 0));
        middle.emit(Instruction::abc(Opcode::CALL, 0, 0, 1));
        middle.emit(Instruction::abc(Opcode::RET, 0, 1, 0));

        let mut main = Chunk::new();
        main.max_registers = 1;
        main.emit(Instruction::abc(Opcode::CLOSURE, 0, 0, 0));
        main.emit(Instruction::abc(Opcode::CALL, 0, 0, 1));
        main.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));

        let mut module = Module::new("test");
        module.entry = Some(0);
        module.constants = vec![Constant::I64(7)];
        module.callables = vec![Callable::Function(1), Callable::Function(2)];
        module.functions.push(Function { 
            name: "main".to_string(),
            chunk: main,
        });
        module.functions.push(Function { 
            name: "middle".to_string(),
            chunk: middle,
        });
        module.functions.push(Function { 
            name: "leaf".to_string(),
            chunk: leaf,
        });
        let mut vm = Vm::new(1024);
        vm.module = Some(Rc::new(module));
        let entry = vm.module.as_ref().unwrap().functions[0].chunk.clone();
        vm.stack.push_entry(Rc::new(entry), "test");
        while !matches!(vm.step(), Err(VMError::Halted)) {}

        // SAFETY: tests verify exact register values produced by the bytecode;
        // reading i64 is sound because LOADK writes the i64 field
        unsafe {
            assert_eq!(vm.scalar(0).unwrap().i64, 7);
        }
    }

    #[test]
    fn invalid_call_target_is_reported() {
        let mut chunk = Chunk::new();
        chunk.max_registers = 1;
        chunk.emit(Instruction::abc(Opcode::CALL, 0, 0, 0));

        let mut vm = Vm::new(1024);
        vm.stack.push_entry(Rc::new(chunk), "test");
        assert_eq!(vm.step(), Err(VMError::ExpectedFunction(0)));
    }

    #[test]
    fn gc_collects_unreachable_objects() {
        let mut vm = Vm::new(1024);

        let obj1 = vm.alloc_managed(1, 16);
        let obj2 = vm.alloc_managed(2, 32);
        let obj3 = vm.alloc_managed(3, 64);

        assert_eq!(vm.memory.managed_type_tag(obj1), 1);
        assert_eq!(vm.memory.managed_type_tag(obj2), 2);
        assert_eq!(vm.memory.managed_type_tag(obj3), 3);
        assert_eq!(vm.memory.gc_allocations.len(), 3);

        vm.memory.collect_gc(&[obj2]);

        assert_eq!(vm.memory.gc_allocations.len(), 1);
        assert!(vm.memory.free_lists.iter().any(|l| !l.is_empty()));
        assert!(!vm.memory.managed_is_marked(obj2));
    }

    #[test]
    fn invalid_pc_register_and_memory_are_reported() {
        let mut empty = Chunk::new();
        empty.max_registers = 1;
        let mut vm = Vm::new(8);
        vm.stack.push_entry(Rc::new(empty), "test");
        assert_eq!(
            vm.step(),
            Err(VMError::InvalidProgramCounter { pc: 0, len: 0 })
        );

        let mut bad_reg = Chunk::new();
        bad_reg.max_registers = 1;
        bad_reg.emit(Instruction::abx(Opcode::LOADK, 2, 0));
        let mut vm = Vm::new(8);
        let mut module = module_for(bad_reg);
        module.constants.push(Constant::I64(1));
        vm.module = Some(Rc::new(module));
        let entry = vm.module.as_ref().unwrap().functions[0].chunk.clone();
        vm.stack.push_entry(Rc::new(entry), "test");
        assert_eq!(vm.step(), Err(VMError::InvalidRegister(2)));

        let mut bad_mem = Chunk::new();
        bad_mem.max_registers = 2;
        bad_mem.emit(Instruction::abx(Opcode::LOADK, 0, 0));
        bad_mem.emit(Instruction::abx(Opcode::LOADK, 1, 1));
        bad_mem.emit(Instruction::abc(Opcode::STORE_I64, 0, 0, 1));
        let mut vm = Vm::new(8);
        let mut module = module_for(bad_mem);
        module.constants = vec![Constant::I64(4), Constant::I64(9)];
        vm.module = Some(Rc::new(module));
        let entry = vm.module.as_ref().unwrap().functions[0].chunk.clone();
        vm.stack.push_entry(Rc::new(entry), "test");
        vm.step().unwrap();
        vm.step().unwrap();
        assert_eq!(
            vm.step(),
            Err(VMError::MemoryOutOfBounds { pc: 2, addr: 4, size: 8 })
        );
    }

    #[test]
    fn throw_and_catch_in_same_frame() {
        let mut chunk = Chunk::new();
        chunk.max_registers = 2;
        chunk.emit(Instruction::abx(Opcode::LOADK, 0, 0));
        chunk.emit(Instruction::abx(Opcode::TRY, 0, 4));  // handler at PC 5, TRY at PC 1, offset = 4
        chunk.emit(Instruction::abc(Opcode::THROW, 0, 0, 0));
        chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));
        chunk.emit(Instruction::abc(Opcode::NOP, 0, 0, 0));
        chunk.emit(Instruction::abc(Opcode::ENDTRY, 0, 0, 0));
        chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));

        let mut module = module_for(chunk);
        module.constants.push(Constant::I64(42));

        let mut vm = Vm::new(1024);
        vm.module = Some(Rc::new(module));
        let entry = vm.module.as_ref().unwrap().functions[0].chunk.clone();
        vm.stack.push_entry(Rc::new(entry), "test");

        vm.step().unwrap(); // LOADK r0, 42
        vm.step().unwrap(); // TRY 5
        vm.step().unwrap(); // THROW r0 — catches and jumps to handler at PC 5
        // After THROW, register 0 should hold the thrown value
        unsafe {
            assert_eq!(vm.scalar(0).unwrap().i64, 42);
        }
        vm.step().unwrap(); // ENDTRY
        assert_eq!(vm.step(), Err(VMError::Halted)); // HALT
    }

    #[test]
    fn throw_without_handler_returns_error() {
        let mut chunk = Chunk::new();
        chunk.max_registers = 1;
        chunk.emit(Instruction::abx(Opcode::LOADK, 0, 0));
        chunk.emit(Instruction::abc(Opcode::THROW, 0, 0, 0));

        let mut module = module_for(chunk);
        module.constants.push(Constant::I64(99));

        let result = run_module(module);
        assert!(result.is_err());
        match result {
            Err(VMError::Runtime { message, .. }) => {
                assert!(message.contains("uncaught exception"));
            }
            _ => panic!("expected Runtime error"),
        }
    }

    #[test]
    fn closure_captures_upvalue_and_returns_it() {
        let mut inner = Chunk::new();
        inner.max_registers = 2;
        inner.emit(Instruction::abc(Opcode::GETUPVAL, 0, 0, 0));
        inner.emit(Instruction::abc(Opcode::RET, 0, 1, 0));

        let mut outer = Chunk::new();
        outer.max_registers = 3;
        outer.emit(Instruction::abx(Opcode::LOADK, 2, 0));
        outer.emit(Instruction::abc(Opcode::CLOSURE, 1, 0, 1));
        outer.emit(Instruction::abc(Opcode::CALL, 1, 0, 1));
        outer.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));

        let mut module = Module::new("test");
        module.entry = Some(0);
        module.constants = vec![Constant::I64(77)];
        module.callables = vec![Callable::Function(1)];
        module.functions.push(Function {
            name: "outer".to_string(),
            chunk: outer,
        });
        module.functions.push(Function {
            name: "inner".to_string(),
            chunk: inner,
        });

        let mut vm = Vm::new(1024);
        vm.module = Some(Rc::new(module));
        let entry = vm.module.as_ref().unwrap().functions[0].chunk.clone();
        vm.stack.push_entry(Rc::new(entry), "test");
        loop {
            match vm.step() {
                Ok(()) => {}
                Err(VMError::Halted) => break,
                Err(error) => panic!("unexpected error: {error:?}"),
            }
        }

        unsafe {
            assert_eq!(vm.scalar(1).unwrap().i64, 77);
        }
    }

    #[test]
    fn tail_call_reuses_frame() {
        let mut inner = Chunk::new();
        inner.max_registers = 2;
        inner.emit(Instruction::abx(Opcode::LOADK, 0, 0));
        inner.emit(Instruction::abc(Opcode::RET, 0, 1, 0));

        let mut outer = Chunk::new();
        outer.max_registers = 3;
        outer.emit(Instruction::abc(Opcode::CLOSURE, 0, 0, 0));
        outer.emit(Instruction::abc(Opcode::CALLTAIL, 0, 0, 0));
        outer.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));

        let mut module = Module::new("test");
        module.entry = Some(0);
        module.constants = vec![Constant::I64(42)];
        module.callables = vec![Callable::Function(1)];
        module.functions.push(Function {
            name: "outer".to_string(),
            chunk: outer,
        });
        module.functions.push(Function {
            name: "inner".to_string(),
            chunk: inner,
        });

        let mut vm = Vm::new(1024);
        vm.module = Some(Rc::new(module));
        let entry = vm.module.as_ref().unwrap().functions[0].chunk.clone();
        vm.stack.push_entry(Rc::new(entry), "outer");
        vm.step().unwrap(); // CLOSURE
        vm.step().unwrap(); // CALLTAIL
        assert_eq!(vm.stack.current().unwrap().pc, 0);
    }

    #[test]
    fn debug_hook_fires_for_each_instruction() {
        let mut chunk = Chunk::new();
        chunk.max_registers = 1;
        chunk.emit(Instruction::abx(Opcode::LOADK, 0, 0));
        chunk.emit(Instruction::abx(Opcode::LOADK, 0, 1));
        chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));

        let mut module = module_for(chunk);
        module.constants = vec![Constant::I64(10), Constant::I64(20)];

        let mut vm = Vm::new(1024);
        vm.module = Some(Rc::new(module));
        let entry = vm.module.as_ref().unwrap().functions[0].chunk.clone();
        vm.stack.push_entry(Rc::new(entry), "test");

        let hook_calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let hook_calls_clone = hook_calls.clone();
        vm.set_debug_hook(Box::new(move |_frame, pc, line| {
            hook_calls_clone.lock().unwrap().push((pc, line));
            DebugAction::Continue
        }));

        loop {
            match vm.step() {
                Ok(()) => {}
                Err(VMError::Halted) => break,
                Err(error) => panic!("unexpected error: {error:?}"),
            }
        }

        let calls = hook_calls.lock().unwrap();
        assert_eq!(calls.len(), 3, "hook should fire for each instruction");
        assert_eq!(calls[0].0, 0);
        assert_eq!(calls[1].0, 1);
        assert_eq!(calls[2].0, 2);
    }

    #[test]
    fn debug_hook_break_stops_execution() {
        let mut chunk = Chunk::new();
        chunk.max_registers = 1;
        chunk.emit(Instruction::abx(Opcode::LOADK, 0, 0));
        chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));

        let mut module = module_for(chunk);
        module.constants = vec![Constant::I64(42)];

        let mut vm = Vm::new(1024);
        vm.module = Some(Rc::new(module));
        let entry = vm.module.as_ref().unwrap().functions[0].chunk.clone();
        vm.stack.push_entry(Rc::new(entry), "test");

        let hook_called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let hook_called_clone = hook_called.clone();
        vm.set_debug_hook(Box::new(move |_frame, _pc, _line| {
            hook_called_clone.store(true, std::sync::atomic::Ordering::Relaxed);
            DebugAction::Break
        }));

        let result = vm.step();
        assert_eq!(result, Ok(()));
        assert!(hook_called.load(std::sync::atomic::Ordering::Relaxed), "hook should have been called");
        assert!(vm.debug_hook.is_none(), "hook should be cleared after Break");
        assert_eq!(vm.stack.current().unwrap().pc, 0, "PC should not advance after Break");

        let result = vm.step();
        assert_eq!(result, Ok(()));
        unsafe {
            assert_eq!(vm.scalar(0).unwrap().i64, 42, "instruction should execute on next step");
        }
    }

    #[test]
    fn debug_hook_step_keeps_hook_active() {
        let mut chunk = Chunk::new();
        chunk.max_registers = 1;
        chunk.emit(Instruction::abx(Opcode::LOADK, 0, 0));
        chunk.emit(Instruction::abx(Opcode::LOADK, 0, 1));
        chunk.emit(Instruction::abc(Opcode::HALT, 0, 0, 0));

        let mut module = module_for(chunk);
        module.constants = vec![Constant::I64(1), Constant::I64(2)];

        let mut vm = Vm::new(1024);
        vm.module = Some(Rc::new(module));
        let entry = vm.module.as_ref().unwrap().functions[0].chunk.clone();
        vm.stack.push_entry(Rc::new(entry), "test");

        let hook_count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let hook_count_clone = hook_count.clone();
        vm.set_debug_hook(Box::new(move |_frame, _pc, _line| {
            hook_count_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            DebugAction::Step
        }));

        loop {
            match vm.step() {
                Ok(()) => {}
                Err(VMError::Halted) => break,
                Err(error) => panic!("unexpected error: {error:?}"),
            }
        }

        assert_eq!(hook_count.load(std::sync::atomic::Ordering::Relaxed), 3, "hook should fire for every instruction with Step");
        assert!(vm.debug_hook.is_some(), "hook should remain set after Step");
    }
}
