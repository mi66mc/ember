use crate::bytecode::import::{ImportDecl, ImportKind};
use crate::vm::memory::Memory;
use crate::vm::register::{Register, VmValue};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeError {
    pub message: String,
}

impl NativeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

pub type NativeResult = Result<Vec<VmValue>, NativeError>;

pub trait NativeModule: Send + Sync {
    fn name(&self) -> &str;
    fn exports(&self) -> u16;
    fn call(&self, index: u16, args: &[VmValue], memory: &mut Memory) -> NativeResult;
    fn function_index(&self, name: &str) -> Option<u16>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportIndex {
    pub module: u16,
    pub function: u16,
}

#[derive(Default)]
pub struct NativeLinker {
    modules: Vec<Box<dyn NativeModule>>,
}

impl NativeLinker {
    pub fn mount(&mut self, module: impl NativeModule + 'static) -> u16 {
        let idx = self.modules.len() as u16;
        self.modules.push(Box::new(module));
        idx
    }

    pub fn resolve(&self, import: &ImportDecl) -> Option<ImportIndex> {
        let ImportKind::Native { module, function } = &import.kind else {
            return None;
        };
        for (module_idx, m) in self.modules.iter().enumerate() {
            if m.name() != *module {
                continue;
            }
            if let Some(func_idx) = m.function_index(function) {
                return Some(ImportIndex {
                    module: module_idx as u16,
                    function: func_idx,
                });
            }
        }
        None
    }

    pub fn get(&self, module: u16) -> Option<&dyn NativeModule> {
        self.modules.get(module as usize).map(|m| m.as_ref())
    }

    pub fn call(
        &self,
        index: ImportIndex,
        args: &[VmValue],
        memory: &mut Memory,
    ) -> Result<Vec<VmValue>, NativeError> {
        let module = self
            .modules
            .get(index.module as usize)
            .ok_or_else(|| NativeError::new("native module not found"))?;
        module.call(index.function, args, memory)
    }

    pub fn contains_native(&self, import: &ImportDecl) -> bool {
        let ImportKind::Native { module, .. } = &import.kind else {
            return false;
        };
        self.modules.iter().any(|m| m.name() == *module)
    }
}

fn scalar_arg(args: &[VmValue], name: &str) -> Result<Register, NativeError> {
    if args.is_empty() {
        return Err(NativeError::new(format!(
            "{name} expects at least 1 argument, got 0"
        )));
    }
    args[0]
        .as_scalar()
        .ok_or_else(|| NativeError::new(format!("{name} expects a scalar argument")))
}

fn print_i64(args: &[VmValue]) -> NativeResult {
    let value = unsafe { scalar_arg(args, "io.print_i64")?.i64 };
    println!("{value}");
    Ok(Vec::new())
}

fn print_u64(args: &[VmValue]) -> NativeResult {
    let value = unsafe { scalar_arg(args, "io.print_u64")?.u64 };
    println!("{value}");
    Ok(Vec::new())
}

fn print_f64(args: &[VmValue]) -> NativeResult {
    let value = unsafe { scalar_arg(args, "io.print_f64")?.f64 };
    println!("{value}");
    Ok(Vec::new())
}

fn print_bool(args: &[VmValue]) -> NativeResult {
    let value = unsafe { scalar_arg(args, "io.print_bool")?.u64 != 0 };
    println!("{value}");
    Ok(Vec::new())
}

fn print_mem(args: &[VmValue], memory: &Memory) -> NativeResult {
    if args.len() < 2 {
        return Err(NativeError::new("io.print_mem expects 2 arguments (ptr, len)"));
    }
    let ptr = unsafe {
        args[0]
            .as_scalar()
            .ok_or_else(|| NativeError::new("io.print_mem: ptr must be scalar"))?
            .ptr
    };
    let len = unsafe {
        args[1]
            .as_scalar()
            .ok_or_else(|| NativeError::new("io.print_mem: len must be scalar"))?
            .u64 as usize
    };
    if ptr + len > memory.size() {
        return Err(NativeError::new("io.print_mem: out of bounds"));
    }
    let bytes = unsafe { std::slice::from_raw_parts(memory.as_ptr().add(ptr), len) };
    match std::str::from_utf8(bytes) {
        Ok(s) => println!("{s}"),
        Err(_) => {
            let s = String::from_utf8_lossy(bytes);
            println!("{s}");
        }
    }
    Ok(Vec::new())
}

pub struct Io;

impl NativeModule for Io {
    fn name(&self) -> &str {
        "io"
    }

    fn exports(&self) -> u16 {
        5
    }

    fn call(&self, index: u16, args: &[VmValue], memory: &mut Memory) -> NativeResult {
        match index {
            0 => print_i64(args),
            1 => print_u64(args),
            2 => print_f64(args),
            3 => print_bool(args),
            4 => print_mem(args, memory),
            _ => Err(NativeError::new(format!(
                "io: unknown function {index}"
            ))),
        }
    }

