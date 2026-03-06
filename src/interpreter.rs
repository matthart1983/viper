use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::ast::*;
use crate::symbol::{Interner, Symbol};

/// Shared function object — cloning is a pointer bump.
/// `local_slots` maps each local variable (params + assignments) to a Vec index.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionObj {
    pub name: Symbol,
    pub params: Vec<Symbol>,
    pub body: Vec<Stmt>,
    pub local_slots: HashMap<Symbol, usize>,
    pub num_locals: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    List(Vec<Value>),
    Dict(Vec<(Value, Value)>),
    None,
    Function(Rc<FunctionObj>),
    NativeFunction {
        name: String,
        func: fn(&[Value]) -> Result<Value, String>,
    },
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
    #[inline]
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

#[derive(Debug)]
enum ControlFlow {
    Return(Value),
    Break,
    Continue,
}

/// A single function call frame with slot-indexed locals.
struct CallFrame {
    locals: Vec<Option<Value>>,
    func: Rc<FunctionObj>,
}

/// Separated globals + slot-based local frames.
/// Lookup: check current frame slots → globals. O(1) in both cases.
pub struct Environment {
    globals: HashMap<Symbol, Value>,
    call_stack: Vec<CallFrame>,
}

impl Environment {
    pub fn new() -> Self {
        Environment {
            globals: HashMap::new(),
            call_stack: Vec::new(),
        }
    }

    pub fn push_frame(&mut self, func: Rc<FunctionObj>) {
        let num_locals = func.num_locals;
        self.call_stack.push(CallFrame {
            locals: vec![Option::None; num_locals],
            func,
        });
    }

    pub fn pop_frame(&mut self) {
        self.call_stack.pop();
    }

    /// Get a variable: check local slot (if in a function) → globals.
    #[inline]
    pub fn get(&self, name: Symbol) -> Option<&Value> {
        if let Some(frame) = self.call_stack.last() {
            if let Some(&slot) = frame.func.local_slots.get(&name) {
                if let Some(val) = &frame.locals[slot] {
                    return Some(val);
                }
                // Local slot exists but uninitialized — fall through to globals
                // (handles reading globals like function names from inside a function)
            }
        }
        self.globals.get(&name)
    }

    /// Set a variable: if in a function and name has a local slot, use the slot.
    /// Otherwise set as global.
    #[inline]
    pub fn set(&mut self, name: Symbol, value: Value) {
        if let Some(frame) = self.call_stack.last_mut() {
            if let Some(&slot) = frame.func.local_slots.get(&name) {
                frame.locals[slot] = Some(value);
                return;
            }
        }
        self.globals.insert(name, value);
    }

    /// Set directly into globals (for function defs).
    pub fn set_global(&mut self, name: Symbol, value: Value) {
        self.globals.insert(name, value);
    }
}

/// Scan a function body to find all locally-assigned variable names,
/// and assign each a slot index. Params get slots 0..n.
fn collect_locals(params: &[Symbol], body: &[Stmt]) -> (HashMap<Symbol, usize>, usize) {
    let mut slots = HashMap::new();
    let mut idx = 0;
    for p in params {
        slots.insert(*p, idx);
        idx += 1;
    }
    scan_assigns(body, &mut slots, &mut idx);
    (slots, idx)
}

fn scan_assigns(stmts: &[Stmt], slots: &mut HashMap<Symbol, usize>, idx: &mut usize) {
    for stmt in stmts {
        match stmt {
            Stmt::Assign { target, .. } | Stmt::AugAssign { target, .. } => {
                slots.entry(*target).or_insert_with(|| {
                    let i = *idx;
                    *idx += 1;
                    i
                });
            }
            Stmt::For { target, body, .. } => {
                slots.entry(*target).or_insert_with(|| {
                    let i = *idx;
                    *idx += 1;
                    i
                });
                scan_assigns(body, slots, idx);
            }
            Stmt::If {
                body,
                elif_clauses,
                else_body,
                ..
            } => {
                scan_assigns(body, slots, idx);
                for (_, elif_body) in elif_clauses {
                    scan_assigns(elif_body, slots, idx);
                }
                if let Some(else_b) = else_body {
                    scan_assigns(else_b, slots, idx);
                }
            }
            Stmt::While { body, .. } => {
                scan_assigns(body, slots, idx);
            }
            // Don't recurse into nested FunctionDef — separate scope
            _ => {}
        }
    }
}

