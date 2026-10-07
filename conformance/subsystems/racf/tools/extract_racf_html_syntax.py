#!/usr/bin/env python3
"""Project the RACF command syntax from the markup the reference is written in.

Every rule here is structural. The retired PDF reader inferred structure from
the shape of a line — a segment was recognised because it opened a parenthesis
at the end of one, so `CICS(OPCLASS(...))` set inline promoted OPCLASS to a
top-level operand of the command — and it recognised a command at all only by
finding a `{KEYWORD | ALIAS}` brace anywhere in the extracted text, which is how
`SET` came back with the aliases `SETONLY` and `NOSET`. Both are operand values
of the SET command; neither is an abbreviation anyone can type. The topics state
what the geometry only implied, so nothing below counts a bracket or measures an
indent.

The rules, in the order they are applied:

1. Scoping. Every claim is scoped to a `<section>` named by its own
   `<h2 class="sectiontitle">`. Operands are read only from the `dl` under
   `Parameters`, and the syntax line only from the tables under `Syntax`. A
   definition list under `Examples` is not syntax. The Syntax table is normally
   marked `role="presentation"`, but four topics mark it with a `summary`
   instead, so the table is taken by its position in the section rather than by
   that attribute — see `syntax_tables`.

2. Aliases. Searched only inside `section.refsyn` — the Syntax section — and
   only for a brace group anchored on the command's own keyword. `SET` has no
   brace group of its own there and correctly returns none. All 34 catalog alias
   lists reproduce.

3. Operands and what they contain. A `dt` at the outermost `dl` depth under
   `Parameters` is an operand, and every `dt` below it is kept where the topic
   puts it: the tree is as deep as the list is, which in this book is six levels
   for ALTUSER and ALTGROUP and five for ADDGROUP, ADDUSER, RALTER, SET and
   SETROPTS. Each term is read against ITS OWN parent, never against the operand
   at the root of its branch. A child name the parent term offers as an
   alternative — between keywords (`AT | ONLYAT` restates `AT(...)` and
   `ONLYAT(...)` beneath itself), inside its own argument
   (`DOM(NORMAL | ALL | NONE)` restates all three), or inside that argument
   where ANOTHER topic writes it (see `restatements`) — is a VALUE it accepts.
   Any other child name is a MEMBER: the segment operands `OMVS` owns, and
   equally the enumerated values of a term no topic restates, which `syntax_only`
   cannot separate and this reader does not pretend to — see `classify`. That
   per-parent reading is the separation the AMS extractor does not make: it
   flattens every depth below the first into one list and then drops any name
   that also appears at top level, which would answer "which operand does this
   belong to" with silence.

4. `syntax_only`. Uppercase tokens the Syntax table shows that the Parameters
   tree never reaches, minus the command's keyword and aliases. The reference's
   own boilerplate is excluded structurally: the intro and outro paragraphs
   carry `data-hd-otherprops="nohelp"` and are dropped before the table is read.
   A non-empty list is a reviewer's question — a keyword the book puts in the
   diagram and does not define under Parameters — not a defect claim.

5. RACDCERT. Its umbrella topic has a Syntax section that says to read the
   subtopics and no Parameters section at all, so its row is synthesised from
   the 26 function topics. Each contributes its own operands, values and
   members, including the `ID(...) | SITE | CERTAUTH` qualifier every function
   restates, and each function's own contribution is kept beside the union so a
   reviewer can see which function a name came from.

6. The value/member overlap, reported as two populations rather than one total.
   `source_values` and `source_members` are per-row unions over every level, so
   a name can land in both. Almost all of that overlap is the list restating a
   name one level below where it introduces it — `OMVS` over
   `MEMLIMIT | NOMEMLIMIT` over `MEMLIMIT(nonshared-memory-size)` makes MEMLIMIT
   a member of OMVS and a value of the term between them — which says nothing
   about the reference. What is worth a reviewer's attention is the rest: a name
   a term calls a member while some other term calls it a value. The two are
   `restated_one_level_down` and `value_and_member_of_different_terms`, and they
   partition the overlap, so nothing is lost by not also reporting their sum.
   See `overlap`.

The emitted projection is a review input. It grants no coverage credit, is not a
normative catalog, and it does not modify
`conformance/subsystems/racf/racf/command-language.json`: the catalog-only operand names it
lists are dispositions for review, not edits.
"""

