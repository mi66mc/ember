use std::rc::Rc;

use crate::bytecode::{
    Callable, Chunk, Constant, Function, Instruction, Module, Opcode, ValueType,
};
use crate::vm::memory::Memory;
use crate::vm::native::{NativeError, NativeLinker};
use crate::vm::register::{Register, VmValue};
use crate::vm::stack::CallStack;

macro_rules! scalar_binop {
    ($vm:ident, $instr:ident, $field:ident, $from:ident, $method:ident) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        let vb = unsafe { $vm.scalar(b)?.$field };
        let vc = unsafe { $vm.scalar(c)?.$field };
        $vm.set_scalar(a, Register::$from(vb.$method(vc)))?;
    }};
}

macro_rules! float_binop {
    ($vm:ident, $instr:ident, $field:ident, $from:ident, $op:tt) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        let vb = unsafe { $vm.scalar(b)?.$field };
        let vc = unsafe { $vm.scalar(c)?.$field };
        $vm.set_scalar(a, Register::$from(vb $op vc))?;
    }};
}

macro_rules! int_divop {
    ($vm:ident, $instr:ident, $field:ident, $from:ident, $method:ident) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        let vb = unsafe { $vm.scalar(b)?.$field };
        let vc = unsafe { $vm.scalar(c)?.$field };
        if vc == 0 {
            return Err(VMError::DivisionByZero);
        }
        $vm.set_scalar(a, Register::$from(vb.$method(vc)))?;
    }};
}

macro_rules! int_negop {
    ($vm:ident, $instr:ident, $field:ident, $from:ident) => {{
        let (a, b) = ($instr.a(), $instr.b());
        let vb = unsafe { $vm.scalar(b)?.$field };
        $vm.set_scalar(a, Register::$from(vb.wrapping_neg()))?;
    }};
}

macro_rules! float_negop {
    ($vm:ident, $instr:ident, $field:ident, $from:ident) => {{
        let (a, b) = ($instr.a(), $instr.b());
        let vb = unsafe { $vm.scalar(b)?.$field };
        $vm.set_scalar(a, Register::$from(-vb))?;
    }};
}

macro_rules! cmpop {
    ($vm:ident, $instr:ident, $field:ident, $op:tt) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        let vb = unsafe { $vm.scalar(b)?.$field };
        let vc = unsafe { $vm.scalar(c)?.$field };
        $vm.set_scalar(a, Register::from_bool(vb $op vc))?;
    }};
}

macro_rules! bitop {
    ($vm:ident, $instr:ident, $field:ident, $from:ident, $op:tt) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        let vb = unsafe { $vm.scalar(b)?.$field };
        let vc = unsafe { $vm.scalar(c)?.$field };
        $vm.set_scalar(a, Register::$from(vb $op vc))?;
    }};
}

macro_rules! notop {
    ($vm:ident, $instr:ident, $field:ident, $from:ident) => {{
        let (a, b) = ($instr.a(), $instr.b());
        let vb = unsafe { $vm.scalar(b)?.$field };
        $vm.set_scalar(a, Register::$from(!vb))?;
    }};
}

macro_rules! shiftop {
    ($vm:ident, $instr:ident, $field:ident, $from:ident, $method:ident) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        let vb = unsafe { $vm.scalar(b)?.$field };
        let vc = unsafe { $vm.scalar(c)?.$field };
        $vm.set_scalar(a, Register::$from(vb.$method(vc as u32)))?;
    }};
}

macro_rules! loadop {
    ($vm:ident, $instr:ident, $typ:ty, $from:ident) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        let base = unsafe { $vm.scalar(b)?.ptr };
        let addr = base
            .checked_add(c as usize)
            .ok_or(VMError::MemoryOutOfBounds {
                addr: base,
                size: size_of::<$typ>(),
            })?;
        let value = $vm
            .memory
            .read_checked::<$typ>(addr)
            .ok_or(VMError::MemoryOutOfBounds {
                addr,
                size: size_of::<$typ>(),
            })?;
        $vm.set_scalar(a, Register::$from(value))?;
    }};
}

