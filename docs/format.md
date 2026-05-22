# Format Specification

Ember uses two module formats: a human-readable text format (`.embt`) and a compact binary format (`.emb`). Both represent the same `Module` structure.

## Text Format (`.embt`)

The text format is a line-oriented assembly language. Each line of bytecode maps one-to-one to an instruction.

### Sections

Sections must appear in this order. Empty lines are ignored.

```
.module "name"           Module name (required, quoted string)
.version N               Format version (optional, defaults to 1)
.entry N                 Entry function index (required for executable modules)
.import                  Import declarations
.constants               Constant table
.callables               Callable table
.functions               Function definitions
```

### `.import`

Declares dependencies. Each line specifies one import.

```
.import
  io.print_i64             Native import: module.function
  "lib.embt".double        External import: "path".function
```

- **Native**: bare word, dot-separated. Resolved at runtime via the linker.
- **External**: quoted path, dot-separated function name. Resolved at link time by merging the referenced module. If no extension is given, `.emb` is tried first, then `.embt`.

### `.constants`

Typed, sequentially numbered constants. Indices must be contiguous starting from 0.

```
.constants
  0 i64 42
  1 f64 3.14
  2 bool true
  3 bytes "hello"
  4 u64 16
```

Valid types: `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `f32`, `f64`, `bool`, `bytes`.

`bytes` constants store raw byte sequences. When loaded via `LOADK`, the VM allocates space in linear memory, copies the bytes, and returns a pointer.

### `.callables`

Maps callable indices to functions or imports. Each callable is self-contained — there is no separate import index indirection.

```
.callables
  0 function 0              Internal function (index into .functions table)
  1 io.print_i64            Native import (must match a .import entry)
  2 "lib.embt".double       External import (must match a .import entry)
```

### `.functions`

Each function has a header line followed by instructions, terminated by `end`.

```
.functions
  0 "function_name" regs=N
    instruction
    instruction
    ...
  end
  1 "another" regs=M
    ...
  end
```

`regs=N` declares the maximum register count for this function. Register indices `0..N-1` are valid within the function body.

### Instructions

Instructions are 4-space indented within functions. Registers are written as `rN` (e.g., `r0`, `r10`). Optional PC numbers are accepted and ignored by the parser.

```
loadk    r0, 0              Load constant index 0 into r0
loadk    r1, 1              Load constant index 1 into r1
add.i64  r2, r0, r1         r2 = r0 + r1
call     r0, 2, 0           Call r0 with 2 args, expect 0 returns
halt                        Terminate
```

Full instruction syntax:

```
halt
nop
loadk    rD, CIDX
loadkx   rD, CIDX           Extended constant index
closure  rD, CIDX
closurex rD, CIDX           Extended callable index
call     rBASE, NARGS, NRETS
ret      rBASE, NRETS
jmp      OFFSET
jmpx     OFFSET             Extended jump offset
jmpif    rTEST, OFFSET
jmpifnot rTEST, OFFSET
move     rDST, rSRC
getg     rDST, GIDX
setg     rSRC, GIDX
conv     rDST, rSRC, TYPEENC
load.X   rDST, rBASE, IMM    X = i8,i16,i32,i64,u8,u16,u32,u64,f32,f64
store.X  rBASE, IMM, rVAL
X.i8     rDST, rA, rB        X = add,sub,mul,div,mod,neg
X.i16    ...                 and,or,xor,not,shl,shr,ushr
X.i32    ...                 eq,ne,lt,le,gt,ge
X.i64    ...
X.u8     ...                 Unsigned arithmetic and comparison variants
X.u16    ...
X.u32    ...
X.u64    ...
X.f32    ...                 Float variants (add,sub,mul,div,neg; eq,ne,lt,le,gt,ge)
X.f64    ...
```

### Labels

Labels provide symbolic names for instruction positions. They resolve to PC-relative offsets during assembly.

```
  @loopname:
  add.i64  r0, r0, r2
  jmp      @loopname
```

A label can appear on the same line as an instruction, or on its own line. It does not consume a PC slot.

### Comments and Escapes

Comments begin with `;;` or `;` and extend to the end of the line.

Quoted strings support C-style escapes: `\n`, `\t`, `\"`, `\\`.

## Binary Format (`.emb`)

All multi-byte integers are little-endian.

### Header

```
Offset  Size  Field
0       4     Magic: "EMB\0" (0x45 0x4D 0x42 0x00)
4       2     Version: u16 (currently 1)
```

### Module Name

```
6       4     Length: u32
10      N     UTF-8 bytes
```

### Entry Point

```
       4     Entry function index as u32, or 0xFFFFFFFF if not set
```

### Imports

```
       4     Count: u32
For each import:
       1     Tag: 0 = native, 1 = external
       4+N   Module name or path: u32 length + UTF-8
       4+N   Function name: u32 length + UTF-8
```

### Constants

```
       4     Count: u32
For each constant:
       1     Tag byte:
             0  = I8 (1 byte)
             1  = I16 (2 bytes LE)
             2  = I32 (4 bytes LE)
             3  = I64 (8 bytes LE)
             4  = U8 (1 byte)
             5  = U16 (2 bytes LE)
             6  = U32 (4 bytes LE)
             7  = U64 (8 bytes LE)
             8  = F32 (4 bytes LE)
             9  = F64 (8 bytes LE)
             10 = Bool (1 byte; 0 = false, non-zero = true)
             11 = Bytes (4+N bytes: u32 length + data)
       N     Payload per tag
```

### Callables

```
       4     Count: u32
For each callable:
       1     Tag: 0 = Function, 1 = Import
       4     ID: u32 (function index or import index)
```

### Functions

```
       4     Count: u32
For each function:
       4+N   Name: u32 length + UTF-8
       1     max_registers: u8
       4     Instruction count: u32
       4*N   Instructions: each [opcode:u8, A:u8, B:u8, C:u8]
```

## Validation

`validate_module` checks:

- Entry function index exists in `functions`
- Every `Callable::Function(id)` points to a valid function index
- Every `Callable::Import(id)` points to a valid import index
- All register accesses within each function are less than `max_registers`
- `LOADK` and `CLOSURE` indices point to existing table entries
- `CALL` args and `RET` returns do not exceed the register frame
