use crate::bytecode::import::{ImportDecl, ImportKind};
// SAFETY CONTRACT: All native functions use unsafe union field access.
// Each function reads a specific named field (i64, u64, f64, ptr, etc.)
// from the Register returned by scalar_arg(). The safety invariant is:
// the caller (CALL instruction) passes arguments that were written via
// Register::from_* constructors matching the expected type. This is
// guaranteed by the bytecode compiler which emits typed opcodes (LOAD_I64,
// ADD_I64, etc.).
//
// The print_mem, alloc, memcpy, and memset functions additionally use
// unsafe pointer operations on Memory. These are sound because:
//  * Memory::as_ptr() and as_mut_ptr() return pointers to a Vec<u8>
//    allocation that is never freed while the VM is running.
//  * Bounds checks are performed before raw pointer arithmetic.
//  * std::ptr::copy / write_bytes require non-overlapping, aligned regions;
//    Memory's bump allocator ensures fresh allocations don't alias.

use crate::vm::memory::Memory;
use crate::vm::register::{Register, VmValue};

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::sync::Mutex;

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
    // SAFETY: per file-level contract, CALL passes arguments matching the
    // expected type; reading i64 from the Register is sound
    let value = unsafe { scalar_arg(args, "io.print_i64")?.i64 };
    println!("{value}");
    Ok(Vec::new())
}

fn print_u64(args: &[VmValue]) -> NativeResult {
    // SAFETY: see file-level safety contract
    let value = unsafe { scalar_arg(args, "io.print_u64")?.u64 };
    println!("{value}");
    Ok(Vec::new())
}

fn print_f64(args: &[VmValue]) -> NativeResult {
    // SAFETY: see file-level safety contract
    let value = unsafe { scalar_arg(args, "io.print_f64")?.f64 };
    println!("{value}");
    Ok(Vec::new())
}

fn print_bool(args: &[VmValue]) -> NativeResult {
    // SAFETY: see file-level safety contract; reading u64 is always sound
    let value = unsafe { scalar_arg(args, "io.print_bool")?.u64 != 0 };
    println!("{value}");
    Ok(Vec::new())
}

fn print_mem(args: &[VmValue], memory: &Memory) -> NativeResult {
    if args.len() < 2 {
        return Err(NativeError::new("io.print_mem expects 2 arguments (ptr, len)"));
    }
    // SAFETY: see file-level safety contract; pointer arguments are written
    // via from_ptr / from_u64 by the compiler
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
    // SAFETY: bounds check above ensures ptr..ptr+len is within the Vec<u8>
    // allocation; the memory is never freed while the VM runs
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
    linker.mount(Fs::new());
    linker.mount(Time);
    linker.mount(Rng::new());
    linker
}

fn malloc_native(args: &[VmValue], memory: &mut Memory) -> NativeResult {
    // SAFETY: see file-level safety contract; the size argument was written
    // via from_u64 by the compiler
    let size = unsafe {
        if args.is_empty() {
            return Err(NativeError::new("core.malloc expects 1 argument (size)"));
        }
        args[0]
            .as_scalar()
            .ok_or_else(|| NativeError::new("core.malloc expects a scalar argument"))?
            .u64 as usize
    };
    let ptr = memory.malloc(size);
    Ok(vec![VmValue::scalar(Register::from_ptr(ptr))])
}

fn free_native(args: &[VmValue], memory: &mut Memory) -> NativeResult {
    let ptr = unsafe {
        if args.is_empty() {
            return Err(NativeError::new("core.free expects 1 argument (ptr)"));
        }
        args[0]
            .as_scalar()
            .ok_or_else(|| NativeError::new("core.free expects a scalar argument"))?
            .ptr
    };
    memory.free_malloc(ptr);
    Ok(Vec::new())
}

fn memcpy(args: &[VmValue], memory: &mut Memory) -> NativeResult {
    if args.len() < 3 {
        return Err(NativeError::new("core.memcpy expects 3 arguments"));
    }
    // SAFETY: see file-level safety contract; pointer arguments are written
    // via from_ptr by the compiler
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
    // SAFETY: bounds check above ensures both src..src+len and dst..dst+len
    // are within the Vec<u8> allocation; allocator guarantees regions don't
    // alias in ways that would violate ptr::copy requirements
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
    // SAFETY: see file-level safety contract; pointer arguments are written
    // via from_ptr / from_u8 by the compiler
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
    // SAFETY: bounds check above ensures dst..dst+len is within the Vec<u8>
    // allocation; write_bytes requires that dst is valid for len bytes of
    // write, which is satisfied by the Vec<u8> backing store
    unsafe {
        let dst_ptr = memory.as_mut_ptr().add(dst);
        std::ptr::write_bytes(dst_ptr, byte, len);
    }
    Ok(Vec::new())
}

