#!/usr/bin/env python3
"""Project the Access Method Services parameter inventory from its DITA lists.

Each `Required Parameters` / `Optional Parameters` topic is a definition list.
A parameter is a top-level `dt`; the values it accepts are `dt` entries of a
`dl` nested inside its own `dd`:

    <dt>ERASE|NOERASE</dt>
    <dd><dl>
      <dt>ERASE</dt><dd>...</dd>
      <dt>NOERASE</dt><dd>...</dd>
    </dl></dd>

That nesting is what the PDF cannot supply. Reading the PDF, a parameter is a
flush-left heading, so a value the typesetter did not indent — `SORTMESSAGELEVEL`'s
`ALL`, `CRITICAL` and `NONE` — is indistinguishable from a parameter. Here
depth decides, and the alternation in a `A|B` heading is split into its names.

The emitted projection is a review input. It grants no coverage credit, is not
a normative catalog, and IBM publication bytes are never written to the
repository.
"""

from __future__ import annotations

import argparse
import json
import re
from html.parser import HTMLParser
from pathlib import Path
from typing import Any, Iterable

NAME = re.compile(r"^[A-Z][A-Z0-9]+(?:-[A-Z0-9]+)*$")
TAG = re.compile(r"<[^>]+>")


def clean(value: str) -> str:
    return " ".join(value.replace("\xa0", " ").split())


class Definitions(HTMLParser):
    """Collect `dt` terms with the `dl` nesting depth they sit at."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self._depth = 0
        self._capture: list[str] | None = None
        self._term_depth = 0
        self.terms: list[tuple[int, str]] = []

    def handle_starttag(self, tag: str, attrs: Any) -> None:
        if tag == "dl":
            self._depth += 1
        elif tag == "dt":
            self._capture = []
            self._term_depth = self._depth

    def handle_endtag(self, tag: str) -> None:
        if tag == "dl":
            self._depth = max(0, self._depth - 1)
        elif tag == "dt" and self._capture is not None:
            text = clean("".join(self._capture))
            if text:
                self.terms.append((self._term_depth, text))
            self._capture = None

    def handle_data(self, data: str) -> None:
        if self._capture is not None:
            self._capture.append(data)


ARGUMENT = re.compile(r"\([^()]*\)")


def names(term: str) -> list[str]:
    """Split a term into the parameter names it introduces.

    Terms carry their argument (`CATALOG(catname)`), their alternation
    (`ERASE|NOERASE`) and sometimes the reference's own brackets. Arguments go
    first: a term reads `INFILE(ddname)|INDATASET(entryname)`, so splitting on
    the alternation before removing the arguments would lose the second name,
    and the arguments contain alternations of their own.
    """
    head = term
    while True:
        stripped = ARGUMENT.sub("", head)
        if stripped == head:
            break
        head = stripped
    found: list[str] = []
    for part in re.split(r"[|/]", head):
        candidate = part.strip().strip("[]{} \t")
        if NAME.match(candidate) and candidate not in found:
            found.append(candidate)
    return found


def parameters(documents: Iterable[str]) -> tuple[list[str], list[str]]:
    """Return top-level parameter names and the value names nested under them."""
    top: list[str] = []
    nested: list[str] = []
    for document in documents:
        parser = Definitions()
        parser.feed(document)
        if not parser.terms:
            continue
        outermost = min(depth for depth, _ in parser.terms)
        for depth, term in parser.terms:
            target = top if depth == outermost else nested
            for name in names(term):
                if name not in target:
                    target.append(name)
    # A name introduced as a value elsewhere is still a parameter if the
    # reference gives it a top-level entry.
    nested = [name for name in nested if name not in top]
    return top, nested


def project(command: dict[str, Any], entry: dict[str, Any], topics: Path) -> dict[str, Any]:
    documents = [
        (topics / topic["file"]).read_text(encoding="utf-8") for topic in entry["topics"]
    ]
    top, nested = parameters(documents)
    return {
        "id": command["id"],
        "label": command["label"],
        "located": True,
        "topics": [topic["label"] for topic in entry["topics"]],
        "source_parameters": top,
        "source_values": nested,
        "catalog_parameters": command.get("parameters", []),
    }


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
    located = {entry["id"]: entry for entry in manifest["commands"] if entry["located"]}

    rows: list[dict[str, Any]] = []
    for command in catalog["commands"]:
        entry = located.get(command["id"])
        if entry is None:
            rows.append(
                {
                    "id": command["id"],
                    "label": command["label"],
                    "located": False,
                    "catalog_parameters": command.get("parameters", []),
                }
            )
            continue
        rows.append(project(command, entry, args.topics))

    output = {
        "schema_version": "mainframe-env.ams-html-parameter-projection@1",
        "coverage_credit": 0,
        "source": {
            "product": manifest["product"],
            "book": manifest["book"],
            "topics": sum(len(entry["topics"]) for entry in located.values()),
            "retained_in_repository": False,
        },
        "rows": rows,
    }
    args.output.write_text(
        json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(
        f"commands={len(rows)} located={len(located)} "
        f"parameters={sum(len(row.get('source_parameters', [])) for row in rows)} "
        f"values={sum(len(row.get('source_values', [])) for row in rows)}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
