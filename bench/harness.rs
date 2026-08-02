use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use ember::bytecode::binary::encode_module;
use ember::bytecode::text::parse_module;
use ember::vm::native::Core;
use ember::{Module, NativeError, NativeLinker, NativeModule, NativeResult, Vm};

pub const VM_MEMORY_BYTES: usize = 1024 * 1024;

pub struct Workload {
    pub name: &'static str,
    pub source_path: PathBuf,
    pub expected: u64,
}

pub fn fib_inline() -> Workload {
    Workload {
        name: "fib_inline",
        source_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("bench/workloads/fib_inline.embt"),
        expected: 832_040,
    }
}

pub fn fib_function() -> Workload {
    Workload {
        name: "fib_function",
        source_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("bench/workloads/fib_function.embt"),
        expected: 832_040,
    }
}

pub fn memory() -> Workload {
    Workload {
        name: "memory",
        source_path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bench/workloads/memory.embt"),
        expected: 150,
    }
}

pub fn closure() -> Workload {
    Workload {
        name: "closure",
        source_path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bench/workloads/closure.embt"),
        expected: 10_000,
    }
}

pub fn gc() -> Workload {
    Workload {
        name: "gc",
        source_path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bench/workloads/gc.embt"),
        expected: 1_000,
    }
}

pub fn all_workloads() -> Vec<Workload> {
    vec![fib_inline(), fib_function(), memory(), closure(), gc()]
}

pub fn parse_workload(workload: &Workload) -> Module {
    let source = std::fs::read_to_string(&workload.source_path).unwrap_or_else(|error| {
        panic!("failed to read {}: {error}", workload.source_path.display())
    });
    parse_module(&source).unwrap_or_else(|error| {
        panic!(
            "failed to parse {}: {error}",
            workload.source_path.display()
        )
    })
}

pub fn encode_workload(module: &Module) -> Vec<u8> {
    encode_module(module).unwrap_or_else(|error| panic!("failed to encode workload: {error:?}"))
}

struct BenchSink {
    value: Arc<AtomicU64>,
}

impl NativeModule for BenchSink {
    fn name(&self) -> &str {
        "bench"
    }

    fn exports(&self) -> u16 {
        1
    }

    fn call(
        &self,
        index: u16,
        args: &[u64],
        _memory: &mut ember::vm::memory::Memory,
    ) -> NativeResult {
        if index != 0 {
            return Err(NativeError::new(format!("bench: unknown function {index}")));
        }
        let [value] = args else {
            return Err(NativeError::new(
                "bench.consume expects exactly one scalar argument",
            ));
        };
        self.value.store(*value, Ordering::Relaxed);
        Ok(vec![])
    }

    fn function_index(&self, name: &str) -> Option<u16> {
        (name == "consume").then_some(0)
    }
}

pub fn execute_workload(module: Module, expected: u64) -> Result<(), String> {
    let value = Arc::new(AtomicU64::new(0));
    let mut linker = NativeLinker::default();
    linker.mount(Core);
    linker.mount(BenchSink {
        value: Arc::clone(&value),
    });

    let mut vm = Vm::with_linker(VM_MEMORY_BYTES, linker);
    vm.run_module(module.clone())
        .map_err(|error| format!("workload execution failed: {error:?}"))?;

    let actual = value.load(Ordering::Relaxed);
    if actual != expected {
        return Err(format!(
            "workload result mismatch: expected {expected}, got {actual}"
        ));
    }
    Ok(())
}