from __future__ import annotations

import argparse
import html as html_module
import json
import re
import sys
from pathlib import Path
from typing import Any, Iterable

REPOSITORY = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(REPOSITORY / "conformance" / "subsystems" / "dataset" / "tools"))

from extract_ams_html_parameters import ARGUMENT, Definitions  # noqa: E402
from extract_ams_html_parameters import names as ams_names  # noqa: E402

SECTION_TAG = re.compile(r"</?section\b[^>]*>", re.I)
SECTION_OPEN = re.compile(r"<section\b[^>]*>", re.I)
SECTION_TITLE = re.compile(
    r"\A\s*<h2[^>]*\bclass=\"[^\"]*\bsectiontitle\b[^\"]*\"[^>]*>(.*?)</h2>", re.S | re.I
)
TABLE_OPEN = re.compile(r"<table\b[^>]*>", re.I)
TABLE_TAG = re.compile(r"</?table\b[^>]*>", re.I)
NOHELP_PARAGRAPH = re.compile(
    r"<p\b[^>]*\bdata-hd-otherprops=\"nohelp\"[^>]*>.*?</p>", re.S | re.I
)
TAG = re.compile(r"<[^>]+>")
TOKEN = re.compile(r"[A-Z][A-Z0-9]+(?:-[A-Z0-9]+)*")

SYNTAX = "Syntax"
PARAMETERS = "Parameters"


def text_of(markup: str) -> str:
    """The visible text of a fragment, with entities resolved and runs collapsed."""
    return " ".join(
        html_module.unescape(TAG.sub(" ", markup)).replace("\xa0", " ").split()
    )


def sections(body: str) -> list[dict[str, str]]:
    """Every `<section>` with the `sectiontitle` heading that names it.

    The elements nest, so the close is found by counting rather than by taking
    the next `</section>`, and a section whose heading is not a `sectiontitle`
    reports a title of None instead of borrowing its neighbour's.
    """
    found: list[dict[str, str]] = []
    for opening in SECTION_OPEN.finditer(body):
        depth = 0
        end = len(body)
        for tag in SECTION_TAG.finditer(body, opening.start()):
            depth += -1 if tag.group(0).startswith("</") else 1
            if depth == 0:
                end = tag.start()
                break
        inner = body[opening.end() : end]
        heading = SECTION_TITLE.match(inner)
        found.append(
            {
                "title": text_of(heading.group(1)) if heading else None,
                "attributes": opening.group(0),
                "html": inner,
            }
        )
    return found


def section_named(body: str, title: str) -> str | None:
    """The one section carrying this heading, or None when the topic has none.

    Two sections with the same heading is a shape nobody has seen in this book
    and would make "the Parameters list" ambiguous, so it is refused rather than
    resolved by taking the first.
    """
    matched = [entry for entry in sections(body) if entry["title"] == title]
    if not matched:
        return None
    if len(matched) > 1:
        raise ValueError(f"{len(matched)} sections are titled {title!r}")
    return matched[0]["html"]


def names(term: str) -> list[str]:
    """The keyword names a `dt` term introduces.

    AMS's `names()` does the work — arguments off first, then the alternation
    split, then the reference's own brackets — and this applies it to each
    whitespace-delimited keyword of the term rather than to the term as a whole.
    RACF needs that because a term can name a sequence: RACDCERT IMPORT opens
    `IMPORT TOKEN(token-name) SEQNUM(sequence-number)`, three operands in one
    `dt`, which AMS's split reads as one unnamed run and discards. Arguments are
    removed before the whitespace split, not after, because an argument contains
    spaces of its own.
    """
    head = term
    while True:
        stripped = ARGUMENT.sub("", head)
        if stripped == head:
            break
        head = stripped
    found: list[str] = []
    for chunk in head.split():
        for name in ams_names(chunk):
            if name not in found:
                found.append(name)
    return found


