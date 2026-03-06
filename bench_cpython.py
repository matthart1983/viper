"""Equivalent benchmarks in CPython for comparison with rustpython_interp."""

import time
import statistics
import sys
import io

# ---------------------------------------------------------------------------
# Benchmark harness
# ---------------------------------------------------------------------------

def bench(name, iterations, fn):
    # Warm-up
    for _ in range(3):
        fn()

    times = []
    for _ in range(iterations):
        start = time.perf_counter()
        fn()
        times.append(time.perf_counter() - start)

    times.sort()
    mean = statistics.mean(times)
    med = statistics.median(times)
    mn = min(times)
    mx = max(times)
    ops = iterations / sum(times)
    print(f"  {name:<40s} {mean*1e6:>10.0f}µs  "
          f"(min {mn*1e6:>10.0f}µs  max {mx*1e6:>10.0f}µs  "
          f"median {med*1e6:>10.0f}µs  {iterations} iters  {ops:.0f} runs/s)")
    return mean


def run_silent(code):
    """exec() with stdout suppressed."""
    old = sys.stdout
    sys.stdout = io.StringIO()
    try:
        exec(code, {})
    finally:
        sys.stdout = old

# ---------------------------------------------------------------------------
# Generate identical Python programs
# ---------------------------------------------------------------------------

def gen_arithmetic_heavy(n):
    lines = ["x = 0"]
    for i in range(n):
        lines.append(f"x = x + {i} * {i+1} - {i%7} + {i%3}")
    lines.append("print(x)")
    return "\n".join(lines) + "\n"

def gen_nested_loops(outer, inner):
    return f"""\
total = 0
i = 0
while i < {outer}:
    j = 0
    while j < {inner}:
        total = total + i * j
        j = j + 1
    i = i + 1
print(total)
"""

def gen_recursive_fib():
    return """\
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)
print(fib(20))
"""

def gen_iterative_fib(n):
    return f"""\
a = 0
b = 1
i = 0
while i < {n}:
    c = a + b
    a = b
    b = c
    i = i + 1
print(b)
"""

def gen_string_concat(n):
    return f"""\
s = ""
i = 0
while i < {n}:
    s = s + "a"
    i = i + 1
print(s)
"""

def gen_list_build_and_sum(n):
    return f"""\
lst = []
i = 0
while i < {n}:
    lst = lst + [i]
    i = i + 1
total = 0
j = 0
while j < {n}:
    total = total + lst[j]
    j = j + 1
print(total)
"""

def gen_many_function_calls(n):
    return f"""\
def inc(x):
    return x + 1
result = 0
i = 0
while i < {n}:
    result = inc(result)
    i = i + 1
print(result)
"""

def gen_deep_nesting(depth):
    lines = ["x = 1"]
    for i in range(depth):
        indent = "    " * i
        lines.append(f"{indent}if x > 0:")
        lines.append(f"{indent}    x = x + 1")
    lines.append("    " * depth + "print(x)")
    return "\n".join(lines) + "\n"

def gen_many_variables(n):
    lines = []
    for i in range(n):
        lines.append(f"var_{i} = {i}")
    lines.append("total = 0")
    for i in range(n):
        lines.append(f"total = total + var_{i}")
    lines.append("print(total)")
    return "\n".join(lines) + "\n"

def gen_heavy_branching(n):
    return f"""\
count = 0
i = 0
while i < {n}:
    if i % 3 == 0:
        count = count + 1
    elif i % 3 == 1:
        count = count + 2
    else:
        count = count + 3
    i = i + 1
print(count)
"""

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    print("=" * 100)
    print(f"  CPython {sys.version.split()[0]} — Performance Benchmark (same workloads)")
    print("=" * 100)
    print()

    results = []

    # -- Interpreter benchmarks (end-to-end, matching Rust suite) --
    print("── Interpreter (end-to-end) " + "─" * 72)

    code = gen_arithmetic_heavy(5000)
    results.append(("5k arithmetic ops", bench("interp: 5k arithmetic ops", 50, lambda: run_silent(code))))

    code = gen_nested_loops(100, 100)
    results.append(("nested loops 100×100", bench("interp: nested loops 100×100", 50, lambda: run_silent(code))))

    code = gen_recursive_fib()
    results.append(("recursive fib(20)", bench("interp: recursive fib(20)", 50, lambda: run_silent(code))))

    code = gen_iterative_fib(10_000)
    results.append(("iterative fib(10k)", bench("interp: iterative fib(10k)", 50, lambda: run_silent(code))))

    code = gen_string_concat(2000)
    results.append(("string concat ×2000", bench("interp: string concat ×2000", 50, lambda: run_silent(code))))

    code = gen_list_build_and_sum(500)
    results.append(("list build+sum (500)", bench("interp: list build+sum (500)", 50, lambda: run_silent(code))))

    code = gen_many_function_calls(5000)
    results.append(("5k function calls", bench("interp: 5k function calls", 50, lambda: run_silent(code))))

    code = gen_many_variables(500)
    results.append(("500 variable lookups", bench("interp: 500 variable lookups", 100, lambda: run_silent(code))))

    code = gen_deep_nesting(20)
    results.append(("20-deep nesting", bench("interp: 20-deep nesting", 200, lambda: run_silent(code))))

    code = gen_heavy_branching(5000)
    results.append(("heavy branching ×5000", bench("interp: heavy branching ×5000", 50, lambda: run_silent(code))))

    print()
    print("=" * 100)

if __name__ == "__main__":
    main()
