#!/usr/bin/env python3
"""Check every publication-backed catalog row locator against IBM.

The 0.2 catalogs record where each row came from. A locator now names a
documentation topic outright, which makes the row inventory directly checkable
against the publication: the topic either serves that heading under that path or
it does not.

This audits row *identity*, not row *content* — a weaker claim than a syntax
projection, but a uniform one. It applies to topic, table and link locators,
including the two baselines with no syntax reader of their own (Db2, z/OSMF),
and it is the check that catches an inventory drifting off its publication.

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

A locator may carry a fourth `table:` component and a fifth `row:`. The 20 JCL
statement rows cite one row of a table inside a shared topic rather than a topic
of their own, and for those the heading is checked against THAT row's cells and
no others. Scanning the whole table would accept any of the 20 labels for any of
the 20 rows, which is to say it would check that the table still lists the
statement and call it a check that the row cites the right one. When a table is
cited the table decides: the served heading and the tree label are the topic's,
and the topic is shared by all 20.

Citing a row only discriminates if the comparison does. See `cell_names_heading`
for the rule and `table_verdict` for what happens when it cannot tell two rows
apart.

The three older embedded locator forms are resolved according to the identity
their source actually publishes:

  html-table:  CICS matches the command, EIBFN and family cells; IMS matches the
               cited body-row ordinal plus its call and command cells; RACROUTE
               matches one request-type header cell in its cross-reference
               matrix.  Exact means that complete identity resolves once.
  html-link:   MQ matches the reviewed call name and target filename as one
               pair.  Byte-identical duplicate anchors collapse to one semantic
               link, which preserves the catalog's documented deduplication.
  roadmap-normalization:
               the five VSAM organization rows are a deliberate normalized
               taxonomy rather than claims that IBM publishes five rows in one
               topic.  They remain visible as a documented ``skipped`` result.

A source body that cannot be retrieved is also reported as skipped. An
unreachable endpoint is NEVER reported as missing: not knowing is not a finding.

The emitted report is a review input. It grants no coverage credit, it is
written outside the repository, and IBM publication bytes are never retained.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.parse
from pathlib import Path, PurePosixPath
from typing import Any, Iterable

sys.path.insert(0, str(Path(__file__).resolve().parent))

import docs_api

REPOSITORY = Path(__file__).resolve().parents[2]
COMPONENT = re.compile(r"^[a-z][a-z0-9-]*:")
DEFAULT_CACHE = docs_api.default_cache()

#: The three prefixes the JCL Statement column prints a statement behind, and
#: the whole vocabulary of that column: `// DD`, `//* comment`, `/*`, `//`. The
#: prefix must stand as its own token — it either ends the cell or a space
#: follows — so a prose cell that merely opens with a slash is not mistaken for
#: a coded one.
STATEMENT_MARKER = re.compile(r"^(?://\*|//|/\*)(?=\s|$)")

# Two catalogs predate literal table ids in source locators.  These aliases are
# resolved to the DITA ids in the pinned bodies and then checked like every
# literal table citation.  Keeping the mapping here makes publication drift a
# visible ``html-table-absent`` finding instead of silently choosing whichever
# table happens to be first.
TABLE_ALIASES = {
    ("ims", "comparison"): "ims_comparingexecdlicmdsanddlicalls__p5tcmp1",
    ("racf-saf", "keyword-and-parameter-cross-reference"): "rrkpcr__pkcx",
}

FOOTNOTE_SUFFIX = re.compile(r"\s+\d+$")

ROADMAP_NORMALIZATION = "vsam-primary-organizations"
ROADMAP_NORMALIZATION_REASON = (
    "The five VSAM organization rows are the deliberate normalized taxonomy "
    "frozen by conformance/roadmap/ibm-official-coverage-roadmap.json; they do "
    "not claim that one IBM topic publishes a five-row inventory."
)


def locator_kind(locator: str) -> str:
    """The leading component, which decides how the row is resolved."""
    return locator.partition(":")[0]


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


def cell_names_heading(heading: str, cell: str) -> bool:
    """Whether ONE table cell names this reviewed label.

    The reviewed labels are normalizations, not transcriptions, so raw equality
    would reject correct rows: the Statement column prints `// DD` for `DD` and
    `//* comment` for `comment`, and the Name column prints `output JCL` where
    the label reads `OUTPUT JCL`. But token containment — is the label one of the
    cell's words — is far too loose, because the third column is a paragraph of
    prose. Against the served table it accepts 5 of the 20 labels at more than
    one ordinal, `JOB` at seven of them, and a discriminator that admits seven
    answers discriminates nothing.

    So the rule is whole-cell equality under one stated relaxation each, chosen
    by what the cell IS rather than by which column it sits in:

      a coded cell — one opening with `//`, `//*` or `/*`, which is how the book
      spells a JCL statement — matches when the label is EXACTLY the rest of the
      cell, letter case included

      any other cell matches when the label is EXACTLY the whole cell, ignoring
      letter case

    Whole-cell equality is what excludes the Purpose column: no label is equal to
    a sentence. Nothing needs to know that Purpose is column three.

    Case is the load-bearing asymmetry, and it is the publication's own. The book
    distinguishes two statements by case and nothing else — row 1 prints
    `// command` and is named `JCL command`, row 2 prints `// COMMAND` and is
    named `command` — so a case-insensitive read of the coded column puts the
    label `COMMAND` at both ordinals, which is precisely the swap the ordinal
    exists to catch. Case in the Name column carries no such freight: `output
    JCL`, `job` and `set` are sentence-style prose capitalization, and folding it
    creates no collision the coded column does not already resolve.

    An empty label names nothing, and neither does a bare marker: `//` and `/*`
    are whole cells with nothing after the prefix, and both those rows are named
    by their Name cell instead.

    Measured against the served table, all 20 labels resolve to exactly one
    ordinal each, and it is their own. `docs_api.heading_in_cells` is the
    containment rule this replaces; nothing calls it now, and the tests keep hold
    of it only to pin the shape of the defect.
    """
    heading = docs_api.normalize(heading)
    if not heading:
        return False
    cell = docs_api.normalize(cell)
    marker = STATEMENT_MARKER.match(cell)
    if marker:
        return cell[marker.end() :].strip() == heading
    return cell.casefold() == heading.casefold()


def rows_naming_heading(heading: str, rows: list[list[str]]) -> list[int]:
    """The 1-based ordinals of every row of the table that names this label."""
    return [
        ordinal
        for ordinal, cells in enumerate(rows, 1)
        if any(cell_names_heading(heading, cell) for cell in cells)
    ]


def table_verdict(
    heading: str, table: str, ordinal: str | None, body: str
) -> tuple[bool, str, dict[str, Any]]:
    """Whether the CITED row of the cited table names the reviewed heading.

    The ordinal is what makes this a check of the row rather than of the table.
    Without it the 20 JCL statement rows are all interchangeable: each names a
    label the table carries somewhere, so any permutation of them passes. So a
    `table:` with no `row:` is not resolved at all — it reads `table-row-uncited`
    and fails, because a citation this tool cannot check should not read as one
    it checked.

    A label that names two rows is the same failure wearing different clothes,
    and gets the same answer. `table-row-ambiguous` fails even when one of those
    rows is the cited one, because a label satisfied at two ordinals does not
    establish which one the catalog meant, and reporting it `exact` would credit
    the citation with a discrimination nothing performed. That makes the property
    `cell_names_heading` is built for — one label, one ordinal — checked on every
    run against whatever the publication currently serves, rather than a
    measurement taken once and assumed to hold.

    A row ordinal past the end of the table is `table-row-absent`, which is drift
    in the publication rather than in the locator, and is reported separately for
    that reason. A row that resolves but names something else carries the cells
    it does name, and, when the label turns up in some other row, that row's
    ordinal — the shape a swapped pair makes.
    """
    detail: dict[str, Any] = {"table": table}
    if ordinal is None:
        return False, "table-row-uncited", detail
    if not ordinal.isdigit() or int(ordinal) < 1:
        return False, "table-row-malformed", {**detail, "row": ordinal}
    wanted = int(ordinal)
    detail["row"] = wanted
    rows = docs_api.table_rows(body, table)
    if rows is None:
        return False, "table-absent", detail
    if wanted > len(rows):
        return False, "table-row-absent", {**detail, "table_body_rows": len(rows)}
    naming = rows_naming_heading(heading, rows)
    if len(naming) > 1:
        return False, "table-row-ambiguous", {**detail, "heading_found_in_rows": naming}
    if naming == [wanted]:
        return True, "table-row", detail
    detail["row_cells"] = rows[wanted - 1]
    if naming:
        detail["heading_found_in_rows"] = naming
    return False, "table-row", detail


def heading_verdict(
    heading: str,
    served: str | None,
    labels: list[str],
    table: str | None,
    ordinal: str | None,
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
      table-row                the row cites one row of a table in a shared
                               topic, so the label lives in a cell of that row

    A cited table is decided by the table alone and never falls back to the
    heading or the label, both of which belong to the shared topic and would
    hand every row of the table the same answer.
    """
    wanted = docs_api.normalize(heading)
    if not wanted:
        return False, "no-heading", {"served_heading": served}
    if table:
        return table_verdict(wanted, table, ordinal, body)
    bare = docs_api.without_chapter_number(heading)
    candidates = [("h1", served)] + [("toc-label", label) for label in labels if label]
    for how, candidate in candidates:
        if candidate is not None and docs_api.normalize(candidate) == wanted:
            return True, how, {}
    if bare != wanted:
        for how, candidate in candidates:
            if candidate is not None and docs_api.normalize(candidate) == bare:
                return True, f"{how}-without-chapter-number", {}
    return False, "none", {"served_heading": served}


