#!/usr/bin/env python3
"""Every figure `docs/research/publication-source-probe.md` quotes, from the artifact it cites.

The record has gone stale in the same way in each of four waves: a workstream
moves a number, the record was written before it landed, and nobody diffs
prose. Hand-correcting it a fifth time fixes the four waves and not the fifth,
so this reads the committed artifacts and prints what they say, and the record
cites this tool's keys inline so a test can diff prose against artifact.

Offline. Nothing here touches the network, and nothing here reads a publication
body: every input is a JSON projection, a catalog, a manifest or a Rust source
file already in the repository. Publication bytes are never retained, so the
projections are as close to the books as anything in the tree gets, and the
figures below are therefore facts about this repository -- what it recorded when
the readers last ran -- and not fresh readings of IBM's publications. That
distinction is the one the record has to keep, and it is why `--format json`
emits `coverage_credit: 0`.

Two kinds of figure are deliberately absent, because pretending to derive them
offline is how a checked number becomes an unchecked one:

  * anything only a live run knows -- how the 732 non-chapter-numbered,
    non-table-cited locator rows split between the served `h1` and a
    table-of-contents label, or how many topics a re-read reports changed;
  * anything stated only in prose -- a tool docstring's count of what a
    descendant walk would have admitted, or a measurement written to $TMPDIR.

The record marks those as unchecked where it quotes them.

`--check` fails on a citation that disagrees with its artifact and on a citation
of a key no artifact reports. It does *not* fail on a figure this tool reports
that the record cites nowhere: which of these 206 the record ought to quote is an
editorial judgement, and making silence red would buy agreement by padding the
prose with numbers nobody wants to read. Those are listed under `not cited` so
the judgement can be made, and never counted as failures.
`conformance/tools/tests/test_probe_figures.py` asserts the same thing the
`--check` exit status does, so a wrong number in the record fails the Python
suite whether or not anyone runs this tool by hand.

Usage:

    python3 conformance/tools/report_probe_figures.py
    python3 conformance/tools/report_probe_figures.py --format json
    python3 conformance/tools/report_probe_figures.py --check
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any, Callable, Iterator, NamedTuple

REPOSITORY = Path(__file__).resolve().parents[2]
RECORD = "docs/research/publication-source-probe.md"

#: The record writes small numbers as words -- "all nine pins", "two rows still"
#: -- and forcing them into digits to make them checkable would trade the prose
#: for the check. So a citation may be spelled either way.
NUMBER_WORDS = {
    word: value
    for value, word in enumerate(
        "zero one two three four five six seven eight nine ten eleven twelve "
        "thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty".split()
    )
}

#: `1,506` and `4,488` are how the record writes them, so the marker syntax has
#: to admit the separator. `<!--f:KEY-->` renders as nothing.
CITATION = re.compile(
    r"(?P<value>\d[\d,]*|(?i:" + "|".join(NUMBER_WORDS) + r"))<!--f:(?P<key>[a-z0-9_.]+)-->"
)

CATALOGS = "conformance/0.2/catalogs"
MANIFESTS = "conformance/0.2/manifests"

#: Baselines in the order `conformance/0.2/catalogs/index.json` lists them, so
#: the tables the record prints come out in the record's own order rather than
#: alphabetically.
BASELINES = (
    "cobol",
    "cics",
    "jcl-jes2",
    "dataset-vsam-ams",
    "racf-saf",
    "zosmf",
    "db2",
    "ims",
    "mq",
)

#: Reader manifest -> the baseline pin every topic in it must appear in.
READERS = (
    ("cobol", "conformance/0.3/generated/cobol-topic-manifest.json", "cobol"),
    ("ams", "conformance/0.6/generated/ams-topic-manifest.json", "dataset-vsam-ams"),
    ("racf", "conformance/0.5/generated/racf-topic-manifest.json", "racf-saf"),
    ("jcl", "conformance/0.7/generated/jcl-topic-manifest.json", "jcl-jes2"),
)


class Figure(NamedTuple):
    """One number, and the file the record has to agree with about it."""

    key: str
    value: int
    source: str

    @property
    def line(self) -> str:
        return f"{self.key:<44} {self.value:>12,}  {self.source}"


class Mismatch(NamedTuple):
    key: str
    quoted: int
    actual: int
    source: str


def slug(name: str) -> str:
    """A figure-key fragment: `jcl-jes2` and `DEFINE CLUSTER` are not keys."""
    return re.sub(r"[^a-z0-9]+", "_", name.lower()).strip("_")


def read(root: Path, relative: str) -> Any:
    return json.loads((root / relative).read_text(encoding="utf-8"))


def digest(value: str) -> str:
    """Manifests write bare hex and the reader manifests write `sha256:`-prefixed."""
    return value.split(":", 1)[1] if value.startswith("sha256:") else value


def topics_of(node: Any) -> Iterator[tuple[str, str]]:
    """Every `(topic_path, digest)` anywhere in a document.

    The four reader manifests nest their topics differently -- COBOL under
    `topics[].topics[]`, AMS under `commands[].topics[]`, JCL flat, RACF under
    `families[]` -- and the shape is theirs to choose. Walking for the pair is
    what lets one containment check serve all four without this file having to
    be edited whenever one of them is regenerated with another level.
    """
    if isinstance(node, dict):
        if "topic_path" in node and "sha256" in node:
            yield str(node["topic_path"]), digest(str(node["sha256"]))
        for value in node.values():
            yield from topics_of(value)
    elif isinstance(node, list):
        for value in node:
            yield from topics_of(value)


def rust_function(root: Path, relative: str, name: str) -> tuple[str, int]:
    """The body of a top-level Rust function, and the 1-based line it opens on.

    The record cites four line numbers in Rust and Python source. A line number
    is the fastest-rotting figure in the file and the one a reader is least
    likely to check, so they are figures here like any other.
    """
    text = (root / relative).read_text(encoding="utf-8")
    opening = f"\nfn {name}("
    at = text.find(opening)
    if at < 0:
        raise SystemExit(f"{relative}: no top-level `fn {name}(`")
    start = at + 1
    end = text.find("\n}\n", start)
    if end < 0:
        raise SystemExit(f"{relative}: `fn {name}` does not close at column 0")
    return text[start:end], text.count("\n", 0, start) + 1


def python_line(root: Path, relative: str, needle: str) -> int:
    """The 1-based line a statement is on, for the same reason."""
    lines = (root / relative).read_text(encoding="utf-8").splitlines()
    for number, line in enumerate(lines, start=1):
        if line.startswith(needle):
            return number
    raise SystemExit(f"{relative}: no line starting `{needle}`")


def pins(root: Path) -> Iterator[Figure]:
    """Per-baseline topic and byte counts, and the two totals over all nine."""
    topics = bytes_ = 0
    for baseline in BASELINES:
        source = f"{MANIFESTS}/{baseline}-topics.json"
        manifest = read(root, source)
        topics += int(manifest["topic_count"])
        bytes_ += int(manifest["total_bytes"])
        yield Figure(f"pins.topics.{slug(baseline)}", manifest["topic_count"], f"{source} topic_count")
        yield Figure(f"pins.bytes.{slug(baseline)}", manifest["total_bytes"], f"{source} total_bytes")
    yield Figure("pins.baselines", len(BASELINES), f"{CATALOGS}/index.json baselines[]")
    yield Figure("pins.topics_total", topics, f"{MANIFESTS}/*.json sum of topic_count")
    yield Figure("pins.bytes_total", bytes_, f"{MANIFESTS}/*.json sum of total_bytes")

    index = read(root, f"{CATALOGS}/index.json")
    supporting = [
        entry
        for baseline in index["baselines"]
        for entry in baseline.get("supporting_sources", [])
    ]
    yield Figure(
        "pins.racroute_bytes",
        supporting[0]["bytes"],
        f"{CATALOGS}/index.json racf-saf.supporting_sources[0].bytes",
    )


def locators(root: Path) -> Iterator[Figure]:
    """What the 1,506 rows are located by, counted off the catalogs themselves.

    The locator audit's own table is a live-run artifact; these are the
    denominators it runs against, and they are the half of it a reader can
    recompute without asking IBM anything.
    """
    kinds: dict[str, int] = {}
    rows = located = publication_located = embedded_located = chapters = cited = 0
    headings: dict[tuple[str, str], int] = {}
    numbered: dict[str, int] = {}
    for baseline in BASELINES:
        source = f"{CATALOGS}/{baseline}.json"
        catalog = read(root, source)
        here: dict[str, int] = {}
        for unit in catalog["units"]:
            for row in unit["rows"]:
                locator = row["source_locator"]
                kind = locator.split(":", 1)[0]
                here[kind] = here.get(kind, 0) + 1
                kinds[kind] = kinds.get(kind, 0) + 1
                rows += 1
                if kind in ("topic", "html-table", "html-link"):
                    publication_located += 1
                if kind in ("html-table", "html-link"):
                    embedded_located += 1
                if kind != "topic":
                    continue
                parts = dict(
                    piece.split(":", 1) for piece in locator.split(";") if ":" in piece
                )
                heading = parts.get("heading", "")
                key = (baseline, heading.strip().lower())
                headings[key] = headings.get(key, 0) + 1
                if re.match(r"^Chapter \d+\. ", heading):
                    chapters += 1
                    numbered[baseline] = numbered.get(baseline, 0) + 1
                if "table" in parts:
                    cited += 1
        yield Figure(f"catalog.rows.{slug(baseline)}", sum(here.values()), f"{source} units[].rows[]")
        yield Figure(
            f"catalog.topic_located.{slug(baseline)}",
            here.get("topic", 0),
            f"{source} rows whose source_locator opens `topic:`",
        )
        if sum(here.values()) != here.get("topic", 0):
            yield Figure(
                f"catalog.other_located.{slug(baseline)}",
                sum(here.values()) - here.get("topic", 0),
                f"{source} rows located some other way",
            )
        located += here.get("topic", 0)
    repeated = sum(count for count in headings.values() if count > 1)

    every = f"{CATALOGS}/*.json (index.json excluded) units[].rows[]"
    yield Figure("catalog.rows_total", rows, every)
    yield Figure("catalog.topic_located_total", located, f"{every}, `topic:` prefix")
    yield Figure(
        "catalog.publication_located_total",
        publication_located,
        f"{every}, `topic:`, `html-table:` or `html-link:` prefix",
    )
    yield Figure(
        "catalog.embedded_located_total",
        embedded_located,
        f"{every}, `html-table:` or `html-link:` prefix",
    )
    yield Figure("catalog.not_topic_located_total", rows - located, f"{every}, other prefixes")
    for kind in ("html-table", "html-link", "roadmap-normalization"):
        yield Figure(
            f"catalog.{slug(kind)}_total", kinds.get(kind, 0), f"{every}, `{kind}:` prefix"
        )
    yield Figure(
        "catalog.chapter_numbered_headings",
        chapters,
        f"{every}, topic rows whose heading matches `^Chapter \\d+\\. `",
    )
    for baseline, count in numbered.items():
        yield Figure(
            f"catalog.chapter_numbered_headings.{slug(baseline)}",
            count,
            f"{CATALOGS}/{baseline}.json, headings matching `^Chapter \\d+\\. `",
        )
    yield Figure(
        "catalog.table_cited_rows", cited, f"{every}, topic rows that also cite a `table:`"
    )
    yield Figure(
        "catalog.heading_only_rows",
        located - chapters - cited,
        f"{every}, topic rows resolved on a heading with no chapter number and no table",
    )
    yield Figure(
        "catalog.repeated_headings",
        repeated,
        f"{every}, topic rows sharing a heading with another row of the same catalog",
    )
    for baseline in BASELINES:
        here = sum(
            count for (book, _), count in headings.items() if book == baseline and count > 1
        )
        if here:
            yield Figure(
                f"catalog.repeated_headings.{slug(baseline)}",
                here,
                f"{CATALOGS}/{baseline}.json, headings shared by two or more rows",
            )


def ledger(root: Path) -> Iterator[Figure]:
    """The two numbers that say none of this bought coverage."""
    source = "conformance/0.2/evidence/coverage-ledger.json"
    book = read(root, source)
    yield Figure(
        "ledger.official_compatibility_numerator",
        book["official_compatibility_numerator"],
        f"{source} official_compatibility_numerator",
    )
    yield Figure(
        "ledger.generated_catalog_credit",
        book["generated_catalog_credit"],
        f"{source} generated_catalog_credit",
    )


def readers(root: Path) -> Iterator[Figure]:
    """Each reader reads only pinned topics, at pinned digests -- checkably.

    The claim the record makes is containment at an identical digest, not merely
    that the paths look familiar, so both halves are counted: how many topics a
    reader records, and how many of those the baseline pin carries at the same
    sha256. A reader that drifted onto a same-version stand-in would show the
    two apart.
    """
    for name, manifest, baseline in READERS:
        pin = {
            entry["topic_path"]: digest(entry["sha256"])
            for entry in read(root, f"{MANIFESTS}/{baseline}-topics.json")["topics"]
        }
        recorded = dict(topics_of(read(root, manifest)))
        inside = sum(1 for path, sha in recorded.items() if pin.get(path) == sha)
        yield Figure(f"readers.{name}.topics", len(recorded), f"{manifest} distinct topic_path")
        yield Figure(
            f"readers.{name}.topics_in_pin",
            inside,
            f"{manifest} topics also in {MANIFESTS}/{baseline}-topics.json at an identical sha256",
        )


def cobol(root: Path) -> Iterator[Figure]:
    """The comparison's own totals, plus the per-row figures the record names."""
    source = "conformance/0.3/generated/cobol-grammar-comparison.json"
    comparison = read(root, source)
    for name, value in sorted(comparison["totals"].items()):
        yield Figure(f"cobol.{name}", value, f"{source} totals.{name}")

    rows = {row["id"]: row for row in comparison["rows"]}
    for statement in ("accept", "set", "json_generate", "json_parse", "xml_generate", "start"):
        row = rows[statement.replace("_", "-")]
        yield Figure(
            f"cobol.forms.{statement}",
            row["catalog_form_count"],
            f"{source} rows[{row['id']}].catalog_form_count",
        )
        yield Figure(
            f"cobol.formats.{statement}",
            row["source_format_count"],
            f"{source} rows[{row['id']}].source_format_count",
        )
        yield Figure(
            f"cobol.missing_keywords.{statement}",
            len(row["keywords_missing_from_catalog"]),
            f"{source} rows[{row['id']}].keywords_missing_from_catalog",
        )
    yield Figure(
        "cobol.format_titles.set",
        len(rows["set"]["source_format_titles"]),
        f"{source} rows[set].source_format_titles",
    )
    yield Figure(
        "cobol.rows_keyword_complete",
        sum(1 for row in rows.values() if not row["keywords_missing_from_catalog"]),
        f"{source} rows with an empty keywords_missing_from_catalog",
    )
    word = re.compile(r"[A-Z][A-Z0-9-]*")
    residual = [
        row
        for row in rows.values()
        if row["keywords_missing_from_catalog"]
        and any(word.fullmatch(keyword) for keyword in row["keywords_missing_from_catalog"])
    ]
    yield Figure(
        "cobol.rows_with_word_shaped_residual",
        len(residual),
        f"{source} rows missing a keyword that is one bare uppercase word",
    )
    yield Figure(
        "cobol.rows_missing_only_multi_word",
        comparison["totals"]["rows_with_missing_keywords"] - len(residual),
        f"{source} rows_with_missing_keywords less the word-shaped ones",
    )
    yield Figure(
        "cobol.rows_with_more_catalog_forms",
        sum(
            1
            for row in rows.values()
            if row["catalog_form_count"] > row["source_format_count"]
        ),
        f"{source} rows carrying more forms than the publication has formats",
    )

    language = "conformance/0.3/cobol/language.json"
    statements = read(root, language)["procedure_statements"]
    yield Figure(
        "cobol.dispositions",
        sum(1 for row in statements if row.get("disposition")),
        f"{language} procedure_statements[] carrying a disposition",
    )
    yield Figure(
        "cobol.language_forms",
        sum(len(row.get("forms", [])) for row in statements),
        f"{language} sum of len(procedure_statements[].forms)",
    )

    generated = "crates/kernel/mainframe-env-compiler/src/generated/cobol_language.rs"
    text = (root / generated).read_text(encoding="utf-8")
    union = {
        keyword
        for block in re.findall(r"grammar_keywords:\s*&\[(.*?)\]", text, re.S)
        for keyword in re.findall(r'"([^"]+)"', block)
    }
    yield Figure(
        "cobol.form_keyword_union",
        len(union),
        f"{generated} union of grammar_keywords over the 44 rows",
    )


