use crate::bytecode::{Chunk, Constant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeImport {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Callable {
    Function(u32),
    Native(u32),
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
    pub natives: Vec<NativeImport>,
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
            natives: Vec::new(),
            callables: Vec::new(),
            functions: Vec::new(),
        }
    }

    pub fn entry_function(&self) -> Option<&Function> {
        self.functions.get(self.entry as usize)
    }
}