def alias_pattern(keyword: str) -> re.Pattern[str]:
    """`{KEYWORD | ALIAS | ...}`, anchored on the keyword.

    Anchoring is the whole fix, because the book puts a brace group around an
    operand's alternatives as readily as around a command's abbreviations and
    the two are the same shape. The example is `{SET | SETONLY | NOSET}`, and it
    is in `addsd.htm`, not in `set.htm`: it is ADDSD's own
    `SET | SETONLY | NOSET` operand, which the Parameters list defines with
    `SET`, `SETONLY` and `NOSET` restated beneath it. An unanchored search over
    that one Syntax section returns `SET | SETONLY | NOSET` and
    `GENERIC | MODEL | TAPE` beside the real `{ADDSD | AD}`. `set.htm` contains
    neither string anywhere in its body, and the three brace groups its Syntax
    section does carry — `{SYSTEM | JOBNAME(jobname ...)}`,
    `{COUNT(number) | RESET}` and `{[ALL | NONE] [ALTER | NOALTER] ...}` — are
    likewise operands rather than abbreviations.

    An earlier revision of this docstring said instead that an unanchored search
    "over the SET topic" matches `{SETONLY | NOSET}`. No such group exists, in
    that topic or in any other. The sentence carried over the SETONLY/NOSET
    finding from the retired PDF reader, which matched a brace group anywhere in
    the extracted text of the whole book and so had no topic to be attributed
    to. The anchoring is unchanged and the SET regression still holds — this
    reader returns no alias for SET — but it holds because the only group naming
    SETONLY belongs to ADDSD, not because SET's own page carries one.
    """
    return re.compile(
        r"\{\s*" + re.escape(keyword) + r"((?:\s*\|\s*[A-Z][A-Z0-9]*)*)\s*\}"
    )


def aliases(refsyn: str, keyword: str) -> list[str]:
    """The command's documented abbreviations, from its own brace group."""
    found: list[str] = []
    for group in alias_pattern(keyword).findall(text_of(refsyn)):
        for part in group.split("|"):
            name = part.strip()
            if name and name != keyword and name not in found:
                found.append(name)
    return found


def operand_tree(parameters: str) -> list[dict[str, Any]]:
    """The `dt` terms of the Parameters list, nested as the topic nests them.

    Depth comes from `dl` nesting, which is what the topic publishes; the PDF
    had only leading whitespace to go on. Every level is kept. Collapsing the
    ones below the first is the defect this replaces: ALTUSER's list runs six
    levels deep, so `NETVIEW | NONETVIEW > NETVIEW > MSGRECVR | NOMSGRECVR >
    MSGRECVR(YES | NO) > YES` reported `YES` as a member of the NETVIEW segment,
    and `AUTH`'s MASTER, ALL, INFO, CONS, IO and SYS and `LEVEL`'s NB, ALL, CE
    and IN arrived as contents of OPERPARM.

    A term one level deeper than the previous one is that term's child; a term
    at the outermost depth starts a new operand. A term more than one level
    deeper than anything open above it has no parent in the list, which is a
    finding about this reader rather than about the publication, so it stops the
    run instead of being attached to the nearest available term. No topic of the
    60 skips a level.
    """
    parser = Definitions()
    parser.feed(parameters)
    if not parser.terms:
        return []
    outermost = min(depth for depth, _ in parser.terms)
    roots: list[dict[str, Any]] = []
    ancestors: list[dict[str, Any]] = []
    for depth, term in parser.terms:
        level = depth - outermost
        node: dict[str, Any] = {"term": term, "children": []}
        if level == 0:
            roots.append(node)
            ancestors = [node]
            continue
        if level > len(ancestors):
            raise ValueError(
                f"term {term!r} sits at nesting level {level} with no term open "
                f"at level {level - 1}"
            )
        del ancestors[level:]
        ancestors[-1]["children"].append(node)
        ancestors.append(node)
    return roots


