# Ember VM vs BEAM, JVM, Lua VM — Structured Comparison

## 1. Type System

### Ember (Today)
- **Scalar-only**: i8, i16, i32, i64, u8, u16, u32, u64, f32, f64, Bool (stored as u64), Ptr (usize).
- Stored in a **64-bit untagged union** (`Register`) with no runtime type tag. Each typed opcode family (ADD_I64, ADD_F64, etc.) knows the type statically.
- `VmValue` wraps `Register` with two reference variants: `Function(Rc<Chunk>)` and `NativeImport(ImportIndex)`.
- `Constant` mirrors the same scalar types plus `Bytes(Vec<u8>)` (allocated into linear memory at LOADK time).
- Constants table is flat — no string interning, no deduplication.
- **Gaps**: No strings, no lists/arrays, no maps/hash-tables, no nil/null sentinel, no objects/structs, no tagged/boxed dynamic type, no symbols/atoms, no big integers.

### BEAM (Erlang/Elixir)
- **Immutable, tagged** values: small integers (tagged in 28+4 bits), floats (boxed on heap), atoms (global interned), tuples, lists (cons cells), binaries (heap-allocated byte arrays with sub-binary references), maps (hash array mapped tries since OTP 17), pids/ports/refs (process identifiers), funs (closures with environment).
- Every term carries a 2-4 bit tag for the GC and runtime dispatch.
- No user-defined mutable state inside the VM — all values are copied between processes.

### JVM
- **Static nominal types**: boolean, byte, short, int, long, float, double, char, object references. Types are erased at the bytecode level via `java.lang.Object` hierarchy.
- Object references point to heap-allocated instances with vtables. Arrays are objects. Strings (`java.lang.String`) are first-class objects with interning via constant pool.
- Type descriptors in the constant pool (`field_info`, `method_info`, `CONSTANT_Class`) encode full signatures.

### Lua VM
- **Dynamic tagged** values: nil, boolean, number (float or integer since 5.3), string (interned), table (hash+array part), function (Lua closure + C closure), userdata (light/heavy for FFI), thread (coroutine).
- Everything is a `TValue` (tagged union with type tag byte). Numbers are either `lua_Integer` (i64) or `lua_Number` (f64), unioned in `Value`.
- Tables serve as the universal data structure (arrays, maps, objects, modules).

### What Ember Needs
| Feature | Priority | Effort |
|---------|----------|--------|
| Tagged runtime type (discriminated VmValue) | High | Replacing Register union with tagged enum; all opcodes need type guards |
| Nil/null sentinel value | High | Representation + VMError for operations on nil |
| String type (interned or heap) | High | String table, LOADK for strings, CONV from bytes→str |
| List/array type | Medium | Heap-allocated growable array |
| Map/hash-table type | Medium | Key-value heap object |
| Struct/user-defined composite | Medium | Schema-based layout |
| Big integers | Low | GMP/rug-based bignum |

---

## 2. Memory Management

### Ember (Today)
- **No GC. Bump allocator + free list** for a linear `Vec<u8>` heap (`Memory` struct, `src/vm/memory.rs`).
- `alloc(size)` → bumps pointer or splits a free-list block. `free(ptr, size)` → adds to free list. `reset()` → bumps to zero, clears free list.
- Functions use `Rc<Chunk>` reference counting — the only form of automatic memory management.
- Heap is a fixed-size pre-allocated buffer that can grow via `grow()`.
- Read/write via typed `read<T>`/`write<T>` (unsafe) and `read_checked`/`write_checked` (safe).

### BEAM
- **Per-process generational GC** (minor + major). Each Erlang process has its own heap (typically small, ~2KB).
- Young heap (nursery) + old heap. Minor GC collects nursery (fast, stop-the-world for that process only). Major GC (mark-sweep or mark-compact) runs when old heap is full.
- Shared heap for large binaries (>64 bytes) since OTP 20 — reference-counted with process-local reference tracking.

### JVM
- **Pluggable generational GCs**: Serial, Parallel, G1 (default modern), ZGC (low-latency), Shenandoah.
- Heap split into Young (Eden + Survivor) and Old (Tenured) generations. Objects promoted based on age.
- Escape analysis for stack allocation of short-lived objects. Thread-local allocation buffers (TLABs).

