use std::hint::black_box;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use ember::bytecode::binary::decode_module;
use ember::bytecode::text::parse_module;

#[path = "../bench/harness.rs"]
mod harness;

fn benchmarks(criterion: &mut Criterion) {
    let workload = harness::fib_inline();
    assert_eq!(workload.name, "fib_inline");
    let source = std::fs::read_to_string(&workload.source_path)
        .expect("benchmark workload source must be readable");
    let module = harness::parse_workload(&workload);
    let encoded = harness::encode_workload(&module);

    criterion.bench_function("parse/embt/fib_inline", |bencher| {
        bencher.iter_batched(
            || (),
            |_| {
                black_box(
                    parse_module(black_box(source.as_str()))
                        .expect("benchmark workload source must parse"),
                )
            },
            BatchSize::SmallInput,
        );
    });

    criterion.bench_function("decode/emb/fib_inline", |bencher| {
        bencher.iter_batched(
            || (),
            |_| {
                black_box(
                    decode_module(black_box(encoded.as_slice()))
                        .expect("benchmark workload bytes must decode"),
                )
            },
            BatchSize::SmallInput,
        );
    });

    for workload in harness::all_workloads() {
        let module = harness::parse_workload(&workload);
        let mut execute = criterion.benchmark_group(format!("execute/{}", workload.name));
        if matches!(workload.name, "fib_inline" | "fib_function") {
            // This is logical VM work (10,000 fib calculations), not instructions/second yet.
            execute.throughput(Throughput::Elements(10_000));
        }
        execute.bench_function(workload.name, |bencher| {
            bencher.iter_batched(
                || module.clone(),
                |module| {
                    harness::execute_workload(black_box(module), workload.expected)
                        .expect("benchmark workload must produce its expected result");
                },
                BatchSize::SmallInput,
            );
        });
        execute.finish();
    }
}

criterion_group! {
    name = vm;
    config = Criterion::default()
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(10))
        .sample_size(50);
    targets = benchmarks
}
criterion_main!(vm);
