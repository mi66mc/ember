# Ember performance benchmark protocol

This is the canonical procedure for measuring Ember. It separates in-process
VM-component measurements from startup-inclusive process comparisons. Neither
class of measurement establishes general runtime superiority.

## Why the previous benchmark was retired

The retired program measured itself through the VM's `time.now` native call.
For short runs, millisecond-resolution timing, native-call overhead, and clock
scheduling noise can dominate the work being measured. It was not a reliable
duration for the VM loop.

It also compared an inline Ember Fibonacci loop with Python that performed a
function call per outer iteration. Those workloads do not have equivalent
control flow. The replacement provides separate inline and function-call pairs,
so each Ember workload is compared only with its matching CPython reference.

## Benchmark classes

### Component benchmarks (Criterion)

`benches/vm.rs` runs Criterion in-process. It reports text parsing, binary
decoding, and VM execution independently. Setup is outside the timed execution
routine: execution clones a parsed module, creates a fresh VM, runs it, and
checks its sink result. Parsing, encoding, file I/O, expected-result
construction, and printing are not charged to the execution measurement.

This is a VM-component measurement, not a full CLI benchmark. For Fibonacci,
Criterion reports logical Fibonacci calculations per second; that throughput is
**not** VM instructions per second.

### Process comparisons

`bench/compare.py` starts a new Ember or CPython process for every observation.
It includes process creation, module loading, and final output validation. It
runs only equivalent `fib_inline` and `fib_function` pairs. A command that
fails, writes to stderr, or produces anything other than `832040` cannot publish
a report.

The runner writes `latest.json` (raw samples, configuration, and environment)
and `latest.md` (a descriptive table) to the requested output directory. These
are local generated artifacts and must not be committed.

## Workloads and expected results

| Workload | Measured behavior | Expected sink/result |
| --- | --- | ---: |
| `fib_inline` | 10,000 inline iterative `fib(30)` calculations | 832040 |
| `fib_function` | 10,000 iterative `fib(30)` calculations through the VM function-call path | 832040 |
| `memory` | One 40-byte allocation followed by repeated integer loads, stores, and additions | 150 |
| `closure` | 10,000 closure calls that increment a captured value | 10000 |
| `gc` | 1,000 managed allocations with explicit collection | 1000 |

All Criterion workloads use `bench.consume` to validate output without printing
in the measured execution path. The process comparison transforms the two
Fibonacci fixtures to use `io.print_i64`, then requires exact output from both
runtimes.

## Reproducible baseline

From the repository root on Windows, run these commands in order:

```powershell
cargo test
cargo build --release
cargo bench --bench vm
python bench/compare.py --ember target/release/ember.exe --warmup 5 --samples 30 --output target/bench-results
```

`cargo test` validates parsing, encoding, decoding, execution, expected values,
and the comparison-report contract. The release build supplies the process
runner's CLI. Criterion must complete parse, decode, and all five execution
workloads before a baseline is accepted.

Use at least 5 warmup observations and 30 retained samples for every
startup-inclusive comparison. The runner permits no fewer than one warmup and
ten samples, but the baseline policy above is stricter. Do not compare results
collected with different workload definitions, sample policies, or process scope.

## Reading reports

- **Median** is the middle observation and the primary typical-duration summary.
- **P95** is the 95th-percentile observation and reveals a latency tail that the
  median can hide.
- **MAD** (median absolute deviation) is a robust measure of dispersion around
  the median; a large MAD means the run is unstable and should be investigated
  or repeated.

Treat a report as descriptive data, not a verdict. Retain the JSON alongside any
analysis because it contains individual samples that the generated Markdown omits.

## Required metadata and claim policy

Every retained process report must include a complete environment record: Git
commit and dirty state; generation time; operating system; architecture; CPU
description; Python version; warmup and sample counts; timeout; and CPU affinity
request/status. Record the Ember build configuration and any relevant power,
thermal, background-load, or affinity conditions with the published analysis.

Never make a performance claim from one machine, one run, or one microbenchmark.
Repeat representative workloads on appropriate machines, preserve raw data, and
state the benchmark class and environment. Do not infer whole-program performance
from a component benchmark or instructions per second from Fibonacci's
logical-iteration throughput.

## Final cleanup check

Before committing a benchmark-documentation change, run:

```powershell
cargo test
cargo bench --bench vm --no-run
python bench/reference.py fib_inline
python bench/reference.py fib_function
git diff --check
```

Both Python commands must print `832040`; `git diff --check` must be silent.