fn alloc_gc(args: &[VmValue], memory: &mut Memory) -> NativeResult {
    if args.len() < 2 {
        return Err(NativeError::new(
            "core.alloc_gc expects 2 arguments (type_tag, size)",
        ));
    }
    let type_tag = unsafe {
        args[0]
            .as_scalar()
            .ok_or_else(|| NativeError::new("core.alloc_gc: type_tag must be scalar"))?
            .u8
    };
    let size = unsafe {
        args[1]
            .as_scalar()
            .ok_or_else(|| NativeError::new("core.alloc_gc: size must be scalar"))?
            .u64 as usize
    };
    let ptr = memory.alloc_managed(type_tag, size, &[]);
    Ok(vec![VmValue::scalar(Register::from_ptr(ptr))])
}

fn gc_collect(args: &[VmValue], memory: &mut Memory) -> NativeResult {
    let roots: Vec<usize> = args
        .iter()
        .filter_map(|v| v.as_scalar().map(|r| unsafe { r.ptr }))
        .filter(|&p| p != 0)
        .collect();
    memory.collect_gc(&roots);
    Ok(Vec::new())
}

pub struct Core;

impl NativeModule for Core {
    fn name(&self) -> &str {
        "core"
    }

    fn exports(&self) -> u16 {
        6
    }

    fn call(&self, index: u16, args: &[VmValue], memory: &mut Memory) -> NativeResult {
        match index {
            0 => malloc_native(args, memory),
            1 => free_native(args, memory),
            2 => memcpy(args, memory),
            3 => memset(args, memory),
            4 => alloc_gc(args, memory),
            5 => gc_collect(args, memory),
            _ => Err(NativeError::new(format!(
                "core: unknown function {index}"
            ))),
        }
    }

    fn function_index(&self, name: &str) -> Option<u16> {
        match name {
            "malloc" => Some(0),
            "free" => Some(1),
            "memcpy" => Some(2),
            "memset" => Some(3),
            "alloc_gc" => Some(4),
            "gc_collect" => Some(5),
            _ => None,
        }
    }
}

fn sqrt_f64(args: &[VmValue]) -> NativeResult {
    // SAFETY: see file-level safety contract; CALL passes f64 argument
    let value = unsafe { scalar_arg(args, "math.sqrt")?.f64 };
    Ok(vec![VmValue::scalar(Register::from_f64(value.sqrt()))])
}

fn abs_i64(args: &[VmValue]) -> NativeResult {
    // SAFETY: see file-level safety contract; CALL passes i64 argument
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

// ── fs module (file I/O) ──────────────────────────────────────────

pub struct Fs {
    files: Mutex<HashMap<i64, File>>,
    next_fd: Mutex<i64>,
}

impl Fs {
    pub fn new() -> Self {
        Fs {
            files: Mutex::new(HashMap::new()),
            next_fd: Mutex::new(0),
        }
    }
}

fn fs_open(args: &[VmValue], memory: &Memory, fs: &Fs) -> NativeResult {
    if args.len() < 3 {
        return Err(NativeError::new(
            "fs.open expects 3 arguments (path_ptr, path_len, mode)",
        ));
    }
    // SAFETY: see file-level safety contract
    let path_ptr = unsafe {
        args[0]
            .as_scalar()
            .ok_or_else(|| NativeError::new("fs.open: path_ptr must be scalar"))?
            .ptr
    };
    let path_len = unsafe {
        args[1]
            .as_scalar()
            .ok_or_else(|| NativeError::new("fs.open: path_len must be scalar"))?
            .u64 as usize
    };
    let mode = unsafe {
        args[2]
            .as_scalar()
            .ok_or_else(|| NativeError::new("fs.open: mode must be scalar"))?
            .i64
    };

    if path_ptr + path_len > memory.size() {
        return Err(NativeError::new("fs.open: path out of bounds"));
    }

    // SAFETY: bounds checked above
    let path_bytes = unsafe { std::slice::from_raw_parts(memory.as_ptr().add(path_ptr), path_len) };
    let path = String::from_utf8_lossy(path_bytes);

    let file_result = match mode {
        0 => File::open(path.as_ref()),
        1 => File::create(path.as_ref()),
        2 => std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path.as_ref()),
        _ => return Err(NativeError::new(format!("fs.open: invalid mode {mode}"))),
    };

    match file_result {
        Ok(file) => {
            let fd = {
                let mut next = fs.next_fd.lock().unwrap();
                let fd = *next;
                *next += 1;
                fd
            };
            fs.files.lock().unwrap().insert(fd, file);
            Ok(vec![VmValue::scalar(Register::from_i64(fd))])
        }
        Err(_) => Ok(vec![VmValue::scalar(Register::from_i64(-1))]),
    }
}

