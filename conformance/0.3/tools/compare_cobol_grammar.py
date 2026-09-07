#!/usr/bin/env python3
"""Diff the reviewed COBOL language catalog against a PDF grammar projection.

The catalog forms in `conformance/0.3/cobol/language.json` are reviewed prose
sketches.  This tool reports where the source diagrams carry structure the
sketch does not: extra formats, keywords absent from the sketch, and operands
the sketch folds into an undefined placeholder.

Diagnostic only.  It grants no coverage credit and proposes no catalog rows.
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Any, Iterable

TOKEN = re.compile(r"[A-Za-z][A-Za-z0-9-]*")
PLACEHOLDER = re.compile(r"^[a-z][a-z0-9-]*$")


def keywords_of(form: dict[str, Any]) -> set[str]:
    values = {item["value"] for item in form["main_line"] if item["kind"] == "keyword"}
    values |= {
        item["value"] for item in form["branches"] if item["kind"] == "keyword"
    }
    return {value for value in values if not value.startswith("PHRASE")}


def operands_of(form: dict[str, Any]) -> set[str]:
    values = {item["value"] for item in form["main_line"] if item["kind"] == "operand"}
    values |= {item["value"] for item in form["branches"] if item["kind"] == "operand"}
    return values


def catalog_tokens(forms: Iterable[str]) -> tuple[set[str], set[str]]:
    keywords: set[str] = set()
    placeholders: set[str] = set()
    for form in forms:
        for token in TOKEN.findall(form):
            if token.isupper():
                keywords.add(token)
            elif PLACEHOLDER.match(token):
                placeholders.add(token)
    return keywords, placeholders


def compare(row: dict[str, Any]) -> dict[str, Any]:
    catalog_forms = row.get("catalog_forms", [])
    catalog_keywords, catalog_placeholders = catalog_tokens(catalog_forms)
    source_keywords: set[str] = set()
    source_operands: set[str] = set()
    for form in row["forms"]:
        source_keywords |= keywords_of(form)
        source_operands |= operands_of(form)
    return {
        "id": row["id"],
        "row_id": row["row_id"],
        "title": row["title"],
        "catalog_form_count": len(catalog_forms),
        "source_diagram_count": len(row["forms"]),
        "source_format_titles": row.get("format_titles", []),
        "catalog_keywords": sorted(catalog_keywords),
        "source_keywords": sorted(source_keywords),
        "keywords_missing_from_catalog": sorted(source_keywords - catalog_keywords),
        "keywords_absent_from_source": sorted(catalog_keywords - source_keywords),
        "catalog_placeholders": sorted(catalog_placeholders),
        "source_operands": sorted(source_operands),
        "alternatives": sorted(
            {
                item["value"]
                for form in row["forms"]
                for item in form["branches"]
                if item["relation"] == "alternative"
            }
        ),
        "optionals": sorted(
            {
                item["value"]
                for form in row["forms"]
                for item in form["branches"]
                if item["relation"] == "optional"
            }
        ),
    }


def parse_args(argv: Iterable[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--projection", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args(list(argv) if argv is not None else None)


def main(argv: Iterable[str] | None = None) -> int:
    args = parse_args(argv)
    projection = json.loads(args.projection.read_text(encoding="utf-8"))
    rows = [compare(row) for row in projection["rows"]]
    totals = {
        "rows": len(rows),
        "catalog_forms": sum(row["catalog_form_count"] for row in rows),
        "source_diagrams": sum(row["source_diagram_count"] for row in rows),
        "source_format_titles": sum(len(row["source_format_titles"]) for row in rows),
        "rows_with_more_source_formats": sum(
            1
            for row in rows
            if row["source_diagram_count"] > row["catalog_form_count"]
        ),
        "rows_with_missing_keywords": sum(
            1 for row in rows if row["keywords_missing_from_catalog"]
        ),
        "distinct_keywords_missing": len(
            {
                keyword
                for row in rows
                for keyword in row["keywords_missing_from_catalog"]
            }
        ),
        "distinct_source_operands": len(
            {operand for row in rows for operand in row["source_operands"]}
        ),
        "distinct_catalog_placeholders": len(
            {name for row in rows for name in row["catalog_placeholders"]}
        ),
    }
    report = {
        "schema_version": "mainframe-env.cobol-grammar-comparison@1",
        "coverage_credit": 0,
        "source": projection["source"],
        "totals": totals,
        "rows": rows,
    }
    args.output.write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    for key, value in totals.items():
        print(f"{key:32} {value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
