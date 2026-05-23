"""
Benchmark: Ember VM vs CPython — iterative fib(30)

Runs fib(30) 10,000 times in a single process.
Equivalent to the Ember bytecode in examples/bench/main.embt
"""

def fib_iter(n):
    a, b = 0, 1
    for _ in range(n):
        a, b = b, a + b
    return a

def main():
    ITER = 10_000
    n = 30
    for _ in range(ITER):
        fib_iter(n)
    print(fib_iter(n))

if __name__ == "__main__":
    import time
    start = time.perf_counter()
    main()
    elapsed = time.perf_counter() - start
    print(f"{elapsed*1000:.1f}ms")
