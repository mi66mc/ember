use crate::bytecode::{
    Callable, Chunk, Constant, Function, Instruction, Module, NativeImport, Opcode,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl TextError {
    fn new(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            column,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for TextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for TextError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    None,
    Natives,
    Constants,
    Callables,
    Functions,
}

pub fn parse_module(source: &str) -> Result<Module, TextError> {
    let mut module = Module::new("main");
    let mut section = Section::None;
    let mut current_function: Option<Function> = None;

    for (idx, raw) in source.lines().enumerate() {
        let line_no = idx + 1;
        let line = strip_comment(raw).trim().to_string();
        if line.is_empty() {
            continue;
        }

        if let Some(function) = current_function.as_mut() {
            if line == "end" {
                module
                    .functions
                    .push(current_function.take().expect("function exists"));
            } else {
                parse_instruction_line(&line, line_no, &mut function.chunk)?;
            }
            continue;
        }

        match line.as_str() {
            ".natives" => section = Section::Natives,
            ".constants" => section = Section::Constants,
            ".callables" => section = Section::Callables,
            ".functions" => section = Section::Functions,
            _ if line.starts_with(".module ") => {
                module.name = parse_quoted(line[".module ".len()..].trim(), line_no)?;
            }
            _ if line.starts_with(".version ") => {
                module.version = line[".version ".len()..]
                    .trim()
                    .parse()
                    .map_err(|_| TextError::new(line_no, 1, "invalid module version"))?;
            }
            _ if line.starts_with(".entry ") => {
                module.entry = line[".entry ".len()..]
                    .trim()
                    .parse()
                    .map_err(|_| TextError::new(line_no, 1, "invalid entry function id"))?;
            }
            _ => match section {
                Section::Natives => parse_native(&line, line_no, &mut module)?,
                Section::Constants => parse_constant_entry(&line, line_no, &mut module)?,
                Section::Callables => parse_callable(&line, line_no, &mut module)?,
                Section::Functions => {
                    current_function = Some(parse_function_header(&line, line_no)?);
                }
                Section::None => {
                    return Err(TextError::new(
                        line_no,
                        1,
                        "expected module directive or section",
                    ));
                }
            },
        }
    }

    if current_function.is_some() {
        return Err(TextError::new(
            source.lines().count(),
            1,
            "unterminated function",
        ));
    }
    validate_module(&module).map_err(|message| TextError::new(1, 1, message))?;
    Ok(module)
}

pub fn validate_module(module: &Module) -> Result<(), String> {
    if module.entry as usize >= module.functions.len() {
        return Err(format!("entry function {} does not exist", module.entry));
    }
    for (idx, callable) in module.callables.iter().enumerate() {
        match callable {
            Callable::Function(function_id) if *function_id as usize >= module.functions.len() => {
                return Err(format!(
                    "callable {idx} references missing function {function_id}"
                ));
            }
            Callable::Native(native_id) if *native_id as usize >= module.natives.len() => {
                return Err(format!(
                    "callable {idx} references missing native {native_id}"
                ));
            }
            _ => {}
        }
    }
    for (function_id, function) in module.functions.iter().enumerate() {
        for (pc, instr) in function.chunk.code.iter().enumerate() {
            validate_instruction(module, function_id, function.chunk.max_registers, pc, instr)?;
        }
    }
    Ok(())
}

pub fn module_to_text(module: &Module) -> String {
    let mut out = String::new();
    out.push_str(&format!(".module \"{}\"\n", escape_string(&module.name)));
    out.push_str(&format!(".version {}\n", module.version));
    out.push_str(&format!(".entry {}\n\n", module.entry));

    out.push_str(".natives\n");
    for (idx, native) in module.natives.iter().enumerate() {
        out.push_str(&format!("  {} \"{}\"\n", idx, escape_string(&native.name)));
    }

    out.push_str("\n.constants\n");
    for (idx, constant) in module.constants.iter().enumerate() {
        out.push_str(&format!("  {} {}\n", idx, format_constant(constant)));
    }

    out.push_str("\n.callables\n");
    for (idx, callable) in module.callables.iter().enumerate() {
        let rendered = match callable {
            Callable::Function(id) => format!("function {id}"),
            Callable::Native(id) => format!("native {id}"),
        };
        out.push_str(&format!("  {} {}\n", idx, rendered));
    }

    out.push_str("\n.functions\n");
    for (idx, function) in module.functions.iter().enumerate() {
        out.push_str(&format!(
            "  {} \"{}\" regs={}\n",
            idx,
            escape_string(&function.name),
            function.chunk.max_registers
        ));
        for (pc, instr) in function.chunk.code.iter().enumerate() {
            out.push_str(&format!("    {:04} {}\n", pc, format_instruction(instr)));
        }
        out.push_str("  end\n");
    }
    out
}

pub fn format_instruction(instr: &Instruction) -> String {
    match instr.opcode() {
        Opcode::LOADK => format!("loadk r{}, {}", instr.a(), instr.bx()),
        Opcode::MOVE => format!("move r{}, r{}, r{}", instr.a(), instr.b(), instr.c()),
        Opcode::LOAD_I64 => format!("load.i64 r{}, r{}, {}", instr.a(), instr.b(), instr.c()),
        Opcode::STORE_I64 => format!("store.i64 r{}, {}, r{}", instr.a(), instr.b(), instr.c()),
        Opcode::ADD_I64 => format!("add.i64 r{}, r{}, r{}", instr.a(), instr.b(), instr.c()),
        Opcode::SUB_I64 => format!("sub.i64 r{}, r{}, r{}", instr.a(), instr.b(), instr.c()),
        Opcode::MUL_I64 => format!("mul.i64 r{}, r{}, r{}", instr.a(), instr.b(), instr.c()),
        Opcode::DIV_I64 => format!("div.i64 r{}, r{}, r{}", instr.a(), instr.b(), instr.c()),
        Opcode::LT_I64 => format!("lt.i64 r{}, r{}, r{}", instr.a(), instr.b(), instr.c()),
        Opcode::GT_I64 => format!("gt.i64 r{}, r{}, r{}", instr.a(), instr.b(), instr.c()),
        Opcode::JMP => format!("jmp {}", instr.sbx_ab()),
        Opcode::JMPIF => format!("jmpif r{}, {}", instr.a(), instr.sbx()),
        Opcode::JMPIFNOT => format!("jmpifnot r{}, {}", instr.a(), instr.sbx()),
        Opcode::CLOSURE => format!("closure r{}, {}", instr.a(), instr.bx()),
        Opcode::CALL => format!("call r{}, {}, {}", instr.a(), instr.b(), instr.c()),
        Opcode::RET => format!("ret r{}, {}", instr.a(), instr.b()),
        Opcode::NOP => "nop".to_string(),
        Opcode::HALT => "halt".to_string(),
        op => format!("{:?} {}, {}, {}", op, instr.a(), instr.b(), instr.c()),
    }
}

fn validate_instruction(
    module: &Module,
    function_id: usize,
    regs: u8,
    pc: usize,
    instr: &Instruction,
) -> Result<(), String> {
    let reg_count = regs as usize;
    let check_reg = |reg: u8| -> Result<(), String> {
        if reg as usize >= reg_count {
            Err(format!(
                "function {function_id} instruction {pc} uses invalid register r{reg}"
            ))
        } else {
            Ok(())
        }
    };

    match instr.opcode() {
        Opcode::LOADK => {
            check_reg(instr.a())?;
            if instr.bx() as usize >= module.constants.len() {
                return Err(format!(
                    "function {function_id} instruction {pc} references missing constant {}",
                    instr.bx()
                ));
            }
        }
        Opcode::CLOSURE => {
            check_reg(instr.a())?;
            if instr.bx() as usize >= module.callables.len() {
                return Err(format!(
                    "function {function_id} instruction {pc} references missing callable {}",
                    instr.bx()
                ));
            }
        }
        Opcode::CALL => {
            check_reg(instr.a())?;
            let last = instr.a() as usize + instr.b() as usize;
            if last >= reg_count {
                return Err(format!(
                    "function {function_id} instruction {pc} call args exceed register frame"
                ));
            }
        }
        Opcode::RET => {
            check_reg(instr.a())?;
            if instr.b() > 0 && instr.a() as usize + instr.b() as usize > reg_count {
                return Err(format!(
                    "function {function_id} instruction {pc} return values exceed register frame"
                ));
            }
        }
        Opcode::HALT | Opcode::NOP | Opcode::JMP => {}
        Opcode::JMPIF | Opcode::JMPIFNOT => check_reg(instr.a())?,
        Opcode::LOAD_I8
        | Opcode::LOAD_I16
        | Opcode::LOAD_I32
        | Opcode::LOAD_I64
        | Opcode::LOAD_U8
        | Opcode::LOAD_U16
        | Opcode::LOAD_U32
        | Opcode::LOAD_U64
        | Opcode::LOAD_F32
        | Opcode::LOAD_F64 => {
            check_reg(instr.a())?;
            check_reg(instr.b())?;
        }
        Opcode::STORE_I8
        | Opcode::STORE_I16
        | Opcode::STORE_I32
        | Opcode::STORE_I64
        | Opcode::STORE_U8
        | Opcode::STORE_U16
        | Opcode::STORE_U32
        | Opcode::STORE_U64
        | Opcode::STORE_F32
        | Opcode::STORE_F64 => {
            check_reg(instr.a())?;
            check_reg(instr.c())?;
        }
        _ => {
            check_reg(instr.a())?;
            check_reg(instr.b())?;
            check_reg(instr.c())?;
        }
    }
    Ok(())
}

fn parse_native(line: &str, line_no: usize, module: &mut Module) -> Result<(), TextError> {
    let tokens = tokenize(line, line_no)?;
    expect_len(&tokens, 2, line_no)?;
    let idx = parse_usize(&tokens[0], line_no)?;
    ensure_next_index(idx, module.natives.len(), line_no)?;
    module.natives.push(NativeImport {
        name: parse_quoted(&tokens[1], line_no)?,
    });
    Ok(())
}

fn parse_constant_entry(line: &str, line_no: usize, module: &mut Module) -> Result<(), TextError> {
    let tokens = tokenize(line, line_no)?;
    if tokens.len() < 3 {
        return Err(TextError::new(line_no, 1, "expected `index type value`"));
    }
    let idx = parse_usize(&tokens[0], line_no)?;
    ensure_next_index(idx, module.constants.len(), line_no)?;
    module
        .constants
        .push(parse_typed_constant(&tokens[1], &tokens[2], line_no)?);
    Ok(())
}

fn parse_callable(line: &str, line_no: usize, module: &mut Module) -> Result<(), TextError> {
    let tokens = tokenize(line, line_no)?;
    expect_len(&tokens, 3, line_no)?;
    let idx = parse_usize(&tokens[0], line_no)?;
    ensure_next_index(idx, module.callables.len(), line_no)?;
    let target = parse_u32(&tokens[2], line_no)?;
    let callable = match tokens[1].as_str() {
        "function" => Callable::Function(target),
        "native" => Callable::Native(target),
        _ => {
            return Err(TextError::new(
                line_no,
                1,
                "expected callable kind function|native",
            ));
        }
    };
    module.callables.push(callable);
    Ok(())
}

fn parse_function_header(line: &str, line_no: usize) -> Result<Function, TextError> {
    let tokens = tokenize(line, line_no)?;
    expect_len(&tokens, 3, line_no)?;
    parse_usize(&tokens[0], line_no)?;
    let name = parse_quoted(&tokens[1], line_no)?;
    let regs = tokens[2]
        .strip_prefix("regs=")
        .ok_or_else(|| TextError::new(line_no, 1, "expected regs=N"))?
        .parse()
        .map_err(|_| TextError::new(line_no, 1, "invalid register count"))?;
    let mut chunk = Chunk::new();
    chunk.max_registers = regs;
    Ok(Function { name, chunk })
}

fn parse_instruction_line(line: &str, line_no: usize, chunk: &mut Chunk) -> Result<(), TextError> {
    let mut tokens = tokenize(line, line_no)?;
    if tokens.is_empty() {
        return Ok(());
    }
    if tokens[0].chars().all(|ch| ch.is_ascii_digit()) {
        tokens.remove(0);
    }
    let instr = parse_instruction(&tokens, line_no)?;
    chunk.emit(instr);
    Ok(())
}

fn parse_instruction(tokens: &[String], line_no: usize) -> Result<Instruction, TextError> {
    let op = tokens
        .first()
        .ok_or_else(|| TextError::new(line_no, 1, "empty instruction"))?
        .as_str();
    match op {
        "halt" => Ok(Instruction::abc(Opcode::HALT, 0, 0, 0)),
        "nop" => Ok(Instruction::abc(Opcode::NOP, 0, 0, 0)),
        "loadk" => {
            expect_len(tokens, 3, line_no)?;
            Ok(Instruction::abx(
                Opcode::LOADK,
                parse_reg(&tokens[1], line_no)?,
                parse_u16(&tokens[2], line_no)?,
            ))
        }
        "closure" => {
            expect_len(tokens, 3, line_no)?;
            Ok(Instruction::abx(
                Opcode::CLOSURE,
                parse_reg(&tokens[1], line_no)?,
                parse_u16(&tokens[2], line_no)?,
            ))
        }
        "call" => {
            expect_len(tokens, 4, line_no)?;
            Ok(Instruction::abc(
                Opcode::CALL,
                parse_reg(&tokens[1], line_no)?,
                parse_u8(&tokens[2], line_no)?,
                parse_u8(&tokens[3], line_no)?,
            ))
        }
        "ret" => {
            expect_len(tokens, 3, line_no)?;
            Ok(Instruction::abc(
                Opcode::RET,
                parse_reg(&tokens[1], line_no)?,
                parse_u8(&tokens[2], line_no)?,
                0,
            ))
        }
        "jmp" => {
            expect_len(tokens, 2, line_no)?;
            Ok(Instruction::jmp(
                Opcode::JMP,
                parse_i16(&tokens[1], line_no)?,
            ))
        }
        "jmpif" | "jmpifnot" => {
            expect_len(tokens, 3, line_no)?;
            Ok(Instruction::asbx(
                if op == "jmpif" {
                    Opcode::JMPIF
                } else {
                    Opcode::JMPIFNOT
                },
                parse_reg(&tokens[1], line_no)?,
                parse_i16(&tokens[2], line_no)?,
            ))
        }
        "move" => abc(tokens, line_no, Opcode::MOVE),
        "load.i64" => abc(tokens, line_no, Opcode::LOAD_I64),
        "store.i64" => abc_reg_imm_reg(tokens, line_no, Opcode::STORE_I64),
        "add.i64" => abc(tokens, line_no, Opcode::ADD_I64),
        "sub.i64" => abc(tokens, line_no, Opcode::SUB_I64),
        "mul.i64" => abc(tokens, line_no, Opcode::MUL_I64),
        "div.i64" => abc(tokens, line_no, Opcode::DIV_I64),
        "lt.i64" => abc(tokens, line_no, Opcode::LT_I64),
        "gt.i64" => abc(tokens, line_no, Opcode::GT_I64),
        _ => Err(TextError::new(
            line_no,
            1,
            format!("unknown instruction `{op}`"),
        )),
    }
}

fn abc(tokens: &[String], line_no: usize, opcode: Opcode) -> Result<Instruction, TextError> {
    expect_len(tokens, 4, line_no)?;
    Ok(Instruction::abc(
        opcode,
        parse_reg(&tokens[1], line_no)?,
        parse_reg(&tokens[2], line_no)?,
        parse_reg(&tokens[3], line_no)?,
    ))
}

fn abc_reg_imm_reg(
    tokens: &[String],
    line_no: usize,
    opcode: Opcode,
) -> Result<Instruction, TextError> {
    expect_len(tokens, 4, line_no)?;
    Ok(Instruction::abc(
        opcode,
        parse_reg(&tokens[1], line_no)?,
        parse_u8(&tokens[2], line_no)?,
        parse_reg(&tokens[3], line_no)?,
    ))
}

fn format_constant(constant: &Constant) -> String {
    match constant {
        Constant::I8(v) => format!("i8 {v}"),
        Constant::I16(v) => format!("i16 {v}"),
        Constant::I32(v) => format!("i32 {v}"),
        Constant::I64(v) => format!("i64 {v}"),
        Constant::U8(v) => format!("u8 {v}"),
        Constant::U16(v) => format!("u16 {v}"),
        Constant::U32(v) => format!("u32 {v}"),
        Constant::U64(v) => format!("u64 {v}"),
        Constant::F32(v) => format!("f32 {v}"),
        Constant::F64(v) => format!("f64 {v}"),
        Constant::Bool(v) => format!("bool {v}"),
        Constant::String(v) => format!("string \"{}\"", escape_string(v)),
    }
}

fn parse_typed_constant(kind: &str, value: &str, line_no: usize) -> Result<Constant, TextError> {
    Ok(match kind {
        "i8" => Constant::I8(parse_num(value, line_no)?),
        "i16" => Constant::I16(parse_num(value, line_no)?),
        "i32" => Constant::I32(parse_num(value, line_no)?),
        "i64" => Constant::I64(parse_num(value, line_no)?),
        "u8" => Constant::U8(parse_num(value, line_no)?),
        "u16" => Constant::U16(parse_num(value, line_no)?),
        "u32" => Constant::U32(parse_num(value, line_no)?),
        "u64" => Constant::U64(parse_num(value, line_no)?),
        "f32" => Constant::F32(parse_num(value, line_no)?),
        "f64" => Constant::F64(parse_num(value, line_no)?),
        "bool" => Constant::Bool(parse_num(value, line_no)?),
        "string" => Constant::String(parse_quoted(value, line_no)?),
        _ => {
            return Err(TextError::new(
                line_no,
                1,
                format!("unknown constant type `{kind}`"),
            ));
        }
    })
}

fn parse_num<T: std::str::FromStr>(value: &str, line_no: usize) -> Result<T, TextError> {
    value
        .parse()
        .map_err(|_| TextError::new(line_no, 1, format!("invalid number `{value}`")))
}

fn parse_quoted(token: &str, line_no: usize) -> Result<String, TextError> {
    if !token.starts_with('"') || !token.ends_with('"') || token.len() < 2 {
        return Err(TextError::new(line_no, 1, "expected quoted string"));
    }
    let mut out = String::new();
    let mut chars = token[1..token.len() - 1].chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let escaped = chars
                .next()
                .ok_or_else(|| TextError::new(line_no, 1, "invalid escape"))?;
            out.push(match escaped {
                'n' => '\n',
                't' => '\t',
                '"' => '"',
                '\\' => '\\',
                other => {
                    return Err(TextError::new(
                        line_no,
                        1,
                        format!("unsupported escape `\\{other}`"),
                    ));
                }
            });
        } else {
            out.push(ch);
        }
    }
    Ok(out)
}