ALTERNATION = re.compile(r"\(([^()]*)\)")


def argument_alternatives(term: str) -> list[str]:
    """The names the term's own parenthesised argument alternates between.

    `MSGRECVR(NO | YES)`, `DOM(NORMAL | ALL | NONE)` and `XRFSOFF(FORCE |
    NOFORCE)` state their alternatives inside the parentheses; a placeholder
    argument such as `LEVEL(message-level)` states none.
    """
    offered: list[str] = []
    for argument in ALTERNATION.findall(term):
        if "|" not in argument:
            continue
        for part in argument.split("|"):
            for name in names(part):
                if name not in offered:
                    offered.append(name)
    return offered


def restatements(stated: Iterable[tuple[str, str]]) -> dict[str, dict[str, Any]]:
    """Every operand name the book writes with its alternatives spelled out.

    This is read across all 60 topics before any of them is classified, and it
    exists because the reference writes the same operand two ways. ALTUSER
    writes `CTL (GENERAL | GLOBAL | SPECIFIC)` over the same three nested terms
    that ADDUSER hangs under a bare `CTL`; RDEFINE writes `ACEE( YES | NO )` and
    `MIXED( YES | NO )` where RALTER writes both bare over YES and NO beneath.
    Reading each term alone therefore made GENERAL a value under one command and
    a member under another for markup that says the same thing, and made YES a
    value of RDEFINE's ACEE and a member of RALTER's. That is a fact about which
    topic happened to typeset the alternation, not about the reference, and the
    reader is a reader of the book rather than of one page in it.

    So a term's alternatives are the ones ANY topic states for that operand
    name, and the divergence resolves toward the topic that spells them out --
    the more informative of the two readings, and the one the publication makes
    explicitly somewhere. Over the pinned 60 topics 52 operand names are stated
    this way and the union changes the reading of 12 child names under 5 of
    them: CTL, DOM and LOGCMDRESP in ADDUSER, ACEE and MIXED in RALTER. Each is
    recorded in the projection's `cross_topic_restatements` with the term it was
    read from, because a value that cannot be found on the page it is reported
    against has to be traceable to the page it did come from.

    It still reaches no further than a restatement. Where NO topic spells the
    enumeration into the term -- `AUTH` over MASTER, ALL, INFO, CONS, IO and
    SYS, `LEVEL(message-level)` over NB, ALL, CE and IN -- those names stay
    members. That is the rule stating what it can see, not a claim that AUTH is
    a segment.
    """
    found: dict[str, dict[str, Any]] = {}
    for keyword, term in stated:
        offered = argument_alternatives(term)
        if not offered:
            continue
        for name in names(term):
            entry = found.setdefault(
                name, {"operand": name, "alternatives": [], "stated_by": []}
            )
            for alternative in offered:
                if alternative not in entry["alternatives"]:
                    entry["alternatives"].append(alternative)
            source = {"keyword": keyword, "term": term}
            if source not in entry["stated_by"]:
                entry["stated_by"].append(source)
    return found


def alternatives(
    term: str, restated: dict[str, dict[str, Any]] | None = None
) -> list[str]:
    """Every name this term offers as one of its own alternatives.

    Three sources, in order: the keywords the term itself names (`AT | ONLYAT`),
    the alternation inside its own argument, and -- when `restated` is supplied
    -- the alternation another topic writes for the same operand name. Called
    with no map it is the single-page reading, which is what
    `cross_topic_restatements` is measured against.

    The map is consulted for the term's OWN names only, never for the
    alternatives it has just been given. Following those too would make the
    lookup transitive -- `SET(... | CLASS)` would collect whatever some other
    topic writes for CLASS -- and the extension is a restatement of one operand,
    not a closure over the book's vocabulary.
    """
    offered = list(names(term))
    for name in names(term):
        for alternative in (restated or {}).get(name, {}).get("alternatives", []):
            if alternative not in offered:
                offered.append(alternative)
    for name in argument_alternatives(term):
        if name not in offered:
            offered.append(name)
    return offered


