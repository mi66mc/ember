# Opcode Reference

## Instruction Format

Every instruction is exactly 4 bytes (32 bits).

```
[opcode: u8] [A: u8] [B: u8] [C: u8]
```

### Operand Formats

| Format | Method | Encoding |
|--------|--------|----------|
| ABC | `Instruction::abc(op, a, b, c)` | opcode, [a, b, c] |
| ABx | `Instruction::abx(op, a, bx: u16)` | opcode, [a, lo(bx), hi(bx)] |
| AsBx | `Instruction::asbx(op, a, sbx: i16)` | opcode, [a, lo(sbx), hi(sbx)] |
| JMP | `Instruction::jmp(op, offset: i16)` | opcode, [lo(offset), hi(offset), 0] |

### Operand Accessors

| Accessor | Returns | Typical use |
|----------|---------|-------------|
| `a()` | `u8` | Destination register |
| `b()` | `u8` | Source register or immediate |
| `c()` | `u8` | Source register, immediate, or type encoding |
| `bx()` | `u16` | Table index (constants, callables, globals) |
| `sbx()` | `i16` | Signed branch offset (from B+C) |
| `sbx_ab()` | `i16` | Signed branch offset (from A+B, used by JMP) |

### EXT Prefix

When an operand exceeds the 16-bit range of ABx/AsBx/JMP, the `EXT` opcode (`0xFD`) is emitted as a prefix. The two bytes of the prefix instruction contribute the upper 16 bits:

1. `EXT A=0, B=hi_byte, C=lo_byte` — contributes `(extra as u32) << 16`
2. The base instruction follows with the lower 16 bits

The text format uses `*x` variants: `loadkx`, `closurex`, `jmpx`.

## Opcode Table

### Data Movement

| Mnemonic | Hex | Format | Description |
|----------|-----|--------|-------------|
| `LOADK` | `0x00` | ABx | `reg[A] = constants[bx]` |
| `MOVE` | `0x01` | ABC | `reg[A] = reg[B]` |

### Memory Load

Load typed value from `memory[reg[B] + C]` into `reg[A]`.

| Mnemonic | Hex | Mnemonic | Hex |
|----------|-----|----------|-----|
| `LOAD_I8` | `0x02` | `LOAD_U8` | `0x06` |
| `LOAD_I16` | `0x03` | `LOAD_U16` | `0x07` |
| `LOAD_I32` | `0x04` | `LOAD_U32` | `0x08` |
| `LOAD_I64` | `0x05` | `LOAD_U64` | `0x09` |
| `LOAD_F32` | `0x0A` | `LOAD_F64` | `0x0B` |

Format: ABC (`a`=dest, `b`=base ptr reg, `c`=offset imm).

### Memory Store

Store `reg[C]` at `memory[reg[A] + B]`.

| Mnemonic | Hex | Mnemonic | Hex |
|----------|-----|----------|-----|
| `STORE_I8` | `0x0C` | `STORE_U8` | `0x10` |
| `STORE_I16` | `0x0D` | `STORE_U16` | `0x11` |
| `STORE_I32` | `0x0E` | `STORE_U32` | `0x12` |
| `STORE_I64` | `0x0F` | `STORE_U64` | `0x13` |
| `STORE_F32` | `0x14` | `STORE_F64` | `0x15` |

Format: ABC (`a`=base ptr reg, `b`=offset imm, `c`=value reg).

### Signed Integer Arithmetic

`reg[A] = reg[B] OP reg[C]`. All operations wrap on overflow. Division and modulo check for zero divisor.

| Op | i8 | i16 | i32 | i64 |
|----|-----|-----|-----|-----|
| Add | `ADD_I8 (0x20)` | `ADD_I16 (0x21)` | `ADD_I32 (0x22)` | `ADD_I64 (0x23)` |
| Sub | `SUB_I8 (0x24)` | `SUB_I16 (0x25)` | `SUB_I32 (0x26)` | `SUB_I64 (0x27)` |
| Mul | `MUL_I8 (0x28)` | `MUL_I16 (0x29)` | `MUL_I32 (0x2A)` | `MUL_I64 (0x2B)` |
| Div | `DIV_I8 (0x2C)` | `DIV_I16 (0x2D)` | `DIV_I32 (0x2E)` | `DIV_I64 (0x2F)` |
| Mod | `MOD_I8 (0x30)` | `MOD_I16 (0x31)` | `MOD_I32 (0x32)` | `MOD_I64 (0x33)` |
| Neg | `NEG_I8 (0x34)` | `NEG_I16 (0x35)` | `NEG_I32 (0x36)` | `NEG_I64 (0x37)` |

Neg is unary: `reg[A] = -reg[B]`. C is unused.

### Unsigned Integer Arithmetic

Same semantics as signed. Neg is not available for unsigned types.

