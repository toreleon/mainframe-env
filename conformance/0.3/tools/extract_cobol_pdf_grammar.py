#!/usr/bin/env python3
"""Project COBOL Language Reference syntax diagrams into a review grammar.

The IBM Enterprise COBOL Language Reference draws every railroad diagram as
inline vector art: the rails are stroked paths and the labels are 8pt text runs
whose font style separates keywords from operands.  This tool replays the page
content stream, recovers device coordinates for both, and reconstructs the
main line, its alternatives, and its bypassed (optional) segments.

The emitted projection is a review input.  It grants no coverage credit, is not
a normative catalog, and IBM publication bytes are never written to the
repository.  Requires pypdf.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path
from typing import Any, Iterable

from pypdf import PdfReader
from pypdf.generic import ContentStream

DIAGRAM_SIZE = 8.0
SIZE_TOLERANCE = 0.6
ROW_TOLERANCE = 2.0
GAP_TOLERANCE = 1.5
BAND_GAP = 60.0
RAIL_LINE_GAP = 25.0
FORMAT_TITLE = re.compile(r"^Format\s+(\d+)\s*:\s*(.*)$")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def digest(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def multiply(a: list[float], b: list[float]) -> list[float]:
    return [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]


def apply(matrix: list[float], x: float, y: float) -> tuple[float, float]:
    return (
        matrix[0] * x + matrix[2] * y + matrix[4],
        matrix[1] * x + matrix[3] * y + matrix[5],
    )


BF_CHAR = re.compile(rb"beginbfchar(.*?)endbfchar", re.DOTALL)
BF_RANGE = re.compile(rb"beginbfrange(.*?)endbfrange", re.DOTALL)
HEX = re.compile(rb"<([0-9A-Fa-f]+)>")
RANGE_ROW = re.compile(rb"<([0-9A-Fa-f]+)>\s*<([0-9A-Fa-f]+)>\s*(<[0-9A-Fa-f]+>|\[[^\]]*\])", re.DOTALL)


def utf16(raw: bytes) -> str:
    return bytes.fromhex(raw.decode("ascii")).decode("utf-16-be", "replace")


def to_unicode(font: Any) -> dict[int, str]:
    """Parse a /ToUnicode CMap into a code point table."""
    if "/ToUnicode" not in font:
        return {}
    data = font["/ToUnicode"].get_object().get_data()
    table: dict[int, str] = {}
    for block in BF_CHAR.findall(data):
        pairs = HEX.findall(block)
        for index in range(0, len(pairs) - 1, 2):
            table[int(pairs[index], 16)] = utf16(pairs[index + 1])
    for block in BF_RANGE.findall(data):
        for low, high, target in RANGE_ROW.findall(block):
            start, end = int(low, 16), int(high, 16)
            if target.startswith(b"["):
                for offset, item in enumerate(HEX.findall(target)):
                    table[start + offset] = utf16(item)
            else:
                base = utf16(HEX.findall(target)[0])
                for offset in range(end - start + 1):
                    table[start + offset] = chr(ord(base[0]) + offset) if base else ""
    return table


class Page:
    """Replays one page content stream into positioned text and rails."""

    def __init__(self, page: Any, reader: PdfReader) -> None:
        self.tokens: list[dict[str, Any]] = []
        self.segments: list[dict[str, float]] = []
        self.fonts = self._fonts(page)
        self._run(ContentStream(page.get_contents(), reader))

    @staticmethod
    def _fonts(page: Any) -> dict[str, dict[str, Any]]:
        resources = page.get("/Resources")
        table: dict[str, dict[str, Any]] = {}
        if not resources:
            return table
        fonts = resources.get("/Font")
        if not fonts:
            return table
        for key in fonts.keys():
            font = fonts[key].get_object()
            base = str(font.get("/BaseFont", "")).split("+")[-1]
            table[str(key)] = {
                "base": base,
                "italic": "Italic" in base,
                "bold": "Bold" in base,
                "width": 2 if str(font.get("/Subtype")) == "/Type0" else 1,
                "cmap": to_unicode(font),
            }
        return table

    @staticmethod
    def _text(raw: Any, font: dict[str, Any]) -> str:
        if not isinstance(raw, bytes):
            return str(raw)
        cmap = font.get("cmap") or {}
        if not cmap:
            return raw.decode("latin-1", "replace")
        width = font.get("width", 1)
        out: list[str] = []
        for index in range(0, len(raw) - width + 1, width):
            code = int.from_bytes(raw[index : index + width], "big")
            out.append(cmap.get(code, ""))
        return "".join(out)

    def _run(self, stream: ContentStream) -> None:
        ctm: list[float] = [1, 0, 0, 1, 0, 0]
        stack: list[list[float]] = []
        text: list[float] = [1, 0, 0, 1, 0, 0]
        line: list[float] = [1, 0, 0, 1, 0, 0]
        leading = 0.0
        size = 0.0
        font = {"base": "", "italic": False, "bold": False}
        pen = (0.0, 0.0)

        for operands, operator in stream.operations:
            if operator == b"q":
                stack.append(list(ctm))
            elif operator == b"Q":
                ctm = stack.pop() if stack else [1, 0, 0, 1, 0, 0]
            elif operator == b"cm":
                ctm = multiply([float(v) for v in operands], ctm)
            elif operator == b"BT":
                text = [1, 0, 0, 1, 0, 0]
                line = list(text)
            elif operator == b"Tm":
                text = [float(v) for v in operands]
                line = list(text)
            elif operator == b"TL":
                leading = float(operands[0])
            elif operator in (b"Td", b"TD"):
                if operator == b"TD":
                    leading = -float(operands[1])
                line = multiply(
                    [1, 0, 0, 1, float(operands[0]), float(operands[1])], line
                )
                text = list(line)
            elif operator == b"T*":
                line = multiply([1, 0, 0, 1, 0, -leading], line)
                text = list(line)
            elif operator == b"Tf":
                font = self.fonts.get(str(operands[0]), font)
                size = float(operands[1])
            elif operator in (b"Tj", b"TJ"):
                parts = operands[0] if operator == b"TJ" else [operands[0]]
                value = "".join(
                    self._text(part.original_bytes if hasattr(part, "original_bytes") else part, font)
                    for part in parts
                    if not isinstance(part, (int, float))
                )
                if value.strip():
                    matrix = multiply(text, ctm)
                    x, y = apply(matrix, 0.0, 0.0)
                    width = abs(matrix[0]) * size * 0.5 * len(value)
                    self.tokens.append(
                        {
                            "text": value,
                            "x": x,
                            "y": y,
                            "x_end": x + width,
                            "size": abs(matrix[0]) * size,
                            "italic": font["italic"],
                            "bold": font["bold"],
                        }
                    )
            elif operator == b"m":
                pen = apply(ctm, float(operands[0]), float(operands[1]))
            elif operator == b"l":
                point = apply(ctm, float(operands[0]), float(operands[1]))
                self.segments.append(
                    {"x0": pen[0], "y0": pen[1], "x1": point[0], "y1": point[1]}
                )
                pen = point


def diagram_tokens(page: Page) -> list[dict[str, Any]]:
    return [
        token
        for token in page.tokens
        if abs(token["size"] - DIAGRAM_SIZE) <= SIZE_TOLERANCE
    ]


def cluster(values: Iterable[float], tolerance: float) -> list[list[float]]:
    groups: list[list[float]] = []
    for value in sorted(values):
        if groups and value - groups[-1][-1] <= tolerance:
            groups[-1].append(value)
        else:
            groups.append([value])
    return groups


def horizontal(rails: list[dict[str, float]], low: float, high: float) -> list[dict[str, float]]:
    return [
        rail
        for rail in rails
        if abs(rail["y0"] - rail["y1"]) < GAP_TOLERANCE
        and abs(rail["x1"] - rail["x0"]) > DIAGRAM_SIZE
        and low <= rail["y0"] <= high
    ]


def split_diagrams(
    tokens: list[dict[str, Any]], rails: list[dict[str, float]]
) -> list[list[dict[str, Any]]]:
    """A diagram is one vertically contiguous band of railed 8pt runs.

    Code samples in the reference are also set at 8pt, so a band only counts as
    a diagram when the page also strokes horizontal rails across it.
    """
    if not tokens:
        return []
    diagrams: list[list[dict[str, Any]]] = []
    for band in cluster({token["y"] for token in tokens}, BAND_GAP):
        low, high = min(band), max(band)
        members = [token for token in tokens if low <= token["y"] <= high]
        if not members:
            continue
        if not horizontal(rails, low - DIAGRAM_SIZE, high + DIAGRAM_SIZE):
            continue
        diagrams.append(members)
    diagrams.sort(key=lambda group: -max(token["y"] for token in group))
    return diagrams


def rows_of(tokens: list[dict[str, Any]]) -> list[list[dict[str, Any]]]:
    rows: list[list[dict[str, Any]]] = []
    for band in cluster({token["y"] for token in tokens}, ROW_TOLERANCE):
        low, high = min(band), max(band)
        members = [token for token in tokens if low <= token["y"] <= high]
        members.sort(key=lambda token: token["x"])
        rows.append(members)
    rows.sort(key=lambda row: -row[0]["y"])
    return rows


def node(token: dict[str, Any]) -> dict[str, Any]:
    value = token["text"].strip()
    kind = "operand" if token["italic"] else "keyword"
    return {"kind": kind, "value": value if kind == "operand" else value.upper()}


def bypassed(main: list[dict[str, Any]], rails: list[dict[str, float]], y: float) -> bool:
    """True when a horizontal rail spans the main line at this row's height."""
    return any(
        abs(rail["y0"] - rail["y1"]) < GAP_TOLERANCE and rail["y0"] > y + ROW_TOLERANCE
        for rail in rails
    )


