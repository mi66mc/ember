use std::collections::HashMap;
use std::rc::Rc;

use crate::vm::register::{Register, VmValue};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeError {
    pub message: String,
}

impl NativeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

pub type NativeResult = Result<Vec<VmValue>, NativeError>;
pub type NativeFn = fn(&[VmValue]) -> NativeResult;

#[derive(Clone)]
pub struct NativeFunction {
    pub name: Rc<str>,
    pub function: NativeFn,
}

impl std::fmt::Debug for NativeFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeFunction")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl NativeFunction {
    pub fn new(name: impl Into<Rc<str>>, function: NativeFn) -> Self {
        Self {
            name: name.into(),
            function,
        }
    }

    pub fn call(&self, args: &[VmValue]) -> NativeResult {
        (self.function)(args)
    }
}

#[derive(Clone, Debug, Default)]
pub struct NativeRegistry {
    functions: HashMap<String, NativeFunction>,
}

impl NativeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_std() -> Self {
        let mut registry = Self::new();
        registry.register("std.print_i64", native_print_i64);
        registry.register("std.print_u64", native_print_u64);
        registry.register("std.print_f64", native_print_f64);
        registry.register("std.print_bool", native_print_bool);
        registry.register("std.print_str", native_print_str);
        registry
    }

    pub fn register(&mut self, name: impl Into<String>, function: NativeFn) {
        let name = name.into();
        self.functions
            .insert(name.clone(), NativeFunction::new(name, function));
    }

    pub fn get(&self, name: &str) -> Option<NativeFunction> {
        self.functions.get(name).cloned()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }
}

fn expect_one<'a>(args: &'a [VmValue], name: &str) -> Result<&'a VmValue, NativeError> {
    if args.len() == 1 {
        Ok(&args[0])
    } else {
        Err(NativeError::new(format!(
            "{name} expects 1 argument, got {}",
            args.len()
        )))
    }
}

fn scalar_arg(args: &[VmValue], name: &str) -> Result<Register, NativeError> {
    expect_one(args, name)?
        .as_scalar()
        .ok_or_else(|| NativeError::new(format!("{name} expects a scalar argument")))
}

fn native_print_i64(args: &[VmValue]) -> NativeResult {
    let value = unsafe { scalar_arg(args, "std.print_i64")?.i64 };
    println!("{value}");
    Ok(Vec::new())
}

fn native_print_u64(args: &[VmValue]) -> NativeResult {
    let value = unsafe { scalar_arg(args, "std.print_u64")?.u64 };
    println!("{value}");
    Ok(Vec::new())
}

fn native_print_f64(args: &[VmValue]) -> NativeResult {
    let value = unsafe { scalar_arg(args, "std.print_f64")?.f64 };
    println!("{value}");
    Ok(Vec::new())
}

fn native_print_bool(args: &[VmValue]) -> NativeResult {
    let value = unsafe { scalar_arg(args, "std.print_bool")?.u64 != 0 };
    println!("{value}");
    Ok(Vec::new())
}

fn native_print_str(args: &[VmValue]) -> NativeResult {
    let value = match expect_one(args, "std.print_str")? {
        VmValue::String(value) => value,
        _ => return Err(NativeError::new("std.print_str expects a string argument")),
    };
    println!("{value}");
    Ok(Vec::new())
}