fn fs_read(args: &[VmValue], memory: &mut Memory, fs: &Fs) -> NativeResult {
    let fd = unsafe { scalar_arg(args, "fs.read")?.i64 };
    if args.len() < 3 {
        return Err(NativeError::new(
            "fs.read expects 3 arguments (fd, buf_ptr, len)",
        ));
    }
    let buf_ptr = unsafe {
        args[1]
            .as_scalar()
            .ok_or_else(|| NativeError::new("fs.read: buf_ptr must be scalar"))?
            .ptr
    };
    let len = unsafe {
        args[2]
            .as_scalar()
            .ok_or_else(|| NativeError::new("fs.read: len must be scalar"))?
            .u64 as usize
    };

    if buf_ptr + len > memory.size() {
        return Err(NativeError::new("fs.read: buffer out of bounds"));
    }

    let mut files = fs.files.lock().unwrap();
    if let Some(file) = files.get_mut(&fd) {
        // SAFETY: bounds checked above; buf_ptr..buf_ptr+len is within memory allocation
        let buf =
            unsafe { std::slice::from_raw_parts_mut(memory.as_mut_ptr().add(buf_ptr), len) };
        match file.read(buf) {
            Ok(n) => Ok(vec![VmValue::scalar(Register::from_i64(n as i64))]),
            Err(_) => Ok(vec![VmValue::scalar(Register::from_i64(-1))]),
        }
    } else {
        Ok(vec![VmValue::scalar(Register::from_i64(-1))])
    }
}

fn fs_write(args: &[VmValue], memory: &Memory, fs: &Fs) -> NativeResult {
    let fd = unsafe { scalar_arg(args, "fs.write")?.i64 };
    if args.len() < 3 {
        return Err(NativeError::new(
            "fs.write expects 3 arguments (fd, buf_ptr, len)",
        ));
    }
    let buf_ptr = unsafe {
        args[1]
            .as_scalar()
            .ok_or_else(|| NativeError::new("fs.write: buf_ptr must be scalar"))?
            .ptr
    };
    let len = unsafe {
        args[2]
            .as_scalar()
            .ok_or_else(|| NativeError::new("fs.write: len must be scalar"))?
            .u64 as usize
    };

    if buf_ptr + len > memory.size() {
        return Err(NativeError::new("fs.write: buffer out of bounds"));
    }

    let mut files = fs.files.lock().unwrap();
    if let Some(file) = files.get_mut(&fd) {
        // SAFETY: bounds checked above; buf_ptr..buf_ptr+len is within memory allocation
        let buf = unsafe { std::slice::from_raw_parts(memory.as_ptr().add(buf_ptr), len) };
        match file.write(buf) {
            Ok(n) => Ok(vec![VmValue::scalar(Register::from_i64(n as i64))]),
            Err(_) => Ok(vec![VmValue::scalar(Register::from_i64(-1))]),
        }
    } else {
        Ok(vec![VmValue::scalar(Register::from_i64(-1))])
    }
}

fn fs_close(args: &[VmValue], fs: &Fs) -> NativeResult {
    let fd = unsafe { scalar_arg(args, "fs.close")?.i64 };
    let mut files = fs.files.lock().unwrap();
    if files.remove(&fd).is_some() {
        Ok(vec![VmValue::scalar(Register::from_i64(0))])
    } else {
        Ok(vec![VmValue::scalar(Register::from_i64(-1))])
    }
}

