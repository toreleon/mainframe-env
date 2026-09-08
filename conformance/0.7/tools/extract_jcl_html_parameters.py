#!/usr/bin/env python3
"""Project the JCL Reference statement and parameter inventory from its topics.

The MVS JCL Reference gives every statement its own chapter and every parameter
its own topic beneath that chapter (`ACCODE parameter`, `AMP parameter`, ...).
That structure is the published inventory, so this projection reads the topic
tree rather than parsing syntax art.

It replaces a reader that took the same inventory from the PDF outline, and the
one thing that reader got right is the thing easiest to lose here. The outline
was read at depth exactly 1 -- chapter, then its immediate entries. The topic
tree is deeper: the DD statement subtree is 566 nodes over four levels
(1 / 75 / 451 / 39). Most of that depth is inert, and saying which part is not
is the whole of the guard. A section heading cannot be taken for a parameter at
any depth, because `PARAMETER` requires the label to end in the word: across the
book's 204 parameter topics their children include 199 `Syntax`, 188
`Subparameter definition`, 124 `Defaults` and 109 `Overrides`, and none of those
can match at level 1 or at level 3. What a descendant walk would really admit is
the cross-reference and example topics, which do end in the word -- 91
`Relationship to other parameters`, `Examples of the AMP parameter`, and at
level 3 `Effect of DCB=dsname parameter`. Deduplicated by name, that reports 424
parameters instead of 204: 150 DD, 48 EXEC, 80 JOB and 146 OUTPUT. Not 451,
which is only the DD subtree's level-2 node count and is what the number here
used to say. `parameters()` therefore takes a chapter's DIRECT CHILDREN and
nothing below them, and the tests assert the four counts the catalog records.

Three levels are emitted under three separate keys:

  inventory   the parity-critical one: 74 / 19 / 35 / 76 parameters, by
              position among a chapter's children
  statements  the 20 JCL and 13 JES2 JECL statements, each resolved to the
              chapter that documents it, cross-checked against the two summary
              tables that state the same rosters
  syntax      per parameter, the syntax art and the subparameter terms its
              `Syntax` and `Subparameter definition` GRANDCHILDREN carry

The keys are separate so a defect in the third cannot move the first's counts.

The emitted projection is a review input. It grants no coverage credit, is not
a normative catalog, and IBM publication bytes are never written to the
repository.
"""

from __future__ import annotations

import argparse
import html as html_module
import json
import re
import sys
from pathlib import Path
from typing import Any, Iterable

CONFORMANCE = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(CONFORMANCE / "tools"))
sys.path.insert(0, str(CONFORMANCE / "0.6" / "tools"))

import docs_api

# The AMS reader already decided how a DITA definition list is read: a term is
# a `dt`, its nesting depth separates a parameter from the values it accepts,
# and a term carries its argument and its alternation. The JCL subparameter
# topics are the same markup, so these are imported rather than restated.
from extract_ams_html_parameters import Definitions, clean, names  # noqa: E402

PRODUCT = "SSLTBW_3.2.0"
BOOK = "z/OS MVS JCL Reference"

#: Resolved by href, never by label. The navigation tree files this book's
#: `abstract.htm` twice -- once as the book node carrying all 34 chapters and
#: once as a childless `abstract.htm?pos=2` leaf titled "Abstract for MVS JCL
#: Reference" -- so a label lookup can hand back the empty one.
BOOK_HREF = "SSLTBW_3.2.0/com.ibm.zos.v3r2.ieab600/abstract.htm"

CHAPTER = re.compile(r"^Chapter\s+(\d+)\.\s+(.*)$")
PARAMETER = re.compile(r"^(.+?)\s+parameters?$", re.IGNORECASE)

# Catalog unit -> the chapter title that owns its parameters.
UNITS = {
    "dd-parameters": "DD statement",
    "exec-parameters": "EXEC statement",
    "job-parameters": "JOB statement",
    "output-parameters": "OUTPUT JCL statement",
}

