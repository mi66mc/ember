use ember::common::types::{instr::Instruction, opcode::Opcode};

fn main() {
    println!("{:?}", Instruction::new(Opcode::HALT, [0, 0, 0]));
}
