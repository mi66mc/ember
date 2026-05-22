pub mod bytecode;
pub mod vm;

pub use bytecode::{
    Builder, Callable, Chunk, Constant, Function, Instruction, Module, Opcode, SourceLocation,
    ValueType,
};
pub use vm::native::{ImportDecl, ImportIndex, NativeError, NativeLinker, NativeModule, NativeResult};
pub use vm::{Memory, Register, VMError, Vm, VmValue};