#: Catalog unit -> the chapter whose children are its statements.
JECL_CHAPTER = "SSLTBW_3.2.0/com.ibm.zos.v3r2.ieab600/j2st.htm"

#: The JES2 chapter opens with a prose section that is not a statement.
JECL_EXCLUDED = {"Description"}

#: Two catalog labels do not become a chapter title by adding " statement".
#: The book titles them "IF/THEN/ELSE/ENDIF statement construct" and "XMIT JCL
#: statement", so both name their topic instead of being matched by text. The
#: topic is named by its last path segment, which is unique within the book and
#: survives a product-key bump that would move the whole path.
STATEMENT_BY_TOPIC = {
    "IF/THEN/ELSE/ENDIF": "ifuse.htm",
    "XMIT": "xmitst.htm",
}

#: The summary tables that publish the same two rosters, and the count each
#: must state once its header row is removed. The denominators are frozen; this
#: is the publication's own statement of them, read back.
STATEMENT_TABLES = {
    "jcl-statements": {
        "topic_path": "SSLTBW_3.2.0/com.ibm.zos.v3r2.ieab600/iea3b6_JCL_statements.htm",
        "table_id": "idg6175__cjsts",
        "expected_body_rows": 20,
    },
    "jes2-jecl-statements": {
        "topic_path": "SSLTBW_3.2.0/com.ibm.zos.v3r2.ieab600/iea3b6_JECL_statements.htm",
        "table_id": "idg6277__cjsts1",
        "expected_body_rows": 13,
    },
}

SYNTAX_LABEL = "Syntax"
SUBPARAMETER_PREFIX = "Subparameter definition"

CODEBLOCK = re.compile(
    r"<pre\b[^>]*\bclass=\"[^\"]*\bcodeblock\b[^\"]*\"[^>]*>(.*?)</pre>", re.S
)
ROW = re.compile(r"<tr\b", re.IGNORECASE)


# --------------------------------------------------------------------------
# Structure. Everything here reads the table of contents and nothing else.
# --------------------------------------------------------------------------


def label_of(node: dict[str, Any]) -> str:
    return (node.get("label") or "").strip()


def href_of(node: dict[str, Any]) -> str:
    """A node's topic path with any `?pos=` navigation suffix dropped."""
    return (node.get("href") or "").split("?", 1)[0]


def children(node: dict[str, Any]) -> list[dict[str, Any]]:
    return list(node.get("topics") or [])


def chapter_title(node: dict[str, Any]) -> str:
    """A chapter's title as published, with any printed chapter number dropped.

    The topic tree does not number this book's chapters, but the reader that
    read the PDF outline had to strip `Chapter 12. ` and the reviewed labels
    elsewhere in the tree still carry that form, so the same narrow prefix is
    dropped here and both lookups agree on what a chapter is called.
    """
    return CHAPTER.sub(lambda match: match.group(2).strip(), label_of(node))


def book(document: dict[str, Any], href: str = BOOK_HREF) -> dict[str, Any]:
    """The book node, found by href and required to be the one with children.

    Fails closed. A book that cannot be found is a retrieval or pin problem and
    must stop the run; silently projecting an empty inventory would report the
    denominators as zero and read as a finding about the publication.
    """
    found: list[dict[str, Any]] = []

    def walk(node: dict[str, Any]) -> None:
        if href_of(node) == href:
            found.append(node)
        for child in children(node):
            walk(child)

    walk(document["toc"])
    carrying = [node for node in found if children(node)]
    if not carrying:
        raise ValueError(f"book node not found or has no chapters: {href}")
    if len(carrying) > 1:
        raise ValueError(f"book href resolves to {len(carrying)} distinct subtrees: {href}")
    return carrying[0]