def catalog_ordinal(row: dict[str, Any]) -> int | None:
    """The frozen 1-based ordinal carried by an official row id."""
    suffix = str(row.get("id", "")).rsplit(":", 1)[-1]
    return int(suffix) if suffix.isdigit() and int(suffix) > 0 else None


def locator_body_path(
    row: dict[str, Any], catalog: dict[str, Any], baseline: dict[str, Any]
) -> str | None:
    """The pinned topic body a row locator has to be checked inside.

    Topic locators name their body outright.  CICS, IMS and MQ are inventories
    extracted from the baseline's ``book_href`` topic.  RACROUTE is explicitly
    a supporting source in the receipt; requiring exactly one such topic avoids
    silently selecting an arbitrary RACF command-language topic.
    """
    parts = components(row["source_locator"])
    kind = locator_kind(row["source_locator"])
    if kind == "topic":
        return parts.get("topic")
    if kind == "roadmap-normalization":
        return None

    subsystem = catalog["subsystem"]
    if (subsystem, kind) in {
        ("cics", "html-table"),
        ("ims", "html-table"),
        ("mq", "html-link"),
    }:
        return baseline["source"]["book_href"]
    if (subsystem, kind) == ("racf-saf", "html-table"):
        paths = [
            source.get("topic_path")
            for source in baseline.get("supporting_sources", [])
            if source.get("topic_path")
        ]
        if len(paths) != 1:
            raise ValueError(
                f"{baseline['id']} must name exactly one supporting topic for its html-table rows"
            )
        return paths[0]
    raise ValueError(f"no body-source convention for {subsystem} {kind} locator")


