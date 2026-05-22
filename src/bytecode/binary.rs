use crate::bytecode::{
    Callable, Chunk, Constant, Function, Instruction, Module, Opcode,
};
use crate::vm::native::ImportDecl;

const MAGIC: &[u8; 4] = b"EMB\0";
const VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinaryError {
    InvalidMagic,
    UnsupportedVersion(u16),
    UnexpectedEof,
    InvalidUtf8,
    InvalidOpcode(u8),
    InvalidConstantTag(u8),
    InvalidCallableTag(u8),
    InvalidImportTag(u8),
    CountTooLarge,
}

pub fn encode_module(module: &Module) -> Result<Vec<u8>, BinaryError> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    write_u16(&mut out, VERSION);
    write_string(&mut out, &module.name)?;
    write_u32(&mut out, module.entry)?;

    write_u32(&mut out, module.imports.len() as u32)?;
    for import in &module.imports {
        out.push(if import.is_native() { 0 } else { 1 });
        write_string(&mut out, import.module_name())?;
        write_string(&mut out, import.function_name())?;
    }

    write_u32(&mut out, module.constants.len() as u32)?;
    for constant in &module.constants {
        encode_constant(&mut out, constant)?;
    }

    write_u32(&mut out, module.callables.len() as u32)?;
    for callable in &module.callables {
        match callable {
            Callable::Function(id) => {
                out.push(0);
                write_u32(&mut out, *id)?;
            }
            Callable::Import(id) => {
                out.push(1);
                write_u32(&mut out, *id)?;
            }
        }
    }

    write_u32(&mut out, module.functions.len() as u32)?;
    for function in &module.functions {
        write_string(&mut out, &function.name)?;
        encode_chunk(&mut out, &function.chunk)?;
    }
    Ok(out)
}

pub fn decode_module(bytes: &[u8]) -> Result<Module, BinaryError> {
    let mut reader = Reader { bytes, offset: 0 };
    if reader.read_exact(4)? != MAGIC {
        return Err(BinaryError::InvalidMagic);
    }
    let version = reader.read_u16()?;
    if version != VERSION {
        return Err(BinaryError::UnsupportedVersion(version));
    }

    let mut module = Module::new(reader.read_string()?);
    module.version = version;
    module.entry = reader.read_u32()?;

    let import_count = reader.read_u32()? as usize;
    for _ in 0..import_count {
        let tag = reader.read_u8()?;
        match tag {
            0 => {
                module.imports.push(ImportDecl::native(
                    reader.read_string()?,
                    reader.read_string()?,
                ));
            }
            1 => {
                module.imports.push(ImportDecl::external(
                    reader.read_string()?,
                    reader.read_string()?,
                ));
            }
            tag => return Err(BinaryError::InvalidImportTag(tag)),
        }
    }

    let constant_count = reader.read_u32()? as usize;
    for _ in 0..constant_count {
        module.constants.push(decode_constant(&mut reader)?);
    }

    let callable_count = reader.read_u32()? as usize;
    for _ in 0..callable_count {
        let tag = reader.read_u8()?;
        module.callables.push(match tag {
            0 => Callable::Function(reader.read_u32()?),
            1 => Callable::Import(reader.read_u32()?),
            tag => return Err(BinaryError::InvalidCallableTag(tag)),
        });
    }

    let function_count = reader.read_u32()? as usize;
    for _ in 0..function_count {
        let name = reader.read_string()?;
        let chunk = decode_chunk(&mut reader)?;
        module.functions.push(Function { name, chunk });
    }
    Ok(module)
}

fn encode_chunk(out: &mut Vec<u8>, chunk: &Chunk) -> Result<(), BinaryError> {
    out.push(chunk.max_registers);
    write_u32(out, chunk.code.len() as u32)?;
    for instr in &chunk.code {
        out.push(instr.opcode().to_byte());
        out.push(instr.a());
        out.push(instr.b());
        out.push(instr.c());
    }
    Ok(())
}

fn decode_chunk(reader: &mut Reader<'_>) -> Result<Chunk, BinaryError> {
    let mut chunk = Chunk::new();
    chunk.max_registers = reader.read_u8()?;
    let code_count = reader.read_u32()? as usize;
    for _ in 0..code_count {
        let opcode_byte = reader.read_u8()?;
        let opcode =
            Opcode::from_byte(opcode_byte).ok_or(BinaryError::InvalidOpcode(opcode_byte))?;
        let a = reader.read_u8()?;
        let b = reader.read_u8()?;
        let c = reader.read_u8()?;
        chunk.code.push(Instruction::new(opcode, [a, b, c]));
    }
    Ok(chunk)
}

