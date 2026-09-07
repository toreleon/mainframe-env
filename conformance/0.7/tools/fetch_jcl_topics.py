#!/usr/bin/env python3
"""Fetch the JCL Reference topics the statement and parameter reader needs.

The topics fetched here are what the baseline pins:
`conformance/0.2/manifests/jcl-jes2-topics.json` names all 1,985 topics of this
book, and the 626 this tool reads are a subset of them at identical digests --
which the run checks rather than assumes, because a reader that quietly drifts
off the pin produces numbers nobody can reproduce.

The book is also typeset as a PDF, which this repository no longer reads. That
is not a style preference. In the outline a parameter and a section of a
parameter are distinguished only by indentation depth, so the reader had to
take depth exactly 1 and could say nothing about what sat below. Here the tree
states the nesting outright: a parameter is a chapter's child, and its `Syntax`
and `Subparameter definition` sections are its own children.

Four kinds of topic are collected, and the manifest records which is which:

  parameter               204, the children of the four parameter chapters
  syntax                  199, their `Syntax` children
  subparameter-definition 188, their `Subparameter definition...` children
  statement                33, the chapters the 20 JCL and 13 JECL statements
                               are documented in
  statement-table           2, the two summary tables that state those rosters

Retrieval goes through `conformance/tools/docs_api.py`, which declares the one
User-Agent the edge accepts, the two endpoint templates and the `parsebody=true`
that belongs to the pin. Topics land outside the repository; IBM publication
bytes are never written into the tree.
"""

from __future__ import annotations

import argparse
import json
import sys
from datetime import date
from pathlib import Path
from typing import Any, Iterable

CONFORMANCE = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(CONFORMANCE / "tools"))
sys.path.insert(0, str(Path(__file__).resolve().parent))

import docs_api

from extract_jcl_html_parameters import (  # noqa: E402
    BOOK,
    BOOK_HREF,
    JECL_CHAPTER,
    PRODUCT,
    STATEMENT_TABLES,
    UNITS,
    book,
    catalog_labels,
    chapters,
    children,
    href_of,
    jecl_statements,
    parameters,
    sections,
    statements,
)

TOC_URL = docs_api.TOC_URL.format(product=PRODUCT)


def wanted(node: dict[str, Any], catalog: dict[str, Any]) -> list[tuple[str, str, str]]:
    """Every topic to fetch, as `(topic_path, role, unit)`, in reading order.

    The order is the book's own: chapter by chapter, parameter by parameter,
    each parameter followed by the sections beneath it. Nothing here is keyed
    by heading text.
    """
    found: list[tuple[str, str, str]] = []
    seen: set[str] = set()

    def add(path: str, role: str, unit: str) -> None:
        if path and path not in seen:
            seen.add(path)
            found.append((path, role, unit))

    by_chapter = chapters(node)
    for unit_id, chapter in UNITS.items():
        current = by_chapter.get(chapter)
        if current is None:
            raise ValueError(f"chapter not found for {unit_id}: {chapter}")
        by_href = {href_of(child): child for child in children(current)}
        for item in parameters(current):
            add(item["topic_path"], "parameter", unit_id)
            named = sections(by_href[item["topic_path"]])
            for path in named["syntax"]:
                add(path, "syntax", unit_id)
            for path in named["subparameter"]:
                add(path, "subparameter-definition", unit_id)

    for item in statements(node, catalog_labels(catalog, "jcl-statements")):
        add(item["topic_path"], "statement", "jcl-statements")
    for item in jecl_statements(node, JECL_CHAPTER):
        add(item["topic_path"], "statement", "jes2-jecl-statements")
    for unit_id, table in STATEMENT_TABLES.items():
        add(table["topic_path"], "statement-table", unit_id)
    return found


def file_name(topic_path: str) -> str:
    """The file a topic body is stored under.

    The book is one DITA component, so every topic path shares a directory and
    the last segment is unique within the book. The run asserts that rather
    than trusting it, because two topics collapsing onto one file would be
    invisible in the counts.
    """
    return topic_path.rsplit("/", 1)[-1]


