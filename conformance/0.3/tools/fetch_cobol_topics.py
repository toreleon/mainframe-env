#!/usr/bin/env python3
"""Fetch the COBOL Language Reference topics a catalog unit cites.

The reference is published twice: as the PDF the baseline pins, and as web
topics carrying DITA syntax markup. The topics are the better machine source —
`groupseq`, `groupchoice`, `boxed syntaxkwd` and `boxed syntaxvar` state the
diagram structure that the PDF only draws — so this fetches them for the
grammar reader.

Topic bodies come from IBM Documentation's content endpoint rather than the
rendered page, because the rendered page is an application shell. The table of
contents supplies each topic's path, keyed by the same heading the catalog
records in its `pdf-page;outline` locator.

Topics land outside the repository; IBM publication bytes are never written
into the tree. Requires websocket-client and a Chrome on a debugging port.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import urllib.parse
from pathlib import Path
from typing import Any, Iterable

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools"))

from browser_fetch import establish, fetch_binary, open_tab

PRODUCT = "SS6SG3_6.5"
TOC = "https://www.ibm.com/docs/api/v1/toc/{product}?lang=en"
CONTENT = "https://www.ibm.com/docs/api/v1/content/{path}?parsebody=true&lang=en"


def digest(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def locate(toc: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """Map every table-of-contents heading to its node."""
    found: dict[str, dict[str, Any]] = {}

    def walk(node: dict[str, Any]) -> None:
        label = (node.get("label") or "").strip()
        if label and node.get("href"):
            found.setdefault(label, node)
        for child in node.get("topics") or []:
            walk(child)

    walk(toc["toc"])
    return found


def subtree(node: dict[str, Any]) -> list[tuple[str, str]]:
    """A statement and every topic beneath it.

    Statements with several formats publish an overview topic and one child
    topic per format — `ACCEPT statement` carries no diagram at all, while its
    children carry one each. Reading only the named topic would report those
    statements as diagram-free.
    """
    found: list[tuple[str, str]] = []

    def walk(current: dict[str, Any]) -> None:
        label = (current.get("label") or "").strip()
        href = current.get("href")
        if label and href:
            found.append((label, href))
        for child in current.get("topics") or []:
            walk(child)

    walk(node)
    return found


def heading(row: dict[str, Any]) -> str:
    """The catalog locator carries the publication's own heading."""
    locator = row["source_locator"]
    marker = "outline:"
    index = locator.find(marker)
    if index < 0:
        raise ValueError(f"row {row['id']} has no outline locator: {locator}")
    return locator[index + len(marker) :].strip()


def parse_args(argv: Iterable[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=9222)
    parser.add_argument("--catalog", type=Path, required=True)
    parser.add_argument("--unit", default="procedure_statements")
    parser.add_argument("--destination", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    return parser.parse_args(list(argv) if argv is not None else None)


def main(argv: Iterable[str] | None = None) -> int:
    args = parse_args(argv)
    catalog = json.loads(args.catalog.read_text(encoding="utf-8"))
    rows = catalog[args.unit]
    args.destination.mkdir(parents=True, exist_ok=True)

    tab = open_tab(args.port)
    entries: list[dict[str, Any]] = []
    try:
        establish(tab)
        status, body = fetch_binary(tab, TOC.format(product=PRODUCT))
        if body is None:
            print(f"table of contents unavailable: status={status}")
            return 1
        nodes = locate(json.loads(body.decode("utf-8")))
        for row in rows:
            title = heading(row)
            node = nodes.get(title)
            if node is None:
                entries.append({"id": row["id"], "title": title, "located": False,
                                "topics": []})
                print(f"{row['id']:12} {title:44} no topic")
                continue
            collected: list[dict[str, Any]] = []
            for index, (label, path) in enumerate(subtree(node)):
                url = CONTENT.format(path=urllib.parse.quote(path, safe=""))
                status, data = fetch_binary(tab, url)
                if data is None:
                    print(f"{row['id']:12} {label:44} status={status}")
                    continue
                name = f"{row['id']}-{index:02d}.html"
                (args.destination / name).write_bytes(data)
                collected.append(
                    {
                        "label": label,
                        "topic_path": path,
                        "url": url,
                        "file": name,
                        "bytes": len(data),
                        "sha256": digest(data),
                    }
                )
            entries.append(
                {
                    "id": row["id"],
                    "row_id": row["row_id"],
                    "title": title,
                    "located": bool(collected),
                    "topics": collected,
                }
            )
            print(f"{row['id']:12} {title:44} {len(collected):>3} topics")
    finally:
        tab.close()

    args.manifest.write_text(
        json.dumps(
            {
                "schema_version": "mainframe-env.cobol-topic-manifest@1",
                "coverage_credit": 0,
                "product": PRODUCT,
                "unit": args.unit,
                "retained_in_repository": False,
                "topics": entries,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    located = sum(1 for entry in entries if entry["located"])
    print(f"topics={len(entries)} located={located}")
    return 0 if located == len(entries) else 1


if __name__ == "__main__":
    raise SystemExit(main())