def unavailable_result(
    row: dict[str, Any], source_path: str, body: bytes | Exception | None
) -> dict[str, Any] | None:
    """A result for a body that did not answer, or None for a served body."""
    result = {"id": row["id"], "topic_path": source_path}
    if isinstance(body, docs_api.NotFound):
        return {
            **result,
            "verdict": "missing",
            "reason": "source-topic-not-found",
            "locator": row["source_locator"],
        }
    if body is None or isinstance(body, Exception):
        return {
            **result,
            "verdict": "skipped",
            "reason": "endpoint-unreachable",
            "detail": getattr(body, "reason", "not-retrieved"),
        }
    return None


def table_id(subsystem: str, cited: str) -> str:
    """A literal DITA table id for a literal or legacy semantic citation."""
    return TABLE_ALIASES.get((subsystem, cited), cited)


def _retitled(
    row: dict[str, Any], body: str, matched_on: str, detail: dict[str, Any]
) -> dict[str, Any]:
    return {
        "id": row["id"],
        "verdict": "retitled",
        "heading": row["label"],
        "matched_on": matched_on,
        **detail,
        "last_modified": docs_api.last_modified_of(body),
    }


def check_cics_table(
    row: dict[str, Any], parts: dict[str, str], body: str
) -> dict[str, Any]:
    """Resolve one CICS row by its complete three-cell identity."""
    cited = parts.get("html-table", "")
    actual = table_id("cics", cited)
    rows = docs_api.table_rows(body, actual)
    detail: dict[str, Any] = {"table": cited, "resolved_table_id": actual}
    if rows is None:
        return _retitled(row, body, "html-table-absent", detail)
    if "eibfn" not in parts or "family" not in parts:
        return _retitled(row, body, "html-table-locator-incomplete", detail)

    wanted = tuple(
        docs_api.normalize(value)
        for value in (row["label"], parts["eibfn"], parts["family"])
    )
    matches = [
        ordinal
        for ordinal, cells in enumerate(rows, 1)
        if len(cells) == 3
        and tuple(docs_api.normalize(value) for value in cells) == wanted
    ]
    if len(matches) == 1:
        return {
            "id": row["id"],
            "verdict": "exact",
            "matched_on": "html-table-row",
            **detail,
            "row": matches[0],
        }
    if len(matches) > 1:
        return _retitled(
            row,
            body,
            "html-table-row-ambiguous",
            {**detail, "heading_found_in_rows": matches},
        )

    locator_rows = [
        ordinal
        for ordinal, cells in enumerate(rows, 1)
        if len(cells) == 3
        and docs_api.normalize(cells[1]) == wanted[1]
        and docs_api.normalize(cells[2]) == wanted[2]
    ]
    heading_rows = [
        ordinal
        for ordinal, cells in enumerate(rows, 1)
        if len(cells) == 3 and docs_api.normalize(cells[0]) == wanted[0]
    ]
    if locator_rows:
        detail["locator_rows"] = locator_rows
        detail["row_cells"] = [rows[ordinal - 1] for ordinal in locator_rows]
    if heading_rows:
        detail["heading_found_in_rows"] = heading_rows
    return _retitled(row, body, "html-table-row", detail)