def parse_args(argv: Iterable[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--catalog", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    parser.add_argument(
        "--reuse",
        type=Path,
        action="append",
        default=[],
        help="a directory of already-fetched topic bodies, read by file name",
    )
    parser.add_argument("--toc", type=Path, help="cache the table of contents here")
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--pinned-manifest", type=Path, help="the 0.2 baseline manifest")
    parser.add_argument("--workers", type=int, default=6)
    parser.add_argument("--book-href", default=BOOK_HREF)
    return parser.parse_args(list(argv) if argv is not None else None)


def main(argv: Iterable[str] | None = None) -> int:
    args = parse_args(argv)
    catalog = json.loads(args.catalog.read_text(encoding="utf-8"))
    args.destination.mkdir(parents=True, exist_ok=True)

    if args.toc and args.toc.is_file():
        toc_bytes = args.toc.read_bytes()
    else:
        toc_bytes = docs_api.fetch(TOC_URL)
        if args.toc:
            args.toc.parent.mkdir(parents=True, exist_ok=True)
            args.toc.write_bytes(toc_bytes)
    node = book(json.loads(toc_bytes.decode("utf-8")), args.book_href)

    targets = wanted(node, catalog)
    names = {file_name(path) for path, _, _ in targets}
    if len(names) != len(targets):
        print(f"topic file names collide: {len(targets)} topics, {len(names)} names")
        return 1

    reused = 0
    outstanding: list[str] = []
    for path, _, _ in targets:
        target = args.destination / file_name(path)
        if target.is_file():
            reused += 1
            continue
        source = next(
            (
                directory / file_name(path)
                for directory in args.reuse
                if (directory / file_name(path)).is_file()
            ),
            None,
        )
        if source is None:
            outstanding.append(path)
            continue
        target.write_bytes(source.read_bytes())
        reused += 1

    failures: list[str] = []
    if outstanding:
        for path, result in docs_api.topics(
            outstanding, cache=None, workers=max(1, args.workers)
        ):
            if isinstance(result, Exception):
                failures.append(f"{path}: {result}")
                continue
            (args.destination / file_name(path)).write_bytes(result)
    print(f"topics={len(targets)} reused={reused} fetched={len(outstanding) - len(failures)}")
    for failure in failures:
        print(f"  unreachable {failure}")

    entries: list[dict[str, Any]] = []
    for path, role, unit in targets:
        target = args.destination / file_name(path)
        if not target.is_file():
            continue
        data = target.read_bytes()
        body = data.decode("utf-8", errors="replace")
        entries.append(
            {
                "topic_path": path,
                "sha256": docs_api.digest(data),
                "bytes": len(data),
                "last_modified": docs_api.last_modified_of(body),
                "heading": docs_api.heading_of(body),
                "role": role,
                "unit": unit,
                "file": file_name(path),
            }
        )

    roles: dict[str, int] = {}
    for entry in entries:
        roles[entry["role"]] = roles.get(entry["role"], 0) + 1

    pinned: dict[str, Any] = {"checked": False}
    if args.pinned_manifest is not None:
        recorded = json.loads(args.pinned_manifest.read_text(encoding="utf-8"))
        by_path = {topic["topic_path"]: topic for topic in recorded["topics"]}
        absent = [entry["topic_path"] for entry in entries if entry["topic_path"] not in by_path]
        differing = [
            entry["topic_path"]
            for entry in entries
            if entry["topic_path"] in by_path
            and by_path[entry["topic_path"]]["sha256"] != entry["sha256"]
        ]
        pinned = {
            "checked": True,
            "manifest_file": args.pinned_manifest.name,
            "baseline_topic_count": recorded["topic_count"],
            "absent_from_pin": absent,
            "differing_from_pin": differing,
            "subset_of_pin": not absent and not differing,
        }
        print(
            f"pin: absent={len(absent)} differing={len(differing)} "
            f"subset={'yes' if pinned['subset_of_pin'] else 'NO'}"
        )

    args.manifest.parent.mkdir(parents=True, exist_ok=True)
    args.manifest.write_text(
        json.dumps(
            {
                "schema_version": "mainframe-env.jcl-topic-manifest@1",
                "coverage_credit": 0,
                "retained_in_repository": False,
                "product": PRODUCT,
                "book": BOOK,
                "book_href": args.book_href,
                "toc_url": TOC_URL,
                "toc_sha256": docs_api.digest(toc_bytes),
                "content_url_template": docs_api.CONTENT_URL,
                "snapshot_date": date.today().isoformat(),
                "topic_count": len(entries),
                "total_bytes": sum(entry["bytes"] for entry in entries),
                "topic_manifest_digest": docs_api.manifest_digest(entries),
                "topic_manifest_digest_definition": docs_api.DIGEST_DEFINITION,
                "role_counts": roles,
                "pinned_baseline": pinned,
                "topics": entries,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    print(" ".join(f"{role}={count}" for role, count in sorted(roles.items())))
    return 0 if not failures and len(entries) == len(targets) else 1


if __name__ == "__main__":
    raise SystemExit(main())
