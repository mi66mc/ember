#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportKind {
    Native { module: String, function: String },
    External { path: String, function: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDecl {
    pub(crate) kind: ImportKind,
}

impl ImportDecl {
    pub fn native(module: impl Into<String>, function: impl Into<String>) -> Self {
        Self {
            kind: ImportKind::Native {
                module: module.into(),
                function: function.into(),
            },
        }
    }

    pub fn external(path: impl Into<String>, function: impl Into<String>) -> Self {
        Self {
            kind: ImportKind::External {
                path: path.into(),
                function: function.into(),
            },
        }
    }

    pub fn module_name(&self) -> &str {
        match &self.kind {
            ImportKind::Native { module, .. } => module,
            ImportKind::External { path, .. } => path,
        }
    }

    pub fn function_name(&self) -> &str {
        match &self.kind {
            ImportKind::Native { function, .. } => function,
            ImportKind::External { function, .. } => function,
        }
    }

    pub fn is_native(&self) -> bool {
        matches!(self.kind, ImportKind::Native { .. })
    }
}

impl std::fmt::Display for ImportDecl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            ImportKind::Native { module, function } => write!(f, "{module}.{function}"),
            ImportKind::External { path, function } => write!(f, "\"{path}\".{function}"),
        }
    }
}
