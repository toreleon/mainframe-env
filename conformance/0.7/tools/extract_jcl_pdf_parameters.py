#!/usr/bin/env python3
"""Project the JCL Reference parameter inventory from its PDF outline.

The MVS JCL Reference gives every statement its own chapter and every
parameter its own outline entry (`ACCODE parameter`, `AMP parameter`, ...).
That structure is the published inventory, so this projection reads the
outline rather than parsing syntax art.

The emitted projection is a review input. It grants no coverage credit, is not
a normative catalog, and IBM publication bytes are never written to the
repository. Requires pypdf.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import logging
import re
from pathlib import Path
from typing import Any, Iterable

logging.disable(logging.WARNING)

from pypdf import PdfReader

CHAPTER = re.compile(r"^Chapter\s+(\d+)\.\s+(.*)$")
PARAMETER = re.compile(r"^(.+?)\s+parameters?$", re.IGNORECASE)

# Catalog unit -> the chapter title that owns its parameters.
UNITS = {
    "dd-parameters": "DD statement",
    "exec-parameters": "EXEC statement",
    "job-parameters": "JOB statement",
    "output-parameters": "OUTPUT JCL statement",
}


def digest(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def outline(reader: PdfReader) -> list[dict[str, Any]]:
    found: list[dict[str, Any]] = []

    def walk(items: Any, depth: int = 0) -> None:
        for item in items:
            if isinstance(item, list):
                walk(item, depth + 1)
                continue
            try:
                page = reader.get_destination_page_number(item)
            except Exception:  # noqa: BLE001 - malformed outline entries are skipped
                continue
            found.append({"title": str(item.title).strip(), "page": page, "depth": depth})

    walk(reader.outline)
    return found


def chapters(entries: list[dict[str, Any]]) -> dict[str, tuple[int, int]]:
    """Map a chapter title to its half-open outline index range."""
    starts: list[tuple[int, str]] = []
    for index, entry in enumerate(entries):
        match = CHAPTER.match(entry["title"])
        if match and entry["depth"] == 0:
            starts.append((index, match.group(2).strip()))
    spans: dict[str, tuple[int, int]] = {}
    for position, (index, title) in enumerate(starts):
        end = starts[position + 1][0] if position + 1 < len(starts) else len(entries)
        spans.setdefault(title, (index, end))
    return spans


def parameters(entries: list[dict[str, Any]], span: tuple[int, int]) -> list[dict[str, Any]]:
    start, end = span
    found: list[dict[str, Any]] = []
    seen: set[str] = set()
    for entry in entries[start:end]:
        if entry["depth"] != 1:
            continue
        match = PARAMETER.match(entry["title"])
        if not match:
            continue
        name = match.group(1).strip().upper()
        if name in seen:
            continue
        seen.add(name)
        found.append({"name": name, "page": entry["page"] + 1, "title": entry["title"]})
    return found


SUFFIX = re.compile(r"\s+parameters?$", re.IGNORECASE)


def normalize(label: str) -> str:
    """Catalog labels keep the reference's own `X parameter` outline wording."""
    return SUFFIX.sub("", label.strip()).strip().upper()


def catalog_names(catalog: dict[str, Any], unit_id: str) -> list[str]:
    for unit in catalog["units"]:
        if unit["id"] == unit_id:
            return [normalize(row["label"]) for row in unit["rows"]]
    raise ValueError(f"catalog unit not found: {unit_id}")


def parse_args(argv: Iterable[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--pdf", type=Path, required=True)
    parser.add_argument("--catalog", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args(list(argv) if argv is not None else None)


def main(argv: Iterable[str] | None = None) -> int:
    args = parse_args(argv)
    reader = PdfReader(str(args.pdf))
    entries = outline(reader)
    spans = chapters(entries)
    catalog = json.loads(args.catalog.read_text(encoding="utf-8"))

    units: list[dict[str, Any]] = []
    for unit_id, chapter in UNITS.items():
        span = spans.get(chapter)
        source = parameters(entries, span) if span else []
        names = {item["name"] for item in source}
        recorded = catalog_names(catalog, unit_id)
        catalog_set = set(recorded)
        units.append(
            {
                "unit": unit_id,
                "chapter": chapter,
                "chapter_located": span is not None,
                "catalog_count": len(recorded),
                "source_count": len(source),
                "source_parameters": source,
                "only_in_source": sorted(names - catalog_set),
                "only_in_catalog": sorted(catalog_set - names),
                "shared": len(names & catalog_set),
            }
        )

    output = {
        "schema_version": "mainframe-env.jcl-pdf-parameter-projection@1",
        "coverage_credit": 0,
        "source": {
            "path": args.pdf.name,
            "sha256": digest(args.pdf),
            "pages": len(reader.pages),
            "outline_entries": len(entries),
            "retained_in_repository": False,
        },
        "units": units,
    }
    args.output.write_text(
        json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    for unit in units:
        print(
            f"{unit['unit']:20} catalog={unit['catalog_count']:3} "
            f"source={unit['source_count']:3} shared={unit['shared']:3} "
            f"+src={len(unit['only_in_source']):3} +cat={len(unit['only_in_catalog']):3}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
