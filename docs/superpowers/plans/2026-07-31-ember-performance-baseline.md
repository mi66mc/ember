# Ember Performance Baseline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a trustworthy, reproducible benchmark suite that measures Ember parsing, binary decoding, VM execution, calls, control flow, memory, closures, and GC, while providing fair CPython comparisons.

**Architecture:** Criterion measures VM components in-process with setup excluded from hot timings. A separate Python runner measures end-to-end Ember and CPython processes with warmups, repeated samples, medians, p95, environment metadata, and machine-readable JSON. Workload fixtures are validated and smoke-tested before any timing is accepted.

**Tech Stack:** Rust 2024, Criterion, Ember `.embt`/`.emb`, Python 3.10+, Cargo tests and benches.

## Global Constraints

- This plan establishes measurements only; it must not optimize or redesign VM production paths.
- Benchmarks must run with release optimizations and must never use `time.now` inside Ember bytecode.
- Parsing, binary decoding, VM construction, and VM execution must be reported as distinct measurements.
- Every workload must have a deterministic expected result verified outside its timed section.
- Comparisons with CPython must use equivalent control flow; inline Ember may only be compared with inline Python, and function-call Ember only with function-call Python.
- Reports must include warmup count, sample count, Python version, Ember commit, operating system, architecture, and CPU description when available.
- Machine-specific timing output must be written below `target/bench-results/` and must not be committed.
- Keep the existing `bench/bench.embt` and `bench/bench.py` until the replacement suite and documentation are verified.

---

## Relationship to the Full VM Roadmap

This is the first independently testable project in the larger remediation roadmap:

1. Establish trustworthy benchmarks and performance baselines.
2. Close bytecode-validation, arithmetic-overflow, and public-API safety holes.
3. Unify the value/register representation and remove redundant register mirrors.
4. Design a precise object model and tracing GC for functional data.
5. Redesign lexical closures and upvalues.
6. Correct linker relocation, binary-format validation, native capabilities, and documentation.
7. Profile and optimize calls, dispatch, superinstructions, allocation, PGO, and target-specific builds.

Each later item requires its own design and implementation plan. Results from this benchmark suite become the acceptance baseline for items 2–7.

## Planned File Structure

- `Cargo.toml`: register Criterion as a development-only dependency and define the `vm` benchmark target.
- `Cargo.lock`: lock the benchmark-only dependencies.
- `bench/harness.rs`: shared workload loader, result sink native module, VM setup, result verification, and environment metadata.
- `bench/workloads/fib_inline.embt`: arithmetic and branch dispatch without bytecode function calls in the hot loop.
- `bench/workloads/fib_function.embt`: the same Fibonacci work with a bytecode function call in each outer iteration.
- `bench/workloads/memory.embt`: repeated checked linear-memory loads and stores.
- `bench/workloads/closure.embt`: repeated closure invocation and upvalue access.
- `bench/workloads/gc.embt`: managed allocation and explicit collection workload.
- `benches/vm.rs`: Criterion groups for text parsing, binary decoding, and execution.
- `tests/benchmark_workloads.rs`: correctness and smoke tests for every workload.
- `bench/reference.py`: CPython implementations equivalent to the cross-runtime Ember workloads.
- `bench/compare.py`: end-to-end comparison runner and JSON/Markdown report generator.
- `bench/BENCH.md`: benchmark protocol, commands, interpretation rules, and initial baseline procedure.
- `.gitignore`: ignore generated benchmark reports under `target/bench-results/`.

### Task 1: Establish the Criterion Harness

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Create: `bench/harness.rs`
- Create: `benches/vm.rs`
- Create: `bench/workloads/fib_inline.embt`

**Interfaces:**
- Produces: `harness::Workload { name: &'static str, source_path: PathBuf, expected: u64 }`
- Produces: `harness::parse_workload(&Workload) -> Module`
- Produces: `harness::encode_workload(&Module) -> Vec<u8>`
- Produces: `harness::execute_workload(Module, u64) -> Result<(), String>`
- Produces: Criterion benchmark names `parse/embt/fib_inline`, `decode/emb/fib_inline`, and `execute/fib_inline`

- [ ] **Step 1: Add the initial workload and verify it through the benchmark harness**

Create `bench/workloads/fib_inline.embt` as the current iterative Fibonacci loop, remove `time.now`, and replace both print calls with a single `bench.consume` native call:

