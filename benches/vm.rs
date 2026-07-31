use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
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

    criterion.bench_function("execute/fib_inline", |bencher| {
        bencher.iter_batched(
            || module.clone(),
            |module| {
                harness::execute_workload(black_box(module), workload.expected)
                    .expect("benchmark workload must produce its expected result");
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(vm, benchmarks);
criterion_main!(vm);
