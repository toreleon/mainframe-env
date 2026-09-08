#!/usr/bin/env python3
"""Generate the frozen CICS operation-to-family descriptor module."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
CATALOG_PATH = Path("conformance/0.9/cics/command-descriptors.json")
OUTPUT_PATH = Path("crates/providers/mainframe-env-cics/src/generated/command_descriptors.rs")
SCHEMA_VERSION = "mainframe-env.cics-command-descriptors@1"
IDENTIFIER = re.compile(r"^[A-Z][A-Za-z0-9]*$")
FAMILY_ID = re.compile(r"^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$")
EXPECTED_FAMILIES = {
    "task-control": "TaskControl",
    "time": "Time",
    "program-control": "ProgramControl",
    "terminal-control": "TerminalControl",
    "file-control": "FileControl",
    "queue-control": "QueueControl",
    "recovery": "Recovery",
}


class DescriptorError(ValueError):
    """The descriptor authority is malformed or its generated source is stale."""


def _object(value: Any, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise DescriptorError(f"{label} must be an object")
    return value


def _array(value: Any, label: str) -> list[Any]:
    if not isinstance(value, list):
        raise DescriptorError(f"{label} must be an array")
    return value


def _text(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise DescriptorError(f"{label} must be non-empty text")
    return value


def _read_json(path: Path) -> dict[str, Any]:
    try:
        return _object(json.loads(path.read_text()), str(path))
    except (OSError, json.JSONDecodeError) as error:
        raise DescriptorError(f"{path}: {error}") from error


def load_catalog(root: Path = ROOT) -> dict[str, Any]:
    path = root / CATALOG_PATH
    catalog = _read_json(path)
    expected = {
        "schema_version",
        "target_version",
        "official_catalog",
        "command_count",
        "families",
        "commands",
    }
    if set(catalog) != expected:
        raise DescriptorError(
            f"{path} fields differ: missing={sorted(expected - set(catalog))} "
            f"unknown={sorted(set(catalog) - expected)}"
        )
    if catalog["schema_version"] != SCHEMA_VERSION or catalog["target_version"] != "0.9.0":
        raise DescriptorError(f"{path} has an unsupported schema or target version")

    official_relative = Path(_text(catalog["official_catalog"], "official_catalog"))
    if official_relative.is_absolute() or ".." in official_relative.parts:
        raise DescriptorError("official_catalog must be repository-relative")
    official = _read_json(root / official_relative)
    if (
        official.get("schema_version") != "mainframe-env.official-catalog@1"
        or official.get("baseline_id") != "ibm-cics-ts-6x-2026-08-31"
        or official.get("subsystem") != "cics"
    ):
        raise DescriptorError("official CICS catalog identity drifted")
    official_rows = {}
    for unit in _array(official.get("units"), "official units"):
        for row in _array(_object(unit, "official unit").get("rows"), "official rows"):
            value = _object(row, "official row")
            official_rows[_text(value.get("id"), "official row id")] = _text(
                value.get("label"), "official row label"
            )

    families: dict[str, str] = {}
    for index, value in enumerate(_array(catalog["families"], "families")):
        family = _object(value, f"families[{index}]")
        if set(family) != {"id", "rust_variant", "responsibility"}:
            raise DescriptorError(f"families[{index}] fields differ from the contract")
        family_id = _text(family["id"], f"families[{index}].id")
        variant = _text(family["rust_variant"], f"families[{index}].rust_variant")
        _text(family["responsibility"], f"families[{index}].responsibility")
        if not FAMILY_ID.fullmatch(family_id) or not IDENTIFIER.fullmatch(variant):
            raise DescriptorError(f"families[{index}] has a non-canonical identity")
        if family_id in families or variant in families.values():
            raise DescriptorError(f"duplicate CICS family identity {family_id}/{variant}")
        families[family_id] = variant
    if families != EXPECTED_FAMILIES:
        raise DescriptorError(
            f"CICS command families differ from the frozen seven-family layout: {families}"
        )

    commands = _array(catalog["commands"], "commands")
    if catalog["command_count"] != len(commands) or len(commands) != 25:
        raise DescriptorError("CICS descriptor command_count must remain exactly 25")
    operations: set[str] = set()
    rows: set[str] = set()
    for index, value in enumerate(commands):
        command = _object(value, f"commands[{index}]")
        if set(command) != {"operation", "syntax", "family", "mutating", "official_row"}:
            raise DescriptorError(f"commands[{index}] fields differ from the contract")
        operation = _text(command["operation"], f"commands[{index}].operation")
        syntax = _text(command["syntax"], f"commands[{index}].syntax")
        family = _text(command["family"], f"commands[{index}].family")
        official_row = _text(command["official_row"], f"commands[{index}].official_row")
        if not IDENTIFIER.fullmatch(operation) or operation in operations:
            raise DescriptorError(f"duplicate or invalid CICS operation {operation}")
        if family not in families:
            raise DescriptorError(f"CICS operation {operation} names unknown family {family}")
        if not isinstance(command["mutating"], bool):
            raise DescriptorError(f"CICS operation {operation} mutating must be boolean")
        if official_row in rows or official_rows.get(official_row) != syntax:
            raise DescriptorError(
                f"CICS operation {operation} does not match unique official row {official_row}"
            )
        operations.add(operation)
        rows.add(official_row)
    return catalog


def _rust_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=True)


def render(root: Path = ROOT) -> str:
    catalog = load_catalog(root)
    family_variants = {row["id"]: row["rust_variant"] for row in catalog["families"]}
    lines = [
        "// @generated by `python3 -B tools/generate_cics_descriptors.py`; do not edit.",
        "",
        "use mainframe_env_host_api::CicsOperation;",
        "",
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]",
        "pub(crate) enum CicsCommandFamily {",
    ]
    lines.extend(f"    {row['rust_variant']}," for row in catalog["families"])
    lines.extend(
        [
            "}",
            "",
            "#[derive(Clone, Copy, Debug, Eq, PartialEq)]",
            "pub(crate) struct CicsCommandDescriptor {",
            "    pub(crate) operation: CicsOperation,",
            "    pub(crate) syntax: &'static str,",
            "    pub(crate) official_row: &'static str,",
            "    pub(crate) family: CicsCommandFamily,",
            "    pub(crate) mutating: bool,",
            "}",
            "",
            "pub(crate) const CICS_COMMAND_DESCRIPTORS: &[CicsCommandDescriptor] = &[",
        ]
    )
    for command in catalog["commands"]:
        lines.extend(
            [
                "    CicsCommandDescriptor {",
                f"        operation: CicsOperation::{command['operation']},",
                f"        syntax: {_rust_string(command['syntax'])},",
                f"        official_row: {_rust_string(command['official_row'])},",
                f"        family: CicsCommandFamily::{family_variants[command['family']]},",
                f"        mutating: {str(command['mutating']).lower()},",
                "    },",
            ]
        )
    lines.extend(
        [
            "];",
            "",
            "pub(crate) const fn command_descriptor(operation: CicsOperation) -> &'static CicsCommandDescriptor {",
            "    match operation {",
        ]
    )
    for index, command in enumerate(catalog["commands"]):
        lines.append(
            f"        CicsOperation::{command['operation']} => &CICS_COMMAND_DESCRIPTORS[{index}],"
        )
    lines.extend(["    }", "}", ""])
    return "\n".join(lines)


def check(root: Path = ROOT) -> None:
    output = root / OUTPUT_PATH
    try:
        actual = output.read_text()
    except OSError as error:
        raise DescriptorError(f"{output}: {error}") from error
    if actual != render(root):
        raise DescriptorError(
            f"{OUTPUT_PATH} is stale; run python3 -B tools/generate_cics_descriptors.py"
        )


def generate(root: Path = ROOT) -> None:
    output = root / OUTPUT_PATH
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(render(root))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail when generated Rust is stale")
    args = parser.parse_args()
    if args.check:
        check()
        print("cics-command-descriptors: pass")
    else:
        generate()
        print(f"cics-command-descriptors: generated {OUTPUT_PATH}")


if __name__ == "__main__":
    main()