| Op | u8 | u16 | u32 | u64 |
|----|-----|-----|-----|-----|
| Add | `ADD_U8 (0x40)` | `ADD_U16 (0x41)` | `ADD_U32 (0x42)` | `ADD_U64 (0x43)` |
| Sub | `SUB_U8 (0x44)` | `SUB_U16 (0x45)` | `SUB_U32 (0x46)` | `SUB_U64 (0x47)` |
| Mul | `MUL_U8 (0x48)` | `MUL_U16 (0x49)` | `MUL_U32 (0x4A)` | `MUL_U64 (0x4B)` |
| Div | `DIV_U8 (0x4C)` | `DIV_U16 (0x4D)` | `DIV_U32 (0x4E)` | `DIV_U64 (0x4F)` |
| Mod | `MOD_U8 (0x50)` | `MOD_U16 (0x51)` | `MOD_U32 (0x52)` | `MOD_U64 (0x53)` |

### Floating-Point Arithmetic

IEEE 754 semantics. Division by zero produces infinity (not an error).

| Op | f32 | f64 |
|----|-----|-----|
| Add | `ADD_F32 (0x58)` | `ADD_F64 (0x59)` |
| Sub | `SUB_F32 (0x5A)` | `SUB_F64 (0x5B)` |
| Mul | `MUL_F32 (0x5C)` | `MUL_F64 (0x5D)` |
| Div | `DIV_F32 (0x5E)` | `DIV_F64 (0x5F)` |
| Neg | `NEG_F32 (0x60)` | `NEG_F64 (0x61)` |

### Bitwise Operations

| Op | i8 | i16 | i32 | i64 |
|----|-----|-----|-----|-----|
| And | `AND_I8 (0x68)` | `AND_I16 (0x69)` | `AND_I32 (0x6A)` | `AND_I64 (0x6B)` |
| Or  | `OR_I8 (0x6C)` | `OR_I16 (0x6D)` | `OR_I32 (0x6E)` | `OR_I64 (0x6F)` |
| Xor | `XOR_I8 (0x70)` | `XOR_I16 (0x71)` | `XOR_I32 (0x72)` | `XOR_I64 (0x73)` |
| Not | `NOT_I8 (0x74)` | `NOT_I16 (0x75)` | `NOT_I32 (0x76)` | `NOT_I64 (0x77)` |

Not is unary: `reg[A] = !reg[B]`. C is unused.

### Shift Operations

| Op | i8 | i16 | i32 | i64 |
|----|-----|-----|-----|-----|
| Shl | `SHL_I8 (0x78)` | `SHL_I16 (0x79)` | `SHL_I32 (0x7A)` | `SHL_I64 (0x7B)` |
| Shr | `SHR_I8 (0x7C)` | `SHR_I16 (0x7D)` | `SHR_I32 (0x7E)` | `SHR_I64 (0x7F)` |
| UShr| `USHR_I8 (0x80)` | `USHR_I16 (0x81)` | `USHR_I32 (0x82)` | `USHR_I64 (0x83)` |

`SHR` is arithmetic shift right (sign-extending). `USHR` is logical shift right (zero-filling). All shift amounts are taken modulo the bit width.

### Signed Integer Comparison

`reg[A] = (reg[B] OP reg[C]) ? 1 : 0`.

| Op | i8 | i16 | i32 | i64 |
|----|-----|-----|-----|-----|
| Eq  | `EQ_I8 (0x90)` | `EQ_I16 (0x91)` | `EQ_I32 (0x92)` | `EQ_I64 (0x93)` |
| Ne  | `NE_I8 (0x94)` | `NE_I16 (0x95)` | `NE_I32 (0x96)` | `NE_I64 (0x97)` |
| Lt  | `LT_I8 (0x98)` | `LT_I16 (0x99)` | `LT_I32 (0x9A)` | `LT_I64 (0x9B)` |
| Le  | `LE_I8 (0x9C)` | `LE_I16 (0x9D)` | `LE_I32 (0x9E)` | `LE_I64 (0x9F)` |
| Gt  | `GT_I8 (0xA0)` | `GT_I16 (0xA1)` | `GT_I32 (0xA2)` | `GT_I64 (0xA3)` |
| Ge  | `GE_I8 (0xA4)` | `GE_I16 (0xA5)` | `GE_I32 (0xA6)` | `GE_I64 (0xA7)` |

### Unsigned Integer Comparison

| Op | u8 | u16 | u32 | u64 |
|----|-----|-----|-----|-----|
| Lt  | `LT_U8 (0xA8)` | `LT_U16 (0xA9)` | `LT_U32 (0xAA)` | `LT_U64 (0xAB)` |
| Le  | `LE_U8 (0xAC)` | `LE_U16 (0xAD)` | `LE_U32 (0xAE)` | `LE_U64 (0xAF)` |
| Gt  | `GT_U8 (0xB0)` | `GT_U16 (0xB1)` | `GT_U32 (0xB2)` | `GT_U64 (0xB3)` |
| Ge  | `GE_U8 (0xB4)` | `GE_U16 (0xB5)` | `GE_U32 (0xB6)` | `GE_U64 (0xB7)` |

