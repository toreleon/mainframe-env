#!/usr/bin/env python3
"""Print the reviewable highlights of the COBOL grammar comparison."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--comparison", type=Path, required=True)
    parser.add_argument("--projection", type=Path, required=True)
    parser.add_argument("--top", type=int, default=12)
    args = parser.parse_args()

    comparison = json.loads(args.comparison.read_text(encoding="utf-8"))
    projection = json.loads(args.projection.read_text(encoding="utf-8"))
    catalog = {row["title"]: row for row in projection["rows"]}
    rows = sorted(
        comparison["rows"],
        key=lambda row: -len(row["keywords_missing_from_catalog"]),
    )

    print("statement            catalog  fmt  diagrams  missing")
    for row in rows[: args.top]:
        name = row["title"].replace(" statement", "")
        print(
            f"{name:20} {row['catalog_form_count']:7} "
            f"{len(row['source_format_titles']):4} {row['source_diagram_count']:9} "
            f"{len(row['keywords_missing_from_catalog']):8}  "
            + ", ".join(row["keywords_missing_from_catalog"][:6])
        )

    print("\nformat inventory gaps")
    for row in sorted(
        comparison["rows"],
        key=lambda row: -(row["source_diagram_count"] - row["catalog_form_count"]),
    )[:8]:
        name = row["title"].replace(" statement", "")
        print(
            f"  {name:20} catalog={row['catalog_form_count']} "
            f"titles={len(row['source_format_titles'])} "
            f"diagrams={row['source_diagram_count']}"
        )
        for title in row["source_format_titles"][:4]:
            print(f"      - {title}")

    exact = [
        row["title"].replace(" statement", "")
        for row in comparison["rows"]
        if not row["keywords_missing_from_catalog"]
    ]
    print(f"\ncatalog keyword-complete for {len(exact)} rows: {', '.join(exact)}")

    placeholders = sorted(
        {name for row in comparison["rows"] for name in row["catalog_placeholders"]}
    )
    operands = sorted(
        {name for row in comparison["rows"] for name in row["source_operands"]}
    )
    print(f"\ncatalog placeholders ({len(placeholders)}): {', '.join(placeholders)}")
    print(f"\nsource operands ({len(operands)}): {', '.join(operands)}")
    print(f"\ncatalog rows parsed from: {len(catalog)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
