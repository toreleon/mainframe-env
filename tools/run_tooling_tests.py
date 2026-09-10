#!/usr/bin/env python3
"""Discover and run every shipped Python or shell tooling test."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
from typing import BinaryIO, Iterable


UNITTEST_COUNT = re.compile(rb"^Ran ([0-9]+) tests? in ", re.MULTILINE)
UNITTEST_SKIPS = re.compile(rb"^OK \(skipped=([0-9]+)\)$", re.MULTILINE)


@dataclass(frozen=True)
class TestInventory:
    python_tests: tuple[str, ...]
    shell_tests: tuple[str, ...]
    shell_tools: tuple[str, ...]


@dataclass(frozen=True)
class RunSummary:
    executed: int
    skipped: int
    failed_files: tuple[str, ...]
    shell_syntax_checks: int


def _is_tool_test(path: PurePosixPath) -> bool:
    pairs = tuple(zip(path.parts, path.parts[1:]))
    stem = path.name.removesuffix(path.suffix)
    return (
        ("tools", "tests") in pairs
        and (stem.startswith("test_") or stem.endswith("_test"))
        and path.suffix in {".py", ".sh", ".bash"}
    )


def inventory_from_paths(paths: Iterable[str]) -> TestInventory:
    python_tests: list[str] = []
    shell_tests: list[str] = []
    shell_tools: list[str] = []
    for raw in sorted(set(paths)):
        path = PurePosixPath(raw)
        if not raw or path.is_absolute() or ".." in path.parts or "\\" in raw:
            raise ValueError(f"unsafe tracked path: {raw!r}")
        if _is_tool_test(path):
            if path.suffix == ".py":
                python_tests.append(raw)
            else:
                shell_tests.append(raw)
        if (("tools" in path.parts or path.parts[0] == "docker")
                and (path.suffix in {".sh", ".bash"} or raw == "docker/dev")):
            shell_tools.append(raw)
    return TestInventory(tuple(python_tests), tuple(shell_tests), tuple(shell_tools))


def discover(root: Path) -> TestInventory:
    root = root.resolve()
    output = subprocess.check_output(["git", "ls-files", "-z"], cwd=root)
    paths = [part.decode("utf-8") for part in output.split(b"\0") if part]
    inventory = inventory_from_paths(paths)
    for raw in (*inventory.python_tests, *inventory.shell_tests, *inventory.shell_tools):
        candidate = root / raw
        if candidate.is_symlink() or not candidate.is_file():
            raise ValueError(f"tracked tooling path is missing or is a symlink: {raw}")
        try:
            candidate.resolve().relative_to(root)
        except ValueError as error:
            raise ValueError(f"tracked tooling path escapes the repository: {raw}") from error
    if not inventory.python_tests and not inventory.shell_tests:
        raise ValueError("no shipped tooling tests were discovered")
    return inventory


def _run(command: list[str], root: Path, output: BinaryIO) -> tuple[int, bytes]:
    environment = os.environ.copy()
    environment["PYTHONDONTWRITEBYTECODE"] = "1"
    result = subprocess.run(
        command,
        cwd=root,
        env=environment,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    output.write(result.stdout)
    output.flush()
    return result.returncode, result.stdout


def run_inventory(
    root: Path,
    inventory: TestInventory,
    output: BinaryIO,
    python: str = sys.executable,
    bash: str | None = None,
) -> RunSummary:
    root = root.resolve()
    bash = bash or shutil.which("bash")
    if not bash:
        raise ValueError("bash is required to validate shipped shell tooling")
    failed: list[str] = []
    for path in inventory.shell_tools:
        code, _ = _run([bash, "-n", path], root, output)
        if code:
            failed.append(path)

    executed = 0
    skipped = 0
    for path in inventory.python_tests:
        output.write(f"tooling test file: {path}\n".encode())
        test = PurePosixPath(path)
        code, transcript = _run(
            [
                python,
                "-B",
                "-m",
                "unittest",
                "discover",
                "-s",
                str(test.parent),
                "-p",
                test.name,
            ],
            root,
            output,
        )
        counts = [int(match) for match in UNITTEST_COUNT.findall(transcript)]
        if code or len(counts) != 1 or counts[0] == 0:
            failed.append(path)
        else:
            executed += counts[0]
            skip = UNITTEST_SKIPS.findall(transcript)
            if skip:
                skipped += int(skip[-1])

    for path in inventory.shell_tests:
        output.write(f"tooling test file: {path}\n".encode())
        code, _ = _run([bash, path], root, output)
        if code:
            failed.append(path)
        else:
            executed += 1

    if executed == 0:
        failed.append("<empty-discovery>")
    return RunSummary(executed, skipped, tuple(sorted(set(failed))), len(inventory.shell_tools))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--list", action="store_true", help="print the discovered inventory as JSON")
    args = parser.parse_args(argv)
    inventory = discover(args.root)
    if args.list:
        print(json.dumps(inventory.__dict__, indent=2, sort_keys=True))
        return 0
    summary = run_inventory(args.root, inventory, sys.stdout.buffer)
    if summary.failed_files:
        print(
            "tooling test result: FAILED. "
            f"{summary.executed} executed; failed files: {', '.join(summary.failed_files)}"
        )
        return 1
    print(
        "tooling test result: ok. "
        f"{summary.executed} executed; {summary.skipped} skipped; "
        f"{len(inventory.python_tests)} python files; "
        f"{len(inventory.shell_tests)} shell test files; "
        f"{summary.shell_syntax_checks} shell syntax checks"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, UnicodeError, ValueError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"tooling test discovery failed closed: {error}")
