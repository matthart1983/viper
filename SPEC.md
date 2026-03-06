# Viper — Technical Specification

## 1. Overview

Viper is a Python interpreter implemented in Rust. It compiles a subset of Python source code into bytecode and executes it on a custom virtual machine. The system has two execution backends: a legacy tree-walking interpreter (used by the REPL) and a high-performance bytecode VM (used by the benchmark suite and available via the library API).

## 2. Lexer (`token.rs`, `lexer.rs`)

### 2.1 Token Types

The lexer produces a flat `Vec<Token>` from source text. Token variants include:

- **Literals**: `Integer(i64)`, `Float(f64)`, `StringLiteral(String)`, `Boolean(bool)`
- **Identifiers**: `Identifier(String)`
- **Keywords**: `Def`, `Return`, `If`, `Elif`, `Else`, `While`, `For`, `In`, `Break`, `Continue`, `Pass`, `None`, `True`, `False`, `And`, `Or`, `Not`, `Print`, `Class`, `Import`, `From`, `As`
- **Operators**: `Plus`, `Minus`, `Star`, `Slash`, `DoubleSlash`, `Percent`, `DoubleStar`, `Assign`, `PlusAssign`, `MinusAssign`, `StarAssign`, `SlashAssign`
- **Comparison**: `Equal`, `NotEqual`, `Less`, `LessEqual`, `Greater`, `GreaterEqual`
- **Delimiters**: `LeftParen`, `RightParen`, `LeftBracket`, `RightBracket`, `LeftBrace`, `RightBrace`, `Comma`, `Colon`, `Dot`, `Arrow`
- **Indentation**: `Indent`, `Dedent`, `Newline`
- **Special**: `Eof`

### 2.2 Indentation Handling

The lexer maintains an `indent_stack: Vec<usize>` and an `at_line_start` flag. At the beginning of each line, it counts leading spaces and emits `Indent` or `Dedent` tokens relative to the current indentation level. Multiple `Dedent` tokens are emitted via a `pending_tokens` queue when dedenting across several levels.

## 3. Parser (`ast.rs`, `parser.rs`)

### 3.1 AST Nodes

**Expressions** (`Expr`):
- `Integer(i64)`, `Float(f64)`, `StringLiteral(String)`, `Boolean(bool)`, `None`
- `Identifier(Symbol)` — interned string reference
- `BinaryOp { left, op: BinOp, right }` — arithmetic and boolean operators
- `UnaryOp { op: UnaryOp, operand }` — negation, logical not
- `Call { function, args }` — function invocation
- `Index { object, index }` — subscript access
- `Attribute { object, name }` — dot access
- `List(Vec<Expr>)`, `Dict(Vec<(Expr, Expr)>)`
- `Compare { left, ops, comparators }` — chained comparisons (`a < b <= c`)

**Statements** (`Stmt`):
- `Expression(Expr)`, `Assign { target, value }`, `AugAssign { target, op, value }`
- `Print(Vec<Expr>)` — built-in print statement
- `If { condition, body, elif_clauses, else_body }`
- `While { condition, body }`, `For { target, iter, body }`
- `FunctionDef { name, params, body }`, `Return(Option<Expr>)`
- `Break`, `Continue`, `Pass`

### 3.2 Parsing Strategy

Recursive-descent parser consuming `Vec<Token>`. Operator precedence is handled by separate `parse_or_expr` → `parse_and_expr` → `parse_not_expr` → `parse_comparison` → `parse_addition` → `parse_multiplication` → `parse_power` → `parse_unary` → `parse_primary` call chain.

### 3.3 Symbol Interning

All identifiers are interned via `Interner` (a bidirectional `HashMap<String, u32>` / `Vec<String>` mapping). At runtime, variable lookups use `Symbol(u32)` comparisons instead of string hashing.

## 4. Bytecode Compiler (`bytecode.rs`, `compiler.rs`)

### 4.1 Value Representation

```rust
enum Value {
    Integer(i64),
    Float(f64),
    String(Rc<String>),
    Boolean(bool),
    List(Rc<Vec<Value>>),
    Dict(Vec<(Value, Value)>),
    None,
    Function(Rc<FunctionObj>),
}
```

`Value` is 24 bytes. Heap types use `Rc` for reference counting. `Value` is not `Copy` due to `Rc` fields.

### 4.2 Code Objects