def rail_lines(rows: list[list[dict[str, Any]]]) -> list[list[list[dict[str, Any]]]]:
    """Split rows into wrapped rail lines; each starts a new main line."""
    groups: list[list[list[dict[str, Any]]]] = []
    for row in rows:
        if groups and groups[-1][-1][0]["y"] - row[0]["y"] <= RAIL_LINE_GAP:
            groups[-1].append(row)
        else:
            groups.append([row])
    return groups


def attach(main: list[dict[str, Any]], token: dict[str, Any]) -> dict[str, Any]:
    anchor = next(
        (
            candidate
            for candidate in main
            if candidate["x"] - DIAGRAM_SIZE
            <= token["x"]
            <= candidate["x_end"] + DIAGRAM_SIZE
        ),
        None,
    )
    entry = node(token)
    entry["relation"] = "alternative" if anchor is not None else "optional"
    if anchor is not None:
        entry["alternative_to"] = anchor["text"].strip()
    return entry


def build(tokens: list[dict[str, Any]], rails: list[dict[str, float]]) -> dict[str, Any]:
    rows = rows_of(tokens)
    require(rows, "diagram has no rows")
    lines: list[dict[str, Any]] = []
    items: list[dict[str, Any]] = []
    branches: list[dict[str, Any]] = []
    for index, group in enumerate(rail_lines(rows)):
        main = group[0]
        main_nodes = [node(token) for token in main]
        line_branches = [attach(main, token) for row in group[1:] for token in row]
        for entry in line_branches:
            entry["rail_line"] = index
        lines.append({"index": index, "main": main_nodes, "branches": line_branches})
        items.extend(main_nodes)
        branches.extend(line_branches)
    return {
        "rail_lines": lines,
        "main_line": items,
        "branches": branches,
        "row_count": len(rows),
        "rail_line_count": len(lines),
    }


