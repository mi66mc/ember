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

pub type NativeResult = Result<Vec<u64>, NativeError>;

pub trait NativeModule: Send + Sync {
    fn name(&self) -> &str;
    fn exports(&self) -> u16;
    fn call(&self, index: u16, args: &[u64], memory: &mut Memory) -> NativeResult;
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
        args: &[u64],
        memory: &mut Memory,
    ) -> Result<Vec<u64>, NativeError> {
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

fn print_i64(args: &[u64]) -> NativeResult {
    let value = args[0] as i64;
    println!("{value}");
    Ok(vec![])
}

fn print_u64(args: &[u64]) -> NativeResult {
    let value = args[0];
    println!("{value}");
    Ok(vec![])
}

fn print_f64(args: &[u64]) -> NativeResult {
    let value = f64::from_bits(args[0]);
    println!("{value}");
    Ok(vec![])
}

fn print_bool(args: &[u64]) -> NativeResult {
    let value = args[0] != 0;
    println!("{value}");
    Ok(vec![])
}

fn print_mem(args: &[u64], memory: &Memory) -> NativeResult {
    if args.len() < 2 {
        return Err(NativeError::new("io.print_mem expects 2 arguments (ptr, len)"));
    }
    let ptr = args[0] as usize;
    let len = args[1] as usize;
    if ptr + len > memory.size() {
        return Err(NativeError::new("io.print_mem: out of bounds"));
    }
    let bytes = unsafe { std::slice::from_raw_parts(memory.as_ptr().add(ptr), len) };
    let s = unsafe { std::str::from_utf8_unchecked(bytes) };
    println!("{s}");
    Ok(vec![])
}

pub struct Io;

impl NativeModule for Io {
    fn name(&self) -> &str {
        "io"
    }

    fn exports(&self) -> u16 {
        5
    }

    fn call(&self, index: u16, args: &[u64], memory: &mut Memory) -> NativeResult {
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

fn malloc_native(args: &[u64], memory: &mut Memory) -> NativeResult {
    if args.is_empty() {
        return Err(NativeError::new("core.malloc expects 1 argument (size)"));
    }
    let size = args[0] as usize;
    let ptr = memory.malloc(size);
    Ok(vec![ptr as u64])
}

fn free_native(args: &[u64], memory: &mut Memory) -> NativeResult {
    if args.is_empty() {
        return Err(NativeError::new("core.free expects 1 argument (ptr)"));
    }
    let ptr = args[0] as usize;
    memory.free_malloc(ptr);
    Ok(vec![])
}

fn memcpy(args: &[u64], memory: &mut Memory) -> NativeResult {
    if args.len() < 3 {
        return Err(NativeError::new("core.memcpy expects 3 arguments"));
    }
    let dst = args[0] as usize;
    let src = args[1] as usize;
    let len = args[2] as usize;
    if src + len > memory.size() || dst + len > memory.size() {
        return Err(NativeError::new("core.memcpy: out of bounds"));
    }
    unsafe {
        let src_ptr = memory.as_ptr().add(src);
        let dst_ptr = memory.as_mut_ptr().add(dst);
        std::ptr::copy(src_ptr, dst_ptr, len);
    }
    Ok(vec![])
}

fn memset(args: &[u64], memory: &mut Memory) -> NativeResult {
    if args.len() < 3 {
        return Err(NativeError::new("core.memset expects 3 arguments"));
    }
    let dst = args[0] as usize;
    let byte = args[1] as u8;
    let len = args[2] as usize;
    if dst + len > memory.size() {
        return Err(NativeError::new("core.memset: out of bounds"));
    }
    unsafe {
        let dst_ptr = memory.as_mut_ptr().add(dst);
        std::ptr::write_bytes(dst_ptr, byte, len);
    }
    Ok(vec![])
}

fn alloc_gc(args: &[u64], memory: &mut Memory) -> NativeResult {
    if args.len() < 2 {
        return Err(NativeError::new(
            "core.alloc_gc expects 2 arguments (type_tag, size)",
        ));
    }
    let type_tag = args[0] as u8;
    let size = args[1] as usize;
    let ptr = memory.alloc_managed(type_tag, size, &[]);
    Ok(vec![ptr as u64])
}

fn gc_collect(args: &[u64], memory: &mut Memory) -> NativeResult {
    let roots: Vec<usize> = args.iter().map(|&v| v as usize).filter(|&p| p != 0).collect();
    memory.collect_gc(&roots);
    Ok(vec![])
}

pub struct Core;

impl NativeModule for Core {
    fn name(&self) -> &str {
        "core"
    }

    fn exports(&self) -> u16 {
        6
    }

    fn call(&self, index: u16, args: &[u64], memory: &mut Memory) -> NativeResult {
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

fn sqrt_f64(args: &[u64]) -> NativeResult {
    let value = f64::from_bits(args[0]);
    Ok(vec![(value.sqrt()).to_bits()])
}

fn abs_i64(args: &[u64]) -> NativeResult {
    let value = args[0] as i64;
    Ok(vec![(value.abs()) as u64])
}

pub struct Math;

impl NativeModule for Math {
    fn name(&self) -> &str {
        "math"
    }

    fn exports(&self) -> u16 {
        2
    }

    fn call(&self, index: u16, args: &[u64], _memory: &mut Memory) -> NativeResult {
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

fn fs_open(args: &[u64], memory: &Memory, fs: &Fs) -> NativeResult {
    if args.len() < 3 {
        return Err(NativeError::new(
            "fs.open expects 3 arguments (path_ptr, path_len, mode)",
        ));
    }
    let path_ptr = args[0] as usize;
    let path_len = args[1] as usize;
    let mode = args[2] as i64;

    if path_ptr + path_len > memory.size() {
        return Err(NativeError::new("fs.open: path out of bounds"));
    }

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
            Ok(vec![fd as u64])
        }
        Err(_) => Ok(vec![(-1i64) as u64]),
    }
}

fn fs_read(args: &[u64], memory: &mut Memory, fs: &Fs) -> NativeResult {
    if args.len() < 3 {
        return Err(NativeError::new(
            "fs.read expects 3 arguments (fd, buf_ptr, len)",
        ));
    }
    let fd = args[0] as i64;
    let buf_ptr = args[1] as usize;
    let len = args[2] as usize;

    if buf_ptr + len > memory.size() {
        return Err(NativeError::new("fs.read: buffer out of bounds"));
    }

    let mut files = fs.files.lock().unwrap();
    if let Some(file) = files.get_mut(&fd) {
        let buf =
            unsafe { std::slice::from_raw_parts_mut(memory.as_mut_ptr().add(buf_ptr), len) };
        match file.read(buf) {
            Ok(n) => Ok(vec![n as u64]),
            Err(_) => Ok(vec![(-1i64) as u64]),
        }
    } else {
        Ok(vec![(-1i64) as u64])
    }
}

fn fs_write(args: &[u64], memory: &Memory, fs: &Fs) -> NativeResult {
    if args.len() < 3 {
        return Err(NativeError::new(
            "fs.write expects 3 arguments (fd, buf_ptr, len)",
        ));
    }
    let fd = args[0] as i64;
    let buf_ptr = args[1] as usize;
    let len = args[2] as usize;

    if buf_ptr + len > memory.size() {
        return Err(NativeError::new("fs.write: buffer out of bounds"));
    }

    let mut files = fs.files.lock().unwrap();
    if let Some(file) = files.get_mut(&fd) {
        let buf = unsafe { std::slice::from_raw_parts(memory.as_ptr().add(buf_ptr), len) };
        match file.write(buf) {
            Ok(n) => Ok(vec![n as u64]),
            Err(_) => Ok(vec![(-1i64) as u64]),
        }
    } else {
        Ok(vec![(-1i64) as u64])
    }
}

fn fs_close(args: &[u64], fs: &Fs) -> NativeResult {
    let fd = args[0] as i64;
    let mut files = fs.files.lock().unwrap();
    if files.remove(&fd).is_some() {
        Ok(vec![0u64])
    } else {
        Ok(vec![(-1i64) as u64])
    }
}

impl NativeModule for Fs {
    fn name(&self) -> &str {
        "fs"
    }

    fn exports(&self) -> u16 {
        4
    }

    fn call(&self, index: u16, args: &[u64], memory: &mut Memory) -> NativeResult {
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

fn time_now(_args: &[u64]) -> NativeResult {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    Ok(vec![ms as u64])
}

fn time_sleep(args: &[u64]) -> NativeResult {
    let ms = args[0] as i64;
    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    Ok(vec![])
}

impl NativeModule for Time {
    fn name(&self) -> &str {
        "time"
    }

    fn exports(&self) -> u16 {
        2
    }

    fn call(&self, index: u16, args: &[u64], _memory: &mut Memory) -> NativeResult {
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

fn rng_u64(state: &Mutex<u64>, _args: &[u64]) -> NativeResult {
    let v = rand_u64(state);
    Ok(vec![v])
}

fn rng_range(state: &Mutex<u64>, args: &[u64]) -> NativeResult {
    if args.len() < 2 {
        return Err(NativeError::new(
            "rand.range expects 2 arguments (min, max)",
        ));
    }
    let min = args[0] as i64;
    let max = args[1] as i64;
    if min > max {
        return Err(NativeError::new("rand.range: min > max"));
    }
    let range = (max - min + 1) as u64;
    let v = rand_u64(state) % range;
    Ok(vec![(min + v as i64) as u64])
}

impl NativeModule for Rng {
    fn name(&self) -> &str {
        "rand"
    }

    fn exports(&self) -> u16 {
        2
    }

    fn call(&self, index: u16, args: &[u64], _memory: &mut Memory) -> NativeResult {
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
        let ms = result[0] as i64;
        assert!(ms > 0);
    }

    #[test]
    fn test_time_sleep() {
        let time = Time;
        let result = time.call(1, &[10u64], &mut Memory::new(1));
        assert!(result.is_ok());
    }

    #[test]
    fn test_rand_u64() {
        let rng = Rng::new();
        let result = rng.call(0, &[], &mut Memory::new(1)).unwrap();
        let v = result[0];
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
                    &[10u64, 20u64],
                    &mut Memory::new(1),
                )
                .unwrap();
            let v = result[0] as i64;
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
                    path_ptr as u64,
                    path_bytes.len() as u64,
                    1u64,
                ],
                &mut mem,
            )
            .unwrap();
        let fd = result[0] as i64;
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
                    fd as u64,
                    data_ptr as u64,
                    data.len() as u64,
                ],
                &mut mem,
            )
            .unwrap();
        let written = result[0] as i64;
        assert_eq!(written, data.len() as i64);

        // close
        let result = fs
            .call(3, &[fd as u64], &mut mem)
            .unwrap();
        assert_eq!(result[0] as i64, 0);

        // open for read (mode=0)
        let result = fs
            .call(
                0,
                &[
                    path_ptr as u64,
                    path_bytes.len() as u64,
                    0u64,
                ],
                &mut mem,
            )
            .unwrap();
        let fd2 = result[0] as i64;
        assert!(fd2 >= 0, "open for read failed");

        // read back
        let read_buf = mem.alloc(128);
        let result = fs
            .call(
                1,
                &[
                    fd2 as u64,
                    read_buf as u64,
                    128u64,
                ],
                &mut mem,
            )
            .unwrap();
        let n = result[0] as i64;
        assert_eq!(n, data.len() as i64);

        let read_data =
            unsafe { std::slice::from_raw_parts(mem.as_ptr().add(read_buf), n as usize) };
        assert_eq!(read_data, data);

        // close read fd
        let result = fs
            .call(3, &[fd2 as u64], &mut mem)
            .unwrap();
        assert_eq!(result[0] as i64, 0);

        // clean up
        std::fs::remove_file(&temp_path).ok();
    }
}