```rust
struct CodeObject {
    instructions: Vec<Op>,
    constants: Vec<Value>,
    num_locals: u16,
    local_slots: HashMap<Symbol, u16>,
}

struct FunctionObj {
    name: Symbol,
    params: Vec<Symbol>,
    param_slots: Vec<u16>,
    code: Rc<CodeObject>,
}
```

Each function definition produces a separate `CodeObject`. Module-level code is also a `CodeObject`. Functions store their compiled code behind `Rc` for zero-copy sharing.

### 4.3 Instruction Set (`Op`)

The `Op` enum is `Copy` (~12 bytes) with the following variants:

**Load/Store:**
- `LoadConst(u16)` — push constant by index
- `LoadLocal(u16)` — push local variable by slot
- `StoreLocal(u16)` — pop into local slot
- `LoadGlobal(Symbol)` — push global by symbol
- `StoreGlobal(Symbol)` — pop into global

**Arithmetic:** `BinaryAdd`, `BinarySub`, `BinaryMul`, `BinaryDiv`, `BinaryFloorDiv`, `BinaryMod`, `BinaryPow`

**Comparison:** `CompareEq`, `CompareNotEq`, `CompareLt`, `CompareLtE`, `CompareGt`, `CompareGtE`, `CompareIn`, `CompareNotIn`

**Unary:** `UnaryNeg`, `UnaryNot`

**Control flow:**
- `Jump(u32)` — unconditional jump
- `PopJumpIfFalse(u32)` — pop and jump if falsy
- `JumpIfTrueOrPop(u32)` — short-circuit or
- `JumpIfFalseOrPop(u32)` — short-circuit and

**Functions:** `Call(u8)`, `Return`, `ReturnNone`

**Collections:** `BuildList(u16)`, `BuildDict(u16)`, `BinarySubscript`, `LoadAttr(Symbol)`, `Len`, `ListAppend`

**Superinstructions** (fused hot-path sequences):
- `IncrLocalByConst(u32)` — `LoadLocal + LoadConst + BinaryAdd + StoreLocal`
- `LocalLtLocalJump(u32, u32)` — `LoadLocal + LoadLocal + CompareLt + PopJumpIfFalse`
- `LocalLtEConstJump(u32, u32)` — `LoadLocal + LoadConst + CompareLtE + PopJumpIfFalse`
- `LocalLtConstJump(u32, u32)` — `LoadLocal + LoadConst + CompareLt + PopJumpIfFalse`
- `LocalGtConstJump(u32, u32)` — `LoadLocal + LoadConst + CompareGt + PopJumpIfFalse`
- `LoadLocalPair(u32)` — `LoadLocal + LoadLocal`
- `IncrGlobalByConst(Symbol, u16)` — `LoadGlobal + LoadConst + BinaryAdd + StoreGlobal`
- `TakeGlobal(Symbol)` — `LoadGlobal` with ownership transfer (removes from map)
- `CallGlobal(Symbol, u8)` — direct global function call (skips operand stack for function value)

**Other:** `Pop`, `Print(u8)`

### 4.4 Compilation

- **Module level** (`compile_module`): `is_function = false`, all variables are globals
- **Function level** (`compile_function`): `is_function = true`, `scan_locals` pre-scans body for assignments to build `local_slots` map. Parameters get first slots.
- **CallGlobal optimization**: When compiling `Expr::Call` where the function is a global identifier, emits `CallGlobal(sym, argc)` directly instead of `LoadGlobal + Call`
- **TakeGlobal optimization**: For `x = x + "str"` where x is global, emits `TakeGlobal` to transfer ownership and avoid `Rc::clone`

### 4.5 Peephole Optimizer

Runs after initial codegen in two passes:

**4-instruction fusions** (checked first):
- `LoadLocal(s) + LoadConst(c) + BinaryAdd + StoreLocal(s)` → `IncrLocalByConst`
- `LoadLocal(a) + LoadLocal(b) + CompareLt + PopJumpIfFalse(t)` → `LocalLtLocalJump`
- `LoadLocal(s) + LoadConst(c) + Compare{LtE,Lt,Gt} + PopJumpIfFalse(t)` → `Local{LtE,Lt,Gt}ConstJump`
- `LoadGlobal(s) + LoadConst(c) + BinaryAdd + StoreGlobal(s)` → `IncrGlobalByConst`

**2-instruction fusions** (second pass):
- `LoadLocal(a) + LoadLocal(b)` → `LoadLocalPair`