fn encode_constant(out: &mut Vec<u8>, constant: &Constant) -> Result<(), BinaryError> {
    match constant {
        Constant::I8(v) => {
            out.push(0);
            out.push(*v as u8);
        }
        Constant::I16(v) => {
            out.push(1);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Constant::I32(v) => {
            out.push(2);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Constant::I64(v) => {
            out.push(3);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Constant::U8(v) => {
            out.push(4);
            out.push(*v);
        }
        Constant::U16(v) => {
            out.push(5);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Constant::U32(v) => {
            out.push(6);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Constant::U64(v) => {
            out.push(7);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Constant::F32(v) => {
            out.push(8);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Constant::F64(v) => {
            out.push(9);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Constant::Bool(v) => {
            out.push(10);
            out.push(*v as u8);
        }
        Constant::Bytes(v) => {
            out.push(11);
            write_bytes(out, v)?;
        }
    }
    Ok(())
}

fn decode_constant(reader: &mut Reader<'_>) -> Result<Constant, BinaryError> {
    Ok(match reader.read_u8()? {
        0 => Constant::I8(reader.read_u8()? as i8),
        1 => Constant::I16(i16::from_le_bytes(reader.read_array()?)),
        2 => Constant::I32(i32::from_le_bytes(reader.read_array()?)),
        3 => Constant::I64(i64::from_le_bytes(reader.read_array()?)),
        4 => Constant::U8(reader.read_u8()?),
        5 => Constant::U16(u16::from_le_bytes(reader.read_array()?)),
        6 => Constant::U32(u32::from_le_bytes(reader.read_array()?)),
        7 => Constant::U64(u64::from_le_bytes(reader.read_array()?)),
        8 => Constant::F32(f32::from_le_bytes(reader.read_array()?)),
        9 => Constant::F64(f64::from_le_bytes(reader.read_array()?)),
        10 => Constant::Bool(reader.read_u8()? != 0),
        11 => Constant::Bytes(reader.read_bytes()?),
        tag => return Err(BinaryError::InvalidConstantTag(tag)),
    })
}

fn write_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_u32(out: &mut Vec<u8>, value: u32) -> Result<(), BinaryError> {
    out.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_string(out: &mut Vec<u8>, value: &str) -> Result<(), BinaryError> {
    let len = u32::try_from(value.len()).map_err(|_| BinaryError::CountTooLarge)?;
    write_u32(out, len)?;
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn write_bytes(out: &mut Vec<u8>, value: &[u8]) -> Result<(), BinaryError> {
    let len = u32::try_from(value.len()).map_err(|_| BinaryError::CountTooLarge)?;
    write_u32(out, len)?;
    out.extend_from_slice(value);
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn read_exact(&mut self, len: usize) -> Result<&'a [u8], BinaryError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or(BinaryError::UnexpectedEof)?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or(BinaryError::UnexpectedEof)?;
        self.offset = end;
        Ok(bytes)
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], BinaryError> {
        self.read_exact(N)?
            .try_into()
            .map_err(|_| BinaryError::UnexpectedEof)
    }

    fn read_u8(&mut self) -> Result<u8, BinaryError> {
        Ok(self.read_exact(1)?[0])
    }

    fn read_u16(&mut self) -> Result<u16, BinaryError> {
        Ok(u16::from_le_bytes(self.read_array()?))
    }

    fn read_u32(&mut self) -> Result<u32, BinaryError> {
        Ok(u32::from_le_bytes(self.read_array()?))
    }

    fn read_string(&mut self) -> Result<String, BinaryError> {
        let len = self.read_u32()? as usize;
        let bytes = self.read_exact(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| BinaryError::InvalidUtf8)
    }

    fn read_bytes(&mut self) -> Result<Vec<u8>, BinaryError> {
        let len = self.read_u32()? as usize;
        Ok(self.read_exact(len)?.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::text::parse_module;

    #[test]
    fn round_trips_module() {
        let module = parse_module(
            r#"
            .module "hello"
            .version 1
            .entry 0
            .import
              io.print_i64
            .constants
              0 i64 42
            .callables
              0 io.print_i64
            .functions
              0 "main" regs=2
                closure r0, 0
                loadk r1, 0
                call r0, 1, 0
                halt
              end
            "#,
        )
        .unwrap();
        let encoded = encode_module(&module).unwrap();
        let decoded = decode_module(&encoded).unwrap();
        assert_eq!(decoded.name, "hello");
        assert_eq!(decoded.functions[0].chunk.code.len(), 4);
    }
}
