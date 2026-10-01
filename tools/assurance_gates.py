#!/usr/bin/env python3
"""Validate fuzz/model inventory and instrumented coverage reports."""

from __future__ import annotations

import argparse
import json
from pathlib import Path, PurePosixPath
import re
from typing import Any


REGISTRY = "tools/assurance-gates.json"
HEX_TARGET = re.compile(r"[a-z][a-z0-9_]*\Z")


def read_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain one JSON object")
    return value


def safe_path(root: Path, raw: str, *, directory: bool = False) -> Path:
    relative = PurePosixPath(raw)
    if not raw or relative.is_absolute() or ".." in relative.parts or "\\" in raw:
        raise ValueError(f"unsafe assurance path: {raw!r}")
    path = root / raw
    if path.is_symlink() or (not path.is_dir() if directory else not path.is_file()):
        kind = "directory" if directory else "file"
        raise ValueError(f"assurance {kind} is missing or unsafe: {raw}")
    try:
        path.resolve().relative_to(root.resolve())
    except ValueError as error:
        raise ValueError(f"assurance path escapes the repository: {raw}") from error
    return path


def load_registry(root: Path) -> dict[str, Any]:
    registry = read_json(safe_path(root, REGISTRY))
    if registry.get("schema_version") != "mainframe-env.assurance-gates@1":
        raise ValueError("assurance registry schema is unsupported")
    return registry


def validate_inventory(root: Path) -> dict[str, int]:
    root = root.resolve()
    registry = load_registry(root)
    fuzz = registry.get("fuzz")
    model = registry.get("model")
    coverage = registry.get("coverage")
    if not all(isinstance(section, dict) for section in (fuzz, model, coverage)):
        raise ValueError("assurance registry sections are missing")
    if (
        fuzz.get("tool") != "cargo-fuzz"
        or fuzz.get("tool_version") != "0.13.2"
        or fuzz.get("toolchain") != "nightly-2026-09-01"
        or not isinstance(fuzz.get("max_input_bytes"), int)
        or not 1 <= fuzz["max_input_bytes"] <= 65_536
    ):
        raise ValueError("fuzz toolchain or input bound drifted")
    for field in ("smoke_runs_per_target", "periodic_runs_per_target"):
        if not isinstance(fuzz.get(field), int) or fuzz[field] <= 0:
            raise ValueError(f"fuzz {field} must be positive")
    if fuzz["periodic_runs_per_target"] <= fuzz["smoke_runs_per_target"]:
        raise ValueError("periodic fuzzing must exceed the smoke budget")

    targets = fuzz.get("targets")
    if not isinstance(targets, list) or len(targets) < 2:
        raise ValueError("parser and decoder fuzz targets are both required")
    manifest = safe_path(root, "fuzz/Cargo.toml").read_text(encoding="utf-8")
    names: set[str] = set()
    corpus_files = 0
    for target in targets:
        if not isinstance(target, dict):
            raise ValueError("fuzz target entry must be an object")
        name = target.get("name")
        source = target.get("source")
        corpus = target.get("corpus")
        if (
            not isinstance(name, str)
            or not HEX_TARGET.fullmatch(name)
            or name in names
            or not isinstance(source, str)
            or not isinstance(corpus, str)
        ):
            raise ValueError("fuzz target identity is malformed or duplicated")
        names.add(name)
        source_path = safe_path(root, source)
        corpus_path = safe_path(root, corpus, directory=True)
        if f'name = "{name}"' not in manifest or f'path = "fuzz_targets/{name}.rs"' not in manifest:
            raise ValueError(f"fuzz manifest omits target {name}")
        if "fuzz_target!" not in source_path.read_text(encoding="utf-8"):
            raise ValueError(f"fuzz target {name} does not invoke libFuzzer")
        seeds = [path for path in corpus_path.iterdir() if path.is_file() and not path.is_symlink()]
        if not seeds or any(path.stat().st_size == 0 for path in seeds):
            raise ValueError(f"fuzz target {name} has an empty corpus")
        corpus_files += len(seeds)

    required = {"cobol_parser", "ir_decoder", "cics_source_parser", "cics_plan_decoder"}
    if names != required:
        raise ValueError("COBOL, IR, CICS source, and CICS plan fuzz targets are required")

    if (
        model.get("tool") != "loom"
        or model.get("tool_version") != "0.7.2"
        or model.get("package") != "mainframe-env-store-api"
        or model.get("test_name") != "loom_durable_state"
        or not isinstance(model.get("minimum_tests"), int)
        or model["minimum_tests"] <= 0
        or not isinstance(model.get("test_target"), str)
    ):
        raise ValueError("Loom model declaration is malformed")
    model_source = safe_path(root, model["test_target"]).read_text(encoding="utf-8")
    model_tests = model_source.count("#[test]")
    if model_tests < model["minimum_tests"] or "loom::model" not in model_source:
        raise ValueError("Loom model gate is empty")

    roots = coverage.get("package_roots")
    if (
        coverage.get("tool") != "cargo-llvm-cov"
        or coverage.get("tool_version") != "0.9.1"
        or not isinstance(roots, list)
        or len(roots) < 3
        or any(not isinstance(path, str) for path in roots)
    ):
        raise ValueError("coverage declaration is incomplete")
    for path in roots:
        safe_path(root, path, directory=True)
    for field in ("minimum_total_lines", "minimum_covered_lines", "minimum_covered_functions"):
        if not isinstance(coverage.get(field), int) or coverage[field] <= 0:
            raise ValueError(f"coverage {field} must be positive")
    return {
        "fuzz_targets": len(targets),
        "corpus_files": corpus_files,
        "model_tests": model_tests,
        "coverage_packages": len(roots),
    }