    fn function_index(&self, name: &str) -> Option<u16> {
        match name {
            "print_i64" => Some(0),
            "print_u64" => Some(1),
            "print_f64" => Some(2),
            "print_bool" => Some(3),
            "print_mem" => Some(4),
            _ => None,
        }
    }
}

pub fn std_linker() -> NativeLinker {
    let mut linker = NativeLinker::default();
    linker.mount(Io);
    linker.mount(Core);
    linker.mount(Math);
    linker
}

fn alloc(args: &[VmValue], memory: &mut Memory) -> NativeResult {
    let size = unsafe {
        if args.is_empty() {
            return Err(NativeError::new("core.alloc expects 1 argument (size)"));
        }
        args[0]
            .as_scalar()
            .ok_or_else(|| NativeError::new("core.alloc expects a scalar argument"))?
            .u64 as usize
    };
    let ptr = memory.alloc(size);
    Ok(vec![VmValue::scalar(Register::from_ptr(ptr))])
}

fn memcpy(args: &[VmValue], memory: &mut Memory) -> NativeResult {
    if args.len() < 3 {
        return Err(NativeError::new("core.memcpy expects 3 arguments"));
    }
    let dst = unsafe {
        args[0]
            .as_scalar()
            .ok_or_else(|| NativeError::new("core.memcpy: dst must be scalar"))?
            .ptr
    };
    let src = unsafe {
        args[1]
            .as_scalar()
            .ok_or_else(|| NativeError::new("core.memcpy: src must be scalar"))?
            .ptr
    };
    let len = unsafe {
        args[2]
            .as_scalar()
            .ok_or_else(|| NativeError::new("core.memcpy: len must be scalar"))?
            .ptr
    };
    if src + len > memory.size() || dst + len > memory.size() {
        return Err(NativeError::new("core.memcpy: out of bounds"));
    }
    unsafe {
        let src_ptr = memory.as_ptr().add(src);
        let dst_ptr = memory.as_mut_ptr().add(dst);
        std::ptr::copy(src_ptr, dst_ptr, len);
    }
    Ok(Vec::new())
}

fn memset(args: &[VmValue], memory: &mut Memory) -> NativeResult {
    if args.len() < 3 {
        return Err(NativeError::new("core.memset expects 3 arguments"));
    }
    let dst = unsafe {
        args[0]
            .as_scalar()
            .ok_or_else(|| NativeError::new("core.memset: dst must be scalar"))?
            .ptr
    };
    let byte = unsafe {
        args[1]
            .as_scalar()
            .ok_or_else(|| NativeError::new("core.memset: byte must be scalar"))?
            .u8
    };
    let len = unsafe {
        args[2]
            .as_scalar()
            .ok_or_else(|| NativeError::new("core.memset: len must be scalar"))?
            .ptr
    };
    if dst + len > memory.size() {
        return Err(NativeError::new("core.memset: out of bounds"));
    }
    unsafe {
        let dst_ptr = memory.as_mut_ptr().add(dst);
        std::ptr::write_bytes(dst_ptr, byte, len);
    }
    Ok(Vec::new())
}

pub struct Core;

impl NativeModule for Core {
    fn name(&self) -> &str {
        "core"
    }

    fn exports(&self) -> u16 {
        3
    }

    fn call(&self, index: u16, args: &[VmValue], memory: &mut Memory) -> NativeResult {
        match index {
            0 => alloc(args, memory),
            1 => memcpy(args, memory),
            2 => memset(args, memory),
            _ => Err(NativeError::new(format!(
                "core: unknown function {index}"
            ))),
        }
    }

    fn function_index(&self, name: &str) -> Option<u16> {
        match name {
            "alloc" => Some(0),
            "memcpy" => Some(1),
            "memset" => Some(2),
            _ => None,
        }
    }
}

fn sqrt_f64(args: &[VmValue]) -> NativeResult {
    let value = unsafe { scalar_arg(args, "math.sqrt")?.f64 };
    Ok(vec![VmValue::scalar(Register::from_f64(value.sqrt()))])
}

fn abs_i64(args: &[VmValue]) -> NativeResult {
    let value = unsafe { scalar_arg(args, "math.abs_i64")?.i64 };
    Ok(vec![VmValue::scalar(Register::from_i64(value.abs()))])
}

pub struct Math;

impl NativeModule for Math {
    fn name(&self) -> &str {
        "math"
    }

    fn exports(&self) -> u16 {
        2
    }

    fn call(&self, index: u16, args: &[VmValue], _memory: &mut Memory) -> NativeResult {
        match index {
            0 => sqrt_f64(args),
            1 => abs_i64(args),
            _ => Err(NativeError::new(format!(
                "math: unknown function {index}"
            ))),
        }
    }

    fn function_index(&self, name: &str) -> Option<u16> {
        match name {
            "sqrt" => Some(0),
            "abs_i64" => Some(1),
            _ => None,
        }
    }
}
