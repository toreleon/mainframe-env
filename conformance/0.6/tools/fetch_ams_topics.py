#!/usr/bin/env python3
"""Fetch the Access Method Services parameter topics for each command.

The topics fetched here are what the baseline pins:
`conformance/0.2/manifests/dataset-vsam-ams-topics.json` names all 516 topics of
this book, and the 89 this tool reads are a subset of them at identical digests.
The reference is also typeset as a PDF, which this repository no longer reads.
That is not a style preference. In the PDF a parameter is a flush-left heading,
which is a typographic accident: values the typesetter did not indent read as
parameters too. The topics put the same material in definition lists, where a
parameter is a `dt` and the values it accepts are `dt` entries of a nested `dl`.
Nesting states what indentation only implied.

Each command owns a chapter whose children include a `<CMD> Parameters` section
splitting into `Required Parameters` and `Optional Parameters`. Only those
topics are fetched; the examples are not syntax.

Topics land outside the repository; IBM publication bytes are never written
into the tree. Requires websocket-client and a Chrome on a debugging port.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import urllib.parse
from pathlib import Path
from typing import Any, Iterable

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools"))

from browser_fetch import establish, fetch_binary, open_tab

PRODUCT = "SSLTBW_3.2.0"
BOOK = "z/OS DFSMS Access Method Services Commands"
TOC = "https://www.ibm.com/docs/api/v1/toc/{product}?lang=en"
CONTENT = "https://www.ibm.com/docs/api/v1/content/{path}?parsebody=true&lang=en"
PARAMETERS = re.compile(r"\bParameters?\s*$", re.IGNORECASE)


def digest(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def find_book(node: dict[str, Any]) -> dict[str, Any] | None:
    if (node.get("label") or "").strip() == BOOK:
        return node
    for child in node.get("topics") or []:
        found = find_book(child)
        if found is not None:
            return found
    return None


def parameter_topics(node: dict[str, Any]) -> list[tuple[str, str]]:
    """Every `... Parameters` topic beneath a command chapter."""
    found: list[tuple[str, str]] = []

    def walk(current: dict[str, Any]) -> None:
        label = (current.get("label") or "").strip()
        href = current.get("href")
        if label and href and PARAMETERS.search(label):
            found.append((label, href))
        for child in current.get("topics") or []:
            walk(child)

    walk(node)
    return found


def parse_args(argv: Iterable[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=9222)
    parser.add_argument("--catalog", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--toc", type=Path, help="reuse a saved table of contents")
    return parser.parse_args(list(argv) if argv is not None else None)


def main(argv: Iterable[str] | None = None) -> int:
    args = parse_args(argv)
    catalog = json.loads(args.catalog.read_text(encoding="utf-8"))
    args.destination.mkdir(parents=True, exist_ok=True)

    tab = open_tab(args.port)
    entries: list[dict[str, Any]] = []
    try:
        establish(tab)
        if args.toc and args.toc.is_file():
            toc = json.loads(args.toc.read_text(encoding="utf-8"))
        else:
            status, body = fetch_binary(tab, TOC.format(product=PRODUCT))
            if body is None:
                print(f"table of contents unavailable: status={status}")
                return 1
            toc = json.loads(body.decode("utf-8"))
            if args.toc:
                args.toc.write_bytes(body)
        book = find_book(toc["toc"])
        if book is None:
            print(f"book not found in table of contents: {BOOK}")
            return 1
        chapters = {
            (child.get("label") or "").strip(): child
            for child in book.get("topics") or []
        }

        for command in catalog["commands"]:
            chapter = chapters.get(command["label"])
            if chapter is None:
                entries.append({"id": command["id"], "label": command["label"],
                                "located": False, "topics": []})
                print(f"{command['label']:26} no chapter")
                continue
            collected: list[dict[str, Any]] = []
            for index, (label, path) in enumerate(parameter_topics(chapter)):
                url = CONTENT.format(path=urllib.parse.quote(path, safe=""))
                status, data = fetch_binary(tab, url)
                if data is None:
                    print(f"{command['label']:26} {label:24} status={status}")
                    continue
                name = f"{command['id']}-{index:02d}.html"
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
                    "id": command["id"],
                    "label": command["label"],
                    "located": bool(collected),
                    "topics": collected,
                }
            )
            print(f"{command['label']:26} {len(collected):>2} parameter topics")
    finally:
        tab.close()

    args.manifest.write_text(
        json.dumps(
            {
                "schema_version": "mainframe-env.ams-topic-manifest@1",
                "coverage_credit": 0,
                "product": PRODUCT,
                "book": BOOK,
                "retained_in_repository": False,
                "commands": entries,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    located = sum(1 for entry in entries if entry["located"])
    print(f"commands={len(entries)} located={located}")
    return 0 if located == len(entries) else 1


if __name__ == "__main__":
    raise SystemExit(main())