### Lua VM
- **Incremental mark-sweep GC** with generational mode (since 5.4). Tri-color marking, write barriers.
- String interning (all strings are unique; hash lookup on creation).
- Tables have array part (integer-keyed, stored contiguously) and hash part — both can resize independently.
- `__gc` metamethod for userdata finalization.

### What Ember Needs
| Feature | Priority | Effort |
|---------|----------|--------|
| Tracing GC (mark-sweep or copying) | High | Root scanning from stack frames + globals; write barriers; object header with mark bits |
| Reference types with GC integration | High | VmValue tagged refs pointing to GC-managed heap, replacing bump-allocated ptrs |
| Generational nursery | Medium | Young-space collection, remembered sets |
| String interning | Medium | Intern table, deduplication at constant/load time |

---

## 3. Function Model

### Ember (Today)
- **Flat functions**: `Function { name, chunk }` — no environment, no captures.
- `CLOSURE` instruction resolves a `Callable` reference, producing a `VmValue::Function(Rc<Chunk>)` or `VmValue::NativeImport`.
- `CALL base, arg_count, expected_returns` — args laid out consecutively from `base+1`. Multiple return values are supported (RET returns N values, CALL captures up to expected_returns).
- **No upvalues**: Functions cannot capture variables from enclosing scopes. There is no "closure" in the lexical sense — `CLOSURE` is just function lookup by table index.
- **No tail call optimization**: Every call pushes a frame.
- Stack frames are fixed-size: `Frame { chunk, pc, registers: Box<[VmValue]>, return_base, expected_returns }`.

### BEAM
- **Closures (funs)**: `make_fun` instructions capture up to 256 free variables from the surrounding function's stack (the "environment" vector).
- **Tail calls** via `call_last` / `call_only` — replace current frame instead of pushing. Essential for OTP actor loops.
- Multiple return values are the norm (returns `N` values from a continuation label).
- Functions are identified by `{Module, Name, Arity}` triples. Hot code reloading uses module-level indirection (two versions of the same module can coexist).

### JVM
- **Methods belong to classes**. Invocations: `invokestatic`, `invokevirtual` (vtable dispatch), `invokespecial` (constructors/super), `invokeinterface`, `invokedynamic` (bootstrap method + call site caching).
- **Lambdas**: `invokedynamic` generates anonymous classes that capture free variables as constructor arguments. No first-class "upvalue" mechanism — captured vars become fields.
- **Tail calls**: Not in the JVM spec. Neither `invoke` nor `invokedynamic` support tail call elimination. Libraries (e.g., JDK streams) use loops instead.

### Lua VM
- **Closures**: `CLOSURE` instruction creates a closure from a function prototype + array of upvalue descriptors. Upvalues are either open (point to local var still on stack of enclosing function) or closed (copied into the closure when the outer function returns).
- `GETUPVAL` / `SETUPVAL` instructions access/set upvalues by index. Upvalues are shared — if two closures capture the same outer variable, they share the same `UpVal` object.
- **Tail calls** via `RETURN` with a function call as the return value — reuses the current stack slot. Essential for state machines and proper tail recursion.
- Multiple return values: `RETURN R(A), R(A+1), ..., R(A+B-2)` returns B-1 values. Callers can adjust via `CALL` parameter count.

### What Ember Needs
| Feature | Priority | Effort |
|---------|----------|--------|
| Upvalue capture (closures) | High | Upvalue storage in VmValue; CLOSURE instruction extended to capture N upvalues; GETUPVAL/SETUPVAL opcodes |
| Tail call optimization | Medium | CALL_TAIL opcode that reuses/replaces current frame; careful stack management for multi-return |
| Function identity (module-qualified names) | Low | Module-scoped function IDs; import resolution at compile time |

---

## 4. Error Handling

### Ember (Today)
- `VMError` enum: `StackUnderflow`, `DivisionByZero`, `InvalidConstantIndex`, `InvalidCallableIndex`, `InvalidFunctionIndex`, `InvalidConversionType`, `InvalidProgramCounter`, `InvalidRegister`, `ExpectedScalar`, `ExpectedFunction`, `UnresolvedNativeImport`, `NativeError(String)`, `InvalidJump`, `MemoryOutOfBounds`, `Runtime { message, backtrace }`, `Halted`.
- Non-Runtime errors are auto-wrapped into `VMError::Runtime` with a backtrace captured from `self.stack.frames()` — collecting function names, PCs, and source lines.
- **No try/catch**: No mechanism in the bytecode for user-level exception handling. No `UNWIND` or `THROW` instructions.
- **No finally/defer**: No cleanup-on-error mechanism.
- Stack traces are strings (`FrameInfo { function_name, pc, source_line }`). No symbolic debug info beyond source map line numbers.