def classify(
    node: dict[str, Any], restated: dict[str, dict[str, Any]] | None = None
) -> dict[str, Any]:
    """One term with the values and members of its DIRECT children, recursively.

    `values` and `members` describe one level. They are the names the children
    of this term introduce, split by whether this term offers them — not the
    names of the whole subtree, which is what the collapsed reading reported and
    why a segment appeared to own its members' values.
    """
    offered = alternatives(node["term"], restated)
    values: list[str] = []
    members: list[str] = []
    for child in node["children"]:
        for name in names(child["term"]):
            target = values if name in offered else members
            if name not in target:
                target.append(name)
    return {
        "term": node["term"],
        "names": names(node["term"]),
        "values": values,
        "members": members,
        "children": [classify(child, restated) for child in node["children"]],
    }


def operands(
    parameters: str, restated: dict[str, dict[str, Any]] | None = None
) -> list[dict[str, Any]]:
    """The operand terms, each carrying its own nested terms to full depth."""
    return [classify(node, restated) for node in operand_tree(parameters)]


def descendants(entries: Iterable[dict[str, Any]]) -> Iterable[dict[str, Any]]:
    """Every classified term in the forest, parents before children."""
    for entry in entries:
        yield entry
        yield from descendants(entry["children"])


def overlap(entries: Iterable[dict[str, Any]]) -> tuple[list[str], list[str]]:
    """The names that read as both a value and a member, split into two.

    `source_values` and `source_members` are unions over every level of the row,
    so a name that a term offers and another term contains lands in both. A
    single count of that overlap reads as "the reference uses this many names in
    both roles" and does not mean it. Against the projection ff50ae5 produced,
    where that count was 405, this splits 384 / 21; against this one, where the
    cross-topic restatement has already resolved two of them, 384 / 19. Almost
    all of it either way is the list restating a name one level below where it
    introduces it, which is a property of how a `dl` is typeset.

    The shape is `OMVS` > `MEMLIMIT | NOMEMLIMIT` > `MEMLIMIT(...)`. The middle
    term is an alternation header rather than an operand of its own, so its
    names are members of OMVS -- MEMLIMIT is genuinely an operand of the OMVS
    segment -- and the terms it heads are its values. Both readings are true and
    neither is a finding, so a name whose EVERY member occurrence is restated
    that way goes in the first list and is not what anyone should be looking at.

    The second list is the one that is: a name some term calls a member without
    restating it beneath, while some other term calls it a value. ADDUSER's ALL
    is a member of AUTH and LEVEL and a value of
    `ROUTCODE(ALL | NONE | routing-codes)`; SET's ALL is a value of
    `ALL | NONE | TYPE` and a member of DATABASE. Those are the reference using
    one word for two things, which is a question a reviewer can act on.

    The split is by occurrence, not by name, and that matters. Over ff50ae5's
    projection 10 names carry both a restated occurrence and an unrestated one
    -- ADDGROUP's GID is restated under OMVS and bare under OVM -- and they
    belong in the second list, because one unexplained occurrence is enough to
    be worth reading. An existential rule that asks only whether SOME occurrence
    is restated puts all 10 in the first and reports 394 and 11 where this
    reports 384 and 21.
    """
    terms = list(descendants(entries))
    restated_here: list[str] = []
    elsewhere: list[str] = []
    offered = {name for term in terms for name in term["values"]}
    for term in terms:
        for name in term["members"]:
            if name not in offered:
                continue
            if name in restated_here or name in elsewhere:
                continue
            sites = [entry for entry in terms if name in entry["members"]]
            target = (
                restated_here
                if all(
                    any(name in child["values"] for child in site["children"])
                    for site in sites
                )
                else elsewhere
            )
            target.append(name)
    return sorted(restated_here), sorted(elsewhere)