def racf(root: Path) -> Iterator[Figure]:
    """The projection's totals, the nesting histogram, and the dispositions."""
    source = "conformance/0.5/generated/racf-html-syntax-projection.json"
    projection = read(root, source)
    for name, value in sorted(projection["totals"].items()):
        yield Figure(f"racf.{name}", value, f"{source} totals.{name}")

    rows = projection["rows"]
    yield Figure("racf.topics", projection["source"]["topics"], f"{source} source.topics")
    yield Figure(
        "racf.shared_operands",
        projection["totals"]["catalog_operands"] - projection["totals"]["catalog_only"],
        f"{source} totals.catalog_operands less totals.catalog_only",
    )
    for depth in range(1, projection["totals"]["max_nesting_depth"] + 1):
        yield Figure(
            f"racf.rows_at_depth_{depth}",
            sum(1 for row in rows if row["source_nesting_depth"] == depth),
            f"{source} rows[].source_nesting_depth == {depth}",
        )
    yield Figure(
        "racf.families_with_an_alias",
        sum(1 for row in rows if row["source_aliases"]),
        f"{source} rows[] with a non-empty source_aliases",
    )
    yield Figure(
        "racf.families_without_an_alias",
        sum(1 for row in rows if not row["source_aliases"]),
        f"{source} rows[] with an empty source_aliases",
    )
    yield Figure(
        "racf.rows_without_catalog_operands",
        sum(1 for row in rows if not row["catalog_operands"]),
        f"{source} rows[] with an empty catalog_operands",
    )
    synthesised = [row for row in rows if row.get("synthesised_from_functions")]
    yield Figure(
        "racf.racdcert_source_operands",
        len(synthesised[0]["source_operands"]),
        f"{source} rows[RACDCERT].source_operands",
    )
    yield Figure(
        "racf.racdcert_function_topics",
        len(synthesised[0]["topics"]) - 1,
        f"{source} rows[RACDCERT].topics less its own umbrella topic",
    )
    yield Figure(
        "racf.command_topics",
        len(rows),
        f"{source} rows[], one command topic each",
    )

    dispositions = "conformance/0.5/racf/operand-dispositions.json"
    book = read(root, dispositions)
    for name in (
        "baseline_catalog_only_names",
        "baseline_catalog_only_families",
        "applied_names",
        "remaining_catalog_only_names",
        "remaining_catalog_only_families",
        "reviewed_catalog_only_names",
        "implemented_catalog_only_names",
        "opaque_catalog_only_names",
        "unsupported_catalog_only_names",
        "source_only_names",
        "source_only_families",
        "implemented_source_only_names",
        "unsupported_source_only_names",
        "catalog_gap_source_only_names",
        "syntax_only_names",
        "syntax_only_families",
        "implemented_syntax_only_names",
        "unsupported_syntax_only_names",
        "context_only_syntax_only_names",
        "catalog_gap_syntax_only_names",
    ):
        yield Figure(f"racf.{name}", book[name], f"{dispositions} {name}")
    unapplied = [entry for entry in book["dispositions"] if not entry.get("applied")]
    reasons: dict[str, int] = {}
    families: dict[str, int] = {}
    for entry in unapplied:
        reasons[entry["reason"]] = reasons.get(entry["reason"], 0) + 1
        families[entry["keyword"]] = families.get(entry["keyword"], 0) + 1
    for reason, count in sorted(reasons.items(), key=lambda pair: (-pair[1], pair[0])):
        yield Figure(
            f"racf.reason.{slug(reason)}",
            count,
            f"{dispositions} undeferred dispositions with reason `{reason}`",
        )
    for family in ("SETROPTS", "RACMAP", "ALTDSD"):
        yield Figure(
            f"racf.dispositioned.{slug(family)}",
            families.get(family, 0),
            f"{dispositions} unapplied dispositions for {family}",
        )