### BEAM
- **try/catch/after**: `try` blocks with catch patterns (matching error classes via pattern matching) and `after` for guaranteed cleanup. Implemented via `try_case` and `try_end` instructions + jump tables.
- `throw/1`, `error/1`, `exit/1` — three distinct exception classes with different semantics (throw is non-local return, error is crash, exit is process termination).
- Stack traces via `erlang:get_stacktrace/0` (pre-OTP 21) or the `__STACKTRACE__` variable in catch clauses. Rich trace with filenames, line numbers, function arities.
- **Process isolation**: Errors crash the process, not the VM. Supervisors restart processes.

### JVM
- **Structured exception handling**: `athrow`, exception tables in `Code` attribute (`start_pc`, `end_pc`, `handler_pc`, `catch_type`). `finally` blocks compiled as subroutines (pre-Java 6) or duplicated code.
- Checked vs unchecked exceptions at the language level; bytecode only cares about `Throwable` superclass.
- Stack traces contain `StackTraceElement[]` with declaring class, method name, file name, line number.
- `try-with-resources` (synthetic `Throwable.addSuppressed`).

### Lua VM
- **Protected calls**: `lua_pcall` / `pcall()` in C/Lua. Sets up an error recovery point on the call stack. On error, unwinds to that point and returns error object.
- `xpcall()` adds a message handler (traceback).
- Error objects can be any Lua value (string, table, etc.). No exception hierarchy.
- No syntax-level try/catch. `pcall(function() ... end)` is the idiomatic pattern.

### What Ember Needs
| Feature | Priority | Effort |
|---------|----------|--------|
| THROW/TRY/CATCH opcodes | High | Exception table per function (start_pc..end_pc → handler_pc); stack unwinding in `step()` loop; catch handler frame setup |
| User-defined error values | Medium | Error payload as VmValue instead of just String |
| Finally/cleanup blocks | Medium | Instr after catch block, or explicit cleanup instruction |
| Column-level source info | Low | Extend SourceLocation usage in backtraces |

---

## 5. Concurrency

### Ember (Today)
- **None**. Single-threaded execution loop (`loop { self.step() }`). No threading primitives, no async, no green threads. The `Vm` struct owns the stack, memory, and linker; no concept of multiple concurrent execution contexts.

### BEAM
- **Actor model (processes)**: Lightweight processes (~2KB initial heap, ~500 words overhead). Preemptive scheduling via reduction counts (each function call/bif costs N reductions; after ~4000 reductions, process yields).
- **Message passing**: Copy-send semantics (`!` operator). Each process has a mailbox (queue). Selective receive via pattern matching on mailbox.
- **No shared mutable state**. Links and monitors for process lifecycle tracking. OTP provides `gen_server`, `gen_statem`, supervisors.
- **SMP**: One scheduler per CPU core since OTP R11B. Dirty schedulers for long-running native code (NIFs).

### JVM
- **Native OS threads (`java.lang.Thread`)**. Synchronization via monitors (`synchronized`, `wait`/`notify`), `java.util.concurrent` (locks, atomics, thread pools, executors).
- **Virtual Threads (Project Loom, Java 21+)**: M:N green threads managed by the JVM, scheduled onto a small pool of carrier threads. Stack chunks are heap objects (continuations) that can be mounted/unmounted.
- Memory model: Java Memory Model (JMM) with happens-before ordering, volatile, final fields.

### Lua VM
- **Coroutines**: Cooperative multitasking via `coroutine.create()`, `coroutine.resume()`, `coroutine.yield()`. Full asymmetric coroutines (stackful). Each coroutine has its own Lua stack.
- **No OS threads**. `lua_State` is not thread-safe (Lua 5.x locks the whole VM). Lua 5.4 `lua_newthread` creates a new coroutine state.
- **No parallelism**. Libraries like `lua-lanes` provide multi-state parallelism with message passing.

### What Ember Needs
| Feature | Priority | Effort |
|---------|----------|--------|
| Coroutines/green threads | Medium | Multiple Vm/stack contexts; YIELD/RESUME opcodes; scheduler loop |
| Actor model (processes) | Low | Per-process heap+stack; message queues; scheduler with reduction counts |
| OS threads + shared-memory safety | Low | Memory model; atomic opcodes; synchronized memory access |
| Async I/O integration | Medium | Event loop; non-blocking native calls; callback scheduling |