impl NativeModule for Fs {
    fn name(&self) -> &str {
        "fs"
    }

    fn exports(&self) -> u16 {
        4
    }

    fn call(&self, index: u16, args: &[VmValue], memory: &mut Memory) -> NativeResult {
        match index {
            0 => fs_open(args, memory, self),
            1 => fs_read(args, memory, self),
            2 => fs_write(args, memory, self),
            3 => fs_close(args, self),
            _ => Err(NativeError::new(format!("fs: unknown function {index}"))),
        }
    }

    fn function_index(&self, name: &str) -> Option<u16> {
        match name {
            "open" => Some(0),
            "read" => Some(1),
            "write" => Some(2),
            "close" => Some(3),
            _ => None,
        }
    }
}

// ── time module ───────────────────────────────────────────────────

pub struct Time;

fn time_now(_args: &[VmValue]) -> NativeResult {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    Ok(vec![VmValue::scalar(Register::from_i64(ms))])
}

fn time_sleep(args: &[VmValue]) -> NativeResult {
    // SAFETY: see file-level safety contract
    let ms = unsafe { scalar_arg(args, "time.sleep")?.i64 };
    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    Ok(Vec::new())
}

impl NativeModule for Time {
    fn name(&self) -> &str {
        "time"
    }

    fn exports(&self) -> u16 {
        2
    }

    fn call(&self, index: u16, args: &[VmValue], _memory: &mut Memory) -> NativeResult {
        match index {
            0 => time_now(args),
            1 => time_sleep(args),
            _ => Err(NativeError::new(format!("time: unknown function {index}"))),
        }
    }

    fn function_index(&self, name: &str) -> Option<u16> {
        match name {
            "now" => Some(0),
            "sleep" => Some(1),
            _ => None,
        }
    }
}

// ── rand module ───────────────────────────────────────────────────

pub struct Rng {
    state: Mutex<u64>,
}

impl Rng {
    pub fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        Rng {
            state: Mutex::new(if seed == 0 { 1 } else { seed }),
        }
    }
}

fn rand_u64(state: &Mutex<u64>) -> u64 {
    let mut s = state.lock().unwrap();
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    *s
}

fn rng_u64(state: &Mutex<u64>, _args: &[VmValue]) -> NativeResult {
    let v = rand_u64(state);
    Ok(vec![VmValue::scalar(Register::from_u64(v))])
}

fn rng_range(state: &Mutex<u64>, args: &[VmValue]) -> NativeResult {
    if args.len() < 2 {
        return Err(NativeError::new(
            "rand.range expects 2 arguments (min, max)",
        ));
    }
    // SAFETY: see file-level safety contract
    let min = unsafe {
        args[0]
            .as_scalar()
            .ok_or_else(|| NativeError::new("rand.range: min must be scalar"))?
            .i64
    };
    let max = unsafe {
        args[1]
            .as_scalar()
            .ok_or_else(|| NativeError::new("rand.range: max must be scalar"))?
            .i64
    };
    if min > max {
        return Err(NativeError::new("rand.range: min > max"));
    }
    let range = (max - min + 1) as u64;
    let v = rand_u64(state) % range;
    Ok(vec![VmValue::scalar(Register::from_i64(min + v as i64))])
}

impl NativeModule for Rng {
    fn name(&self) -> &str {
        "rand"
    }

    fn exports(&self) -> u16 {
        2
    }

    fn call(&self, index: u16, args: &[VmValue], _memory: &mut Memory) -> NativeResult {
        match index {
            0 => rng_u64(&self.state, args),
            1 => rng_range(&self.state, args),
            _ => Err(NativeError::new(format!("rand: unknown function {index}"))),
        }
    }