def sections(reader: PdfReader) -> list[dict[str, Any]]:
    found: list[dict[str, Any]] = []

    def walk(items: Any, depth: int = 0) -> None:
        for item in items:
            if isinstance(item, list):
                walk(item, depth + 1)
                continue
            try:
                page = reader.get_destination_page_number(item)
            except Exception:  # noqa: BLE001 - malformed outline entries are skipped
                continue
            found.append({"title": str(item.title).strip(), "page": page, "depth": depth})

    walk(reader.outline)
    return found


def statement_pages(entries: list[dict[str, Any]], title: str) -> tuple[int, int, str]:
    """Return the inclusive page window and the title that terminates it."""
    ordered = sorted(entries, key=lambda entry: entry["page"])
    for index, entry in enumerate(ordered):
        if entry["title"] != title:
            continue
        start = entry["page"]
        for follower in ordered[index + 1 :]:
            if follower["depth"] <= entry["depth"] and follower["page"] >= start:
                return start, follower["page"], follower["title"]
        return start, min(start + 2, start + 2), ""
    raise ValueError(f"outline entry not found: {title!r}")


def heading_y(page: Page, title: str, default: float) -> float:
    """The y of a section heading on this page, or `default` when absent."""
    if not title:
        return default
    for token in page.tokens:
        if token["size"] > DIAGRAM_SIZE + SIZE_TOLERANCE and token["text"].strip().startswith(
            title[:28]
        ):
            return token["y"]
    return default


