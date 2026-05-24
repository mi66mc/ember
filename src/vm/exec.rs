use std::collections::HashMap;
use std::rc::Rc;

use crate::bytecode::{
    Callable, Chunk, Constant, Function, Instruction, Module, Opcode, ValueType,
};
use crate::vm::memory::Memory;
use crate::vm::native::{NativeError, NativeLinker};
use crate::vm::register::{Register, VmValue};
use crate::vm::stack::{CallStack, Frame};

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

        'execute: loop {
            let frame = unsafe { self.stack.current_unchecked() };
            let code_ptr = frame.code_ptr;
            let code_len = frame.code_len;
            let regs_ptr = frame.registers.as_ptr() as *mut VmValue;
            let scalar_ptr = frame.scalar_regs.as_ptr() as *mut u64;
            let frame_pc = frame.pc;
            if frame_pc >= code_len {
                return Ok(());
            }
            let instr = unsafe { *code_ptr.add(frame_pc) };
            let max_regs = frame.chunk.max_registers;

            let (opcode_byte, extended_bits) = if instr.opcode_byte == Opcode::EXT as u8 {
                unsafe { self.stack.current_mut_unchecked() }.pc += 1;
                let next_pc = frame_pc + 1;
                if next_pc >= code_len {
                    return Err(VMError::InvalidProgramCounter { pc: next_pc, len: code_len });
                }
                let next = unsafe { code_ptr.add(next_pc).read() };
                let extra = ((instr.c as u16) << 8) | instr.b as u16;
                (next.opcode_byte, (extra as u32) << 16)
            } else {
                (instr.opcode_byte, 0)
            };
            unsafe { self.stack.current_mut_unchecked() }.pc += 1;

            match opcode_byte {
                    0x00 => {
                        let bx = if extended_bits != 0 { (instr.bx() as u32 | extended_bits) as usize } else { instr.bx() as usize };
                        let constant = self.module.as_ref().unwrap()
                            .constants
                            .get(bx)
                            .ok_or(VMError::InvalidConstantIndex(instr.bx()))?
                            .clone();
                        unsafe {
                            let frame = self.stack.current_mut_unchecked();
                            match constant {
                                Constant::Bytes(_) => {
                                    let offset = self.constant_section.get(&bx)
                                        .copied()
                                        .ok_or(VMError::InvalidConstantIndex(instr.bx()))?;
                                    let val = VmValue::scalar(Register::from_ptr(offset));
                                    unsafe { *scalar_ptr.add(instr.a as usize) = val.as_scalar().unwrap_unchecked().bits; }
                                    if !frame.set(instr.a, val) {
                                        return Err(VMError::InvalidRegister(instr.a));
                                    }
                                }
                                constant => {
                                    let bits = constant.to_bits().expect("non-bytes constants always have scalar bits");
                                    unsafe { *scalar_ptr.add(instr.a as usize) = bits; }
                                    if !frame.set(instr.a, VmValue::scalar(Register { bits })) {
                                        return Err(VMError::InvalidRegister(instr.a));
                                    }
                                }
                            }
                        }
                        continue 'execute;
                    }
                    0x01 => {
                        let src = unsafe { &*regs_ptr.add(instr.b as usize) };
                        let val: Register = match src {
                            VmValue::Scalar(r) => *r,
                            _ => {
                                let cloned = src.clone();
                                let frame = unsafe { self.stack.current_mut_unchecked() };
                                if !frame.set(instr.a, cloned) { return Err(VMError::InvalidRegister(instr.a)); }
                                continue 'execute;
                            }
                        };
                        let bits = unsafe { val.bits };
                        unsafe {
                            *scalar_ptr.add(instr.a as usize) = bits;
                            *regs_ptr.add(instr.a as usize) = VmValue::scalar(val);
                        }
                        continue 'execute;
                    }
                    0x02 => {
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<i8>(),
                        })?;
                        let value = self.memory.read_checked::<i8>(addr).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr, size: size_of::<i8>(),
                        })?;
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i8(value)); }
                        continue 'execute;
                    }
                    0x03 => {
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<i16>(),
                        })?;
                        let value = self.memory.read_checked::<i16>(addr).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr, size: size_of::<i16>(),
                        })?;
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i16(value)); }
                        continue 'execute;
                    }
                    0x04 => {
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<i32>(),
                        })?;
                        let value = self.memory.read_checked::<i32>(addr).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr, size: size_of::<i32>(),
                        })?;
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i32(value)); }
                        continue 'execute;
                    }
                    0x05 => {
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<i64>(),
                        })?;
                        let value = self.memory.read_checked::<i64>(addr).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr, size: size_of::<i64>(),
                        })?;
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(value)); }
                        continue 'execute;
                    }
                    0x06 => {
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<u8>(),
                        })?;
                        let value = self.memory.read_checked::<u8>(addr).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr, size: size_of::<u8>(),
                        })?;
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_u8(value)); }
                        continue 'execute;
                    }
                    0x07 => {
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<u16>(),
                        })?;
                        let value = self.memory.read_checked::<u16>(addr).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr, size: size_of::<u16>(),
                        })?;
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_u16(value)); }
                        continue 'execute;
                    }
                    0x08 => {
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<u32>(),
                        })?;
                        let value = self.memory.read_checked::<u32>(addr).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr, size: size_of::<u32>(),
                        })?;
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_u32(value)); }
                        continue 'execute;
                    }
                    0x09 => {
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<u64>(),
                        })?;
                        let value = self.memory.read_checked::<u64>(addr).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr, size: size_of::<u64>(),
                        })?;
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_u64(value)); }
                        continue 'execute;
                    }
                    0x0A => {
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<f32>(),
                        })?;
                        let value = self.memory.read_checked::<f32>(addr).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr, size: size_of::<f32>(),
                        })?;
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_f32(value)); }
                        continue 'execute;
                    }
                    0x0B => {
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<f64>(),
                        })?;
                        let value = self.memory.read_checked::<f64>(addr).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr, size: size_of::<f64>(),
                        })?;
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_f64(value)); }
                        continue 'execute;
                    }
                    0x0C => {
                        let a = instr.a as usize;
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<i8>(),
                        })?;
                        let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().i8 };
                        if !self.memory.write_checked::<i8>(addr, value) {
                            return Err(VMError::MemoryOutOfBounds {
                                pc: frame_pc, addr, size: size_of::<i8>(),
                            });
                        }
                        continue 'execute;
                    }
                    0x0D => {
                        let a = instr.a as usize;
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<i16>(),
                        })?;
                        let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().i16 };
                        if !self.memory.write_checked::<i16>(addr, value) {
                            return Err(VMError::MemoryOutOfBounds {
                                pc: frame_pc, addr, size: size_of::<i16>(),
                            });
                        }
                        continue 'execute;
                    }
                    0x0E => {
                        let a = instr.a as usize;
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<i32>(),
                        })?;
                        let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().i32 };
                        if !self.memory.write_checked::<i32>(addr, value) {
                            return Err(VMError::MemoryOutOfBounds {
                                pc: frame_pc, addr, size: size_of::<i32>(),
                            });
                        }
                        continue 'execute;
                    }
                    0x0F => {
                        let a = instr.a as usize;
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<i64>(),
                        })?;
                        let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().i64 };
                        if !self.memory.write_checked::<i64>(addr, value) {
                            return Err(VMError::MemoryOutOfBounds {
                                pc: frame_pc, addr, size: size_of::<i64>(),
                            });
                        }
                        continue 'execute;
                    }
                    0x10 => {
                        let a = instr.a as usize;
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<u8>(),
                        })?;
                        let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().u8 };
                        if !self.memory.write_checked::<u8>(addr, value) {
                            return Err(VMError::MemoryOutOfBounds {
                                pc: frame_pc, addr, size: size_of::<u8>(),
                            });
                        }
                        continue 'execute;
                    }
                    0x11 => {
                        let a = instr.a as usize;
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<u16>(),
                        })?;
                        let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().u16 };
                        if !self.memory.write_checked::<u16>(addr, value) {
                            return Err(VMError::MemoryOutOfBounds {
                                pc: frame_pc, addr, size: size_of::<u16>(),
                            });
                        }
                        continue 'execute;
                    }
                    0x12 => {
                        let a = instr.a as usize;
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<u32>(),
                        })?;
                        let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().u32 };
                        if !self.memory.write_checked::<u32>(addr, value) {
                            return Err(VMError::MemoryOutOfBounds {
                                pc: frame_pc, addr, size: size_of::<u32>(),
                            });
                        }
                        continue 'execute;
                    }
                    0x13 => {
                        let a = instr.a as usize;
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<u64>(),
                        })?;
                        let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().u64 };
                        if !self.memory.write_checked::<u64>(addr, value) {
                            return Err(VMError::MemoryOutOfBounds {
                                pc: frame_pc, addr, size: size_of::<u64>(),
                            });
                        }
                        continue 'execute;
                    }
                    0x14 => {
                        let a = instr.a as usize;
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<f32>(),
                        })?;
                        let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().f32 };
                        if !self.memory.write_checked::<f32>(addr, value) {
                            return Err(VMError::MemoryOutOfBounds {
                                pc: frame_pc, addr, size: size_of::<f32>(),
                            });
                        }
                        continue 'execute;
                    }
                    0x15 => {
                        let a = instr.a as usize;
                        let b = instr.b as usize;
                        let c = instr.c as usize;
                        let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                        let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds {
                            pc: frame_pc, addr: base, size: size_of::<f64>(),
                        })?;
                        let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().f64 };
                        if !self.memory.write_checked::<f64>(addr, value) {
                            return Err(VMError::MemoryOutOfBounds {
                                pc: frame_pc, addr, size: size_of::<f64>(),
                            });
                        }
                        continue 'execute;
                    }
                    0x23 => {
                        let vb = unsafe { *scalar_ptr.add(instr.b as usize) } as i64;
                        let vc = unsafe { *scalar_ptr.add(instr.c as usize) } as i64;
                        let result = vb.wrapping_add(vc);
                        unsafe {
                            *scalar_ptr.add(instr.a as usize) = result as u64;
                            *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(result));
                        }
                        continue 'execute;
                    }
                    0x27 => {
                        let vb = unsafe { *scalar_ptr.add(instr.b as usize) } as i64;
                        let vc = unsafe { *scalar_ptr.add(instr.c as usize) } as i64;
                        let result = vb.wrapping_sub(vc);
                        unsafe {
                            *scalar_ptr.add(instr.a as usize) = result as u64;
                            *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(result));
                        }
                        continue 'execute;
                    }
                    0x2B => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb.wrapping_mul(vc))); }
                        continue 'execute;
                    }
                    0x2F => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                        if vc == 0 { return Err(VMError::DivisionByZero); }
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb.wrapping_div(vc))); }
                        continue 'execute;
                    }
                    0x33 => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                        if vc == 0 { return Err(VMError::DivisionByZero); }
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb.wrapping_rem(vc))); }
                        continue 'execute;
                    }
                    0x37 => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb.wrapping_neg())); }
                        continue 'execute;
                    }
                    0x4F => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().u64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().u64 };
                        if vc == 0 { return Err(VMError::DivisionByZero); }
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_u64(vb.wrapping_div(vc))); }
                        continue 'execute;
                    }
                    0x53 => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().u64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().u64 };
                        if vc == 0 { return Err(VMError::DivisionByZero); }
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_u64(vb.wrapping_rem(vc))); }
                        continue 'execute;
                    }
                    0x59 => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_f64(vb + vc)); }
                        continue 'execute;
                    }
                    0x5B => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_f64(vb - vc)); }
                        continue 'execute;
                    }
                    0x5D => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_f64(vb * vc)); }
                        continue 'execute;
                    }
                    0x5F => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_f64(vb / vc)); }
                        continue 'execute;
                    }
                    0x61 => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_f64(-vb)); }
                        continue 'execute;
                    }
                    0x6B => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb & vc)); }
                        continue 'execute;
                    }
                    0x6F => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb | vc)); }
                        continue 'execute;
                    }
                    0x73 => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb ^ vc)); }
                        continue 'execute;
                    }
                    0x77 => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(!vb)); }
                        continue 'execute;
                    }
                    0x7B => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb.wrapping_shl(vc as u32))); }
                        continue 'execute;
                    }
                    0x7F => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb.wrapping_shr(vc as u32))); }
                        continue 'execute;
                    }
                    0x83 => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64((vb as u64).wrapping_shr(vc as u32) as i64)); }
                        continue 'execute;
                    }
                    0x93 => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb == vc)); }
                        continue 'execute;
                    }
                    0x97 => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb != vc)); }
                        continue 'execute;
                    }
                    0x9B => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb < vc)); }
                        continue 'execute;
                    }
                    0x9F => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb <= vc)); }
                        continue 'execute;
                    }
                    0xA3 => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb > vc)); }
                        continue 'execute;
                    }
                    0xA7 => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb >= vc)); }
                        continue 'execute;
                    }
                    0xAB => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().u64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().u64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb < vc)); }
                        continue 'execute;
                    }
                    0xAF => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().u64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().u64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb <= vc)); }
                        continue 'execute;
                    }
                    0xB3 => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().u64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().u64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb > vc)); }
                        continue 'execute;
                    }
                    0xB7 => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().u64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().u64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb >= vc)); }
                        continue 'execute;
                    }
                    0xB9 => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb == vc)); }
                        continue 'execute;
                    }
                    0xBB => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb != vc)); }
                        continue 'execute;
                    }
                    0xBD => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb < vc)); }
                        continue 'execute;
                    }
                    0xBF => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb <= vc)); }
                        continue 'execute;
                    }
                    0xC1 => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb > vc)); }
                        continue 'execute;
                    }
                    0xC3 => {
                        let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                        let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb >= vc)); }
                        continue 'execute;
                    }
                    0xC8 => {
                        let from_type = instr.c >> 4;
                        let to_type = instr.c & 0x0F;
                        let from = ValueType::from_byte(from_type)
                            .ok_or(VMError::InvalidConversionType(from_type))?;
                        let to = ValueType::from_byte(to_type)
                            .ok_or(VMError::InvalidConversionType(to_type))?;
                        let result = convert_register(
                            unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked() },
                            from,
                            to,
                        );
                        unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(result); }
                        continue 'execute;
                    }
                    0xD0 => {
                        let offset = if extended_bits != 0 {
                            (((instr.a as u16 as i16) | ((instr.b as i16) << 8)) as i32 | (extended_bits as i32)) as i16
                        } else {
                            instr.sbx_ab()
                        };
                        self.stack.jump(offset - 1);
                        continue 'execute;
                    }
                    0xD1 => {
                        let offset = if extended_bits != 0 {
                            (((instr.a as u16 as i16) | ((instr.b as i16) << 8)) as i32 | (extended_bits as i32)) as i16
                        } else {
                            instr.sbx()
                        };
                        if unsafe { (*regs_ptr.add(instr.a as usize)).as_scalar().unwrap_unchecked().u64 } != 0 {
                            self.stack.jump(offset - 1);
                        }
                        continue 'execute;
                    }
                    0xD2 => {
                        let offset = if extended_bits != 0 {
                            (((instr.a as u16 as i16) | ((instr.b as i16) << 8)) as i32 | (extended_bits as i32)) as i16
                        } else {
                            instr.sbx()
                        };
                        if unsafe { *scalar_ptr.add(instr.a as usize) } == 0 {
                            self.stack.jump(offset - 1);
                        }
                        continue 'execute;
                    }
                    0xD3 => {
                        let offset = instr.bx() as i16 as isize;
                        let handler_pc = frame_pc.wrapping_add(offset as usize);
                        unsafe { self.stack.current_mut_unchecked().push_handler(handler_pc as u32); }
                        continue 'execute;
                    }
                    0xD4 => {
                        unsafe { self.stack.current_mut_unchecked().pop_handler(); }
                        continue 'execute;
                    }
                    0xD5 => {
                        let value = unsafe { (*regs_ptr.add(instr.a as usize)).clone() };
                        loop {
                            if let Some(handler_pc) = unsafe {
                                self.stack.current_unchecked().current_handler()
                            } {
                                unsafe {
                                    self.stack.current_mut_unchecked().pc = handler_pc as usize;
                                    let rp = self.stack.current_unchecked().registers.as_ptr() as *mut VmValue;
                                    *rp = value;
                                    self.stack.current_mut_unchecked().pop_handler();
                                }
                                continue 'execute;
                            }
                            self.stack.pop_frame();
                            if self.stack.is_empty() {
                                return Err(VMError::Runtime {
                                    message: "uncaught exception".to_string(),
                                    backtrace: vec![],
                                });
                            }
                        }
                    }
                    0xD6 => {
                        let dest = instr.a as usize;
                        let idx = instr.b as usize;
                        let closure_reg = instr.c as usize;
                        let value = match unsafe { &*regs_ptr.add(closure_reg) } {
                            VmValue::Closure(data) => {
                                let upvalues = unsafe { &*data.upvalues.get() };
                                upvalues.get(idx).cloned()
                            }
                            _ => return Err(VMError::ExpectedFunction(instr.c)),
                        }
                        .ok_or(VMError::InvalidRegister(idx as u8))?;
                        if let VmValue::Scalar(r) = &value {
                            unsafe { *scalar_ptr.add(dest) = r.bits; }
                        }
                        unsafe { *regs_ptr.add(dest) = value; }
                        continue 'execute;
                    }
                    0xD7 => {
                        let upvalue_count = instr.c as usize;
                        let callable_idx = if extended_bits != 0 {
                            (instr.b as u32 | (extended_bits >> 16)) as usize
                        } else {
                            instr.b as usize
                        };
                        let module = self.module.as_ref().unwrap();
                        let closure = match module
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
                                let reg_count = max_regs as usize;
                                let frame = unsafe { self.stack.current_unchecked() };
                                for i in 0..upvalue_count {
                                    let reg_idx = reg_count - upvalue_count + i;
                                    upvalues.push(
                                        unsafe { frame.get_unchecked(reg_idx as u8) }.clone(),
                                    );
                                }
                                VmValue::closure(Rc::new(function.chunk.clone()), upvalues)
                            }
                            Callable::Import(import_idx) => {
                                let import_decl = module
                                    .imports
                                    .get(*import_idx as usize)
                                    .ok_or(VMError::InvalidCallableIndex(callable_idx as u16))?;
                                let resolved = self
                                    .linker
                                    .resolve(import_decl)
                                    .ok_or_else(|| {
                                        VMError::UnresolvedNativeImport(import_decl.to_string())
                                    })?;
                                VmValue::native_import(resolved)
                            }
                        };
                        unsafe { *regs_ptr.add(instr.a as usize) = closure; }
                        continue 'execute;
                    }
                    0xD8 => {
                        self.call(instr.a, instr.b, instr.c)?;
                        continue 'execute;
                    }
                    0xD9 => {
                        self.ret(instr.a, instr.b)?;
                        continue 'execute;
                    }
                    0xDA => {
                        let src = instr.a as usize;
                        let idx = instr.b as usize;
                        let value = unsafe { (*regs_ptr.add(src)).clone() };
                        let slot = unsafe {
                            self.stack.current_mut_unchecked().get_mut_unchecked(instr.c)
                        };
                        match slot {
                            VmValue::Closure(data) => {
                                let upvalues = unsafe { &mut *data.upvalues.get() };
                                if idx >= upvalues.len() {
                                    return Err(VMError::InvalidRegister(idx as u8));
                                }
                                upvalues[idx] = value;
                            }
                            _ => return Err(VMError::ExpectedFunction(instr.c)),
                        }
                        continue 'execute;
                    }
                    0xDB => {
                        let base = instr.a;
                        let arg_count = instr.b;

                        let mut args = Vec::with_capacity(arg_count as usize);
                        for index in 0..arg_count {
                            let src = base
                                .checked_add(1)
                                .and_then(|v| v.checked_add(index))
                                .ok_or(VMError::InvalidRegister(base))?;
                            args.push(unsafe { (*regs_ptr.add(src as usize)).clone() });
                        }

                        let callable = unsafe { (*regs_ptr.add(base as usize)).clone() };

                        if let Some(function) = callable.as_function() {
                            let frame_mut = unsafe { self.stack.current_mut_unchecked() };
                            frame_mut.set_chunk(function);
                            frame_mut.pc = 0;
                            for (i, arg) in args.into_iter().enumerate() {
                                unsafe { frame_mut.set_unchecked(i as u8, arg); }
                            }
                            unsafe { frame_mut.set_unchecked(arg_count, callable); }
                        } else if let Some(idx) = callable.as_native_import() {
                            let returns = self
                                .linker
                                .call(idx, &args, &mut self.memory)
                                .map_err(|e| VMError::NativeError(e.message))?;
                            for (i, val) in returns.into_iter().enumerate() {
                                let tgt = base
                                    .checked_add(i as u8)
                                    .ok_or(VMError::InvalidRegister(base))?;
                                unsafe { *regs_ptr.add(tgt as usize) = val; }
                            }
                            return self.ret(base, 0);
                        } else if let Some(closure) = callable.as_closure() {
                            let frame_mut = unsafe { self.stack.current_mut_unchecked() };
                            frame_mut.set_chunk(closure.chunk.clone());
                            frame_mut.pc = 0;
                            for (i, arg) in args.into_iter().enumerate() {
                                unsafe { frame_mut.set_unchecked(i as u8, arg); }
                            }
                        } else {
                            return Err(VMError::ExpectedFunction(base));
                        }
                        continue 'execute;
                    }
                    0xE0 => {
                        let value = self
                            .globals
                            .get(instr.bx() as usize)
                            .cloned()
                            .unwrap_or_default();
                        unsafe { *regs_ptr.add(instr.a as usize) = value; }
                        continue 'execute;
                    }
                    0xE1 => {
                        const MAX_GLOBALS: usize = 256;
                        let gx = instr.bx() as usize;
                        if gx >= MAX_GLOBALS {
                            return Err(VMError::NativeError("global index out of range".to_string()));
                        }
                        let value = unsafe { (*regs_ptr.add(instr.a as usize)).clone() };
                        if gx >= self.globals.len() {
                            self.globals.resize(gx + 1, VmValue::default());
                        }
                        self.globals[gx] = value;
                        continue 'execute;
                    }
                    0xFE => continue 'execute,
                    0xFF => {
                        self.stack.pop_frame();
                        self.module = None;
                        return Ok(());
                    }
                _ => return Err(VMError::NativeError(format!("unknown opcode {}", instr.opcode_byte))),
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

        let frame_pc = unsafe { self.stack.current_unchecked().pc.wrapping_sub(1) };
        let max_regs = unsafe { self.stack.current_unchecked().chunk.max_registers };
        let regs_ptr = unsafe { self.stack.current_unchecked().registers.as_ptr() as *mut VmValue };
        let effective_bx = instr.bx() as u32 | extended_bits;
        let effective_offset = ((instr.a as u16 as i16) | ((instr.b as i16) << 8)) as i32 | (extended_bits as i32);

        match instr.opcode_byte {
            0x00 => {
                let bx = if extended_bits != 0 { (instr.bx() as u32 | extended_bits) as usize } else { instr.bx() as usize };
                let constant = self.module()?
                    .constants
                    .get(bx)
                    .ok_or(VMError::InvalidConstantIndex(instr.bx()))?
                    .clone();
                unsafe {
                    let frame = self.stack.current_mut_unchecked();
                    match constant {
                        Constant::Bytes(_) => {
                            let offset = self.constant_section.get(&bx)
                                .copied()
                                .ok_or(VMError::InvalidConstantIndex(instr.bx()))?;
                            if !frame.set(instr.a, VmValue::scalar(Register::from_ptr(offset))) {
                                return Err(VMError::InvalidRegister(instr.a));
                            }
                        }
                        constant => {
                            if !frame.set(instr.a, VmValue::scalar(Register { bits: constant.to_bits().expect("non-bytes constants always have scalar bits") })) {
                                return Err(VMError::InvalidRegister(instr.a));
                            }
                        }
                    }
                }
                Ok(())
            }
            0x01 => {
                let src = unsafe { &*regs_ptr.add(instr.b as usize) };
                let val: Register = match src {
                    VmValue::Scalar(r) => *r,
                    _ => {
                        let cloned = src.clone();
                        unsafe {
                            let frame = self.stack.current_mut_unchecked();
                            if !frame.set(instr.a, cloned) {
                                return Err(VMError::InvalidRegister(instr.a));
                            }
                        }
                        return Ok(());
                    }
                };
                unsafe {
                    let frame = self.stack.current_mut_unchecked();
                    if !frame.set(instr.a, VmValue::scalar(val)) {
                        return Err(VMError::InvalidRegister(instr.a));
                    }
                }
                Ok(())
            }
            0x02 => {
                let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<i8>() })?;
                let value = self.memory.read_checked::<i8>(addr).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<i8>() })?;
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i8(value)); }
                Ok(())
            }
            0x03 => {
                let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<i16>() })?;
                let value = self.memory.read_checked::<i16>(addr).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<i16>() })?;
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i16(value)); }
                Ok(())
            }
            0x04 => {
                let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<i32>() })?;
                let value = self.memory.read_checked::<i32>(addr).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<i32>() })?;
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i32(value)); }
                Ok(())
            }
            0x05 => {
                let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<i64>() })?;
                let value = self.memory.read_checked::<i64>(addr).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<i64>() })?;
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(value)); }
                Ok(())
            }
            0x06 => {
                let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<u8>() })?;
                let value = self.memory.read_checked::<u8>(addr).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<u8>() })?;
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_u8(value)); }
                Ok(())
            }
            0x07 => {
                let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<u16>() })?;
                let value = self.memory.read_checked::<u16>(addr).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<u16>() })?;
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_u16(value)); }
                Ok(())
            }
            0x08 => {
                let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<u32>() })?;
                let value = self.memory.read_checked::<u32>(addr).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<u32>() })?;
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_u32(value)); }
                Ok(())
            }
            0x09 => {
                let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<u64>() })?;
                let value = self.memory.read_checked::<u64>(addr).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<u64>() })?;
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_u64(value)); }
                Ok(())
            }
            0x0A => {
                let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<f32>() })?;
                let value = self.memory.read_checked::<f32>(addr).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<f32>() })?;
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_f32(value)); }
                Ok(())
            }
            0x0B => {
                let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(b)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(c).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<f64>() })?;
                let value = self.memory.read_checked::<f64>(addr).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<f64>() })?;
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_f64(value)); }
                Ok(())
            }
            0x0C => {
                let a = instr.a as usize; let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<i8>() })?;
                let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().i8 };
                if !self.memory.write_checked::<i8>(addr, value) {
                    return Err(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<i8>() });
                }
                Ok(())
            }
            0x0D => {
                let a = instr.a as usize; let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<i16>() })?;
                let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().i16 };
                if !self.memory.write_checked::<i16>(addr, value) {
                    return Err(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<i16>() });
                }
                Ok(())
            }
            0x0E => {
                let a = instr.a as usize; let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<i32>() })?;
                let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().i32 };
                if !self.memory.write_checked::<i32>(addr, value) {
                    return Err(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<i32>() });
                }
                Ok(())
            }
            0x0F => {
                let a = instr.a as usize; let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<i64>() })?;
                let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().i64 };
                if !self.memory.write_checked::<i64>(addr, value) {
                    return Err(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<i64>() });
                }
                Ok(())
            }
            0x10 => {
                let a = instr.a as usize; let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<u8>() })?;
                let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().u8 };
                if !self.memory.write_checked::<u8>(addr, value) {
                    return Err(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<u8>() });
                }
                Ok(())
            }
            0x11 => {
                let a = instr.a as usize; let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<u16>() })?;
                let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().u16 };
                if !self.memory.write_checked::<u16>(addr, value) {
                    return Err(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<u16>() });
                }
                Ok(())
            }
            0x12 => {
                let a = instr.a as usize; let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<u32>() })?;
                let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().u32 };
                if !self.memory.write_checked::<u32>(addr, value) {
                    return Err(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<u32>() });
                }
                Ok(())
            }
            0x13 => {
                let a = instr.a as usize; let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<u64>() })?;
                let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().u64 };
                if !self.memory.write_checked::<u64>(addr, value) {
                    return Err(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<u64>() });
                }
                Ok(())
            }
            0x14 => {
                let a = instr.a as usize; let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<f32>() })?;
                let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().f32 };
                if !self.memory.write_checked::<f32>(addr, value) {
                    return Err(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<f32>() });
                }
                Ok(())
            }
            0x15 => {
                let a = instr.a as usize; let b = instr.b as usize; let c = instr.c as usize;
                let base = unsafe { (*regs_ptr.add(a)).as_scalar().unwrap_unchecked().ptr };
                let addr = base.checked_add(b).ok_or(VMError::MemoryOutOfBounds { pc: frame_pc, addr: base, size: size_of::<f64>() })?;
                let value = unsafe { (*regs_ptr.add(c)).as_scalar().unwrap_unchecked().f64 };
                if !self.memory.write_checked::<f64>(addr, value) {
                    return Err(VMError::MemoryOutOfBounds { pc: frame_pc, addr, size: size_of::<f64>() });
                }
                Ok(())
            }
            0x23 => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb.wrapping_add(vc))); }
                Ok(())
            }
            0x27 => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb.wrapping_sub(vc))); }
                Ok(())
            }
            0x2B => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb.wrapping_mul(vc))); }
                Ok(())
            }
            0x2F => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                if vc == 0 { return Err(VMError::DivisionByZero); }
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb.wrapping_div(vc))); }
                Ok(())
            }
            0x33 => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                if vc == 0 { return Err(VMError::DivisionByZero); }
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb.wrapping_rem(vc))); }
                Ok(())
            }
            0x37 => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb.wrapping_neg())); }
                Ok(())
            }
            0x4F => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().u64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().u64 };
                if vc == 0 { return Err(VMError::DivisionByZero); }
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_u64(vb.wrapping_div(vc))); }
                Ok(())
            }
            0x53 => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().u64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().u64 };
                if vc == 0 { return Err(VMError::DivisionByZero); }
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_u64(vb.wrapping_rem(vc))); }
                Ok(())
            }
            0x59 => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_f64(vb + vc)); }
                Ok(())
            }
            0x5B => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_f64(vb - vc)); }
                Ok(())
            }
            0x5D => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_f64(vb * vc)); }
                Ok(())
            }
            0x5F => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_f64(vb / vc)); }
                Ok(())
            }
            0x61 => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_f64(-vb)); }
                Ok(())
            }
            0x6B => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb & vc)); }
                Ok(())
            }
            0x6F => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb | vc)); }
                Ok(())
            }
            0x73 => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb ^ vc)); }
                Ok(())
            }
            0x77 => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(!vb)); }
                Ok(())
            }
            0x7B => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb.wrapping_shl(vc as u32))); }
                Ok(())
            }
            0x7F => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64(vb.wrapping_shr(vc as u32))); }
                Ok(())
            }
            0x83 => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_i64((vb as u64).wrapping_shr(vc as u32) as i64)); }
                Ok(())
            }
            0x93 => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb == vc)); }
                Ok(())
            }
            0x97 => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb != vc)); }
                Ok(())
            }
            0x9B => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb < vc)); }
                Ok(())
            }
            0x9F => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb <= vc)); }
                Ok(())
            }
            0xA3 => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb > vc)); }
                Ok(())
            }
            0xA7 => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().i64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().i64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb >= vc)); }
                Ok(())
            }
            0xAB => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().u64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().u64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb < vc)); }
                Ok(())
            }
            0xAF => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().u64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().u64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb <= vc)); }
                Ok(())
            }
            0xB3 => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().u64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().u64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb > vc)); }
                Ok(())
            }
            0xB7 => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().u64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().u64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb >= vc)); }
                Ok(())
            }
            0xB9 => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb == vc)); }
                Ok(())
            }
            0xBB => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb != vc)); }
                Ok(())
            }
            0xBD => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb < vc)); }
                Ok(())
            }
            0xBF => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb <= vc)); }
                Ok(())
            }
            0xC1 => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb > vc)); }
                Ok(())
            }
            0xC3 => {
                let vb = unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked().f64 };
                let vc = unsafe { (*regs_ptr.add(instr.c as usize)).as_scalar().unwrap_unchecked().f64 };
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(Register::from_bool(vb >= vc)); }
                Ok(())
            }
            0xC8 => {
                let from_type = instr.c >> 4;
                let to_type = instr.c & 0x0F;
                let from = ValueType::from_byte(from_type)
                    .ok_or(VMError::InvalidConversionType(from_type))?;
                let to = ValueType::from_byte(to_type)
                    .ok_or(VMError::InvalidConversionType(to_type))?;
                let result = convert_register(
                    unsafe { (*regs_ptr.add(instr.b as usize)).as_scalar().unwrap_unchecked() },
                    from,
                    to,
                );
                unsafe { *regs_ptr.add(instr.a as usize) = VmValue::scalar(result); }
                Ok(())
            }
            0xD0 => {
                let offset = if extended_bits != 0 { (((instr.a as u16 as i16) | ((instr.b as i16) << 8)) as i32 | (extended_bits as i32)) as i16 } else { instr.sbx_ab() };
                self.stack.jump(offset - 1);
                Ok(())
            }
            0xD1 => {
                let offset = if extended_bits != 0 { (((instr.a as u16 as i16) | ((instr.b as i16) << 8)) as i32 | (extended_bits as i32)) as i16 } else { instr.sbx() };
                if unsafe { (*regs_ptr.add(instr.a as usize)).as_scalar().unwrap_unchecked().u64 } != 0 {
                    self.stack.jump(offset - 1);
                }
                Ok(())
            }
            0xD2 => {
                let offset = if extended_bits != 0 { (((instr.a as u16 as i16) | ((instr.b as i16) << 8)) as i32 | (extended_bits as i32)) as i16 } else { instr.sbx() };
                if unsafe { (*regs_ptr.add(instr.a as usize)).as_scalar().unwrap_unchecked().u64 } == 0 {
                    self.stack.jump(offset - 1);
                }
                Ok(())
            }
            0xD3 => {
                let offset = instr.bx() as i16 as isize;
                let handler_pc = frame_pc.wrapping_add(offset as usize);
                unsafe { self.stack.current_mut_unchecked().push_handler(handler_pc as u32); }
                Ok(())
            }
            0xD4 => {
                unsafe { self.stack.current_mut_unchecked().pop_handler(); }
                Ok(())
            }
            0xD5 => {
                let value = unsafe { (*regs_ptr.add(instr.a as usize)).clone() };
                loop {
                    if let Some(handler_pc) = unsafe {
                        self.stack.current_unchecked().current_handler()
                    } {
                        unsafe {
                            self.stack.current_mut_unchecked().pc = handler_pc as usize;
                            let rp = self.stack.current_unchecked().registers.as_ptr() as *mut VmValue;
                            *rp = value;
                            self.stack.current_mut_unchecked().pop_handler();
                        }
                        return Ok(());
                    }
                    self.stack.pop_frame();
                    if self.stack.is_empty() {
                        return Err(VMError::Runtime {
                            message: "uncaught exception".to_string(),
                            backtrace: vec![],
                        });
                    }
                }
            }
            0xD6 => {
                let dest = instr.a as usize;
                let idx = instr.b as usize;
                let closure_reg = instr.c as usize;
                let value = match unsafe { &*regs_ptr.add(closure_reg) } {
                    VmValue::Closure(data) => {
                        let upvalues = unsafe { &*data.upvalues.get() };
                        upvalues.get(idx).cloned()
                    }
                    _ => return Err(VMError::ExpectedFunction(instr.c)),
                }
                .ok_or(VMError::InvalidRegister(idx as u8))?;
                unsafe { *regs_ptr.add(dest) = value; }
                Ok(())
            }
            0xD7 => {
                let upvalue_count = instr.c as usize;
                let callable_idx = if extended_bits != 0 {
                    (instr.b as u32 | (extended_bits >> 16)) as usize
                } else {
                    instr.b as usize
                };
                let module = self.module()?;
                let closure = match module
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
                        let reg_count = max_regs as usize;
                        let frame = unsafe { self.stack.current_unchecked() };
                        for i in 0..upvalue_count {
                            let reg_idx = reg_count - upvalue_count + i;
                            upvalues.push(
                                unsafe { frame.get_unchecked(reg_idx as u8) }.clone(),
                            );
                        }
                        VmValue::closure(Rc::new(function.chunk.clone()), upvalues)
                    }
                    Callable::Import(import_idx) => {
                        let import_decl = module
                            .imports
                            .get(*import_idx as usize)
                            .ok_or(VMError::InvalidCallableIndex(callable_idx as u16))?;
                        let resolved = self
                            .linker
                            .resolve(import_decl)
                            .ok_or_else(|| {
                                VMError::UnresolvedNativeImport(import_decl.to_string())
                            })?;
                        VmValue::native_import(resolved)
                    }
                };
                unsafe { *regs_ptr.add(instr.a as usize) = closure; }
                Ok(())
            }
            0xD8 => { self.call(instr.a, instr.b, instr.c)?; Ok(()) }
            0xD9 => { self.ret(instr.a, instr.b)?; Ok(()) }
            0xDA => {
                let src = instr.a as usize;
                let idx = instr.b as usize;
                let value = unsafe { (*regs_ptr.add(src)).clone() };
                let slot = unsafe { self.stack.current_mut_unchecked().get_mut_unchecked(instr.c) };
                match slot {
                    VmValue::Closure(data) => {
                        let upvalues = unsafe { &mut *data.upvalues.get() };
                        if idx >= upvalues.len() { return Err(VMError::InvalidRegister(idx as u8)); }
                        upvalues[idx] = value;
                    }
                    _ => return Err(VMError::ExpectedFunction(instr.c)),
                }
                Ok(())
            }
            0xDB => {
                let base = instr.a;
                let arg_count = instr.b;

                let mut args = Vec::with_capacity(arg_count as usize);
                for index in 0..arg_count {
                    let src = base.checked_add(1).and_then(|v| v.checked_add(index)).ok_or(VMError::InvalidRegister(base))?;
                    args.push(unsafe { (*regs_ptr.add(src as usize)).clone() });
                }

                let callable = unsafe { (*regs_ptr.add(base as usize)).clone() };

                if let Some(function) = callable.as_function() {
                    let frame_mut = unsafe { self.stack.current_mut_unchecked() };
                    frame_mut.set_chunk(function);
                    frame_mut.pc = 0;
                    for (i, arg) in args.into_iter().enumerate() {
                        unsafe { frame_mut.set_unchecked(i as u8, arg); }
                    }
                    unsafe { frame_mut.set_unchecked(arg_count, callable); }
                } else if let Some(idx) = callable.as_native_import() {
                    let returns = self.linker.call(idx, &args, &mut self.memory).map_err(|e| VMError::NativeError(e.message))?;
                    for (i, val) in returns.into_iter().enumerate() {
                        let tgt = base.checked_add(i as u8).ok_or(VMError::InvalidRegister(base))?;
                        unsafe { *regs_ptr.add(tgt as usize) = val; }
                    }
                    return self.ret(base, 0);
                } else if let Some(closure) = callable.as_closure() {
                    let frame_mut = unsafe { self.stack.current_mut_unchecked() };
                    frame_mut.set_chunk(closure.chunk.clone());
                    frame_mut.pc = 0;
                    for (i, arg) in args.into_iter().enumerate() {
                        unsafe { frame_mut.set_unchecked(i as u8, arg); }
                    }
                } else {
                    return Err(VMError::ExpectedFunction(base));
                }
                Ok(())
            }
            0xE0 => {
                let value = self.globals.get(instr.bx() as usize).cloned().unwrap_or_default();
                unsafe { *regs_ptr.add(instr.a as usize) = value; }
                Ok(())
            }
            0xE1 => {
                const MAX_GLOBALS: usize = 256;
                let gx = instr.bx() as usize;
                if gx >= MAX_GLOBALS {
                    return Err(VMError::NativeError("global index out of range".to_string()));
                }
                let value = unsafe { (*regs_ptr.add(instr.a as usize)).clone() };
                if gx >= self.globals.len() {
                    self.globals.resize(gx + 1, VmValue::default());
                }
                self.globals[gx] = value;
                Ok(())
            }
            0xFE => Ok(()),
            0xFF => Err(VMError::Halted),
            _ => Err(VMError::NativeError(format!("unknown opcode {}", instr.opcode_byte))),
        }
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
        match val {
            VmValue::Scalar(r) => *r,
            _ => unsafe { std::hint::unreachable_unchecked() },
        }
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
        let frame = unsafe { self.stack.current_mut_unchecked() };
        if let VmValue::Scalar(r) = &value {
            unsafe { *frame.scalar_regs.get_unchecked_mut(register as usize) = r.bits; }
        }
        unsafe { frame.set_unchecked(register, value); }
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
            // Sync scalar_regs for the new frame
            {
                let frame = unsafe { self.stack.current_unchecked() };
                let sp = frame.scalar_regs.as_ptr() as *mut u64;
                for i in 0..=arg_count as usize {
                    if let VmValue::Scalar(r) = unsafe { frame.get_unchecked(i as u8) } {
                        unsafe { *sp.add(i) = r.bits; }
                    }
                }
            }
            return Ok(());
        }

        if let Some(closure_data) = callable.as_closure() {
            if arg_count as usize > closure_data.chunk.max_registers as usize {
                return Err(VMError::InvalidRegister(arg_count));
            }

            let chunk_clone = closure_data.chunk.clone();
            self.stack.push_call(chunk_clone, base, expected_returns, "anon");
            unsafe { self.set_value_unchecked(arg_count, callable); }
            for (index, value) in args.into_iter().enumerate() {
                unsafe { self.set_value_unchecked(index as u8, value); }
            }
            // Sync scalar_regs
            {
                let frame = unsafe { self.stack.current_unchecked() };
                let sp = frame.scalar_regs.as_ptr() as *mut u64;
                for i in 0..=arg_count as usize {
                    if let VmValue::Scalar(r) = unsafe { frame.get_unchecked(i as u8) } {
                        unsafe { *sp.add(i) = r.bits; }
                    }
                }
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
        chunk.emit(Instruction::abx(Opcode::TRY, 0, 4));
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

        vm.step().unwrap();
        vm.step().unwrap();
        vm.step().unwrap();
        unsafe {
            assert_eq!(vm.scalar(0).unwrap().i64, 42);
        }
        vm.step().unwrap();
        assert_eq!(vm.step(), Err(VMError::Halted));
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
        vm.step().unwrap();
        vm.step().unwrap();
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
