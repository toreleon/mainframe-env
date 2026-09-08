"""Offline checks for the table, link, and normalization locator audit.

The publication bodies are deliberately not fixtures in the repository.  The
tests instead construct the same DITA shapes from the committed catalog rows,
exercise every one of the 636 publication-backed embedded locators, and mutate
the discriminator so a verifier that merely reports ``exact`` cannot pass.
"""

from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
REPOSITORY = TOOLS.parents[1]

sys.path.insert(0, str(TOOLS))

import docs_api as DOCS  # noqa: E402
import verify_topic_locators as LOCATORS  # noqa: E402


def official_row(number: int, label: str, locator: str, unit: str = "unit") -> dict[str, object]:
    return {
        "id": f"baseline:{unit}:{number:04d}",
        "label": label,
        "source_locator": locator,
        "mandatory": True,
    }


def topic_body(content: str) -> str:
    return (
        '<article role="article"><h1 class="topictitle1">Fixture</h1>'
        '<div id="lastModifiedDate">Last Updated 2026-09-08</div>'
        f"{content}</article>"
    )


def data_table(table_id: str, rows: list[list[str]]) -> str:
    body = "".join(
        "<tr>" + "".join(f"<td>{cell}</td>" for cell in row) + "</tr>"
        for row in rows
    )
    return f'<table id="{table_id}"><tbody>{body}</tbody></table>'


def header_table(table_id: str, cells: list[str]) -> str:
    headings = "".join(f"<th>{cell}</th>" for cell in cells)
    return f'<table id="{table_id}"><thead><tr>{headings}</tr></thead></table>'


def catalog(name: str) -> dict:
    return json.loads(
        (REPOSITORY / f"conformance/0.2/catalogs/{name}.json").read_text(encoding="utf-8")
    )


def rows(document: dict) -> list[dict]:
    return [row for unit in document["units"] for row in unit["rows"]]


class MarkupHelperTests(unittest.TestCase):
    def test_table_ids_accept_both_html_quote_styles(self) -> None:
        body = '<table id="first"></table><table id=\'second\'></table>'
        self.assertEqual(DOCS.table_ids(body), ["first", "second"])

    def test_header_rows_are_kept_separate_from_body_rows(self) -> None:
        body = (
            '<table id="matrix"><thead><tr><th>Request</th><th>A U T H</th></tr></thead>'
            "<tbody><tr><td>CLASS=</td><td>X</td></tr></tbody></table>"
        )
        self.assertEqual(DOCS.table_header_rows(body, "matrix"), [["Request", "A U T H"]])
        self.assertEqual(DOCS.table_rows(body, "matrix"), [["CLASS=", "X"]])

    def test_link_text_and_target_are_read_as_one_anchor(self) -> None:
        body = '<a class="x" href=\'/docs/q1.html?x=1\'>MQ&lt;ONE&gt; - <em>description</em></a>'
        self.assertEqual(DOCS.html_links(body), [("MQ<ONE> - description", "/docs/q1.html?x=1")])


class CicsLocatorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.catalog = catalog("cics")
        tables: dict[str, list[list[str]]] = {}
        for row in rows(cls.catalog):
            parts = LOCATORS.components(row["source_locator"])
            tables.setdefault(parts["html-table"], []).append(
                [row["label"], parts["eibfn"], parts["family"]]
            )
        cls.body = topic_body("".join(data_table(name, data) for name, data in tables.items()))

    def test_all_571_committed_rows_resolve_once(self) -> None:
        results = [LOCATORS.check_html_table(row, "cics", self.body) for row in rows(self.catalog)]
        self.assertEqual(571, len(results))
        self.assertEqual({"exact"}, {result["verdict"] for result in results})
        self.assertEqual(
            {"html-table-row"}, {result["matched_on"] for result in results}
        )

    def test_swapping_two_locators_is_detected_on_both_rows(self) -> None:
        first, second = rows(self.catalog)[:2]
        swapped_first = {**first, "source_locator": second["source_locator"]}
        swapped_second = {**second, "source_locator": first["source_locator"]}
        for row in (swapped_first, swapped_second):
            with self.subTest(row=row["id"]):
                result = LOCATORS.check_html_table(row, "cics", self.body)
                self.assertEqual("retitled", result["verdict"])
                self.assertIn("heading_found_in_rows", result)

    def test_an_identity_that_occurs_twice_is_not_credited(self) -> None:
        row = rows(self.catalog)[0]
        parts = LOCATORS.components(row["source_locator"])
        duplicate = [row["label"], parts["eibfn"], parts["family"]]
        body = topic_body(data_table(parts["html-table"], [duplicate, duplicate]))
        result = LOCATORS.check_html_table(row, "cics", body)
        self.assertEqual("retitled", result["verdict"])
        self.assertEqual("html-table-row-ambiguous", result["matched_on"])
        self.assertEqual([1, 2], result["heading_found_in_rows"])


class ImsLocatorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.catalog = catalog("ims")
        cells = []
        for row in rows(cls.catalog):
            parts = LOCATORS.components(row["source_locator"])
            cells.append([row["label"], parts["command"], "Purpose"])
        cls.body = topic_body(
            data_table(LOCATORS.TABLE_ALIASES[("ims", "comparison")], cells)
        )

    def test_all_25_committed_rows_resolve_by_ordinal_and_cells(self) -> None:
        results = [LOCATORS.check_html_table(row, "ims", self.body) for row in rows(self.catalog)]
        self.assertEqual(25, len(results))
        self.assertEqual({"exact"}, {result["verdict"] for result in results})

    def test_swapping_duplicate_visible_rows_is_still_detected(self) -> None:
        # IMS rows 1 and 12 both publish INIT call / ACCEPT command.  Comparing
        # cells alone cannot see this swap; the frozen catalog ordinal can.
        source = rows(self.catalog)
        first, twelfth = source[0], source[11]
        self.assertEqual(first["label"], twelfth["label"])
        self.assertEqual(
            LOCATORS.components(first["source_locator"])["command"],
            LOCATORS.components(twelfth["source_locator"])["command"],
        )
        for row, locator in (
            (first, twelfth["source_locator"]),
            (twelfth, first["source_locator"]),
        ):
            with self.subTest(row=row["id"]):
                result = LOCATORS.check_html_table(
                    {**row, "source_locator": locator}, "ims", self.body
                )
                self.assertEqual("retitled", result["verdict"])
                self.assertNotEqual(result["catalog_row"], result["row"])

    def test_printed_footnote_numbers_do_not_change_the_identity(self) -> None:
        row = rows(self.catalog)[4]
        parts = LOCATORS.components(row["source_locator"])
        body = topic_body(
            data_table(
                LOCATORS.TABLE_ALIASES[("ims", "comparison")],
                [["unused", "unused", ""]] * 4
                + [[row["label"] + " 1", parts["command"] + " 1", "Purpose"]],
            )
        )
        self.assertEqual("exact", LOCATORS.check_html_table(row, "ims", body)["verdict"])


class RacrouteLocatorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.catalog = catalog("racf-saf")
        cls.requests = next(unit for unit in cls.catalog["units"] if unit["id"] == "racroute-request-types")
        published = ["RACROUTE parameters"] + [
            " ".join(row["label"]) for row in cls.requests["rows"]
        ]
        cls.body = topic_body(
            header_table(
                LOCATORS.TABLE_ALIASES[
                    ("racf-saf", "keyword-and-parameter-cross-reference")
                ],
                published,
            )
        )

    def test_all_14_requests_resolve_to_one_header_cell(self) -> None:
        results = [
            LOCATORS.check_html_table(row, "racf-saf", self.body)
            for row in self.requests["rows"]
        ]
        self.assertEqual(14, len(results))
        self.assertEqual({"exact"}, {result["verdict"] for result in results})
        self.assertEqual(list(range(2, 16)), [result["column"] for result in results])

    def test_a_repeated_request_header_is_ambiguous(self) -> None:
        row = self.requests["rows"][0]
        body = topic_body(
            header_table(
                LOCATORS.TABLE_ALIASES[
                    ("racf-saf", "keyword-and-parameter-cross-reference")
                ],
                ["RACROUTE parameters", "A U D I T", "A U D I T"],
            )
        )
        result = LOCATORS.check_html_table(row, "racf-saf", body)
        self.assertEqual("retitled", result["verdict"])
        self.assertEqual("html-table-header-cell-ambiguous", result["matched_on"])


class MqLocatorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.catalog = catalog("mq")
        anchors = []
        for row in rows(cls.catalog):
            target = LOCATORS.components(row["source_locator"])["html-link"]
            anchors.append(
                f'<a href="/docs/en/SSFKSJ_9.4.0/refdev/{target}">'
                f'{row["label"]} - description</a>'
            )
            if row["label"] == "MQMHBUF":
                anchors.append(anchors[-1])
        cls.body = topic_body("".join(anchors))

    def test_all_26_committed_rows_resolve_to_one_semantic_link(self) -> None:
        results = [LOCATORS.check_html_link(row, self.body) for row in rows(self.catalog)]
        self.assertEqual(26, len(results))
        self.assertEqual({"exact"}, {result["verdict"] for result in results})
        duplicate = next(result for result in results if result["target"] == "q101860_.html")
        self.assertEqual(2, duplicate["duplicate_anchor_occurrences"])

    def test_swapping_two_link_targets_is_detected(self) -> None:
        first, second = rows(self.catalog)[:2]
        for row, locator in (
            (first, second["source_locator"]),
            (second, first["source_locator"]),
        ):
            with self.subTest(row=row["id"]):
                result = LOCATORS.check_html_link({**row, "source_locator": locator}, self.body)
                self.assertNotEqual("exact", result["verdict"])
                self.assertEqual("moved", result["verdict"])

    def test_only_byte_identical_duplicate_anchors_collapse(self) -> None:
        row = next(row for row in rows(self.catalog) if row["label"] == "MQMHBUF")
        target = LOCATORS.components(row["source_locator"])["html-link"]
        body = topic_body(
            f'<a href="/docs/ref/{target}">MQMHBUF - first description</a>'
            f'<a href="/other/ref/{target}?view=alternate">MQMHBUF - second description</a>'
        )
        result = LOCATORS.check_html_link(row, body)
        self.assertEqual("retitled", result["verdict"])
        self.assertEqual("html-link-ambiguous", result["matched_on"])
        self.assertEqual(2, len(result["matching_anchors"]))