---

## 6. Module System

### Ember (Today)
- `Module { name, version, entry, constants, imports, callables, functions }` (`src/bytecode/module.rs`).
- **Text format** (`.embt`) and **binary format** (`.emb`) with parser/serializer.
- **Imports**: `ImportKind::Native { module, function }` (resolved at runtime via NativeLinker) and `ImportKind::External { path, function }` (resolved at link time via `link_modules`).
- **link_modules** (`src/bytecode/module.rs:51`): Recursively resolves external dependencies, merges constants/functions/callables into a single flat module with offset-adjusted indices. Version check: external modules must not exceed root version.
- No namespacing within a module — all function IDs are flat. No cyclic dependency check.
- `CLOSURE` instruction indexes into the flat `callables` table.

### BEAM
- **Module = compilation unit**: `-module(name).`, `-export([...]).`, `-import(Module, [Func/Arity]).`
- **Code server**: `code:load_file/1`, `code:ensure_loaded/1`. Modules loaded on demand into a global code table.
- **Preloaded/embedded modules** (OTP kernel, stdlib). Each module has a version attribute (`-vsn`).
- **Hot code reloading**: Two versions of a module can coexist. Existing processes continue using old code; new calls go to new version. Full module replacement at runtime.
- **BEAM files**: Compiled to `.beam` binary format (IFF-like chunks: `Atom`, `Code`, `StrT`, `ImpT`, `ExpT`, `LitT`, `LocT`, `Attr`, `CInf`, `Dbgi`, `Line`).

### JVM
- **Class files** (`.class`) with `this_class`, `super_class`, constant pool, interfaces, fields, methods, attributes. Fully qualified names (`java/lang/String`).
- **Class loaders**: Hierarchical delegation (bootstrap → extension → application). Custom loaders for dynamic loading, network classes, bytecode generation.
- **Packages & modules (Java 9+ JPMS)**: `module-info.class` with `requires`, `exports`, `provides`, `uses`. Strong encapsulation with access control at module boundaries.
- JAR files aggregate class files with manifest. No versioning at the JVM level (rely on build tools).

### Lua VM
- **No built-in module system**. Modules are a convention: return a table from a file, store in `package.loaded`.
- `require(modname)` searches `package.path` (Lua) and `package.cpath` (C), loads and caches the result.
- No namespacing — everything is in the global table `_G` unless explicitly local.
- Libraries like `LuaRocks` provide package management externally.
- C modules link via `luaopen_*` entry points.

### What Ember Needs
| Feature | Priority | Effort |
|---------|----------|--------|
| Namespaced imports (`import io; io.print_i64`) | Medium | Namespace resolution in text parser; qualified CLOSURE targets |
| Cyclic dependency detection | Medium | Check in `link_modules` before recursion |
| Dynamic module loading at runtime | Medium | EXTLOAD opcode; VM runtime that loads `.emb` files on demand |
| Module version compatibility ranges | Low | Semver-style version constraints (`>= 1.0, < 2.0`) |
| Export visibility control | Low | `pub`/`priv` annotations; link-time validation |

---

## 7. FFI / Native Interop

### Ember (Today)
- **NativeModule trait** (`src/vm/native.rs:19`): `name()`, `exports()`, `call(index, &[VmValue], &mut Memory) -> NativeResult`, `function_index(name) -> Option<u16>`.
- **NativeLinker**: Registry of `Box<dyn NativeModule>`. `mount()`, `resolve(ImportDecl) -> ImportIndex`, `call(ImportIndex, &[VmValue], &mut Memory)`.
- Three built-in modules: **io** (print_i64/u64/f64/bool, print_mem), **core** (alloc, memcpy, memset), **math** (sqrt, abs_i64).
- Native functions receive linear memory mutably; all communication via scalar args + memory writes.
- External imports (`"path".function`) resolve to other Ember modules, not native code.

### BEAM
- **NIFs (Native Implemented Functions)**: C libraries loaded via `erl_nif` API. Functions registered at load with arity. Operate on `ERL_NIF_TERM` values (can inspect/create any Erlang term). Resource objects for wrapping C pointers with destructors.
- **Dirty NIFs** marked as CPU or I/O bound, run on dirty scheduler threads to avoid blocking the BEAM schedulers.
- **Ports**: OS processes communicating via stdin/stdout with Erlang. `open_port({spawn, Cmd}, ...)`.
- **C nodes**: Erlang nodes implemented in C, communicating via distributed Erlang protocol.