    fn function_index(&self, name: &str) -> Option<u16> {
        match name {
            "u64" => Some(0),
            "range" => Some(1),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::memory::Memory;

    #[test]
    fn test_time_now() {
        let time = Time;
        let result = time.call(0, &[], &mut Memory::new(1)).unwrap();
        let ms = unsafe { result[0].as_scalar().unwrap().i64 };
        assert!(ms > 0);
    }

    #[test]
    fn test_time_sleep() {
        let time = Time;
        let arg = VmValue::scalar(Register::from_i64(10));
        let result = time.call(1, &[arg], &mut Memory::new(1));
        assert!(result.is_ok());
    }

    #[test]
    fn test_rand_u64() {
        let rng = Rng::new();
        let result = rng.call(0, &[], &mut Memory::new(1)).unwrap();
        let v = unsafe { result[0].as_scalar().unwrap().u64 };
        // extremely unlikely to be 0 with xorshift seeded by nanosecond time
        assert!(v != 0);
    }

    #[test]
    fn test_rand_range() {
        let rng = Rng::new();
        for _ in 0..100 {
            let result = rng
                .call(
                    1,
                    &[
                        VmValue::scalar(Register::from_i64(10)),
                        VmValue::scalar(Register::from_i64(20)),
                    ],
                    &mut Memory::new(1),
                )
                .unwrap();
            let v = unsafe { result[0].as_scalar().unwrap().i64 };
            assert!(v >= 10 && v <= 20, "got {v}");
        }
    }

    #[test]
    fn test_fs_open_write_read_close() {
        let fs = Fs::new();
        let mut mem = Memory::new(1024);

        let temp_path = std::env::temp_dir().join("ember_test_fs.txt");
        let path_str = temp_path.to_str().unwrap();
        let path_bytes = path_str.as_bytes();

        let path_ptr = mem.alloc(path_bytes.len());
        unsafe {
            std::ptr::copy_nonoverlapping(
                path_bytes.as_ptr(),
                mem.as_mut_ptr().add(path_ptr),
                path_bytes.len(),
            );
        }

        // open for write (mode=1)
        let result = fs
            .call(
                0,
                &[
                    VmValue::scalar(Register::from_ptr(path_ptr)),
                    VmValue::scalar(Register::from_u64(path_bytes.len() as u64)),
                    VmValue::scalar(Register::from_i64(1)),
                ],
                &mut mem,
            )
            .unwrap();
        let fd = unsafe { result[0].as_scalar().unwrap().i64 };
        assert!(fd >= 0, "open for write failed");

        // write data
        let data = b"hello ember fs";
        let data_ptr = mem.alloc(data.len());
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                mem.as_mut_ptr().add(data_ptr),
                data.len(),
            );
        }

        let result = fs
            .call(
                2,
                &[
                    VmValue::scalar(Register::from_i64(fd)),
                    VmValue::scalar(Register::from_ptr(data_ptr)),
                    VmValue::scalar(Register::from_u64(data.len() as u64)),
                ],
                &mut mem,
            )
            .unwrap();
        let written = unsafe { result[0].as_scalar().unwrap().i64 };
        assert_eq!(written, data.len() as i64);

        // close
        let result = fs
            .call(3, &[VmValue::scalar(Register::from_i64(fd))], &mut mem)
            .unwrap();
        assert_eq!(unsafe { result[0].as_scalar().unwrap().i64 }, 0);

        // open for read (mode=0)
        let result = fs
            .call(
                0,
                &[
                    VmValue::scalar(Register::from_ptr(path_ptr)),
                    VmValue::scalar(Register::from_u64(path_bytes.len() as u64)),
                    VmValue::scalar(Register::from_i64(0)),
                ],
                &mut mem,
            )
            .unwrap();
        let fd2 = unsafe { result[0].as_scalar().unwrap().i64 };
        assert!(fd2 >= 0, "open for read failed");

        // read back
        let read_buf = mem.alloc(128);
        let result = fs
            .call(
                1,
                &[
                    VmValue::scalar(Register::from_i64(fd2)),
                    VmValue::scalar(Register::from_ptr(read_buf)),
                    VmValue::scalar(Register::from_u64(128)),
                ],
                &mut mem,
            )
            .unwrap();
        let n = unsafe { result[0].as_scalar().unwrap().i64 };
        assert_eq!(n, data.len() as i64);

        // SAFETY: n was returned by fs.read which wrote exactly n bytes into read_buf
        let read_data =
            unsafe { std::slice::from_raw_parts(mem.as_ptr().add(read_buf), n as usize) };
        assert_eq!(read_data, data);

        // close read fd
        let result = fs
            .call(3, &[VmValue::scalar(Register::from_i64(fd2))], &mut mem)
            .unwrap();
        assert_eq!(unsafe { result[0].as_scalar().unwrap().i64 }, 0);

        // clean up
        std::fs::remove_file(&temp_path).ok();
    }
}