def nesting_depth(entries: list[dict[str, Any]]) -> int:
    """How many levels the list runs, counting the operand level as 1."""
    if not entries:
        return 0
    return 1 + max(nesting_depth(entry["children"]) for entry in entries)


def syntax_tables(syntax: str) -> list[str]:
    """The tables of the Syntax section, boilerplate paragraphs dropped first.

    The section holds nothing else. Its prose is exactly two paragraphs — a
    pointer to the syntax key and a pointer to the TSO and operator command
    chapters — and both carry `data-hd-otherprops="nohelp"`, which is how they
    are dropped: by the attribute the publisher puts on them, not by matching
    their wording.

    Every table, not only the `role="presentation"` ones. 55 of the 60 topics
    mark the syntax table that way and four — DELUSER, RACDCERT EXPORT,
    RACDCERT REKEY and RACPRMCK — give it a `summary="Syntax of the ... command"`
    instead. Requiring the attribute reads those four as having no syntax at
    all, which is a silent miss of exactly the kind this reader exists to stop.
    """
    body = NOHELP_PARAGRAPH.sub(" ", syntax)
    found: list[str] = []
    consumed = 0
    for opening in TABLE_OPEN.finditer(body):
        if opening.start() < consumed:
            continue  # a table nested inside one already taken
        depth = 0
        end = len(body)
        for tag in TABLE_TAG.finditer(body, opening.start()):
            depth += -1 if tag.group(0).startswith("</") else 1
            if depth == 0:
                end = tag.start()
                break
        found.append(body[opening.end() : end])
        consumed = end
    return found


def syntax_tokens(syntax: str) -> list[str]:
    """Uppercase tokens the Syntax section's tables show."""
    collected: list[str] = []
    for table in syntax_tables(syntax):
        for token in TOKEN.findall(text_of(table)):
            if token not in collected:
                collected.append(token)
    return collected


def read(topics: Path, record: dict[str, Any]) -> str:
    return (topics / record["file"]).read_text(encoding="utf-8")


def parameter_terms(body: str) -> list[str]:
    """Every `dt` term of the topic's Parameters list, flat and unclassified.

    The first of the two passes: `restatements` needs the terms of all 60 topics
    before any topic can be classified, and needs only their text.
    """
    parameters = section_named(body, PARAMETERS)
    if parameters is None:
        return []
    parser = Definitions()
    parser.feed(parameters)
    return [term for _, term in parser.terms]


def surface(
    body: str, keyword: str, restated: dict[str, dict[str, Any]] | None = None
) -> dict[str, Any]:
    """One topic's syntax surface: aliases, the operand terms, and the tokens."""
    syntax = section_named(body, SYNTAX)
    parameters = section_named(body, PARAMETERS)
    return {
        "aliases": aliases(syntax, keyword) if syntax else [],
        "terms": operands(parameters, restated) if parameters else [],
        "tokens": syntax_tokens(syntax) if syntax else [],
        "has_parameters": parameters is not None,
    }


def merge(target: list[str], addition: Iterable[str]) -> None:
    for name in addition:
        if name not in target:
            target.append(name)


def row_terms(row: dict[str, Any]) -> list[dict[str, Any]]:
    """A row's classified operand terms, its own and its functions', in order."""
    collected = list(row.get("operand_terms") or [])
    for function in row.get("functions") or []:
        collected.extend(function["operand_terms"])
    return collected


