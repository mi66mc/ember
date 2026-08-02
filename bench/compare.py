#!/usr/bin/env python3
"""Run a startup-inclusive Ember versus CPython Fibonacci comparison."""

from __future__ import annotations

import argparse
import json
import math
import os
import platform
import statistics
import subprocess
import sys
from datetime import UTC, datetime
from pathlib import Path
from time import perf_counter_ns
from typing import Any


EXPECTED_OUTPUT = "832040\n"
WORKLOADS = ("fib_inline", "fib_function")
REPOSITORY_ROOT = Path(__file__).resolve().parent.parent


def positive_int(value: str) -> int:
    parsed = int(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("must be greater than zero")
    return parsed


def nonnegative_int(value: str) -> int:
    parsed = int(value)
    if parsed < 0:
        raise argparse.ArgumentTypeError("must not be negative")
    return parsed


def command_for(runtime: str, workload: str, ember: str) -> list[str]:
    if runtime == "ember":
        return [ember, "run", f"target/bench-results/programs/{workload}.emb"]
    return [sys.executable, "bench/reference.py", workload]


def run_once(command: list[str]) -> int:
    started_ns = perf_counter_ns()
    completed = subprocess.run(
        command,
        cwd=REPOSITORY_ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    elapsed_ns = perf_counter_ns() - started_ns

    if completed.returncode != 0:
        raise RuntimeError(
            f"command failed with exit {completed.returncode}: {format_command(command)}\n"
            f"stderr: {completed.stderr.decode('utf-8', errors='replace')}"
        )
    if completed.stderr:
        raise RuntimeError(
            f"command wrote to stderr: {format_command(command)}\n"
            f"stderr: {completed.stderr.decode('utf-8', errors='replace')}"
        )
    stdout = completed.stdout.decode("utf-8", errors="replace")
    normalized_stdout = stdout.replace("\r\n", "\n")
    if normalized_stdout != EXPECTED_OUTPUT:
        raise RuntimeError(
            f"command stdout must be exactly 832040: {format_command(command)}\n"
            f"stdout: {stdout!r}"
        )
    return elapsed_ns


def format_command(command: list[str]) -> str:
    return subprocess.list2cmdline(command)


def median(values: list[int] | list[float]) -> float:
    return float(statistics.median(values))


def sample_statistics(samples_ns: list[int]) -> dict[str, float]:
    ordered = sorted(samples_ns)
    midpoint = median(ordered)
    deviations = [abs(value - midpoint) for value in ordered]
    p95_index = math.ceil(len(ordered) * 0.95) - 1

    return {
        "min_ms": ordered[0] / 1_000_000,
        "median_ms": midpoint / 1_000_000,
        "p95_ms": ordered[p95_index] / 1_000_000,
        "max_ms": ordered[-1] / 1_000_000,
        "mad_ms": median(deviations) / 1_000_000,
    }


def git_value(*args: str) -> str | None:
    try:
        completed = subprocess.run(
            ["git", *args],
            cwd=REPOSITORY_ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
    except OSError:
        return None
    if completed.returncode != 0:
        return None
    return completed.stdout.decode("utf-8", errors="replace").strip()


def metadata() -> dict[str, Any]:
    cpu_description = (
        platform.processor()
        or platform.uname().processor
        or os.environ.get("PROCESSOR_IDENTIFIER")
        or None
    )
    commit = git_value("rev-parse", "HEAD")
    dirty_output = git_value("status", "--porcelain")
    complete = all(
        (
            commit,
            dirty_output is not None,
            platform.python_version(),
            platform.platform(),
            platform.machine(),
            cpu_description,
        )
    )
    return {
        "generated_at_utc": datetime.now(UTC).isoformat(),
        "git_commit": commit,
        "git_dirty": None if dirty_output is None else bool(dirty_output),
        "python_version": platform.python_version(),
        "operating_system": platform.platform(),
        "architecture": platform.machine(),
        "cpu_description": cpu_description,
        "complete": complete,
    }


def render_markdown(results: list[dict[str, Any]], run_metadata: dict[str, Any]) -> str:
    lines = [
        "# Ember / CPython Fibonacci comparison",
        "",
        "> Warning: these are startup-inclusive, local measurements; process creation, module loading, and final output are included.",
        "",
        "The table is descriptive only. It does not make a runtime-superiority claim.",
        "",
        "| Workload | Runtime | Samples | Min (ms) | Median (ms) | P95 (ms) | Max (ms) | MAD (ms) |",
        "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for result in results:
        stats = result["statistics_ms"]
        lines.append(
            "| {workload} | {runtime} | {samples} | {min_ms:.3f} | {median_ms:.3f} | "
            "{p95_ms:.3f} | {max_ms:.3f} | {mad_ms:.3f} |".format(
                workload=result["workload"],
                runtime=result["runtime"],
                samples=len(result["samples_ns"]),
                **stats,
            )
        )
    lines.extend(
        [
            "",
            f"Metadata complete: {'yes' if run_metadata['complete'] else 'no'}.",
            f"Git commit: {run_metadata['git_commit'] or 'unavailable'}.",
        ]
    )
    return "\n".join(lines) + "\n"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--ember", required=True, help="path to the Ember release CLI")
    parser.add_argument("--warmup", type=nonnegative_int, default=5)
    parser.add_argument("--samples", type=positive_int, default=30)
    parser.add_argument("--output", required=True, type=Path)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    results: list[dict[str, Any]] = []
    for workload in WORKLOADS:
        for runtime in ("ember", "cpython"):
            command = command_for(runtime, workload, args.ember)
            for _ in range(args.warmup):
                run_once(command)
            samples_ns = [run_once(command) for _ in range(args.samples)]
            results.append(
                {
                    "workload": workload,
                    "runtime": runtime,
                    "command": command,
                    "samples_ns": samples_ns,
                    "statistics_ms": sample_statistics(samples_ns),
                }
            )

    run_metadata = metadata()
    output_dir = args.output
    output_dir.mkdir(parents=True, exist_ok=True)
    json_path = output_dir / "latest.json"
    markdown_path = output_dir / "latest.md"
    json_path.write_text(
        json.dumps(
            {
                "configuration": {"warmup": args.warmup, "samples": args.samples},
                "metadata": run_metadata,
                "results": results,
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    markdown_path.write_text(render_markdown(results, run_metadata), encoding="utf-8")
    print(f"Wrote {json_path}")
    print(f"Wrote {markdown_path}")


if __name__ == "__main__":
    main()