def _without_footnote(value: str) -> str:
    return FOOTNOTE_SUFFIX.sub("", docs_api.normalize(value))


def check_ims_table(
    row: dict[str, Any], parts: dict[str, str], body: str
) -> dict[str, Any]:
    """Resolve one IMS comparison row by ordinal, call and command."""
    cited = parts.get("html-table", "")
    actual = table_id("ims", cited)
    rows = docs_api.table_rows(body, actual)
    detail: dict[str, Any] = {"table": cited, "resolved_table_id": actual}
    if rows is None:
        return _retitled(row, body, "html-table-absent", detail)
    ordinal = parts.get("row", "")
    if not ordinal.isdigit() or int(ordinal) < 1:
        return _retitled(row, body, "html-table-row-malformed", {**detail, "row": ordinal})
    wanted = int(ordinal)
    detail["row"] = wanted
    if wanted > len(rows):
        return _retitled(
            row,
            body,
            "html-table-row-absent",
            {**detail, "table_body_rows": len(rows)},
        )
    if "command" not in parts:
        return _retitled(row, body, "html-table-locator-incomplete", detail)

    expected_ordinal = catalog_ordinal(row)
    call = _without_footnote(rows[wanted - 1][0]) if rows[wanted - 1] else ""
    command = _without_footnote(rows[wanted - 1][1]) if len(rows[wanted - 1]) > 1 else ""
    label_matches = call == docs_api.normalize(row["label"])
    command_matches = command == docs_api.normalize(parts["command"])
    ordinal_matches = expected_ordinal == wanted
    if label_matches and command_matches and ordinal_matches:
        return {
            "id": row["id"],
            "verdict": "exact",
            "matched_on": "html-table-row",
            **detail,
        }

    detail.update(
        {
            "catalog_row": expected_ordinal,
            "row_cells": rows[wanted - 1],
            "served_call": call,
            "served_command": command,
        }
    )
    return _retitled(row, body, "html-table-row", detail)


