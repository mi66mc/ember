#[path = "../bench/harness.rs"]
mod harness;

use std::path::PathBuf;
use std::process::Command;

use ember::bytecode::text::parse_module;

fn execute_and_validate(module: ember::Module, expected: u64) -> Result<(), String> {
    let (mut vm, module, validation) = harness::prepare_workload(module, expected).into_parts();
    let execution = vm.run_module(module);
    harness::validate_workload(execution, validation)
}

#[test]
fn benchmark_workloads_parse_encode_decode_and_execute() {
    for workload in harness::all_workloads() {
        let module = harness::parse_workload(&workload);
        let bytes = harness::encode_workload(&module);
        let decoded = ember::bytecode::binary::decode_module(&bytes)
            .unwrap_or_else(|error| panic!("{} failed to decode: {error:?}", workload.name));
        execute_and_validate(decoded, workload.expected)
            .unwrap_or_else(|error| panic!("{}: {error}", workload.name));
    }
}

#[test]
fn benchmark_workload_preparation_and_validation_are_separate_from_vm_execution() {
    let workload = harness::memory();
    let module = harness::parse_workload(&workload);
    let prepared = harness::prepare_workload(module, workload.expected);
    let (mut vm, module, validation) = prepared.into_parts();

    let execution = vm.run_module(module);

    harness::validate_workload(execution, validation)
        .expect("prepared workload must validate after VM execution");
}

#[test]
fn fibonacci_workloads_run_through_the_cargo_provided_cli() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let programs_dir = manifest_dir.join("target/bench-results/programs");
    std::fs::create_dir_all(&programs_dir)
        .unwrap_or_else(|error| panic!("failed to create {}: {error}", programs_dir.display()));

    let cargo_cli = PathBuf::from(env!("CARGO_BIN_EXE_ember"));
    assert!(
        cargo_cli.is_file(),
        "Cargo-provided CLI is missing at {}",
        cargo_cli.display()
    );

    for workload in [harness::fib_inline(), harness::fib_function()] {
        let source = std::fs::read_to_string(&workload.source_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", workload.source_path.display())
        });
        assert!(
            source.contains("bench.consume"),
            "{} must contain bench.consume before CLI transformation",
            workload.name
        );
        let printable_source = source.replace("bench.consume", "io.print_i64");
        assert!(
            !printable_source.contains("bench.consume"),
            "{} did not replace every bench.consume occurrence",
            workload.name
        );

        let module = parse_module(&printable_source)
            .unwrap_or_else(|error| panic!("{} failed to parse: {error}", workload.name));
        let bytes = harness::encode_workload(&module);
        let program_path = programs_dir.join(format!("{}.emb", workload.name));
        std::fs::write(&program_path, bytes)
            .unwrap_or_else(|error| panic!("failed to write {}: {error}", program_path.display()));

        let output = Command::new(&cargo_cli)
            .arg("run")
            .arg(&program_path)
            .output()
            .unwrap_or_else(|error| panic!("failed to run {}: {error}", cargo_cli.display()));
        assert!(
            output.status.success(),
            "{} CLI run failed: {}",
            workload.name,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap_or_else(|error| panic!(
                "{} emitted non-UTF-8 output: {error}",
                workload.name
            )),
            format!("{}\n", workload.expected),
            "{} CLI output",
            workload.name
        );
    }
}
