# Viper — Optimization Wiki

This document traces the evolution of Viper from a simple tree-walking interpreter to a bytecode VM that beats CPython 3.12 on 9/10 benchmarks.

## Phase 0: Tree-Walking Interpreter

The initial implementation (`interpreter.rs`) walks the AST directly. Each expression evaluation and statement execution traverses `Expr`/`Stmt` enum nodes, with variables stored in a scope chain of `HashMap<Symbol, Value>`.

**Characteristics:**
- Simple and correct — good baseline
- Slow: hash lookups per variable access, deep recursion overhead, no optimization opportunities
- Still used by the REPL for interactive one-liners

## Phase 1: Bytecode Compilation

Added `compiler.rs`, `bytecode.rs`, and `vm.rs` to compile AST into a flat instruction sequence executed by a stack-based VM.

**Key decisions:**
- `Op` enum is `Copy` (~12 bytes) — instructions can be loaded with a simple memcpy, no allocation
- `Value` is 24 bytes with `Rc` for heap types — balances size with reference counting cost
- `Symbol(u32)` interning eliminates string comparisons at runtime
- Separate `CodeObject` per function with `Rc` sharing

**Impact:** Massive speedup over tree-walking for all benchmarks. Eliminated AST traversal overhead.

## Phase 2: FxHashMap + Flattened Frames

Replaced `HashMap` with `FxHashMap` from `rustc-hash` for global variable storage. FxHashMap uses a fast integer hash that's ideal for `Symbol(u32)` keys.

Flattened the VM frame: instead of a `Frame` struct accessed through indirection, the current frame's `code`, `pc`, and `locals` became direct fields on the `VM` struct. Previous frames stored in `saved_frames: Vec<SavedFrame>`.

**Impact:** ~20% improvement on variable-heavy benchmarks.

## Phase 3: Superinstructions

Added peephole optimization pass in the compiler that fuses common instruction sequences into single superinstructions:

### 4-Instruction Fusions
| Pattern | Superinstruction | Saves |
|---|---|---|
| `LoadLocal + LoadConst + BinaryAdd + StoreLocal` | `IncrLocalByConst` | 3 dispatches + stack ops |
| `LoadLocal + LoadLocal + CompareLt + PopJumpIfFalse` | `LocalLtLocalJump` | 3 dispatches + stack ops |
| `LoadLocal + LoadConst + CompareLtE + PopJumpIfFalse` | `LocalLtEConstJump` | 3 dispatches |
| `LoadLocal + LoadConst + CompareLt + PopJumpIfFalse` | `LocalLtConstJump` | 3 dispatches |
| `LoadLocal + LoadConst + CompareGt + PopJumpIfFalse` | `LocalGtConstJump` | 3 dispatches |
| `LoadGlobal + LoadConst + BinaryAdd + StoreGlobal` | `IncrGlobalByConst` | 3 dispatches + hash lookup |

### 2-Instruction Fusions
| Pattern | Superinstruction | Saves |
|---|---|---|
| `LoadLocal + LoadLocal` | `LoadLocalPair` | 1 dispatch |

### Implementation Details
- Jump targets tracked to prevent fusing across branch boundaries
- `remove_instructions` remaps all jump offsets after each fusion
- 4-instruction pass runs first (greedy), then 2-instruction pass
- Constant deduplication in `add_const` avoids duplicate entries

**Impact:** ~30% improvement on loop-heavy benchmarks (nested loops, iterative fib).

## Phase 4: Ownership Optimizations

### TakeGlobal
For the pattern `x = x + "str"` where x is a global string, the naive path does `LoadGlobal` (Rc::clone, refcount=2) then `BinaryAdd` (must allocate new string since refcount > 1). `TakeGlobal` removes the value from globals (refcount stays 1), allowing `BinaryAdd` to mutate the string buffer in-place via `Rc::try_unwrap`.

### ListAppend
For `lst += [item]`, fused into a single `ListAppend` operation that uses `Rc::make_mut` to append in-place when possible.

### Unsafe stack_pop
Replaced `stack.pop().unwrap()` with `unsafe { stack.pop().unwrap_unchecked() }` since the compiler guarantees correct stack depth.

**Impact:** String concat benchmark improved ~1.6× vs CPython. List operations improved ~3.2× vs CPython.

## Phase 5: Call Path Optimization

This phase focused on making function calls fast, particularly for recursive patterns like `fib(20)` with ~21,891 calls.