def is_diagram(tokens: list[dict[str, Any]]) -> bool:
    """Reject railed figures that carry no command vocabulary."""
    if len(tokens) < 2:
        return False
    return any(
        not token["italic"] and re.match(r"^[A-Za-z]", token["text"].strip())
        for token in tokens
    )


def extract(reader: PdfReader, title: str, entries: list[dict[str, Any]]) -> dict[str, Any]:
    start, end, terminator = statement_pages(entries, title)
    forms: list[dict[str, Any]] = []
    labels: list[str] = []
    for number in range(start, min(end + 1, len(reader.pages))):
        page = Page(reader.pages[number], reader)
        floor = heading_y(page, terminator, float("-inf")) if number == end else float("-inf")
        ceiling = heading_y(page, title, float("inf")) if number == start else float("inf")
        for heading in page.tokens:
            match = FORMAT_TITLE.match(heading["text"].strip())
            if match and floor < heading["y"] < ceiling:
                labels.append(heading["text"].strip())
        for group in split_diagrams(diagram_tokens(page), page.segments):
            if max(token["y"] for token in group) < floor:
                continue
            if min(token["y"] for token in group) > ceiling:
                continue
            if not is_diagram(group):
                continue
            low = min(token["y"] for token in group) - DIAGRAM_SIZE * 3
            high = max(token["y"] for token in group) + DIAGRAM_SIZE * 3
            rails = [
                rail for rail in page.segments if low <= rail["y0"] <= high
            ]
            form = build(group, rails)
            form["page"] = number + 1
            forms.append(form)
    return {"title": title, "start_page": start + 1, "forms": forms, "format_titles": labels}


def linear(form: dict[str, Any]) -> str:
    parts = [item["value"] for item in form["main_line"]]
    return " ".join(parts)


def parse_args(argv: Iterable[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--pdf", type=Path, required=True)
    parser.add_argument("--catalog", type=Path, required=True)
    parser.add_argument("--unit", default="procedure_statements")
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args(list(argv) if argv is not None else None)


def main(argv: Iterable[str] | None = None) -> int:
    args = parse_args(argv)
    reader = PdfReader(str(args.pdf))
    entries = sections(reader)
    catalog = json.loads(args.catalog.read_text(encoding="utf-8"))
    unit = catalog[args.unit]
    projected: list[dict[str, Any]] = []
    missing: list[str] = []
    for row in unit:
        try:
            projection = extract(reader, row["label"], entries)
        except ValueError:
            missing.append(row["label"])
            continue
        projection["id"] = row["id"]
        projection["row_id"] = row["row_id"]
        projection["catalog_forms"] = row.get("forms", [])
        projection["linear_forms"] = [linear(form) for form in projection["forms"]]
        projected.append(projection)
    output = {
        "schema_version": "mainframe-env.cobol-pdf-grammar-projection@1",
        "coverage_credit": 0,
        "source": {
            "path": args.pdf.name,
            "sha256": digest(args.pdf),
            "pages": len(reader.pages),
            "retained_in_repository": False,
        },
        "unit": args.unit,
        "rows": projected,
        "unmatched_outline_titles": missing,
    }
    args.output.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(
        f"rows={len(projected)} unmatched={len(missing)} "
        f"forms={sum(len(row['forms']) for row in projected)}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