def chapters(node: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """Map a chapter title to the book child that carries it.

    The printed book numbers its chapters and the topic tree does not, so a
    title is taken as published here -- `DD statement`, not `Chapter 12. DD
    statement`. The first child under a repeated title wins, matching the
    `setdefault` the outline reader used.
    """
    found: dict[str, dict[str, Any]] = {}
    for child in children(node):
        title = chapter_title(child)
        if title:
            found.setdefault(title, child)
    return found


def parameters(chapter: dict[str, Any]) -> list[dict[str, Any]]:
    """The parameters a chapter documents: its DIRECT CHILDREN, in order.

    Direct children only -- though not because of the section headings, which
    are the wrong thing to picture here. `Syntax`, `Subparameter definition`,
    `Defaults` and `Overrides` do not end in the word and so cannot match
    `PARAMETER` at any depth. What a descendant walk admits is the deeper
    topics that do end in it, `Relationship to other parameters` and `Examples
    of the AMP parameter` among them, and it reports 150 "DD parameters"
    instead of 74.
    """
    found: list[dict[str, Any]] = []
    seen: set[str] = set()
    for child in children(chapter):
        match = PARAMETER.match(label_of(child))
        if not match:
            continue
        name = match.group(1).strip().upper()
        if name in seen:
            continue
        seen.add(name)
        found.append(
            {
                "name": name,
                "label": label_of(child),
                "topic_path": href_of(child),
                "topic_id": child.get("topicId"),
            }
        )
    return found


def subtree_size(node: dict[str, Any]) -> dict[str, int]:
    """Node counts per level below a chapter, kept as the depth trap's receipt."""
    levels: dict[int, int] = {}

    def walk(current: dict[str, Any], depth: int = 0) -> None:
        levels[depth] = levels.get(depth, 0) + 1
        for child in children(current):
            walk(child, depth + 1)

    walk(node)
    return {str(depth): levels[depth] for depth in sorted(levels)}


def statements(node: dict[str, Any], labels: Iterable[str]) -> list[dict[str, Any]]:
    """Resolve catalog statement labels onto the chapters that document them.

    A label becomes a chapter by adding ` statement` and comparing case-folded,
    because the catalog reads `comment` and `null` where the book heads
    `Comment statement` and `Null Statement`. Two labels the book titles
    differently are named by href instead.

    Fails closed on any label it cannot resolve: an unresolved statement is a
    finding about this reader, and reporting it as an absent statement would
    make it look like one about the publication.
    """
    by_label = {chapter_title(child).casefold(): child for child in children(node)}
    by_topic = {
        href_of(child).rsplit("/", 1)[-1]: child for child in children(node)
    }
    found: list[dict[str, Any]] = []
    unresolved: list[str] = []
    for label in labels:
        if label in STATEMENT_BY_TOPIC:
            child = by_topic.get(STATEMENT_BY_TOPIC[label])
            rule = "topic"
        else:
            child = by_label.get(f"{label} statement".casefold())
            rule = "label-plus-statement"
        if child is None:
            unresolved.append(label)
            continue
        found.append(
            {
                "label": label,
                "chapter": label_of(child),
                "topic_path": href_of(child),
                "topic_id": child.get("topicId"),
                "rule": rule,
            }
        )
    if unresolved:
        raise ValueError(f"statement labels unresolved: {', '.join(unresolved)}")
    return found


def jecl_statements(node: dict[str, Any], href: str = JECL_CHAPTER) -> list[dict[str, Any]]:
    """The JES2 control statements: the JECL chapter's children minus Description."""
    chapter = next((child for child in children(node) if href_of(child) == href), None)
    if chapter is None:
        raise ValueError(f"JES2 control statement chapter not found: {href}")
    found: list[dict[str, Any]] = []
    for child in children(chapter):
        label = label_of(child)
        if label in JECL_EXCLUDED:
            continue
        found.append(
            {
                "label": label,
                "chapter": label,
                "topic_path": href_of(child),
                "topic_id": child.get("topicId"),
                "rule": "jecl-chapter-child",
            }
        )
    return found


def sections(node: dict[str, Any]) -> dict[str, list[str]]:
    """A parameter's `Syntax` and `Subparameter definition` GRANDCHILDREN.

    Grandchildren of the chapter, children of the parameter. Naming the level
    explicitly is the guard: these are exactly the nodes `parameters()` must
    never see, and collecting them here is what proves they were excluded there
    rather than lost.
    """
    syntax: list[str] = []
    subparameter: list[str] = []
    for child in children(node):
        label = label_of(child)
        if label == SYNTAX_LABEL:
            syntax.append(href_of(child))
        elif label.startswith(SUBPARAMETER_PREFIX):
            subparameter.append(href_of(child))
    return {"syntax": syntax, "subparameter": subparameter}


# --------------------------------------------------------------------------
# Bodies. Everything here reads one topic and nothing about the tree.
# --------------------------------------------------------------------------


def codeblocks(body: str) -> list[str]:
    """The syntax art a topic carries, line structure intact.

    Deliberately not `clean()`. A syntax diagram is laid out in columns --

        DISP= ( [NEW] [,DELETE ] [,DELETE ] )
                [OLD] [,KEEP   ] [,KEEP   ]

    -- so collapsing whitespace would destroy the only thing the block states.
    Leading and trailing blank lines go; nothing inside a line moves.
    """
    found: list[str] = []
    for match in CODEBLOCK.finditer(body):
        text = html_module.unescape(docs_api.TAG.sub("", match.group(1)))
        lines = [line.replace("\xa0", " ").rstrip() for line in text.splitlines()]
        while lines and not lines[0].strip():
            lines.pop(0)
        while lines and not lines[-1].strip():
            lines.pop()
        if lines:
            found.append("\n".join(lines))
    return found


def subparameters(body: str) -> tuple[list[str], list[str]]:
    """The outermost definition terms of a subparameter topic, and their names.

    The terms are kept verbatim because a JCL subparameter is often lowercase
    metasyntax -- `access-code`, `nnn`, `delivery address` -- which the AMS
    `names()` filter is right to reject as a parameter name and wrong to be the
    only record of. Both are emitted: the terms as published, and the names
    `names()` recognises within them.
    """
    parser = Definitions()
    parser.feed(body)
    if not parser.terms:
        return [], []
    outermost = min(depth for depth, _ in parser.terms)
    terms: list[str] = []
    for depth, term in parser.terms:
        if depth != outermost:
            continue
        text = clean(term)
        if text and text not in terms:
            terms.append(text)
    recognised: list[str] = []
    for term in terms:
        for name in names(term):
            if name not in recognised:
                recognised.append(name)
    return terms, recognised


def table_body_rows(body: str, table_id: str) -> int | None:
    """`<tr>` count of the named table less its header row, or None if absent."""
    opening = re.search(rf"<table\b[^>]*\bid=\"{re.escape(table_id)}\"", body)
    if not opening:
        return None
    end = body.find("</table>", opening.start())
    if end < 0:
        return None
    return len(ROW.findall(body[opening.start() : end])) - 1


# --------------------------------------------------------------------------
# Catalog labels. Unchanged from the reader this replaces; both are pure
# functions over labels and were already covered by their own tests.
# --------------------------------------------------------------------------

SUFFIX = re.compile(r"\s+parameters?$", re.IGNORECASE)


def normalize(label: str) -> str:
    """Catalog labels keep the reference's own `X parameter` outline wording."""
    return SUFFIX.sub("", label.strip()).strip().upper()


def catalog_names(catalog: dict[str, Any], unit_id: str) -> list[str]:
    for unit in catalog["units"]:
        if unit["id"] == unit_id:
            return [normalize(row["label"]) for row in unit["rows"]]
    raise ValueError(f"catalog unit not found: {unit_id}")


def catalog_labels(catalog: dict[str, Any], unit_id: str) -> list[str]:
    for unit in catalog["units"]:
        if unit["id"] == unit_id:
            return [row["label"] for row in unit["rows"]]
    raise ValueError(f"catalog unit not found: {unit_id}")


# --------------------------------------------------------------------------
# Projection.
# --------------------------------------------------------------------------


def read_body(topics: Path | None, topic_path: str) -> str | None:
    if topics is None:
        return None
    candidate = topics / topic_path.rsplit("/", 1)[-1]
    if not candidate.is_file():
        return None
    return candidate.read_text(encoding="utf-8", errors="replace")


def inventory(node: dict[str, Any], catalog: dict[str, Any]) -> list[dict[str, Any]]:
    units: list[dict[str, Any]] = []
    found = chapters(node)
    for unit_id, chapter in UNITS.items():
        current = found.get(chapter)
        source = parameters(current) if current is not None else []
        source_names = [item["name"] for item in source]
        recorded = catalog_names(catalog, unit_id)
        units.append(
            {
                "unit": unit_id,
                "chapter": chapter,
                "chapter_located": current is not None,
                "chapter_topic_path": href_of(current) if current is not None else None,
                "chapter_subtree_nodes": subtree_size(current) if current is not None else {},
                "chapter_direct_children": len(children(current)) if current is not None else 0,
                "catalog_count": len(recorded),
                "source_count": len(source),
                "source_parameters": source,
                "ordered_match": source_names == recorded,
                "only_in_source": sorted(set(source_names) - set(recorded)),
                "only_in_catalog": sorted(set(recorded) - set(source_names)),
                "shared": len(set(source_names) & set(recorded)),
            }
        )
    return units


def statement_units(
    node: dict[str, Any], catalog: dict[str, Any], topics: Path | None
) -> list[dict[str, Any]]:
    units: list[dict[str, Any]] = []
    for unit_id in ("jcl-statements", "jes2-jecl-statements"):
        recorded = catalog_labels(catalog, unit_id)
        if unit_id == "jcl-statements":
            source = statements(node, recorded)
        else:
            source = jecl_statements(node)
        table = dict(STATEMENT_TABLES[unit_id])
        body = read_body(topics, table["topic_path"])
        table["body_rows"] = (
            table_body_rows(body, table["table_id"]) if body is not None else None
        )
        table["read"] = body is not None
        table["agrees"] = (
            table["body_rows"] == table["expected_body_rows"] if body is not None else None
        )
        units.append(
            {
                "unit": unit_id,
                "catalog_count": len(recorded),
                "source_count": len(source),
                "source_statements": source,
                "ordered_match": [item["label"] for item in source] == recorded,
                "table": table,
            }
        )
    return units


def syntax_units(node: dict[str, Any], topics: Path | None) -> dict[str, Any]:
    found = chapters(node)
    rows: list[dict[str, Any]] = []
    for unit_id, chapter in UNITS.items():
        current = found.get(chapter)
        if current is None:
            continue
        by_href = {href_of(child): child for child in children(current)}
        for item in parameters(current):
            named = sections(by_href[item["topic_path"]])
            art: list[str] = []
            for path in named["syntax"]:
                body = read_body(topics, path)
                if body is not None:
                    art.extend(codeblocks(body))
            terms: list[str] = []
            recognised: list[str] = []
            for path in named["subparameter"]:
                body = read_body(topics, path)
                if body is None:
                    continue
                found_terms, found_names = subparameters(body)
                terms.extend(term for term in found_terms if term not in terms)
                recognised.extend(name for name in found_names if name not in recognised)
            rows.append(
                {
                    "unit": unit_id,
                    "name": item["name"],
                    "topic_path": item["topic_path"],
                    "syntax_topics": named["syntax"],
                    "subparameter_topics": named["subparameter"],
                    "source_syntax": art,
                    "source_subparameters": terms,
                    "source_subparameter_names": recognised,
                }
            )
    # Three gaps are properties of the publication rather than of this reader,
    # and are listed so that nobody has to rediscover them by subtraction.
    # Six parameters have no `Syntax` child at all: RECFM splits its syntax
    # across three access-method sections, and five leaf parameters carry it in
    # their own topic. Three `Syntax` topics -- all three CCSID parameters --
    # set `CCSID= nnnnn` in a borderless one-cell table instead of a code
    # block. Seven `Subparameter definition` topics are containers whose terms
    # sit one level further down.
    return {
        "parameters": rows,
        "counts": {
            "parameters": len(rows),
            "syntax_topics": sum(len(row["syntax_topics"]) for row in rows),
            "subparameter_topics": sum(len(row["subparameter_topics"]) for row in rows),
            "with_syntax": sum(1 for row in rows if row["source_syntax"]),
            "with_subparameters": sum(1 for row in rows if row["source_subparameters"]),
        },
        "without_syntax_topic": [
            f"{row['unit']}:{row['name']}" for row in rows if not row["syntax_topics"]
        ],
        "syntax_topic_without_codeblock": [
            f"{row['unit']}:{row['name']}"
            for row in rows
            if row["syntax_topics"] and not row["source_syntax"]
        ],
        "subparameter_topic_without_terms": [
            f"{row['unit']}:{row['name']}"
            for row in rows
            if row["subparameter_topics"] and not row["source_subparameters"]
        ],
    }


def parse_args(argv: Iterable[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--toc", type=Path, required=True, help="a saved table of contents")
    parser.add_argument("--catalog", type=Path, required=True)
    parser.add_argument("--topics", type=Path, help="a directory of fetched topic bodies")
    parser.add_argument("--manifest", type=Path, help="the topic manifest, recorded as the source")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--book-href", default=BOOK_HREF)
    return parser.parse_args(list(argv) if argv is not None else None)


def main(argv: Iterable[str] | None = None) -> int:
    args = parse_args(argv)
    toc_bytes = args.toc.read_bytes()
    document = json.loads(toc_bytes.decode("utf-8"))
    node = book(document, args.book_href)
    catalog = json.loads(args.catalog.read_text(encoding="utf-8"))

    units = inventory(node, catalog)
    statement_rows = statement_units(node, catalog, args.topics)
    syntax = syntax_units(node, args.topics)

    source: dict[str, Any] = {
        "product": PRODUCT,
        "book": BOOK,
        "book_href": args.book_href,
        "toc_sha256": docs_api.digest(toc_bytes),
        "content_url_template": docs_api.CONTENT_URL,
        "retained_in_repository": False,
    }
    if args.manifest is not None:
        manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
        source["topic_manifest_digest"] = manifest["topic_manifest_digest"]
        source["topics"] = manifest["topic_count"]

    output = {
        "schema_version": "mainframe-env.jcl-html-parameter-projection@1",
        "coverage_credit": 0,
        "source": source,
        "inventory": {"units": units},
        "statements": {"units": statement_rows},
        "syntax": syntax,
    }
    args.output.write_text(
        json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    for unit in units:
        print(
            f"{unit['unit']:20} catalog={unit['catalog_count']:3} "
            f"source={unit['source_count']:3} shared={unit['shared']:3} "
            f"+src={len(unit['only_in_source']):3} +cat={len(unit['only_in_catalog']):3} "
            f"ordered={'yes' if unit['ordered_match'] else 'NO'}"
        )
    for unit in statement_rows:
        table = unit["table"]
        print(
            f"{unit['unit']:20} catalog={unit['catalog_count']:3} "
            f"source={unit['source_count']:3} "
            f"ordered={'yes' if unit['ordered_match'] else 'NO'} "
            f"table={table['body_rows'] if table['read'] else 'unread'}"
            f"/{table['expected_body_rows']}"
        )
    counts = syntax["counts"]
    print(
        f"{'syntax':20} parameters={counts['parameters']:3} "
        f"syntax_topics={counts['syntax_topics']:3} "
        f"subparameter_topics={counts['subparameter_topics']:3} "
        f"with_syntax={counts['with_syntax']:3} "
        f"with_subparameters={counts['with_subparameters']:3}"
    )
    # A table nobody could read is not a table that disagreed. Without
    # `--topics` there are no bodies, so levels 2 and 3 report what they could
    # not check and the run still fails, but it fails saying `unread` rather
    # than reporting a denominator mismatch that was never observed.
    healthy = all(unit["ordered_match"] for unit in units) and all(
        unit["ordered_match"] and unit["table"]["agrees"] is True for unit in statement_rows
    )
    return 0 if healthy else 1


if __name__ == "__main__":
    raise SystemExit(main())