### Floating-Point Comparison

| Op | f32 | f64 |
|----|-----|-----|
| Eq  | `EQ_F32 (0xB8)` | `EQ_F64 (0xB9)` |
| Ne  | `NE_F32 (0xBA)` | `NE_F64 (0xBB)` |
| Lt  | `LT_F32 (0xBC)` | `LT_F64 (0xBD)` |
| Le  | `LE_F32 (0xBE)` | `LE_F64 (0xBF)` |
| Gt  | `GT_F32 (0xC0)` | `GT_F64 (0xC1)` |
| Ge  | `GE_F32 (0xC2)` | `GE_F64 (0xC3)` |

### Type Conversion

| Mnemonic | Hex | Format | Description |
|----------|-----|--------|-------------|
| `CONV` | `0xC8` | ABC | Convert `reg[B]` to `reg[A]` using type encoding in `C` |

The `C` operand encodes the source and destination types as `(from_type << 4) | to_type`. Integer and pointer types convert via `i64`; float types convert via `f64`.

### Control Flow

| Mnemonic | Hex | Format | Description |
|----------|-----|--------|-------------|
| `JMP` | `0xD0` | JMP | Unconditional: `PC += sbx_ab()` |
| `JMPIF` | `0xD1` | AsBx | If `reg[A]` is non-zero: `PC += sbx()` |
| `JMPIFNOT` | `0xD2` | AsBx | If `reg[A]` is zero: `PC += sbx()` |

All jump offsets are relative to the instruction following the jump. An offset of `0` is a no-op; an offset of `-1` re-executes the jump instruction.

### Functions

| Mnemonic | Hex | Format | Description |
|----------|-----|--------|-------------|
| `CLOSURE` | `0xD7` | ABx | `reg[A] = callables[bx]` — load a function or native reference |
| `CALL` | `0xD8` | ABC | Call `reg[A]` with `B` args starting at `reg[A+1]`, expect `C` return values |
| `RET` | `0xD9` | ABC | Return with `B` values starting at `reg[A]` |

Calling convention:

- **CALL**: Reads the callable from `reg[A]`. If it is a bytecode function, pushes a new frame and copies arguments into the callee's `reg[0..B-1]`. If it is a native import, dispatches immediately and writes up to `C` return values back to `reg[A..A+C-1]`.
- **RET**: Pops the current frame. Copies up to `expected_returns` values from the callee's `reg[A..A+B-1]` to the caller's `reg[return_base..return_base+expected_returns-1]`. A RET from the entry frame (no `return_base`) triggers `VMError::Halted` — normal termination.
- Args and returns are truncated or zero-filled if counts differ.

### Globals

| Mnemonic | Hex | Format | Description |
|----------|-----|--------|-------------|
| `GETG` | `0xE0` | ABx | `reg[A] = globals[bx]` |
| `SETG` | `0xE1` | ABx | `globals[bx] = reg[A]` — auto-resizes globals vector |

### Special

| Mnemonic | Hex | Description |
|----------|-----|-------------|
| `EXT` | `0xFD` | Extended-arg prefix for `LOADKX`, `CLOSUREX`, `JMPX` |
| `NOP` | `0xFE` | No operation |
| `HALT` | `0xFF` | Terminates execution (returns `VMError::Halted`) |

## Value Types

| Type | Discriminant | Size (bytes) | Binary tag |
|------|-------------|-------------|------------|
| `I8` | 0 | 1 | 0 |
| `I16` | 1 | 2 | 1 |
| `I32` | 2 | 4 | 2 |
| `I64` | 3 | 8 | 3 |
| `U8` | 4 | 1 | 4 |
| `U16` | 5 | 2 | 5 |
| `U32` | 6 | 4 | 6 |
| `U64` | 7 | 8 | 7 |
| `F32` | 8 | 4 | 8 |
| `F64` | 9 | 8 | 9 |
| `Bool` | 10 | 1 | 10 |
| `Ptr` | 11 | 8 | 11 |

## Numerical Semantics

| Class | Behaviour |
|-------|-----------|
| Integer add/sub/mul | Wrapping two's complement |
| Integer div/mod | Wrapping; `VMError::DivisionByZero` on zero divisor |
| Integer neg | Wrapping negation |
| Float arithmetic | IEEE 754; division by zero produces infinity |
| Bitwise | Standard bitwise operations |
| Shift | Shift amount modulo bit width; `SHR` is arithmetic, `USHR` is logical |
| Comparison | Integer: direct. Float: IEEE ordered comparison |
| Memory load/store | Bounds-checked; `VMError::MemoryOutOfBounds` on overflow |
