use crate::bytecode::{
    Callable, Chunk, Constant, Function, ImportDecl, Instruction, Module, Opcode,
};
use crate::bytecode::module::SourceLocation;

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
    Imports,
    Constants,
    Callables,
    Functions,
}

pub fn parse_module(source: &str) -> Result<Module, TextError> {
    let mut module = Module::new("main");
    let mut section = Section::None;
    let mut pending_function: Option<(Function, Vec<String>, Vec<usize>)> = None;

    for (idx, raw) in source.lines().enumerate() {
        let line_no = idx + 1;
        let line = strip_comment(raw).trim().to_string();
        if line.is_empty() {
            continue;
        }

        if let Some((ref _func, ref mut body_lines, ref mut body_line_nos)) = pending_function {
            if line == "end" {
                let (func, body_lines, body_line_nos) = pending_function.take().unwrap();
                let function = parse_function_body(func, &body_lines, &body_line_nos)?;
                module.functions.push(function);
            } else {
                body_lines.push(line);
                body_line_nos.push(line_no);
            }
            continue;
        }

        match line.as_str() {
            ".import" => section = Section::Imports,
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
                module.entry = Some(line[".entry ".len()..]
                    .trim()
                    .parse()
                    .map_err(|_| TextError::new(line_no, 1, "invalid entry function id"))?);
            }
            _ => match section {
                Section::Imports => parse_import(&line, line_no, &mut module)?,
                Section::Constants => parse_constant_entry(&line, line_no, &mut module)?,
                Section::Callables => parse_callable(&line, line_no, &mut module)?,
                Section::Functions => {
                    let func = parse_function_header(&line, line_no)?;
                    pending_function = Some((func, Vec::new(), Vec::new()));
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

    if pending_function.is_some() {
        return Err(TextError::new(
            source.lines().count(),
            1,
            "unterminated function",
        ));
    }
    Ok(module)
}

fn parse_function_body(
    mut function: Function,
    body_lines: &[String],
    body_line_nos: &[usize],
) -> Result<Function, TextError> {
    use std::collections::HashMap;

    // Pass 1: collect label positions
    let mut labels: HashMap<String, usize> = HashMap::new();
    {
        let mut pc: usize = 0;
        for (i, line) in body_lines.iter().enumerate() {
            let line_no = body_line_nos[i];
            let tokens = tokenize(line, line_no)?;
            if tokens.is_empty() {
                continue;
            }
            let first = tokens[0].as_str();
            if let Some(label_name) = first.strip_suffix(':') {
                if label_name.starts_with('@') && label_name.len() > 1 {
                    let name = label_name[1..].to_string();
                    if labels.contains_key(&name) {
                        return Err(TextError::new(line_no, 1, format!("duplicate label @{name}")));
                    }
                    labels.insert(name, pc);
                    // Check if there's an instruction after the label on the same line
                    if tokens.len() > 1 {
                        let rest: Vec<String> = tokens[1..].to_vec();
                        if is_instruction(&rest) {
                            pc += 1;
                        }
                    }
                }
            } else if first.starts_with('@') && first.len() > 1 {
                // Label on its own: @name
                let name = first[1..].to_string();
                if labels.contains_key(&name) {
                    return Err(TextError::new(line_no, 1, format!("duplicate label @{name}")));
                }
                labels.insert(name, pc);
                if tokens.len() > 1 {
                    let rest: Vec<String> = tokens[1..].to_vec();
                    if is_instruction(&rest) {
                        pc += 1;
                    }
                }
            } else if is_instruction(&tokens) {
                pc += 1;
            }
        }
    }

    // Pass 2: emit instructions with resolved labels
    for (i, line) in body_lines.iter().enumerate() {
        let line_no = body_line_nos[i];
        let tokens = tokenize(line, line_no)?;
        if tokens.is_empty() {
            continue;
        }
        let first = tokens[0].as_str();

        // Skip label declarations, keep instructions on the same line
        let instr_tokens: Vec<String> = if first.ends_with(':') && first.starts_with('@') {
            if tokens.len() > 1 {
                tokens[1..].to_vec()
            } else {
                continue;
            }
        } else if first.starts_with('@') && first.len() > 1 {
            if tokens.len() > 1 && is_instruction(&tokens[1..]) {
                tokens[1..].to_vec()
            } else {
                continue;
            }
        } else {
            tokens.clone()
        };

        if instr_tokens.is_empty() || !is_instruction(&instr_tokens) {
            continue;
        }

        // Remove PC prefix if present
        let cleaned_tokens = if instr_tokens[0]
            .chars()
            .all(|ch| ch.is_ascii_digit())
        {
            instr_tokens[1..].to_vec()
        } else {
            instr_tokens
        };

        // Resolve label references in operands
        let resolved: Vec<String> = cleaned_tokens
            .iter()
            .map(|token| {
                if let Some(name) = token.strip_prefix('@') {
                    if let Some(&target_pc) = labels.get(name) {
                        let current_pc: usize = function.chunk.len();
                        let offset = target_pc as isize - current_pc as isize;
                        offset.to_string()
                    } else {
                        token.clone()
                    }
                } else {
                    token.clone()
                }
            })
            .collect();

        let pc_before = function.chunk.len();
        let instrs = parse_instruction(&resolved, line_no)?;
        for instr in instrs {
            function.chunk.emit(instr);
        }
        function.chunk.source_map.insert(pc_before as u32, SourceLocation {
            line: line_no as u32,
            column: 1,
        });
    }

    Ok(function)
}

fn is_instruction(tokens: &[String]) -> bool {
    if tokens.is_empty() {
        return false;
    }
    let first = tokens[0].as_str();
    matches!(
        first,
        "halt" | "nop" | "ext"
            | "loadk" | "loadkx"
            | "closure" | "closurex"
            | "call"
            | "ret"
            | "jmp" | "jmpx"
            | "jmpif" | "jmpifnot"
            | "getg"
            | "setg"
            | "move"
            | "conv"
    ) || first.contains('.')
        || first
            .chars()
            .all(|ch| ch.is_ascii_digit())
}

pub fn validate_module(module: &Module) -> Result<(), String> {
    if let Some(entry) = module.entry {
        if entry as usize >= module.functions.len() {
            return Err(format!("entry function {entry} does not exist"));
        }
    }
    for (idx, callable) in module.callables.iter().enumerate() {
        match callable {
            Callable::Function(function_id)
                if *function_id as usize >= module.functions.len() =>
            {
                return Err(format!(
                    "callable {idx} references missing function {function_id}"
                ));
            }
            Callable::Import(id) => {
                if *id as usize >= module.imports.len() {
                    return Err(format!(
                        "callable {idx} references undeclared import index {id}"
                    ));
                }
            }
            _ => {}
        }
    }
    for (function_id, function) in module.functions.iter().enumerate() {
        if function.chunk.max_registers > crate::vm::stack::MAX_REGISTERS {
            return Err(format!(
                "function {function_id} declares {regs} registers, max is {max}",
                regs = function.chunk.max_registers,
                max = crate::vm::stack::MAX_REGISTERS
            ));
        }
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
    if let Some(entry) = module.entry {
        out.push_str(&format!(".entry {entry}\n"));
    }

    out.push_str("\n.import\n");
    for import in &module.imports {
        out.push_str(&format!("  {}\n", import.to_string()));
    }

    out.push_str("\n.constants\n");
    for (idx, constant) in module.constants.iter().enumerate() {
        out.push_str(&format!("  {} {}\n", idx, format_constant(constant)));
    }

    out.push_str("\n.callables\n");
    for (idx, callable) in module.callables.iter().enumerate() {
        let rendered = match callable {
            Callable::Function(id) => format!("function {id}"),
            Callable::Import(id) => format!("{}", module.imports[*id as usize]),
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

        let max_instr_len = function
            .chunk
            .code
            .iter()
            .map(|i| format_instruction(i).len())
            .max()
            .unwrap_or(0);
        let comment_col = max_instr_len + 4;

        for (pc, instr) in function.chunk.code.iter().enumerate() {
            let instr_text = format_instruction(instr);
            out.push_str(&format!(
                "    {instr_text:<comment_col$} ;; {pc}\n"
            ));
        }
        out.push_str("  end\n");
    }
    out
}

pub fn format_instruction(instr: &Instruction) -> String {
    match instr.opcode() {
        Opcode::LOADK => format!("loadk r{}, {}", instr.a(), instr.bx()),
        Opcode::MOVE => format!("move r{}, r{}", instr.a(), instr.b()),
        Opcode::CLOSURE => format!("closure r{}, {}", instr.a(), instr.bx()),
        Opcode::CALL => format!("call r{}, {}, {}", instr.a(), instr.b(), instr.c()),
        Opcode::RET => format!("ret r{}, {}", instr.a(), instr.b()),
        Opcode::JMP => format!("jmp {}", instr.sbx_ab()),
        Opcode::JMPIF => format!("jmpif r{}, {}", instr.a(), instr.sbx()),
        Opcode::JMPIFNOT => format!("jmpifnot r{}, {}", instr.a(), instr.sbx()),
        Opcode::GETG => format!("getg r{}, {}", instr.a(), instr.bx()),
        Opcode::SETG => format!("setg r{}, {}", instr.a(), instr.bx()),
        Opcode::NOP => "nop".to_string(),
        Opcode::HALT => "halt".to_string(),
        Opcode::EXT => "ext".to_string(),
        Opcode::CONV => format!("conv r{}, r{}, r{}", instr.a(), instr.b(), instr.c()),

        op => {
            let name = lowercase_opcode(op);
            match instruction_format(op) {
                InstrFormat::RRR => format!("{name} r{}, r{}, r{}", instr.a(), instr.b(), instr.c()),
                InstrFormat::RIR => format!("{name} r{}, {}, r{}", instr.a(), instr.b(), instr.c()),
                InstrFormat::RRI => format!("{name} r{}, r{}, {}", instr.a(), instr.b(), instr.c()),
            }
        }
    }
}

fn lowercase_opcode(opcode: Opcode) -> String {
    let debug = format!("{opcode:?}");
    debug.to_lowercase().replace('_', ".")
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
        Opcode::HALT | Opcode::NOP | Opcode::JMP | Opcode::EXT => {}
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

fn parse_import(line: &str, line_no: usize, module: &mut Module) -> Result<(), TextError> {
    let tokens = tokenize(line, line_no)?;
    // Syntax: `io.print_i64` (native) or `"lib".double` (external)
    let raw = tokens.join(" ").replace(" .", ".");

    // Check if it's a quoted external: "path".function
    if raw.starts_with('"') {
        if let Some(dot_pos) = raw.rfind("\".") {
            let path = parse_quoted(&raw[..=dot_pos], line_no)?;
            let function = raw[dot_pos + 2..].trim().to_string();
            if function.is_empty() {
                return Err(TextError::new(line_no, 1, "expected function name after path"));
            }
            module
                .imports
                .push(ImportDecl::external(path, function));
            return Ok(());
        }
        return Err(TextError::new(line_no, 1, "expected external import: \"path\".function"));
    }

    // Native import: module.function
    if let Some(dot_pos) = raw.find('.') {
        let module_name = raw[..dot_pos].trim().to_string();
        let function = raw[dot_pos + 1..].trim().to_string();
        if module_name.is_empty() || function.is_empty() {
            return Err(TextError::new(line_no, 1, "expected `module.function`"));
        }
        module
            .imports
            .push(ImportDecl::native(module_name, function));
        return Ok(());
    }

    Err(TextError::new(line_no, 1, "expected `module.function` or `\"path\".function`"))
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
    if tokens.len() < 2 {
        return Err(TextError::new(line_no, 1, "expected `index function N` or `index mod.func`"));
    }
    let idx = parse_usize(&tokens[0], line_no)?;
    ensure_next_index(idx, module.callables.len(), line_no)?;

    if tokens[1] == "function" {
        if tokens.len() < 3 {
            return Err(TextError::new(line_no, 1, "expected function index"));
        }
        module
            .callables
            .push(Callable::Function(parse_u32(&tokens[2], line_no)?));
        return Ok(());
    }

    // Import reference: `io.print_i64` (native) or `"lib".double` (external)
    // Reconstruct qualified name — it may be tokenized as one or multiple tokens
    let raw = tokens[1..].join(" ").replace(" .", ".");
    let import: ImportDecl = if raw.starts_with('"') {
        if let Some(dot_pos) = raw.rfind("\".") {
            let path = parse_quoted(&raw[..=dot_pos], line_no)?;
            let function = raw[dot_pos + 2..].trim().to_string();
            ImportDecl::external(path, function)
        } else {
            return Err(TextError::new(line_no, 1, "expected external: \"path\".function"));
        }
    } else if let Some(dot_pos) = raw.find('.') {
        let module_name = raw[..dot_pos].trim().to_string();
        let function = raw[dot_pos + 1..].trim().to_string();
        ImportDecl::native(module_name, function)
    } else {
        return Err(TextError::new(line_no, 1, "expected `module.function`, `\"path\".function`, or `function N`"));
    };

    // Find matching import declaration
    let import_idx = module
        .imports
        .iter()
        .position(|i| *i == import)
        .ok_or_else(|| {
            TextError::new(
                line_no,
                1,
                format!("undeclared import `{import}`"),
            )
        })?;

    module.callables.push(Callable::Import(import_idx as u32));
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstrFormat {
    RRR,
    RIR,
    RRI,
}

fn instruction_format(opcode: Opcode) -> InstrFormat {
    match opcode {
        Opcode::LOAD_I8
        | Opcode::LOAD_I16
        | Opcode::LOAD_I32
        | Opcode::LOAD_I64
        | Opcode::LOAD_U8
        | Opcode::LOAD_U16
        | Opcode::LOAD_U32
        | Opcode::LOAD_U64
        | Opcode::LOAD_F32
        | Opcode::LOAD_F64 => InstrFormat::RRI,
        Opcode::STORE_I8
        | Opcode::STORE_I16
        | Opcode::STORE_I32
        | Opcode::STORE_I64
        | Opcode::STORE_U8
        | Opcode::STORE_U16
        | Opcode::STORE_U32
        | Opcode::STORE_U64
        | Opcode::STORE_F32
        | Opcode::STORE_F64 => InstrFormat::RIR,
        _ => InstrFormat::RRR,
    }
}

fn parse_typed_mnemonic(
    op: &str,
    tokens: &[String],
    line_no: usize,
) -> Result<Option<Vec<Instruction>>, TextError> {
    if let Some(dot) = op.rfind('.') {
        let base = &op[..dot];
        let suffix = &op[dot + 1..];
        let opcode = match (base, suffix) {
            ("load", "i8") => Opcode::LOAD_I8,
            ("load", "i16") => Opcode::LOAD_I16,
            ("load", "i32") => Opcode::LOAD_I32,
            ("load", "i64") => Opcode::LOAD_I64,
            ("load", "u8") => Opcode::LOAD_U8,
            ("load", "u16") => Opcode::LOAD_U16,
            ("load", "u32") => Opcode::LOAD_U32,
            ("load", "u64") => Opcode::LOAD_U64,
            ("load", "f32") => Opcode::LOAD_F32,
            ("load", "f64") => Opcode::LOAD_F64,

            ("store", "i8") => Opcode::STORE_I8,
            ("store", "i16") => Opcode::STORE_I16,
            ("store", "i32") => Opcode::STORE_I32,
            ("store", "i64") => Opcode::STORE_I64,
            ("store", "u8") => Opcode::STORE_U8,
            ("store", "u16") => Opcode::STORE_U16,
            ("store", "u32") => Opcode::STORE_U32,
            ("store", "u64") => Opcode::STORE_U64,
            ("store", "f32") => Opcode::STORE_F32,
            ("store", "f64") => Opcode::STORE_F64,

            ("add", "i8") => Opcode::ADD_I8,
            ("add", "i16") => Opcode::ADD_I16,
            ("add", "i32") => Opcode::ADD_I32,
            ("add", "i64") => Opcode::ADD_I64,
            ("add", "u8") => Opcode::ADD_U8,
            ("add", "u16") => Opcode::ADD_U16,
            ("add", "u32") => Opcode::ADD_U32,
            ("add", "u64") => Opcode::ADD_U64,
            ("add", "f32") => Opcode::ADD_F32,
            ("add", "f64") => Opcode::ADD_F64,

            ("sub", "i8") => Opcode::SUB_I8,
            ("sub", "i16") => Opcode::SUB_I16,
            ("sub", "i32") => Opcode::SUB_I32,
            ("sub", "i64") => Opcode::SUB_I64,
            ("sub", "u8") => Opcode::SUB_U8,
            ("sub", "u16") => Opcode::SUB_U16,
            ("sub", "u32") => Opcode::SUB_U32,
            ("sub", "u64") => Opcode::SUB_U64,
            ("sub", "f32") => Opcode::SUB_F32,
            ("sub", "f64") => Opcode::SUB_F64,

            ("mul", "i8") => Opcode::MUL_I8,
            ("mul", "i16") => Opcode::MUL_I16,
            ("mul", "i32") => Opcode::MUL_I32,
            ("mul", "i64") => Opcode::MUL_I64,
            ("mul", "u8") => Opcode::MUL_U8,
            ("mul", "u16") => Opcode::MUL_U16,
            ("mul", "u32") => Opcode::MUL_U32,
            ("mul", "u64") => Opcode::MUL_U64,
            ("mul", "f32") => Opcode::MUL_F32,
            ("mul", "f64") => Opcode::MUL_F64,

            ("div", "i8") => Opcode::DIV_I8,
            ("div", "i16") => Opcode::DIV_I16,
            ("div", "i32") => Opcode::DIV_I32,
            ("div", "i64") => Opcode::DIV_I64,
            ("div", "u8") => Opcode::DIV_U8,
            ("div", "u16") => Opcode::DIV_U16,
            ("div", "u32") => Opcode::DIV_U32,
            ("div", "u64") => Opcode::DIV_U64,
            ("div", "f32") => Opcode::DIV_F32,
            ("div", "f64") => Opcode::DIV_F64,

            ("mod", "i8") => Opcode::MOD_I8,
            ("mod", "i16") => Opcode::MOD_I16,
            ("mod", "i32") => Opcode::MOD_I32,
            ("mod", "i64") => Opcode::MOD_I64,
            ("mod", "u8") => Opcode::MOD_U8,
            ("mod", "u16") => Opcode::MOD_U16,
            ("mod", "u32") => Opcode::MOD_U32,
            ("mod", "u64") => Opcode::MOD_U64,

            ("neg", "i8") => Opcode::NEG_I8,
            ("neg", "i16") => Opcode::NEG_I16,
            ("neg", "i32") => Opcode::NEG_I32,
            ("neg", "i64") => Opcode::NEG_I64,
            ("neg", "f32") => Opcode::NEG_F32,
            ("neg", "f64") => Opcode::NEG_F64,

            ("and", "i8") => Opcode::AND_I8,
            ("and", "i16") => Opcode::AND_I16,
            ("and", "i32") => Opcode::AND_I32,
            ("and", "i64") => Opcode::AND_I64,

            ("or", "i8") => Opcode::OR_I8,
            ("or", "i16") => Opcode::OR_I16,
            ("or", "i32") => Opcode::OR_I32,
            ("or", "i64") => Opcode::OR_I64,

            ("xor", "i8") => Opcode::XOR_I8,
            ("xor", "i16") => Opcode::XOR_I16,
            ("xor", "i32") => Opcode::XOR_I32,
            ("xor", "i64") => Opcode::XOR_I64,

            ("not", "i8") => Opcode::NOT_I8,
            ("not", "i16") => Opcode::NOT_I16,
            ("not", "i32") => Opcode::NOT_I32,
            ("not", "i64") => Opcode::NOT_I64,

            ("shl", "i8") => Opcode::SHL_I8,
            ("shl", "i16") => Opcode::SHL_I16,
            ("shl", "i32") => Opcode::SHL_I32,
            ("shl", "i64") => Opcode::SHL_I64,

            ("shr", "i8") => Opcode::SHR_I8,
            ("shr", "i16") => Opcode::SHR_I16,
            ("shr", "i32") => Opcode::SHR_I32,
            ("shr", "i64") => Opcode::SHR_I64,

            ("ushr", "i8") => Opcode::USHR_I8,
            ("ushr", "i16") => Opcode::USHR_I16,
            ("ushr", "i32") => Opcode::USHR_I32,
            ("ushr", "i64") => Opcode::USHR_I64,

            ("eq", "i8") => Opcode::EQ_I8,
            ("eq", "i16") => Opcode::EQ_I16,
            ("eq", "i32") => Opcode::EQ_I32,
            ("eq", "i64") => Opcode::EQ_I64,
            ("eq", "f32") => Opcode::EQ_F32,
            ("eq", "f64") => Opcode::EQ_F64,

            ("ne", "i8") => Opcode::NE_I8,
            ("ne", "i16") => Opcode::NE_I16,
            ("ne", "i32") => Opcode::NE_I32,
            ("ne", "i64") => Opcode::NE_I64,
            ("ne", "f32") => Opcode::NE_F32,
            ("ne", "f64") => Opcode::NE_F64,

            ("lt", "i8") => Opcode::LT_I8,
            ("lt", "i16") => Opcode::LT_I16,
            ("lt", "i32") => Opcode::LT_I32,
            ("lt", "i64") => Opcode::LT_I64,
            ("lt", "u8") => Opcode::LT_U8,
            ("lt", "u16") => Opcode::LT_U16,
            ("lt", "u32") => Opcode::LT_U32,
            ("lt", "u64") => Opcode::LT_U64,
            ("lt", "f32") => Opcode::LT_F32,
            ("lt", "f64") => Opcode::LT_F64,

            ("le", "i8") => Opcode::LE_I8,
            ("le", "i16") => Opcode::LE_I16,
            ("le", "i32") => Opcode::LE_I32,
            ("le", "i64") => Opcode::LE_I64,
            ("le", "u8") => Opcode::LE_U8,
            ("le", "u16") => Opcode::LE_U16,
            ("le", "u32") => Opcode::LE_U32,
            ("le", "u64") => Opcode::LE_U64,
            ("le", "f32") => Opcode::LE_F32,
            ("le", "f64") => Opcode::LE_F64,

            ("gt", "i8") => Opcode::GT_I8,
            ("gt", "i16") => Opcode::GT_I16,
            ("gt", "i32") => Opcode::GT_I32,
            ("gt", "i64") => Opcode::GT_I64,
            ("gt", "u8") => Opcode::GT_U8,
            ("gt", "u16") => Opcode::GT_U16,
            ("gt", "u32") => Opcode::GT_U32,
            ("gt", "u64") => Opcode::GT_U64,
            ("gt", "f32") => Opcode::GT_F32,
            ("gt", "f64") => Opcode::GT_F64,

            ("ge", "i8") => Opcode::GE_I8,
            ("ge", "i16") => Opcode::GE_I16,
            ("ge", "i32") => Opcode::GE_I32,
            ("ge", "i64") => Opcode::GE_I64,
            ("ge", "u8") => Opcode::GE_U8,
            ("ge", "u16") => Opcode::GE_U16,
            ("ge", "u32") => Opcode::GE_U32,
            ("ge", "u64") => Opcode::GE_U64,
            ("ge", "f32") => Opcode::GE_F32,
            ("ge", "f64") => Opcode::GE_F64,

            _ => return Ok(None),
        };
        let instr = match instruction_format(opcode) {
            InstrFormat::RRR => abc(tokens, line_no, opcode)?,
            InstrFormat::RIR => abc_rir(tokens, line_no, opcode)?,
            InstrFormat::RRI => abc_rri(tokens, line_no, opcode)?,
        };
        return Ok(Some(vec![instr]));
    }
    Ok(None)
}

fn parse_instruction(tokens: &[String], line_no: usize) -> Result<Vec<Instruction>, TextError> {
    let op = tokens
        .first()
        .ok_or_else(|| TextError::new(line_no, 1, "empty instruction"))?
        .as_str();

    if let Some(instrs) = parse_typed_mnemonic(op, tokens, line_no)? {
        return Ok(instrs);
    }

    let result: Vec<Instruction> = match op {
        "halt" => vec![Instruction::abc(Opcode::HALT, 0, 0, 0)],
        "nop" => vec![Instruction::abc(Opcode::NOP, 0, 0, 0)],
        "loadk" => {
            expect_len(tokens, 3, line_no)?;
            vec![Instruction::abx(
                Opcode::LOADK,
                parse_reg(&tokens[1], line_no)?,
                parse_u16(&tokens[2], line_no)?,
            )]
        }
        "loadkx" => {
            expect_len(tokens, 3, line_no)?;
            let (ext, instr) = emit_ext_abx(
                Opcode::LOADK,
                parse_reg(&tokens[1], line_no)?,
                parse_u32(&tokens[2], line_no)?,
            )?;
            vec![ext, instr]
        }
        "closure" => {
            expect_len(tokens, 3, line_no)?;
            vec![Instruction::abx(
                Opcode::CLOSURE,
                parse_reg(&tokens[1], line_no)?,
                parse_u16(&tokens[2], line_no)?,
            )]
        }
        "closurex" => {
            expect_len(tokens, 3, line_no)?;
            let (ext, instr) = emit_ext_abx(
                Opcode::CLOSURE,
                parse_reg(&tokens[1], line_no)?,
                parse_u32(&tokens[2], line_no)?,
            )?;
            vec![ext, instr]
        }
        "call" => {
            expect_len(tokens, 4, line_no)?;
            vec![Instruction::abc(
                Opcode::CALL,
                parse_reg(&tokens[1], line_no)?,
                parse_u8(&tokens[2], line_no)?,
                parse_u8(&tokens[3], line_no)?,
            )]
        }
        "ret" => {
            expect_len(tokens, 3, line_no)?;
            vec![Instruction::abc(
                Opcode::RET,
                parse_reg(&tokens[1], line_no)?,
                parse_u8(&tokens[2], line_no)?,
                0,
            )]
        }
        "jmp" => {
            expect_len(tokens, 2, line_no)?;
            vec![Instruction::jmp(
                Opcode::JMP,
                parse_i16(&tokens[1], line_no)?,
            )]
        }
        "jmpx" => {
            expect_len(tokens, 2, line_no)?;
            let (ext, instr) = emit_ext_jmp(parse_i32(&tokens[1], line_no)?)?;
            vec![ext, instr]
        }
        "jmpif" | "jmpifnot" => {
            expect_len(tokens, 3, line_no)?;
            vec![Instruction::asbx(
                if op == "jmpif" {
                    Opcode::JMPIF
                } else {
                    Opcode::JMPIFNOT
                },
                parse_reg(&tokens[1], line_no)?,
                parse_i16(&tokens[2], line_no)?,
            )]
        }
        "getg" => {
            expect_len(tokens, 3, line_no)?;
            vec![Instruction::abx(
                Opcode::GETG,
                parse_reg(&tokens[1], line_no)?,
                parse_u16(&tokens[2], line_no)?,
            )]
        }
        "setg" => {
            expect_len(tokens, 3, line_no)?;
            vec![Instruction::abx(
                Opcode::SETG,
                parse_reg(&tokens[1], line_no)?,
                parse_u16(&tokens[2], line_no)?,
            )]
        }
        "move" => {
            expect_len(tokens, 3, line_no)?;
            vec![Instruction::abc(
                Opcode::MOVE,
                parse_reg(&tokens[1], line_no)?,
                parse_reg(&tokens[2], line_no)?,
                0,
            )]
        }
        "conv" => vec![abc(tokens, line_no, Opcode::CONV)?],
        _ => {
            return Err(TextError::new(
                line_no,
                1,
                format!("unknown instruction `{op}`"),
            ))
        }
    };
    Ok(result)
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

fn abc_rir(
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

fn abc_rri(
    tokens: &[String],
    line_no: usize,
    opcode: Opcode,
) -> Result<Instruction, TextError> {
    expect_len(tokens, 4, line_no)?;
    Ok(Instruction::abc(
        opcode,
        parse_reg(&tokens[1], line_no)?,
        parse_reg(&tokens[2], line_no)?,
        parse_u8(&tokens[3], line_no)?,
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
        Constant::Bytes(v) => {
            let escaped: String = v
                .iter()
                .flat_map(|&b| std::ascii::escape_default(b).map(char::from))
                .collect();
            format!("bytes \"{}\"", escaped)
        }
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
        "bytes" => Constant::Bytes(parse_quoted(value, line_no)?.into_bytes()),
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

fn parse_i32(token: &str, line_no: usize) -> Result<i32, TextError> {
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

fn emit_ext_abx(
    opcode: Opcode,
    a: u8,
    bx: u32,
) -> Result<(Instruction, Instruction), TextError> {
    let lo = bx as u16;
    let hi = (bx >> 16) as u16;
    let [bl, bh] = hi.to_le_bytes();
    let ext = Instruction::abc(Opcode::EXT, 0, bh, bl);
    let instr = Instruction::abx(opcode, a, lo);
    Ok((ext, instr))
}

fn emit_ext_jmp(offset: i32) -> Result<(Instruction, Instruction), TextError> {
    let lo = offset as i16;
    let hi = (offset >> 16) as u16;
    let [bl, bh] = hi.to_le_bytes();
    let ext = Instruction::abc(Opcode::EXT, 0, bh, bl);
    let instr = Instruction::jmp(Opcode::JMP, lo);
    Ok((ext, instr))
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
        "#;

        let module = parse_module(source).unwrap();
        assert_eq!(module.name, "hello");
        assert_eq!(module.constants[0], Constant::I64(42));
        assert_eq!(module.callables[0], Callable::Import(0));
        assert_eq!(module.functions[0].chunk.code.len(), 4);
    }

    #[test]
    fn labels_resolve_to_correct_offsets() {
        let source = r#"
            .module "loop"
            .version 1
            .entry 0

            .constants
              0 i64 0
              1 i64 10
              2 i64 1

            .functions
              0 "main" regs=4
                loadk r0, 0       ;; r0 = 0 (counter)
                loadk r1, 1       ;; r1 = 10 (limit)
                loadk r2, 2       ;; r2 = 1 (step)
              @loop:
                lt.i64 r3, r0, r1
                jmpifnot r3, @end
                add.i64 r0, r0, r2
                jmp @loop
              @end:
                halt
              end
        "#;

        let module = parse_module(source).unwrap();
        let chunk = &module.functions[0].chunk;
        assert_eq!(chunk.code.len(), 8);

        // @loop: points at PC 3 (the lt.i64 instruction)
        // @end: points at PC 7 (the halt instruction)
        // Instruction 3 (index 3): lt.i64
        assert_eq!(chunk.code[3].opcode(), Opcode::LT_I64);
        // Instruction 4 (index 4): jmpifnot r3, @end  → @end is at PC 7, offset = 7 - 4 = 3
        assert_eq!(chunk.code[4].opcode(), Opcode::JMPIFNOT);
        assert_eq!(chunk.code[4].sbx(), 3);
        // Instruction 6 (index 6): jmp @loop → @loop is at PC 3, offset = 3 - 6 = -3
        assert_eq!(chunk.code[6].opcode(), Opcode::JMP);
        assert_eq!(chunk.code[6].sbx_ab(), -3);
    }

    #[test]
    fn round_trips_all_opcode_families() {
        let source = r#"
            .module "all"
            .version 1
            .entry 0

            .constants
              0 i64 10
              1 f64 1.5

            .functions
              0 "main" regs=6
                loadk r0, 0
                loadk r1, 1
                move r2, r0
                add.i64 r2, r2, r1
                sub.i64 r3, r0, r1
                mul.i64 r4, r0, r1
                div.i64 r5, r0, r1
                mul.f64 r0, r1, r1
                eq.i64 r2, r0, r1
                lt.i64 r3, r0, r1
                and.i64 r4, r0, r1
                shl.i64 r5, r0, r1
                conv r0, r0, r1
                halt
              end
        "#;

        let module = parse_module(source).unwrap();
        let text = module_to_text(&module);
        let reparsed = parse_module(&text).unwrap();

        assert_eq!(module.name, reparsed.name);
        assert_eq!(module.constants, reparsed.constants);
        assert_eq!(
            module.functions[0].chunk.code.len(),
            reparsed.functions[0].chunk.code.len()
        );
        for (a, b) in module.functions[0]
            .chunk
            .code
            .iter()
            .zip(reparsed.functions[0].chunk.code.iter())
        {
            assert_eq!(a, b, "instruction mismatch");
        }
    }
}
