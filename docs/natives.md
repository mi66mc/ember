# Native Modules

Native modules provide functions implemented in Rust that are callable from Ember bytecode. They are registered with the VM at startup via the `NativeLinker`.

## Architecture

A native module implements the `NativeModule` trait:

```rust
pub trait NativeModule: Send + Sync {
    fn name(&self) -> &str;
    fn exports(&self) -> u16;
    fn call(&self, index: u16, args: &[VmValue], memory: &mut Memory) -> NativeResult;
    fn function_index(&self, name: &str) -> Option<u16>;
}
```

- `name()` returns the module name used in `.embt` imports (e.g., `"io"`, `"core"`).
- `exports()` returns the total number of functions provided.
- `call()` dispatches by index. Receives the argument list and mutable access to VM memory.
- `function_index()` maps a function name to its index, enabling import resolution.

### Registration

Modules are registered with a `NativeLinker`:

```rust
let mut linker = NativeLinker::default();
linker.mount(Io);
linker.mount(Core);
linker.mount(Math);
let vm = Vm::with_linker(1024 * 1024, linker);
```

The pre-built `std_linker()` includes `Io`, `Core`, and `Math`.

### Import Resolution

In `.embt`, a native import is written as:

```embt
.import
  io.print_i64
  core.alloc
  math.sqrt
```

At runtime, the VM resolves the import via `NativeLinker::resolve()`, which calls `module.function_index(name)` to get the internal index, then returns `ImportIndex { module, function }`. A `CLOSURE` instruction loads this index into a register as a `VmValue::NativeImport`. A `CALL` dispatches via `linker.call(index, args, memory)`.

### Argument and Return Convention

Native functions receive `&[VmValue]` and return `NativeResult = Result<Vec<VmValue>, NativeError>`. Arguments are the register values collected by `CALL`; returns are written back to the caller's registers.

## Built-in Modules

### `io` — Input/Output

5 exports.

| Index | Function | Arguments | Returns | Description |
|-------|----------|-----------|---------|-------------|
| 0 | `print_i64` | `value: i64` | — | Print as decimal integer |
| 1 | `print_u64` | `value: u64` | — | Print as decimal integer |
| 2 | `print_f64` | `value: f64` | — | Print as decimal float |
| 3 | `print_bool` | `value: bool` | — | Print `true` or `false` |
| 4 | `print_mem` | `ptr: u64, len: u64` | — | Read `len` bytes from memory at `ptr`, print as UTF-8. Invalid UTF-8 is printed lossily. |

### `core` — Memory Management

6 exports.

| Index | Function | Arguments | Returns | Description |
|-------|----------|-----------|---------|-------------|
| 0 | `malloc` | `size: u64` | `ptr: u64` | Allocate `size` bytes in linear memory. No GC tracking. Free with `free(ptr)`. Returns pointer past 8-byte size header. |
| 1 | `free` | `ptr: u64` | — | Free a block allocated by `malloc`. Reads block size from internal header. |
| 2 | `memcpy` | `dst: u64, src: u64, len: u64` | — | Copy `len` bytes from `src` to `dst` in linear memory. Errors if either range is out of bounds. |
| 3 | `memset` | `dst: u64, byte: u8, len: u64` | — | Fill `len` bytes at `dst` with `byte`. Errors if range is out of bounds. |
| 4 | `alloc_gc` | `type_tag: u8, size: u64` | `ptr: u64` | Allocate `size` bytes on the GC-managed heap with a type tag. Returns pointer past 2-byte `[mark][tag]` header. Collected automatically when no register references the pointer. |
| 5 | `gc_collect` | — | — | Trigger a manual GC cycle. Roots are all `ptr` values in active stack frames. |

### `math` — Mathematics

2 exports.

| Index | Function | Arguments | Returns | Description |
|-------|----------|-----------|---------|-------------|
| 0 | `sqrt` | `value: f64` | `result: f64` | Square root |
| 1 | `abs_i64` | `value: i64` | `result: i64` | Absolute value |

## Creating a Custom Module

Implement `NativeModule` on a struct and mount it:

```rust
use ember::vm::native::{NativeModule, NativeResult, NativeError};
use ember::vm::{Memory, Register, VmValue};

struct MyMod;

impl NativeModule for MyMod {
    fn name(&self) -> &str { "mymod" }

    fn exports(&self) -> u16 { 1 }

    fn call(&self, index: u16, args: &[VmValue], _memory: &mut Memory) -> NativeResult {
        match index {
            0 => {
                let value = args[0].as_scalar()
                    .ok_or_else(|| NativeError::new("expected scalar"))?;
                let doubled = unsafe { value.i64 * 2 };
                Ok(vec![VmValue::scalar(Register::from_i64(doubled))])
            }
            _ => Err(NativeError::new(format!("unknown function {index}")))
        }
    }

    fn function_index(&self, name: &str) -> Option<u16> {
        match name {
            "double" => Some(0),
            _ => None,
        }
    }
}
```

Use in `.embt`:

```embt
.import
  mymod.double

.callables
  0 mymod.double

.functions
  0 "main" regs=3
    loadk    r0, 0          r0 = 21
    closure  r1, 0          r1 = mymod.double
    move     r2, r0         r2 = 21 (arg)
    call     r1, 1, 1       r1 = double(21) = 42
    halt
  end
```

## Memory Access from Natives

Native functions receive `&mut Memory`. Use the raw pointer API to read or write:

```rust
fn call(&self, index: u16, args: &[VmValue], memory: &mut Memory) -> NativeResult {
    let ptr = unsafe { args[0].as_scalar().unwrap().ptr };
    let len = unsafe { args[1].as_scalar().unwrap().u64 as usize };

    if ptr + len > memory.size() {
        return Err(NativeError::new("out of bounds"));
    }

    let bytes = unsafe {
        std::slice::from_raw_parts(memory.as_ptr().add(ptr), len)
    };

    // Read bytes...
    Ok(vec![])
}
```

Use `memory.alloc(size)` to allocate and `memory.grow(bytes)` to extend.