Jump targets are tracked to prevent fusing across branch boundaries. `remove_instructions` remaps all jump offsets after each fusion.

## 5. Virtual Machine (`vm.rs`)

### 5.1 Architecture

Flattened frame design — the current frame's state is stored directly as VM fields for minimal indirection:

```rust
struct VM {
    stack: Vec<Value>,           // operand stack
    code: *const CodeObject,     // raw pointer to current code (no Rc overhead)
    pc: usize,                   // program counter
    locals_stack: Vec<Value>,    // unified locals for all frames
    locals_base: usize,          // offset into locals_stack for current frame
    _module_code: Option<Rc<CodeObject>>,  // keeps module code alive
    saved_frames: Vec<SavedFrame>,         // call stack
    globals: FxHashMap<Symbol, Value>,     // global variables
    interner: Interner,
    output: Vec<String>,
    suppress_output: bool,
}

struct SavedFrame {
    code: *const CodeObject,    // 8 bytes
    pc: usize,                  // 8 bytes
    locals_base: usize,         // 8 bytes
}                               // total: 24 bytes
```

### 5.2 Key Design Decisions

- **Raw pointers for code**: `*const CodeObject` instead of `Rc<CodeObject>` eliminates atomic refcount inc/dec on every call/return (~5–10ns savings per call)
- **Unified locals stack**: All frames' locals are contiguous in a single `Vec<Value>`, indexed by `locals_base`. Eliminates per-call Vec allocation/deallocation
- **FxHashMap for globals**: `rustc-hash`'s `FxHashMap` provides fast integer-key hashing for `Symbol(u32)` lookups
- **Unsafe stack_pop**: Uses `unwrap_unchecked()` for stack pops since the compiler guarantees correct stack depth
- **SavedFrame is 24 bytes**: Fits in a cache line; `saved_frames.push/pop` is just a pointer bump

### 5.3 Call Path

For `CallGlobal(sym, argc)`:
1. Look up function in `globals` via FxHashMap
2. Extract raw `*const CodeObject` and `*const u16` param_slots pointer
3. `push_locals(num_locals)` — extend unified locals stack with `Value::None`
4. Move arguments from operand stack into locals slots
5. Truncate operand stack
6. Save current frame (code ptr, pc, locals_base) — 24-byte push
7. Set new code/pc/locals_base

For `Return`:
1. Pop return value from operand stack
2. `pop_locals(locals_base)` — truncate locals stack
3. Pop saved frame — 24-byte pop, restore code/pc/locals_base
4. Push return value onto operand stack

### 5.4 Dispatch

Standard `match` dispatch in a `loop`. Each iteration:
1. Bounds-check `pc` against `instructions.len()` (handles implicit return)
2. Copy the `Op` (it's `Copy`, ~12 bytes)
3. Increment `pc`
4. Match on op variant

## 6. Tree-Walking Interpreter (`interpreter.rs`)

Legacy backend used by the REPL. Operates directly on AST nodes with an `Environment` (scope chain of `HashMap<Symbol, Value>`). Supports the same language features as the VM but without bytecode compilation or optimization. Retained for interactive use where compilation overhead would dominate single-expression execution.

## 7. Supported Python Subset

### Types
- `int` (i64), `float` (f64), `str`, `bool`, `list`, `dict`, `None`
- First-class functions (closures not supported)

### Statements
- `x = expr`, `x += expr`, `x -= expr`, `x *= expr`, `x /= expr`
- `if/elif/else`, `while`, `for x in iterable`
- `def name(params): body`, `return expr`
- `break`, `continue`, `pass`
- `print(args)`

### Expressions
- Arithmetic: `+`, `-`, `*`, `/`, `//`, `%`, `**`
- Comparison: `==`, `!=`, `<`, `<=`, `>`, `>=`, `in`, `not in`
- Chained comparisons: `a < b <= c`
- Boolean: `and`, `or`, `not`
- Subscript: `lst[i]`, `d[key]`
- Attribute: `obj.attr`
- Calls: `func(args)`
- Literals: `[1, 2, 3]`, `{"a": 1}`, `"string"`, `True`, `False`, `None`

### Not Supported
- Classes, imports, generators, comprehensions, closures, exceptions, decorators, `*args`/`**kwargs`, multiple assignment, slicing, `with`, `try/except`, `lambda`
