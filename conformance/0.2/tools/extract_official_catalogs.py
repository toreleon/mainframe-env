#!/usr/bin/env python3
"""Extract reviewed 0.2 official catalog rows from pinned IBM documentation topics.

The IBM publications are deliberately not copied into the repository.  This
maintainer tool accepts a cache directory holding the product tables of contents
and the handful of topic fragments the readers need, verifies every byte digest,
and emits deterministic normalized catalog JSON.  Normal validation consumes only
the emitted catalogs and the checked-in immutable source receipts.

PDF is retired as a source.  Every row is now located by a documentation topic,
because the PDF outline readers inferred structure from page geometry and were
provably wrong about it.  A row's topic is decided by WHERE the topic sits in the
table of contents -- the ordered children of a named anchor -- never by matching
its heading text.  That is what keeps the three z/OSMF rows all labelled "Error
reporting categories" attached to the three different topics they came from: they
are the 75th, 141st and 153rd child of their subtree, and nothing about them is
compared as text.  The twenty rows that are body rows of a table rather than
topics of their own are addressed the same way, by the table's id and the row's
ordinal within it, so the locator this tool emits identifies every row without
appealing to the heading beside it.

Two retrieval endpoints back the cache:

    toc      https://www.ibm.com/docs/api/v1/toc/<PRODUCT>?lang=en
    content  https://www.ibm.com/docs/api/v1/content/<TOPIC_PATH>?parsebody=true&lang=en

`parsebody=true` is load-bearing -- dropping it changes what the origin emits --
and curl's DEFAULT User-Agent is load-bearing too, because Akamai answers 403 to a
browser User-Agent and to urllib.  `--fetch` populates the cache through curl and
is the only code path that talks to ibm.com; extraction itself is offline.

Requires beautifulsoup4.  It is not used by the Rust build.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
from dataclasses import dataclass, field
from pathlib import Path, PurePosixPath
from typing import Any, Iterable

from bs4 import BeautifulSoup


SCHEMA = "mainframe-env.official-catalog@1"

TOC_URL = "https://www.ibm.com/docs/api/v1/toc/{product}?lang=en"
CONTENT_URL = "https://www.ibm.com/docs/api/v1/content/{topic_path}?parsebody=true&lang=en"

# Products whose table of contents drives a selector.  cics, ims and mq are
# single-topic baselines reached by topic path, so they need no table of contents.
TOC_PRODUCTS = ("SS6SG3_6.5", "SSEPEK_13.0.0", "SSLTBW_3.2.0")

# Cache-relative path -> sha256 of the pinned bytes.  Verified before anything is
# read.  These are documentation topics; no PDF is pinned and none is read.
PINNED = {
    "toc/SS6SG3_6.5.json": "c3527d5e084e9749bef54ddf7bfdb1ecbbed94dd686c724417a4a3b669f671ce",
    "toc/SSEPEK_13.0.0.json": "b083904697db57a7e746e79491f73c22c7e58c267dd83ea6e4ce39eb401071fb",
    "toc/SSLTBW_3.2.0.json": "477a3de1989e3cb185f2dacdc7cea9bab8304ef148e7c9e5e8be55fe3f77dc2b",
    # readers that parse a topic body rather than the table of contents
    "content/SSJL4D_6.x/reference-diagnostics/eib/dfha8mf.html":
        "78f90b09987b1a56da7cd9f0a2a36fa43a549966fa7dbeff2ad9608106ef7c25",
    "content/SSEPH2_15.6.0/com.ibm.ims156.doc.apg/ims_comparingexecdlicmdsanddlicalls.htm":
        "ce4a179eac0f18ed4ff71bed9ca576b31bcbbdb076d305dbcbf942e26ce13e30",
    "content/SSFKSJ_9.4.0/refdev/q101650_.html":
        "24025a9f40dfc613b8243fef16f902a94794a9fbe517730ae1b6c489a338009f",
    "content/SSLTBW_3.2.0/com.ibm.zos.v3r2.ichc600/racri.htm":
        "acebd6ebc34ffd70843db61620351ae35df699c8e29bd3da486a409d9854db3a",
    "content/SSLTBW_3.2.0/com.ibm.zos.v3r2.ieab600/iea3b6_JCL_statements.htm":
        "02e3fec6baf442675ebadfcadfda9587e5eda6a4de3aec6ff19b73bef9c4c820",
    # Db2 SQL PL, whose reviewed labels are the topic titles rather than the
    # shorter table-of-contents labels
    "content/SSEPEK_13.0.0/sqlref/src/tpc/db2z_assignmentstatement4nativesqlplv11.html":
        "3b2cd66535bc1174a6f7a3d3cad95d9dc12b614a6ef7166bb441c845106fdd0d",
    "content/SSEPEK_13.0.0/sqlref/src/tpc/db2z_callstatement4nativesqlpl.html":
        "3ca7b8616635efdb265f87264e8dc52ba6837e1e4735216d7eed411ac32ca6de",
    "content/SSEPEK_13.0.0/sqlref/src/tpc/db2z_casestatement4nativesqlpl.html":
        "6b9be9112701d81d251ad3778e6100c63bc6e68a85ec96f65e2469f992c95d2a",
    "content/SSEPEK_13.0.0/sqlref/src/tpc/db2z_compoundstatement4nativesqlpl.html":
        "6edc047b9b2d255e06bc56416b725cf45bf370db7562ed1e4ba985d2cdfe23b8",
    "content/SSEPEK_13.0.0/sqlref/src/tpc/db2z_forstatement4nativesqlpl.html":
        "1b60e185eb751a5399b4344cbc984d9714ed2f91106f758fd491dd70f16fc5a1",
    "content/SSEPEK_13.0.0/sqlref/src/tpc/db2z_getdiagnosticsstatement4nativesqlpl.html":
        "3539f87d1d36083d6511b5dce055f33dfab82916474bb31542d1cd5e8e87842b",
    "content/SSEPEK_13.0.0/sqlref/src/tpc/db2z_gotostatement4nativesqlpl.html":
        "8e0bc8faaeca87da6de0f9c18a87e396c20b14e06a2c40c39392c994055d6685",
    "content/SSEPEK_13.0.0/sqlref/src/tpc/db2z_ifstatement4nativesqlpl.html":
        "99b02dd670db03b6a6793edc2f89ad73e1caed03098253334584b67452c73b9a",
    "content/SSEPEK_13.0.0/sqlref/src/tpc/db2z_iteratestatement4nativesqlpl.html":
        "17a75ac6825e76c600bb00aecafd7f783120862c19dfa6da0315bd70a1bbea0c",
    "content/SSEPEK_13.0.0/sqlref/src/tpc/db2z_leavestatement4nativesqlpl.html":
        "c082dde834881e536ce2cef5a780cc4686361a249b02ea16136f452c2243c7ad",
    "content/SSEPEK_13.0.0/sqlref/src/tpc/db2z_loopstatement4nativesqlpl.html":
        "fab677c6ba2aadb6ffac843ad2b83da333c4f5ea3920942549f2949023b04074",
    "content/SSEPEK_13.0.0/sqlref/src/tpc/db2z_repeatstatement4nativesqlpl.html":
        "00949e4d5e7e71697d642f0a3ebde3cfd2a142b957632249b554e886ab1ec7b6",
    "content/SSEPEK_13.0.0/sqlref/src/tpc/db2z_resignalstatement4nativesqlpl.html":
        "46d816220949258524a137969294e02fe4f6256b3893152ac371b07d31f72043",
    "content/SSEPEK_13.0.0/sqlref/src/tpc/db2z_returnstatement4nativesqlpl.html":
        "da2273d7cd5a903757aebf0396ff2a891c658802ae3e0c4cc0b2aacf8333c63e",
    "content/SSEPEK_13.0.0/sqlref/src/tpc/db2z_signalstatement4nativesqlpl.html":
        "03bac94d3cccb493e0b245ea16d869a2f2c3223ae40b95e43f685547550217d1",
    "content/SSEPEK_13.0.0/sqlref/src/tpc/db2z_whilestatement4nativesqlpl.html":
        "f7b32a5dacb18880bc6b646b89ad9abfd2263766a097ddf2c7c961d5b20ac13a",
}


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def clean(value: str) -> str:
    return " ".join(value.replace("\xa0", " ").split())


def topic_path(href: str) -> str:
    """Content path for a table-of-contents href.

    `?pos=N` is a navigation-only disambiguator for a topic that is cross-listed
    in the tree.  The content endpoint 302s on it and the redirect drops
    parsebody=true, so the query is stripped before the topic is addressed.
    """
    return href.split("?", 1)[0]


# --------------------------------------------------------------------------
# retrieval
# --------------------------------------------------------------------------


def fetch(url: str, destination: Path) -> None:
    """Fetch one URL into the cache with curl's DEFAULT User-Agent.

    Akamai rejects a browser User-Agent and rejects urllib; plain curl is what
    gets through.  Nothing else in this tool touches the network.
    """
    destination.parent.mkdir(parents=True, exist_ok=True)
    result = subprocess.run(
        ["curl", "-sS", "--fail", "--compressed", "-o", str(destination), url],
        check=False,
    )
    require(result.returncode == 0, f"fetch failed for {url}")


def populate(cache: Path) -> None:
    for product in TOC_PRODUCTS:
        fetch(TOC_URL.format(product=product), cache / "toc" / f"{product}.json")
    for name in PINNED:
        if not name.startswith("content/"):
            continue
        path = name[len("content/"):]
        fetch(CONTENT_URL.format(topic_path=path), cache / name)


def verify_pinned(cache: Path) -> None:
    for name, expected in PINNED.items():
        path = cache / name
        require(path.is_file(), f"missing pinned source {path}")
        require(digest(path) == expected, f"digest mismatch for {path}")


# --------------------------------------------------------------------------
# table of contents
# --------------------------------------------------------------------------


class Toc:
    """One product table of contents, addressed by href."""

    def __init__(self, path: Path) -> None:
        self.root = json.loads(path.read_text(encoding="utf-8"))["toc"]

    def anchor(self, href: str) -> dict[str, Any]:
        hits = [node for node in self._walk(self.root) if node.get("href") == href]
        require(len(hits) == 1, f"anchor href {href} matched {len(hits)} nodes, expected 1")
        return hits[0]

    @classmethod
    def _walk(cls, node: dict[str, Any]) -> Iterable[dict[str, Any]]:
        yield node
        for child in node.get("topics") or ():
            yield from cls._walk(child)

    @staticmethod
    def descend(node: dict[str, Any], depth: int) -> list[dict[str, Any]]:
        """Nodes exactly `depth` levels below `node`, in document order."""
        if depth == 0:
            return [node]
        found: list[dict[str, Any]] = []
        for child in node.get("topics") or ():
            found.extend(Toc.descend(child, depth - 1))
        return found


@dataclass(frozen=True)
class Appendix:
    """A second named anchor and the exact children taken from it, in order.

    One reviewed unit ends with headings the review promoted from deeper in the
    tree than the rest of the unit.  They are declared the same way everything
    else here is -- one named parent href, then the child hrefs by name with the
    label the tree carries beside each -- so that the unit's tail says which
    topics it is and stops if the publication no longer agrees.  It replaces a
    positional slice of the whole subtree, which named none of that and would
    have taken six different topics without complaint.
    """

    anchor: str
    take: tuple[tuple[str, str], ...] = ()

    def select(self, toc: Toc) -> list[dict[str, Any]]:
        children = {
            node.get("href"): node for node in (toc.anchor(self.anchor).get("topics") or ())
        }
        chosen: list[dict[str, Any]] = []
        for href, label in self.take:
            require(href in children, f"appended {href} is not a child of {self.anchor}")
            found = clean(children[href].get("label", ""))
            require(
                found == clean(label),
                f"appended {href} is labelled {found!r}, declared {clean(label)!r}",
            )
            chosen.append(children[href])
        return chosen


@dataclass(frozen=True)
class Selector:
    """An ordered table-of-contents subtree, minus a named exclusion set.

    No field of this selector is a heading to be matched.  `anchor`, every
    `exclude` entry and every `append` entry is an exact href; the labels beside
    those hrefs are carried only so the declaration is readable, and both must
    agree or the tool refuses to run.  Row order is document order, so a row's
    identity is its position, which is the one thing a heading-driven rewrite
    cannot forge.
    """

    product: str
    anchor: str
    depth: int = 1
    exclude: tuple[tuple[str, str], ...] = ()
    # Headings the review promoted from deeper in the tree, appended after the
    # whole depth-`depth` sequence.  Named, never sliced.
    append: Appendix | None = None
    # "toc-label": the label the table of contents carries.
    # "topic-h1":  the topic's own title, for units whose reviewed labels are the
    #              full topic titles rather than the shorter navigation labels.
    label_source: str = "toc-label"
    # The reviewed labels of two units carry the PDF's chapter numbering, which
    # no HTML source states.  The numbering is contiguous over the unit, so it is
    # declared once as a starting chapter rather than matched per row.
    chapter_start: int | None = None

    def select(self, toc: Toc) -> list[dict[str, Any]]:
        anchor = toc.anchor(self.anchor)
        nodes = Toc.descend(anchor, self.depth)
        present = {node.get("href"): clean(node.get("label", "")) for node in nodes}
        for href, label in self.exclude:
            require(href in present, f"exclusion {href} is not a child of {self.anchor}")
            require(
                present[href] == clean(label),
                f"exclusion {href} is labelled {present[href]!r}, declared {clean(label)!r}",
            )
        dropped = {href for href, _ in self.exclude}
        kept = [node for node in nodes if node.get("href") not in dropped]
        if self.append is not None:
            kept = kept + self.append.select(toc)
        return kept


def locator(node: dict[str, Any], heading: str, *, table: str | None = None) -> str:
    value = (
        f"topic:{topic_path(node['href'])}"
        f";topic-id:{node['topicId']}"
        f";heading:{heading}"
    )
    return f"{value};table:{table}" if table else value


def toc_values(
    selector: Selector,
    tocs: dict[str, Toc],
    headings: dict[str, str],
) -> list[tuple[str, str]]:
    nodes = selector.select(tocs[selector.product])
    values: list[tuple[str, str]] = []
    for index, node in enumerate(nodes):
        if selector.label_source == "topic-h1":
            label = headings[topic_path(node["href"])]
        else:
            label = clean(node["label"])
        if selector.chapter_start is not None:
            label = f"Chapter {selector.chapter_start + index}. {label}"
        values.append((label, locator(node, label)))
    return values


# --------------------------------------------------------------------------
# topic bodies
# --------------------------------------------------------------------------


def html(cache: Path, path: str) -> BeautifulSoup:
    return BeautifulSoup((cache / "content" / path).read_text(encoding="utf-8"), "html.parser")


def heading_of(cache: Path, path: str) -> str:
    title = html(cache, path).select_one("h1.topictitle1")
    require(title is not None, f"topic {path} carries no h1.topictitle1")
    return clean(title.get_text(" ", strip=True))


def table_rows(table: Any) -> list[list[str]]:
    values = []
    for tr in table.select("tr"):
        cells = [clean(cell.get_text(" ", strip=True)) for cell in tr.find_all(["th", "td"], recursive=False)]
        if cells:
            values.append(cells)
    return values


# --------------------------------------------------------------------------
# catalog assembly
# --------------------------------------------------------------------------


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


COBOL_LR = "SS6SG3_6.5/lr/ref"
DB2_SQL = "SSEPEK_13.0.0/sqlref/src/tpc"
JCL_BOOK = "SSLTBW_3.2.0/com.ibm.zos.v3r2.ieab600"
AMS_BOOK = "SSLTBW_3.2.0/com.ibm.zos.v3r2.idai200"
RACF_BOOK = "SSLTBW_3.2.0/com.ibm.zos.v3r2.icha400"
ZOSMF_BOOK = "SSLTBW_3.2.0/com.ibm.zos.v3r2.izua700"
ZOSMF_RESOURCE_POOL = f"{ZOSMF_BOOK}/izuprog_API_CloudResourcePoolServices.htm"

SELECTORS: dict[tuple[str, str], Selector] = {
    ("cobol", "procedure-statements"): Selector("SS6SG3_6.5", f"{COBOL_LR}/rlpdst.html"),
    ("cobol", "intrinsic-functions"): Selector(
        "SS6SG3_6.5",
        f"{COBOL_LR}/rlinf.html",
        exclude=(
            (f"{COBOL_LR}/rlinfspe.html", "Specifying a function"),
            (f"{COBOL_LR}/rlinffun.html", "Function definitions"),
        ),
        chapter_start=31,
    ),
    ("cobol", "compiler-directing-statements"): Selector("SS6SG3_6.5", f"{COBOL_LR}/rlcds.html"),
    ("cobol", "compiler-directive-groups"): Selector("SS6SG3_6.5", f"{COBOL_LR}/rldir.html"),
    ("cobol", "file-description-clauses"): Selector(
        "SS6SG3_6.5",
        f"{COBOL_LR}/rlfde.html",
        exclude=((f"{COBOL_LR}/rlfden.html", "FILE SECTION"),),
    ),
    ("cobol", "data-description-clauses"): Selector(
        "SS6SG3_6.5",
        f"{COBOL_LR}/rldde.html",
        exclude=(
            (f"{COBOL_LR}/rlddefm1.html", "Format 1"),
            (f"{COBOL_LR}/rlddefm2.html", "Format 2"),
            (f"{COBOL_LR}/rlddefm3.html", "Format 3"),
            (f"{COBOL_LR}/rlddefm4a.html", "Format 4"),
            (f"{COBOL_LR}/rlddelev.html", "Level-numbers"),
        ),
    ),
    ("db2", "sql-statements"): Selector(
        "SSEPEK_13.0.0",
        f"{DB2_SQL}/db2z_sql_statementsintro.html",
        exclude=(
            (f"{DB2_SQL}/db2z_sqlstmtcategories.html", "Categories of SQL statements"),
            (f"{DB2_SQL}/db2z_howsqlstatementsareinvoked.html", "How SQL statements are invoked"),
            (f"{DB2_SQL}/db2z_sqlcomments.html", "SQL comments"),
        ),
    ),
    ("db2", "sql-pl-statements"): Selector(
        "SSEPEK_13.0.0",
        f"{DB2_SQL}/db2z_sqlplnativeintro.html",
        exclude=(
            (f"{DB2_SQL}/db2z_refs2parmsandvarsinnativesqlpl.html",
             "References to SQL parameters and variables"),
            (f"{DB2_SQL}/db2z_refs2conditionnamesinnativesqlpl.html", "References to SQL condition names"),
            (f"{DB2_SQL}/db2z_refs2cursornamesinnativesqlpl.html", "References to SQL cursor names"),
            (f"{DB2_SQL}/db2z_refs2labelsinnativesqlpl.html", "References to SQL labels"),
            (f"{DB2_SQL}/db2z_refs2statementnamesinnativesqlpl.html", "References to SQL statement names"),
            (f"{DB2_SQL}/db2z_nestedcompoundstatementsinnativesqlpl.html",
             "Summary of name scoping in nested compound statements"),
            (f"{DB2_SQL}/db2z_sqlprocedurestatement4nativesqlpl.html",
             "SQL-procedure-statement (SQL PL)"),
        ),
        label_source="topic-h1",
    ),
    ("jcl-jes2", "jes2-jecl-statements"): Selector(
        "SSLTBW_3.2.0",
        f"{JCL_BOOK}/j2st.htm",
        exclude=((f"{JCL_BOOK}/iea3b6_Description20.htm", "Description"),),
    ),
    ("jcl-jes2", "dd-parameters"): Selector(
        "SSLTBW_3.2.0",
        f"{JCL_BOOK}/ddst.htm",
        exclude=((f"{JCL_BOOK}/iea3b6_Description4.htm", "Description"),),
    ),
    ("jcl-jes2", "exec-parameters"): Selector(
        "SSLTBW_3.2.0",
        f"{JCL_BOOK}/execst.htm",
        exclude=((f"{JCL_BOOK}/iea3b6_Description8.htm", "Description"),),
    ),
    ("jcl-jes2", "job-parameters"): Selector(
        "SSLTBW_3.2.0",
        f"{JCL_BOOK}/jobst.htm",
        exclude=((f"{JCL_BOOK}/iea3b6_Description13.htm", "Description"),),
    ),
    ("jcl-jes2", "output-parameters"): Selector(
        "SSLTBW_3.2.0",
        f"{JCL_BOOK}/outst.htm",
        exclude=((f"{JCL_BOOK}/iea3b6_Description15.htm", "Description"),),
    ),
    ("dataset-vsam-ams", "ams-functional-commands"): Selector(
        "SSLTBW_3.2.0",
        f"{AMS_BOOK}/abstract.htm",
        exclude=(
            (f"{AMS_BOOK}/abstract.htm?pos=2", "Abstract for DFSMS Access Method Services Commands"),
            (f"{AMS_BOOK}/notnew.htm", "Notational conventions"),
            (f"{AMS_BOOK}/code.htm", "How to code access method services commands"),
            (f"{AMS_BOOK}/zossoc_main.htm", "Summary of changes"),
            (f"{AMS_BOOK}/using.htm", "Using Access Method Services"),
            (f"{AMS_BOOK}/modal.htm", "Modal Commands"),
            (f"{AMS_BOOK}/format.htm", "Functional Command Syntax"),
            (f"{AMS_BOOK}/scrty.htm", "Security Authorization Levels"),
            (f"{AMS_BOOK}/apxbcat.htm", "Interpreting LISTCAT Output Listings"),
            (f"{AMS_BOOK}/apxshcd.htm", "Interpreting SHCDS Output Listings"),
            (f"{AMS_BOOK}/probpgm.htm", "Invoking Access Method Services from Your Program"),
            (f"{AMS_BOOK}/apxdxt.htm", "DCOLLECT User Exit"),
            (f"{AMS_BOOK}/apxdcol.htm", "Interpreting DCOLLECT Output"),
        ),
        chapter_start=4,
    ),
    ("racf-saf", "racf-command-families"): Selector("SSLTBW_3.2.0", f"{RACF_BOOK}/cmdsyn.htm"),
    ("zosmf", "rest-service-families"): Selector(
        "SSLTBW_3.2.0", f"{ZOSMF_BOOK}/IZUHPINFO_RESTServices.htm"
    ),
    ("zosmf", "direct-guide-operation-headings"): Selector(
        "SSLTBW_3.2.0",
        f"{ZOSMF_BOOK}/IZUHPINFO_RESTServices.htm",
        depth=2,
        # The reviewed unit ends with six operations of the cloud provisioning
        # resource pool service, which sit one level below the rest of it.  They
        # are the first six of that parent's sixteen children, and the six are
        # named here rather than sliced off the front of the whole depth-3
        # sequence: the six IP, port and SNA application-name operations are the
        # reviewed roster, and the ten LPAR-pool and classification-rule
        # operations beside them are not.
        append=Appendix(
            ZOSMF_RESOURCE_POOL,
            (
                (f"{ZOSMF_BOOK}/izuprog_API_RPObtainIP.htm", "Obtain an IP address"),
                (f"{ZOSMF_BOOK}/izuprog_API_RPReleaseIP.htm", "Release an IP address"),
                (f"{ZOSMF_BOOK}/izuprog_API_RPObtainPort.htm", "Obtain a port"),
                (f"{ZOSMF_BOOK}/izuprog_API_RPReleasePort.htm", "Release a port"),
                (f"{ZOSMF_BOOK}/izuprog_API_RPObtainSNAAppl.htm", "Obtain a SNA application name"),
                (f"{ZOSMF_BOOK}/izuprog_API_RPReleaseSNAAppl.htm", "Release a SNA application name"),
            ),
        ),
    ),
}

CICS_TOPIC = "SSJL4D_6.x/reference-diagnostics/eib/dfha8mf.html"
IMS_TOPIC = "SSEPH2_15.6.0/com.ibm.ims156.doc.apg/ims_comparingexecdlicmdsanddlicalls.htm"
MQ_TOPIC = "SSFKSJ_9.4.0/refdev/q101650_.html"
RACROUTE_TOPIC = "SSLTBW_3.2.0/com.ibm.zos.v3r2.ichc600/racri.htm"
JCL_STATEMENTS_TOPIC = f"{JCL_BOOK}/iea3b6_JCL_statements.htm"
JCL_STATEMENTS_TABLE = "idg6175__cjsts"
JCL_STATEMENTS_TOPIC_ID = "statements-jcl"

JCL_STATEMENTS_COLUMNS = ("Statement", "Name", "Purpose")

# The reviewed JCL statement roster, each name beside the published column it is
# spelled in: 1 is the table's "Statement" column, 2 is its "Name" column.  The
# roster is declared here rather than read out of the table because the table
# spells half of it in lowercase, and it is checked against the table below.
#
# These twenty rows are body rows of one table, not table-of-contents nodes, so
# their discriminator has to come out of the table itself.  There is no cell
# anchor to cite: the markup gives every HEADER cell an id
# (idg6175__cjsts__entry__1 through __3) and gives the body cells none, and a
# body cell carries only `headers=` naming its column, which is identical all
# the way down a column and so separates no two rows.  What is left is the body
# row's ordinal, and it is this roster that makes the ordinal mean something --
# entry i is required to be body row i, in the column declared beside it, so a
# reordered or reworded table stops the tool rather than quietly repointing a
# row.  That is why the emitted locator can carry `row:N` and be believed.
JCL_STATEMENTS = (
    ("JCL command", 2), ("COMMAND", 1), ("comment", 1), ("CNTL", 1), ("DD", 1),
    ("delimiter", 2), ("ENDCNTL", 1), ("EXEC", 1), ("EXPORT", 1),
    ("IF/THEN/ELSE/ENDIF", 1), ("INCLUDE", 1), ("JCLLIB", 1), ("JOB", 1),
    ("null", 2), ("OUTPUT JCL", 2), ("PEND", 1), ("PROC", 1), ("SCHEDULE", 1),
    ("SET", 1), ("XMIT", 1),
)


def toc_units(cache: Path, tocs: dict[str, Toc]) -> dict[tuple[str, str], list[tuple[str, str]]]:
    headings: dict[str, str] = {}
    for key, selector in SELECTORS.items():
        if selector.label_source == "topic-h1":
            for node in selector.select(tocs[selector.product]):
                path = topic_path(node["href"])
                headings[path] = heading_of(cache, path)
    return {key: toc_values(selector, tocs, headings) for key, selector in SELECTORS.items()}


def cobol(units: dict[tuple[str, str], list[tuple[str, str]]]) -> dict[str, Any]:
    baseline = "ibm-enterprise-cobol-6.5-2026-05-31"
    return catalog(
        baseline,
        "cobol",
        [
            unit(baseline, name, units[("cobol", name)])
            for name in (
                "procedure-statements",
                "intrinsic-functions",
                "compiler-directing-statements",
                "compiler-directive-groups",
                "file-description-clauses",
                "data-description-clauses",
            )
        ],
    )


def cics(cache: Path) -> dict[str, Any]:
    baseline = "ibm-cics-ts-6x-2026-08-31"
    soup = html(cache, CICS_TOPIC)

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

    return catalog(
        baseline,
        "cics",
        [
            unit(baseline, "api-commands", values("dfha8mf__eibfn_table_cmds_api", False)),
            unit(baseline, "spi-commands-unique", values("dfha8mf__eibfn_table_cmds_spi", True)),
            unit(baseline, "fepi-commands", values("dfha8mf__eibfn_table_cmds_fepi", False)),
        ],
    )


def jcl(cache: Path, units: dict[tuple[str, str], list[tuple[str, str]]]) -> dict[str, Any]:
    baseline = "ibm-zos-3.2-jcl-jes2-2026-06"
    table = html(cache, JCL_STATEMENTS_TOPIC).select_one(f"#{JCL_STATEMENTS_TABLE}")
    require(table is not None, f"missing JCL table {JCL_STATEMENTS_TABLE}")
    rows = table_rows(table)
    require(
        tuple(rows[0]) == JCL_STATEMENTS_COLUMNS,
        f"JCL statement table columns are {rows[0]}, declared {list(JCL_STATEMENTS_COLUMNS)}",
    )
    body = rows[1:]
    require(
        len(body) == len(JCL_STATEMENTS),
        f"JCL statement table has {len(body)} body rows, roster has {len(JCL_STATEMENTS)}",
    )
    statements: list[tuple[str, str]] = []
    for index, ((name, column), cells) in enumerate(zip(JCL_STATEMENTS, body), 1):
        require(len(cells) == 3, f"malformed JCL statement row: {cells}")
        # The Statement column codes each name the way it is punched, so the
        # comment and delimiter markers come off before the comparison.  The
        # Name column carries the name alone.
        cell = re.sub(r"^//\*?\s*", "", cells[0]) if column == 1 else cells[column - 1]
        require(
            cell.casefold() == name.casefold(),
            f"JCL statement roster row {index} declares {name!r} in the "
            f"{JCL_STATEMENTS_COLUMNS[column - 1]} column, where the published "
            f"table carries {cell!r}",
        )
        statements.append(
            (
                name,
                f"topic:{JCL_STATEMENTS_TOPIC};topic-id:{JCL_STATEMENTS_TOPIC_ID}"
                f";heading:{name};table:{JCL_STATEMENTS_TABLE};row:{index}",
            )
        )
    return catalog(
        baseline,
        "jcl-jes2",
        [
            unit(baseline, "jcl-statements", statements),
            unit(baseline, "jes2-jecl-statements", units[("jcl-jes2", "jes2-jecl-statements")]),
            unit(baseline, "dd-parameters", units[("jcl-jes2", "dd-parameters")]),
            unit(baseline, "exec-parameters", units[("jcl-jes2", "exec-parameters")]),
            unit(baseline, "job-parameters", units[("jcl-jes2", "job-parameters")]),
            unit(baseline, "output-parameters", units[("jcl-jes2", "output-parameters")]),
        ],
    )


def ams(units: dict[tuple[str, str], list[tuple[str, str]]]) -> dict[str, Any]:
    baseline = "ibm-zos-3.2-dfsms-ams-2026-06"
    organizations = [
        ("entry-sequenced data set", "roadmap-normalization:vsam-primary-organizations"),
        ("key-sequenced data set", "roadmap-normalization:vsam-primary-organizations"),
        ("linear data set", "roadmap-normalization:vsam-primary-organizations"),
        ("relative record data set", "roadmap-normalization:vsam-primary-organizations"),
        ("variable-length relative record data set", "roadmap-normalization:vsam-primary-organizations"),
    ]
    return catalog(
        baseline,
        "dataset-vsam-ams",
        [
            unit(
                baseline,
                "ams-functional-commands",
                units[("dataset-vsam-ams", "ams-functional-commands")],
            ),
            unit(baseline, "vsam-primary-organizations", organizations),
        ],
    )


def racf(cache: Path, units: dict[tuple[str, str], list[tuple[str, str]]]) -> dict[str, Any]:
    baseline = "ibm-zos-3.2-racf-saf-2026"
    soup = html(cache, RACROUTE_TOPIC)
    matrix = next(
        (cells for table in soup.select("table") for cells in table_rows(table) if cells and cells[0] == "RACROUTE parameters"),
        None,
    )
    require(matrix is not None and len(matrix) == 15, "RACROUTE request matrix changed")
    requests = [
        (name.replace(" ", ""), "html-table:keyword-and-parameter-cross-reference")
        for name in matrix[1:]
    ]
    return catalog(
        baseline,
        "racf-saf",
        [
            unit(baseline, "racf-command-families", units[("racf-saf", "racf-command-families")]),
            unit(baseline, "racroute-request-types", requests),
        ],
    )


def zosmf(units: dict[tuple[str, str], list[tuple[str, str]]]) -> dict[str, Any]:
    baseline = "ibm-zosmf-3.2-2026-07-27"
    return catalog(
        baseline,
        "zosmf",
        [
            unit(baseline, "rest-service-families", units[("zosmf", "rest-service-families")]),
            unit(
                baseline,
                "direct-guide-operation-headings",
                units[("zosmf", "direct-guide-operation-headings")],
                normalization="heading-only-pending-endpoint-normalization",
            ),
        ],
    )


def db2(units: dict[tuple[str, str], list[tuple[str, str]]]) -> dict[str, Any]:
    baseline = "ibm-db2-for-zos-13-2026-08-13"
    return catalog(
        baseline,
        "db2",
        [
            unit(baseline, "sql-statements", units[("db2", "sql-statements")]),
            unit(baseline, "sql-pl-statements", units[("db2", "sql-pl-statements")]),
        ],
    )


def ims(cache: Path) -> dict[str, Any]:
    baseline = "ibm-ims-15.6-dli-2026-08-31"
    table = html(cache, IMS_TOPIC).select_one("table")
    require(table is not None, "IMS comparison table is missing")
    values = []
    for index, cells in enumerate(table_rows(table)[1:], 1):
        require(len(cells) == 3, f"malformed IMS row: {cells}")
        call, command, _purpose = cells
        values.append((clean(re.sub(r"\s+\d+$", "", call)), f"html-table:comparison;row:{index};command:{clean(re.sub(r'\s+\d+$', '', command))}"))
    return catalog(baseline, "ims", [unit(baseline, "dli-call-families", values)])


def mq(cache: Path) -> dict[str, Any]:
    baseline = "ibm-mq-9.4-mqi-2026-08-31"
    soup = html(cache, MQ_TOPIC)
    seen: set[str] = set()
    values = []
    for anchor in soup.select("article a[href]"):
        title = clean(anchor.get_text(" ", strip=True))
        name = title.split(" - ", 1)[0]
        if not name.startswith("MQ") or name in seen:
            continue
        seen.add(name)
        # The content endpoint serves site-absolute hrefs where the reviewed
        # snapshot carried bare filenames.  The locator keeps the filename, so
        # the reviewed mq rows are unaffected by the change of source.
        values.append((name, f"html-link:{PurePosixPath(anchor.get('href')).name}"))
    return catalog(baseline, "mq", [unit(baseline, "mqi-calls-unique", values)])


# --------------------------------------------------------------------------
# assertions the tool refuses to write past
# --------------------------------------------------------------------------

# The twenty JCL statement rows are body rows of one table in one topic, so they
# share a topic href by construction and are separated by the `row:` ordinal the
# locator carries.  Every other relocated row gets a topic of its own.
SHARED_TOPIC_UNITS = {("ibm-zos-3.2-jcl-jes2-2026-06", "jcl-statements")}


@dataclass
class Assertions:
    checks: list[dict[str, Any]] = field(default_factory=list)

    def record(self, name: str, ok: bool, detail: Any) -> None:
        self.checks.append({"assertion": name, "passed": bool(ok), "detail": detail})
        require(ok, f"{name} failed: {json.dumps(detail, ensure_ascii=False)[:2000]}")


def ordered_rows(catalogs: list[dict[str, Any]]) -> list[tuple[str, str, str, str]]:
    """(baseline, unit, row_id, label) in catalog order, plus the locator."""
    rows = []
    for value in catalogs:
        for item in value["units"]:
            for row in item["rows"]:
                rows.append((value["baseline_id"], item["id"], row["id"], row["label"], row["source_locator"]))
    return rows


def check(catalogs: list[dict[str, Any]], oracle: dict[str, Any], index: dict[str, Any]) -> Assertions:
    assertions = Assertions()
    produced = ordered_rows(catalogs)

    oracle_rows: list[tuple[str, str, str]] = []
    oracle_units: dict[tuple[str, str], list[str]] = {}
    for subsystem, baseline in oracle["baselines"].items():
        for item in baseline["units"]:
            unit_id = item["rows"][0]["row_id"].split(":", 2)[1]
            oracle_units[(baseline["baseline_id"], unit_id)] = [row["row_id"] for row in item["rows"]]
            for row in item["rows"]:
                oracle_rows.append((baseline["baseline_id"], unit_id, row["row_id"]))

    # 1. identical row_id multiset
    produced_ids = sorted(row[2] for row in produced)
    oracle_ids = sorted(row[2] for row in oracle_rows)
    assertions.record(
        "row-id multiset identical to the frozen oracle",
        produced_ids == oracle_ids,
        {
            "produced": len(produced_ids),
            "oracle": len(oracle_ids),
            "only_produced": sorted(set(produced_ids) - set(oracle_ids))[:20],
            "only_oracle": sorted(set(oracle_ids) - set(produced_ids))[:20],
        },
    )

    # 2. identical row_id -> label map, byte for byte
    oracle_labels = {
        row["row_id"]: row["label"]
        for baseline in oracle["baselines"].values()
        for item in baseline["units"]
        for row in item["rows"]
    }
    moved = [
        {"row_id": row[2], "oracle": oracle_labels.get(row[2]), "produced": row[3]}
        for row in produced
        if oracle_labels.get(row[2]) != row[3]
    ]
    assertions.record(
        "row-id -> label map identical to the frozen oracle",
        not moved,
        {"rows_with_a_changed_label": len(moved), "examples": moved[:20]},
    )

    # 3. identical row ORDER within every unit -- the whole defence against a
    #    silent permutation, and a hard failure, never a warning.
    produced_units: dict[tuple[str, str], list[str]] = {}
    for baseline_id, unit_id, row_id, _label, _locator in produced:
        produced_units.setdefault((baseline_id, unit_id), []).append(row_id)
    reordered = [
        {"unit": f"{key[0]}/{key[1]}", "oracle": oracle_units.get(key), "produced": value}
        for key, value in produced_units.items()
        if oracle_units.get(key) != value
    ]
    assertions.record(
        "row order within every unit identical to the frozen oracle",
        not reordered and set(produced_units) == set(oracle_units),
        {
            "units": len(produced_units),
            "reordered_units": [item["unit"] for item in reordered],
            "only_produced": sorted(f"{a}/{b}" for a, b in set(produced_units) - set(oracle_units)),
            "only_oracle": sorted(f"{a}/{b}" for a, b in set(oracle_units) - set(produced_units)),
        },
    )

    # 4. every denominator equal to index.json immutable_denominators
    frozen = {
        baseline["id"]: baseline["immutable_denominators"] for baseline in index["baselines"]
    }
    wrong = []
    for value in catalogs:
        expected = frozen.get(value["baseline_id"])
        produced_denominators = {item["id"]: item["denominator"] for item in value["units"]}
        if expected != produced_denominators:
            wrong.append(
                {"baseline": value["baseline_id"], "index": expected, "produced": produced_denominators}
            )
        if sum(produced_denominators.values()) != value["mandatory_rows"]:
            wrong.append({"baseline": value["baseline_id"], "mandatory_rows": value["mandatory_rows"]})
    assertions.record(
        "every denominator equal to index.json immutable_denominators",
        not wrong,
        {"baselines": len(catalogs), "disagreements": wrong},
    )

    # 5. the relocated rows form a bijection onto distinct topics
    relocated = [row for row in produced if row[4].startswith("topic:")]
    per_topic = [row for row in relocated if (row[0], row[1]) not in SHARED_TOPIC_UNITS]
    shared = [row for row in relocated if (row[0], row[1]) in SHARED_TOPIC_UNITS]
    topics = [row[4].split(";", 1)[0] for row in per_topic]
    locators = [row[4] for row in relocated]
    assertions.record(
        "relocated rows form a bijection onto distinct topics",
        len(topics) == len(set(topics)) and len(locators) == len(set(locators)),
        {
            "relocated_rows": len(relocated),
            "rows_with_their_own_topic": len(per_topic),
            "distinct_topics": len(set(topics)),
            "rows_sharing_one_table_topic": len(shared),
            "distinct_locators": len(set(locators)),
        },
    )
    return assertions


# --------------------------------------------------------------------------


def write_catalog(path: Path, value: dict[str, Any]) -> None:
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cache", type=Path, help="directory holding toc/ and content/ bytes")
    parser.add_argument("output", type=Path, help="directory the catalogs are written to")
    parser.add_argument(
        "--oracle",
        type=Path,
        required=True,
        help="frozen row oracle the five assertions are checked against",
    )
    parser.add_argument(
        "--index",
        type=Path,
        default=Path(__file__).resolve().parent.parent / "catalogs" / "index.json",
        help="catalog index carrying immutable_denominators",
    )
    parser.add_argument("--fetch", action="store_true", help="populate the cache from ibm.com first")
    parser.add_argument("--assertions", type=Path, help="write the assertion report here")
    args = parser.parse_args()

    if args.fetch:
        populate(args.cache)
    verify_pinned(args.cache)

    tocs = {product: Toc(args.cache / "toc" / f"{product}.json") for product in TOC_PRODUCTS}
    units = toc_units(args.cache, tocs)
    catalogs = [
        cobol(units), cics(args.cache), jcl(args.cache, units), ams(units), racf(args.cache, units),
        zosmf(units), db2(units), ims(args.cache), mq(args.cache),
    ]

    oracle = json.loads(args.oracle.read_text(encoding="utf-8"))
    index = json.loads(args.index.read_text(encoding="utf-8"))
    assertions = check(catalogs, oracle, index)

    args.output.mkdir(parents=True, exist_ok=True)
    for value in catalogs:
        write_catalog(args.output / f"{value['subsystem']}.json", value)
    if args.assertions:
        args.assertions.write_text(
            json.dumps(
                {
                    "schema_version": "survey.catalog-extractor-assertions@1",
                    "oracle": str(args.oracle),
                    "checks": assertions.checks,
                    "coverage_credit": 0,
                    "retained_in_repository": False,
                },
                indent=2,
                ensure_ascii=False,
            )
            + "\n",
            encoding="utf-8",
        )


if __name__ == "__main__":
    main()