def ams(root: Path) -> Iterator[Figure]:
    """The parameter/value split, the wide commands, and the emulator's allowlist."""
    source = "conformance/0.6/generated/ams-html-parameter-projection.json"
    projection = read(root, source)
    rows = projection["rows"]
    yield Figure("ams.commands", len(rows), f"{source} rows[]")
    yield Figure(
        "ams.located", sum(1 for row in rows if row["located"]), f"{source} rows[].located"
    )
    yield Figure("ams.topics", projection["source"]["topics"], f"{source} source.topics")
    yield Figure(
        "ams.source_parameters",
        sum(len(row["source_parameters"]) for row in rows),
        f"{source} sum of len(rows[].source_parameters)",
    )
    yield Figure(
        "ams.source_values",
        sum(len(row["source_values"]) for row in rows),
        f"{source} sum of len(rows[].source_values)",
    )
    yield Figure(
        "ams.catalog_parameters",
        sum(len(row["catalog_parameters"]) for row in rows),
        f"{source} sum of len(rows[].catalog_parameters)",
    )
    by_label = {row["label"]: row for row in rows}
    for label in ("ALTER", "ALLOCATE", "DEFINE CLUSTER", "REPRO", "DELETE", "BLDINDEX", "VERIFY"):
        yield Figure(
            f"ams.parameters.{slug(label)}",
            len(by_label[label]["source_parameters"]),
            f"{source} rows[{label}].source_parameters",
        )

    grammar = "conformance/0.6/ams/grammar.json"
    entries = read(root, grammar)
    entries = entries["commands"] if isinstance(entries, dict) and "commands" in entries else entries
    if isinstance(entries, list):
        yield Figure("ams.grammar_entries", len(entries), f"{grammar} command entries")

    service = "crates/apps/mainframe-env-batch/src/service.rs"
    body, line = rust_function(root, service, "ams_operand_allowed")
    definition = body[body.find("const DEFINITION") :]
    base = re.findall(r'"([A-Z][A-Z0-9]*)"', definition[: definition.find("];")])
    yield Figure(
        "ams.allowlist_names",
        len(set(re.findall(r'"([A-Z][A-Z0-9]*)"', body))),
        f"{service} distinct operand names in ams_operand_allowed",
    )
    yield Figure(
        "ams.allowlist_base_names",
        len(set(base)),
        f"{service} ams_operand_allowed const DEFINITION",
    )
    yield Figure("ams.allowlist_line", line, f"{service} line of `fn ams_operand_allowed`")
    _, line = rust_function(root, service, "unimplemented_ams_operand")
    yield Figure("ams.unimplemented_line", line, f"{service} line of `fn unimplemented_ams_operand`")
    text = (root / service).read_text(encoding="utf-8").splitlines()
    caller = next(
        number
        for number, content in enumerate(text, start=1)
        if "unimplemented_ams_operand(command)" in content
    )
    yield Figure("ams.unimplemented_caller_line", caller, f"{service} call of unimplemented_ams_operand")


