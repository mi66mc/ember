#!/usr/bin/env python3
"""Run a startup-inclusive Ember versus CPython Fibonacci comparison."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform
import statistics
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from time import perf_counter_ns
from typing import Any


EXPECTED_OUTPUT = "832040\n"
WORKLOADS = ("fib_inline", "fib_function")
REPOSITORY_ROOT = Path(__file__).resolve().parent.parent


def minimum_int(minimum: int):
    def parse(value: str) -> int:
        parsed = int(value)
        if parsed < minimum:
            raise argparse.ArgumentTypeError(f"must be at least {minimum}")
        return parsed

    return parse


def nonnegative_int(value: str) -> int:
    parsed = int(value)
    if parsed < 0:
        raise argparse.ArgumentTypeError("must not be negative")
    return parsed


class CommandFailure(RuntimeError):
    def __init__(
        self,
        command: list[str],
        exit_code: int | None,
        stdout: bytes | None,
        stderr: bytes | None,
        reason: str,
    ) -> None:
        self.command = command
        self.exit_code = exit_code
        self.stdout = (stdout or b"").decode("utf-8", errors="replace")
        self.stderr = (stderr or b"").decode("utf-8", errors="replace")
        self.reason = reason
        super().__init__(str(self))

    def __str__(self) -> str:
        return "\n".join(
            (
                f"reason: {self.reason}",
                f"command: {format_command(self.command)}",
                f"exit_code: {self.exit_code if self.exit_code is not None else 'unavailable'}",
                f"stdout: {self.stdout!r}",
                f"stderr: {self.stderr!r}",
            )
        )


def command_for(runtime: str, workload: str, ember: str) -> list[str]:
    if runtime == "ember":
        return [ember, "run", f"target/bench-results/programs/{workload}.emb"]
    return [sys.executable, "bench/reference.py", workload]


def run_once(
    command: list[str], timeout_seconds: int, process_options: dict[str, Any]
) -> int:
    started_ns = perf_counter_ns()
    try:
        completed = subprocess.run(
            command,
            cwd=REPOSITORY_ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout_seconds,
            check=False,
            **process_options,
        )
    except subprocess.TimeoutExpired as error:
        raise CommandFailure(
            command,
            None,
            error.stdout,
            error.stderr,
            f"timed out after {timeout_seconds} seconds",
        ) from error
    except (OSError, subprocess.SubprocessError) as error:
        raise CommandFailure(
            command,
            None,
            None,
            None,
            f"could not start command: {error}",
        ) from error
    elapsed_ns = perf_counter_ns() - started_ns

    if completed.returncode != 0:
        raise CommandFailure(
            command,
            completed.returncode,
            completed.stdout,
            completed.stderr,
            "command exited unsuccessfully",
        )
    if completed.stderr:
        raise CommandFailure(
            command,
            completed.returncode,
            completed.stdout,
            completed.stderr,
            "command wrote to stderr",
        )
    stdout = completed.stdout.decode("utf-8", errors="replace")
    normalized_stdout = stdout.replace("\r\n", "\n")
    if normalized_stdout != EXPECTED_OUTPUT:
        raise CommandFailure(
            command,
            completed.returncode,
            completed.stdout,
            completed.stderr,
            f"command stdout must be exactly {EXPECTED_OUTPUT.strip()}",
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


def executable_identity(executable: str) -> tuple[str, dict[str, Any]]:
    path = Path(executable)
    if not path.is_absolute():
        path = REPOSITORY_ROOT / path
    canonical_path = path.resolve(strict=True)
    executable_stat = canonical_path.stat()
    digest = hashlib.sha256()
    with canonical_path.open("rb") as executable_file:
        for block in iter(lambda: executable_file.read(1024 * 1024), b""):
            digest.update(block)
    return str(canonical_path), {
        "canonical_path": str(canonical_path),
        "size_bytes": executable_stat.st_size,
        "mtime_ns": executable_stat.st_mtime_ns,
        "sha256": digest.hexdigest(),
    }


def environment(ember_executable: dict[str, Any]) -> dict[str, Any]:
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
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "git_commit": commit,
        "git_dirty": None if dirty_output is None else bool(dirty_output),
        "python_version": platform.python_version(),
        "operating_system": platform.platform(),
        "architecture": platform.machine(),
        "cpu_description": cpu_description,
        "ember_executable": ember_executable,
        "complete": complete,
    }


def cpu_affinity_options(requested: int | None) -> tuple[dict[str, Any], dict[str, Any]]:
    if requested is None:
        return {"cpu_affinity": None, "cpu_affinity_status": "not_requested"}, {}

    if os.name == "nt":
        print(
            "warning: CPU affinity was requested, but standard Windows subprocess "
            "facilities cannot set it; continuing without CPU affinity.",
            file=sys.stderr,
        )
        return {
            "cpu_affinity": None,
            "cpu_affinity_status": "unavailable",
            "cpu_affinity_requested": requested,
        }, {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP}

    if hasattr(os, "sched_setaffinity"):
        return {
            "cpu_affinity": requested,
            "cpu_affinity_status": "applied",
        }, {"preexec_fn": lambda: os.sched_setaffinity(0, {requested})}

    print(
        f"warning: CPU affinity is unsupported on {sys.platform}; continuing without it.",
        file=sys.stderr,
    )
    return {
        "cpu_affinity": requested,
        "cpu_affinity_status": "unsupported",
    }, {}


def atomic_write(path: Path, contents: str) -> None:
    with tempfile.NamedTemporaryFile(
        mode="w",
        encoding="utf-8",
        dir=path.parent,
        prefix=f".{path.name}.",
        suffix=".tmp",
        delete=False,
    ) as temporary:
        temporary.write(contents)
        temporary.flush()
        os.fsync(temporary.fileno())
        temporary_path = Path(temporary.name)
    temporary_path.replace(path)


def render_markdown(results: list[dict[str, Any]], run_metadata: dict[str, Any]) -> str:
    ember_executable = run_metadata["ember_executable"]
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
            "",
            "Ember executable:",
            f"- Canonical path: `{ember_executable['canonical_path']}`",
            f"- Size: {ember_executable['size_bytes']} bytes",
            f"- Modification time: {ember_executable['mtime_ns']} ns since the Unix epoch",
            f"- SHA-256: `{ember_executable['sha256']}`",
        ]
    )
    return "\n".join(lines) + "\n"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--ember", required=True, help="path to the Ember release CLI")
    parser.add_argument("--warmup", type=minimum_int(1), default=5)
    parser.add_argument("--samples", type=minimum_int(10), default=30)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--timeout-seconds", type=minimum_int(1), default=30)
    parser.add_argument("--label", help="optional label recorded with the comparison")
    parser.add_argument("--cpu-affinity", type=nonnegative_int, metavar="N")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    affinity_configuration, process_options = cpu_affinity_options(args.cpu_affinity)
    output_dir = args.output
    json_path = output_dir / "latest.json"
    markdown_path = output_dir / "latest.md"
    try:
        ember_path, ember_executable = executable_identity(args.ember)
    except OSError as error:
        print(f"comparison failed; could not identify --ember: {error}", file=sys.stderr)
        raise SystemExit(1) from error
    results: list[dict[str, Any]] = []
    try:
        for workload in WORKLOADS:
            for runtime in ("ember", "cpython"):
                command = command_for(runtime, workload, ember_path)
                for _ in range(args.warmup):
                    run_once(command, args.timeout_seconds, process_options)
                samples_ns = [
                    run_once(command, args.timeout_seconds, process_options)
                    for _ in range(args.samples)
                ]
                results.append(
                    {
                        "workload": workload,
                        "runtime": runtime,
                        "command": command,
                        "samples_ns": samples_ns,
                        "statistics_ms": sample_statistics(samples_ns),
                    }
                )
    except CommandFailure as error:
        if json_path.exists() or markdown_path.exists():
            outcome = "previous latest reports were preserved"
        else:
            outcome = "no report was published"
        print(f"comparison failed; {outcome}:\n{error}", file=sys.stderr)
        raise SystemExit(1) from error

    run_environment = environment(ember_executable)
    output_dir.mkdir(parents=True, exist_ok=True)
    report = {
        "schema_version": 1,
        "environment": run_environment,
        "configuration": {
            "warmup": args.warmup,
            "samples": args.samples,
            "timeout_seconds": args.timeout_seconds,
            "label": args.label,
            **affinity_configuration,
        },
        "results": results,
    }
    atomic_write(
        json_path,
        json.dumps(report, indent=2) + "\n",
    )
    atomic_write(markdown_path, render_markdown(results, run_environment))
    print(f"Wrote {json_path}")
    print(f"Wrote {markdown_path}")


if __name__ == "__main__":
    main()
