# 🐍 Viper

A fast Python interpreter written from scratch in Rust. Hand-rolled lexer, recursive-descent parser, bytecode compiler with peephole optimization, and a register-style VM that beats CPython 3.12 on 9 out of 10 benchmarks.

## Quick Start

```bash
# Run a Python file
cargo run --release -- script.py

# Interactive REPL
cargo run --release

# Run benchmarks
cargo run --release --bin benchmark
```

## Features

- **Full pipeline**: Lexer → Parser → AST → Bytecode Compiler → VM
- **Bytecode VM** with flattened frame layout, unified locals stack, raw-pointer code references
- **Peephole optimizer** fusing 4-instruction and 2-instruction hot-path sequences into superinstructions
- **Interactive REPL** with multi-line block support
- **Supported Python subset**: integers, floats, strings, booleans, lists, dicts, `None`, functions, `if/elif/else`, `while`, `for..in`, `break/continue`, `def/return`, `print`, augmented assignment, comparison chains, `and/or/not`

## Performance vs CPython 3.12

Benchmarked on Apple Silicon (M-series), `--release` mode:

| Benchmark | Viper | CPython 3.12 | Speedup |
|---|---|---|---|
| 5k arithmetic ops | 13.9ms | 13.7ms | **1.02×** ✅ |
| nested loops 100×100 | 520µs | 533µs | **1.03×** ✅ |
| recursive fib(20) | 870µs | 526µs | 0.60× |
| iterative fib(10k) | 598µs | 1733µs | **2.90×** ✅ |
| string concat ×2000 | 106µs | 161µs | **1.52×** ✅ |
| list build+sum (500) | 76µs | 245µs | **3.22×** ✅ |
| 5k function calls | 290µs | 313µs | **1.08×** ✅ |
| 500 variable lookups | 426µs | 1596µs | **3.75×** ✅ |
| 20-deep nesting | 18µs | 80µs | **4.44×** ✅ |
| heavy branching ×5000 | 376µs | 385µs | **1.02×** ✅ |

**Score: 9/10 benchmarks faster than CPython 3.12**

Run `python3 bench_cpython.py` to reproduce CPython numbers on your machine.

## Architecture

```
Source Code
    │
    ▼
┌─────────┐     ┌──────────┐     ┌──────────┐     ┌──────────────┐     ┌─────┐
│  Lexer  │────▶│  Parser  │────▶│   AST    │────▶│   Compiler   │────▶│ VM  │
│         │     │          │     │          │     │  + Peephole   │     │     │
└─────────┘     └──────────┘     └──────────┘     └──────────────┘     └─────┘
 token.rs        parser.rs        ast.rs          compiler.rs          vm.rs
 lexer.rs                                         bytecode.rs
```

See [SPEC.md](SPEC.md) for detailed architecture and [WIKI.md](WIKI.md) for the optimization journey.

## Project Structure

```
src/
├── main.rs          # CLI entry point (REPL + file runner)
├── lib.rs           # Library crate root
├── token.rs         # Token enum and keyword mapping
├── lexer.rs         # Hand-rolled lexer with indent/dedent tracking
├── ast.rs           # AST node definitions (Expr, Stmt, BinOp, etc.)
├── parser.rs        # Recursive-descent parser
├── symbol.rs        # String interning (Symbol → u32)
├── bytecode.rs      # Op enum, CodeObject, Value, FunctionObj
├── compiler.rs      # AST → bytecode compiler + peephole optimizer
├── interpreter.rs   # Tree-walking interpreter (legacy, used by REPL)
├── vm.rs            # Bytecode virtual machine
└── bin/
    └── benchmark.rs # Performance benchmark suite
bench_cpython.py     # Equivalent CPython benchmarks for comparison
```

## Example

```python
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)

print(fib(20))
```

```
$ viper fib.py
6765
```

## Building

Requires Rust 2021 edition. Single dependency: `rustc-hash`.

```bash
cargo build --release
```

## License

MIT