### JVM
- **JNI (Java Native Interface)**: Declare `native` methods, implement in C/C++ with `JNIEnv` pointer. Full access to JVM objects, classes, exceptions. Type mapping between Java and C.
- **JNA (Java Native Access)**: Pure Java FFI with dynamic proxies and `com.sun.jna.Library` mapping.
- **Panama (Foreign Function & Memory API, Java 22+)**: `MemorySegment`, `MemoryLayout`, `Linker`, `SymbolLookup`. Safe, efficient native calls without JNI boilerplate. `jextract` for generating bindings from C headers.

### Lua VM
- **C API (`lua_State *L`)**: Stack-based interface. Push C values → call `lua_pcall` → pop results. `luaL_Reg` to register C functions.
- **LuaJIT FFI**: Inline C declarations (`ffi.cdef "int printf(const char *fmt, ...);"`) with automatic type marshaling. Direct `ffi.C.printf`. Vastly more performant than Lua C API.
- **Userdata**: `lua_newuserdata` allocates a block of memory with an associated metatable. Light userdata is a void pointer with no metatable.
- **`package.loadlib`**: Dynamic loading of `.so`/`.dll` with `luaopen_*` entry point.

### What Ember Needs
| Feature | Priority | Effort |
|---------|----------|--------|
| Resource/object wrapping (C pointer + destructor) | High | VmValue::Resource { ptr, drop_fn }; auto-free in NativeModule |
| Dynamic library loading at runtime | Medium | dlopen/LoadLibrary in Core module; `core.dlopen("lib.so")` |
| Foreign function declaration syntax in .embt | Low | `native` block declaring C function signatures with type mapping |
| Async native calls (non-blocking) | Low | Thread pool for native dispatch; callback into VM event loop |

---

## 8. Performance

### Ember (Today)
- **Pure interpreter**: Single `step()` method with a `match instr.opcode()` dispatch. No profiling, no optimization passes.
- Instructions are 32-bit fixed-width (opcode byte + 3 operand bytes).
- `EXT` prefix opcode extends operand ranges to 32 bits for LOADK, CLOSURE, JMP, JMPIF, JMPIFNOT.
- **No JIT compilation**, no inline caching, no superinstructions, no direct threading.
- Typed opcode families (separate ADD for i8, i16, i32, i64, u8, ...) avoid runtime type checks — good for raw throughput.
- `scalar()` / `set_scalar()` use unsafe field access on Register union. No bounds checks on register index during execution (checked at validation time via `validate_module`).

### BEAM
- **Threaded-code interpreter** (computed goto, label-as-value). `beam_hot.h` / `beam_cold.h` split instructions by frequency.
- **No JIT in stock OTP** (Elixir uses BEAM with same interpreter). Research projects: HiPE (native code compilation, deprecated), JIT in OTP 24+ (experimental BeamAsm JIT, x86-64 only, OTP 26 enabled by default).
- **Reduction counting**: Processes are preempted every N reductions (function calls + BIFs), ensuring low latency.
- Binary pattern matching is compiled to efficient bytecode sequences (skip, match, test instructions).

### JVM
- **Multi-tier JIT**: C1 (client compiler, fast startup, simple optimizations), C2 (server compiler, aggressive optimizations including inlining, escape analysis, loop unrolling, vectorization).
- **Tiered compilation**: Interpreted → C1 with profiling → C2 recompilation for hot methods.
- **On-stack replacement (OSR)**: Switch from interpreted to compiled mid-loop.
- **Profile-guided optimization**: Branch prediction, type profiling (monomorphic/bimorphic/megamorphic dispatch).
- Class hierarchy analysis (CHA) for devirtualization. Intrinsics for core JDK methods.

### Lua VM
- **Register-based bytecode** (Lua 5.x): Instructions like `ADD A B C` (A, B, C are register indices). Typically 1.5-2x fewer instructions than stack-based VMs.
- **No JIT in PUC-Rio Lua** (reference implementation). Pure interpreter with direct threaded dispatch.
- **LuaJIT**: Aggressive tracing JIT with guards, specialization, linear trace recording, SSA IR, assembler backend. Frequently outperforms V8 on numeric code. Trace stitching and tail-recursion optimization.
- **Table array/hash split**: `OP_NEWTABLE`, `OP_SETLIST`, `OP_SETTABLE` optimize integer-keyed initialization.

