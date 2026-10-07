#!/usr/bin/env python3
"""Verify the identity-only SPI-1001 source authority without network access."""

from __future__ import annotations

import argparse
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re
from typing import Any


ROOT = Path(__file__).resolve().parents[5]
AUTHORITY_PATH = Path("conformance/subsystems/cics/system/cics/spi-fepi-source-authority.json")
CATALOG_PATH = Path("conformance/subsystems/coverage/catalogs/cics.json")
MANIFEST_PATH = Path("conformance/subsystems/coverage/manifests/cics-topics.json")
EXPECTED_BLOCKED_FACTS = [
    "grammar",
    "options",
    "resource_schema",
    "conditions",
    "execution_context",
    "authorized_intent",
    "audit_effect",
    "concurrency",
    "lock_order",
    "implicit_syncpoint",
    "quiesce_drain",
    "lifecycle",
    "restart",
    "recovery",
]
EXPECTED_DUPLICATES = {
    "INQUIRE NETNAME": ("5216", ["5206"]),
    "INQUIRE SYSTEM": ("5402", ["5412"]),
    "INQUIRE TERMINAL": ("5202", ["5212"]),
    "SET TERMINAL": ("5204", ["5214"]),
}
LOCATOR = re.compile(
    r"^html-table:(?P<table>[a-z0-9_]+);eibfn:(?P<eibfn>[0-9A-F ]+);family:(?P<family>SPI|FEPI)$"
)


