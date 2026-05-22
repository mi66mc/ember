# Ember

A register-based bytecode virtual machine with linear memory, static module linking, and a native function system.

## Overview

Ember executes bytecode modules written in a human-readable text format (`.embt`) or a compact binary format (`.emb`). The VM uses a fixed-width 32-bit instruction encoding, a growable linear memory space, and a register-based calling convention similar to Lua.

- Register machine with 64-bit typed registers
- Linear memory with bump allocator and free list
- Static module linking with symbol resolution
- Native function system via trait-based modules
- Extensible instruction set with extended-arg prefix

## Installation

```sh
cargo build --release
```

The binary is `target/release/ember`.

## CLI Usage

```
ember run    <file.embt|file.emb>              Run a module
ember check  <file.embt|file.emb>              Validate without running
ember build  <input.embt> -o <output.emb>      Compile to binary
ember disasm <file.embt|file.emb>              Disassemble to text
ember dump   <file.embt|file.emb>              Debug dump
```

## Quick Start

Create `hello.embt`:

```embt
.module "hello"
.entry 0

.import
  io.print_mem

.constants
  0 bytes "hello from ember"
  1 u64 16

.callables
  0 io.print_mem

.functions
  0 "main" regs=3
    closure r0, 0
    loadk r1, 0
    loadk r2, 1
    call r0, 2, 0
    halt
  end
```

Run it:

```sh
ember run hello.embt
# hello from ember
```

## Examples

| Example | Description |
|---------|-------------|
| `examples/hello` | Print a string via `io.print_mem` |
| `examples/numbers` | Arithmetic: `7 * 6 + 3 = 45` |
| `examples/loop` | Sum 1..10 using labels and jumps |
| `examples/link` | Cross-module linking (`double`, `square`) |
| `examples/fib` | Recursive Fibonacci via CALL/RET |
| `examples/memory` | Memory store/load with `core.alloc` |
| `examples/math` | `math.sqrt` and `math.abs_i64` |

## Documentation

- [Opcode Reference](docs/opcodes.md) — complete instruction set with encodings
- [Format Specification](docs/format.md) — `.embt` text and `.emb` binary formats
- [Native Modules](docs/natives.md) — built-in natives and custom module API

## Using as a Library

```rust
use ember::{Module, Vm, std_linker};

let source = std::fs::read_to_string("program.embt")?;
let module = ember::bytecode::text::parse_module(&source)?;
let mut vm = Vm::with_linker(1024 * 1024, std_linker());
vm.run_module(module)?;
```

## License

MIT
