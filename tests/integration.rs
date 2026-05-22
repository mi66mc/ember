use std::path::Path;

use ember::bytecode::module::link_modules;
use ember::bytecode::text::{parse_module, validate_module};
use ember::vm::native::std_linker;
use ember::{Module, Vm, VMError};

fn load_and_link(path: &Path) -> Result<Module, String> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let module = parse_module(&source).map_err(|e| format!("parse error: {e}"))?;
    let module = if module.imports.iter().any(|i| !i.is_native()) {
        let dir = path.parent().unwrap_or(Path::new("."));
        link_modules(module, &|link_path| {
            let resolved = dir.join(link_path);
            load_and_link(&resolved)
        })?
    } else {
        module
    };
    validate_module(&module).map_err(|e| format!("validation: {e}"))?;
    Ok(module)
}

fn run_module(module: Module) -> Result<(), VMError> {
    let mut vm = Vm::with_linker(1024 * 1024, std_linker());
    vm.run_module(module)
}

fn run_example(name: &str) -> Result<(), String> {
    let path = format!("examples/{name}/main.embt");
    let module = load_and_link(Path::new(&path))?;
    run_module(module).map_err(|e| format!("{e:?}"))
}

#[test]
fn hello_runs() {
    run_example("hello").expect("hello should run");
}

#[test]
fn numbers_runs() {
    run_example("numbers").expect("numbers should run");
}

#[test]
fn loop_runs() {
    run_example("loop").expect("loop should run");
}

#[test]
fn link_runs() {
    run_example("link").expect("link should run");
}

#[test]
fn fib_runs() {
    run_example("fib").expect("fib should run");
}

#[test]
fn memory_runs() {
    run_example("memory").expect("memory should run");
}

#[test]
fn math_runs() {
    run_example("math").expect("math should run");
}

#[test]
fn lib_has_no_entry() {
    let module = load_and_link(Path::new("examples/link/lib.embt")).unwrap();
    assert!(module.entry.is_none());
}

#[test]
fn bytecode_round_trip() {
    use ember::bytecode::binary::{decode_module, encode_module};

    let module = load_and_link(Path::new("examples/numbers/main.embt")).unwrap();
    let encoded = encode_module(&module).unwrap();
    let decoded = decode_module(&encoded).unwrap();
    assert_eq!(decoded.name, "numbers");
    assert_eq!(decoded.functions.len(), module.functions.len());
    run_module(decoded).expect("round-tripped module should run");
}