```embt
.module "fib_inline"
.entry 0

.import
  bench.consume

.constants
  0 i64 30
  1 i64 0
  2 i64 1
  3 i64 10000

.callables
  0 bench.consume

.functions
  0 "main" regs=7
    loadk r6, 3
    @outer:
    loadk r0, 0
    loadk r2, 1
    loadk r3, 2
    loadk r5, 2
    @inner:
    jmpifnot r0, @done
    move r4, r2
    move r2, r3
    add.i64 r3, r4, r3
    sub.i64 r0, r0, r5
    jmp @inner
    @done:
    sub.i64 r6, r6, r5
    jmpif r6, @outer
    closure r0, 0, 0
    move r1, r2
    call r0, 1, 0
    halt
  end
```

Run:

The normal CLI must not validate this fixture: it mounts only the production
native linker, while this fixture deliberately imports the benchmark-only
`bench.consume` sink. Validate parsing and execution once the Task 1 harness
mounts `BenchSink`; extending the production linker would be an out-of-scope
semantic/API change.

- [ ] **Step 2: Add Criterion configuration**

Add to `Cargo.toml`:

```toml
[dev-dependencies]
criterion = "0.7"

[[bench]]
name = "vm"
harness = false
```

Run:

```powershell
cargo bench --bench vm --no-run
```

Expected: FAIL because `benches/vm.rs` does not exist.

- [ ] **Step 3: Implement the shared result sink and loaders**

Create `bench/harness.rs`. Implement `BenchSink` with `Arc<AtomicU64>`, expose only one native function named `consume`, and reject any call that does not contain exactly one scalar argument. Implement the interfaces above using:

```rust
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
```

`execute_workload` must mount `BenchSink` on a fresh `NativeLinker`, run a cloned `Module` in a fresh `Vm`, then compare the atomic sink value with `expected`. Return a descriptive `Err` on mismatch.

- [ ] **Step 4: Implement three isolated Criterion measurements**

Create `benches/vm.rs` and include the support module with:

```rust
#[path = "../bench/harness.rs"]
mod harness;
```

Implement:

- `parse/embt/fib_inline`: call `parse_module` on a preloaded source string.
- `decode/emb/fib_inline`: call `decode_module` on bytes encoded before timing.
- `execute/fib_inline`: clone the pre-parsed module during Criterion setup and call `execute_workload` in the measured routine.

Use `criterion::black_box`, `iter_batched`, and `BatchSize::SmallInput`. Do not read files, parse text, encode binaries, construct expected results, or print inside the execution timing.

- [ ] **Step 5: Compile and run the harness**

Run:

```powershell
cargo bench --bench vm --no-run
cargo bench --bench vm
```

Expected: compilation succeeds and Criterion reports all three benchmark names with no result mismatch.

- [ ] **Step 6: Commit the harness**

```powershell
git add Cargo.toml Cargo.lock bench/harness.rs bench/workloads/fib_inline.embt benches/vm.rs
git commit -m "bench: add reproducible VM benchmark harness"
```

### Task 2: Add a Correctness-Gated Workload Corpus

**Files:**
- Modify: `bench/harness.rs`
- Create: `bench/workloads/fib_function.embt`
- Create: `bench/workloads/memory.embt`
- Create: `bench/workloads/closure.embt`
- Create: `bench/workloads/gc.embt`
- Create: `tests/benchmark_workloads.rs`
- Modify: `benches/vm.rs`

**Interfaces:**
- Consumes: `Workload`, `parse_workload`, `encode_workload`, and `execute_workload` from Task 1.
- Produces: `harness::all_workloads() -> Vec<Workload>`.
- Produces: Criterion groups `execute/{fib_inline,fib_function,memory,closure,gc}`.

- [ ] **Step 1: Write the workload smoke test first**

Create `tests/benchmark_workloads.rs`:

```rust
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
```

Run:

```powershell
cargo test --test benchmark_workloads
```

Expected: FAIL because `all_workloads` is not defined.

- [ ] **Step 2: Add the function-call workload**

Create `fib_function.embt` using the same `n=30`, `iterations=10000`, and expected result `832040` as `fib_inline`. Move the inner Fibonacci calculation into function index 1, load callable 0 in every outer iteration, invoke `call` with one argument and one result, and send only the final result to `bench.consume`.

The callable table must be:

```embt
.callables
  0 function 1
  1 bench.consume
```

The callee must return with:

```embt
ret r1, 1
```

This workload intentionally measures `CLOSURE` + `CALL` + `RET` overhead in the outer loop.

- [ ] **Step 3: Add the memory workload**

Create `memory.embt` by adapting the existing `examples/memory/main.embt` algorithm. Allocate one 40-byte block before a 10000-iteration loop; perform the five stores, five loads, and sum during every iteration; call `bench.consume` once with expected result `150`. Do not allocate or call a native inside the timed loop.

