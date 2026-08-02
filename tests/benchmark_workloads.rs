#[path = "../bench/harness.rs"]
mod harness;

#[test]
fn benchmark_workloads_parse_encode_decode_and_execute() {
    for workload in harness::all_workloads() {
        let module = harness::parse_workload(&workload);
        let bytes = harness::encode_workload(&module);
        let decoded = ember::bytecode::binary::decode_module(&bytes)
            .unwrap_or_else(|error| panic!("{} failed to decode: {error:?}", workload.name));
        harness::execute_workload(decoded, workload.expected)
            .unwrap_or_else(|error| panic!("{}: {error}", workload.name));
    }
}