def validate_coverage(root: Path, report_path: Path) -> dict[str, int | float]:
    validate_inventory(root)
    coverage = load_registry(root)["coverage"]
    report = read_json(report_path)
    data = report.get("data")
    if report.get("type") != "llvm.coverage.json.export" or not isinstance(data, list) or len(data) != 1:
        raise ValueError("coverage report is not one LLVM coverage export")
    result = data[0]
    totals = result.get("totals") if isinstance(result, dict) else None
    files = result.get("files") if isinstance(result, dict) else None
    if not isinstance(totals, dict) or not isinstance(files, list) or not files:
        raise ValueError("coverage report has no totals or files")
    observed: dict[str, int | float] = {}
    for name in ("lines", "functions"):
        metric = totals.get(name)
        if not isinstance(metric, dict):
            raise ValueError(f"coverage report omits {name}")
        count = metric.get("count")
        covered = metric.get("covered")
        percent = metric.get("percent")
        if (
            not isinstance(count, int)
            or not isinstance(covered, int)
            or not isinstance(percent, (int, float))
            or count <= 0
            or not 0 <= covered <= count
            or not 0 <= percent <= 100
        ):
            raise ValueError(f"coverage {name} totals are malformed or empty")
        observed[f"total_{name}"] = count
        observed[f"covered_{name}"] = covered
        observed[f"{name}_percent"] = round(float(percent), 4)

    if observed["total_lines"] < coverage["minimum_total_lines"]:
        raise ValueError("coverage report contains fewer lines than the recorded baseline")
    if observed["covered_lines"] < coverage["minimum_covered_lines"]:
        raise ValueError("covered lines fell below the recorded baseline")
    if observed["covered_functions"] < coverage["minimum_covered_functions"]:
        raise ValueError("covered functions fell below the recorded baseline")

    filenames = [entry.get("filename") for entry in files if isinstance(entry, dict)]
    for package in coverage["package_roots"]:
        marker = f"/{package}/"
        if not any(isinstance(name, str) and marker in name.replace("\\", "/") for name in filenames):
            raise ValueError(f"coverage report omits required package {package}")
    return observed


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("inventory")
    fuzz_plan = subparsers.add_parser("fuzz-plan")
    fuzz_plan.add_argument("--mode", choices=("smoke", "periodic"), required=True)
    subparsers.add_parser("coverage-plan")
    subparsers.add_parser("model-plan")
    coverage = subparsers.add_parser("coverage")
    coverage.add_argument("--report", type=Path, required=True)
    args = parser.parse_args(argv)
    if args.command == "inventory":
        result = validate_inventory(args.root)
        print(json.dumps(result, sort_keys=True))
    elif args.command == "fuzz-plan":
        validate_inventory(args.root)
        fuzz = load_registry(args.root)["fuzz"]
        runs = fuzz[f"{args.mode}_runs_per_target"]
        for target in fuzz["targets"]:
            print(
                "\t".join(
                    (
                        target["name"],
                        target["corpus"],
                        fuzz["toolchain"],
                        fuzz["tool_version"],
                        str(runs),
                        str(fuzz["max_input_bytes"]),
                    )
                )
            )
    elif args.command == "coverage-plan":
        validate_inventory(args.root)
        for path in load_registry(args.root)["coverage"]["package_roots"]:
            print(PurePosixPath(path).name)
    elif args.command == "model-plan":
        validate_inventory(args.root)
        model = load_registry(args.root)["model"]
        print(f"{model['package']}\t{model['test_name']}\t{model['minimum_tests']}")
    else:
        result = validate_coverage(args.root, args.report)
        print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, json.JSONDecodeError) as error:
        raise SystemExit(f"assurance gate failed closed: {error}")
