#!/usr/bin/env python3
"""Check every `topic:PATH;topic-id:ID;heading:TITLE` row locator against IBM.

The 0.2 catalogs record where each row came from. A locator now names a
documentation topic outright, which makes the row inventory directly checkable
against the publication: the topic either serves that heading under that path or
it does not.

This audits row *identity*, not row *content* — a weaker claim than a syntax
projection, but a uniform one. It applies to every topic-located baseline,
including the two with no syntax reader of their own (Db2, z/OSMF), and it is
the check that catches an inventory drifting off its publication.

Three things are compared, and all three have to agree for a row to read
`exact`:

  topic:     the path resolves, compared modulo the leading product key so a
             product bump reads as `moved` rather than as a wall of `missing`
  topic-id:  the slug the table of contents publishes for that node
  heading:   the reviewed label, against the served `topictitle1` heading or the
             node's own table-of-contents label — Db2 labels a statement
             `ALLOCATE CURSOR` where its heading reads `ALLOCATE CURSOR
             statement`, so requiring the heading alone would fail 158 rows that
             are correct

A locator may carry a fourth `table:` component. The 20 JCL statement rows cite
one row of a table inside a shared topic rather than a topic of their own, so
for those the heading is looked for in that table's cells.

Rows located some other way (`html-table:`, `html-link:`,
`roadmap-normalization:`) are reported as skipped with a reason rather than
silently ignored, and so is a row whose topic could not be retrieved. An
unreachable endpoint is NEVER reported as missing: not knowing is not a finding.

The emitted report is a review input. It grants no coverage credit, it is
written outside the repository, and IBM publication bytes are never retained.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tempfile
from pathlib import Path
from typing import Any, Iterable

sys.path.insert(0, str(Path(__file__).resolve().parent))

import docs_api

REPOSITORY = Path(__file__).resolve().parents[2]
COMPONENT = re.compile(r"^[a-z][a-z0-9-]*:")
DEFAULT_CACHE = Path(tempfile.gettempdir()) / "cobolgrammar" / "topic-cache"


def components(locator: str) -> dict[str, str]:
    """The `name:value` components of a locator, in order.

    Split conservatively: a fragment that does not itself open a `name:` is
    rejoined to the component before it, so a heading containing a semicolon
    stays whole instead of silently truncating.
    """
    found: dict[str, str] = {}
    order: list[str] = []
    for fragment in locator.split(";"):
        name, _, value = fragment.partition(":")
        if COMPONENT.match(fragment) and name not in found:
            found[name] = value
            order.append(name)
        elif order:
            found[order[-1]] += ";" + fragment
    return {name: value.strip() for name, value in found.items()}


def label_index(nodes: dict[str, list[dict[str, Any]]]) -> dict[str, list[str]]:
    """Normalized table-of-contents label -> the topic paths carrying it."""
    found: dict[str, list[str]] = {}
    for path, filed in nodes.items():
        for node in filed:
            label = docs_api.normalize(node.get("label") or "")
            if label and path not in found.setdefault(label, []):
                found[label].append(path)
    return found


def tail_index(nodes: dict[str, list[dict[str, Any]]]) -> dict[str, list[str]]:
    """Product-stripped topic path -> the full paths carrying it."""
    found: dict[str, list[str]] = {}
    for path in nodes:
        found.setdefault(docs_api.without_product(path), []).append(path)
    return found


def heading_verdict(
    heading: str,
    served: str | None,
    labels: list[str],
    table: str | None,
    body: str,
) -> tuple[bool, str, dict[str, Any]]:
    """Whether the reviewed heading is the one the publication serves.

    Four ways to agree, reported separately so a reviewer can see which claim
    each row rests on:

      h1                       the served heading is the reviewed label
      toc-label                the node's own label is, where the heading adds
                               or drops a trailing word the tree does not carry
      *-without-chapter-number the same, after dropping a printed-book chapter
                               number the topic tree has no equivalent for
      table-cell               the row cites one row of a table in a shared
                               topic, so the label lives in a cell
    """
    wanted = docs_api.normalize(heading)
    if not wanted:
        return False, "no-heading", {"served_heading": served}
    bare = docs_api.without_chapter_number(heading)
    candidates = [("h1", served)] + [("toc-label", label) for label in labels if label]
    for how, candidate in candidates:
        if candidate is not None and docs_api.normalize(candidate) == wanted:
            return True, how, {}
    if bare != wanted:
        for how, candidate in candidates:
            if candidate is not None and docs_api.normalize(candidate) == bare:
                return True, f"{how}-without-chapter-number", {}
    if table:
        cells = docs_api.table_cells(body, table)
        if cells is None:
            return False, "table-absent", {"table": table, "served_heading": served}
        folded = wanted.casefold()
        for cell in cells:
            if cell.casefold() == folded or folded in cell.casefold().split():
                return True, "table-cell", {"table": table}
        return False, "table-cell", {"table": table, "served_heading": served}
    return False, "none", {"served_heading": served}


def check(
    row: dict[str, Any],
    nodes: dict[str, list[dict[str, Any]]],
    labels: dict[str, list[str]],
    tails: dict[str, list[str]],
    body: bytes | Exception | None,
) -> dict[str, Any]:
    parts = components(row["source_locator"])
    result: dict[str, Any] = {"id": row["id"]}
    if "topic" not in parts:
        return {**result, "verdict": "skipped", "reason": "not-a-topic-locator",
                "locator": row["source_locator"]}

    path, heading = parts["topic"], parts.get("heading", "")
    result["topic_path"] = path
    filed = nodes.get(path, [])
    if not filed:
        elsewhere = [
            other for other in tails.get(docs_api.without_product(path), []) if other != path
        ]
        if len(elsewhere) == 1:
            return {**result, "verdict": "moved", "reason": "product-key",
                    "found_topic_path": elsewhere[0], "heading": heading}

    if isinstance(body, docs_api.NotFound):
        found = [other for other in labels.get(docs_api.normalize(heading), []) if other != path]
        if len(found) == 1:
            return {**result, "verdict": "moved", "reason": "heading-found-elsewhere",
                    "found_topic_path": found[0], "heading": heading}
        return {**result, "verdict": "missing", "heading": heading,
                "candidates": sorted(found)}
    if body is None or isinstance(body, Exception):
        return {**result, "verdict": "skipped", "reason": "endpoint-unreachable",
                "detail": getattr(body, "reason", "not-retrieved")}

    text = body.decode("utf-8", "replace")
    served = docs_api.heading_of(text)
    matched, how, detail = heading_verdict(
        heading, served, [node.get("label") or "" for node in filed], parts.get("table"), text
    )
    identifier = parts.get("topic-id")
    published = [node.get("topicId") for node in filed]
    identifier_matches = not filed or identifier is None or identifier in published

    if matched and identifier_matches and filed:
        return {**result, "verdict": "exact", "matched_on": how}
    entry = {**result, "verdict": "retitled", "heading": heading, "matched_on": how, **detail}
    if not identifier_matches:
        entry["topic_id"] = identifier
        entry["published_topic_id"] = published
    if not filed:
        entry["reason"] = "absent-from-table-of-contents"
    entry["last_modified"] = docs_api.last_modified_of(text)
    return entry


def outside_repository(path: Path) -> Path:
    """Refuse to write a report into the tree.

    Unbound JSON under `conformance/0.2` is what broke this branch once already:
    the 0.2 gate validates every artifact it finds against a declared schema, so
    an audit report dropped there fails the build. Reports are review inputs and
    belong outside the repository entirely.
    """
    resolved = path.resolve()
    if resolved == REPOSITORY or REPOSITORY in resolved.parents:
        raise ValueError(f"report path is inside the repository: {resolved}")
    return resolved


def parse_args(argv: Iterable[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--catalog", type=Path, required=True)
    parser.add_argument(
        "--index", type=Path, default=Path("conformance/0.2/catalogs/index.json")
    )
    parser.add_argument("--report", type=Path)
    parser.add_argument(
        "--cache",
        type=Path,
        default=DEFAULT_CACHE,
        help="reuse retrieved topic bodies from this directory, outside the tree",
    )
    parser.add_argument("--no-cache", action="store_true")
    parser.add_argument("--workers", type=int, default=8)
    return parser.parse_args(list(argv) if argv is not None else None)


def main(argv: Iterable[str] | None = None) -> int:
    args = parse_args(argv)
    catalog = json.loads(args.catalog.read_text(encoding="utf-8"))
    index = json.loads(args.index.read_text(encoding="utf-8"))
    baseline = next(
        row for row in index["baselines"] if row["subsystem"] == catalog["subsystem"]
    )
    source = baseline["source"]
    report_path = outside_repository(
        args.report
        or DEFAULT_CACHE.parent / f"topic-locator-audit-{catalog['subsystem']}.json"
    )
    cache = None if args.no_cache else outside_repository(args.cache)

    toc_state: dict[str, Any] = {"url": source["url"], "pinned_sha256": source["toc_sha256"]}
    try:
        raw = docs_api.toc_bytes(source["url"], cache)
        nodes = docs_api.toc_index(json.loads(raw.decode("utf-8")))
        toc_state["sha256"] = "sha256:" + docs_api.digest(raw)
        toc_state["matches_pin"] = toc_state["sha256"] == source["toc_sha256"]
    except docs_api.Unreachable as error:
        print(f"table of contents unreachable: {error.reason}")
        return 2

    labels, tails = label_index(nodes), tail_index(nodes)
    template = source["content_url_template"]

    locators = {
        row["id"]: components(row["source_locator"])
        for unit in catalog["units"]
        for row in unit["rows"]
    }
    wanted = sorted({parts["topic"] for parts in locators.values() if "topic" in parts})
    bodies = dict(docs_api.topics(wanted, template, cache, args.workers))

    units: list[dict[str, Any]] = []
    totals: dict[str, int] = {}
    matched_on: dict[str, int] = {}
    for unit in catalog["units"]:
        results = [
            check(row, nodes, labels, tails, bodies.get(locators[row["id"]].get("topic", "")))
            for row in unit["rows"]
        ]
        counts: dict[str, int] = {}
        for result in results:
            key = result["verdict"]
            if key == "skipped":
                key = f"skipped:{result['reason']}"
            counts[key] = counts.get(key, 0) + 1
            totals[key] = totals.get(key, 0) + 1
            if result["verdict"] == "exact":
                matched_on[result["matched_on"]] = matched_on.get(result["matched_on"], 0) + 1
        units.append(
            {
                "unit": unit["id"],
                "rows": len(results),
                "counts": counts,
                "unresolved": [
                    r for r in results if r["verdict"] in ("missing", "moved", "retitled")
                ],
            }
        )
        print(
            f"{unit['id']:34} rows={len(results):4} "
            f"exact={counts.get('exact', 0):4} retitled={counts.get('retitled', 0):3} "
            f"moved={counts.get('moved', 0):3} missing={counts.get('missing', 0):3} "
            f"skipped={sum(v for k, v in counts.items() if k.startswith('skipped')):3}"
        )

    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(
        json.dumps(
            {
                "schema_version": "mainframe-env.topic-locator-audit@1",
                "coverage_credit": 0,
                "retained_in_repository": False,
                "baseline_id": catalog["baseline_id"],
                "subsystem": catalog["subsystem"],
                "content_url_template": template,
                "table_of_contents": toc_state,
                "totals": totals,
                "exact_matched_on": matched_on,
                "units": units,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    unreachable = totals.get("skipped:endpoint-unreachable", 0)
    unresolved = sum(totals.get(key, 0) for key in ("missing", "moved", "retitled"))
    print(
        f"{catalog['subsystem']} toc_matches_pin={toc_state.get('matches_pin')} "
        f"unresolved={unresolved} unreachable={unreachable} report={report_path}"
    )
    return 1 if unresolved else 0


if __name__ == "__main__":
    raise SystemExit(main())