- [ ] **Step 4: Add the closure workload**

Create `closure.embt` from the existing counter semantics:

- construct one closure capturing an initial `0`;
- call it 10000 times;
- each call executes `GETUPVAL`, increments by one, executes `SETUPVAL`, and returns the new value;
- call `bench.consume` once with expected result `10000`.

Use a dedicated register for the closure so the returned scalar never overwrites the only reference to it.

- [ ] **Step 5: Add the GC workload**

Create `gc.embt` with 1000 repetitions of:

- `core.alloc_gc(type_tag=1, size=32)`;
- write one `i64` into the allocated payload;
- overwrite the pointer register;
- invoke `core.gc_collect`.

Maintain a scalar counter and call `bench.consume` once with expected result `1000`. This workload intentionally measures the current allocation and explicit collection implementation, not a future tracing design.

- [ ] **Step 6: Register workloads and make the correctness test pass**

Implement `all_workloads()` with exact expected values:

```rust
vec![
    fib_inline(),
    fib_function(),
    memory(),
    closure(),
    gc(),
]
```

Run:

```powershell
cargo test --test benchmark_workloads
cargo test
```

Expected: every workload round-trips through `.emb` and produces its expected value; the complete test suite passes.

- [ ] **Step 7: Parameterize the Criterion execution group**

Update `benches/vm.rs` to iterate over `all_workloads()`. Keep parse and decode groups, and add each workload to `execute/<name>`.

Run:

```powershell
cargo bench --bench vm
```

Expected: Criterion reports five execution workloads independently.

- [ ] **Step 8: Commit the workload corpus**

```powershell
git add bench/harness.rs bench/workloads benches/vm.rs tests/benchmark_workloads.rs
git commit -m "bench: cover calls memory closures and GC"
```

### Task 3: Make the CPython Comparison Semantically Fair

**Files:**
- Create: `bench/reference.py`
- Create: `bench/compare.py`
- Modify: `tests/benchmark_workloads.rs`

**Interfaces:**
- Produces: `reference.run_inline(iterations: int, n: int) -> int`.
- Produces: `reference.run_function(iterations: int, n: int) -> int`.
- Produces CLI: `python bench/reference.py {fib_inline|fib_function}` printing only the integer result.
- Produces CLI: `python bench/compare.py --ember PATH --warmup 5 --samples 30 --output DIR`.

- [ ] **Step 1: Write deterministic Python references**

Create `bench/reference.py`:

```python
def fib(n: int) -> int:
    a, b = 0, 1
    for _ in range(n):
        a, b = b, a + b
    return a

def run_function(iterations: int = 10_000, n: int = 30) -> int:
    result = 0
    for _ in range(iterations):
        result = fib(n)
    return result

def run_inline(iterations: int = 10_000, n: int = 30) -> int:
    result = 0
    for _ in range(iterations):
        a, b = 0, 1
        remaining = n
        while remaining:
            a, b = b, a + b
            remaining -= 1
        result = a
    return result
```

Add an `argparse` CLI accepting exactly `fib_inline` or `fib_function`, and print only `832040`.

Run:

```powershell
python bench/reference.py fib_inline
python bench/reference.py fib_function
```

Expected: both commands print `832040`.

- [ ] **Step 2: Add process-level Ember fixtures**

The Criterion fixtures use `bench.consume`, which the normal CLI does not mount. Extend `tests/benchmark_workloads.rs` with a helper that reads the source and performs the exact textual replacement below before parsing:

```rust
let cli_source = source.replace("bench.consume", "io.print_i64");
assert_ne!(cli_source, source, "fixture did not contain bench.consume");
let module = ember::bytecode::text::parse_module(&cli_source)
    .expect("CLI fixture must parse");
let bytes = ember::bytecode::binary::encode_module(&module)
    .expect("CLI fixture must encode");
```

Write the generated `.emb` files into `target/bench-results/programs`, then verify the release CLI output is `832040`. This source-level transformation is required because `Module` tables are intentionally private to the library.

Generate only `fib_inline.emb` and `fib_function.emb` for cross-runtime comparison. Do not commit generated binaries.

- [ ] **Step 3: Implement the comparison runner**

Create `bench/compare.py` using only the Python standard library. For each pair:

- Ember inline: `ember run target/bench-results/programs/fib_inline.emb`.
- Python inline: `python bench/reference.py fib_inline`.
- Ember function: `ember run target/bench-results/programs/fib_function.emb`.
- Python function: `python bench/reference.py fib_function`.