pub struct Interpreter {
    env: Environment,
    interner: Interner,
    output: Vec<String>,
    suppress_output: bool,
}

impl Interpreter {
    pub fn new(interner: Interner) -> Self {
        Interpreter {
            env: Environment::new(),
            interner,
            output: Vec::new(),
            suppress_output: false,
        }
    }

    pub fn interner_mut(&mut self) -> &mut Interner {
        &mut self.interner
    }

    pub fn set_global(&mut self, name: Symbol, value: Value) {
        self.env.set_global(name, value);
    }

    pub fn set_suppress_output(&mut self, suppress: bool) {
        self.suppress_output = suppress;
    }

    pub fn run(&mut self, stmts: &[Stmt]) -> Result<(), String> {
        for stmt in stmts {
            if let Some(cf) = self.exec_stmt(stmt)? {
                match cf {
                    ControlFlow::Return(_) => {
                        return Err("'return' outside function".to_string());
                    }
                    ControlFlow::Break => {
                        return Err("'break' outside loop".to_string());
                    }
                    ControlFlow::Continue => {
                        return Err("'continue' outside loop".to_string());
                    }
                }
            }
        }
        Ok(())
    }

    pub fn get_output(&self) -> &[String] {
        &self.output
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> Result<Option<ControlFlow>, String> {
        match stmt {
            Stmt::Expression(expr) => {
                self.eval_expr(expr)?;
                Ok(None)
            }
            Stmt::Assign { target, value } => {
                let val = self.eval_expr(value)?;
                self.env.set(*target, val);
                Ok(None)
            }
            Stmt::AugAssign { target, op, value } => {
                let current = self
                    .env
                    .get(*target)
                    .cloned()
                    .ok_or_else(|| {
                        format!("Undefined variable: {}", self.interner.resolve(*target))
                    })?;
                let rhs = self.eval_expr(value)?;
                let result = self.apply_binop(op, &current, &rhs)?;
                self.env.set(*target, result);
                Ok(None)
            }
            Stmt::Print(args) => {
                let values: Result<Vec<String>, _> = args
                    .iter()
                    .map(|a| self.eval_expr(a).map(|v| v.to_string()))
                    .collect();
                let line = values?.join(" ");
                if !self.suppress_output {
                    println!("{}", line);
                }
                self.output.push(line);
                Ok(None)
            }
            Stmt::If {
                condition,
                body,
                elif_clauses,
                else_body,
            } => {
                let cond = self.eval_expr(condition)?;
                if cond.is_truthy() {
                    return self.exec_block(body);
                }
                for (elif_cond, elif_body) in elif_clauses {
                    let c = self.eval_expr(elif_cond)?;
                    if c.is_truthy() {
                        return self.exec_block(elif_body);
                    }
                }
                if let Some(else_b) = else_body {
                    return self.exec_block(else_b);
                }
                Ok(None)
            }
            Stmt::While { condition, body } => loop {
                let cond = self.eval_expr(condition)?;
                if !cond.is_truthy() {
                    break Ok(None);
                }
                match self.exec_block(body)? {
                    Some(ControlFlow::Break) => break Ok(None),
                    Some(ControlFlow::Continue) => continue,
                    Some(cf @ ControlFlow::Return(_)) => break Ok(Some(cf)),
                    None => {}
                }
            },
            Stmt::For { target, iter, body } => {
                let iterable = self.eval_expr(iter)?;
                let items = match iterable {
                    Value::List(items) => items,
                    Value::String(s) => {
                        s.chars().map(|c| Value::String(c.to_string())).collect()
                    }
                    _ => return Err("Cannot iterate over non-iterable".to_string()),
                };

                for item in items {
                    self.env.set(*target, item);
                    match self.exec_block(body)? {
                        Some(ControlFlow::Break) => break,
                        Some(ControlFlow::Continue) => continue,
                        Some(cf @ ControlFlow::Return(_)) => return Ok(Some(cf)),
                        None => {}
                    }
                }
                Ok(None)
            }
            Stmt::FunctionDef { name, params, body } => {
                let (local_slots, num_locals) = collect_locals(params, body);
                let func = Value::Function(Rc::new(FunctionObj {
                    name: *name,
                    params: params.clone(),
                    body: body.clone(),
                    local_slots,
                    num_locals,
                }));
                self.env.set_global(*name, func);
                Ok(None)
            }
            Stmt::Return(expr) => {
                let val = match expr {
                    Some(e) => self.eval_expr(e)?,
                    None => Value::None,
                };
                Ok(Some(ControlFlow::Return(val)))
            }
            Stmt::Break => Ok(Some(ControlFlow::Break)),
            Stmt::Continue => Ok(Some(ControlFlow::Continue)),
            Stmt::Pass => Ok(None),
        }
    }

    fn exec_block(&mut self, stmts: &[Stmt]) -> Result<Option<ControlFlow>, String> {
        for stmt in stmts {
            if let Some(cf) = self.exec_stmt(stmt)? {
                return Ok(Some(cf));
            }
        }
        Ok(None)
    }

    fn eval_expr(&mut self, expr: &Expr) -> Result<Value, String> {
        match expr {
            Expr::Integer(n) => Ok(Value::Integer(*n)),
            Expr::Float(f) => Ok(Value::Float(*f)),
            Expr::StringLiteral(s) => Ok(Value::String(s.clone())),
            Expr::Boolean(b) => Ok(Value::Boolean(*b)),
            Expr::None => Ok(Value::None),
            Expr::Identifier(name) => self
                .env
                .get(*name)
                .cloned()
                .ok_or_else(|| {
                    format!("Undefined variable: {}", self.interner.resolve(*name))
                }),
            Expr::BinaryOp { left, op, right } => {
                let l = self.eval_expr(left)?;
                let r = self.eval_expr(right)?;
                self.apply_binop(op, &l, &r)
            }
            Expr::UnaryOp { op, operand } => {
                let val = self.eval_expr(operand)?;
                match op {
                    UnaryOp::Neg => match val {
                        Value::Integer(n) => Ok(Value::Integer(-n)),
                        Value::Float(f) => Ok(Value::Float(-f)),
                        _ => Err("Cannot negate non-numeric value".to_string()),
                    },
                    UnaryOp::Not => Ok(Value::Boolean(!val.is_truthy())),
                }
            }
            Expr::Call { function, args } => {
                let func = self.eval_expr(function)?;
                let mut eval_args = Vec::with_capacity(args.len());
                for arg in args {
                    eval_args.push(self.eval_expr(arg)?);
                }
                self.call_function(&func, eval_args)
            }
            Expr::Index { object, index } => {
                let obj = self.eval_expr(object)?;
                let idx = self.eval_expr(index)?;
                match (&obj, &idx) {
                    (Value::List(items), Value::Integer(i)) => {
                        let index = if *i < 0 {
                            (items.len() as i64 + i) as usize
                        } else {
                            *i as usize
                        };
                        items
                            .get(index)
                            .cloned()
                            .ok_or_else(|| "Index out of range".to_string())
                    }
                    (Value::String(s), Value::Integer(i)) => {
                        let index = if *i < 0 {
                            (s.len() as i64 + i) as usize
                        } else {
                            *i as usize
                        };
                        s.chars()
                            .nth(index)
                            .map(|c| Value::String(c.to_string()))
                            .ok_or_else(|| "Index out of range".to_string())
                    }
                    _ => Err("Invalid index operation".to_string()),
                }
            }
            Expr::Attribute { object, name } => {
                let obj = self.eval_expr(object)?;
                let attr = self.interner.resolve(*name);
                match (&obj, attr) {
                    (Value::List(items), "len") => Ok(Value::Integer(items.len() as i64)),
                    (Value::String(s), "len") => Ok(Value::Integer(s.len() as i64)),
                    _ => Err(format!("No attribute '{}' on {:?}", attr, obj)),
                }
            }
            Expr::List(elements) => {
                let mut items = Vec::with_capacity(elements.len());
                for el in elements {
                    items.push(self.eval_expr(el)?);
                }
                Ok(Value::List(items))
            }
            Expr::Dict(pairs) => {
                let mut result = Vec::with_capacity(pairs.len());
                for (k, v) in pairs {
                    let key = self.eval_expr(k)?;
                    let val = self.eval_expr(v)?;
                    result.push((key, val));
                }
                Ok(Value::Dict(result))
            }
            Expr::Compare {
                left,
                ops,
                comparators,
            } => {
                let mut current = self.eval_expr(left)?;
                for (op, comp) in ops.iter().zip(comparators.iter()) {
                    let right = self.eval_expr(comp)?;
                    let result = self.apply_cmp(op, &current, &right)?;
                    if !result {
                        return Ok(Value::Boolean(false));
                    }
                    current = right;
                }
                Ok(Value::Boolean(true))
            }
        }
    }

    fn call_function(&mut self, func: &Value, args: Vec<Value>) -> Result<Value, String> {
        match func {
            Value::Function(func_obj) => {
                if args.len() != func_obj.params.len() {
                    return Err(format!(
                        "Expected {} arguments, got {}",
                        func_obj.params.len(),
                        args.len()
                    ));
                }

                let func_obj = Rc::clone(func_obj);

                self.env.push_frame(Rc::clone(&func_obj));
                for (param, arg) in func_obj.params.iter().zip(args) {
                    self.env.set(*param, arg);
                }

                let result = self.exec_block(&func_obj.body);
                self.env.pop_frame();

                match result? {
                    Some(ControlFlow::Return(val)) => Ok(val),
                    _ => Ok(Value::None),
                }
            }
            Value::NativeFunction { func, .. } => {
                func(&args)
            }
            _ => Err(format!("{} is not callable", func)),
        }
    }

    fn apply_binop(&self, op: &BinOp, left: &Value, right: &Value) -> Result<Value, String> {
        match (op, left, right) {
            // Integer arithmetic
            (BinOp::Add, Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a + b)),
            (BinOp::Sub, Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a - b)),
            (BinOp::Mul, Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a * b)),
            (BinOp::Div, Value::Integer(a), Value::Integer(b)) => {
                if *b == 0 {
                    Err("Division by zero".to_string())
                } else {
                    Ok(Value::Float(*a as f64 / *b as f64))
                }
            }
            (BinOp::FloorDiv, Value::Integer(a), Value::Integer(b)) => {
                if *b == 0 {
                    Err("Division by zero".to_string())
                } else {
                    Ok(Value::Integer(a / b))
                }
            }
            (BinOp::Mod, Value::Integer(a), Value::Integer(b)) => {
                if *b == 0 {
                    Err("Modulo by zero".to_string())
                } else {
                    Ok(Value::Integer(((a % b) + b) % b))
                }
            }
            (BinOp::Pow, Value::Integer(a), Value::Integer(b)) => {
                Ok(Value::Integer(a.pow(*b as u32)))
            }

