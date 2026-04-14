//! Standard Python builtins for both the bytecode VM and tree-walking interpreter.
//!
//! Provides: str, int, float, bool, len, range, repr, abs, min, max, sum.

use std::rc::Rc;

use crate::bytecode::Value as VmValue;
use crate::interpreter::Value as InterpValue;
use crate::symbol::Interner;
use crate::vm::VM;
use crate::interpreter::Interpreter;

// ── VM (bytecode) builtins ──────────────────────────────────────────

fn vm_str(args: &[VmValue]) -> Result<VmValue, String> {
    if args.len() != 1 { return Err(format!("str() takes 1 argument, got {}", args.len())); }
    let s = match &args[0] {
        VmValue::String(s) => s.as_str().to_string(),
        VmValue::Integer(n) => n.to_string(),
        VmValue::Float(f) => if f.fract() == 0.0 { format!("{}.0", f) } else { format!("{}", f) },
        VmValue::Boolean(b) => if *b { "True".to_string() } else { "False".to_string() },
        VmValue::None => "None".to_string(),
        other => format!("{}", other),
    };
    Ok(VmValue::String(Rc::new(s)))
}

fn vm_int(args: &[VmValue]) -> Result<VmValue, String> {
    if args.len() != 1 { return Err(format!("int() takes 1 argument, got {}", args.len())); }
    match &args[0] {
        VmValue::Integer(n) => Ok(VmValue::Integer(*n)),
        VmValue::Float(f) => Ok(VmValue::Integer(*f as i64)),
        VmValue::Boolean(b) => Ok(VmValue::Integer(if *b { 1 } else { 0 })),
        VmValue::String(s) => s.trim().parse::<i64>()
            .map(VmValue::Integer)
            .map_err(|_| format!("int() invalid literal: {:?}", s)),
        other => Err(format!("int() cannot convert {}", other)),
    }
}

fn vm_float(args: &[VmValue]) -> Result<VmValue, String> {
    if args.len() != 1 { return Err(format!("float() takes 1 argument, got {}", args.len())); }
    match &args[0] {
        VmValue::Float(f) => Ok(VmValue::Float(*f)),
        VmValue::Integer(n) => Ok(VmValue::Float(*n as f64)),
        VmValue::Boolean(b) => Ok(VmValue::Float(if *b { 1.0 } else { 0.0 })),
        VmValue::String(s) => s.trim().parse::<f64>()
            .map(VmValue::Float)
            .map_err(|_| format!("float() invalid literal: {:?}", s)),
        other => Err(format!("float() cannot convert {}", other)),
    }
}

fn vm_bool(args: &[VmValue]) -> Result<VmValue, String> {
    if args.is_empty() { return Ok(VmValue::Boolean(false)); }
    if args.len() != 1 { return Err(format!("bool() takes at most 1 argument, got {}", args.len())); }
    Ok(VmValue::Boolean(args[0].is_truthy()))
}

fn vm_len(args: &[VmValue]) -> Result<VmValue, String> {
    if args.len() != 1 { return Err(format!("len() takes 1 argument, got {}", args.len())); }
    match &args[0] {
        VmValue::String(s) => Ok(VmValue::Integer(s.chars().count() as i64)),
        VmValue::List(items) => Ok(VmValue::Integer(items.len() as i64)),
        VmValue::Dict(pairs) => Ok(VmValue::Integer(pairs.len() as i64)),
        other => Err(format!("len() not supported for {}", other)),
    }
}

fn vm_range(args: &[VmValue]) -> Result<VmValue, String> {
    let (start, stop, step) = match args.len() {
        1 => (0i64, as_int(&args[0], "range")?, 1i64),
        2 => (as_int(&args[0], "range")?, as_int(&args[1], "range")?, 1i64),
        3 => {
            let s = as_int(&args[2], "range")?;
            if s == 0 { return Err("range() step cannot be zero".to_string()); }
            (as_int(&args[0], "range")?, as_int(&args[1], "range")?, s)
        }
        n => return Err(format!("range() takes 1-3 arguments, got {}", n)),
    };
    let mut items = Vec::new();
    let mut i = start;
    if step > 0 { while i < stop { items.push(VmValue::Integer(i)); i += step; } }
    else { while i > stop { items.push(VmValue::Integer(i)); i += step; } }
    Ok(VmValue::List(Rc::new(items)))
}

