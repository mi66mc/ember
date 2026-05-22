use std::fs;
use std::path::{Path, PathBuf};

use ember::bytecode::binary::{decode_module, encode_module};
use ember::bytecode::text::{module_to_text, parse_module, validate_module};
use ember::bytecode::module::link_modules;
use ember::vm::native::std_linker;
use ember::{Module, VMError, Vm};

fn main() {
    if let Err(error) = run_cli(std::env::args().skip(1).collect()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run_cli(args: Vec<String>) -> Result<(), String> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err(usage());
    };

    match command {
        "run" => {
            let path = arg_path(&args, 1)?;
            let module = load_module_with_links(&path)?;
            require_entry(&module)?;
            check_imports(&module)?;
            run_module(module)
        }
        "check" => {
            let path = arg_path(&args, 1)?;
            let module = load_module_with_links(&path)?;
            require_entry(&module)?;
            check_imports(&module)?;
            println!("ok");
            Ok(())
        }
        "test" => {
            let path = arg_path(&args, 1)?;
            let module = load_module_with_links(&path)?;
            check_imports(&module)?;
            run_tests(module)
        }
        "build" => {
            let input = arg_path(&args, 1)?;
            let output = parse_output_path(&args)?;
            let module = load_module_with_links(&input)?;
            require_entry(&module)?;
            check_imports(&module)?;
            let bytes =
                encode_module(&module).map_err(|error| format!("encode error: {error:?}"))?;
            fs::write(&output, bytes)
                .map_err(|error| format!("failed to write {}: {error}", output.display()))?;
            Ok(())
        }
        "disasm" => {
            let path = arg_path(&args, 1)?;
            let module = load_module_with_links(&path)?;
            print!("{}", module_to_text(&module));
            Ok(())
        }
        "dump" => {
            let path = arg_path(&args, 1)?;
            let module = load_module_with_links(&path)?;
            println!("{module:#?}");
            Ok(())
        }
        _ => Err(usage()),
    }
}

fn usage() -> String {
    "usage:\n  ember run <file.embt|file.emb>\n  ember check <file.embt|file.emb>\n  ember test <file.embt|file.emb>\n  ember build <input.embt> -o <output.emb>\n  ember disasm <file.embt|file.emb>\n  ember dump <file.embt|file.emb>".to_string()
}

fn arg_path(args: &[String], index: usize) -> Result<PathBuf, String> {
    args.get(index).map(PathBuf::from).ok_or_else(usage)
}

fn parse_output_path(args: &[String]) -> Result<PathBuf, String> {
    let Some(pos) = args.iter().position(|arg| arg == "-o") else {
        return Err("build requires `-o <output.emb>`".to_string());
    };
    args.get(pos + 1)
        .map(PathBuf::from)
        .ok_or_else(|| "build requires `-o <output.emb>`".to_string())
}

fn load_module(path: &Path) -> Result<Module, String> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("embt") => load_text_module(path),
        Some("emb") => {
            let bytes = fs::read(path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
            decode_module(&bytes).map_err(|error| format!("decode error: {error:?}"))
        }
        _ => Err(format!(
            "unsupported file extension for {}; expected .embt or .emb",
            path.display()
        )),
    }
}

fn load_module_with_links(path: &Path) -> Result<Module, String> {
    let raw = load_module(path)?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let merged = link_modules(raw, &|link_path| {
        let resolved = dir.join(link_path);
        load_module(&resolved)
    })?;
    validate_module(&merged).map_err(|error| format!("validation error: {error}"))?;
    Ok(merged)
}

fn load_text_module(path: &Path) -> Result<Module, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    parse_module(&source).map_err(|error| format!("{}: {error}", path.display()))
}

fn check_imports(module: &Module) -> Result<(), String> {
    validate_module(module).map_err(|error| format!("validation error: {error}"))?;
    let linker = std_linker();
    for callable in &module.callables {
        if let ember::Callable::Import(id) = callable {
            let import = &module.imports[*id as usize];
            if import.is_native() && !linker.contains_native(import) {
                return Err(format!("unresolved native import `{import}`"));
            }
        }
    }
    Ok(())
}

fn require_entry(module: &Module) -> Result<(), String> {
    if module.entry.is_none() {
        return Err("module has no entry point".to_string());
    }
    Ok(())
}

fn run_module(module: Module) -> Result<(), String> {
    let mut vm = Vm::with_linker(1024 * 1024, std_linker());
    vm.run_module(module).map_err(format_vm_error)
}

fn run_tests(module: Module) -> Result<(), String> {
    let test_indices: Vec<(u32, String)> = module
        .functions
        .iter()
        .enumerate()
        .filter(|(_, f)| f.name.starts_with("test_"))
        .map(|(idx, f)| (idx as u32, f.name.clone()))
        .collect();

    if test_indices.is_empty() {
        println!("0 tests found");
        return Ok(());
    }

    let mut passed = 0;
    let mut failed = 0;

    for (idx, name) in &test_indices {
        let mut test_module = Module::new(module.name.clone());
        test_module.version = module.version;
        test_module.constants = module.constants.clone();
        test_module.imports = module.imports.clone();
        test_module.callables = module.callables.clone();
        test_module.functions = module.functions.clone();
        test_module.entry = Some(*idx);

        let mut vm = Vm::with_linker(1024 * 1024, std_linker());
        match vm.run_module(test_module) {
            Ok(()) => {
                passed += 1;
                println!("  ok {name}");
            }
            Err(error) => {
                failed += 1;
                println!("  FAIL {name}: {}", format_vm_error(error));
            }
        }
    }

    println!("\n{passed} passed, {failed} failed");
    if failed > 0 {
        Err("some tests failed".to_string())
    } else {
        Ok(())
    }
}

fn format_vm_error(error: VMError) -> String {
    match &error {
        VMError::Runtime { message, backtrace } => {
            let mut out = format!("runtime error: {message}\n");
            for frame in backtrace {
                match frame.source_line {
                    Some(line) => out.push_str(&format!("  at {}:{} (line {})\n", frame.function_name, frame.pc, line)),
                    None => out.push_str(&format!("  at {}:{}\n", frame.function_name, frame.pc)),
                }
            }
            out
        }
        VMError::NativeError(message) => format!("native error: {message}"),
        other => format!("runtime error: {other:?}"),
    }
}
