#!/usr/bin/env python3
"""Project COBOL syntax diagrams from the reference's own DITA markup.

The web topics carry the structure the PDF only draws. Every diagram is an SVG
whose `g` elements name what they are:

    g class='groupseq'
      g class='boxed syntaxvar'      -> identifier-2
      g class=''                     <- optional wrapper
        g class=''                   <- the bypass rail: no text beneath it
        g class='boxed syntaxkwd'    -> ROUNDED

So nesting, alternation, optionality and the keyword/operand split are all
stated rather than inferred. That removes every guess the PDF reader has to
make from coordinates: no font-size window, no horizontal-overlap test for
alternation, no rail-gap heuristic separating one diagram from the next.

Optionality is the one rule worth naming: DITA renders an optional segment as a
group holding both the segment and an empty bypass sibling, so a group with a
text-free child marks its remaining children optional.

Two things are counted rather than assumed. A diagram is a statement format or
a named phrase fragment, and only the formats go into `format_titles`, because
`JSON PARSE` publishes one format beside five phrase diagrams and reporting six
would say the statement has six ways of being written. And a handful of
diagrams in this reference are drawn with syntax markup while describing
something other than statement syntax; those are declared in `NON_SYNTAX` by
topic path and title together and dropped.

This is the only producer of that projection. The schema it emits is the one
the deleted PDF projection used, so `compare_cobol_grammar.py` and the
comparison committed under `conformance/0.3/generated/` did not have to change
when the source did. It is a review input: it grants no coverage credit, is not
a normative catalog, and IBM publication bytes are never written to the
repository.
"""

from __future__ import annotations

import argparse
import json
import re
from html.entities import name2codepoint
from pathlib import Path
from typing import Any, Iterable
from xml.etree import ElementTree

SVG = "{http://www.w3.org/2000/svg}"
TITLE = re.compile(r'<h3 class="syntaxdiagram-title"[^>]*>(.*?)</h3>', re.DOTALL)
OPEN = re.compile(r"<svg\b[^>]*>")
TAG = re.compile(r"<[^>]+>")
# Attributes the topics carry that ElementTree rejects, either as undeclared
# prefixes or as presentation noise. Fragment references are anchors carrying
# `xlink:*` attributes whose prefix the SVG root never declares.
HOSTILE = re.compile(
    r'\s(?:contentscripttype|contentstyletype|zoomAndPan|preserveAspectRatio'
    r'|xml:base|longdesc|xmlns:xlink|xlink:[a-z]+)="[^"]*"'
)
# `a` matters: a fragment reference is an anchor, not a group.
STRUCTURAL = {"g", "svg", "a"}


def clean(value: str) -> str:
    return " ".join(TAG.sub("", value).replace("\xa0", " ").split())


def pieces(region: str) -> list[str]:
    """Every syntax-diagram SVG in a slice of the topic.

    A diagram too wide for the page is emitted as several
    `syntaxdiagram-piece` SVGs. They are pieces of one diagram, not separate
    diagrams, so callers join them rather than counting them.
    """
    found: list[str] = []
    for match in OPEN.finditer(region):
        if 'class="syntaxdiagram"' not in match.group(0):
            continue
        end = region.find("</svg>", match.end())
        if end < 0:
            continue
        found.append(region[match.start() : end + len("</svg>")])
    return found


def diagrams(document: str) -> list[tuple[str, list[str]]]:
    """Return each syntax diagram as its title and its SVG pieces.

    Titles delimit diagrams: everything between one `syntaxdiagram-title` and
    the next belongs to that diagram. Keying on the title rather than on the
    container div matters because the container carries varying attributes.
    """
    headings = list(TITLE.finditer(document))
    if not headings:
        return [("", pieces(document))] if pieces(document) else []
    found: list[tuple[str, list[str]]] = []
    leading = pieces(document[: headings[0].start()])
    if leading:
        found.append(("", leading))
    for index, heading in enumerate(headings):
        stop = headings[index + 1].start() if index + 1 < len(headings) else len(document)
        svgs = pieces(document[heading.end() : stop])
        if svgs:
            found.append((clean(heading.group(1)), svgs))
    return found


ENTITY = re.compile(r"&([A-Za-z][A-Za-z0-9]*);")
XML_ENTITIES = {"amp", "lt", "gt", "quot", "apos"}


def numeric(match: re.Match[str]) -> str:
    """XML knows five named entities; the topics use the HTML set."""
    name = match.group(1)
    if name in XML_ENTITIES:
        return match.group(0)
    codepoint = name2codepoint.get(name)
    return f"&#{codepoint};" if codepoint else ""


def parse(svg: str) -> ElementTree.Element:
    return ElementTree.fromstring(ENTITY.sub(numeric, HOSTILE.sub("", svg)))


def groups(node: ElementTree.Element) -> list[ElementTree.Element]:
    return [child for child in node if child.tag.replace(SVG, "") in STRUCTURAL]


def label(node: ElementTree.Element) -> str:
    return clean("".join(node.itertext()))


def speaks(node: ElementTree.Element) -> bool:
    """A group with no text is a bypass rail rather than a diagram element."""
    return bool(label(node))


def classes(node: ElementTree.Element) -> set[str]:
    return set((node.get("class") or "").split())