def cross_topic_restatements(
    rows: Iterable[dict[str, Any]], restated: dict[str, dict[str, Any]]
) -> list[dict[str, Any]]:
    """Where a value was read from another topic's spelling of the same operand.

    Measured against the single-page reading rather than tracked through
    `classify`: a value the term does not offer when `alternatives` is called
    without the map is one the book supplied from elsewhere. Only the entries
    that changed something are emitted, so this is the whole cost of reading the
    book instead of the page, listed by name.
    """
    applied: dict[str, dict[str, Any]] = {}
    for row in rows:
        for entry in descendants(row_terms(row)):
            local = alternatives(entry["term"])
            extra = [name for name in entry["values"] if name not in local]
            if not extra:
                continue
            for operand in names(entry["term"]):
                supplied = [
                    name
                    for name in extra
                    if name in restated.get(operand, {}).get("alternatives", [])
                ]
                if not supplied:
                    continue
                record = applied.setdefault(
                    operand,
                    {
                        "operand": operand,
                        "alternatives": restated[operand]["alternatives"],
                        "stated_by": restated[operand]["stated_by"],
                        "applied_to": [],
                    },
                )
                record["applied_to"].append(
                    {
                        "keyword": row["keyword"],
                        "term": entry["term"],
                        "names": supplied,
                    }
                )
    return [applied[operand] for operand in sorted(applied)]


def project(
    family: dict[str, Any],
    record: dict[str, Any],
    topics: Path,
    restated: dict[str, dict[str, Any]] | None = None,
) -> dict[str, Any]:
    keyword = family["keyword"]
    row: dict[str, Any] = {
        "row_id": family["row_id"],
        "keyword": keyword,
        "located": bool(record and record.get("located")),
        "catalog_aliases": family.get("aliases", []),
        "catalog_operands": family.get("operands", []),
    }
    if not row["located"]:
        return row

    read_topics = [
        {
            "topic_path": record["topic_path"],
            "topic_id": record["topic_id"],
            "sha256": record["sha256"],
            "label": record["label"],
        }
    ]
    own = surface(read(topics, record), keyword, restated)
    terms = list(own["terms"])
    tokens = list(own["tokens"])
    functions: list[dict[str, Any]] = []

    for function in record.get("functions") or []:
        read_topics.append(
            {
                "topic_path": function["topic_path"],
                "topic_id": function["topic_id"],
                "sha256": function["sha256"],
                "label": function["label"],
            }
        )
        contribution = surface(read(topics, function), keyword, restated)
        terms.extend(contribution["terms"])
        merge(tokens, contribution["tokens"])
        functions.append(
            {
                "function": function["keyword"],
                "topic_path": function["topic_path"],
                "operand_terms": contribution["terms"],
            }
        )

    found: list[str] = []
    values: list[str] = []
    members: list[str] = []
    for term in terms:
        merge(found, term["names"])
    # Values and members are collected from every level, because every level
    # now classifies its own children; `found` stays the operand level alone.
    for term in descendants(terms):
        merge(values, term["values"])
        merge(members, term["members"])

    reachable = set(found) | set(values) | set(members)
    excluded = reachable | {keyword} | set(own["aliases"])
    catalog_operands = family.get("operands", [])
    restated_down, elsewhere = overlap(terms)
    row.update(
        {
            "topic_path": record["topic_path"],
            "topic_id": record["topic_id"],
            "topic_sha256": record["sha256"],
            "topics": read_topics,
            "synthesised_from_functions": bool(functions),
            "source_aliases": own["aliases"],
            "source_operands": found,
            "source_values": values,
            "source_members": members,
            # `source_values` and `source_members` overlap, and the two lists
            # below partition that overlap instead of totalling it. The first is
            # the list restating a name one level below where it introduces it,
            # which is typography; the second is a name one term contains and
            # another offers, which is the reference. See `overlap`.
            "restated_one_level_down": restated_down,
            "value_and_member_of_different_terms": elsewhere,
            "source_nesting_depth": nesting_depth(terms),
            "operand_terms": own["terms"],
            "catalog_only": [
                name for name in catalog_operands if name not in found
            ],
            "source_only": [
                name for name in found if name not in catalog_operands
            ],
            "syntax_only": [name for name in tokens if name not in excluded],
        }
    )
    if functions:
        row["functions"] = functions
    return row


