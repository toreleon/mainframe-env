#!/usr/bin/env python3
"""Generate the fail-closed SPI-1001 identity catalog."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import sys
from typing import Any


TOOLS = Path(__file__).resolve().parent
ROOT = TOOLS.parents[2]
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))
import verify_spi1001_source as source  # noqa: E402


OUTPUT_PATH = Path("conformance/0.10/generated/cics-spi-fepi-identity-catalog.json")
RUST_OUTPUT_PATH = Path(
    "crates/foundation/mainframe-env-ir/src/generated/cics_spi_fepi_registry.rs"
)
GRAMMAR_OUTPUT_PATH = Path(
    "crates/foundation/mainframe-env-ir/src/generated/cics_administrative_grammar.rs"
)
FAMILY_PATH = Path("conformance/0.10/cics/families")
FAMILY_ROWS = {
    "spi-program": ("spi", ["0026", "0084", "0155", "0241"]),
    "fepi-pool": ("fepi", ["0001", "0007", "0009", "0018", "0021", "0034"]),
    "spi-file": ("spi", ["0012", "0072", "0127", "0224"]),
    "fepi-resources": ("fepi", ["0008", "0010", "0011", "0017", "0019", "0020", "0022", "0023", "0024", "0032", "0033", "0036", "0037"]),
    "fepi-pool-list": ("fepi", ["0035"]),
}
GRAMMAR_DOMAIN = b"mainframe-env.cics-administrative-grammar@1\0"
SCHEMA_VERSION = "mainframe-env.cics-spi-fepi-identity-catalog@1"
DIGEST_DOMAIN = b"mainframe-env.cics-spi-fepi-identity-catalog@1\0"


class CatalogError(ValueError):
    """The identity catalog cannot be generated safely."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise CatalogError(message)


def canonical_bytes(value: object) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")


def pretty_bytes(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=False) + "\n").encode("utf-8")