def collect(
    node: ElementTree.Element,
    relation: str | None,
    main: list[dict[str, str]],
    branches: list[dict[str, Any]],
) -> None:
    names = classes(node)
    if "boxed" in names or names & {"fragref", "syntaxfragref"}:
        kind = (
            "keyword"
            if "syntaxkwd" in names
            else "operand"
            if "syntaxvar" in names
            else "fragment"
        )
        value = label(node)
        if not value:
            return
        if relation is None:
            main.append({"kind": kind, "value": value})
        else:
            branches.append({"kind": kind, "value": value, "relation": relation})
        return

    children = groups(node)
    spoken = [child for child in children if speaks(child)]
    # A text-free sibling is the bypass rail around an optional segment.
    if len(spoken) < len(children) and spoken:
        for child in spoken:
            collect(child, relation or "optional", main, branches)
        return
    if "groupchoice" in names:
        for index, child in enumerate(spoken):
            # Only a choice on the main line introduces alternation. Inside an
            # optional segment every branch stays optional, because none of
            # them can be required.
            collect(
                child,
                relation if index == 0 else (relation or "alternative"),
                main,
                branches,
            )
        return
    for child in spoken:
        collect(child, relation, main, branches)


FORMAT = re.compile(r"^Format\b", re.IGNORECASE)


def kind_of(title: str) -> str:
    """Separate a statement format from a named phrase fragment.

    `JSON GENERATE` publishes one statement format and five phrase diagrams
    (`when-phrase Format`, `converting-phrase Format 1`, ...). Counting those
    as formats overstates how many ways the statement can be written.
    """
    return "format" if FORMAT.match(title.strip()) else "fragment"


def form(title: str, svgs: Iterable[str]) -> dict[str, Any]:
    main: list[dict[str, str]] = []
    branches: list[dict[str, Any]] = []
    for svg in svgs:
        collect(parse(svg), None, main, branches)
    return {
        "title": title,
        "kind": kind_of(title),
        "main_line": main,
        "branches": branches,
    }


# Diagrams the reference draws with syntax markup that are not statement
# syntax. Each is declared as the topic path AND the title the diagram would
# enter under, both must agree, and `main` refuses to write unless every
# declared exclusion fired exactly once -- so an exclusion cannot quietly start
# dropping a different diagram the way a title-only match could.
#
# rlpsjsopj.html is the `JSON PARSE` topic "Valid and invalid elementary
# moves". Its diagram is not a form of the statement: it sits inside an `ol`
# inside a `td colspan="9"` footnote of the elemoves table, introduced by "If
# the JSON string conforms to the following syntax diagram, it is either
# treated as an integer or fixed-point non-integer", and its main line reads
# `" [0-9] "` with optional `'+'`, `'-'`, `'.'` and `space` branches. It
# pictures the lexical shape of a JSON number, which is why the only keywords
# it contributes are `[0-9]` and `space`. The topic carries no
# `syntaxdiagram-title`, so the diagram would otherwise enter under the topic
# heading and be counted as a seventh `JSON PARSE` diagram.
NON_SYNTAX = {
    "SS6SG3_6.5/lr/ref/rlpsjsopj.html": "Valid and invalid elementary moves",
}


def project(
    row: dict[str, Any],
    documents: Iterable[tuple[str, str, str]],
    excluded: dict[tuple[str, str], int],
) -> dict[str, Any]:
    forms: list[dict[str, Any]] = []
    for topic, path, document in documents:
        for title, svgs in diagrams(document):
            name = title or topic
            if NON_SYNTAX.get(path) == name:
                excluded[(path, name)] = excluded.get((path, name), 0) + 1
                continue
            item = form(name, svgs)
            if item["main_line"] or item["branches"]:
                forms.append(item)
    return {
        "id": row["id"],
        "row_id": row["row_id"],
        "title": row["label"],
        "catalog_forms": row.get("forms", []),
        # Statement formats only. A phrase fragment is a diagram of a phrase
        # the statement may take, not another way of writing the statement, so
        # listing it here would report `JSON PARSE` as having seven formats
        # against the catalog's one when the publication states one.
        "format_titles": [
            item["title"] for item in forms if item["title"] and item["kind"] == "format"
        ],
        "forms": forms,
    }


def parse_args(argv: Iterable[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--topics", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--catalog", type=Path, required=True)
    parser.add_argument("--unit", default="procedure_statements")
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args(list(argv) if argv is not None else None)


def main(argv: Iterable[str] | None = None) -> int:
    args = parse_args(argv)
    catalog = json.loads(args.catalog.read_text(encoding="utf-8"))
    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    topics = {entry["id"]: entry for entry in manifest["topics"] if entry["located"]}

    rows: list[dict[str, Any]] = []
    excluded: dict[tuple[str, str], int] = {}
    for row in catalog[args.unit]:
        entry = topics.get(row["id"])
        if entry is None:
            rows.append({"id": row["id"], "row_id": row["row_id"], "title": row["label"],
                         "catalog_forms": row.get("forms", []), "format_titles": [], "forms": []})
            continue
        documents = [
            (
                topic["label"],
                topic["topic_path"],
                (args.topics / topic["file"]).read_text(encoding="utf-8"),
            )
            for topic in entry["topics"]
        ]
        rows.append(project(row, documents, excluded))

    for path, title in NON_SYNTAX.items():
        if excluded.get((path, title)) != 1:
            raise SystemExit(
                f"non-syntax exclusion {path} / {title!r} matched "
                f"{excluded.get((path, title), 0)} diagrams, expected 1"
            )

    output = {
        "schema_version": "mainframe-env.cobol-html-grammar-projection@1",
        "coverage_credit": 0,
        "source": {
            "product": manifest["product"],
            "topics": sum(len(entry["topics"]) for entry in topics.values()),
            "retained_in_repository": False,
        },
        "rows": rows,
    }
    args.output.write_text(
        json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    formats = sum(
        1 for row in rows for item in row["forms"] if item["kind"] == "format"
    )
    fragments = sum(
        1 for row in rows for item in row["forms"] if item["kind"] == "fragment"
    )
    print(f"rows={len(rows)} formats={formats} phrase-fragments={fragments}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
