#!/usr/bin/env python3
"""Keep application and gateway production driving inside ExecutionCoordinator."""

from __future__ import annotations

import argparse
from pathlib import Path
import sys

from check_module_boundaries import terminal_test_suffix_start


def check_source(source: str) -> bool:
    # Reuse the module ratchet's brace/string-aware proof. An interior cfg(test)
    # marker is not permission to ignore production appended after the test block.
    marker = terminal_test_suffix_start(source)
    production = source if marker is None else source[:marker]
    return ".drive(" not in production


def check(root: Path) -> list[str]:
    failures = []
    for directory in ["crates/apps", "crates/gateways"]:
        source_root = root / directory
        if not source_root.is_dir():
            raise OSError(f"missing production source root: {source_root}")
        for path in sorted(source_root.rglob("*.rs")):
            if not check_source(path.read_text(encoding="utf-8")):
                failures.append(f"{path.relative_to(root)} drives a machine outside ExecutionCoordinator")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    try:
        failures = check(args.root.resolve())
    except (OSError, UnicodeDecodeError) as error:
        print(f"execution-route: {error}", file=sys.stderr)
        return 2
    for failure in failures:
        print(f"execution-route: {failure}", file=sys.stderr)
    if failures:
        return 1
    print("execution-route: pass (production sources; proven terminal unit tests excluded)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