def jcl(root: Path) -> Iterator[Figure]:
    """The four unit counts the comparison confirms, and the subtree it refuses."""
    source = "conformance/0.7/generated/jcl-html-parameter-projection.json"
    projection = read(root, source)
    yield Figure("jcl.topics", projection["source"]["topics"], f"{source} source.topics")
    total = 0
    for unit in projection["inventory"]["units"]:
        name = slug(unit["unit"])
        for field in ("catalog_count", "source_count", "shared"):
            yield Figure(
                f"jcl.{name}.{field}", unit[field], f"{source} inventory.units[{unit['unit']}].{field}"
            )
        total += unit["catalog_count"]
        yield Figure(
            f"jcl.{name}.chapter_direct_children",
            unit["chapter_direct_children"],
            f"{source} inventory.units[{unit['unit']}].chapter_direct_children",
        )
    for unit in projection["inventory"]["units"]:
        if unit["unit"] != "dd-parameters":
            continue
        nodes = unit["chapter_subtree_nodes"]
        for level, count in sorted(nodes.items()):
            yield Figure(
                f"jcl.dd_subtree.level_{level}",
                count,
                f"{source} inventory.units[dd-parameters].chapter_subtree_nodes[{level}]",
            )
        yield Figure(
            "jcl.dd_subtree.nodes",
            sum(nodes.values()),
            f"{source} inventory.units[dd-parameters].chapter_subtree_nodes summed",
        )
    statements = 0
    for unit in projection["statements"]["units"]:
        yield Figure(
            f"jcl.{slug(unit['unit'])}.count",
            unit["catalog_count"],
            f"{source} statements.units[{unit['unit']}].catalog_count",
        )
        statements += unit["catalog_count"]
    for name, value in sorted(projection["syntax"]["counts"].items()):
        yield Figure(f"jcl.syntax.{name}", value, f"{source} syntax.counts.{name}")
    yield Figure(
        "jcl.catalog_rows",
        total + statements,
        f"{source} inventory and statements catalog_count summed",
    )
    manifest = "conformance/0.7/generated/jcl-topic-manifest.json"
    for role, count in sorted(read(root, manifest)["role_counts"].items()):
        yield Figure(f"jcl.role.{slug(role)}", count, f"{manifest} role_counts.{role}")