def _compact(value: str) -> str:
    return "".join(docs_api.normalize(value).split())


def check_racroute_table(
    row: dict[str, Any], parts: dict[str, str], body: str
) -> dict[str, Any]:
    """Resolve one RACROUTE request type to one matrix header cell."""
    cited = parts.get("html-table", "")
    actual = table_id("racf-saf", cited)
    headers = docs_api.table_header_rows(body, actual)
    detail: dict[str, Any] = {"table": cited, "resolved_table_id": actual}
    if headers is None:
        return _retitled(row, body, "html-table-absent", detail)
    wanted = _compact(row["label"])
    matches = [
        (row_number, column_number)
        for row_number, cells in enumerate(headers, 1)
        for column_number, cell in enumerate(cells, 1)
        if _compact(cell) == wanted
    ]
    ordinal = catalog_ordinal(row)
    expected = (1, ordinal + 1) if ordinal is not None else None
    if matches == [expected]:
        return {
            "id": row["id"],
            "verdict": "exact",
            "matched_on": "html-table-header-cell",
            **detail,
            "header_row": expected[0],
            "column": expected[1],
        }
    if len(matches) > 1:
        how = "html-table-header-cell-ambiguous"
    else:
        how = "html-table-header-cell"
    return _retitled(
        row,
        body,
        how,
        {**detail, "catalog_column": expected[1] if expected else None, "matching_cells": matches},
    )


def check_html_table(
    row: dict[str, Any], subsystem: str, body: str
) -> dict[str, Any]:
    """Dispatch one reachable html-table locator to its published shape."""
    parts = components(row["source_locator"])
    if subsystem == "cics":
        return check_cics_table(row, parts, body)
    if subsystem == "ims":
        return check_ims_table(row, parts, body)
    if subsystem == "racf-saf":
        return check_racroute_table(row, parts, body)
    return _retitled(
        row,
        body,
        "html-table-convention-unknown",
        {"table": parts.get("html-table")},
    )


def link_name(text: str) -> str:
    """The MQ call name before the prose description in a link."""
    return docs_api.normalize(text).split(" - ", 1)[0]


def link_filename(href: str) -> str:
    """The filename component an ``html-link:`` locator records."""
    return PurePosixPath(urllib.parse.urlparse(href).path).name


def check_html_link(row: dict[str, Any], body: str) -> dict[str, Any]:
    """Resolve an MQ link by the unique semantic (name, target) pair."""
    parts = components(row["source_locator"])
    target = parts.get("html-link", "")
    anchors = docs_api.html_links(body)
    raw = [(link_name(text), link_filename(href)) for text, href in anchors]
    identities = sorted(set(raw))
    wanted = (docs_api.normalize(row["label"]), target)
    matching_anchors = [anchor for anchor, identity in zip(anchors, raw) if identity == wanted]
    distinct_anchors = sorted(set(matching_anchors))
    if identities.count(wanted) == 1 and len(distinct_anchors) == 1:
        result: dict[str, Any] = {
            "id": row["id"],
            "verdict": "exact",
            "matched_on": "html-link",
            "target": target,
        }
        if len(matching_anchors) > 1:
            result["duplicate_anchor_occurrences"] = len(matching_anchors)
        return result

    if len(distinct_anchors) > 1:
        return _retitled(
            row,
            body,
            "html-link-ambiguous",
            {
                "target": target,
                "matching_anchors": [
                    {"text": text, "href": href} for text, href in distinct_anchors
                ],
            },
        )

    same_name = sorted({filename for name, filename in identities if name == wanted[0]})
    same_target = sorted({name for name, filename in identities if filename == target})
    detail: dict[str, Any] = {"target": target}
    if same_name:
        detail["heading_found_at_targets"] = same_name
    if same_target:
        detail["target_names"] = same_target
    if len(same_name) == 1 and same_name[0] != target:
        return {
            "id": row["id"],
            "verdict": "moved",
            "reason": "link-target",
            "found_target": same_name[0],
            "heading": row["label"],
        }
    if same_target:
        return _retitled(row, body, "html-link", detail)
    return {
        "id": row["id"],
        "verdict": "missing",
        "heading": row["label"],
        **detail,
    }