            // Float arithmetic
            (BinOp::Add, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (BinOp::Sub, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
            (BinOp::Mul, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
            (BinOp::Div, Value::Float(a), Value::Float(b)) => {
                if *b == 0.0 {
                    Err("Division by zero".to_string())
                } else {
                    Ok(Value::Float(a / b))
                }
            }

            // Mixed int/float
            (BinOp::Add, Value::Integer(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
            (BinOp::Add, Value::Float(a), Value::Integer(b)) => Ok(Value::Float(a + *b as f64)),
            (BinOp::Sub, Value::Integer(a), Value::Float(b)) => Ok(Value::Float(*a as f64 - b)),
            (BinOp::Sub, Value::Float(a), Value::Integer(b)) => Ok(Value::Float(a - *b as f64)),
            (BinOp::Mul, Value::Integer(a), Value::Float(b)) => Ok(Value::Float(*a as f64 * b)),
            (BinOp::Mul, Value::Float(a), Value::Integer(b)) => Ok(Value::Float(a * *b as f64)),
            (BinOp::Div, Value::Integer(a), Value::Float(b)) => Ok(Value::Float(*a as f64 / b)),
            (BinOp::Div, Value::Float(a), Value::Integer(b)) => Ok(Value::Float(a / *b as f64)),

            // String concatenation
            (BinOp::Add, Value::String(a), Value::String(b)) => {
                let mut result = String::with_capacity(a.len() + b.len());
                result.push_str(a);
                result.push_str(b);
                Ok(Value::String(result))
            }
            (BinOp::Mul, Value::String(a), Value::Integer(b)) => {
                Ok(Value::String(a.repeat(*b as usize)))
            }

            // List concatenation
            (BinOp::Add, Value::List(a), Value::List(b)) => {
                let mut result = Vec::with_capacity(a.len() + b.len());
                result.extend_from_slice(a);
                result.extend_from_slice(b);
                Ok(Value::List(result))
            }

            // Boolean logic
            (BinOp::And, _, _) => {
                if left.is_truthy() {
                    Ok(right.clone())
                } else {
                    Ok(left.clone())
                }
            }
            (BinOp::Or, _, _) => {
                if left.is_truthy() {
                    Ok(left.clone())
                } else {
                    Ok(right.clone())
                }
            }

            _ => Err(format!(
                "Unsupported operation: {:?} {:?} {:?}",
                left, op, right
            )),
        }
    }

    fn apply_cmp(&self, op: &CmpOp, left: &Value, right: &Value) -> Result<bool, String> {
        match (op, left, right) {
            (CmpOp::Eq, a, b) => Ok(a == b),
            (CmpOp::NotEq, a, b) => Ok(a != b),
            (CmpOp::Lt, Value::Integer(a), Value::Integer(b)) => Ok(a < b),
            (CmpOp::LtE, Value::Integer(a), Value::Integer(b)) => Ok(a <= b),
            (CmpOp::Gt, Value::Integer(a), Value::Integer(b)) => Ok(a > b),
            (CmpOp::GtE, Value::Integer(a), Value::Integer(b)) => Ok(a >= b),
            (CmpOp::Lt, Value::Float(a), Value::Float(b)) => Ok(a < b),
            (CmpOp::LtE, Value::Float(a), Value::Float(b)) => Ok(a <= b),
            (CmpOp::Gt, Value::Float(a), Value::Float(b)) => Ok(a > b),
            (CmpOp::GtE, Value::Float(a), Value::Float(b)) => Ok(a >= b),
            (CmpOp::Lt, Value::String(a), Value::String(b)) => Ok(a < b),
            (CmpOp::Gt, Value::String(a), Value::String(b)) => Ok(a > b),
            (CmpOp::In, val, Value::List(items)) => Ok(items.contains(val)),
            (CmpOp::NotIn, val, Value::List(items)) => Ok(!items.contains(val)),
            (CmpOp::In, Value::String(sub), Value::String(s)) => Ok(s.contains(sub.as_str())),
            _ => Err(format!("Cannot compare {:?} and {:?}", left, right)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let mut interp = Interpreter::new(interner);
        interp.run(&stmts).unwrap();
        interp.output.clone()
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
        let output = run("x = 10\ny = 20\nprint(x + y)\n");
        assert_eq!(output, vec!["30"]);
    }

    #[test]
    fn test_function() {
        let code = "def add(a, b):\n    return a + b\nprint(add(3, 4))\n";
        assert_eq!(run(code), vec!["7"]);
    }

    #[test]
    fn test_if_else() {
        let code = "x = 10\nif x > 5:\n    print(\"big\")\nelse:\n    print(\"small\")\n";
        assert_eq!(run(code), vec!["big"]);
    }

    #[test]
    fn test_while_loop() {
        let code =
            "i = 0\nresult = 0\nwhile i < 5:\n    result += i\n    i += 1\nprint(result)\n";
        assert_eq!(run(code), vec!["10"]);
    }

    #[test]
    fn test_for_loop() {
        let code = "total = 0\nfor x in [1, 2, 3, 4, 5]:\n    total += x\nprint(total)\n";
        assert_eq!(run(code), vec!["15"]);
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
        let code = "\
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)
print(fib(10))
";
        assert_eq!(run(code), vec!["55"]);
    }
}
