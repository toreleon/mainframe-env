#!/usr/bin/env python3
"""Check every `pdf-page:N;outline:TITLE` row locator against its publication.

The 0.2 catalogs record where each row came from. For the PDF-backed baselines
that locator is an outline heading and the page it sits on, which makes the row
inventory directly checkable: the heading either exists at that page or it does
not.

This is a weaker claim than a syntax projection — it audits row *identity*, not
row *content* — but it applies uniformly to every PDF baseline, including the
ones with no syntax reader of their own (Db2, z/OSMF), and it is the check that
catches an inventory drifting off its publication.

Rows located some other way (`html-table:`, `roadmap-normalization:`, or the
JCL statement rows that cite a table rather than a heading) are reported as
skipped rather than silently ignored.

The emitted report is a review input. It grants no coverage credit and IBM
publication bytes are never written to the repository. Requires pypdf.
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

LOCATOR = re.compile(r"^pdf-page:(\d+);outline:(.*)$")


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
            found.append({"title": clean(str(item.title)), "page": page + 1, "depth": depth})

    walk(reader.outline)
    return found


def index(entries: list[dict[str, Any]]) -> dict[str, list[int]]:
    pages: dict[str, list[int]] = {}
    for entry in entries:
        pages.setdefault(entry["title"], []).append(entry["page"])
    return pages


def check(row: dict[str, Any], pages: dict[str, list[int]]) -> dict[str, Any]:
    locator = row["source_locator"]
    match = LOCATOR.match(locator)
    if not match:
        return {"id": row["id"], "verdict": "skipped", "locator": locator}
    page, title = int(match.group(1)), clean(match.group(2))
    found = pages.get(title)
    if not found:
        return {"id": row["id"], "verdict": "missing", "title": title, "page": page}
    if page in found:
        return {"id": row["id"], "verdict": "exact"}
    return {
        "id": row["id"],
        "verdict": "moved",
        "title": title,
        "page": page,
        "found_pages": found,
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
    pages = index(outline(reader))
    catalog = json.loads(args.catalog.read_text(encoding="utf-8"))

    units: list[dict[str, Any]] = []
    for unit in catalog["units"]:
        results = [check(row, pages) for row in unit["rows"]]
        counts: dict[str, int] = {}
        for result in results:
            counts[result["verdict"]] = counts.get(result["verdict"], 0) + 1
        units.append(
            {
                "unit": unit["id"],
                "rows": len(results),
                "counts": counts,
                "unresolved": [r for r in results if r["verdict"] in ("missing", "moved")],
            }
        )

    output = {
        "schema_version": "mainframe-env.outline-locator-audit@1",
        "coverage_credit": 0,
        "baseline_id": catalog["baseline_id"],
        "source": {
            "path": args.pdf.name,
            "sha256": digest(args.pdf),
            "pages": len(reader.pages),
            "outline_titles": len(pages),
            "retained_in_repository": False,
        },
        "units": units,
    }
    args.output.write_text(
        json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    for unit in units:
        counts = unit["counts"]
        print(
            f"{unit['unit']:34} rows={unit['rows']:4} "
            f"exact={counts.get('exact', 0):4} moved={counts.get('moved', 0):3} "
            f"missing={counts.get('missing', 0):3} skipped={counts.get('skipped', 0):3}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