Use `time.perf_counter_ns()` around each child process, validate stdout equals `832040`, run five untimed warmups by default, then 30 measured samples. Compute minimum, median, p95, maximum, and median absolute deviation in milliseconds. Reject non-zero exit codes and wrong results immediately.

- [ ] **Step 4: Record environment metadata and reports**

Write both:

- `target/bench-results/latest.json` containing raw samples, summary statistics, command, git commit, dirty flag, Python version, OS, architecture, and CPU description.
- `target/bench-results/latest.md` containing a human-readable table and a warning that process measurements include startup.

Do not label Ember “faster than Python” unless the median and p95 are both lower for the same workload and the complete environment metadata is present.

- [ ] **Step 5: Run the comparison**

```powershell
cargo build --release
cargo test --test benchmark_workloads
python bench/compare.py --ember target/release/ember.exe --warmup 5 --samples 30 --output target/bench-results
```

Expected: four result rows, 30 valid samples per row, JSON and Markdown reports created, all outputs equal `832040`.

- [ ] **Step 6: Commit the fair comparison tools**

```powershell
git add bench/reference.py bench/compare.py tests/benchmark_workloads.rs
git commit -m "bench: add fair CPython comparison runner"
```

### Task 4: Add Reproducibility Guardrails

**Files:**
- Modify: `.gitignore`
- Modify: `bench/compare.py`
- Modify: `benches/vm.rs`
- Create: `tests/benchmark_report.rs`

**Interfaces:**
- Consumes: JSON schema emitted by `bench/compare.py`.
- Produces: stable schema version `1`.
- Produces: optional CLI flags `--cpu-affinity`, `--timeout-seconds`, and `--label`.

- [ ] **Step 1: Define and test the report schema**

Create `tests/benchmark_report.rs` to run:

```powershell
python bench/compare.py --help
```

and assert that the help text includes `--ember`, `--warmup`, `--samples`, `--output`, `--timeout-seconds`, and `--label`. Also load a checked-in minimal JSON string in the test and assert required top-level fields: `schema_version`, `environment`, `configuration`, and `results`.

- [ ] **Step 2: Add failure boundaries**

Update `compare.py`:

- default subprocess timeout: 30 seconds;
- refuse `samples < 10`;
- refuse `warmup < 1`;
- store failures with command, exit code, stdout, and stderr;
- exit non-zero without publishing `latest.json` if any sample fails;
- write reports atomically via a temporary file followed by `Path.replace`.

- [ ] **Step 3: Add optional Windows affinity**

When `--cpu-affinity N` is supplied on Windows, use `subprocess.CREATE_NEW_PROCESS_GROUP` and `psutil` must not be introduced. If setting affinity cannot be done using standard-library/OS facilities, print a clear warning, record `"cpu_affinity": null`, and continue. On unsupported systems, record the requested value and the unsupported status rather than silently claiming affinity.

- [ ] **Step 4: Configure Criterion stability**

For every execution group, set:

```rust
group.warm_up_time(Duration::from_secs(3));
group.measurement_time(Duration::from_secs(10));
group.sample_size(50);
```

Use throughput based on logical VM iterations for `fib_inline` and `fib_function`; document that throughput is not yet an instruction-per-second metric.

- [ ] **Step 5: Ignore generated results**

Add:

```gitignore
/target/bench-results/
```

Run:

```powershell
cargo test
cargo bench --bench vm --no-run
python bench/compare.py --help
git status --short
```

Expected: tests pass, benchmark compiles, help shows all flags, generated result files are absent from Git status.

- [ ] **Step 6: Commit reproducibility guardrails**

```powershell
git add .gitignore bench/compare.py benches/vm.rs tests/benchmark_report.rs
git commit -m "bench: add reproducibility and reporting guardrails"
```

### Task 5: Replace Stale Benchmark Documentation and Capture Baseline

**Files:**
- Modify: `bench/BENCH.md`
- Delete after replacement verification: `bench/bench.emb`
- Delete after replacement verification: `bench/bench.embt`
- Delete after replacement verification: `bench/bench.py`

**Interfaces:**
- Consumes: Criterion commands and `compare.py` from Tasks 1–4.
- Produces: one canonical benchmark protocol in `bench/BENCH.md`.

- [ ] **Step 1: Rewrite the protocol**

Document:

- why the old internal `time.now` measurement was invalid for short runs;
- why the old Ember-inline versus Python-function comparison was not equivalent;
- the difference between component benchmarks and process benchmarks;
- exact build, test, Criterion, and comparison commands;
- workload descriptions and expected results;
- minimum warmup/sample policy;
- how to read median, p95, and median absolute deviation;
- environment metadata requirements;
- the rule forbidding performance claims from a single machine or one microbenchmark.

