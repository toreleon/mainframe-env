#!/usr/bin/env python3
"""Ratchet missing Rust API documentation across contract crates."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys


SCHEMA = "mainframe-env.public-api-doc-ratchet@1"


def load_policy(path: Path) -> dict[str, int]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if value.get("schema_version") != SCHEMA or set(value) != {"schema_version", "packages"}:
        raise ValueError("public API documentation policy has an incompatible shape")
    packages = value["packages"]
    if (
        not isinstance(packages, dict)
        or not packages
        or any(
            not isinstance(name, str)
            or not name.startswith("mainframe-env-")
            or not isinstance(limit, int)
            or isinstance(limit, bool)
            or limit < 0
            for name, limit in packages.items()
        )
    ):
        raise ValueError("public API documentation policy contains an invalid package limit")
    return dict(sorted(packages.items()))


def contract_packages(metadata: dict, root: Path) -> dict[str, Path]:
    workspace_members = set(metadata.get("workspace_members", []))
    contracts = {}
    contract_root = (root / "crates" / "contracts").resolve()
    for package in metadata.get("packages", []):
        if package.get("id") not in workspace_members:
            continue
        manifest = Path(package["manifest_path"]).resolve()
        try:
            manifest.relative_to(contract_root)
        except ValueError:
            continue
        contracts[package["name"]] = manifest
    return dict(sorted(contracts.items()))


def missing_doc_counts(lines: list[str], manifests: dict[str, Path]) -> tuple[dict[str, int], set[str]]:
    by_manifest = {str(path): name for name, path in manifests.items()}
    counts = {name: 0 for name in manifests}
    observed: set[str] = set()
    for line in lines:
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        manifest = message.get("manifest_path")
        package = by_manifest.get(str(Path(manifest).resolve())) if manifest else None
        if package is None:
            continue
        if message.get("reason") in {"compiler-artifact", "compiler-message"}:
            observed.add(package)
        diagnostic = message.get("message", {})
        diagnostic_code = diagnostic.get("code") or {}
        if (
            message.get("reason") == "compiler-message"
            and diagnostic_code.get("code") == "missing_docs"
            and diagnostic.get("level") == "warning"
        ):
            counts[package] += 1
    return counts, observed


def enforce(policy: dict[str, int], counts: dict[str, int], observed: set[str]) -> list[str]:
    failures = []
    for package, maximum in policy.items():
        if package not in observed:
            failures.append(f"{package}: rustdoc emitted no package result")
            continue
        actual = counts.get(package, 0)
        if actual > maximum:
            failures.append(f"{package}: {actual} undocumented items exceeds ratchet {maximum}")
        elif actual < maximum:
            failures.append(
                f"{package}: documentation improved to {actual}; lower the ratchet from {maximum}"
            )
    return failures


def run(root: Path, policy_path: Path) -> int:
    policy = load_policy(policy_path)
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
            cwd=root,
            text=True,
        )
    )
    manifests = contract_packages(metadata, root)
    if set(policy) != set(manifests):
        missing = sorted(set(manifests) - set(policy))
        stale = sorted(set(policy) - set(manifests))
        raise ValueError(f"contract package policy mismatch: missing={missing} stale={stale}")

    command = [
        "cargo",
        "doc",
        "--all-features",
        "--no-deps",
        "--locked",
        "--message-format=json",
    ]
    for package in policy:
        command.extend(["--package", package])
    environment = os.environ.copy()
    environment["RUSTDOCFLAGS"] = "-W missing-docs"
    environment["CARGO_TERM_COLOR"] = "never"
    completed = subprocess.run(
        command,
        cwd=root,
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if completed.returncode != 0:
        sys.stderr.write(completed.stderr)
        return completed.returncode
    counts, observed = missing_doc_counts(completed.stdout.splitlines(), manifests)
    failures = enforce(policy, counts, observed)
    for package in policy:
        print(f"{package}: missing_docs={counts[package]} ratchet={policy[package]}")
    if failures:
        print("public API documentation ratchet failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1
    print("public API documentation ratchet: pass")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--policy", type=Path)
    arguments = parser.parse_args()
    root = arguments.root.resolve()
    policy = arguments.policy or root / "tools" / "public-api-docs.json"
    try:
        return run(root, policy.resolve())
    except (OSError, subprocess.SubprocessError, ValueError, json.JSONDecodeError) as error:
        print(f"public API documentation ratchet: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