class CatalogConventionTests(unittest.TestCase):
    def test_all_641_previously_skipped_rows_are_accounted_for(self) -> None:
        counts: dict[str, int] = {}
        for name in (
            "cics",
            "cobol",
            "dataset-vsam-ams",
            "db2",
            "ims",
            "jcl-jes2",
            "mq",
            "racf-saf",
            "zosmf",
        ):
            for row in rows(catalog(name)):
                kind = LOCATORS.locator_kind(row["source_locator"])
                counts[kind] = counts.get(kind, 0) + 1
        self.assertEqual(610, counts["html-table"])
        self.assertEqual(26, counts["html-link"])
        self.assertEqual(5, counts["roadmap-normalization"])
        self.assertEqual(641, sum(counts[kind] for kind in counts if kind != "topic"))

    def test_every_embedded_body_is_named_by_a_pinned_receipt(self) -> None:
        index = json.loads(
            (REPOSITORY / "conformance/0.2/catalogs/index.json").read_text(encoding="utf-8")
        )
        for name in ("cics", "ims", "mq", "racf-saf"):
            document = catalog(name)
            baseline = next(row for row in index["baselines"] if row["subsystem"] == name)
            embedded = [
                row
                for row in rows(document)
                if LOCATORS.locator_kind(row["source_locator"])
                in ("html-table", "html-link")
            ]
            paths = {
                LOCATORS.locator_body_path(row, document, baseline) for row in embedded
            }
            if name == "racf-saf":
                pinned = {
                    source["topic_path"] for source in baseline["supporting_sources"]
                }
            else:
                manifest = json.loads(
                    (REPOSITORY / baseline["source"]["manifest"]).read_text(encoding="utf-8")
                )
                pinned = {topic["topic_path"] for topic in manifest["topics"]}
            self.assertTrue(paths)
            self.assertTrue(paths <= pinned, f"{name}: {paths - pinned}")

    def test_unreachable_embedded_body_is_skipped_never_missing(self) -> None:
        document = catalog("cics")
        row = rows(document)[0]
        baseline = {
            "id": document["baseline_id"],
            "source": {"book_href": "product/topic.html"},
        }
        result = LOCATORS.audit_row(
            row,
            document,
            baseline,
            {},
            {},
            {},
            {"product/topic.html": DOCS.Unreachable("https://example.invalid", "http-503")},
        )
        self.assertEqual("skipped", result["verdict"])
        self.assertEqual("endpoint-unreachable", result["reason"])
        self.assertNotEqual("missing", result["verdict"])

    def test_a_404_is_a_missing_source_rather_than_an_unreachable_one(self) -> None:
        document = catalog("mq")
        row = rows(document)[0]
        baseline = {
            "id": document["baseline_id"],
            "source": {"book_href": "product/topic.html"},
        }
        result = LOCATORS.audit_row(
            row,
            document,
            baseline,
            {},
            {},
            {},
            {"product/topic.html": DOCS.NotFound("https://example.invalid")},
        )
        self.assertEqual("missing", result["verdict"])

    def test_the_five_roadmap_rows_have_a_deliberate_documented_disposition(self) -> None:
        document = catalog("dataset-vsam-ams")
        normalized = [
            row
            for row in rows(document)
            if LOCATORS.locator_kind(row["source_locator"]) == "roadmap-normalization"
        ]
        self.assertEqual(5, len(normalized))
        for row in normalized:
            result = LOCATORS.check_roadmap_normalization(row)
            self.assertEqual("skipped", result["verdict"])
            self.assertEqual("documented-roadmap-normalization", result["reason"])
            self.assertEqual("deliberate", result["disposition"])
            self.assertIn("do not claim", result["detail"])
            documentation = REPOSITORY / result["documentation"]
            self.assertTrue(documentation.is_file())
            self.assertIn(
                "roadmap-normalization:vsam-primary-organizations",
                documentation.read_text(encoding="utf-8"),
            )


if __name__ == "__main__":
    unittest.main()
