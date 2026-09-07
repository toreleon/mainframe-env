#!/usr/bin/env python3
"""Fetch the RACF command syntax topics, one per command family.

The RACF Command Language Reference is pinned as a book of topics —
`conformance/0.2/manifests/racf-saf-topics.json` names all 109 of them — and the
60 this tool reads are a subset of that pin at identical digests. The same book
is typeset as a PDF, which this repository no longer reads: in the PDF a command
is a bracket-notation block whose nesting has to be recovered from indentation
and line shape, and the reader that did so reached 25 of the 34 families and
leaked segment members to top level whenever a segment opened inline. The topics
state the same material as a definition list, where nesting is markup.

Selection is by href, never by label, and it is stated here so a second person
reproduces the same 60 topics:

  1. The book is the table-of-contents node whose href is
     `SSLTBW_3.2.0/com.ibm.zos.v3r2.icha400/abstract.htm` and which has a child
     at `cmdsyn.htm`. That href appears twice in the tree — the book root and,
     as `abstract.htm?pos=2`, its own abstract — so the child is what tells them
     apart. Matching the label instead is the mistake the three identically
     labelled z/OSMF rows exist to warn about.
  2. Its `RACF command syntax` child (`cmdsyn.htm`) has exactly 34 children, and
     those are the families. 34 is an immutable denominator, so a count that is
     not 34 stops the tool rather than producing a short manifest.
  3. A family is keyed by the first whitespace-delimited token of its TOC label:
     `PASSWORD or PHRASE (Specify user password or password phrase)` keys as
     `PASSWORD`, which is the keyword the catalog carries.
  4. RACDCERT publishes one syntax block per function, so its 28 children are
     taken too — the 26 whose label begins `RACDCERT `. The other two are
     `Examples of controlling ...` topics, which are not syntax.

`?pos=` is a navigation disambiguator rather than a second document, and is
stripped before anything is fetched or recorded; keeping it is why an earlier
probe counted 110 topics in a 109-topic book.

Topic bodies land outside the repository. IBM publication bytes are never
written into the tree, and the manifest this emits carries zero coverage credit.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Iterable

REPOSITORY = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPOSITORY / "conformance" / "tools"))

import docs_api  # noqa: E402

PRODUCT = "SSLTBW_3.2.0"
BOOK_HREF = "SSLTBW_3.2.0/com.ibm.zos.v3r2.icha400/abstract.htm"
SYNTAX_HREF = "SSLTBW_3.2.0/com.ibm.zos.v3r2.icha400/cmdsyn.htm"
RACDCERT_HREF = "SSLTBW_3.2.0/com.ibm.zos.v3r2.icha400/radcertg.htm"
RACDCERT_PREFIX = "RACDCERT "

#: Frozen. `conformance/0.5/racf/command-language.json` carries 34 families and
#: `immutable_denominators` says so; a book that no longer publishes 34 is a
#: republication to be reviewed, not a number to be re-derived.
FAMILY_COUNT = 34
#: The 26 RACDCERT function subtopics, of 28 children.
RACDCERT_FUNCTION_COUNT = 26

DEFAULT_PIN = Path("conformance/0.2/manifests/racf-saf-topics.json")


class SelectionError(Exception):
    """The topic tree does not have the shape the selection predicate states."""


def strip_position(href: str) -> str:
    return href.split("?", 1)[0]


def book_node(document: dict[str, Any]) -> dict[str, Any]:
    """The book root, identified by its href and by the child it must have."""
    candidates = [
        node
        for node in docs_api.toc_index(document).get(BOOK_HREF, [])
        if any(
            strip_position(child.get("href") or "") == SYNTAX_HREF
            for child in node.get("topics") or []
        )
    ]
    if len(candidates) != 1:
        raise SelectionError(
            f"{BOOK_HREF} resolves to {len(candidates)} nodes carrying "
            f"{SYNTAX_HREF} as a child; expected exactly one"
        )
    return candidates[0]


def syntax_node(book: dict[str, Any]) -> dict[str, Any]:
    for child in book.get("topics") or []:
        if strip_position(child.get("href") or "") == SYNTAX_HREF:
            return child
    raise SelectionError(f"the book has no child at {SYNTAX_HREF}")


def keyword_of(node: dict[str, Any]) -> str:
    label = (node.get("label") or "").strip()
    if not label:
        raise SelectionError(f"a child of {SYNTAX_HREF} has no label")
    return label.split()[0]


def families(document: dict[str, Any]) -> list[dict[str, Any]]:
    """The 34 command families, in the order the book publishes them."""
    children = syntax_node(book_node(document)).get("topics") or []
    if len(children) != FAMILY_COUNT:
        raise SelectionError(
            f"{SYNTAX_HREF} has {len(children)} children; "
            f"{FAMILY_COUNT} is an immutable denominator"
        )
    selected = [
        {
            "keyword": keyword_of(child),
            "label": (child.get("label") or "").strip(),
            "topic_path": strip_position(child.get("href") or ""),
            "topic_id": child.get("topicId"),
            "node": child,
        }
        for child in children
    ]
    keywords = [entry["keyword"] for entry in selected]
    if len(set(keywords)) != FAMILY_COUNT:
        repeated = sorted({key for key in keywords if keywords.count(key) > 1})
        raise SelectionError(f"family keywords are not unique: {repeated}")
    return selected


def racdcert_functions(node: dict[str, Any]) -> list[dict[str, Any]]:
    """The RACDCERT function subtopics, by label prefix rather than by count."""
    selected = [
        {
            "keyword": keyword_of_function(child),
            "label": (child.get("label") or "").strip(),
            "topic_path": strip_position(child.get("href") or ""),
            "topic_id": child.get("topicId"),
        }
        for child in node.get("topics") or []
        if (child.get("label") or "").strip().startswith(RACDCERT_PREFIX)
    ]
    if len(selected) != RACDCERT_FUNCTION_COUNT:
        raise SelectionError(
            f"RACDCERT publishes {len(selected)} function subtopics; "
            f"expected {RACDCERT_FUNCTION_COUNT}"
        )
    return selected


def keyword_of_function(node: dict[str, Any]) -> str:
    """`RACDCERT ADDRING (Add key ring)` names the function `ADDRING`."""
    return (node.get("label") or "").strip().split()[1]


def outside_repository(path: Path) -> Path:
    """Refuse to write publication bytes into the tree."""
    resolved = path.resolve()
    if resolved == REPOSITORY or REPOSITORY in resolved.parents:
        raise ValueError(f"topic destination is inside the repository: {resolved}")
    return resolved


def file_name(topic_path: str) -> str:
    return topic_path.replace("/", "_") + ".html"


def parse_args(argv: Iterable[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--destination", type=Path, required=True,
                        help="where topic bodies are written, outside the tree")
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--cache", type=Path,
                        help="reuse retrieved bodies from this directory")
    parser.add_argument("--toc", type=Path, help="reuse a saved table of contents")
    parser.add_argument("--pin", type=Path, default=DEFAULT_PIN,
                        help="the baseline manifest each topic is checked against")
    parser.add_argument("--workers", type=int, default=6)
    return parser.parse_args(list(argv) if argv is not None else None)


def main(argv: Iterable[str] | None = None) -> int:
    args = parse_args(argv)
    destination = outside_repository(args.destination)
    destination.mkdir(parents=True, exist_ok=True)
    cache = outside_repository(args.cache) if args.cache else None

    toc_url = docs_api.TOC_URL.format(product=PRODUCT)
    if args.toc and args.toc.is_file():
        toc_body = args.toc.read_bytes()
    else:
        toc_body = docs_api.toc_bytes(toc_url, cache)
        if args.toc:
            args.toc.write_bytes(toc_body)
    document = json.loads(toc_body.decode("utf-8"))

    selected = families(document)
    for entry in selected:
        if entry["topic_path"] == RACDCERT_HREF:
            entry["functions"] = racdcert_functions(entry["node"])
        else:
            entry["functions"] = []

    wanted: list[str] = []
    for entry in selected:
        wanted.append(entry["topic_path"])
        wanted.extend(function["topic_path"] for function in entry["functions"])

    retrieved: dict[str, bytes] = {}
    failed: list[str] = []
    for path, result in docs_api.topics(
        wanted, cache=cache, workers=max(1, args.workers)
    ):
        if isinstance(result, Exception):
            failed.append(f"{path}: {result}")
            continue
        retrieved[path] = result
        (destination / file_name(path)).write_bytes(result)
    for failure in failed:
        print(f"unreachable {failure}")

    def described(entry: dict[str, Any]) -> dict[str, Any]:
        body = retrieved.get(entry["topic_path"])
        described_entry = {
            "keyword": entry["keyword"],
            "label": entry["label"],
            "topic_path": entry["topic_path"],
            "topic_id": entry["topic_id"],
            "located": body is not None,
        }
        if body is not None:
            text = body.decode("utf-8", "replace")
            described_entry.update(
                {
                    "bytes": len(body),
                    "sha256": docs_api.digest(body),
                    "last_modified": docs_api.last_modified_of(text),
                    "h1": docs_api.heading_of(text),
                    "file": file_name(entry["topic_path"]),
                }
            )
        return described_entry

    records = []
    for entry in selected:
        record = described(entry)
        record["functions"] = [described(function) for function in entry["functions"]]
        records.append(record)

    flat = [record for record in records]
    flat += [function for record in records for function in record["functions"]]
    present = [record for record in flat if record["located"]]

    pin_report = compare_with_pin(args.pin, present)

    manifest = {
        "schema_version": "mainframe-env.racf-topic-manifest@1",
        "coverage_credit": 0,
        "retained_in_repository": False,
        "product": PRODUCT,
        "book_label": "z/OS Security Server RACF Command Language Reference",
        "book_href": BOOK_HREF,
        "syntax_href": SYNTAX_HREF,
        "toc_url": toc_url,
        "toc_sha256": docs_api.digest(toc_body),
        "content_url_template": docs_api.CONTENT_URL,
        "topic_manifest_digest": docs_api.manifest_digest(present),
        "topic_manifest_digest_definition": docs_api.DIGEST_DEFINITION,
        "family_count": len(records),
        "topic_count": len(present),
        "total_bytes": sum(record["bytes"] for record in present),
        "pinned_baseline": pin_report,
        "families": records,
    }
    args.manifest.parent.mkdir(parents=True, exist_ok=True)
    args.manifest.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    located = sum(1 for record in records if record["located"])
    print(
        f"families={len(records)} located={located} topics={len(present)} "
        f"pinned_match={pin_report['topics_matching']}/{pin_report['topics_compared']}"
    )
    return 0 if located == FAMILY_COUNT and not failed else 1


def compare_with_pin(pin: Path | None, present: list[dict[str, Any]]) -> dict[str, Any]:
    """Check every fetched topic against the digest the baseline already pins.

    The book is pinned once, for the whole baseline. A syntax reader that
    silently read different bytes from the ones `conformance/0.2` says the
    catalogs rest on would be evidence about nothing, so the comparison is part
    of the manifest rather than a thing a reviewer is asked to do by hand.
    """
    if pin is None or not pin.is_file():
        return {"path": str(pin) if pin else None, "compared": False,
                "topics_compared": 0, "topics_matching": 0, "differing": []}
    pinned = {
        entry["topic_path"]: entry["sha256"]
        for entry in json.loads(pin.read_text(encoding="utf-8"))["topics"]
    }
    differing = [
        {"topic_path": record["topic_path"], "pinned": pinned.get(record["topic_path"]),
         "served": record["sha256"]}
        for record in present
        if pinned.get(record["topic_path"]) != record["sha256"]
    ]
    return {
        "path": str(pin),
        "compared": True,
        "topics_compared": len(present),
        "topics_matching": len(present) - len(differing),
        "differing": differing,
    }


if __name__ == "__main__":
    raise SystemExit(main())
