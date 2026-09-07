#!/usr/bin/env python3
"""Read the reference's own reserved-word appendix into a word list.

A COBOL word that appears in a syntax diagram is not automatically a word a
program may not use as a data-name. `Reserved words` (`lr/ref/rlres.html`)
publishes the distinction as a four-column table, and `Context-sensitive words`
(`lr/ref/rlcont.html`) names the words that are keywords only inside the
statement that defines them. Both are topics of the pinned COBOL baseline, so
the answer is read rather than assumed.

The reserved table is one `tbody` of 529 rows over four columns -- the word and
one `X` under exactly one of `Reserved`, `Standard only` and `Potential
reserved words`. The partition is checked, not hoped for: every row this tool
keeps carries exactly one mark, and 402 + 52 + 61 is the 515 word-shaped rows.
The remaining 14 rows are the arithmetic, relational and delimiter symbols,
whose word cell is a symbol followed by prose (`** Arithmetic operator -
exponentiation`); they are collected separately and never enter a word list.

Which of the three columns forbids a user-defined name is stated by the topic
itself. `Reserved` and `Standard only` are both flagged with an S-level
message, so a conforming program cannot spell a data-name with either.
`Potential reserved words` draws an I-level message -- IBM recommends against
them, the compiler accepts them -- so a word that only appears there is still a
legal data-name today. END-ACCEPT sits in that column, which is the reason the
distinction is drawn here rather than collapsed into one list.

Nothing publication-shaped is written: the output is a word list with the two
topics and their pinned digests beside it, `coverage_credit` 0, and no topic
body is retained. `--cache` is the one argument that carries retrieved bytes --
`docs_api.topic` writes each fetched body into it -- so it is a
`docs_api.retrieval_path` and cannot name the tree. `--output` is written inside
the tree on purpose: it is the word list this tool composes, and the topics
behind it appear in it only as a path and a digest.
"""

from __future__ import annotations

import argparse
import hashlib
import html
import json
import re
import sys
from pathlib import Path
from typing import Any, Iterable

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools"))

import docs_api

REPOSITORY = Path(__file__).resolve().parents[3]
RESERVED_TOPIC = "SS6SG3_6.5/lr/ref/rlres.html"
CONTEXT_TOPIC = "SS6SG3_6.5/lr/ref/rlcont.html"

TBODY = re.compile(r"<tbody\b[^>]*>(.*?)</tbody>", re.DOTALL)
ROW = re.compile(r"<tr\b[^>]*>(.*?)</tr>", re.DOTALL)
CELL = re.compile(r"<t[dh]\b[^>]*>(.*?)</t[dh]>", re.DOTALL)
TAG = re.compile(r"<[^>]+>")
# A COBOL word is letters, digits and hyphens, and the appendix prints every
# one of them in upper case. Anything else in the word column is a symbol row.
WORD = re.compile(r"[A-Z0-9][A-Z0-9-]*")

COLUMNS = ("reserved", "standard_only", "potential")


def text_of(fragment: str) -> str:
    """The visible text of a table cell, whitespace collapsed."""
    return html.unescape(re.sub(r"\s+", " ", TAG.sub(" ", fragment))).strip()


def rows_of(body: str) -> list[list[str]]:
    """Every `tbody` row of the topic's single table, as cell text."""
    tbody = TBODY.search(body)
    if tbody is None:
        raise ValueError("topic carries no table body")
    return [[text_of(cell) for cell in CELL.findall(row)] for row in ROW.findall(tbody.group(1))]


