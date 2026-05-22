use crate::bytecode::{Chunk, Constant, Instruction, Opcode};
use crate::vm::native::{ImportDecl, ImportKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Callable {
    Function(u32),
    Import(u32),
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub chunk: Chunk,
}

#[derive(Debug, Clone)]
pub struct Module {
    pub name: String,
    pub version: u16,
    pub entry: u32,
    pub constants: Vec<Constant>,
    pub imports: Vec<ImportDecl>,
    pub callables: Vec<Callable>,
    pub functions: Vec<Function>,
}

impl Module {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: 1,
            entry: 0,
            constants: Vec::new(),
            imports: Vec::new(),
            callables: Vec::new(),
            functions: Vec::new(),
        }
    }

    pub fn entry_function(&self) -> Option<&Function> {
        self.functions.get(self.entry as usize)
    }
}

pub fn link_modules(
    root: Module,
    loader: &dyn Fn(&str) -> Result<Module, String>,
) -> Result<Module, String> {
    // Collect unique external paths
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    let mut linked: Vec<Module> = Vec::new();
    for import in &root.imports {
        if let ImportKind::External { path, .. } = &import.kind {
            if seen.insert(path.clone()) {
                let dep = loader(path)?;
                linked.push(link_modules(dep, loader)?);
            }
        }
    }

    if linked.is_empty() {
        return Ok(root);
    }

    let mut merged = Module::new(root.name);
    merged.version = root.version;

    // Merge constants from dependencies first
    for l in &linked {
        merged.constants.extend(l.constants.clone());
    }
    let our_const_base = merged.constants.len();
    merged.constants.extend(root.constants);

    // Merge imports from dependencies, then our own (native only — externals were resolved)
    for l in &linked {
        merged.imports.extend(l.imports.clone());
    }
    for import in &root.imports {
        if import.is_native() {
            merged.imports.push(import.clone());
        }
    }

    // Merge functions from dependencies first
    for l in &linked {
        for f in &l.functions {
            merged.functions.push(f.clone());
        }
    }
    let our_func_base = merged.functions.len() as u32;

    // Merge callables from dependencies
    let callable_offset: usize = linked.iter().map(|l| l.callables.len()).sum();
    for l in &linked {
        for c in &l.callables {
            merged.callables.push(match c {
                Callable::Function(id) => Callable::Function(*id),
                Callable::Import(id) => Callable::Import(*id),
            });
        }
    }

    // Adjust and merge our own functions
    let our_callable_count = root.callables.len();
    for mut f in root.functions {
        for instr in f.chunk.code.iter_mut() {
            match instr.opcode() {
                Opcode::LOADK => {
                    let bx = instr.bx();
                    let adjusted = bx as usize + our_const_base;
                    *instr = Instruction::abx(Opcode::LOADK, instr.a(), adjusted as u16);
                }
                Opcode::CLOSURE => {
                    let bx = instr.bx() as usize;
                    if bx < our_callable_count {
                        *instr = Instruction::abx(
                            Opcode::CLOSURE,
                            instr.a(),
                            (bx + callable_offset) as u16,
                        );
                    }
                }
                _ => {}
            }
        }
        merged.functions.push(f);
    }

    // Adjust our callables — resolve Import references
    for c in &root.callables {
        merged.callables.push(match c {
            Callable::Function(id) => Callable::Function(id + our_func_base),
            Callable::Import(import_idx) => {
                let import = &root.imports[*import_idx as usize];
                match &import.kind {
                    // Native imports stay as Import — resolved at runtime
                    ImportKind::Native { .. } => Callable::Import(*import_idx),
                    // External imports must be resolved to a function in the linked module
                    ImportKind::External { path: _, function } => {
                        // Find the function by name in the linked modules
                        let mut found = None;
                        for (linked_idx, linked_module) in linked.iter().enumerate() {
                            let mut func_offset: usize = 0;
                            for j in 0..linked_idx {
                                func_offset += linked[j].functions.len();
                            }
                            if let Some(func_idx) = linked_module.functions.iter().position(|f| &f.name == function) {
                                found = Some(func_offset + func_idx);
                                break;
                            }
                        }
                        match found {
                            Some(func_id) => Callable::Function(func_id as u32),
                            None => {
                                return Err(format!(
                                    "external import `{import}`: function `{function}` not found"
                                ));
                            }
                        }
                    }
                }
            }
        });
    }

    merged.entry = root.entry + our_func_base;
    Ok(merged)
}