def file_sha256(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def identity_digest(commands: list[dict[str, Any]]) -> str:
    digest = hashlib.sha256()
    digest.update(DIGEST_DOMAIN)
    digest.update(canonical_bytes(commands))
    return "sha256:" + digest.hexdigest()


def duplicate_eibfn_by_label(authority: dict[str, Any]) -> dict[str, list[str]]:
    return {
        row["label"]: list(row["additional_source_eibfn"])
        for row in authority["tables"]["spi"]["duplicate_labels"]
    }


def build_catalog(authority: dict[str, Any], official: dict[str, Any]) -> dict[str, Any]:
    source.validate_projection_boundary(authority)
    duplicate_codes = duplicate_eibfn_by_label(authority)
    commands: list[dict[str, Any]] = []
    unit_specs = [
        ("spi", "spi-commands-unique", "SPI", "SPI-1001.source-gap.spi-command-bodies", 269),
        ("fepi", "fepi-commands", "FEPI", "SPI-1001.source-gap.fepi-command-bodies", 39),
    ]
    for table_key, unit_id, interface, source_gap, count in unit_specs:
        table = authority["tables"][table_key]
        rows = source.official_identity_rows(
            source.catalog_unit(official, unit_id), table["table_id"], interface
        )
        require(len(rows) == count, f"{interface} denominator drifted")
        for official_row, label, eibfn in rows:
            commands.append(
                {
                    "official_row": official_row,
                    "interface": interface,
                    "label": label,
                    "label_tokens": label.split(),
                    "source_label_aliases": [],
                    "eibfn": eibfn,
                    "additional_eibfn_codes": duplicate_codes.get(label, [])
                    if interface == "SPI"
                    else [],
                    "shared_eibfn_rows": [],
                    "source": {
                        "topic_path": authority["topic"]["topic_path"],
                        "topic_sha256": authority["topic"]["sha256"],
                        "table_id": table["table_id"],
                        "source_locator": next(
                            row["source_locator"]
                            for row in source.catalog_unit(official, unit_id)
                            if row["id"] == official_row
                        ),
                    },
                    "semantic_contract": {
                        "state": "blocked-source-gap",
                        "source_gap": source_gap,
                        "blocked_facts": list(source.EXPECTED_BLOCKED_FACTS),
                    },
                    "runtime": {
                        "handler": None,
                        "advertised": False,
                        "automatically_registered": False,
                    },
                    "coverage_credit": 0,
                }
            )

    require(len(commands) == 308, "combined SPI/FEPI denominator drifted")
    require(
        len({command["official_row"] for command in commands}) == 308,
        "official SPI/FEPI row identities are duplicated",
    )
    require(
        len({(command["interface"], command["label"]) for command in commands}) == 308,
        "SPI/FEPI command labels are duplicated after reviewed deduplication",
    )
    by_eibfn: dict[tuple[str, str], list[str]] = {}
    for command in commands:
        by_eibfn.setdefault((command["interface"], command["eibfn"]), []).append(
            command["official_row"]
        )
    for command in commands:
        command["shared_eibfn_rows"] = [
            row
            for row in by_eibfn[(command["interface"], command["eibfn"])]
            if row != command["official_row"]
        ]

    return {
        "schema_version": SCHEMA_VERSION,
        "target_version": "0.10.0",
        "work_package": "SPI-1001.catalog",
        "status": "identity-only-source-gapped",
        "inputs": {
            "official_catalog": str(source.CATALOG_PATH),
            "official_catalog_sha256": authority["baseline"]["catalog_sha256"],
            "source_authority": str(source.AUTHORITY_PATH),
            "source_authority_sha256": "sha256:pending-generation",
        },
        "counts": {
            "spi": 269,
            "fepi": 39,
            "total": 308,
            "raw_source_rows": 312,
            "deduplicated_source_rows": 4,
        },
        "projection_boundary": {
            "semantic_authority": False,
            "automatic_registration": False,
            "public_routes": False,
            "runtime_handlers": 0,
        },
        "shared_authorities": dict(authority["shared_authorities"]),
        "commands": commands,
        "identity_sha256": identity_digest(commands),
        "coverage_credit": 0,
    }


def render(root: Path = ROOT) -> bytes:
    return pretty_bytes(final_catalog(root))


def final_catalog(root: Path = ROOT) -> dict[str, Any]:
    authority, official = source.validate_authority(root)
    catalog = build_catalog(authority, official)
    catalog["inputs"]["source_authority_sha256"] = file_sha256(root / source.AUTHORITY_PATH)
    return catalog


def rust_string(value: str) -> str:
    return json.dumps(value)


def rust_string_slice(values: list[str]) -> str:
    return "&[" + ", ".join(rust_string(value) for value in values) + "]"


def rust_eibfn(value: str) -> str:
    require(len(value) == 4, f"invalid generated EIBFN {value!r}")
    try:
        high = int(value[:2], 16)
        low = int(value[2:], 16)
    except ValueError as error:
        raise CatalogError(f"invalid generated EIBFN {value!r}") from error
    return f"[0x{high:02X}, 0x{low:02X}]"


def validate_non_routing_catalog(catalog: dict[str, Any]) -> None:
    require(
        catalog.get("status") == "identity-only-source-gapped"
        and catalog.get("coverage_credit") == 0,
        "generated registry input is not identity-only and zero-credit",
    )
    boundary = catalog.get("projection_boundary", {})
    require(
        boundary
        == {
            "semantic_authority": False,
            "automatic_registration": False,
            "public_routes": False,
            "runtime_handlers": 0,
        },
        "generated registry input grants a runtime or semantic surface",
    )
    commands = catalog.get("commands")
    require(isinstance(commands, list) and len(commands) == 308, "registry input count drifted")
    require(
        identity_digest(commands) == catalog.get("identity_sha256"),
        "registry input identity digest drifted",
    )
    for command in commands:
        require(
            command.get("runtime")
            == {"handler": None, "advertised": False, "automatically_registered": False}
            and command.get("coverage_credit") == 0
            and command.get("semantic_contract", {}).get("state") == "blocked-source-gap",
            f"registry input is executable or credited: {command.get('official_row')}",
        )


def render_rust_from_catalog(catalog: dict[str, Any]) -> bytes:
    validate_non_routing_catalog(catalog)
    commands = catalog["commands"]
    lines = [
        "// @generated by `python3 -B conformance/0.10/tools/generate_spi1001_catalog.py`; do not edit.",
        "",
        "/// Digest of the complete ordered SPI/FEPI identity registry.",
        f"pub const CICS_SPI_FEPI_IDENTITY_REGISTRY_SHA256: &str = {rust_string(catalog['identity_sha256'])};",
        "",
        "/// SHA-256 of the reviewed 0.10 source-authority artifact.",
        f"pub const CICS_SPI_FEPI_SOURCE_AUTHORITY_SHA256: &str = {rust_string(catalog['inputs']['source_authority_sha256'])};",
        "",
        "/// Exact IBM topic that establishes command labels, interfaces, and EIBFN identities.",
        f"pub const CICS_SPI_FEPI_SOURCE_TOPIC: &str = {rust_string(commands[0]['source']['topic_path'])};",
        "",
        "/// SHA-256 of the exact retained IBM identity topic.",
        f"pub const CICS_SPI_FEPI_SOURCE_TOPIC_SHA256: &str = {rust_string(commands[0]['source']['topic_sha256'])};",
        "",
        "/// Identity generation grants no semantic authority.",
        "pub const CICS_SPI_FEPI_SEMANTIC_AUTHORITY: bool = false;",
        "/// Identity generation never registers commands automatically.",
        "pub const CICS_SPI_FEPI_AUTOMATIC_REGISTRATION: bool = false;",
        "/// Identity generation advertises no public routes.",
        "pub const CICS_SPI_FEPI_PUBLIC_ROUTES: bool = false;",
        "/// No SPI or FEPI runtime handler is installed by this registry.",
        "pub const CICS_SPI_FEPI_RUNTIME_HANDLERS: usize = 0;",
        "/// Identity generation grants no conformance coverage credit.",
        "pub const CICS_SPI_FEPI_COVERAGE_CREDIT: usize = 0;",
        "",
        "#[rustfmt::skip]",
        "/// All source-backed SPI and FEPI identities in official-row order.",
        "pub const CICS_SPI_FEPI_IDENTITY_REGISTRY: &[CicsAdministrativeCommandIdentity] = &[",
    ]
    for command in commands:
        additional = "&[" + ", ".join(
            rust_eibfn(value) for value in command["additional_eibfn_codes"]
        ) + "]"
        interface = (
            "CicsAdministrativeInterface::Spi"
            if command["interface"] == "SPI"
            else "CicsAdministrativeInterface::Fepi"
        )
        lines.append(
            "    CicsAdministrativeCommandIdentity { "
            f"official_row: {rust_string(command['official_row'])}, "
            f"interface: {interface}, "
            f"label: {rust_string(command['label'])}, "
            f"label_tokens: {rust_string_slice(command['label_tokens'])}, "
            f"eibfn: {rust_eibfn(command['eibfn'])}, "
            f"additional_eibfn_codes: {additional}, "
            f"shared_eibfn_rows: {rust_string_slice(command['shared_eibfn_rows'])}, "
            f"source_gap: {rust_string(command['semantic_contract']['source_gap'])} "
            "},"
        )
    lines.append("];\n")
    return "\n".join(lines).encode("utf-8")


def render_rust(root: Path = ROOT) -> bytes:
    return render_rust_from_catalog(final_catalog(root))


def project_family_grammar(
    family: dict[str, Any], mapping: dict[str, Any], manifest: dict[str, Any]
) -> list[dict[str, Any]]:
    """Project product facts only; full instance validation belongs to xtask."""
    require(
        family.get("schema_version") == "mainframe-env.cics-system-family@1"
        and family.get("target_version") == "0.10.0"
        and family.get("runtime_binding") == "private-unregistered",
        "family grammar input is not a private 0.10 product contract",
    )
    name = family.get("family")
    require(name in FAMILY_ROWS, f"unknown family grammar: {name}")
    interface, suffixes = FAMILY_ROWS[name]
    unit = "spi-commands-unique" if interface == "spi" else "fepi-commands"
    expected = [f"ibm-cics-ts-6x-2026-08-31:{unit}:{suffix}" for suffix in suffixes]
    commands = family.get("commands", [])
    require(
        [command["official_row"] for command in commands] == expected,
        "family grammar row identity/order drifted",
    )
    mapped = {row["official_row"]: row for row in mapping["rows"]}
    pins = {topic["topic_path"]: topic for topic in manifest["topics"]}
    facts = []
    for command in commands:
        row = mapped[command["official_row"]]
        pin = command["source"]
        require(
            row["state"] == "mapped" and row["label"] == command["label"]
            and pin["topic_path"] == row["topic"]["topic_path"]
            and pin["sha256"] == row["topic"]["sha256"]
            and pin["sha256"] == "sha256:" + pins[pin["topic_path"]]["sha256"]
            and pin["baseline"] == manifest["baseline_id"],
            "family grammar source identity drifted",
        )
        grammar = command["grammar"]
        options = grammar["options"]
        names = [option["name"] for option in options]
        require(bool(names) and names == sorted(set(names)), "family options drifted")
        facts.append({
            "official_row": command["official_row"],
            "family": name,
            "label": command["label"],
            "source": {key: pin[key] for key in ("topic_path", "sha256", "baseline")},
            "grammar": {
                "options": [{key: option[key] for key in (
                    "name", "value_shape", "direction", "source_max_value_bytes"
                )} for option in options],
                "required": grammar["required"],
                "exclusive": grammar["exclusive"],
                "dependencies": grammar["dependencies"],
                "alternative_groups": grammar.get("alternative_groups", []),
            },
        })
    return facts


def family_grammar_facts(root: Path = ROOT) -> list[dict[str, Any]]:
    directory = root / FAMILY_PATH
    facts: list[dict[str, Any]] = []
    if not directory.exists():
        return facts
    for path in sorted(directory.iterdir()):
        require(
            path.is_file() and path.suffix == ".json" and path.stem in FAMILY_ROWS,
            f"unexpected family grammar input: {path}",
        )
        require(path.stat().st_size <= 4 * 1024 * 1024, "family input exceeds byte bound")
        family = json.loads(path.read_text())
        require(family["family"] == path.stem, "family filename/identity differs")
        interface = FAMILY_ROWS[path.stem][0]
        mapping = json.loads((root / f"conformance/0.10/cics/{interface}-command-source-map.json").read_text())
        manifest = json.loads((root / f"conformance/0.10/manifests/cics-{interface}-command-topics.json").read_text())
        facts.extend(project_family_grammar(family, mapping, manifest))
    return sorted(facts, key=lambda fact: fact["official_row"])


def render_grammar_facts(facts: list[dict[str, Any]]) -> bytes:
    shapes = {"flag": "Flag", "value": "Value", "optional-value": "OptionalValue", "unresolved": "BoundedAmbiguity"}
    directions = {"input": "Input", "output": "Output", "input-output": "InputOutput", "none": "None", "unresolved": "BoundedAmbiguity"}
    digest = "sha256:" + hashlib.sha256(GRAMMAR_DOMAIN + canonical_bytes(facts)).hexdigest()
    lines = [
        "// @generated by `python3 -B conformance/0.10/tools/generate_spi1001_catalog.py`; do not edit.",
        "",
        "/// Digest of source-linked product grammar facts; cases and verdicts are excluded.",
        f"pub const CICS_ADMINISTRATIVE_GRAMMAR_SHA256: &str = {rust_string(digest)};",
        "",
        "#[rustfmt::skip]",
        "/// Partial source-reviewed grammar contracts, with Pending completeness and no routes.",
        "pub const CICS_ADMINISTRATIVE_GRAMMAR_CONTRACTS: &[CicsAdministrativeGrammarContract] = &[",
    ]
    for fact in facts:
        grammar = fact["grammar"]
        lines.append("    CicsAdministrativeGrammarContract {")
        for field in ("official_row", "family", "label"):
            lines.append(f"        {field}: {rust_string(fact[field])},")
        for field, key in (("source_baseline", "baseline"), ("source_topic", "topic_path"), ("source_sha256", "sha256")):
            lines.append(f"        {field}: {rust_string(fact['source'][key])},")
        lines.append("        options: &[")
        for option in grammar["options"]:
            maximum = option["source_max_value_bytes"]
            maximum = "None" if maximum is None else f"Some({maximum})"
            lines.append(
                "            CicsApplicationOptionDescriptor { "
                f"name: {rust_string(option['name'])}, "
                f"value_shape: CicsApplicationOptionValueShape::{shapes[option['value_shape']]}, "
                f"direction: CicsApplicationOptionDirection::{directions[option['direction']]}, "
                f"source_max_value_bytes: {maximum} "
                "},"
            )
        lines.append("        ],")
        lines.append(f"        required_options: {rust_string_slice(grammar['required'])},")
        lines.append("        alternative_groups: &[")
        for group in grammar["alternative_groups"]:
            required = str(group["required"]).lower()
            require(type(group["required"]) is bool, "alternative requirement is not boolean")
            lines.append(
                "            CicsApplicationOptionAlternative { "
                f"members: {rust_string_slice(group['members'])}, required: {required} "
                "},"
            )
        lines.append("        ],")
        lines.append("        dependencies: &[")
        for dependency in grammar["dependencies"]:
            lines.append(
                "            CicsApplicationOptionDependency { "
                f"option: {rust_string(dependency['option'])}, "
                f"requires: {rust_string_slice(dependency['requires'])} "
                "},"
            )
        lines.append("        ],")
        exclusions = ", ".join(rust_string_slice(group) for group in grammar["exclusive"])
        lines.append(f"        mutual_exclusion_groups: &[{exclusions}],")
        lines.append("        constraint_status: CicsApplicationConstraintStatus::Pending,")
        lines.append("    },")
    lines.append("];\n")
    return "\n".join(lines).encode("utf-8")


def render_grammar(root: Path = ROOT) -> bytes:
    return render_grammar_facts(family_grammar_facts(root))


def generate(root: Path = ROOT) -> None:
    path = root / OUTPUT_PATH
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(render(root))
    rust_path = root / RUST_OUTPUT_PATH
    rust_path.parent.mkdir(parents=True, exist_ok=True)
    rust_path.write_bytes(render_rust(root))
    grammar_path = root / GRAMMAR_OUTPUT_PATH
    grammar_path.parent.mkdir(parents=True, exist_ok=True)
    grammar_path.write_bytes(render_grammar(root))


def check(root: Path = ROOT) -> None:
    path = root / OUTPUT_PATH
    require(path.is_file(), f"generated identity catalog is missing: {path}")
    require(path.read_bytes() == render(root), f"generated identity catalog is stale: {path}")
    rust_path = root / RUST_OUTPUT_PATH
    require(rust_path.is_file(), f"generated Rust identity registry is missing: {rust_path}")
    require(
        rust_path.read_bytes() == render_rust(root),
        f"generated Rust identity registry is stale: {rust_path}",
    )
    grammar_path = root / GRAMMAR_OUTPUT_PATH
    require(grammar_path.is_file(), f"generated grammar facts missing: {grammar_path}")
    require(grammar_path.read_bytes() == render_grammar(root), f"generated grammar facts stale: {grammar_path}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="verify generated bytes")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.check:
            check()
        else:
            generate()
        print("SPI-1001 catalog: 269 SPI + 39 FEPI identity rows; semantic credit 0")
        return 0
    except (CatalogError, source.SourceAuthorityError, OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        print(f"SPI-1001 catalog: {error}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
