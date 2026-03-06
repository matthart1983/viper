use std::rc::Rc;

use rustc_hash::FxHashMap;

use crate::bytecode::{CodeObject, FunctionObj, Op, Value};
use crate::symbol::{Interner, Symbol};

struct SavedFrame {
    code: *const CodeObject,
    pc: usize,
    locals_base: usize,
}

pub struct VM {
    stack: Vec<Value>,
    // Current frame — flattened into VM fields to avoid indirection per instruction
    code: *const CodeObject,
    pc: usize,
    // Unified locals stack — all frames' locals are contiguous
    locals_stack: Vec<Value>,
    locals_base: usize,
    // Keeps module code alive while raw pointer is used
    _module_code: Option<Rc<CodeObject>>,
    // Call stack (saved frames)
    saved_frames: Vec<SavedFrame>,
    // FxHashMap: fast integer-keyed hash for Symbol(u32)
    globals: FxHashMap<Symbol, Value>,
    interner: Interner,
    output: Vec<String>,
    suppress_output: bool,
}

impl VM {
    pub fn new(interner: Interner) -> Self {
        VM {
            stack: Vec::with_capacity(256),
            code: std::ptr::null(),
            pc: 0,
            locals_stack: Vec::with_capacity(256),
            locals_base: 0,
            _module_code: None,
            saved_frames: Vec::with_capacity(64),
            globals: FxHashMap::default(),
            interner,
            output: Vec::new(),
            suppress_output: false,
        }
    }

    pub fn interner_mut(&mut self) -> &mut Interner {
        &mut self.interner
    }

    pub fn set_suppress_output(&mut self, suppress: bool) {
        self.suppress_output = suppress;
    }

    pub fn get_output(&self) -> &[String] {
        &self.output
    }

    #[inline(always)]
    fn push_locals(&mut self, size: usize) -> usize {
        let base = self.locals_stack.len();
        self.locals_stack.resize(base + size, Value::None);
        base
    }

    fn pop_locals(&mut self, base: usize) {
        self.locals_stack.truncate(base);
    }

    pub fn run(&mut self, code: &CodeObject) -> Result<(), String> {
        let code = Rc::new(code.clone());
        let num_locals = code.num_locals as usize;
        self.locals_base = self.push_locals(num_locals);
        self.code = &*code as *const CodeObject;
        self._module_code = Some(code);
        self.pc = 0;

        match self.execute() {
            Ok(()) => Ok(()),
            Err(e) => Err(e),
        }
    }

    #[inline(always)]
    fn code(&self) -> &CodeObject {
        unsafe { &*self.code }
    }

    #[inline(always)]
    fn stack_pop(&mut self) -> Value {
        unsafe { self.stack.pop().unwrap_unchecked() }
    }

    #[inline(always)]
    fn do_call(&mut self, func_obj: &Rc<FunctionObj>, argc: usize, stack_base: usize) -> Result<(), String> {
        let num_locals = func_obj.code.num_locals as usize;
        let new_base = self.push_locals(num_locals);

        // Move args directly from stack into local slots
        let args_start = self.stack.len() - argc;
        for i in 0..argc {
            let slot = func_obj.param_slots[i] as usize;
            self.locals_stack[new_base + slot] = std::mem::replace(&mut self.stack[args_start + i], Value::None);
        }

        // Remove args (and possibly func) from stack
        self.stack.truncate(stack_base);

        // Save current frame and switch to new one
        let old_code = std::mem::replace(&mut self.code, &*func_obj.code as *const CodeObject);
        let old_pc = std::mem::replace(&mut self.pc, 0);
        let old_base = std::mem::replace(&mut self.locals_base, new_base);
        self.saved_frames.push(SavedFrame {
            code: old_code,
            pc: old_pc,
            locals_base: old_base,
        });
        Ok(())
    }