- [ ] **Step 2: Verify all documented commands**

Run exactly:

```powershell
cargo test
cargo build --release
cargo bench --bench vm
python bench/compare.py --ember target/release/ember.exe --warmup 5 --samples 30 --output target/bench-results
```

Expected: all tests pass, all Criterion workloads complete, and both reports are generated.

- [ ] **Step 3: Remove superseded benchmark files**

Only after Step 2 succeeds, remove the three old benchmark artifacts. Verify `bench/BENCH.md` contains no reference to `time.now`, `Measure-Command`, or the old `~51ms`/`~58ms` claims except in the historical explanation.

- [ ] **Step 4: Run the final verification**

```powershell
cargo test
cargo bench --bench vm --no-run
python bench/reference.py fib_inline
python bench/reference.py fib_function
git diff --check
```

Expected: all commands succeed, both Python references print `832040`, and `git diff --check` prints nothing.

- [ ] **Step 5: Commit documentation and cleanup**

```powershell
git add bench/BENCH.md bench
git commit -m "docs: replace stale benchmark protocol"
```

## Final Acceptance Gate

The benchmark foundation is complete only when all of the following are true:

- `cargo test` passes, including every workload round-trip and expected-result check.
- `cargo bench --bench vm --no-run` succeeds.
- Criterion reports parsing, decoding, and five independent execution workloads.
- Cross-runtime comparisons contain equivalent inline and function-call pairs.
- A failed process or wrong result cannot publish a successful report.
- Generated reports contain raw samples, median, p95, dispersion, and environment metadata.
- No generated timing result or `.emb` artifact is tracked by Git.
- The old millisecond-resolution self-timing benchmark is removed.
- No VM optimization or semantic production change is mixed into this benchmark-only project.

## Final Review Remediation (Authoritative Amendments)

The following amendments supersede any earlier step that implies a different
benchmark or test contract. They remain benchmark-, documentation-, and
test-only changes; `src/` is outside their scope.

### Python compatibility and portable report tests

- `bench/compare.py` supports Python 3.10 and newer. UTC timestamps use
  `datetime.now(timezone.utc)` rather than the Python 3.11-only `datetime.UTC`.
- `tests/benchmark_report.rs` selects one Python 3.10+ executable for every
  subprocess it starts. `EMBER_BENCH_PYTHON` is the explicit override;
  otherwise the test probes `python`/`python3` in platform-appropriate order.
- The selected executable is reused for the comparison runner, JSON validator,
  compatibility check, and fake Ember launchers. No fake launcher relies on an
  unconfigured `python` shebang or command.

### Closure fixture correction

- The closure workload entry function declares `regs=6`.
- It loads the initial scalar `0` into final register `r5`, then constructs the
  one-upvalue counter closure in the dedicated register `r4`.
- The workload executes and validates the literal result `10000`; the captured
  value must not depend on an implicitly zero-initialized register.

### Executable identity in retained reports

- Resolve `--ember` to its canonical absolute path before sampling and use that
  path for every Ember command.
- Record the executable separately from Git metadata as
  `environment.ember_executable`, with `canonical_path`, `size_bytes`,
  `mtime_ns`, and a lowercase `sha256` digest of the executable bytes.
- Render the same four identity fields in `latest.md`. Integration coverage
  derives the expected values from the fake executable rather than from the
  report generator.

### Transactional `latest` behavior

- A comparison failure never changes an existing successful `latest.json` or
  `latest.md`.
- Failure diagnostics explicitly state whether prior latest reports were
  preserved or no report existed to publish.
- One integration test must publish a success, retain both files byte-for-byte,
  and then exercise non-zero exit, wrong stdout, and timeout failures in that
  same output directory.

### Interpretation boundary

- Criterion is an in-process component benchmark. Its execution timing isolates
  `Vm::run_module` from parsing, file I/O, process startup, VM construction, and
  result validation.
- `bench/compare.py` is a startup-inclusive process comparison that includes
  process creation, runtime/module loading, and output validation.
- Neither benchmark class establishes general Ember-versus-CPython superiority;
  results apply only to the named workloads, scope, machine, and recorded run.

### Review-fix execution order

For each executable behavior above: add the smallest regression test, run it to
observe the expected RED failure, implement the minimal benchmark/test change,
and rerun the focused test to GREEN before beginning the next behavior. After
the documentation amendment, run the focused report and workload suites, the
complete Cargo suite, benchmark compilation, Python reference checks, formatting
checks, and `git diff --check` before the final documentation commit.