def reserved_words(body: str) -> dict[str, Any]:
    """The three word lists and the symbol rows, from the reserved appendix."""
    lists: dict[str, list[str]] = {column: [] for column in COLUMNS}
    symbols: list[str] = []
    seen: set[str] = set()
    for cells in rows_of(body):
        if len(cells) != 4:
            raise ValueError(f"reserved row is not four cells: {cells}")
        word, marks = cells[0], cells[1:]
        if WORD.fullmatch(word) is None:
            # The word column of a symbol row is the symbol followed by the
            # prose that names it. Only the symbol is kept: the prose is
            # publication text and this file is a word list.
            symbols.append(word.split(" ", 1)[0])
            continue
        if word in seen:
            raise ValueError(f"reserved table repeats {word}")
        seen.add(word)
        marked = [column for column, mark in zip(COLUMNS, marks) if mark == "X"]
        if len(marked) != 1:
            raise ValueError(f"{word} is marked in {len(marked)} columns, not one")
        lists[marked[0]].append(word)
    return {**{column: sorted(lists[column]) for column in COLUMNS}, "symbol_rows": symbols}


def context_sensitive_words(body: str) -> list[str]:
    """The words the reference reserves only inside a named construct.

    The table is two columns, word and construct, and a word that governs more
    than one statement is printed once against several construct lines, so the
    word column is deduplicated rather than counted.
    """
    words: list[str] = []
    for cells in rows_of(body):
        if not cells:
            continue
        word = cells[0]
        if WORD.fullmatch(word) is not None and word not in words:
            words.append(word)
    return sorted(words)


def pinned(manifest: Path, topic_path: str) -> dict[str, Any]:
    """The manifest entry pinning one topic, by path."""
    document = json.loads(manifest.read_text(encoding="utf-8"))
    for topic in document["topics"]:
        if topic["topic_path"] == topic_path:
            return topic
    raise ValueError(f"{topic_path} is not pinned by {manifest}")


def read(topic_path: str, manifest: Path, cache: Path | None) -> bytes:
    """One pinned topic body, refused unless it hashes to what is pinned."""
    body = docs_api.topic(topic_path, cache=cache)
    served = hashlib.sha256(body).hexdigest()
    expected = pinned(manifest, topic_path)["sha256"]
    if served != expected:
        raise ValueError(f"{topic_path} served {served}, manifest pins {expected}")
    return body


def parse_args(argv: Iterable[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--baseline", default="ibm-enterprise-cobol-6.5-2026-05-31")
    parser.add_argument("--cache", type=docs_api.retrieval_path, default=None,
                        help="reuse retrieved bodies from this directory, outside the tree")
    return parser.parse_args(list(argv) if argv is not None else None)


def main(argv: Iterable[str] | None = None) -> int:
    args = parse_args(argv)
    reserved_body = read(RESERVED_TOPIC, args.manifest, args.cache).decode("utf-8")
    context_body = read(CONTEXT_TOPIC, args.manifest, args.cache).decode("utf-8")
    words = reserved_words(reserved_body)
    context = context_sensitive_words(context_body)
    document = {
        "schema_version": "mainframe-env.cobol-reserved-words@1",
        "baseline_id": args.baseline,
        "coverage_credit": 0,
        "source": {
            "manifest": str(args.manifest.resolve().relative_to(REPOSITORY)),
            "retained_in_repository": False,
            "topics": [
                {
                    "topic_path": RESERVED_TOPIC,
                    "sha256": pinned(args.manifest, RESERVED_TOPIC)["sha256"],
                    "heading": "Reserved words",
                },
                {
                    "topic_path": CONTEXT_TOPIC,
                    "sha256": pinned(args.manifest, CONTEXT_TOPIC)["sha256"],
                    "heading": "Context-sensitive words",
                },
            ],
        },
        "reserved": words["reserved"],
        "standard_only": words["standard_only"],
        "potential": words["potential"],
        "context_sensitive": context,
        "symbol_rows": words["symbol_rows"],
    }
    args.output.write_text(
        json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(
        f"reserved={len(words['reserved'])} standard_only={len(words['standard_only'])} "
        f"potential={len(words['potential'])} symbols={len(words['symbol_rows'])} "
        f"context_sensitive={len(context)}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