fn escape_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
        .replace('"', "\\\"")
}

fn tokenize(line: &str, line_no: usize) -> Result<Vec<String>, TextError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;
    for ch in line.chars() {
        if in_string {
            current.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => {
                in_string = true;
                current.push(ch);
            }
            ',' | ' ' | '\t' => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if in_string {
        return Err(TextError::new(line_no, 1, "unterminated string"));
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

fn strip_comment(line: &str) -> String {
    let mut in_string = false;
    let mut escaped = false;
    let mut out = String::new();
    for ch in line.chars() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            out.push(ch);
        } else if ch == ';' || ch == '#' {
            break;
        } else {
            out.push(ch);
        }
    }
    out
}

fn parse_reg(token: &str, line_no: usize) -> Result<u8, TextError> {
    token
        .strip_prefix('r')
        .ok_or_else(|| TextError::new(line_no, 1, format!("expected register, got `{token}`")))?
        .parse()
        .map_err(|_| TextError::new(line_no, 1, "invalid register"))
}

fn parse_u8(token: &str, line_no: usize) -> Result<u8, TextError> {
    parse_num(token, line_no)
}

fn parse_u16(token: &str, line_no: usize) -> Result<u16, TextError> {
    parse_num(token, line_no)
}

fn parse_u32(token: &str, line_no: usize) -> Result<u32, TextError> {
    parse_num(token, line_no)
}

fn parse_i16(token: &str, line_no: usize) -> Result<i16, TextError> {
    parse_num(token, line_no)
}

fn parse_usize(token: &str, line_no: usize) -> Result<usize, TextError> {
    parse_num(token, line_no)
}

fn ensure_next_index(actual: usize, expected: usize, line_no: usize) -> Result<(), TextError> {
    if actual == expected {
        Ok(())
    } else {
        Err(TextError::new(
            line_no,
            1,
            format!("expected table index {expected}, got {actual}"),
        ))
    }
}

fn expect_len(tokens: &[String], len: usize, line_no: usize) -> Result<(), TextError> {
    if tokens.len() == len {
        Ok(())
    } else {
        Err(TextError::new(
            line_no,
            1,
            format!("expected {len} tokens, got {}", tokens.len()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_literal_print_program() {
        let source = r#"
            .module "hello"
            .version 1
            .entry 0

            .natives
              0 "std.print_i64"

            .constants
              0 i64 42

            .callables
              0 native 0

            .functions
              0 "main" regs=2
                0000 closure r0, 0
                0001 loadk r1, 0
                0002 call r0, 1, 0
                0003 halt
              end
        "#;

        let module = parse_module(source).unwrap();
        assert_eq!(module.name, "hello");
        assert_eq!(module.constants[0], Constant::I64(42));
        assert_eq!(module.callables[0], Callable::Native(0));
        assert_eq!(module.functions[0].chunk.code.len(), 4);
    }
}