def guards(root: Path) -> Iterator[Figure]:
    """Line numbers the record cites, and the size of the two guard rules.

    `check_publication_bytes` is the one the record has been wrong about twice.
    Counting its rules here does not describe them -- the record still has to say
    what they are in prose -- but it does mean a third rule cannot be added
    without the record's sentence going red.
    """
    source = "conformance/tools/fetch_pinned_sources.py"
    yield Figure(
        "tools.unexplained_line",
        python_line(root, source, "UNEXPLAINED ="),
        f"{source} line of `UNEXPLAINED =`",
    )
    resolutions = (root / source).read_text(encoding="utf-8")
    yield Figure(
        "tools.failing_resolutions",
        len(re.search(r"UNEXPLAINED = \((.*?)\)", resolutions).group(1).split(",")),
        f"{source} arity of UNEXPLAINED",
    )
    xtask = "xtask/src/main.rs"
    body, _ = rust_function(root, xtask, "publication_body")
    #: A refusal reason is a phrase, so counting phrase-shaped string literals
    #: survives the reason being bound to a `let` and returned by name -- which
    #: is what a function that grows a third way to recognise the same body
    #: does, and matching on `Some("...")` would have read that as a reason
    #: disappearing.
    yield Figure(
        "guard.refusal_reasons",
        len({found for found in re.findall(r'"([^"\\]+)"', body) if " " in found}),
        f"{xtask} distinct refusal phrases in `publication_body`",
    )
    collect, _ = rust_function(root, xtask, "collect_files")
    yield Figure(
        "guard.skipped_directories",
        len(re.findall(r"root\.join\(", collect)),
        f"{xtask} directories `collect_files` does not walk",
    )