fn vm_repr(args: &[VmValue]) -> Result<VmValue, String> {
    if args.len() != 1 { return Err(format!("repr() takes 1 argument, got {}", args.len())); }
    let s = match &args[0] {
        VmValue::String(s) => format!("'{}'", s),
        other => format!("{}", other),
    };
    Ok(VmValue::String(Rc::new(s)))
}

fn vm_abs(args: &[VmValue]) -> Result<VmValue, String> {
    if args.len() != 1 { return Err(format!("abs() takes 1 argument, got {}", args.len())); }
    match &args[0] {
        VmValue::Integer(n) => Ok(VmValue::Integer(n.wrapping_abs())),
        VmValue::Float(f) => Ok(VmValue::Float(f.abs())),
        other => Err(format!("abs() not supported for {}", other)),
    }
}

fn vm_min(args: &[VmValue]) -> Result<VmValue, String> {
    let items = collect_numeric(args, "min")?;
    items.into_iter().reduce(|a, b| if cmp_lt(&a, &b) { a } else { b })
        .ok_or_else(|| "min() arg is an empty sequence".to_string())
}

fn vm_max(args: &[VmValue]) -> Result<VmValue, String> {
    let items = collect_numeric(args, "max")?;
    items.into_iter().reduce(|a, b| if cmp_lt(&a, &b) { b } else { a })
        .ok_or_else(|| "max() arg is an empty sequence".to_string())
}

fn vm_sum(args: &[VmValue]) -> Result<VmValue, String> {
    if args.len() != 1 { return Err(format!("sum() takes 1 argument, got {}", args.len())); }
    let items = match &args[0] {
        VmValue::List(items) => items.as_ref().clone(),
        other => return Err(format!("sum() argument must be a list, got {}", other)),
    };
    let mut int_total = 0i64;
    let mut float_total = 0f64;
    let mut has_float = false;
    for v in items {
        match v {
            VmValue::Integer(n) => int_total += n,
            VmValue::Float(f) => { float_total += f; has_float = true; }
            other => return Err(format!("sum() non-numeric item: {}", other)),
        }
    }
    Ok(if has_float { VmValue::Float(int_total as f64 + float_total) } else { VmValue::Integer(int_total) })
}

fn as_int(v: &VmValue, fname: &str) -> Result<i64, String> {
    match v {
        VmValue::Integer(n) => Ok(*n),
        other => Err(format!("{}() argument must be int, got {}", fname, other)),
    }
}

fn collect_numeric(args: &[VmValue], fname: &str) -> Result<Vec<VmValue>, String> {
    if args.len() == 1 {
        if let VmValue::List(items) = &args[0] {
            return Ok(items.as_ref().clone());
        }
    }
    if args.is_empty() {
        return Err(format!("{}() expected at least 1 argument", fname));
    }
    Ok(args.to_vec())
}

fn cmp_lt(a: &VmValue, b: &VmValue) -> bool {
    match (a, b) {
        (VmValue::Integer(x), VmValue::Integer(y)) => x < y,
        (VmValue::Float(x), VmValue::Float(y)) => x < y,
        (VmValue::Integer(x), VmValue::Float(y)) => (*x as f64) < *y,
        (VmValue::Float(x), VmValue::Integer(y)) => *x < (*y as f64),
        (VmValue::String(x), VmValue::String(y)) => x < y,
        _ => false,
    }
}

pub fn register_python_builtins_vm(vm: &mut VM) {
    let pairs: &[(&str, fn(&[VmValue]) -> Result<VmValue, String>)] = &[
        ("str", vm_str), ("int", vm_int), ("float", vm_float), ("bool", vm_bool),
        ("len", vm_len), ("range", vm_range), ("repr", vm_repr),
        ("abs", vm_abs), ("min", vm_min), ("max", vm_max), ("sum", vm_sum),
    ];
    for (name, func) in pairs {
        let sym = vm.interner_mut().intern(name);
        vm.set_global(sym, VmValue::NativeFunction { name: Rc::new(name.to_string()), func: *func });
    }
}

