use std::time::{Duration, Instant};

use viper::compiler::compile_module;
use viper::lexer::Lexer;
use viper::parser::Parser;
use viper::symbol::Interner;
use viper::vm::VM;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct BenchResult {
    name: String,
    iterations: u32,
    _total: Duration,
    min: Duration,
    max: Duration,
    mean: Duration,
    median: Duration,
    throughput: Option<String>,
}

impl BenchResult {
    fn display(&self) {
        println!("  {:.<40} {:>10.3?}  (min {:>10.3?}  max {:>10.3?}  median {:>10.3?}  {} iters{})",
            self.name,
            self.mean,
            self.min,
            self.max,
            self.median,
            self.iterations,
            self.throughput.as_ref().map_or(String::new(), |t| format!("  {}", t)),
        );
    }
}

fn bench<F: FnMut()>(name: &str, iterations: u32, throughput_label: Option<&str>, mut f: F) -> BenchResult {
    // Warm-up
    for _ in 0..3 {
        f();
    }

    let mut times = Vec::with_capacity(iterations as usize);
    let start_total = Instant::now();

    for _ in 0..iterations {
        let start = Instant::now();
        f();
        times.push(start.elapsed());
    }

    let total = start_total.elapsed();
    times.sort();

    let min = *times.first().unwrap();
    let max = *times.last().unwrap();
    let mean = total / iterations;
    let median = times[times.len() / 2];

    let throughput = throughput_label.map(|l| {
        let ops_per_sec = iterations as f64 / total.as_secs_f64();
        format!("{:.0} {}/s", ops_per_sec, l)
    });

    BenchResult { name: name.to_string(), iterations, _total: total, min, max, mean, median, throughput }
}

fn run_full(code: &str) {
    let mut interner = Interner::new();
    let mut lexer = Lexer::new(code);
    let tokens = lexer.tokenize().unwrap();
    let stmts = {
        let mut parser = Parser::new(tokens, &mut interner);
        parser.parse().unwrap()
    };
    let code_obj = compile_module(&stmts, &mut interner);
    let mut vm = VM::new(interner);
    vm.set_suppress_output(true);
    vm.run(&code_obj).unwrap();
}

// ---------------------------------------------------------------------------
// Benchmark programs (generated Python source)
// ---------------------------------------------------------------------------

fn gen_arithmetic_heavy(n: usize) -> String {
    let mut code = "x = 0\n".to_string();
    for i in 0..n {
        code.push_str(&format!("x = x + {} * {} - {} + {}\n", i, i + 1, i % 7, i % 3));
    }
    code.push_str("print(x)\n");
    code
}

fn gen_nested_loops(outer: usize, inner: usize) -> String {
    format!(
        "\
total = 0
i = 0
while i < {}:
    j = 0
    while j < {}:
        total = total + i * j
        j = j + 1
    i = i + 1
print(total)
",
        outer, inner
    )
}

fn gen_recursive_fib() -> String {
    "\
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)
print(fib(20))
"
    .to_string()
}

fn gen_iterative_fib(n: usize) -> String {
    format!(
        "\
a = 0
b = 1
i = 0
while i < {}:
    c = a + b
    a = b
    b = c
    i = i + 1
print(b)
",
        n
    )
}

fn gen_string_concat(n: usize) -> String {
    format!(
        "\
s = \"\"
i = 0
while i < {}:
    s = s + \"a\"
    i = i + 1
print(s)
",
        n
    )
}

fn gen_list_build_and_sum(n: usize) -> String {
    format!(
        "\
lst = []
i = 0
while i < {}:
    lst = lst + [i]
    i = i + 1
total = 0
j = 0
while j < {}:
    total = total + lst[j]
    j = j + 1
print(total)
",
        n, n
    )
}

fn gen_many_function_calls(n: usize) -> String {
    format!(
        "\
def inc(x):
    return x + 1
result = 0
i = 0
while i < {}:
    result = inc(result)
    i = i + 1
print(result)
",
        n
    )
}

fn gen_deep_nesting(depth: usize) -> String {
    let mut code = "x = 1\n".to_string();
    for i in 0..depth {
        let indent = "    ".repeat(i);
        code.push_str(&format!("{}if x > 0:\n", indent));
        code.push_str(&format!("{}    x = x + 1\n", indent));
    }
    code.push_str(&format!("{}print(x)\n", "    ".repeat(depth)));
    code
}

fn gen_many_variables(n: usize) -> String {
    let mut code = String::new();
    for i in 0..n {
        code.push_str(&format!("var_{} = {}\n", i, i));
    }
    code.push_str("total = 0\n");
    for i in 0..n {
        code.push_str(&format!("total = total + var_{}\n", i));
    }
    code.push_str("print(total)\n");
    code
}

fn gen_large_token_stream(n: usize) -> String {
    let mut code = "x = 0\n".to_string();
    for _ in 0..n {
        code.push_str("x = (x + 1) * 2 - 1 + (3 * 4) - (5 + 6)\n");
    }
    code.push_str("print(x)\n");
    code
}

