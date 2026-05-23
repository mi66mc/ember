# Benchmark: Ember VM vs CPython

Iterative `fib(30)` computed 10,000 times in a single process.  
All tests run on the same machine (Windows, PowerShell `Measure-Command`).

## Setup

| Runtime | Command |
|---------|---------|
| Ember `.embt` (parse + run) | `ember run bench/bench.embt` |
| Ember `.emb` (decode + run) | `ember build bench/bench.embt -o bench/bench.emb && ember run bench/bench.emb` |
| CPython 3.x | `python bench/bench.py` |

## Algorithm

Both implementations use the same iterative algorithm:

```
a, b = 0, 1
for i in range(30):
    a, b = b, a + b
return a
```

No recursion, no tail calls, no function calls inside the hot loop.

## Results

| Runtime | Time (10k iterations) |
|---------|----------------------|
| Ember `.embt` (parse + run) | ~58ms |
| Ember `.emb` (decode + run) | ~51ms |
| CPython 3.x | ~54ms |

Ember compiled to `.emb` is slightly faster than CPython on this pure arithmetic loop. The `.embt` parser overhead is ~7ms.

## Bytecode

The Ember program for this benchmark (`bench/bench.embt`):

```embt
.module "bench"
.entry 0

.import
  io.print_i64

.constants
  0 i64 30
  1 i64 0
  2 i64 1
  3 i64 1
  4 i64 10000

.callables
  0 io.print_i64

.functions
  0 "main" regs=6
    loadk r5, 4            ;; r5 = 10000

    @outer:
    loadk r0, 0            ;; r0 = 30
    loadk r1, 1            ;; r1 = 0
    loadk r2, 2            ;; r2 = 1
    loadk r3, 3            ;; r3 = 1

    @inner:
    jmpifnot r0, @done
    move r4, r1            ;; temp = a
    move r1, r2            ;; a = b
    add.i64 r2, r4, r2     ;; b = temp + b
    sub.i64 r0, r0, r3     ;; n--
    jmp @inner

    @done:
    sub.i64 r5, r5, r3     ;; iterations--
    jmpif r5, @outer

    closure r0, 0, 0       ;; print_i64
    move r2, r1
    call r0, 1, 0          ;; print fib(30)
    halt
  end
```

## Notes

- Ember is a pure interpreter with a `match`-based opcode dispatch. No JIT, no threaded code, no superinstructions.
- The benchmark measures compute only — I/O runs once at the end, outside the timed loop.
- CPython 3.x was used as the reference. PyPy or LuaJIT would likely be faster than both.
- Each `.embt` → `.emb` compilation avoids the text parse cost on subsequent runs.