macro_rules! storeop {
    ($vm:ident, $instr:ident, $field:ident, $typ:ty) => {{
        let (a, b, c) = ($instr.a(), $instr.b(), $instr.c());
        let base = unsafe { $vm.scalar(a)?.ptr };
        let addr = base
            .checked_add(b as usize)
            .ok_or(VMError::MemoryOutOfBounds {
                addr: base,
                size: size_of::<$typ>(),
            })?;
        let value = unsafe { $vm.scalar(c)?.$field };
        if !$vm.memory.write_checked::<$typ>(addr, value) {
            return Err(VMError::MemoryOutOfBounds {
                addr,
                size: size_of::<$typ>(),
            });
        }
    }};
}

fn convert_register(src: Register, from: ValueType, to: ValueType) -> Register {
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
    MemoryOutOfBounds { addr: usize, size: usize },
    Runtime {
        message: String,
        backtrace: Vec<FrameInfo>,
    },
    Halted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameInfo {
    pub function_name: String,
    pub pc: usize,
    pub source_line: Option<u32>,
}

pub struct Vm {
    pub stack: CallStack,
    pub memory: Memory,
    pub globals: Vec<VmValue>,
    pub linker: NativeLinker,
    module: Option<Rc<Module>>,
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
        }
    }

    pub fn with_linker(memory_size: usize, linker: NativeLinker) -> Self {
        Vm {
            stack: CallStack::new(),
            memory: Memory::new(memory_size),
            globals: Vec::new(),
            linker,
            module: None,
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
        self.module = Some(Rc::new(module));
        self.stack.push_entry(Rc::new(entry), entry_name);

        loop {
            match self.step() {
                Ok(()) => {}
                Err(VMError::Halted) => {
                    self.stack.pop_frame();
                    self.module = None;
                    return Ok(());
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

    pub fn step(&mut self) -> Result<(), VMError> {
        let instr = self.fetch()?;
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

        let effective_bx = instr.bx() as u32 | extended_bits;
        let effective_offset = ((instr.a() as u16 as i16) | ((instr.b() as i16) << 8)) as i32
            | (extended_bits as i32);

        match instr.opcode() {
            Opcode::HALT => return Err(VMError::Halted),
            Opcode::NOP => {}

            Opcode::LOADK => {
                let bx = if extended_bits != 0 {
                    effective_bx as usize
                } else {
                    instr.bx() as usize
                };
                let constant = self.module()?
                    .constants
                    .get(bx)
                    .ok_or(VMError::InvalidConstantIndex(instr.bx()))?
                    .clone();
                match constant {
                    Constant::Bytes(bytes) => {
                        let len = bytes.len();
                        let ptr = self.memory.alloc(len);
                        unsafe {
                            let dst = self.memory.as_mut_ptr().add(ptr);
                            std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, len);
                        }
                        self.set_scalar(instr.a(), Register::from_ptr(ptr))?;
                    }
                    constant => self.set_scalar(
                        instr.a(),
                        Register {
                            bits: constant
                                .to_bits()
                                .expect("non-bytes constants always have scalar bits"),
                        },
                    )?,
                }
            }
            Opcode::MOVE => {
                let value = self.value(instr.b())?;
                self.set_value(instr.a(), value)?;
            }

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

            Opcode::STORE_I8 => storeop!(self, instr, i8, i8),
            Opcode::STORE_I16 => storeop!(self, instr, i16, i16),
            Opcode::STORE_I32 => storeop!(self, instr, i32, i32),
            Opcode::STORE_I64 => storeop!(self, instr, i64, i64),
            Opcode::STORE_U8 => storeop!(self, instr, u8, u8),
            Opcode::STORE_U16 => storeop!(self, instr, u16, u16),
            Opcode::STORE_U32 => storeop!(self, instr, u32, u32),
            Opcode::STORE_U64 => storeop!(self, instr, u64, u64),
            Opcode::STORE_F32 => storeop!(self, instr, f32, f32),
            Opcode::STORE_F64 => storeop!(self, instr, f64, f64),

            Opcode::ADD_I8 => scalar_binop!(self, instr, i8, from_i8, wrapping_add),
            Opcode::ADD_I16 => scalar_binop!(self, instr, i16, from_i16, wrapping_add),
            Opcode::ADD_I32 => scalar_binop!(self, instr, i32, from_i32, wrapping_add),
            Opcode::ADD_I64 => scalar_binop!(self, instr, i64, from_i64, wrapping_add),
            Opcode::SUB_I8 => scalar_binop!(self, instr, i8, from_i8, wrapping_sub),
            Opcode::SUB_I16 => scalar_binop!(self, instr, i16, from_i16, wrapping_sub),
            Opcode::SUB_I32 => scalar_binop!(self, instr, i32, from_i32, wrapping_sub),
            Opcode::SUB_I64 => scalar_binop!(self, instr, i64, from_i64, wrapping_sub),
            Opcode::MUL_I8 => scalar_binop!(self, instr, i8, from_i8, wrapping_mul),
            Opcode::MUL_I16 => scalar_binop!(self, instr, i16, from_i16, wrapping_mul),
            Opcode::MUL_I32 => scalar_binop!(self, instr, i32, from_i32, wrapping_mul),
            Opcode::MUL_I64 => scalar_binop!(self, instr, i64, from_i64, wrapping_mul),
            Opcode::DIV_I8 => int_divop!(self, instr, i8, from_i8, wrapping_div),
            Opcode::DIV_I16 => int_divop!(self, instr, i16, from_i16, wrapping_div),
            Opcode::DIV_I32 => int_divop!(self, instr, i32, from_i32, wrapping_div),
            Opcode::DIV_I64 => int_divop!(self, instr, i64, from_i64, wrapping_div),
            Opcode::MOD_I8 => int_divop!(self, instr, i8, from_i8, wrapping_rem),
            Opcode::MOD_I16 => int_divop!(self, instr, i16, from_i16, wrapping_rem),
            Opcode::MOD_I32 => int_divop!(self, instr, i32, from_i32, wrapping_rem),
            Opcode::MOD_I64 => int_divop!(self, instr, i64, from_i64, wrapping_rem),
            Opcode::NEG_I8 => int_negop!(self, instr, i8, from_i8),
            Opcode::NEG_I16 => int_negop!(self, instr, i16, from_i16),
            Opcode::NEG_I32 => int_negop!(self, instr, i32, from_i32),
            Opcode::NEG_I64 => int_negop!(self, instr, i64, from_i64),

            Opcode::ADD_U8 => scalar_binop!(self, instr, u8, from_u8, wrapping_add),
            Opcode::ADD_U16 => scalar_binop!(self, instr, u16, from_u16, wrapping_add),
            Opcode::ADD_U32 => scalar_binop!(self, instr, u32, from_u32, wrapping_add),
            Opcode::ADD_U64 => scalar_binop!(self, instr, u64, from_u64, wrapping_add),
            Opcode::SUB_U8 => scalar_binop!(self, instr, u8, from_u8, wrapping_sub),
            Opcode::SUB_U16 => scalar_binop!(self, instr, u16, from_u16, wrapping_sub),
            Opcode::SUB_U32 => scalar_binop!(self, instr, u32, from_u32, wrapping_sub),
            Opcode::SUB_U64 => scalar_binop!(self, instr, u64, from_u64, wrapping_sub),
            Opcode::MUL_U8 => scalar_binop!(self, instr, u8, from_u8, wrapping_mul),
            Opcode::MUL_U16 => scalar_binop!(self, instr, u16, from_u16, wrapping_mul),
            Opcode::MUL_U32 => scalar_binop!(self, instr, u32, from_u32, wrapping_mul),
            Opcode::MUL_U64 => scalar_binop!(self, instr, u64, from_u64, wrapping_mul),
            Opcode::DIV_U8 => int_divop!(self, instr, u8, from_u8, wrapping_div),
            Opcode::DIV_U16 => int_divop!(self, instr, u16, from_u16, wrapping_div),
            Opcode::DIV_U32 => int_divop!(self, instr, u32, from_u32, wrapping_div),
            Opcode::DIV_U64 => int_divop!(self, instr, u64, from_u64, wrapping_div),
            Opcode::MOD_U8 => int_divop!(self, instr, u8, from_u8, wrapping_rem),
            Opcode::MOD_U16 => int_divop!(self, instr, u16, from_u16, wrapping_rem),
            Opcode::MOD_U32 => int_divop!(self, instr, u32, from_u32, wrapping_rem),
            Opcode::MOD_U64 => int_divop!(self, instr, u64, from_u64, wrapping_rem),

            Opcode::ADD_F32 => float_binop!(self, instr, f32, from_f32, +),
            Opcode::ADD_F64 => float_binop!(self, instr, f64, from_f64, +),
            Opcode::SUB_F32 => float_binop!(self, instr, f32, from_f32, -),
            Opcode::SUB_F64 => float_binop!(self, instr, f64, from_f64, -),
            Opcode::MUL_F32 => float_binop!(self, instr, f32, from_f32, *),
            Opcode::MUL_F64 => float_binop!(self, instr, f64, from_f64, *),
            Opcode::DIV_F32 => float_binop!(self, instr, f32, from_f32, /),
            Opcode::DIV_F64 => float_binop!(self, instr, f64, from_f64, /),
            Opcode::NEG_F32 => float_negop!(self, instr, f32, from_f32),
            Opcode::NEG_F64 => float_negop!(self, instr, f64, from_f64),

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
            Opcode::NOT_I8 => notop!(self, instr, i8, from_i8),
            Opcode::NOT_I16 => notop!(self, instr, i16, from_i16),
            Opcode::NOT_I32 => notop!(self, instr, i32, from_i32),
            Opcode::NOT_I64 => notop!(self, instr, i64, from_i64),
            Opcode::SHL_I8 => shiftop!(self, instr, i8, from_i8, wrapping_shl),
            Opcode::SHL_I16 => shiftop!(self, instr, i16, from_i16, wrapping_shl),
            Opcode::SHL_I32 => shiftop!(self, instr, i32, from_i32, wrapping_shl),
            Opcode::SHL_I64 => shiftop!(self, instr, i64, from_i64, wrapping_shl),
            Opcode::SHR_I8 => shiftop!(self, instr, i8, from_i8, wrapping_shr),
            Opcode::SHR_I16 => shiftop!(self, instr, i16, from_i16, wrapping_shr),
            Opcode::SHR_I32 => shiftop!(self, instr, i32, from_i32, wrapping_shr),
            Opcode::SHR_I64 => shiftop!(self, instr, i64, from_i64, wrapping_shr),
            Opcode::USHR_I8 => shiftop!(self, instr, u8, from_u8, wrapping_shr),
            Opcode::USHR_I16 => shiftop!(self, instr, u16, from_u16, wrapping_shr),
            Opcode::USHR_I32 => shiftop!(self, instr, u32, from_u32, wrapping_shr),
            Opcode::USHR_I64 => shiftop!(self, instr, u64, from_u64, wrapping_shr),

            Opcode::JMP => {
                let offset = if extended_bits != 0 {
                    effective_offset as i16
                } else {
                    instr.sbx_ab()
                };
                self.jump(offset - 1)?;
            }
            Opcode::JMPIF => {
                let offset = if extended_bits != 0 {
                    effective_offset as i16
                } else {
                    instr.sbx()
                };
                if unsafe { self.scalar(instr.a())?.u64 } != 0 {
                    self.jump(offset - 1)?;
                }
            }
            Opcode::JMPIFNOT => {
                let offset = if extended_bits != 0 {
                    effective_offset as i16
                } else {
                    instr.sbx()
                };
                if unsafe { self.scalar(instr.a())?.u64 } == 0 {
                    self.jump(offset - 1)?;
                }
            }

            Opcode::CLOSURE => {
                let bx = if extended_bits != 0 {
                    effective_bx as usize
                } else {
                    instr.bx() as usize
                };
                let closure = {
                    let module = self.module()?;
                    match module
                        .callables
                        .get(bx)
                        .ok_or(VMError::InvalidCallableIndex(instr.bx()))?
                    {
                        Callable::Function(function_id) => {
                            let function = module
                                .functions
                                .get(*function_id as usize)
                                .ok_or(VMError::InvalidFunctionIndex(*function_id))?;
                            VmValue::function(Rc::new(function.chunk.clone()))
                        }
                        Callable::Import(import_idx) => {
                            let import_decl = module
                                .imports
                                .get(*import_idx as usize)
                                .ok_or(VMError::InvalidCallableIndex(instr.bx()))?;
                            let resolved = self
                                .linker
                                .resolve(import_decl)
                                .ok_or_else(|| {
                                    VMError::UnresolvedNativeImport(import_decl.to_string())
                                })?;
                            VmValue::native_import(resolved)
                        }
                    }
                };
                self.set_value(instr.a(), closure)?;
            }
            Opcode::CALL => self.call(instr.a(), instr.b(), instr.c())?,
            Opcode::RET => self.ret(instr.a(), instr.b())?,

            Opcode::GETG => {
                let value = self
                    .globals
                    .get(instr.bx() as usize)
                    .cloned()
                    .unwrap_or_default();
                self.set_value(instr.a(), value)?;
            }
            Opcode::SETG => {
                let gx = instr.bx() as usize;
                let value = self.value(instr.a())?;
                if gx >= self.globals.len() {
                    self.globals.resize(gx + 1, VmValue::default());
                }
                self.globals[gx] = value;
            }

            Opcode::CONV => {
                let from_type = instr.c() >> 4;
                let to_type = instr.c() & 0x0F;
                let from = ValueType::from_byte(from_type)
                    .ok_or(VMError::InvalidConversionType(from_type))?;
                let to =
                    ValueType::from_byte(to_type).ok_or(VMError::InvalidConversionType(to_type))?;
                let result = convert_register(self.scalar(instr.b())?, from, to);
                self.set_scalar(instr.a(), result)?;
            }
            Opcode::EXT => {
                return Err(VMError::NativeError(
                    "unexpected EXT in execute (should have been handled by fetch)".to_string(),
                ));
            }
        }

        Ok(())
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

    pub fn value(&self, register: u8) -> Result<VmValue, VMError> {
        self.stack
            .current()
            .ok_or(VMError::StackUnderflow)?
            .get(register)
            .cloned()
            .ok_or(VMError::InvalidRegister(register))
    }

    pub fn set_scalar(&mut self, register: u8, value: Register) -> Result<(), VMError> {
        self.set_value(register, VmValue::scalar(value))
    }

    pub fn set_value(&mut self, register: u8, value: VmValue) -> Result<(), VMError> {
        let frame = self.stack.current_mut().ok_or(VMError::StackUnderflow)?;
        if frame.set(register, value) {
            Ok(())
        } else {
            Err(VMError::InvalidRegister(register))
        }
    }

    fn fetch(&self) -> Result<Instruction, VMError> {
        let frame = self.stack.current().ok_or(VMError::StackUnderflow)?;
        frame
            .chunk
            .code
            .get(frame.pc)
            .copied()
            .ok_or(VMError::InvalidProgramCounter {
                pc: frame.pc,
                len: frame.chunk.code.len(),
            })
    }

    fn module(&self) -> Result<&Module, VMError> {
        self.module.as_deref().ok_or(VMError::StackUnderflow)
    }

    fn jump(&mut self, offset: i16) -> Result<(), VMError> {
        let frame = self.stack.current().ok_or(VMError::StackUnderflow)?;
        let from = frame.pc;
        let target = from as isize + offset as isize;
        if target < 0 {
            return Err(VMError::InvalidJump { from, offset });
        }
        self.stack.jump(offset);
        Ok(())
    }

    fn call(&mut self, base: u8, arg_count: u8, expected_returns: u8) -> Result<(), VMError> {
        let callable = self.value(base)?;

        let mut args = Vec::with_capacity(arg_count as usize);
        for index in 0..arg_count {
            let source = base
                .checked_add(1)
                .and_then(|value| value.checked_add(index))
                .ok_or(VMError::InvalidRegister(base))?;
            args.push(self.value(source)?);
        }

        if let Some(function) = callable.as_function() {
            if arg_count as usize > function.max_registers as usize {
                return Err(VMError::InvalidRegister(arg_count));
            }

            self.stack.push_call(function.clone(), base, expected_returns, "anon");
            for (index, value) in args.into_iter().enumerate() {
                self.set_value(index as u8, value)?;
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
                self.set_value(target, value)?;
            }
            for index in copy_count..expected_returns as usize {
                let target = base
                    .checked_add(index as u8)
                    .ok_or(VMError::InvalidRegister(base))?;
                self.set_value(target, VmValue::default())?;
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
            returns.push(self.value(source)?);
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
            self.set_value(target, value)?;
        }
        for index in copy_count..frame.expected_returns as usize {
            let target = return_base
                .checked_add(index as u8)
                .ok_or(VMError::InvalidRegister(return_base))?;
            self.set_value(target, VmValue::default())?;
        }

        Ok(())
    }
}

