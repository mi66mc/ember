use std::rc::Rc;

use ember::{Chunk, Constant, Instruction, Opcode, Vm};

// ═══════════════════════════════════════════════════════════════════════════
// bubble sort in ember bytecode
// ═══════════════════════════════════════════════════════════════════════════
//
// demonstrates:
//   - memory operations (array storage)
//   - nested loops
//   - comparisons and conditional jumps
//   - function calls (swap routine)
//
// algorithm:
//   for i in 0..n-1:
//     for j in 0..n-1-i:
//       if arr[j] > arr[j+1]:
//         swap(arr[j], arr[j+1])
//
// memory layout:
//   [0..8*n): array of i64 values
//
// register allocation (main):
//   R0 = n (array length)
//   R1 = i (outer loop counter)
//   R2 = j (inner loop counter)
//   R3 = limit (n - 1 - i)
//   R4 = addr_j (address of arr[j])
//   R5 = addr_j1 (address of arr[j+1])
//   R6 = val_j (value at arr[j])
//   R7 = val_j1 (value at arr[j+1])
//   R8 = cmp result
//   R9 = temp / constants
//
fn main() {
    let example = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "bubble-sort".to_string());
    if example != "bubble-sort" {
        eprintln!("unknown example: {example}");
        eprintln!("available examples: bubble-sort");
        std::process::exit(2);
    }

    let array = [64, 34, 25, 12, 22, 11, 90, 42, 7, 55];
    let n = array.len();

    println!("Ember VM bytecode example: bubble-sort\n");
    println!("input:  {:?}\n", array);

    let chunk = build_bubble_sort(n);
    let mut vm = Vm::new(n * 8 + 64);

    // load array into memory
    for (i, &val) in array.iter().enumerate() {
        unsafe { vm.memory.write::<i64>(i * 8, val) };
    }

    // run
    vm.stack.push_entry(Rc::new(chunk));

    let mut steps = 0;
    while vm.step().is_ok() {
        steps += 1;
    }

    // read sorted array from memory
    let mut result = Vec::new();
    for i in 0..n {
        result.push(unsafe { vm.memory.read::<i64>(i * 8) });
    }

    println!("output: {:?}\n", result);
    println!("stats:");
    println!("  instructions executed: {}", steps);
    println!("  array size: {}", n);
    let is_sorted = result.windows(2).all(|w| w[0] <= w[1]);
    println!("  correctly sorted: {}", is_sorted);
}

