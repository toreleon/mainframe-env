#!/usr/bin/env python3
"""Extract reviewed 0.2 official catalog rows from locally pinned IBM sources.

The IBM publications are deliberately not copied into the repository.  This
maintainer tool accepts a directory containing the six pinned PDFs and four
HTML snapshots, verifies every byte digest, and emits deterministic normalized
catalog JSON.  Normal validation consumes only the emitted catalogs and the
checked-in immutable source receipts.

Requires pypdf and beautifulsoup4.  It is not used by the Rust build.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path
from typing import Any, Iterable

from bs4 import BeautifulSoup
from pypdf import PdfReader


SCHEMA = "mainframe-env.official-catalog@1"
PINNED = {
    "cobol.pdf": "8b86cbd2d838d8f460dcbe8d1e74d2266799ce1534f4cfdbc08469c36748e85e",
    "cics.html": "cab6321d495098e133f92adeac154d643b022696967559d6c4485cad48fcb4b2",
    "jcl.pdf": "8bc97add9d69609f303ee96a1fa094d755c8bca254fcf574c15c3380fe877c9b",
    "ams.pdf": "2d0c6bb05bc9ead70575a41bdd786f1ea7ad3f0ed1855ba9a2ec041eaec83c67",
    "racf.pdf": "f4c8860aeb4d00b78f9257b28b2d880bd7571d74e2e00b2b1424b203801d5a46",
    "racroute.html": "83e6eed67f5631268e03ad9459c5ac5fde03bd9d78152eb07403b70af3c4cf78",
    "zosmf.pdf": "2c3dde7bd5f4f7e43b58b6be34997d336af26f9f82975bc8573f26ab723e1ebd",
    "db2.pdf": "34c575200ec4fcbe389364a375ef1ee2161ffdceb8a2dc6de90df0fe9377b212",
    "ims.html": "3a843a493ee4676fe8b0bbf4cf7e9b255ab56ea3dc3e1a7c269c90bd00f9b12e",
    "mq.html": "6b04a5194766eb9da79ddd33d57510db15baf7c085fe686ebdb6e372568bb9dc",
}


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def clean(value: str) -> str:
    return " ".join(value.replace("\xa0", " ").split())


def outline(path: Path) -> list[dict[str, Any]]:
    reader = PdfReader(path)
    rows: list[dict[str, Any]] = []

    def walk(items: Iterable[Any], depth: int = 0, parents: tuple[str, ...] = ()) -> None:
        previous: Any | None = None
        for item in items:
            if isinstance(item, list):
                parent = clean(previous.title) if previous is not None else ""
                walk(item, depth + 1, parents + (parent,))
                continue
            rows.append(
                {
                    "depth": depth,
                    "page": reader.get_destination_page_number(item) + 1,
                    "title": clean(item.title),
                    "parents": parents,
                }
            )
            previous = item

    walk(reader.outline)
    return rows


def html(path: Path) -> BeautifulSoup:
    return BeautifulSoup(path.read_text(encoding="utf-8"), "html.parser")


def normalized_rows(
    baseline: str,
    unit: str,
    values: Iterable[tuple[str, str]],
) -> list[dict[str, Any]]:
    return [
        {
            "id": f"{baseline}:{unit}:{index:04d}",
            "label": label,
            "source_locator": locator,
            "mandatory": True,
        }
        for index, (label, locator) in enumerate(values, 1)
    ]


def unit(
    baseline: str,
    unit_id: str,
    values: Iterable[tuple[str, str]],
    *,
    normalization: str = "normalized",
) -> dict[str, Any]:
    rows = normalized_rows(baseline, unit_id, values)
    require(rows, f"{baseline}/{unit_id} is empty")
    return {
        "id": unit_id,
        "denominator": len(rows),
        "normalization": normalization,
        "rows": rows,
    }


def catalog(baseline: str, subsystem: str, units: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        "schema_version": SCHEMA,
        "baseline_id": baseline,
        "subsystem": subsystem,
        "mandatory_rows": sum(item["denominator"] for item in units),
        "units": units,
    }


def pdf_values(
    rows: list[dict[str, Any]],
    predicate: Any,
    label: Any = lambda row: row["title"],
) -> list[tuple[str, str]]:
    return [
        (clean(label(row)), f"pdf-page:{row['page']};outline:{clean(row['title'])}")
        for row in rows
        if predicate(row)
    ]


def cobol(source: Path) -> dict[str, Any]:
    baseline = "ibm-enterprise-cobol-6.5-2026-05-31"
    rows = outline(source / "cobol.pdf")
    procedure = pdf_values(rows, lambda r: r["depth"] == 2 and 339 <= r["page"] < 533)
    functions = pdf_values(rows, lambda r: r["depth"] == 1 and 555 <= r["page"] < 721)
    directing = pdf_values(rows, lambda r: r["depth"] == 2 and 723 <= r["page"] < 747)
    directives = pdf_values(rows, lambda r: r["depth"] == 2 and 747 <= r["page"] < 767)
    file_clauses = pdf_values(
        rows,
        lambda r: r["depth"] == 2
        and 214 <= r["page"] < 223
        and r["title"] != "FILE SECTION",
    )
    data_clauses = pdf_values(rows, lambda r: r["depth"] == 2 and 226 <= r["page"] < 287)
    expected = [44, 82, 15, 5, 10, 17]
    actual = list(map(len, [procedure, functions, directing, directives, file_clauses, data_clauses]))
    require(actual == expected, f"COBOL denominators changed: {actual}")
    return catalog(
        baseline,
        "cobol",
        [
            unit(baseline, "procedure-statements", procedure),
            unit(baseline, "intrinsic-functions", functions),
            unit(baseline, "compiler-directing-statements", directing),
            unit(baseline, "compiler-directive-groups", directives),
            unit(baseline, "file-description-clauses", file_clauses),
            unit(baseline, "data-description-clauses", data_clauses),
        ],
    )


def table_rows(table: Any) -> list[list[str]]:
    values = []
    for tr in table.select("tr"):
        cells = [clean(cell.get_text(" ", strip=True)) for cell in tr.find_all(["th", "td"], recursive=False)]
        if cells:
            values.append(cells)
    return values


def cics(source: Path) -> dict[str, Any]:
    baseline = "ibm-cics-ts-6x-2026-08-31"
    soup = html(source / "cics.html")

    def values(table_id: str, deduplicate: bool) -> list[tuple[str, str]]:
        table = soup.select_one(f"#{table_id}")
        require(table is not None, f"missing CICS table {table_id}")
        result: list[tuple[str, str]] = []
        seen: set[str] = set()
        for cells in table_rows(table)[1:]:
            require(len(cells) == 3, f"malformed CICS row: {cells}")
            command, code, family = cells
            if deduplicate and command in seen:
                continue
            seen.add(command)
            result.append((command, f"html-table:{table_id};eibfn:{code};family:{family}"))
        return result

    api = values("dfha8mf__eibfn_table_cmds_api", False)
    spi = values("dfha8mf__eibfn_table_cmds_spi", True)
    fepi = values("dfha8mf__eibfn_table_cmds_fepi", False)
    require(list(map(len, [api, spi, fepi])) == [263, 269, 39], "CICS denominators changed")
    return catalog(
        baseline,
        "cics",
        [
            unit(baseline, "api-commands", api),
            unit(baseline, "spi-commands-unique", spi),
            unit(baseline, "fepi-commands", fepi),
        ],
    )


def jcl(source: Path) -> dict[str, Any]:
    baseline = "ibm-zos-3.2-jcl-jes2-2026-06"
    rows = outline(source / "jcl.pdf")
    statements = [
        "JCL command", "COMMAND", "comment", "CNTL", "DD", "delimiter", "ENDCNTL",
        "EXEC", "EXPORT", "IF/THEN/ELSE/ENDIF", "INCLUDE", "JCLLIB", "JOB", "null",
        "OUTPUT JCL", "PEND", "PROC", "SCHEDULE", "SET", "XMIT",
    ]
    statement_values = [(name, f"pdf-page:{47 if index < 10 else 48};table:1") for index, name in enumerate(statements)]
    jes2 = pdf_values(
        rows,
        lambda r: r["depth"] == 1
        and 643 <= r["page"] < 673
        and r["title"] != "Description",
    )

    def parameters(start: int, end: int) -> list[tuple[str, str]]:
        return pdf_values(
            rows,
            lambda r: r["depth"] == 1
            and start <= r["page"] < end
            and re.search(r" parameters?$", r["title"], re.IGNORECASE) is not None,
        )

    dd = parameters(141, 341)
    execute = parameters(368, 407)
    job = parameters(442, 499)
    output = parameters(519, 595)
    actual = list(map(len, [statement_values, jes2, dd, execute, job, output]))
    require(actual == [20, 13, 74, 19, 35, 76], f"JCL denominators changed: {actual}")
    return catalog(
        baseline,
        "jcl-jes2",
        [
            unit(baseline, "jcl-statements", statement_values),
            unit(baseline, "jes2-jecl-statements", jes2),
            unit(baseline, "dd-parameters", dd),
            unit(baseline, "exec-parameters", execute),
            unit(baseline, "job-parameters", job),
            unit(baseline, "output-parameters", output),
        ],
    )


def ams(source: Path) -> dict[str, Any]:
    baseline = "ibm-zos-3.2-dfsms-ams-2026-06"
    rows = outline(source / "ams.pdf")
    commands = pdf_values(
        rows,
        lambda r: r["depth"] == 0 and 63 <= r["page"] < 399 and r["title"].startswith("Chapter "),
        lambda r: r["title"].split(".  ", 1)[-1],
    )
    organizations = [
        ("entry-sequenced data set", "roadmap-normalization:vsam-primary-organizations"),
        ("key-sequenced data set", "roadmap-normalization:vsam-primary-organizations"),
        ("linear data set", "roadmap-normalization:vsam-primary-organizations"),
        ("relative record data set", "roadmap-normalization:vsam-primary-organizations"),
        ("variable-length relative record data set", "roadmap-normalization:vsam-primary-organizations"),
    ]
    require(len(commands) == 31, f"AMS denominator changed: {len(commands)}")
    return catalog(
        baseline,
        "dataset-vsam-ams",
        [
            unit(baseline, "ams-functional-commands", commands),
            unit(baseline, "vsam-primary-organizations", organizations),
        ],
    )


def racf(source: Path) -> dict[str, Any]:
    baseline = "ibm-zos-3.2-racf-saf-2026"
    rows = outline(source / "racf.pdf")
    commands = pdf_values(rows, lambda r: r["depth"] == 1 and 47 <= r["page"] < 675)
    soup = html(source / "racroute.html")
    matrix = next(
        (cells for table in soup.select("table") for cells in table_rows(table) if cells and cells[0] == "RACROUTE parameters"),
        None,
    )
    require(matrix is not None and len(matrix) == 15, "RACROUTE request matrix changed")
    requests = [
        (name.replace(" ", ""), "html-table:keyword-and-parameter-cross-reference")
        for name in matrix[1:]
    ]
    require(list(map(len, [commands, requests])) == [34, 14], "RACF denominators changed")
    return catalog(
        baseline,
        "racf-saf",
        [
            unit(baseline, "racf-command-families", commands),
            unit(baseline, "racroute-request-types", requests),
        ],
    )


def zosmf(source: Path) -> dict[str, Any]:
    baseline = "ibm-zosmf-3.2-2026-07-27"
    rows = outline(source / "zosmf.pdf")
    families = pdf_values(rows, lambda r: r["depth"] == 1 and 76 <= r["page"] < 1255)
    direct = [row for row in rows if row["depth"] == 2 and 76 <= row["page"] < 1255]
    # The reviewed baseline counted direct guide headings before endpoint
    # normalization.  The outline contains 183 direct children and six nested
    # headings promoted by that review.  0.11 will replace this heading-level
    # unit with an endpoint-normalized baseline rather than rewriting it.
    promoted = [row for row in rows if row["depth"] == 3 and 76 <= row["page"] < 1255][:6]
    direct_values = pdf_values(direct + promoted, lambda _row: True)
    require(list(map(len, [families, direct_values])) == [27, 189], "z/OSMF denominators changed")
    return catalog(
        baseline,
        "zosmf",
        [
            unit(baseline, "rest-service-families", families),
            unit(
                baseline,
                "direct-guide-operation-headings",
                direct_values,
                normalization="heading-only-pending-endpoint-normalization",
            ),
        ],
    )


def db2(source: Path) -> dict[str, Any]:
    baseline = "ibm-db2-for-zos-13-2026-08-13"
    rows = outline(source / "db2.pdf")
    statements = pdf_values(rows, lambda r: r["depth"] == 1 and 1169 <= r["page"] < 2291)
    sql_pl = pdf_values(rows, lambda r: r["depth"] == 1 and 2302 <= r["page"] < 2333)
    require(list(map(len, [statements, sql_pl])) == [158, 16], "Db2 denominators changed")
    return catalog(
        baseline,
        "db2",
        [
            unit(baseline, "sql-statements", statements),
            unit(baseline, "sql-pl-statements", sql_pl),
        ],
    )


def ims(source: Path) -> dict[str, Any]:
    baseline = "ibm-ims-15.6-dli-2026-08-31"
    soup = html(source / "ims.html")
    table = soup.select_one("table")
    require(table is not None, "IMS comparison table is missing")
    values = []
    for index, cells in enumerate(table_rows(table)[1:], 1):
        require(len(cells) == 3, f"malformed IMS row: {cells}")
        call, command, _purpose = cells
        values.append((clean(re.sub(r"\s+\d+$", "", call)), f"html-table:comparison;row:{index};command:{clean(re.sub(r'\s+\d+$', '', command))}"))
    require(len(values) == 25, f"IMS denominator changed: {len(values)}")
    return catalog(baseline, "ims", [unit(baseline, "dli-call-families", values)])


def mq(source: Path) -> dict[str, Any]:
    baseline = "ibm-mq-9.4-mqi-2026-08-31"
    soup = html(source / "mq.html")
    seen: set[str] = set()
    values = []
    for anchor in soup.select("article a[href]"):
        title = clean(anchor.get_text(" ", strip=True))
        name = title.split(" - ", 1)[0]
        if not name.startswith("MQ") or name in seen:
            continue
        seen.add(name)
        values.append((name, f"html-link:{anchor.get('href')}"))
    require(len(values) == 26, f"MQ denominator changed: {len(values)}")
    return catalog(baseline, "mq", [unit(baseline, "mqi-calls-unique", values)])


def write_catalog(path: Path, value: dict[str, Any]) -> None:
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    for name, expected in PINNED.items():
        path = args.source / name
        require(path.is_file(), f"missing pinned source {path}")
        require(digest(path) == expected, f"digest mismatch for {path}")
    args.output.mkdir(parents=True, exist_ok=True)
    catalogs = [
        cobol(args.source), cics(args.source), jcl(args.source), ams(args.source), racf(args.source),
        zosmf(args.source), db2(args.source), ims(args.source), mq(args.source),
    ]
    for value in catalogs:
        write_catalog(args.output / f"{value['subsystem']}.json", value)


if __name__ == "__main__":
    main()
