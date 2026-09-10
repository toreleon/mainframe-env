#!/usr/bin/env python3
"""Generate the frozen CICS application identity and runtime descriptor modules."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
CATALOG_PATH = Path("conformance/0.9/cics/command-descriptors.json")
OUTPUT_PATH = Path("crates/providers/mainframe-env-cics/src/generated/command_descriptors.rs")
HOST_OUTPUT_PATH = Path(
    "crates/contracts/mainframe-env-host-api/src/generated/cics_application_commands.rs"
)
SCHEMA_VERSION = "mainframe-env.cics-command-descriptors@2"
OFFICIAL_SCHEMA_VERSION = "mainframe-env.official-catalog@1"
OFFICIAL_BASELINE = "ibm-cics-ts-6x-2026-08-31"
OFFICIAL_CATALOG_DIGEST = (
    "sha256:fccd2a8e5cc24dd08aeb32754daf14ed80e9f1b20b5d9e762a1b0cfe429ceeba"
)
APPLICATION_DIGEST_DOMAIN = b"mainframe-env.cics-application-command-identities@2\0"
IDENTIFIER = re.compile(r"^[A-Z][A-Za-z0-9]*$")
FAMILY_ID = re.compile(r"^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$")
EIBFN = re.compile(r"^[0-9A-F]{4}$")
EXPECTED_UNITS = {
    "api-commands": (263, "dfha8mf__eibfn_table_cmds_api", "API"),
    "spi-commands-unique": (269, "dfha8mf__eibfn_table_cmds_spi", "SPI"),
    "fepi-commands": (39, "dfha8mf__eibfn_table_cmds_fepi", "FEPI"),
}
API_LOCATOR_FAMILY_EXCEPTIONS = {
    # The pinned application table itself labels this one locator as SPI. The
    # unit membership remains the denominator authority; preserve the source
    # bytes rather than silently rewriting the publication anomaly.
    f"{OFFICIAL_BASELINE}:api-commands:0193": "SPI",
}
OFFICIAL_EIBFN_SOURCE_EXCEPTIONS = {
    # The pinned SPI table prints this code with an embedded space. It is not
    # part of the application projection, but full unit validation must retain
    # and explicitly normalize the exact reviewed source form.
    f"{OFFICIAL_BASELINE}:spi-commands-unique:0192": "70 32",
}
EXPECTED_FAMILIES = {
    "task-control": "TaskControl",
    "time": "Time",
    "program-control": "ProgramControl",
    "terminal-control": "TerminalControl",
    "file-control": "FileControl",
    "queue-control": "QueueControl",
    "recovery": "Recovery",
}
EXPECTED_RUNTIME_OPERATIONS = [
    ("Abend", "api", "task-control", True, f"{OFFICIAL_BASELINE}:api-commands:0001"),
    ("Asktime", "api", "time", False, f"{OFFICIAL_BASELINE}:api-commands:0009"),
    ("Assign", "api", "task-control", False, f"{OFFICIAL_BASELINE}:api-commands:0011"),
    ("Delete", "api", "file-control", True, f"{OFFICIAL_BASELINE}:api-commands:0040"),
    ("EndBrowse", "api", "file-control", False, f"{OFFICIAL_BASELINE}:api-commands:0058"),
    ("FormatTime", "api", "time", False, f"{OFFICIAL_BASELINE}:api-commands:0080"),
    ("HandleAbend", "api", "task-control", False, f"{OFFICIAL_BASELINE}:api-commands:0097"),
    ("HandleCondition", "api", "task-control", False, f"{OFFICIAL_BASELINE}:api-commands:0099"),
    (
        "Inquire",
        "spi-compatibility",
        "program-control",
        False,
        f"{OFFICIAL_BASELINE}:spi-commands-unique:0155",
    ),
    ("Link", "api", "program-control", True, f"{OFFICIAL_BASELINE}:api-commands:0138"),
    ("Read", "api", "file-control", False, f"{OFFICIAL_BASELINE}:api-commands:0156"),
    ("ReadNext", "api", "file-control", False, f"{OFFICIAL_BASELINE}:api-commands:0157"),
    ("ReadPrev", "api", "file-control", False, f"{OFFICIAL_BASELINE}:api-commands:0158"),
    ("ReceiveMap", "api", "terminal-control", True, f"{OFFICIAL_BASELINE}:api-commands:0163"),
    ("Retrieve", "api", "task-control", False, f"{OFFICIAL_BASELINE}:api-commands:0175"),
    ("Return", "api", "task-control", True, f"{OFFICIAL_BASELINE}:api-commands:0178"),
    ("Rewrite", "api", "file-control", True, f"{OFFICIAL_BASELINE}:api-commands:0181"),
    ("SendMap", "api", "terminal-control", True, f"{OFFICIAL_BASELINE}:api-commands:0189"),
    ("SendText", "api", "terminal-control", True, f"{OFFICIAL_BASELINE}:api-commands:0192"),
    (
        "SetFileStatus",
        "spi-compatibility",
        "file-control",
        True,
        f"{OFFICIAL_BASELINE}:spi-commands-unique:0224",
    ),
    ("StartBrowse", "api", "file-control", False, f"{OFFICIAL_BASELINE}:api-commands:0208"),
    ("Syncpoint", "api", "recovery", True, f"{OFFICIAL_BASELINE}:api-commands:0218"),
    ("Write", "api", "file-control", True, f"{OFFICIAL_BASELINE}:api-commands:0253"),
    (
        "WriteTransientData",
        "api",
        "queue-control",
        True,
        f"{OFFICIAL_BASELINE}:api-commands:0257",
    ),
    ("Xctl", "api", "program-control", True, f"{OFFICIAL_BASELINE}:api-commands:0263"),
]


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


def _integer(value: Any, label: str) -> int:
    if not isinstance(value, int) or isinstance(value, bool):
        raise DescriptorError(f"{label} must be an integer")
    return value


def _read_json(path: Path) -> dict[str, Any]:
    try:
        return _object(json.loads(path.read_text()), str(path))
    except (OSError, json.JSONDecodeError) as error:
        raise DescriptorError(f"{path}: {error}") from error


def _official_units(
    root: Path, relative: Path, expected_digest: str
) -> tuple[dict[str, list[dict[str, Any]]], dict[str, dict[str, Any]]]:
    path = root / relative
    try:
        source = path.read_bytes()
    except OSError as error:
        raise DescriptorError(f"{path}: {error}") from error
    digest = f"sha256:{hashlib.sha256(source).hexdigest()}"
    if digest != expected_digest or digest != OFFICIAL_CATALOG_DIGEST:
        raise DescriptorError("official CICS catalog digest drifted")
    try:
        official = _object(json.loads(source), str(path))
    except json.JSONDecodeError as error:
        raise DescriptorError(f"{path}: {error}") from error
    if (
        official.get("schema_version") != OFFICIAL_SCHEMA_VERSION
        or official.get("baseline_id") != OFFICIAL_BASELINE
        or official.get("subsystem") != "cics"
        or official.get("mandatory_rows") != sum(row[0] for row in EXPECTED_UNITS.values())
    ):
        raise DescriptorError("official CICS catalog identity or denominator drifted")

    units: dict[str, list[dict[str, Any]]] = {}
    rows_by_id: dict[str, dict[str, Any]] = {}
    raw_units = _array(official.get("units"), "official units")
    if len(raw_units) != len(EXPECTED_UNITS):
        raise DescriptorError("official CICS catalog unit set drifted")
    for raw_unit in raw_units:
        unit = _object(raw_unit, "official unit")
        unit_id = _text(unit.get("id"), "official unit id")
        if unit_id in units or unit_id not in EXPECTED_UNITS:
            raise DescriptorError(f"unknown or duplicate official CICS unit {unit_id}")
        denominator, table, family = EXPECTED_UNITS[unit_id]
        if unit.get("denominator") != denominator:
            raise DescriptorError(f"official CICS unit {unit_id} denominator drifted")
        rows = _array(unit.get("rows"), f"official {unit_id} rows")
        if len(rows) != denominator:
            raise DescriptorError(f"official CICS unit {unit_id} row count drifted")
        normalized = []
        labels: set[str] = set()
        for ordinal, raw_row in enumerate(rows, 1):
            row = _object(raw_row, f"official {unit_id} row {ordinal}")
            expected_id = f"{OFFICIAL_BASELINE}:{unit_id}:{ordinal:04d}"
            row_id = _text(row.get("id"), f"official {unit_id} row id")
            label = _text(row.get("label"), f"official {unit_id} label")
            locator = _text(row.get("source_locator"), f"official {unit_id} locator")
            match = re.fullmatch(
                rf"html-table:{re.escape(table)};eibfn:([^;]+);family:([A-Z]+)",
                locator,
            )
            expected_family = API_LOCATOR_FAMILY_EXCEPTIONS.get(row_id, family)
            raw_eibfn = match.group(1) if match is not None else ""
            exceptional_eibfn = OFFICIAL_EIBFN_SOURCE_EXCEPTIONS.get(row_id)
            canonical_eibfn = raw_eibfn.replace(" ", "") if exceptional_eibfn else raw_eibfn
            if (
                row_id != expected_id
                or row.get("mandatory") is not True
                or match is None
                or match.group(2) != expected_family
                or (
                    exceptional_eibfn is not None
                    and raw_eibfn != exceptional_eibfn
                )
                or (
                    exceptional_eibfn is None
                    and EIBFN.fullmatch(raw_eibfn) is None
                )
                or EIBFN.fullmatch(canonical_eibfn) is None
                or row_id in rows_by_id
                or label in labels
            ):
                raise DescriptorError(f"official CICS row {expected_id} is not exact")
            normalized_row = {
                "official_row": row_id,
                "label": label,
                "eibfn": canonical_eibfn,
                "unit": unit_id,
            }
            normalized.append(normalized_row)
            rows_by_id[row_id] = normalized_row
            labels.add(label)
        units[unit_id] = normalized
    if set(units) != set(EXPECTED_UNITS):
        raise DescriptorError("official CICS catalog unit set is incomplete")
    return units, rows_by_id


def load_catalog(root: Path = ROOT) -> dict[str, Any]:
    path = root / CATALOG_PATH
    catalog = _read_json(path)
    expected = {
        "schema_version",
        "target_version",
        "official_catalog",
        "official_catalog_sha256",
        "application_catalog",
        "runtime",
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
    official_digest = _text(catalog["official_catalog_sha256"], "official_catalog_sha256")
    units, official_rows = _official_units(root, official_relative, official_digest)

    application = _object(catalog["application_catalog"], "application_catalog")
    application_fields = {
        "unit",
        "command_count",
        "automatic_registration",
        "generated_coverage_credit",
        "commands",
    }
    if set(application) != application_fields:
        raise DescriptorError("application_catalog fields differ from the contract")
    commands = _array(application["commands"], "application commands")
    official_api = units["api-commands"]
    if (
        application["unit"] != "api-commands"
        or _integer(application["command_count"], "application command_count") != 263
        or len(commands) != 263
        or application["automatic_registration"] is not False
        or _integer(application["generated_coverage_credit"], "generated coverage credit") != 0
    ):
        raise DescriptorError(
            "application catalog must remain 263 identities, unregistered, and zero-credit"
        )
    normalized_commands = []
    for ordinal, raw_command in enumerate(commands, 1):
        command = _object(raw_command, f"application commands[{ordinal - 1}]")
        if set(command) != {"official_row", "label", "eibfn"}:
            raise DescriptorError(f"application commands[{ordinal - 1}] fields differ")
        normalized = {
            "official_row": _text(command["official_row"], "application official_row"),
            "label": _text(command["label"], "application label"),
            "eibfn": _text(command["eibfn"], "application eibfn"),
        }
        expected_row = {
            key: official_api[ordinal - 1][key]
            for key in ("official_row", "label", "eibfn")
        }
        if normalized != expected_row or EIBFN.fullmatch(normalized["eibfn"]) is None:
            raise DescriptorError(
                f"application command ordinal {ordinal:04d} differs from its official row"
            )
        normalized_commands.append(normalized)

    runtime = _object(catalog["runtime"], "runtime")
    runtime_fields = {
        "operation_count",
        "api_operation_count",
        "spi_compatibility_operation_count",
        "families",
        "operations",
    }
    if set(runtime) != runtime_fields:
        raise DescriptorError("runtime fields differ from the contract")
    families: dict[str, str] = {}
    family_rows = _array(runtime["families"], "runtime families")
    for index, raw_family in enumerate(family_rows):
        family = _object(raw_family, f"runtime families[{index}]")
        if set(family) != {"id", "rust_variant", "responsibility"}:
            raise DescriptorError(f"runtime families[{index}] fields differ")
        family_id = _text(family["id"], f"runtime families[{index}].id")
        variant = _text(family["rust_variant"], f"runtime families[{index}].rust_variant")
        _text(family["responsibility"], f"runtime families[{index}].responsibility")
        if (
            FAMILY_ID.fullmatch(family_id) is None
            or IDENTIFIER.fullmatch(variant) is None
            or family_id in families
            or variant in families.values()
        ):
            raise DescriptorError(f"duplicate or invalid CICS runtime family {family_id}/{variant}")
        families[family_id] = variant
    if families != EXPECTED_FAMILIES:
        raise DescriptorError(f"CICS runtime families differ from the frozen layout: {families}")

    operations = _array(runtime["operations"], "runtime operations")
    if (
        _integer(runtime["operation_count"], "runtime operation_count") != 25
        or len(operations) != 25
        or _integer(runtime["api_operation_count"], "runtime api_operation_count") != 23
        or _integer(
            runtime["spi_compatibility_operation_count"],
            "runtime spi_compatibility_operation_count",
        )
        != 2
    ):
        raise DescriptorError("CICS runtime operation counts must remain exactly 25/23/2")
    operation_names: set[str] = set()
    runtime_rows: set[str] = set()
    interface_counts = {"api": 0, "spi-compatibility": 0}
    normalized_operations = []
    for index, raw_operation in enumerate(operations):
        operation = _object(raw_operation, f"runtime operations[{index}]")
        if set(operation) != {
            "operation",
            "interface",
            "family",
            "mutating",
            "official_row",
        }:
            raise DescriptorError(f"runtime operations[{index}] fields differ")
        name = _text(operation["operation"], f"runtime operations[{index}].operation")
        interface = _text(operation["interface"], f"runtime operations[{index}].interface")
        family = _text(operation["family"], f"runtime operations[{index}].family")
        official_row = _text(
            operation["official_row"], f"runtime operations[{index}].official_row"
        )
        official = official_rows.get(official_row)
        expected_unit = {
            "api": "api-commands",
            "spi-compatibility": "spi-commands-unique",
        }.get(interface)
        if (
            IDENTIFIER.fullmatch(name) is None
            or name in operation_names
            or official_row in runtime_rows
            or family not in families
            or not isinstance(operation["mutating"], bool)
            or official is None
            or official["unit"] != expected_unit
        ):
            raise DescriptorError(f"invalid CICS runtime operation {name}")
        operation_names.add(name)
        runtime_rows.add(official_row)
        interface_counts[interface] += 1
        normalized_operations.append(
            {
                "operation": name,
                "interface": interface,
                "family": family,
                "mutating": operation["mutating"],
                "official_row": official_row,
                "label": official["label"],
            }
        )
    if interface_counts != {"api": 23, "spi-compatibility": 2}:
        raise DescriptorError(f"CICS runtime interface split drifted: {interface_counts}")
    observed_runtime = [
        (
            row["operation"],
            row["interface"],
            row["family"],
            row["mutating"],
            row["official_row"],
        )
        for row in normalized_operations
    ]
    if observed_runtime != EXPECTED_RUNTIME_OPERATIONS:
        raise DescriptorError("CICS runtime operation compatibility set drifted")

    result = dict(catalog)
    result["_families"] = families
    result["_application_commands"] = normalized_commands
    result["_runtime_operations"] = normalized_operations
    return result


def _digest_field(hasher: Any, value: bytes) -> None:
    hasher.update(len(value).to_bytes(8, "big"))
    hasher.update(value)


def application_identity_digest(commands: list[dict[str, Any]]) -> str:
    """Return the logical, formatting-independent application identity digest."""
    hasher = hashlib.sha256()
    hasher.update(APPLICATION_DIGEST_DOMAIN)
    ordered = sorted(commands, key=lambda row: row["official_row"])
    hasher.update(len(ordered).to_bytes(8, "big"))
    for row in ordered:
        _digest_field(hasher, row["official_row"].encode())
        _digest_field(hasher, row["label"].encode())
        _digest_field(hasher, bytes.fromhex(row["eibfn"]))
    return f"sha256:{hasher.hexdigest()}"


def _rust_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=True)


def render_provider(root: Path = ROOT) -> str:
    catalog = load_catalog(root)
    family_variants = catalog["_families"]
    families = catalog["runtime"]["families"]
    operations = catalog["_runtime_operations"]
    lines = [
        "// @generated by `python3 -B tools/generate_cics_descriptors.py`; do not edit.",
        "",
        "use mainframe_env_host_api::CicsOperation;",
        "",
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]",
        "pub(crate) enum CicsCommandFamily {",
    ]
    lines.extend(f"    {row['rust_variant']}," for row in families)
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
    for operation in operations:
        lines.extend(
            [
                "    CicsCommandDescriptor {",
                f"        operation: CicsOperation::{operation['operation']},",
                f"        syntax: {_rust_string(operation['label'])},",
                f"        official_row: {_rust_string(operation['official_row'])},",
                f"        family: CicsCommandFamily::{family_variants[operation['family']]},",
                f"        mutating: {str(operation['mutating']).lower()},",
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
    for index, operation in enumerate(operations):
        lines.append(
            f"        CicsOperation::{operation['operation']} => &CICS_COMMAND_DESCRIPTORS[{index}],"
        )
    lines.extend(["    }", "}", ""])
    return "\n".join(lines)


def render_host(root: Path = ROOT) -> str:
    catalog = load_catalog(root)
    commands = catalog["_application_commands"]
    digest = application_identity_digest(commands)
    lines = [
        "// @generated by `python3 -B tools/generate_cics_descriptors.py`; do not edit.",
        "",
        "pub(super) const CICS_APPLICATION_COMMAND_IDENTITY_SET_SHA256: &str =",
        f"    {_rust_string(digest)};",
        "",
        "#[rustfmt::skip]",
        "pub(super) const CICS_APPLICATION_COMMAND_IDENTITIES: &[CicsApplicationCommandIdentityDescriptor] = &[",
    ]
    for command in commands:
        eibfn = bytes.fromhex(command["eibfn"])
        lines.append(
            "    CicsApplicationCommandIdentityDescriptor { "
            f"official_row: {_rust_string(command['official_row'])}, "
            f"label: {_rust_string(command['label'])}, "
            f"eibfn: [0x{eibfn[0]:02X}, 0x{eibfn[1]:02X}] "
            "},"
        )
    lines.extend(["];", ""])
    return "\n".join(lines)


def render(root: Path = ROOT) -> str:
    """Retain the historical provider-render helper for tooling callers."""
    return render_provider(root)


def rendered_outputs(root: Path = ROOT) -> dict[Path, str]:
    return {
        OUTPUT_PATH: render_provider(root),
        HOST_OUTPUT_PATH: render_host(root),
    }


def check(root: Path = ROOT) -> None:
    for relative, expected in rendered_outputs(root).items():
        output = root / relative
        try:
            actual = output.read_text()
        except OSError as error:
            raise DescriptorError(f"{output}: {error}") from error
        if actual != expected:
            raise DescriptorError(
                f"{relative} is stale; run python3 -B tools/generate_cics_descriptors.py"
            )


def generate(root: Path = ROOT) -> None:
    for relative, source in rendered_outputs(root).items():
        output = root / relative
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(source)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail when generated Rust is stale")
    args = parser.parse_args()
    if args.check:
        check()
        print("cics-command-descriptors: pass")
    else:
        generate()
        print(
            "cics-command-descriptors: generated "
            f"{OUTPUT_PATH} and {HOST_OUTPUT_PATH}"
        )


if __name__ == "__main__":
    main()
