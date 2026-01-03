#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum Opcode {
    LOADK,
    MOVE,
    ADD,
    SUB,
    MUL,
    DIV,
    GETG,
    SETG,
    CALL,
    RET,
    HALT,
}