    fn execute(&mut self) -> Result<(), String> {
        loop {
            if self.pc >= self.code().instructions.len() {
                if self.saved_frames.is_empty() {
                    return Ok(());
                }
                // Implicit return None
                self.pop_locals(self.locals_base);
                let frame = self.saved_frames.pop().unwrap();
                self.code = frame.code;
                self.pc = frame.pc;
                self.locals_base = frame.locals_base;
                self.stack.push(Value::None);
                continue;
            }

            // Op is Copy — no clone, just a cheap memcpy of ~8 bytes
            let op = self.code().instructions[self.pc];
            self.pc += 1;

            match op {
                Op::LoadConst(idx) => {
                    self.stack.push(self.code().constants[idx as usize].clone());
                }
                Op::LoadLocal(slot) => {
                    self.stack.push(self.locals_stack[self.locals_base + slot as usize].clone());
                }
                Op::StoreLocal(slot) => {
                    self.locals_stack[self.locals_base + slot as usize] = self.stack_pop();
                }
                Op::LoadGlobal(sym) => {
                    let val = self.globals.get(&sym).cloned().ok_or_else(|| {
                        format!("Undefined variable: {}", self.interner.resolve(sym))
                    })?;
                    self.stack.push(val);
                }
                Op::StoreGlobal(sym) => {
                    let val = self.stack_pop();
                    self.globals.insert(sym, val);
                }

                // Arithmetic — inlined integer fast paths
                Op::BinaryAdd => {
                    let right = self.stack_pop();
                    let left = self.stack_pop();
                    let result = match (left, right) {
                        (Value::Integer(a), Value::Integer(b)) => Value::Integer(a + b),
                        (Value::Float(a), Value::Float(b)) => Value::Float(a + b),
                        (Value::Integer(a), Value::Float(b)) => Value::Float(a as f64 + b),
                        (Value::Float(a), Value::Integer(b)) => Value::Float(a + b as f64),
                        (Value::String(a), Value::String(b)) => {
                            // Reuse left buffer if sole owner (common in s = s + "x")
                            match Rc::try_unwrap(a) {
                                Ok(mut s) => {
                                    s.push_str(&b);
                                    Value::String(Rc::new(s))
                                }
                                Err(a) => {
                                    let mut s = String::with_capacity(a.len() + b.len());
                                    s.push_str(&a);
                                    s.push_str(&b);
                                    Value::String(Rc::new(s))
                                }
                            }
                        }
                        (Value::List(a), Value::List(b)) => {
                            match Rc::try_unwrap(a) {
                                Ok(mut v) => {
                                    v.extend_from_slice(&b);
                                    Value::List(Rc::new(v))
                                }
                                Err(a) => {
                                    let mut r = Vec::with_capacity(a.len() + b.len());
                                    r.extend_from_slice(&a);
                                    r.extend_from_slice(&b);
                                    Value::List(Rc::new(r))
                                }
                            }
                        }
                        (left, right) => return Err(format!("Unsupported + for {:?} and {:?}", left, right)),
                    };
                    self.stack.push(result);
                }
                Op::BinarySub => {
                    let right = self.stack_pop();
                    let left = self.stack_pop();
                    let result = match (left, right) {
                        (Value::Integer(a), Value::Integer(b)) => Value::Integer(a - b),
                        (Value::Float(a), Value::Float(b)) => Value::Float(a - b),
                        (Value::Integer(a), Value::Float(b)) => Value::Float(a as f64 - b),
                        (Value::Float(a), Value::Integer(b)) => Value::Float(a - b as f64),
                        _ => return Err("Unsupported -".to_string()),
                    };
                    self.stack.push(result);
                }
                Op::BinaryMul => {
                    let right = self.stack_pop();
                    let left = self.stack_pop();
                    self.stack.push(match (&left, &right) {
                        (Value::Integer(a), Value::Integer(b)) => Value::Integer(a * b),
                        (Value::Float(a), Value::Float(b)) => Value::Float(a * b),
                        (Value::Integer(a), Value::Float(b)) => Value::Float(*a as f64 * b),
                        (Value::Float(a), Value::Integer(b)) => Value::Float(a * *b as f64),
                        (Value::String(a), Value::Integer(b)) => {
                            Value::String(Rc::new(a.repeat(*b as usize)))
                        }
                        _ => return Err("Unsupported *".to_string()),
                    });
                }
                Op::BinaryDiv => {
                    let right = self.stack_pop();
                    let left = self.stack_pop();
                    self.stack.push(match (&left, &right) {
                        (Value::Integer(a), Value::Integer(b)) => {
                            if *b == 0 {
                                return Err("Division by zero".to_string());
                            }
                            Value::Float(*a as f64 / *b as f64)
                        }
                        (Value::Float(a), Value::Float(b)) => {
                            if *b == 0.0 {
                                return Err("Division by zero".to_string());
                            }
                            Value::Float(a / b)
                        }
                        (Value::Integer(a), Value::Float(b)) => Value::Float(*a as f64 / b),
                        (Value::Float(a), Value::Integer(b)) => Value::Float(a / *b as f64),
                        _ => return Err("Unsupported /".to_string()),
                    });
                }
                Op::BinaryFloorDiv => {
                    let right = self.stack_pop();
                    let left = self.stack_pop();
                    match (&left, &right) {
                        (Value::Integer(a), Value::Integer(b)) => {
                            if *b == 0 {
                                return Err("Division by zero".to_string());
                            }
                            self.stack.push(Value::Integer(a / b));
                        }
                        _ => return Err("Unsupported //".to_string()),
                    }
                }
                Op::BinaryMod => {
                    let right = self.stack_pop();
                    let left = self.stack_pop();
                    match (&left, &right) {
                        (Value::Integer(a), Value::Integer(b)) => {
                            if *b == 0 {
                                return Err("Modulo by zero".to_string());
                            }
                            self.stack.push(Value::Integer(((a % b) + b) % b));
                        }
                        _ => return Err("Unsupported %".to_string()),
                    }
                }
                Op::BinaryPow => {
                    let right = self.stack_pop();
                    let left = self.stack_pop();
                    match (&left, &right) {
                        (Value::Integer(a), Value::Integer(b)) => {
                            self.stack.push(Value::Integer(a.pow(*b as u32)));
                        }
                        _ => return Err("Unsupported **".to_string()),
                    }
                }

                // Comparisons — inlined integer fast path
                Op::CompareEq => {
                    let right = self.stack_pop();
                    let left = self.stack_pop();
                    self.stack.push(Value::Boolean(left == right));
                }
                Op::CompareNotEq => {
                    let right = self.stack_pop();
                    let left = self.stack_pop();
                    self.stack.push(Value::Boolean(left != right));
                }
                Op::CompareLt => {
                    let right = self.stack_pop();
                    let left = self.stack_pop();
                    self.stack.push(Value::Boolean(match (&left, &right) {
                        (Value::Integer(a), Value::Integer(b)) => a < b,
                        (Value::Float(a), Value::Float(b)) => a < b,
                        (Value::String(a), Value::String(b)) => a < b,
                        _ => false,
                    }));
                }
                Op::CompareLtE => {
                    let right = self.stack_pop();
                    let left = self.stack_pop();
                    self.stack.push(Value::Boolean(match (&left, &right) {
                        (Value::Integer(a), Value::Integer(b)) => a <= b,
                        (Value::Float(a), Value::Float(b)) => a <= b,
                        _ => false,
                    }));
                }
                Op::CompareGt => {
                    let right = self.stack_pop();
                    let left = self.stack_pop();
                    self.stack.push(Value::Boolean(match (&left, &right) {
                        (Value::Integer(a), Value::Integer(b)) => a > b,
                        (Value::Float(a), Value::Float(b)) => a > b,
                        (Value::String(a), Value::String(b)) => a > b,
                        _ => false,
                    }));
                }
                Op::CompareGtE => {
                    let right = self.stack_pop();
                    let left = self.stack_pop();
                    self.stack.push(Value::Boolean(match (&left, &right) {
                        (Value::Integer(a), Value::Integer(b)) => a >= b,
                        (Value::Float(a), Value::Float(b)) => a >= b,
                        _ => false,
                    }));
                }
                Op::CompareIn => {
                    let right = self.stack_pop();
                    let left = self.stack_pop();
                    match &right {
                        Value::List(items) => {
                            self.stack.push(Value::Boolean(items.contains(&left)));
                        }
                        Value::String(s) => {
                            if let Value::String(sub) = &left {
                                self.stack.push(Value::Boolean(s.contains(&**sub)));
                            } else {
                                return Err("Invalid 'in' operand".to_string());
                            }
                        }
                        _ => return Err("Invalid 'in' operand".to_string()),
                    }
                }
                Op::CompareNotIn => {
                    let right = self.stack_pop();
                    let left = self.stack_pop();
                    match &right {
                        Value::List(items) => {
                            self.stack.push(Value::Boolean(!items.contains(&left)));
                        }
                        _ => return Err("Invalid 'not in' operand".to_string()),
                    }
                }

                // Unary — modify in place when possible
                Op::UnaryNeg => {
                    let val = self.stack_pop();
                    self.stack.push(match val {
                        Value::Integer(n) => Value::Integer(-n),
                        Value::Float(f) => Value::Float(-f),
                        _ => return Err("Cannot negate".to_string()),
                    });
                }
                Op::UnaryNot => {
                    let val = self.stack_pop();
                    self.stack.push(Value::Boolean(!val.is_truthy()));
                }

                // Control flow
                Op::Jump(target) => {
                    self.pc = target as usize;
                }
                Op::PopJumpIfFalse(target) => {
                    let val = self.stack_pop();
                    if !val.is_truthy() {
                        self.pc = target as usize;
                    }
                }
                Op::JumpIfTrueOrPop(target) => {
                    if self.stack.last().unwrap().is_truthy() {
                        self.pc = target as usize;
                    } else {
                        self.stack.pop();
                    }
                }
                Op::JumpIfFalseOrPop(target) => {
                    if !self.stack.last().unwrap().is_truthy() {
                        self.pc = target as usize;
                    } else {
                        self.stack.pop();
                    }
                }

                // Function calls — optimized frame management
                Op::Call(argc) => {
                    let argc = argc as usize;
                    let args_start = self.stack.len() - argc;
                    let func_idx = args_start - 1;

                    // Take the function value instead of cloning — we'll truncate anyway
                    let func_val = std::mem::replace(&mut self.stack[func_idx], Value::None);

                    match func_val {
                        Value::Function(func_obj) => {
                            self.do_call(&func_obj, argc, func_idx)?;
                        }
                        _ => return Err(format!("{} is not callable", func_val)),
                    }
                }
                Op::CallGlobal(sym, argc) => {
                    let argc = argc as usize;
                    // Get the code+params directly via raw pointers — no Rc clone
                    let (code_ptr, param_slots_ptr, num_locals) = match self.globals.get(&sym) {
                        Some(Value::Function(f)) => (
                            &*f.code as *const CodeObject,
                            f.param_slots.as_ptr(),
                            f.code.num_locals as usize,
                        ),
                        Some(other) => return Err(format!("{} is not callable", other)),
                        None => return Err(format!("Undefined: {}", self.interner.resolve(sym))),
                    };

                    let new_base = self.push_locals(num_locals);
                    let args_start = self.stack.len() - argc;
                    // SAFETY: param_slots_ptr points into the Rc<FunctionObj> in globals
                    for i in 0..argc {
                        let slot = unsafe { *param_slots_ptr.add(i) } as usize;
                        self.locals_stack[new_base + slot] = std::mem::replace(&mut self.stack[args_start + i], Value::None);
                    }
                    self.stack.truncate(args_start);

                    let old_code = std::mem::replace(&mut self.code, code_ptr);
                    let old_pc = std::mem::replace(&mut self.pc, 0);
                    let old_base = std::mem::replace(&mut self.locals_base, new_base);
                    self.saved_frames.push(SavedFrame {
                        code: old_code,
                        pc: old_pc,
                        locals_base: old_base,
                    });
                }
                Op::Return => {
                    let val = self.stack_pop();
                    self.pop_locals(self.locals_base);
                    let frame = unsafe { self.saved_frames.pop().unwrap_unchecked() };
                    self.code = frame.code;
                    self.pc = frame.pc;
                    self.locals_base = frame.locals_base;
                    self.stack.push(val);
                    if self.saved_frames.is_empty() && self.pc >= self.code().instructions.len() {
                        return Ok(());
                    }
                }
                Op::ReturnNone => {
                    self.pop_locals(self.locals_base);
                    if let Some(frame) = self.saved_frames.pop() {
                        self.code = frame.code;
                        self.pc = frame.pc;
                        self.locals_base = frame.locals_base;
                        self.stack.push(Value::None);
                    } else {
                        return Ok(());
                    }
                }

                // Collections
                Op::BuildList(n) => {
                    let n = n as usize;
                    let start = self.stack.len() - n;
                    let items: Vec<Value> = self.stack.drain(start..).collect();
                    self.stack.push(Value::List(Rc::new(items)));
                }
                Op::BuildDict(n) => {
                    let n = n as usize;
                    let start = self.stack.len() - n * 2;
                    let flat: Vec<Value> = self.stack.drain(start..).collect();
                    let mut pairs = Vec::with_capacity(n);
                    for chunk in flat.chunks(2) {
                        pairs.push((chunk[0].clone(), chunk[1].clone()));
                    }
                    self.stack.push(Value::Dict(pairs));
                }
                Op::BinarySubscript => {
                    let index = self.stack_pop();
                    let object = self.stack_pop();
                    match (&object, &index) {
                        (Value::List(items), Value::Integer(i)) => {
                            let idx = if *i < 0 {
                                (items.len() as i64 + i) as usize
                            } else {
                                *i as usize
                            };
                            self.stack.push(
                                items.get(idx).cloned().ok_or("Index out of range")?,
                            );
                        }
                        (Value::String(s), Value::Integer(i)) => {
                            let idx = if *i < 0 {
                                (s.len() as i64 + i) as usize
                            } else {
                                *i as usize
                            };
                            self.stack.push(
                                s.chars()
                                    .nth(idx)
                                    .map(|c| Value::String(Rc::new(c.to_string())))
                                    .ok_or("Index out of range")?,
                            );
                        }
                        _ => return Err("Invalid index operation".to_string()),
                    }
                }
                Op::LoadAttr(sym) => {
                    let object = self.stack_pop();
                    let attr = self.interner.resolve(sym);
                    match (&object, attr) {
                        (Value::List(items), "len") => {
                            self.stack.push(Value::Integer(items.len() as i64));
                        }
                        (Value::String(s), "len") => {
                            self.stack.push(Value::Integer(s.len() as i64));
                        }
                        _ => {
                            return Err(format!("No attribute '{}' on {:?}", attr, object));
                        }
                    }
                }
                Op::Len => {
                    let val = self.stack_pop();
                    self.stack.push(match &val {
                        Value::List(items) => Value::Integer(items.len() as i64),
                        Value::String(s) => Value::Integer(s.len() as i64),
                        _ => return Err("Object has no len".to_string()),
                    });
                }

                // Superinstructions
                Op::IncrLocalByConst(packed) => {
                    let (slot16, const_idx) = Op::unpack_pair(packed);
                    let slot = slot16 as usize;
                    // Read constant via raw pointer to avoid borrow conflict
                    let constants = unsafe { &(*self.code).constants };
                    if let Value::Integer(ref mut n) = self.locals_stack[self.locals_base + slot] {
                        if let Value::Integer(inc) = constants[const_idx as usize] {
                            *n += inc;
                            continue;
                        }
                    }
                    let val = self.locals_stack[self.locals_base + slot].clone();
                    let inc = constants[const_idx as usize].clone();
                    match (&val, &inc) {
                        (Value::Integer(a), Value::Integer(b)) => {
                            self.locals_stack[self.locals_base + slot] = Value::Integer(a + b);
                        }
                        _ => return Err("IncrLocalByConst: non-integer".to_string()),
                    }
                }
                Op::LocalLtLocalJump(packed, target) => {
                    let (a, b) = Op::unpack_pair(packed);
                    let va = &self.locals_stack[self.locals_base + a as usize];
                    let vb = &self.locals_stack[self.locals_base + b as usize];
                    let result = match (va, vb) {
                        (Value::Integer(x), Value::Integer(y)) => *x < *y,
                        _ => false,
                    };
                    if !result {
                        self.pc = target as usize;
                    }
                }
                Op::LocalLtEConstJump(packed, target) => {
                    let (slot, const_idx) = Op::unpack_pair(packed);
                    let val = &self.locals_stack[self.locals_base + slot as usize];
                    let cst = &self.code().constants[const_idx as usize];
                    let result = match (val, cst) {
                        (Value::Integer(a), Value::Integer(b)) => *a <= *b,
                        _ => false,
                    };
                    if !result {
                        self.pc = target as usize;
                    }
                }
                Op::LocalLtConstJump(packed, target) => {
                    let (slot, const_idx) = Op::unpack_pair(packed);
                    let val = &self.locals_stack[self.locals_base + slot as usize];
                    let cst = &self.code().constants[const_idx as usize];
                    let result = match (val, cst) {
                        (Value::Integer(a), Value::Integer(b)) => *a < *b,
                        _ => false,
                    };
                    if !result {
                        self.pc = target as usize;
                    }
                }
                Op::LocalGtConstJump(packed, target) => {
                    let (slot, const_idx) = Op::unpack_pair(packed);
                    let val = &self.locals_stack[self.locals_base + slot as usize];
                    let cst = &self.code().constants[const_idx as usize];
                    let result = match (val, cst) {
                        (Value::Integer(a), Value::Integer(b)) => *a > *b,
                        _ => false,
                    };
                    if !result {
                        self.pc = target as usize;
                    }
                }
                Op::IncrGlobalByConst(sym, const_idx) => {
                    let constants = unsafe { &(*self.code).constants };
                    if let Some(Value::Integer(ref mut n)) = self.globals.get_mut(&sym) {
                        if let Value::Integer(inc) = constants[const_idx as usize] {
                            *n += inc;
                            continue;
                        }
                    }
                    let val = self.globals.get(&sym).cloned().unwrap_or(Value::None);
                    let inc = constants[const_idx as usize].clone();
                    match (&val, &inc) {
                        (Value::Integer(a), Value::Integer(b)) => {
                            self.globals.insert(sym, Value::Integer(a + b));
                        }
                        _ => return Err("IncrGlobalByConst: non-integer".to_string()),
                    }
                }
                Op::LoadLocalPair(packed) => {
                    let (a, b) = Op::unpack_pair(packed);
                    self.stack.push(self.locals_stack[self.locals_base + a as usize].clone());
                    self.stack.push(self.locals_stack[self.locals_base + b as usize].clone());
                }

                Op::TakeGlobal(sym) => {
                    let val = self.globals.remove(&sym).ok_or_else(|| {
                        format!("Undefined variable: {}", self.interner.resolve(sym))
                    })?;
                    self.stack.push(val);
                }

                Op::ListAppend => {
                    let value = self.stack_pop();
                    let list = self.stack_pop();
                    match list {
                        Value::List(mut items) => {
                            Rc::make_mut(&mut items).push(value);
                            self.stack.push(Value::List(items));
                        }
                        _ => return Err("ListAppend on non-list".to_string()),
                    }
                }

                Op::Pop => {
                    self.stack.pop();
                }

                Op::Print(n) => {
                    let n = n as usize;
                    let start = self.stack.len() - n;
                    let args: Vec<Value> = self.stack.drain(start..).collect();
                    let line: String =
                        args.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ");
                    if !self.suppress_output {
                        println!("{}", line);
                    }
                    self.output.push(line);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile_module;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn run(code: &str) -> Vec<String> {
        let mut interner = Interner::new();
        let mut lexer = Lexer::new(code);
        let tokens = lexer.tokenize().unwrap();
        let stmts = {
            let mut parser = Parser::new(tokens, &mut interner);
            parser.parse().unwrap()
        };
        let code_obj = compile_module(&stmts, &mut interner);
        let mut vm = VM::new(interner);
        vm.run(&code_obj).unwrap();
        vm.output.clone()
    }

    #[test]
    fn test_arithmetic() {
        assert_eq!(run("print(2 + 3)\n"), vec!["5"]);
        assert_eq!(run("print(10 - 4)\n"), vec!["6"]);
        assert_eq!(run("print(3 * 7)\n"), vec!["21"]);
        assert_eq!(run("print(2 ** 10)\n"), vec!["1024"]);
    }

    #[test]
    fn test_variables() {
        assert_eq!(run("x = 10\ny = 20\nprint(x + y)\n"), vec!["30"]);
    }

    #[test]
    fn test_function() {
        assert_eq!(
            run("def add(a, b):\n    return a + b\nprint(add(3, 4))\n"),
            vec!["7"]
        );
    }

    #[test]
    fn test_if_else() {
        assert_eq!(
            run("x = 10\nif x > 5:\n    print(\"big\")\nelse:\n    print(\"small\")\n"),
            vec!["big"]
        );
    }

    #[test]
    fn test_while_loop() {
        assert_eq!(
            run("i = 0\nresult = 0\nwhile i < 5:\n    result += i\n    i += 1\nprint(result)\n"),
            vec!["10"]
        );
    }

    #[test]
    fn test_for_loop() {
        assert_eq!(
            run("total = 0\nfor x in [1, 2, 3, 4, 5]:\n    total += x\nprint(total)\n"),
            vec!["15"]
        );
    }

    #[test]
    fn test_string_ops() {
        assert_eq!(
            run("print(\"hello\" + \" \" + \"world\")\n"),
            vec!["hello world"]
        );
        assert_eq!(run("print(\"ha\" * 3)\n"), vec!["hahaha"]);
    }

    #[test]
    fn test_fibonacci() {
        assert_eq!(
            run("def fib(n):\n    if n <= 1:\n        return n\n    return fib(n - 1) + fib(n - 2)\nprint(fib(10))\n"),
            vec!["55"]
        );
    }

    #[test]
    fn test_break_continue() {
        assert_eq!(
            run("i = 0\ntotal = 0\nwhile i < 10:\n    i += 1\n    if i == 5:\n        break\n    total += i\nprint(total)\n"),
            vec!["10"]
        );
    }

    #[test]
    fn test_elif() {
        assert_eq!(
            run("x = 5\nif x > 10:\n    print(\"big\")\nelif x > 3:\n    print(\"mid\")\nelse:\n    print(\"small\")\n"),
            vec!["mid"]
        );
    }
}