def check_roadmap_normalization(row: dict[str, Any]) -> dict[str, Any]:
    """The deliberate non-publication disposition of the five VSAM rows."""
    parts = components(row["source_locator"])
    convention = parts.get("roadmap-normalization")
    if convention != ROADMAP_NORMALIZATION:
        return {
            "id": row["id"],
            "verdict": "skipped",
            "reason": "unknown-roadmap-normalization",
            "locator": row["source_locator"],
        }
    return {
        "id": row["id"],
        "verdict": "skipped",
        "reason": "documented-roadmap-normalization",
        "locator": row["source_locator"],
        "disposition": "deliberate",
        "documentation": "conformance/subsystems/coverage/catalogs/README.md",
        "detail": ROADMAP_NORMALIZATION_REASON,
    }


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
        heading,
        served,
        [node.get("label") or "" for node in filed],
        parts.get("table"),
        parts.get("row"),
        text,
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


def audit_row(
    row: dict[str, Any],
    catalog: dict[str, Any],
    baseline: dict[str, Any],
    nodes: dict[str, list[dict[str, Any]]],
    labels: dict[str, list[str]],
    tails: dict[str, list[str]],
    bodies: dict[str, bytes | Exception],
) -> dict[str, Any]:
    """Resolve any committed locator form through the same verdict vocabulary."""
    kind = locator_kind(row["source_locator"])
    if kind == "roadmap-normalization":
        return check_roadmap_normalization(row)

    source_path = locator_body_path(row, catalog, baseline)
    if source_path is None:
        return {
            "id": row["id"],
            "verdict": "skipped",
            "reason": "locator-has-no-source-body",
            "locator": row["source_locator"],
        }
    body = bodies.get(source_path)
    if kind == "topic":
        return check(row, nodes, labels, tails, body)

    unavailable = unavailable_result(row, source_path, body)
    if unavailable is not None:
        return unavailable
    assert isinstance(body, bytes)
    text = body.decode("utf-8", "replace")
    if kind == "html-table":
        result = check_html_table(row, catalog["subsystem"], text)
    elif kind == "html-link":
        result = check_html_link(row, text)
    else:
        return {
            "id": row["id"],
            "verdict": "skipped",
            "reason": "unsupported-locator",
            "locator": row["source_locator"],
        }
    result.setdefault("topic_path", source_path)
    return result


def outside_repository(path: Path) -> Path:
    """Refuse to write a report into the tree.

    Unbound JSON under `conformance/subsystems/coverage` is what broke this branch once already:
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
        "--index", type=Path, default=Path("conformance/subsystems/coverage/catalogs/index.json")
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

    rows = [row for unit in catalog["units"] for row in unit["rows"]]
    needs_toc = any(locator_kind(row["source_locator"]) == "topic" for row in rows)
    toc_state: dict[str, Any] = {
        "url": source["url"],
        "pinned_sha256": source["toc_sha256"],
        "required": needs_toc,
    }
    nodes: dict[str, list[dict[str, Any]]] = {}
    if needs_toc:
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
    wanted = sorted(
        {
            path
            for row in rows
            if (path := locator_body_path(row, catalog, baseline)) is not None
        }
    )
    bodies = dict(docs_api.topics(wanted, template, cache, args.workers))

    units: list[dict[str, Any]] = []
    totals: dict[str, int] = {}
    matched_on: dict[str, int] = {}
    for unit in catalog["units"]:
        results = [
            audit_row(row, catalog, baseline, nodes, labels, tails, bodies)
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
                "skipped": [r for r in results if r["verdict"] == "skipped"],
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