// ── Interpreter (tree-walking) builtins ─────────────────────────────

fn interp_str(args: &[InterpValue]) -> Result<InterpValue, String> {
    if args.len() != 1 { return Err(format!("str() takes 1 argument, got {}", args.len())); }
    let s = match &args[0] {
        InterpValue::String(s) => s.clone(),
        InterpValue::Integer(n) => n.to_string(),
        InterpValue::Float(f) => if f.fract() == 0.0 { format!("{}.0", f) } else { format!("{}", f) },
        InterpValue::Boolean(b) => if *b { "True".to_string() } else { "False".to_string() },
        InterpValue::None => "None".to_string(),
        other => format!("{}", other),
    };
    Ok(InterpValue::String(s))
}

fn interp_int(args: &[InterpValue]) -> Result<InterpValue, String> {
    if args.len() != 1 { return Err(format!("int() takes 1 argument, got {}", args.len())); }
    match &args[0] {
        InterpValue::Integer(n) => Ok(InterpValue::Integer(*n)),
        InterpValue::Float(f) => Ok(InterpValue::Integer(*f as i64)),
        InterpValue::Boolean(b) => Ok(InterpValue::Integer(if *b { 1 } else { 0 })),
        InterpValue::String(s) => s.trim().parse::<i64>()
            .map(InterpValue::Integer)
            .map_err(|_| format!("int() invalid literal: {:?}", s)),
        other => Err(format!("int() cannot convert {}", other)),
    }
}

fn interp_float(args: &[InterpValue]) -> Result<InterpValue, String> {
    if args.len() != 1 { return Err(format!("float() takes 1 argument, got {}", args.len())); }
    match &args[0] {
        InterpValue::Float(f) => Ok(InterpValue::Float(*f)),
        InterpValue::Integer(n) => Ok(InterpValue::Float(*n as f64)),
        InterpValue::Boolean(b) => Ok(InterpValue::Float(if *b { 1.0 } else { 0.0 })),
        InterpValue::String(s) => s.trim().parse::<f64>()
            .map(InterpValue::Float)
            .map_err(|_| format!("float() invalid literal: {:?}", s)),
        other => Err(format!("float() cannot convert {}", other)),
    }
}

fn interp_bool(args: &[InterpValue]) -> Result<InterpValue, String> {
    if args.is_empty() { return Ok(InterpValue::Boolean(false)); }
    if args.len() != 1 { return Err(format!("bool() takes at most 1 argument, got {}", args.len())); }
    Ok(InterpValue::Boolean(args[0].is_truthy()))
}

fn interp_len(args: &[InterpValue]) -> Result<InterpValue, String> {
    if args.len() != 1 { return Err(format!("len() takes 1 argument, got {}", args.len())); }
    match &args[0] {
        InterpValue::String(s) => Ok(InterpValue::Integer(s.chars().count() as i64)),
        InterpValue::List(items) => Ok(InterpValue::Integer(items.len() as i64)),
        InterpValue::Dict(pairs) => Ok(InterpValue::Integer(pairs.len() as i64)),
        other => Err(format!("len() not supported for {}", other)),
    }
}

fn interp_range(args: &[InterpValue]) -> Result<InterpValue, String> {
    let (start, stop, step) = match args.len() {
        1 => (0i64, as_int_interp(&args[0], "range")?, 1i64),
        2 => (as_int_interp(&args[0], "range")?, as_int_interp(&args[1], "range")?, 1i64),
        3 => {
            let s = as_int_interp(&args[2], "range")?;
            if s == 0 { return Err("range() step cannot be zero".to_string()); }
            (as_int_interp(&args[0], "range")?, as_int_interp(&args[1], "range")?, s)
        }
        n => return Err(format!("range() takes 1-3 arguments, got {}", n)),
    };
    let mut items = Vec::new();
    let mut i = start;
    if step > 0 { while i < stop { items.push(InterpValue::Integer(i)); i += step; } }
    else { while i > stop { items.push(InterpValue::Integer(i)); i += step; } }
    Ok(InterpValue::List(items))
}