SECTIONS: tuple[tuple[str, Callable[[Path], Iterator[Figure]]], ...] = (
    ("Pinned publications", pins),
    ("Catalog row locators", locators),
    ("Coverage ledger", ledger),
    ("Reader topic manifests", readers),
    ("COBOL", cobol),
    ("RACF", racf),
    ("AMS", ams),
    ("JCL", jcl),
    ("Guards and cited source lines", guards),
)


def figures(root: Path) -> dict[str, Figure]:
    collected: dict[str, Figure] = {}
    for _, section in SECTIONS:
        for figure in section(root):
            if figure.key in collected:
                raise SystemExit(f"duplicate figure key {figure.key}")
            collected[figure.key] = figure
    return collected


def quoted(value: str) -> int:
    """`1,506`, `20` and `nine` are all ways the record writes a number."""
    word = NUMBER_WORDS.get(value.lower())
    return word if word is not None else int(value.replace(",", ""))


def citations(root: Path) -> list[tuple[str, int]]:
    """Every `NNN<!--f:key-->` the record carries, in the order it carries them."""
    text = (root / RECORD).read_text(encoding="utf-8")
    return [(match["key"], quoted(match["value"])) for match in CITATION.finditer(text)]


def check(root: Path) -> tuple[list[Mismatch], list[str], list[str]]:
    """Mismatched citations, citations of no figure, and figures cited nowhere."""
    known = figures(root)
    cited = citations(root)
    wrong = [
        Mismatch(key, value, known[key].value, known[key].source)
        for key, value in cited
        if key in known and value != known[key].value
    ]
    unknown = sorted({key for key, _ in cited if key not in known})
    uncited = sorted(set(known) - {key for key, _ in cited})
    return wrong, unknown, uncited


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=REPOSITORY)
    parser.add_argument("--format", choices=("text", "json"), default="text")
    parser.add_argument(
        "--check",
        action="store_true",
        help=(
            f"diff {RECORD}'s cited figures against the artifacts and exit non-zero when one "
            "disagrees or names no artifact; an uncited figure is listed, not failed"
        ),
    )
    arguments = parser.parse_args(argv)
    root = arguments.root.resolve()

    if arguments.check:
        wrong, unknown, uncited = check(root)
        for mismatch in wrong:
            print(
                f"{RECORD} quotes {mismatch.key} = {mismatch.quoted:,}; "
                f"{mismatch.source} says {mismatch.actual:,}"
            )
        for key in unknown:
            print(f"{RECORD} cites {key}, which no artifact in this tree reports")
        for key in uncited:
            print(f"not cited: {key} is reported by this tool and quoted nowhere in {RECORD}")
        cited = citations(root)
        print(
            f"{len(cited)} citations of {len(set(key for key, _ in cited))} figures in "
            f"{RECORD}; {len(wrong)} disagree with their artifacts, {len(unknown)} name no "
            f"artifact, {len(uncited)} figures are not cited"
        )
        return 1 if wrong or unknown else 0

    known = figures(root)
    if arguments.format == "json":
        print(
            json.dumps(
                {
                    "schema_version": "mainframe-env.probe-figures@1",
                    "coverage_credit": 0,
                    "retained_in_repository": False,
                    "record": RECORD,
                    "figures": {
                        key: {"value": figure.value, "source": figure.source}
                        for key, figure in known.items()
                    },
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0

    for title, section in SECTIONS:
        print(f"\n== {title}")
        for figure in section(root):
            print(figure.line)
    print(f"\n{len(known)} figures, all read from the tree; nothing here was retrieved.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
