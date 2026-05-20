use std::fs;
use std::path::{Path, PathBuf};

use ember::bytecode::binary::{decode_module, encode_module};
use ember::bytecode::text::{module_to_text, parse_module, validate_module};
use ember::vm::NativeRegistry;
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
            let module = load_module(&path)?;
            check_imports(&module)?;
            run_module(module)
        }
        "check" => {
            let path = arg_path(&args, 1)?;
            let module = load_module(&path)?;
            check_imports(&module)?;
            println!("ok");
            Ok(())
        }
        "build" => {
            let input = arg_path(&args, 1)?;
            let output = parse_output_path(&args)?;
            let module = load_text_module(&input)?;
            check_imports(&module)?;
            let bytes =
                encode_module(&module).map_err(|error| format!("encode error: {error:?}"))?;
            fs::write(&output, bytes)
                .map_err(|error| format!("failed to write {}: {error}", output.display()))?;
            Ok(())
        }
        "disasm" => {
            let path = arg_path(&args, 1)?;
            let module = load_module(&path)?;
            print!("{}", module_to_text(&module));
            Ok(())
        }
        "dump" => {
            let path = arg_path(&args, 1)?;
            let module = load_module(&path)?;
            println!("{module:#?}");
            Ok(())
        }
        _ => Err(usage()),
    }
}

fn usage() -> String {
    "usage:\n  ember run <file.embt|file.emb>\n  ember check <file.embt|file.emb>\n  ember build <input.embt> -o <output.emb>\n  ember disasm <file.embt|file.emb>\n  ember dump <file.embt|file.emb>".to_string()
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

fn load_text_module(path: &Path) -> Result<Module, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    parse_module(&source).map_err(|error| format!("{}: {error}", path.display()))
}

fn check_imports(module: &Module) -> Result<(), String> {
    validate_module(module).map_err(|error| format!("validation error: {error}"))?;
    let registry = NativeRegistry::with_std();
    for native in &module.natives {
        if !registry.contains(&native.name) {
            return Err(format!("unknown native `{}`", native.name));
        }
    }
    Ok(())
}

fn run_module(module: Module) -> Result<(), String> {
    let mut vm = Vm::new(1024 * 1024);
    vm.run_module(module).map_err(format_vm_error)
}

fn format_vm_error(error: VMError) -> String {
    match error {
        VMError::NativeError(message) => format!("native error: {message}"),
        other => format!("runtime error: {other:?}"),
    }
}