fn interp_repr(args: &[InterpValue]) -> Result<InterpValue, String> {
    if args.len() != 1 { return Err(format!("repr() takes 1 argument, got {}", args.len())); }
    let s = match &args[0] {
        InterpValue::String(s) => format!("'{}'", s),
        other => format!("{}", other),
    };
    Ok(InterpValue::String(s))
}

fn interp_abs(args: &[InterpValue]) -> Result<InterpValue, String> {
    if args.len() != 1 { return Err(format!("abs() takes 1 argument, got {}", args.len())); }
    match &args[0] {
        InterpValue::Integer(n) => Ok(InterpValue::Integer(n.wrapping_abs())),
        InterpValue::Float(f) => Ok(InterpValue::Float(f.abs())),
        other => Err(format!("abs() not supported for {}", other)),
    }
}

fn interp_min(args: &[InterpValue]) -> Result<InterpValue, String> {
    let items = collect_numeric_interp(args, "min")?;
    items.into_iter().reduce(|a, b| if cmp_lt_interp(&a, &b) { a } else { b })
        .ok_or_else(|| "min() arg is an empty sequence".to_string())
}

fn interp_max(args: &[InterpValue]) -> Result<InterpValue, String> {
    let items = collect_numeric_interp(args, "max")?;
    items.into_iter().reduce(|a, b| if cmp_lt_interp(&a, &b) { b } else { a })
        .ok_or_else(|| "max() arg is an empty sequence".to_string())
}

fn interp_sum(args: &[InterpValue]) -> Result<InterpValue, String> {
    if args.len() != 1 { return Err(format!("sum() takes 1 argument, got {}", args.len())); }
    let items = match &args[0] {
        InterpValue::List(items) => items.clone(),
        other => return Err(format!("sum() argument must be a list, got {}", other)),
    };
    let mut int_total = 0i64;
    let mut float_total = 0f64;
    let mut has_float = false;
    for v in items {
        match v {
            InterpValue::Integer(n) => int_total += n,
            InterpValue::Float(f) => { float_total += f; has_float = true; }
            other => return Err(format!("sum() non-numeric item: {}", other)),
        }
    }
    Ok(if has_float { InterpValue::Float(int_total as f64 + float_total) } else { InterpValue::Integer(int_total) })
}

fn as_int_interp(v: &InterpValue, fname: &str) -> Result<i64, String> {
    match v {
        InterpValue::Integer(n) => Ok(*n),
        other => Err(format!("{}() argument must be int, got {}", fname, other)),
    }
}

fn collect_numeric_interp(args: &[InterpValue], fname: &str) -> Result<Vec<InterpValue>, String> {
    if args.len() == 1 {
        if let InterpValue::List(items) = &args[0] {
            return Ok(items.clone());
        }
    }
    if args.is_empty() {
        return Err(format!("{}() expected at least 1 argument", fname));
    }
    Ok(args.to_vec())
}

fn cmp_lt_interp(a: &InterpValue, b: &InterpValue) -> bool {
    match (a, b) {
        (InterpValue::Integer(x), InterpValue::Integer(y)) => x < y,
        (InterpValue::Float(x), InterpValue::Float(y)) => x < y,
        (InterpValue::Integer(x), InterpValue::Float(y)) => (*x as f64) < *y,
        (InterpValue::Float(x), InterpValue::Integer(y)) => *x < (*y as f64),
        (InterpValue::String(x), InterpValue::String(y)) => x < y,
        _ => false,
    }
}

pub fn register_python_builtins_interp(interp: &mut Interpreter) {
    let pairs: &[(&str, fn(&[InterpValue]) -> Result<InterpValue, String>)] = &[
        ("str", interp_str), ("int", interp_int), ("float", interp_float), ("bool", interp_bool),
        ("len", interp_len), ("range", interp_range), ("repr", interp_repr),
        ("abs", interp_abs), ("min", interp_min), ("max", interp_max), ("sum", interp_sum),
    ];
    for (name, func) in pairs {
        let sym = interp.interner_mut().intern(name);
        interp.set_global(sym, InterpValue::NativeFunction { name: name.to_string(), func: *func });
    }
}

// Quiet unused-import lint when compiled standalone
#[allow(dead_code)]
fn _check_interner_usable(_i: &Interner) {}