fn gen_heavy_branching(n: usize) -> String {
    format!(
        "\
count = 0
i = 0
while i < {}:
    if i % 3 == 0:
        count = count + 1
    elif i % 3 == 1:
        count = count + 2
    else:
        count = count + 3
    i = i + 1
print(count)
",
        n
    )
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    println!("═══════════════════════════════════════════════════════════════════════════════════════════════════════");
    println!("  Viper — Performance Benchmark Suite");
    println!("═══════════════════════════════════════════════════════════════════════════════════════════════════════\n");

    let mut all_results: Vec<BenchResult> = Vec::new();

    // ── Lexer benchmarks ──────────────────────────────────────────────────
    println!("── Lexer ──────────────────────────────────────────────────────────────────────────────────");
    {
        let code = gen_large_token_stream(1000);
        all_results.push(bench("lex: 1k expression lines", 200, Some("runs"), || {
            let mut lexer = Lexer::new(&code);
            lexer.tokenize().unwrap();
        }));
        all_results.last().unwrap().display();

        let code = gen_many_variables(500);
        all_results.push(bench("lex: 500 variable decls", 200, Some("runs"), || {
            let mut lexer = Lexer::new(&code);
            lexer.tokenize().unwrap();
        }));
        all_results.last().unwrap().display();

        let code = gen_nested_loops(50, 50);
        all_results.push(bench("lex: nested loops (indentation)", 500, Some("runs"), || {
            let mut lexer = Lexer::new(&code);
            lexer.tokenize().unwrap();
        }));
        all_results.last().unwrap().display();

        println!();
    }

    // ── Parser benchmarks ─────────────────────────────────────────────────
    println!("── Parser ─────────────────────────────────────────────────────────────────────────────────");
    {
        let code = gen_large_token_stream(1000);
        let mut lexer = Lexer::new(&code);
        let tokens = lexer.tokenize().unwrap();
        all_results.push(bench("parse: 1k expression lines", 200, Some("runs"), || {
            let mut interner = Interner::new();
            let mut parser = Parser::new(tokens.clone(), &mut interner);
            parser.parse().unwrap();
        }));
        all_results.last().unwrap().display();

        let code = gen_deep_nesting(20);
        let mut lexer = Lexer::new(&code);
        let tokens = lexer.tokenize().unwrap();
        all_results.push(bench("parse: 20-level deep nesting", 500, Some("runs"), || {
            let mut interner = Interner::new();
            let mut parser = Parser::new(tokens.clone(), &mut interner);
            parser.parse().unwrap();
        }));
        all_results.last().unwrap().display();

        let code = gen_heavy_branching(100);
        let mut lexer = Lexer::new(&code);
        let tokens = lexer.tokenize().unwrap();
        all_results.push(bench("parse: heavy branching", 500, Some("runs"), || {
            let mut interner = Interner::new();
            let mut parser = Parser::new(tokens.clone(), &mut interner);
            parser.parse().unwrap();
        }));
        all_results.last().unwrap().display();

        println!();
    }

    // ── Interpreter benchmarks ────────────────────────────────────────────
    println!("── Interpreter (end-to-end) ────────────────────────────────────────────────────────────────");
    {
        let code = gen_arithmetic_heavy(5000);
        all_results.push(bench("interp: 5k arithmetic ops", 50, Some("runs"), || {
            run_full(&code);
        }));
        all_results.last().unwrap().display();

        let code = gen_nested_loops(100, 100);
        all_results.push(bench("interp: nested loops 100×100", 50, Some("runs"), || {
            run_full(&code);
        }));
        all_results.last().unwrap().display();

        let code = gen_recursive_fib();
        all_results.push(bench("interp: recursive fib(20)", 50, Some("runs"), || {
            run_full(&code);
        }));
        all_results.last().unwrap().display();

        let code = gen_iterative_fib(10_000);
        all_results.push(bench("interp: iterative fib(10k)", 50, Some("runs"), || {
            run_full(&code);
        }));
        all_results.last().unwrap().display();

        let code = gen_string_concat(2000);
        all_results.push(bench("interp: string concat ×2000", 50, Some("runs"), || {
            run_full(&code);
        }));
        all_results.last().unwrap().display();

        let code = gen_list_build_and_sum(500);
        all_results.push(bench("interp: list build+sum (500)", 50, Some("runs"), || {
            run_full(&code);
        }));
        all_results.last().unwrap().display();

        let code = gen_many_function_calls(5000);
        all_results.push(bench("interp: 5k function calls", 50, Some("runs"), || {
            run_full(&code);
        }));
        all_results.last().unwrap().display();

        let code = gen_many_variables(500);
        all_results.push(bench("interp: 500 variable lookups", 100, Some("runs"), || {
            run_full(&code);
        }));
        all_results.last().unwrap().display();

        let code = gen_deep_nesting(20);
        all_results.push(bench("interp: 20-deep nesting", 200, Some("runs"), || {
            run_full(&code);
        }));
        all_results.last().unwrap().display();

        let code = gen_heavy_branching(5000);
        all_results.push(bench("interp: heavy branching ×5000", 50, Some("runs"), || {
            run_full(&code);
        }));
        all_results.last().unwrap().display();

        println!();
    }

    // ── Summary ───────────────────────────────────────────────────────────
    println!("═══════════════════════════════════════════════════════════════════════════════════════════════════════");
    println!("  Summary — Slowest benchmarks:");
    println!("───────────────────────────────────────────────────────────────────────────────────────────────────────");
    let mut sorted: Vec<&BenchResult> = all_results.iter().collect();
    sorted.sort_by(|a, b| b.mean.cmp(&a.mean));
    for r in sorted.iter().take(5) {
        r.display();
    }
    println!("═══════════════════════════════════════════════════════════════════════════════════════════════════════");
}
