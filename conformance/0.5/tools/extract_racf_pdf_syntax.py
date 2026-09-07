#!/usr/bin/env python3
"""Project RACF command syntax blocks into a review grammar.

The RACF Command Language Reference does not draw railroad diagrams. Each
command carries a `Syntax` block in bracket notation:

    [subsystem-prefix]{ADDSD | AD}
          (profile-name-1 [/password] ...)
          [ ADDCATEGORY( category-name ... )]
          [ AT([ node].userid ... ) | ONLYAT([ node].userid ... )]
          [ ERASE ]

Operands are recognized at bracket depth one and parenthesis depth zero, so a
segment's sub-operands (`CICS(OPCLASS(...))`) stay attached to their segment
instead of being promoted to top level.

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

HEADER = re.compile(r"^\s*(?:\d+\s+)?z/OS Security Server RACF|^\s*Chapter \d+\.")
OPERAND = re.compile(r"^[A-Z][A-Z0-9]*(?:-[A-Z0-9]+)*$")
BLOCK_LINE = re.compile(r"^\s*[\[\])(|{}]")
def is_prose(line: str) -> bool:
    """A syntax block line is short and bracketed; prose runs on in words."""
    if "[" in line or "(" in line:
        return False
    return len(line.split()) >= 5
SECTION = re.compile(r"^\s*(Parameters|Operands|Description|Examples?|Authorization)\b")


def digest(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def pages(reader: PdfReader) -> list[str]:
    return [page.extract_text() or "" for page in reader.pages]


ANCHOR = "complete syntax of the"


def spaced(word: str) -> str:
    """Kerning splits words in the extracted text (`RV ARY`, `DISPLA Y`)."""
    return r"\s*".join(re.escape(character) for character in word)


def find_block(text: list[str], keyword: str) -> tuple[list[str], int, bool] | None:
    """Locate the `{KEYWORD ...}` syntax block.

    An occurrence preceded by the reference's own introduction line is
    preferred; an unanchored occurrence is still returned, but flagged so a
    reviewer can see it was matched on the brace alone.
    """
    opener = re.compile(r"\{\s*" + spaced(keyword) + r"\s*(\||\})")
    fallback: tuple[list[str], int, bool] | None = None
    for number, page in enumerate(text):
        lines = page.splitlines()
        for index, line in enumerate(lines):
            if not opener.search(line):
                continue
            lead = " ".join(lines[max(0, index - 6) : index])
            if number:
                lead += " " + " ".join(text[number - 1].splitlines()[-6:])
            anchored = ANCHOR in lead
            block = [line]
            cursor = index + 1
            source = lines
            page_number = number
            while True:
                if cursor >= len(source):
                    page_number += 1
                    if page_number >= len(text):
                        break
                    source = text[page_number].splitlines()
                    cursor = 0
                    continue
                candidate = source[cursor]
                if HEADER.match(candidate) or candidate.strip() == keyword:
                    cursor += 1
                    continue
                if SECTION.match(candidate):
                    break
                if not candidate.strip():
                    cursor += 1
                    continue
                if not BLOCK_LINE.match(candidate) and is_prose(candidate):
                    break
                block.append(candidate)
                cursor += 1
            if anchored:
                return block, number + 1, True
            if fallback is None:
                fallback = (block, number + 1, False)
    return fallback


def aliases_of(line: str, keyword: str) -> list[str]:
    match = re.search(r"\{([^}]*)\}", line)
    if not match:
        return []
    names = [part.strip() for part in match.group(1).split("|")]
    return [name for name in names if name and name != keyword and OPERAND.match(name)]


BRACKET_HEAD = re.compile(r"(?:\[|\|)\s*([A-Z][A-Z0-9]*(?:-[A-Z0-9]+)*)")
SEGMENT_OPEN = re.compile(r"^\s*\[\s*[A-Z][A-Z0-9-]*\s*\[?\s*\(\s*$")
SEGMENT_CLOSE = re.compile(r"^[)\]\s]+$")


def operands_of(block: list[str]) -> tuple[list[str], list[str]]:
    """Return top-level operands and the nested names their segments own.

    Line shape drives the split rather than character depth: the reference
    contains unbalanced syntax lines (`[ XRFSOFF( FORCE | NOFORCE ]`) that make
    a running parenthesis counter diverge for the rest of the command.
    """
    top: list[str] = []
    nested: list[str] = []
    depth = 0
    for line in block[1:]:
        stripped = line.strip()
        if not stripped:
            continue
        if depth and SEGMENT_CLOSE.match(stripped):
            depth -= 1
            continue
        target = nested if depth else top
        for name in BRACKET_HEAD.findall(line):
            if name not in target:
                target.append(name)
        if SEGMENT_OPEN.match(line):
            depth += 1
    return top, nested


def project(reader: PdfReader, text: list[str], family: dict[str, Any]) -> dict[str, Any]:
    keyword = family["keyword"]
    found = find_block(text, keyword)
    if found is None:
        return {
            "row_id": family["row_id"],
            "keyword": keyword,
            "located": False,
            "catalog_operands": family.get("operands", []),
        }
    block, page, anchored = found
    top, nested = operands_of(block)
    return {
        "row_id": family["row_id"],
        "keyword": keyword,
        "located": True,
        "anchored": anchored,
        "page": page,
        "source_aliases": aliases_of(block[0], keyword),
        "catalog_aliases": family.get("aliases", []),
        "source_operands": top,
        "source_nested_operands": nested,
        "catalog_operands": family.get("operands", []),
        "block_lines": len(block),
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
    text = pages(reader)
    catalog = json.loads(args.catalog.read_text(encoding="utf-8"))
    rows = [project(reader, text, family) for family in catalog["families"]]
    output = {
        "schema_version": "mainframe-env.racf-pdf-syntax-projection@1",
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
    located = sum(1 for row in rows if row["located"])
    print(f"families={len(rows)} located={located}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
