#!/usr/bin/env python3
"""Project the Access Method Services parameter inventory from its PDF.

Each functional command owns a chapter, and inside it a `<CMD> Parameters`
section that splits into `Required Parameters` and `Optional Parameters`. Every
parameter is a heading set flush to the left margin:

    Optional Parameters
    ACCOUNT(account-info)
        Account is supported only for SMS-managed VSAM or non-VSAM data sets.
        account-info
            Use this to change accounting information ...
        Abbreviation: ACCT

Subparameters and prose are indented, so column position separates a parameter
from its own description. Abbreviations are collected too: they are part of the
command surface a catalog has to accept.

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

CHAPTER = re.compile(r"^Chapter\s+\d+\.\s+(.*)$")
COMMAND = re.compile(r"^[A-Z][A-Z0-9]*(?: [A-Z][A-Z0-9]*)*$")
PARAMETER_SECTION = re.compile(r"\bParameters\b")
GROUP = re.compile(r"^(Required|Optional) Parameters?\s*$")
EXAMPLES = re.compile(r"\bExamples?\b")
EXAMPLE_HEADING = re.compile(r"^[\w ]{0,40}\bExamples?$")
# Two characters minimum: the reference sets single-letter *values*
# (`AVGREC(U|K|M)`) on their own lines, and no AMS parameter is one character.
HEADING = re.compile(r"^([A-Z][A-Z0-9]+(?:-[A-Z0-9]+)*)\s*(?:\(|\||\s*$)")
ABBREVIATION = re.compile(r"^\s*Abbreviations?:\s*(.+)$")
FOOTER = re.compile(r"^\s*\d*\s*z/OS: z/OS DFSMS|^\s*Chapter \d+\.")


def digest(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def clean(value: str) -> str:
    return " ".join(value.replace("\xa0", " ").split())


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
            found.append({"title": clean(str(item.title)), "page": page, "depth": depth})

    walk(reader.outline)
    return found


def chapters(entries: list[dict[str, Any]]) -> dict[str, tuple[int, int]]:
    """Map a command name to the outline index range of its chapter."""
    starts: list[tuple[int, str]] = []
    for index, entry in enumerate(entries):
        if entry["depth"]:
            continue
        match = CHAPTER.match(entry["title"])
        if match and COMMAND.match(match.group(1).strip()):
            starts.append((index, match.group(1).strip()))
    spans: dict[str, tuple[int, int]] = {}
    for position, (index, name) in enumerate(starts):
        end = starts[position + 1][0] if position + 1 < len(starts) else len(entries)
        spans.setdefault(name, (index, end))
    return spans


def section_pages(entries: list[dict[str, Any]], span: tuple[int, int]) -> tuple[int, int] | None:
    """Return the half-open page range holding a chapter's parameter section."""
    start, end = span
    first: int | None = None
    for entry in entries[start:end]:
        if entry["depth"] != 1:
            continue
        if first is None and PARAMETER_SECTION.search(entry["title"]):
            first = entry["page"]
            continue
        if first is not None and EXAMPLES.search(entry["title"]):
            return first, entry["page"] + 1
    if first is None:
        return None
    last = entries[end]["page"] if end < len(entries) else None
    return first, last if last is not None else first + 1


def section_lines(lines: list[str]) -> list[str]:
    """Trim the page window down to the parameter groups themselves.

    A chapter's parameter section starts partway down a page that still
    carries the tail of a preceding table, and the tables put short words
    (`ALT`, `INDEX`, `TER`) in the left column where a parameter heading
    would be. Anchoring on the group headings drops that material.
    """
    first = next((index for index, line in enumerate(lines) if GROUP.match(line)), None)
    if first is None:
        return []
    tail = lines[first:]
    last = next(
        (index for index, line in enumerate(tail) if EXAMPLE_HEADING.match(line.strip())),
        len(tail),
    )
    return tail[:last]


def headings(lines: Iterable[str], command: str) -> tuple[list[str], list[str]]:
    """Collect flush-left parameter headings and the abbreviations they list."""
    names: list[str] = []
    abbreviations: list[str] = []
    for line in lines:
        listed = ABBREVIATION.match(line)
        if listed:
            for token in re.split(r"[,\s]+", listed.group(1)):
                token = token.strip().strip(".")
                if token and COMMAND.match(token) and token not in abbreviations:
                    abbreviations.append(token)
            continue
        if line[:1].isspace() or not line.strip():
            continue
        if FOOTER.match(line) or clean(line) == command:
            continue
        match = HEADING.match(line)
        if not match:
            continue
        name = match.group(1)
        if name == command or name in names:
            continue
        names.append(name)
    return names, abbreviations


def project(
    reader: PdfReader,
    entries: list[dict[str, Any]],
    spans: dict[str, tuple[int, int]],
    command: dict[str, Any],
) -> dict[str, Any]:
    name = command["label"]
    span = spans.get(name)
    pages = section_pages(entries, span) if span else None
    if pages is None:
        return {
            "id": command["id"],
            "label": name,
            "located": False,
            "catalog_parameters": command.get("parameters", []),
        }
    first, last = pages
    lines: list[str] = []
    for number in range(first, min(last, len(reader.pages))):
        lines.extend((reader.pages[number].extract_text() or "").splitlines())
    body = section_lines(lines)
    names, abbreviations = headings(body, name)
    grouped = [line.strip() for line in body if GROUP.match(line)]
    return {
        "id": command["id"],
        "label": name,
        "located": True,
        "page": first + 1,
        "pages_scanned": max(0, min(last, len(reader.pages)) - first),
        "groups_seen": grouped,
        "source_parameters": names,
        "source_abbreviations": abbreviations,
        "catalog_parameters": command.get("parameters", []),
    }


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
    rows = [project(reader, entries, spans, command) for command in catalog["commands"]]

    output = {
        "schema_version": "mainframe-env.ams-pdf-parameter-projection@1",
        "coverage_credit": 0,
        "source": {
            "path": args.pdf.name,
            "sha256": digest(args.pdf),
            "pages": len(reader.pages),
            "retained_in_repository": False,
        },
        "rows": rows,
    }
    args.output.write_text(
        json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    located = [row for row in rows if row["located"]]
    total = sum(len(row["source_parameters"]) for row in located)
    print(
        f"commands={len(rows)} located={len(located)} source_parameters={total} "
        f"catalog_parameters={sum(len(row['catalog_parameters']) for row in rows)}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