fn build_bubble_sort(n: usize) -> Chunk {
    let swap_fn = build_swap_function();

    let mut chunk = Chunk::new();
    chunk.max_registers = 12;

    let swap_proto = chunk.add_proto(swap_fn);

    let cn = chunk.add_constant(Constant::I64(n as i64));
    let c0 = chunk.add_constant(Constant::I64(0));
    let c1 = chunk.add_constant(Constant::I64(1));
    let c8 = chunk.add_constant(Constant::I64(8));

    // emit returns current index
    let e = |chunk: &mut Chunk, instr: Instruction| -> usize { chunk.emit(instr) };

    // ─── init ───
    e(&mut chunk, Instruction::abx(Opcode::LOADK, 0, cn)); // 0: R0 = n
    e(&mut chunk, Instruction::abx(Opcode::LOADK, 1, c0)); // 1: R1 = i = 0

    // ─── outer loop (starts at 2) ───
    let outer_loop = 2;
    e(&mut chunk, Instruction::abx(Opcode::LOADK, 9, c1)); // 2: R9 = 1
    e(&mut chunk, Instruction::abc(Opcode::SUB_I64, 9, 0, 9)); // 3: R9 = n - 1
    e(&mut chunk, Instruction::abc(Opcode::LT_I64, 8, 1, 9)); // 4: R8 = (i < n-1)
    let outer_exit_jmp = e(&mut chunk, Instruction::asbx(Opcode::JMPIFNOT, 8, 0)); // 5: patch later

    e(&mut chunk, Instruction::abc(Opcode::SUB_I64, 3, 9, 1)); // 6: R3 = limit = n-1-i
    e(&mut chunk, Instruction::abx(Opcode::LOADK, 2, c0)); // 7: R2 = j = 0

    // ─── inner loop (starts at 8) ───
    let inner_loop = 8;
    e(&mut chunk, Instruction::abc(Opcode::LT_I64, 8, 2, 3)); // 8: R8 = (j < limit)
    let inner_exit_jmp = e(&mut chunk, Instruction::asbx(Opcode::JMPIFNOT, 8, 0)); // 9: patch later

    // compute addresses
    e(&mut chunk, Instruction::abx(Opcode::LOADK, 9, c8)); // 10: R9 = 8
    e(&mut chunk, Instruction::abc(Opcode::MUL_I64, 4, 2, 9)); // 11: R4 = j * 8
    e(&mut chunk, Instruction::abx(Opcode::LOADK, 9, c1)); // 12: R9 = 1
    e(&mut chunk, Instruction::abc(Opcode::ADD_I64, 9, 2, 9)); // 13: R9 = j + 1
    e(&mut chunk, Instruction::abx(Opcode::LOADK, 10, c8)); // 14: R10 = 8
    e(&mut chunk, Instruction::abc(Opcode::MUL_I64, 5, 9, 10)); // 15: R5 = (j+1) * 8

    // load values
    e(&mut chunk, Instruction::abc(Opcode::LOAD_I64, 6, 4, 0)); // 16: R6 = arr[j]
    e(&mut chunk, Instruction::abc(Opcode::LOAD_I64, 7, 5, 0)); // 17: R7 = arr[j+1]

    // compare
    e(&mut chunk, Instruction::abc(Opcode::GT_I64, 8, 6, 7)); // 18: R8 = (arr[j] > arr[j+1])
    let skip_swap_jmp = e(&mut chunk, Instruction::asbx(Opcode::JMPIFNOT, 8, 0)); // 19: patch later

    // call swap(addr_j, addr_j1)
    e(&mut chunk, Instruction::abx(Opcode::CLOSURE, 9, swap_proto)); // 20
    e(&mut chunk, Instruction::abc(Opcode::MOVE, 10, 4, 0)); // 21
    e(&mut chunk, Instruction::abc(Opcode::MOVE, 11, 5, 0)); // 22
    e(&mut chunk, Instruction::abc(Opcode::CALL, 9, 2, 0)); // 23

    // j++ (inner_continue = 24)
    let inner_continue = 24;
    e(&mut chunk, Instruction::abx(Opcode::LOADK, 9, c1)); // 24: R9 = 1
    e(&mut chunk, Instruction::abc(Opcode::ADD_I64, 2, 2, 9)); // 25: j = j + 1

    // jump to inner_loop: from 26, target 8 -> offset = 8 - 26 = -18
    e(
        &mut chunk,
        Instruction::jmp(Opcode::JMP, (inner_loop as i16) - 26),
    ); // 26

    // ─── outer continue (27) ───
    let outer_continue = 27;
    e(&mut chunk, Instruction::abx(Opcode::LOADK, 9, c1)); // 27: R9 = 1
    e(&mut chunk, Instruction::abc(Opcode::ADD_I64, 1, 1, 9)); // 28: i = i + 1

    // jump to outer_loop: from 29, target 2 -> offset = 2 - 29 = -27
    e(
        &mut chunk,
        Instruction::jmp(Opcode::JMP, (outer_loop as i16) - 29),
    ); // 29

    // ─── end (30) ───
    let end = 30;
    e(&mut chunk, Instruction::abc(Opcode::HALT, 0, 0, 0)); // 30

    // ─── patch jumps ───
    // outer_exit_jmp (5): jump to end (30) -> offset = 30 - 5 = 25
    chunk.code[outer_exit_jmp] =
        Instruction::asbx(Opcode::JMPIFNOT, 8, (end - outer_exit_jmp) as i16);

    // inner_exit_jmp (9): jump to outer_continue (27) -> offset = 27 - 9 = 18
    chunk.code[inner_exit_jmp] = Instruction::asbx(
        Opcode::JMPIFNOT,
        8,
        (outer_continue - inner_exit_jmp) as i16,
    );

    // skip_swap_jmp (19): jump to inner_continue (24) -> offset = 24 - 19 = 5
    chunk.code[skip_swap_jmp] =
        Instruction::asbx(Opcode::JMPIFNOT, 8, (inner_continue - skip_swap_jmp) as i16);

    chunk
}

fn build_swap_function() -> Chunk {
    // swap(addr1, addr2)
    // R0 = addr1
    // R1 = addr2
    // R2 = mem[addr1]
    // R3 = mem[addr2]
    // mem[addr1] = R3
    // mem[addr2] = R2

    let mut chunk = Chunk::new();
    chunk.max_registers = 4;

    // R2 = mem[R0] (val1)
    chunk.emit(Instruction::abc(Opcode::LOAD_I64, 2, 0, 0)); // 0

    // R3 = mem[R1] (val2)
    chunk.emit(Instruction::abc(Opcode::LOAD_I64, 3, 1, 0)); // 1

    // mem[R0] = R3
    chunk.emit(Instruction::abc(Opcode::STORE_I64, 0, 0, 3)); // 2

    // mem[R1] = R2
    chunk.emit(Instruction::abc(Opcode::STORE_I64, 1, 0, 2)); // 3

    // return
    chunk.emit(Instruction::abc(Opcode::RET, 0, 0, 0)); // 4

    chunk
}