### What Ember Needs
| Feature | Priority | Effort |
|---------|----------|--------|
| Computed-goto / direct-threaded dispatch | Medium | Replace match with label-as-value jump table in `step()` |
| Superinstructions (common opcode pairs fused) | Low | Profile hot paths; hand-write fused `LOADK+ADD_I64` → `LOADK_ADD_I64` |
| Constant folding / dead code elimination pass | Medium | Pre-execution optimization on `Module` or `Chunk` |
| Peephole optimization | Low | Pattern-match and replace adjacent instructions in finished Chunk |
| Method JIT / tracing JIT | Low | Massive effort; would require rearchitecting around bytecode → native translation |

---

## 9. Debugging / Introspection

### Ember (Today)
- **Source maps**: `Chunk.source_map: BTreeMap<u32, SourceLocation>` — maps PC offsets to `SourceLocation { line, column }`.
- **Stack traces**: Captured on non-halted errors in `run_module()`. Backtrace iterates `self.stack.frames()` in reverse, extracting `function_name`, `pc`, and `source_line`.
- **CLI tools**: `ember dump <file.emb>` (Debug-formatted module tree), `ember disasm <file.emb>` (human-readable bytecode text).
- **No**: breakpoints, single-stepping from CLI, variable inspection, hot code reloading, execution tracing, performance profiling, heap inspection.

### BEAM
- **Debugger**: `debugger` application with GUI (`debugger:start()`). Attach to processes, set breakpoints (module/line), single-step, inspect variables.
- **Tracing**: `dbg` module (OTP 22+), `erlang:trace/3` with trace patterns (call, return, send, receive, gc, procs). `recon_trace` for production-safe tracing.
- **Observer**: `observer:start()` — real-time GUI showing process tree, supervision, memory allocation, system load.
- **Hot code reloading**: Two versions of module code coexist. `code:purge/1`, `code:soft_purge/1`. Used for zero-downtime upgrades.
- **Crash dumps**: `erl_crash.dump` on VM crash with full process state, message queues, memory usage.

### JVM
- **JVMTI (JVM Tool Interface)** + **JDI (Java Debug Interface)**: Attach debugger to running JVM. Breakpoints, step, watchpoints, frame pop.
- **`jstack`**: Thread dump with stack traces. `jmap` for heap histograms. `jstat` for GC/compilation stats.
- **Flight Recorder (JFR)**: Low-overhead event recording (allocations, locks, I/O, exceptions, method profiling).
- **JVMTI agents**: Class transformation, method entry/exit callbacks. Used by APM tools (Datadog, New Relic).

### Lua VM
- **`debug` library**: `debug.sethook()` for line/call/return events (used by breakpoint infrastructure). `debug.getinfo()` for stack frames with function name, source, line, locals. `debug.getlocal()` / `debug.setlocal()` for variable inspection. `debug.traceback()` for error traces.
- **No built-in debugger**: External tools (MobDebug, ZeroBrane Studio) use `debug` hooks over TCP.
- **`lua_Debug` struct**: Rich reflection — `name`, `namewhat`, `what`, `source`, `currentline`, `linedefined`, `lastlinedefined`, `nups`, `nparams`, `isvararg`.

### What Ember Needs
| Feature | Priority | Effort |
|---------|----------|--------|
| `DEBUG` opcode / debug hooks (step, breakpoint) | High | Add `DEBUG pc` instruction inserted at each source line; hook callback to CLI or external debugger |
| Variable/register inspection API | Medium | `Vm::inspect_register(reg)`, `Vm::list_frames()` with register dumps |
| Instruction-level execution tracing | Medium | Optional trace mode printing each executed instruction with register state |
| Hot code reloading | Low | Module-level indirection; versioned function dispatch; code purge |
| Performance profiling (instruction count, time per function) | Medium | Instrumentation in `step()`; aggregated per-function counters |
| Heap inspection (memory dump) | Low | Memory walker that understands allocation headers |

---

## 10. Standard Library

### Ember (Today)
- **io**: `print_i64`, `print_u64`, `print_f64`, `print_bool`, `print_mem(ptr, len)` — string output by reading bytes from linear memory.
- **core**: `alloc(size) -> ptr`, `memcpy(dst, src, len)`, `memset(dst, byte, len)`.
- **math**: `sqrt(f64) -> f64`, `abs_i64(i64) -> i64`.
- **Total: 10 native functions** across 3 modules. All I/O goes through Rust's `println!`. No file I/O, no networking, no time/date, no random numbers, no string manipulation, no system access.