def parse_args(argv: Iterable[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--topics", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--catalog", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args(list(argv) if argv is not None else None)


def main(argv: Iterable[str] | None = None) -> int:
    args = parse_args(argv)
    catalog = json.loads(args.catalog.read_text(encoding="utf-8"))
    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    by_keyword = {record["keyword"]: record for record in manifest["families"]}

    # First pass: what the book states about each operand, read from all 60
    # topics, so that no topic's reading depends on the order they are visited
    # in. Second pass: classify each topic against it.
    stated: list[tuple[str, str]] = []
    for family in catalog["families"]:
        record = by_keyword.get(family["keyword"])
        if not (record and record.get("located")):
            continue
        for source in [record, *(record.get("functions") or [])]:
            for term in parameter_terms(read(args.topics, source)):
                stated.append((family["keyword"], term))
    restated = restatements(stated)

    rows = [
        project(family, by_keyword.get(family["keyword"]), args.topics, restated)
        for family in catalog["families"]
    ]

    located = sum(1 for row in rows if row["located"])
    output = {
        # @3: `both_value_and_member` is gone, replaced by the two lists that
        # partition it -- `restated_one_level_down` and
        # `value_and_member_of_different_terms` -- because the single total read
        # as a statement about the reference and was 95% a statement about how a
        # `dl` is typeset. A term's `values` may now also name an alternative
        # another topic spells out for the same operand; `cross_topic_restatements`
        # lists every place that happened.
        #
        # @2: `operand_terms` nests to the depth the topic nests, and an entry's
        # `values` and `members` name its direct children instead of its whole
        # subtree. A reader of @1 asking a segment what it contains got its
        # members' values back as members of the segment.
        "schema_version": "mainframe-env.racf-html-syntax-projection@3",
        "coverage_credit": 0,
        "source": {
            "product": manifest["product"],
            "book": manifest["book_label"],
            "book_href": manifest["book_href"],
            "topic_manifest_digest": manifest["topic_manifest_digest"],
            "topics": manifest["topic_count"],
            "pinned_baseline": manifest["pinned_baseline"],
            "retained_in_repository": False,
        },
        "cross_topic_restatements": cross_topic_restatements(rows, restated),
        "totals": {
            "families": len(rows),
            "located": located,
            "source_operands": sum(len(row.get("source_operands", [])) for row in rows),
            "source_values": sum(len(row.get("source_values", [])) for row in rows),
            "source_members": sum(len(row.get("source_members", [])) for row in rows),
            "restated_one_level_down": sum(
                len(row.get("restated_one_level_down", [])) for row in rows
            ),
            "value_and_member_of_different_terms": sum(
                len(row.get("value_and_member_of_different_terms", []))
                for row in rows
            ),
            "max_nesting_depth": max(
                (row.get("source_nesting_depth", 0) for row in rows), default=0
            ),
            "catalog_operands": sum(len(row["catalog_operands"]) for row in rows),
            "catalog_only": sum(len(row.get("catalog_only", [])) for row in rows),
            "source_only": sum(len(row.get("source_only", [])) for row in rows),
            "syntax_only": sum(len(row.get("syntax_only", [])) for row in rows),
            "alias_lists_reproduced": sum(
                1
                for row in rows
                if row.get("source_aliases", []) == row["catalog_aliases"]
            ),
        },
        "rows": rows,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    totals = output["totals"]
    print(
        f"families={totals['families']} located={totals['located']} "
        f"source_operands={totals['source_operands']} "
        f"values={totals['source_values']} members={totals['source_members']} "
        f"catalog_only={totals['catalog_only']} "
        f"aliases_reproduced={totals['alias_lists_reproduced']}/{totals['families']}"
    )
    return 0 if located == len(rows) else 1


if __name__ == "__main__":
    raise SystemExit(main())