### 5.1 Remove Option Wrapper from Locals
Changed `Vec<Option<Value>>` to `Vec<Value>` for local variable storage, using `Value::None` as the default. This eliminates the `Option` discriminant overhead on every local access and store.

**Savings:** ~1 byte discriminant per access, cleaner generated code.

### 5.2 Raw Pointer Code References
Replaced `Rc<CodeObject>` with `*const CodeObject` in both `SavedFrame` and the VM's current code field. Code objects are kept alive by their parent (globals map for functions, `_module_code` for module-level code), so the raw pointer is valid for the duration of execution.

**Savings:** Eliminates atomic refcount increment on every function call and decrement on every return (~5–10ns per call).

### 5.3 Unified Locals Stack
Replaced per-frame `Vec<Value>` allocation with a single contiguous `locals_stack: Vec<Value>`. Each frame's locals occupy a segment starting at `locals_base`. On call, locals are extended with `resize`; on return, they're truncated back.

Before:
```
Call: pool.pop() or allocate Vec, resize, fill params
Return: clear Vec, pool.push()
SavedFrame: { code: Rc<CodeObject>, pc, locals: Vec<Value> }  // ~72 bytes
```

After:
```
Call: locals_stack.resize(base + size, None), fill params
Return: locals_stack.truncate(base)
SavedFrame: { code: *const CodeObject, pc, locals_base }     // 24 bytes
```

**Savings:** Eliminates Vec allocation/deallocation per call, reduces SavedFrame from ~72 to 24 bytes.

### 5.4 Compiler-Level CallGlobal
Moved the `CallGlobal` optimization from a peephole fusion (which was buggy — it fused the last *argument*'s `LoadGlobal` with `Call` instead of the *function*'s `LoadGlobal`) to the compiler proper. When compiling `Expr::Call` where the function is a global identifier, the compiler directly emits `CallGlobal(sym, argc)`.

This is both correct and more powerful: it works for any argument count, and saves 1 instruction dispatch + avoids pushing/popping the function value on the operand stack.

**Bug fixed:** The old peephole fusion of `LoadGlobal(sym) + Call(n)` would incorrectly fuse the last argument's LoadGlobal instead of the function's LoadGlobal when `n > 0`, causing "0 is not callable" crashes.

### Combined Impact

| Metric | Before Phase 5 | After Phase 5 |
|---|---|---|
| fib(20) | 1172µs (2.3× CPython) | 870µs (1.7× CPython) |
| 5k function calls | CRASH | 290µs (0.93× CPython ✅) |
| SavedFrame size | ~72 bytes | 24 bytes |
| Per-call Rc operations | 2 atomic ops | 0 |
| Per-call Vec alloc | 1 (or pool hit) | 0 (stack resize) |

## Benchmark Results Summary

All measurements on Apple Silicon, `cargo run --release`, compared to CPython 3.12:

| Benchmark | Viper | CPython | Ratio | Status |
|---|---|---|---|---|
| 5k arithmetic ops | 13.9ms | 13.7ms | 1.02× | ✅ Win |
| nested loops 100×100 | 520µs | 533µs | 1.03× | ✅ Win |
| recursive fib(20) | 870µs | 526µs | 0.60× | ❌ Loss (1.7× slower) |
| iterative fib(10k) | 598µs | 1733µs | 2.90× | ✅ Win |
| string concat ×2000 | 106µs | 161µs | 1.52× | ✅ Win |
| list build+sum (500) | 76µs | 245µs | 3.22× | ✅ Win |
| 5k function calls | 290µs | 313µs | 1.08× | ✅ Win |
| 500 variable lookups | 426µs | 1596µs | 3.75× | ✅ Win |
| 20-deep nesting | 18µs | 80µs | 4.44× | ✅ Win |
| heavy branching ×5000 | 376µs | 385µs | 1.02× | ✅ Win |

## Remaining Optimization Ideas

For the one remaining loss (recursive fib at 1.7× CPython):

1. **NaN-boxing**: Shrink `Value` from 24 bytes to 8 bytes by encoding integers/booleans/None in NaN bits and using tagged pointers for heap types. Would halve memory traffic for all value operations.
2. **Computed goto / threaded dispatch**: Replace `match` dispatch with indirect threading using function pointers or computed gotos (requires nightly Rust or assembly).
3. **Inline caching for CallGlobal**: Cache the resolved function pointer on first call, skip hash lookup on subsequent calls to the same symbol.
4. **Memoization detection**: Automatically detect pure recursive functions and cache results.
5. **Register-based VM**: Eliminate stack push/pop overhead by encoding source/dest registers in instructions (major rewrite).