### BEAM
- **Massive OTP standard library**: `kernel`, `stdlib`, `sasl`, `crypto`, `public_key`, `ssl`, `inets`, `ssh`, `ftp`, `tftp`, `xmerl`, `wx`, `tools`, `runtime_tools`, `os_mon`, `snmp`, `mnesia`, `observer`, `debugger`, `dialyzer`, `et`, `eldap`, `megaco`, `diameter`, `reltool`, `parsetools`, `syntax_tools`, `eunit`, `common_test`, `asn1`.
- Rich standard modules: `lists`, `maps`, `string`, `binary`, `io`, `file`, `gen_tcp`, `gen_udp`, `inet`, `calendar`, `timer`, `random`/`rand`, `ets` (in-memory key-value store), `dets` (disk-based), `dets`, `queue`, `sets`, `ordsets`, `gb_trees`, `dict`, `array`, `proplists`, `digraph`.

### JVM
- **JDK standard library**: `java.lang` (String, Math, System, Thread, Object), `java.util` (collections: List, Set, Map, Queue, Deque; concurrency: Executors, locks, atomics; streams, Optional, regex, logging, random, time), `java.io` / `java.nio` (files, sockets, channels, buffers, selectors), `java.net` (URL, HttpURLConnection), `java.sql` (JDBC), `javax.crypto`, `java.security`.
- Huge standard library (~4000+ classes). Extensions via Maven/Gradle ecosystem.

### Lua VM
- **Minimal standard library**: `string` (len, sub, find, match, gsub, gmatch, format, byte, char, rep, reverse, upper, lower, pack/unpack), `table` (insert, remove, sort, concat, move, pack/unpack), `math` (abs, acos, asin, atan, ceil, cos, deg, exp, floor, fmod, log, max, min, modf, rad, sin, sqrt, tan, type, ult, random, randomseed, huge, pi, maxinteger, mininteger, tointeger), `io` (open, read, write, close, flush, lines, type, tmpfile), `os` (clock, date, difftime, execute, exit, getenv, remove, rename, setlocale, time, tmpname), `debug` (traceback, getinfo, getlocal, setlocal, getupvalue, setupvalue, sethook, gethook, getregistry), `coroutine` (create, resume, yield, status, wrap, running), `utf8` (char, codes, codepoint, len, offset).
- Intentionally minimal (~200 functions). Community provides everything else via LuaRocks.

### What Ember Needs
| Feature | Priority | Effort |
|---------|----------|--------|
| String manipulation (len, concat, index, slice) | High | `str` native module with operations on memory-resident byte sequences |
| File I/O (open, read, write, close) | High | `fs` native module; implementation via Rust std::fs |
| Time/date | High | `time` native module; `std::time::SystemTime` |
| Random numbers | Medium | `rand` native module; `rand::rngs` |
| Memory utilities (compare, fill pattern) | Medium | Extend `core` with `memcmp`, `memmove` |
| Network (TCP/UDP) | Low | `net` native module via `std::net` |
| List/table/collection operations | Medium | Depends on adding heap-allocated compound types first |
| Math extended (trig, log, pow, min, max) | Medium | Extend `math` module significantly |

---

## Summary: Critical Path

Based on the gaps across all 10 dimensions, here is the recommended implementation order for closing the biggest gaps:

### Phase 1 — Essential completeness (makes it a "real" VM)
1. **Strings** — Heap-allocated, interned strings + string operations in native modules
2. **Nil value** — Sentinel for uninitialized/absent values
3. **Upvalues / closures** — CLOSURE instruction extended with captured variables
4. **Exception handling** — THROW + TRY/CATCH opcodes with unwinding
5. **Basic GC** — Simple mark-sweep to replace the never-freed bump allocator

### Phase 2 — Productivity & debugging
6. **Lists & maps** — Heap-allocated compound types
7. **File I/O** — `fs` native module
8. **Debugger support** — Hook-based stepping, breakpoints, register inspection

### Phase 3 — Advanced features
9. **Tail call optimization** — Reuse stack frames for tail-position calls
10. **Coroutines / green threads** — Cooperative concurrency model
11. **Performance optimizations** — Threaded dispatch, constant folding, peephole opts