class SourceAuthorityError(ValueError):
    """The reviewed source authority or retained source does not close."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SourceAuthorityError(message)


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SourceAuthorityError(f"invalid JSON {path}: {error}") from error
    require(isinstance(value, dict), f"{path} must contain an object")
    return value


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def verified_bytes(path: Path, expected_sha256: str, expected_size: int | None) -> bytes:
    try:
        body = path.read_bytes()
    except OSError as error:
        raise SourceAuthorityError(f"cannot read retained source {path}: {error}") from error
    require(
        sha256_bytes(body) == expected_sha256.removeprefix("sha256:"),
        f"retained source digest differs: {path}",
    )
    if expected_size is not None:
        require(len(body) == expected_size, f"retained source byte count differs: {path}")
    return body


def catalog_unit(catalog: dict[str, Any], unit_id: str) -> list[dict[str, Any]]:
    units = [unit for unit in catalog.get("units", []) if unit.get("id") == unit_id]
    require(len(units) == 1, f"official catalog unit is missing or duplicated: {unit_id}")
    rows = units[0].get("rows")
    require(isinstance(rows, list), f"official catalog unit has no rows: {unit_id}")
    return rows


def official_identity_rows(
    rows: list[dict[str, Any]], table_id: str, interface: str
) -> list[tuple[str, str, str]]:
    result: list[tuple[str, str, str]] = []
    for row in rows:
        row_id = row.get("id")
        label = row.get("label")
        locator = row.get("source_locator")
        require(
            isinstance(row_id, str) and isinstance(label, str) and isinstance(locator, str),
            "official CICS row shape differs",
        )
        match = LOCATOR.fullmatch(locator)
        require(match is not None, f"invalid official CICS source locator: {row_id}")
        require(match["table"] == table_id, f"official table differs: {row_id}")
        require(match["family"] == interface, f"official interface differs: {row_id}")
        raw_eibfn = match["eibfn"]
        eibfn = raw_eibfn.replace(" ", "")
        require(re.fullmatch(r"[0-9A-F]{4}", eibfn) is not None, f"invalid official EIBFN: {row_id}")
        require(
            raw_eibfn == eibfn
            or (
                row_id == "ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0192"
                and raw_eibfn == "70 32"
            ),
            f"unreviewed official EIBFN normalization: {row_id}",
        )
        result.append((row_id, label, eibfn))
    return result


def validate_projection_boundary(authority: dict[str, Any]) -> None:
    boundary = authority.get("projection_boundary", {})
    require(
        boundary.get("allowed_facts")
        == ["official_row", "command_label", "interface", "eibfn_identity"],
        "identity-only allowed facts drifted",
    )
    require(
        boundary.get("blocked_facts") == EXPECTED_BLOCKED_FACTS,
        "semantic source-gap dimensions drifted",
    )
    require(
        boundary.get("semantic_authority") is False
        and boundary.get("automatic_registration") is False
        and authority.get("coverage_credit") == 0,
        "source authority granted semantic, registration, or coverage credit",
    )


def validate_authority(root: Path = ROOT) -> tuple[dict[str, Any], dict[str, Any]]:
    authority_path = root / AUTHORITY_PATH
    authority = read_json(authority_path)
    catalog_path = root / CATALOG_PATH
    catalog_bytes = verified_bytes(
        catalog_path, authority["baseline"]["catalog_sha256"], None
    )
    catalog = json.loads(catalog_bytes)
    manifest = read_json(root / MANIFEST_PATH)

    require(authority.get("status") == "reviewed-identity-only", "source status drifted")
    validate_projection_boundary(authority)

    topics = manifest.get("topics")
    require(isinstance(topics, list) and len(topics) == 1, "CICS baseline topic set drifted")
    topic = authority["topic"]
    expected_topic = {
        "topic_path": topic["topic_path"],
        "sha256": topic["sha256"].removeprefix("sha256:"),
        "bytes": topic["bytes"],
        "last_modified": topic["last_modified"],
    }
    require(topics[0] == expected_topic, "source authority differs from baseline topic manifest")
    require(
        manifest.get("toc_sha256") == authority["baseline"]["toc_sha256"].removeprefix("sha256:")
        and manifest.get("toc_url") == authority["baseline"]["toc_url"],
        "source authority differs from baseline TOC pin",
    )

    for key, unit_id, interface, count in [
        ("spi", "spi-commands-unique", "SPI", 269),
        ("fepi", "fepi-commands", "FEPI", 39),
    ]:
        table = authority["tables"][key]
        rows = official_identity_rows(catalog_unit(catalog, unit_id), table["table_id"], interface)
        require(len(rows) == count, f"{interface} official denominator drifted")
        require(
            len({row_id for row_id, _, _ in rows}) == count
            and len({label for _, label, _ in rows}) == count,
            f"{interface} official identities are not unique",
        )
        require(
            table["official_denominator"] == count
            and table["unique_command_labels"] == count,
            f"{interface} authority denominator drifted",
        )

    duplicates = {
        row["label"]: (row["catalog_eibfn"], row["additional_source_eibfn"])
        for row in authority["tables"]["spi"]["duplicate_labels"]
    }
    require(duplicates == EXPECTED_DUPLICATES, "SPI duplicate-label disposition drifted")
    normalizations = authority["tables"]["spi"]["normalizations"]
    require(
        normalizations
        == [
            {
                "official_row": "ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0192",
                "source_eibfn": "70 32",
                "catalog_eibfn": "7032",
                "rule": "remove-source-whitespace",
            }
        ],
        "SPI EIBFN normalization disposition drifted",
    )

    gaps = authority.get("source_gaps")
    require(isinstance(gaps, list) and len(gaps) == 2, "source-gap set drifted")
    for gap, interface, count in zip(gaps, ("SPI", "FEPI"), (269, 39), strict=True):
        require(
            gap.get("interface") == interface
            and gap.get("affected_rows") == count
            and gap.get("required_topic_path") is None
            and gap.get("required_topic_sha256") is None
            and gap.get("state") == "blocked-unmapped-unpinned-command-bodies"
            and gap.get("dependent_facts") == EXPECTED_BLOCKED_FACTS,
            f"{interface} source gap does not fail closed",
        )
    return authority, catalog


class CommandTableParser(HTMLParser):
    """Extract only the two reviewed command identity tables."""

    def __init__(self, table_ids: set[str]) -> None:
        super().__init__()
        self.table_ids = table_ids
        self.active_table: str | None = None
        self.row: list[str] | None = None
        self.cell: list[str] | None = None
        self.rows: dict[str, list[list[str]]] = {table_id: [] for table_id in table_ids}

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        attributes = dict(attrs)
        if tag == "table" and attributes.get("id") in self.table_ids:
            require(self.active_table is None, "nested reviewed command tables are invalid")
            self.active_table = attributes["id"]
        elif self.active_table is not None and tag == "tr":
            require(self.row is None, "nested reviewed command rows are invalid")
            self.row = []
        elif self.row is not None and tag in {"th", "td"}:
            require(self.cell is None, "nested reviewed command cells are invalid")
            self.cell = []

    def handle_data(self, data: str) -> None:
        if self.cell is not None:
            self.cell.append(data)

    def handle_endtag(self, tag: str) -> None:
        if self.cell is not None and tag in {"th", "td"}:
            assert self.row is not None
            self.row.append(" ".join("".join(self.cell).split()))
            self.cell = None
        elif self.active_table is not None and tag == "tr" and self.row is not None:
            if self.row and self.row != ["Command", "EIBFN code", "Type"]:
                self.rows[self.active_table].append(self.row)
            self.row = None
        elif self.active_table is not None and tag == "table":
            self.active_table = None


def parse_command_tables(body: bytes, table_ids: set[str]) -> dict[str, list[list[str]]]:
    try:
        text = body.decode("utf-8")
    except UnicodeDecodeError as error:
        raise SourceAuthorityError("retained command topic is not UTF-8") from error
    parser = CommandTableParser(table_ids)
    parser.feed(text)
    parser.close()
    require(all(parser.rows.values()), "retained command topic omits a reviewed table")
    return parser.rows


def verify_table_projection(
    table: dict[str, Any], official: list[tuple[str, str, str]], source_rows: list[list[str]]
) -> None:
    interface = table["interface"]
    require(len(source_rows) == table["raw_rows"], f"{interface} raw row count drifted")
    require(
        all(len(row) == 3 and row[2] == interface for row in source_rows),
        f"{interface} retained table row shape or interface drifted",
    )
    source_by_label: dict[str, list[str]] = {}
    for label, raw_eibfn, _ in source_rows:
        normalized = raw_eibfn.replace(" ", "")
        require(re.fullmatch(r"[0-9A-F]{4}", normalized) is not None, f"invalid {interface} EIBFN")
        source_by_label.setdefault(label, []).append(normalized)
    require(
        len(source_by_label) == table["unique_command_labels"],
        f"{interface} unique label count drifted",
    )
    projected = [(label, codes[0]) for label, codes in source_by_label.items()]
    expected = [(label, eibfn) for _, label, eibfn in official]
    require(projected == expected, f"{interface} retained table differs from official catalog")
    duplicates = {
        label: (codes[0], codes[1:]) for label, codes in source_by_label.items() if len(codes) > 1
    }
    reviewed = {
        row["label"]: (row["catalog_eibfn"], row["additional_source_eibfn"])
        for row in table["duplicate_labels"]
    }
    require(duplicates == reviewed, f"{interface} duplicate-label identities drifted")


def verify_external_sources(
    authority: dict[str, Any], catalog: dict[str, Any], html_path: Path, toc_path: Path
) -> None:
    topic = authority["topic"]
    body = verified_bytes(html_path, topic["sha256"], topic["bytes"])
    verified_bytes(toc_path, authority["baseline"]["toc_sha256"], None)
    table_ids = {authority["tables"][key]["table_id"] for key in ("spi", "fepi")}
    parsed = parse_command_tables(body, table_ids)
    for key, unit_id, interface in [
        ("spi", "spi-commands-unique", "SPI"),
        ("fepi", "fepi-commands", "FEPI"),
    ]:
        table = authority["tables"][key]
        official = official_identity_rows(
            catalog_unit(catalog, unit_id), table["table_id"], interface
        )
        verify_table_projection(table, official, parsed[table["table_id"]])


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="verify committed authority")
    parser.add_argument("--html", type=Path, help="exact retained dfha8mf HTML body")
    parser.add_argument("--toc", type=Path, help="exact retained CICS TS 6.x TOC body")
    args = parser.parse_args()
    if not args.check or (args.html is None) != (args.toc is None):
        parser.error("use --check and supply both --html and --toc or neither")
    return args


def main() -> int:
    args = parse_args()
    try:
        authority, catalog = validate_authority()
        if args.html is not None and args.toc is not None:
            verify_external_sources(authority, catalog, args.html, args.toc)
        print(
            "SPI-1001 source authority verified: SPI 273 raw/269 unique; "
            "FEPI 39 raw/39 unique; semantic credit 0"
        )
        return 0
    except SourceAuthorityError as error:
        print(f"SPI-1001 source authority: {error}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
