#!/usr/bin/env python3
"""CPython reference programs matching Ember's Fibonacci benchmark workloads."""

from __future__ import annotations

import argparse


def fib(n: int) -> int:
    a, b = 0, 1
    while n:
        a, b = b, a + b
        n -= 1
    return a


def run_function(iterations: int = 10_000, n: int = 30) -> int:
    result = 0
    for _ in range(iterations):
        result = fib(n)
    return result


def run_inline(iterations: int = 10_000, n: int = 30) -> int:
    result = 0
    for _ in range(iterations):
        remaining = n
        a, b = 0, 1
        while remaining:
            a, b = b, a + b
            remaining -= 1
        result = a
    return result


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("workload", choices=("fib_inline", "fib_function"))
    args = parser.parse_args()

    if args.workload == "fib_inline":
        print(run_inline())
    else:
        print(run_function())


if __name__ == "__main__":
    main()
