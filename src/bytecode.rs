use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::symbol::Symbol;

#[derive(Debug, Clone, Copy)]
pub enum Op {
    LoadConst(u16),
    LoadLocal(u16),
    StoreLocal(u16),
    LoadGlobal(Symbol),
    StoreGlobal(Symbol),

    BinaryAdd,
    BinarySub,
    BinaryMul,
    BinaryDiv,
    BinaryFloorDiv,
    BinaryMod,
    BinaryPow,

    CompareEq,
    CompareNotEq,
    CompareLt,
    CompareLtE,
    CompareGt,
    CompareGtE,
    CompareIn,
    CompareNotIn,

    UnaryNeg,
    UnaryNot,

    Jump(u32),
    PopJumpIfFalse(u32),
    JumpIfTrueOrPop(u32),
    JumpIfFalseOrPop(u32),

    Call(u8),
    Return,
    ReturnNone,

    BuildList(u16),
    BuildDict(u16),
    BinarySubscript,
    LoadAttr(Symbol),
    Len,

    ListAppend,

    // Superinstructions — fused hot-path sequences
    // For compare+jump variants: pair = (slot_a as u32) << 16 | (slot_b_or_const as u32)
    /// LoadLocal(slot) + LoadConst(idx) + BinaryAdd + StoreLocal(slot)
    IncrLocalByConst(u32),       // hi16=slot, lo16=const_idx
    /// LoadLocal(a) + LoadLocal(b) + CompareLt + PopJumpIfFalse(target)
    LocalLtLocalJump(u32, u32),  // pair, target
    /// LoadLocal(s) + LoadConst(c) + CompareLtE + PopJumpIfFalse(target)
    LocalLtEConstJump(u32, u32), // pair, target
    /// LoadLocal(a) + LoadLocal(b) — pushes both
    LoadLocalPair(u32),          // pair
    /// LoadLocal(s) + LoadConst(c) + CompareLt + PopJumpIfFalse(target)
    LocalLtConstJump(u32, u32),  // pair, target
    /// LoadLocal(s) + LoadConst(c) + CompareGt + PopJumpIfFalse(target)
    LocalGtConstJump(u32, u32),  // pair, target
    /// LoadGlobal(s) + LoadConst(int) + BinaryAdd + StoreGlobal(s)
    IncrGlobalByConst(Symbol, u16),
    /// Like LoadGlobal but takes (removes) the value, so Rc refcount stays 1
    TakeGlobal(Symbol),
    /// LoadGlobal(func) + Call(argc): avoids pushing func on stack
    CallGlobal(Symbol, u8),

    Pop,
    Print(u8),
}

impl Op {
    #[inline(always)]
    pub fn pack_pair(a: u16, b: u16) -> u32 {
        (a as u32) << 16 | (b as u32)
    }
    #[inline(always)]
    pub fn unpack_pair(packed: u32) -> (u16, u16) {
        ((packed >> 16) as u16, packed as u16)
    }
}

#[derive(Debug, Clone)]
pub struct CodeObject {
    pub instructions: Vec<Op>,
    pub constants: Vec<Value>,
    pub num_locals: u16,
    pub local_slots: HashMap<Symbol, u16>,
}

#[derive(Debug, Clone)]
pub struct FunctionObj {
    pub name: Symbol,
    pub params: Vec<Symbol>,
    pub param_slots: Vec<u16>,
    pub code: Rc<CodeObject>,
}

#[derive(Debug, Clone)]
pub enum Value {
    Integer(i64),
    Float(f64),
    String(Rc<String>),
    Boolean(bool),
    List(Rc<Vec<Value>>),
    Dict(Vec<(Value, Value)>),
    None,
    Function(Rc<FunctionObj>),
    NativeFunction {
        name: Rc<String>,
        func: fn(&[Value]) -> Result<Value, String>,
    },
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Integer(a), Value::Integer(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::List(a), Value::List(b)) => a == b,
            (Value::NativeFunction { name: a, .. }, Value::NativeFunction { name: b, .. }) => a == b,
            (Value::None, Value::None) => true,
            _ => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Integer(n) => write!(f, "{}", n),
            Value::Float(n) => {
                if n.fract() == 0.0 {
                    write!(f, "{}.0", n)
                } else {
                    write!(f, "{}", n)
                }
            }
            Value::String(s) => write!(f, "{}", s),
            Value::Boolean(b) => write!(f, "{}", if *b { "True" } else { "False" }),
            Value::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    match item {
                        Value::String(s) => write!(f, "'{}'", s)?,
                        _ => write!(f, "{}", item)?,
                    }
                }
                write!(f, "]")
            }
            Value::Dict(pairs) => {
                write!(f, "{{")?;
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
            Value::None => write!(f, "None"),
            Value::Function(func) => write!(f, "<function {:?}>", func.name),
            Value::NativeFunction { name, .. } => write!(f, "<native:{}>", name),
        }
    }
}

impl Value {
    #[inline(always)]
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Boolean(b) => *b,
            Value::Integer(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::List(l) => !l.is_empty(),
            Value::Dict(d) => !d.is_empty(),
            Value::None => false,
            Value::Function(_) => true,
            Value::NativeFunction { .. } => true,
        }
    }
}
