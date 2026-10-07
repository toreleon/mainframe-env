"""Offline tests for the topic-locator tools.

Nothing here touches the network. Every retrieval is handed in as bytes or as
the exception the retrieval would have raised, which is what makes the one
distinction these tools exist to preserve testable: a topic that 404s is a
finding, and a topic we could not reach is not.
"""

from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
REPOSITORY = TOOLS.parents[1]

sys.path.insert(0, str(TOOLS))

# Imported by name rather than loaded from a path, so that the `docs_api` the
# tools import is the same module object the tests hold. Two copies would give
# two `Unreachable` classes, and `isinstance` would quietly stop matching.
import docs_api as DOCS  # noqa: E402
import fetch_pinned_sources as PINS  # noqa: E402
import verify_topic_locators as LOCATORS  # noqa: E402

TOC = {
    "toc": {
        "label": "book",
        "href": "SSEPEK_13.0.0/sqlref/src/tpc/db2z_book.html",
        "topics": [
            {
                "label": "ALLOCATE CURSOR",
                "href": "SSEPEK_13.0.0/sqlref/src/tpc/db2z_sql_allocatecursor.html",
                "topicId": "statements-allocate-cursor",
            },
            {
                "label": "SET CURRENT ACCELERATOR",
                "href": "SSEPEK_13.0.0/sqlref/src/tpc/db2z_sql_setaccel.html",
                "topicId": "statements-set-current-accelerator",
            },
            {
                "label": "SET CURRENT ACCELERATOR",
                "href": "SSEPEK_13.0.0/sqlref/src/tpc/db2z_sql_setaccel.html",
                "topicId": "accelerators-set-current-accelerator",
            },
            {
                "label": "ALLOCATE",
                "href": "SSLTBW_3.2.0/com.ibm.zos.v3r2.idai200/dalloc.htm",
                "topicId": "commands-allocate",
            },
            {
                "label": "JCL statements",
                "href": "SSLTBW_3.2.0/com.ibm.zos.v3r2.ieab600/iea3b6_JCL_statements.htm",
                "topicId": "statements-jcl",
            },
        ],
    }
}


#: `idg6175__cjsts` in `iea3b6_JCL_statements.htm`, transcribed cell for cell
#: from what IBM serves: a Statement column carrying the statement as the book
#: prints it, a Name column, and a Purpose column of prose. The 20 JCL statement
#: rows all cite this one table, one row each, in this order.
#:
#: All 20 rows are here rather than the few the finding turns on, because the
#: property under test is about the whole table at once — no label may name a
#: second row — and a subset cannot show it. The Purpose column is transcribed
#: whole for the same reason: it is where four of the five ambiguities the old
#: comparison admitted came from, and trimming it would have made the fixture
#: agree with the fix by omission.
JCL_STATEMENTS = [
    ('// command', 'JCL command',
     'Enters an MVS system operator command through the input stream. The '
     'command statement is used primarily by the operator. Use the COMMAND '
     'statement instead of the JCL command statement.'),
    ('// COMMAND', 'command',
     'Specifies an MVS or JES command that the system issues when the JCL '
     'is converted. Use the COMMAND statement instead of the JCL command '
     'statement.'),
    ('//* comment', 'comment',
     'Contains comments. The comment statement is used primarily to '
     'document a program and its resource requirements.'),
    ('// CNTL', 'control',
     'Marks the beginning of one or more program control statements.'),
    ('// DD', 'data definition',
     'Identifies and describes a data set.'),
    ('/*', 'delimiter',
     'Indicates the end of data placed in the input stream. Note: A user '
     'can designate any two characters to be the delimiter.'),
    ('// ENDCNTL', 'end control',
     'Marks the end of one or more program control statements.'),
    ('// EXEC', 'execute',
     'Marks the beginning of a job step; assigns a name to the step; '
     'identifies the program or the cataloged or in-stream procedure to be '
     'executed in this step.'),
    ('// EXPORT', 'export',
     'Makes specific JCL symbols available to the job step program.'),
    ('// IF/THEN/ELSE/ENDIF', 'IF/THEN/ELSE/ ENDIF statement construct',
     'Specifies conditional execution of job steps within a job.'),
    ('// INCLUDE', 'include',
     'Identifies a member of a partitioned data set (PDS) or partitioned '
     'data set extended (PDSE) that contains JCL statements to include in '
     'the job stream.'),
    ('// JCLLIB', 'JCL library',
     'Identifies the libraries that the system searches for: INCLUDE '
     'groups Procedures named in EXEC statements.'),
    ('// JOB', 'job',
     'Marks the beginning of a job; assigns a name to the job.'),
    ('//', 'null',
     'Marks the end of a job.'),
    ('// OUTPUT', 'output JCL',
     'Specifies the processing options that the job entry subsystem is to '
     'use for printing a sysout data set.'),
    ('// PEND', 'procedure end',
     'Marks the end of an in-stream or cataloged procedure.'),
    ('// PROC', 'procedure',
     'Marks the beginning of an in-stream procedure and may mark the '
     'beginning of a cataloged procedure; assigns default values to '
     'parameters defined in the procedure.'),
    ('// SCHEDULE', 'schedule',
     'Specifies scheduling attributes for a job such as the job group it '
     'is associated with and whether the job should be held for a time '
     'before execution.'),
    ('// SET', 'set',
     'Defines and assigns initial values to symbolic parameters used when '
     'processing JCL statements. Changes or nullifies the values assigned '
     'to symbolic parameters.'),
    ('// XMIT', 'transmit',
     'Transmits input stream records from one node to another.'),
]

#: The reviewed label the catalog gives each of those rows, in the same order.
#: Read from the tree as well, by `test_the_fixture_carries_the_labels_the_
#: catalog_actually_cites`, so this list cannot drift away from the catalog and
#: leave the uniqueness assertion testing a table nothing cites.
JCL_STATEMENT_LABELS = [
    "JCL command", "COMMAND", "comment", "CNTL", "DD", "delimiter", "ENDCNTL",
    "EXEC", "EXPORT", "IF/THEN/ELSE/ENDIF", "INCLUDE", "JCLLIB", "JOB", "null",
    "OUTPUT JCL", "PEND", "PROC", "SCHEDULE", "SET", "XMIT",
]

#: The ordinals the rows the tests swap carry in the served table.
JCL_COMMAND_ROW, COMMAND_ROW = 1, 2
SCHEDULE_ROW, SET_ROW = 18, 19


def statements_table(rows: list[tuple[str, str, str]] | None = None) -> str:
    """The table as markup: a `thead` of `th`, then one `tr` of `td` per row.

    The Statement column's double space is written `&nbsp;&nbsp;` where IBM
    serves two plain spaces, so the entity path through `strip_markup` stays
    exercised; both collapse to one space under `normalize`.

    `rows` is open so a test can serve a doctored table — one row duplicated,
    say — and see what the verifier does with a publication that has stopped
    telling its own rows apart.
    """
    cells = "".join(
        "<tr>"
        + "".join(f"<td>{text.replace(' ', '&nbsp;&nbsp;', 1) if i == 0 else text}</td>"
                  for i, text in enumerate(row))
        + "</tr>\n"
        for row in (JCL_STATEMENTS if rows is None else rows)
    )
    return (
        '<table summary="" id="idg6175__cjsts"><thead>\n'
        "<tr><th><strong>Statement</strong></th><th><strong>Name</strong></th>"
        "<th><strong>Purpose</strong></th></tr>\n"
        f"</thead><tbody>\n{cells}</tbody></table>\n"
    )


def body(heading: str, extra: str = "", updated: str = "2026-01-28") -> bytes:
    return (
        '<div><article role="article">\n'
        f'<h1 class="topictitle1" id="t__title__1">{heading}</h1>'
        f'<div id="lastModifiedDate"><span>Last Updated</span>: {updated}</div>\n'
        f"<div class=\"body\">{extra}</div></article></div>"
    ).encode("utf-8")


def statements(
    heading: str = "JCL statements", rows: list[tuple[str, str, str]] | None = None
) -> bytes:
    return body(heading, statements_table(rows))


def jcl_locator(heading: str, ordinal: int | str | None, table: str = "idg6175__cjsts") -> str:
    locator = (
        "topic:SSLTBW_3.2.0/com.ibm.zos.v3r2.ieab600/iea3b6_JCL_statements.htm"
        f";topic-id:statements-jcl;heading:{heading};table:{table}"
    )
    return locator if ordinal is None else f"{locator};row:{ordinal}"


def row(identifier: str, locator: str) -> dict[str, str]:
    return {"id": identifier, "source_locator": locator}


class RetrievalContractTests(unittest.TestCase):
    def test_the_whole_topic_path_is_one_encoded_segment(self) -> None:
        self.assertEqual(
            DOCS.content_url("SSFKSJ_9.4.0/refdev/q101650_.html"),
            "https://www.ibm.com/docs/api/v1/content/"
            "SSFKSJ_9.4.0%2Frefdev%2Fq101650_.html?parsebody=true&lang=en",
        )

    def test_a_navigation_position_suffix_is_dropped_before_retrieval(self) -> None:
        # ?pos=2 disambiguates a repeated navigation node, not a second
        # document, and the content endpoint redirects it while losing
        # parsebody=true on the way.
        self.assertEqual(
            DOCS.content_url("SSLTBW_3.2.0/com.ibm.zos.v3r2.icha400/abstract.htm?pos=2"),
            DOCS.content_url("SSLTBW_3.2.0/com.ibm.zos.v3r2.icha400/abstract.htm"),
        )

    def test_parsebody_is_part_of_the_pin(self) -> None:
        self.assertIn("parsebody=true", DOCS.CONTENT_URL)

    def test_the_user_agent_is_not_a_browser_string(self) -> None:
        # The edge rejects browser user agents and accepts curl's; a Mozilla
        # string here would turn every retrieval into a 403.
        self.assertNotIn("Mozilla", DOCS.USER_AGENT)


class DigestTests(unittest.TestCase):
    def test_the_digest_follows_the_definition_the_schema_states(self) -> None:
        topics = [
            {"topic_path": "b/second.html", "sha256": "b" * 64},
            {"topic_path": "a/first.html", "sha256": "a" * 64},
        ]
        import hashlib

        expected = hashlib.sha256(
            (f"a/first.html {'a' * 64}\n" f"b/second.html {'b' * 64}\n").encode("utf-8")
        ).hexdigest()
        self.assertEqual(DOCS.manifest_digest(topics), expected)

    def test_the_digest_does_not_depend_on_the_order_topics_are_listed_in(self) -> None:
        topics = [
            {"topic_path": "a/first.html", "sha256": "a" * 64},
            {"topic_path": "b/second.html", "sha256": "b" * 64},
        ]
        self.assertEqual(DOCS.manifest_digest(topics), DOCS.manifest_digest(topics[::-1]))

    def test_every_committed_manifest_still_hashes_to_its_recorded_digest(self) -> None:
        manifests = sorted((REPOSITORY / "conformance/subsystems/coverage/manifests").glob("*.json"))
        self.assertEqual(len(manifests), 9)
        for path in manifests:
            manifest = json.loads(path.read_text(encoding="utf-8"))
            with self.subTest(manifest=path.name):
                state = PINS.self_consistency(manifest)
                self.assertTrue(state["agrees"], state["recomputed_digest"])

    def test_the_schema_states_the_same_definition_the_code_uses(self) -> None:
        schema = json.loads(
            (REPOSITORY / "conformance/subsystems/coverage/schemas/topic-manifest.schema.json").read_text(
                encoding="utf-8"
            )
        )
        self.assertEqual(
            schema["properties"]["topic_manifest_digest_definition"]["const"],
            DOCS.DIGEST_DEFINITION,
        )

    def test_a_manifest_whose_digest_no_longer_follows_is_caught_offline(self) -> None:
        manifest = {
            "topics": [{"topic_path": "a/first.html", "sha256": "a" * 64, "bytes": 10}],
            "topic_manifest_digest": "0" * 64,
            "topic_count": 1,
            "total_bytes": 10,
        }
        self.assertFalse(PINS.self_consistency(manifest)["agrees"])


class BodyReadingTests(unittest.TestCase):
    def test_a_heading_broken_across_lines_compares_equal(self) -> None:
        # RACF sets several command headings across a line break inside the h1.
        self.assertEqual(
            DOCS.heading_of(body("RACLINK\n  (Administer user ID\xa0associations)").decode()),
            "RACLINK (Administer user ID associations)",
        )

    def test_the_last_updated_date_is_read_so_republication_is_distinguishable(self) -> None:
        self.assertEqual(DOCS.last_modified_of(body("ADDGROUP").decode()), "2026-01-28")

    def test_a_topic_without_a_date_reports_none_rather_than_guessing(self) -> None:
        self.assertIsNone(DOCS.last_modified_of("<h1 class='topictitle1'>x</h1>"))

    def test_a_named_table_yields_its_cells(self) -> None:
        page = body("JCL statements", '<table id="cjsts"><tr><td>// DD</td><td>DD</td></tr></table>')
        self.assertEqual(DOCS.table_cells(page.decode(), "cjsts"), ["// DD", "DD"])

    def test_an_absent_table_is_none_rather_than_an_empty_cell_list(self) -> None:
        # None means "the topic no longer carries that table", which is drift.
        # An empty list would read as "the table is there and has no rows".
        self.assertIsNone(DOCS.table_cells(body("JCL statements").decode(), "cjsts"))


class TableRowTests(unittest.TestCase):
    """The table is read as rows, because a row ordinal is what a locator cites."""

    def setUp(self) -> None:
        self.rows = DOCS.table_rows(statements().decode(), "idg6175__cjsts")

    def test_the_header_row_is_not_row_one(self) -> None:
        # Row 1 of the locator is the first row of DATA. Counting the header
        # would shift all 20 JCL statement rows by one and read as 20 findings.
        self.assertEqual(len(self.rows), 20)
        self.assertEqual(self.rows[0][:2], ["// command", "JCL command"])

    def test_each_row_keeps_its_own_cells(self) -> None:
        self.assertEqual(self.rows[SCHEDULE_ROW - 1][:2], ["// SCHEDULE", "schedule"])
        self.assertEqual(self.rows[SET_ROW - 1][:2], ["// SET", "set"])

    def test_an_absent_table_is_none_rather_than_no_rows(self) -> None:
        self.assertIsNone(DOCS.table_rows(body("JCL statements").decode(), "idg6175__cjsts"))


class CellComparisonTests(unittest.TestCase):
    """How a reviewed label is matched against a cell, and why not more loosely.

    A row ordinal only discriminates if the comparison does. Two waves gave the
    20 JCL rows an ordinal and read it, and the comparison underneath stayed
    token containment — is the label one of the cell's words — which the Purpose
    column of prose makes almost free to satisfy.
    """

    def setUp(self) -> None:
        self.rows = DOCS.table_rows(statements().decode(), "idg6175__cjsts")

    def ordinals(self, heading: str, match) -> list[int]:
        return [i + 1 for i, cells in enumerate(self.rows) if match(heading, cells)]

    def test_every_label_names_its_own_row_and_no_other(self) -> None:
        # THE property. Not "each label matches the row it cites", which token
        # containment also satisfied, but that no OTHER ordinal would have
        # accepted it — which is the only thing that makes a swap detectable
        # from either side.
        self.assertEqual(len(JCL_STATEMENT_LABELS), len(self.rows))
        for ordinal, heading in enumerate(JCL_STATEMENT_LABELS, 1):
            with self.subTest(row=ordinal, heading=heading):
                self.assertEqual(LOCATORS.rows_naming_heading(heading, self.rows), [ordinal])

    def test_token_containment_accepted_a_quarter_of_them_at_several_ordinals(self) -> None:
        # The defect, pinned to the helper that carried it, so that reverting to
        # it fails here rather than somewhere downstream. `JOB` is a word of
        # seven Purpose sentences; `SET` is one of `data set (PDS)` in INCLUDE's;
        # `COMMAND` is a word of row 1's `//  command` as well as row 2's.
        loose = {
            heading: self.ordinals(heading, DOCS.heading_in_cells)
            for heading in JCL_STATEMENT_LABELS
        }
        self.assertEqual(
            {heading: found for heading, found in loose.items() if len(found) > 1},
            {
                "COMMAND": [1, 2],
                "EXEC": [8, 12],
                "INCLUDE": [11, 12],
                "JOB": [8, 9, 10, 11, 13, 15, 18],
                "SET": [11, 19],
            },
        )

    def test_a_statement_printed_with_its_slashes_still_names_its_row(self) -> None:
        # The relaxation the coded column needs, and all it needs: the marker
        # comes off, the rest must be the label entire.
        self.assertTrue(LOCATORS.cell_names_heading("SCHEDULE", "//  SCHEDULE"))
        self.assertTrue(LOCATORS.cell_names_heading("comment", "//*  comment"))
        self.assertFalse(LOCATORS.cell_names_heading("SCHED", "//  SCHEDULE"))

    def test_the_coded_column_is_compared_with_its_letter_case(self) -> None:
        # The book distinguishes these two statements by case and by nothing
        # else. Folding it is what put `COMMAND` at two ordinals, and it is what
        # let half of a swap of rows 1 and 2 pass unreported.
        self.assertTrue(LOCATORS.cell_names_heading("COMMAND", "//  COMMAND"))
        self.assertFalse(LOCATORS.cell_names_heading("COMMAND", "//  command"))

    def test_the_prose_column_is_not(self) -> None:
        # `output JCL` against the label `OUTPUT JCL` is sentence-style
        # capitalization, not a distinction the book is drawing, and there is no
        # second Name cell for the fold to collide with.
        self.assertTrue(LOCATORS.cell_names_heading("OUTPUT JCL", "output JCL"))
        self.assertTrue(LOCATORS.cell_names_heading("JOB", "job"))

    def test_a_label_is_never_a_word_of_a_sentence(self) -> None:
        # Whole-cell equality is what excludes the Purpose column. Nothing in
        # the rule knows that Purpose is the third column.
        self.assertFalse(
            LOCATORS.cell_names_heading("JOB", "Marks the beginning of a job; assigns a "
                                               "name to the job.")
        )

    def test_a_marker_with_nothing_after_it_names_nothing(self) -> None:
        # The null statement is `//` and the delimiter is `/*`; both are named
        # by their Name cell, and neither coded cell may match an empty label or
        # fall through to a case-insensitive read of itself.
        for cell in ("//", "/*"):
            with self.subTest(cell=cell):
                self.assertFalse(LOCATORS.cell_names_heading("null", cell))
                self.assertFalse(LOCATORS.cell_names_heading("", cell))

    def test_prose_that_merely_begins_with_a_slash_is_not_read_as_coded(self) -> None:
        # The marker has to stand as its own token, or a cell like `/*ff` would
        # be stripped to `ff` and compared as though the book had coded it.
        self.assertTrue(LOCATORS.cell_names_heading("/*ff", "/*ff"))
        self.assertFalse(LOCATORS.cell_names_heading("ff", "/*ff"))


class TocTests(unittest.TestCase):
    def setUp(self) -> None:
        self.nodes = DOCS.toc_index(TOC)

    def test_the_tree_is_keyed_by_path_not_by_label(self) -> None:
        self.assertIn("SSEPEK_13.0.0/sqlref/src/tpc/db2z_sql_allocatecursor.html", self.nodes)

    def test_a_topic_filed_under_two_branches_keeps_both_identifiers(self) -> None:
        filed = self.nodes["SSEPEK_13.0.0/sqlref/src/tpc/db2z_sql_setaccel.html"]
        self.assertEqual(
            [node["topicId"] for node in filed],
            ["statements-set-current-accelerator", "accelerators-set-current-accelerator"],
        )

    def test_the_product_key_is_separable_from_the_rest_of_the_path(self) -> None:
        self.assertEqual(
            DOCS.without_product("SSLTBW_3.2.0/com.ibm.zos.v3r2.idai200/dalloc.htm"),
            "com.ibm.zos.v3r2.idai200/dalloc.htm",
        )

    def test_a_printed_book_chapter_number_is_separable_from_a_heading(self) -> None:
        self.assertEqual(DOCS.without_chapter_number("Chapter 4. ALLOCATE"), "ALLOCATE")
        self.assertEqual(DOCS.without_chapter_number("Chapter 31. ABS"), "ABS")

    def test_no_other_leading_word_is_stripped(self) -> None:
        # The relaxation is exactly as wide as the labels need and no wider.
        for heading in ("Chapter heading", "Appendix B. Codes", "PART parameter"):
            with self.subTest(heading=heading):
                self.assertEqual(DOCS.without_chapter_number(heading), heading)


class ComponentTests(unittest.TestCase):
    def test_every_component_of_a_locator_is_read(self) -> None:
        parts = LOCATORS.components(
            "topic:SSLTBW_3.2.0/com.ibm.zos.v3r2.ieab600/iea3b6_JCL_statements.htm"
            ";topic-id:statements-jcl;heading:JCL command;table:idg6175__cjsts"
        )
        self.assertEqual(parts["topic-id"], "statements-jcl")
        self.assertEqual(parts["heading"], "JCL command")
        self.assertEqual(parts["table"], "idg6175__cjsts")

    def test_a_semicolon_inside_a_heading_does_not_truncate_it(self) -> None:
        parts = LOCATORS.components("topic:a/b.html;topic-id:x;heading:DECLARE; THEN")
        self.assertEqual(parts["heading"], "DECLARE; THEN")

    def test_a_locator_that_names_no_topic_carries_no_topic_component(self) -> None:
        self.assertNotIn("topic", LOCATORS.components("html-table:dfha8mf__eibfn_table"))


class CheckTests(unittest.TestCase):
    def setUp(self) -> None:
        self.nodes = DOCS.toc_index(TOC)
        self.labels = LOCATORS.label_index(self.nodes)
        self.tails = LOCATORS.tail_index(self.nodes)

    def check(self, locator: str, retrieved: object) -> dict:
        return LOCATORS.check(row("r", locator), self.nodes, self.labels, self.tails, retrieved)

    def test_the_served_heading_and_the_published_identifier_agreeing_is_exact(self) -> None:
        result = self.check(
            "topic:SSEPEK_13.0.0/sqlref/src/tpc/db2z_sql_allocatecursor.html"
            ";topic-id:statements-allocate-cursor;heading:ALLOCATE CURSOR",
            body("ALLOCATE CURSOR"),
        )
        self.assertEqual(result["verdict"], "exact")
        self.assertEqual(result["matched_on"], "h1")

    def test_a_heading_the_tree_carries_but_the_topic_extends_is_exact(self) -> None:
        # Db2 labels the statement `ALLOCATE CURSOR` and heads the topic
        # `ALLOCATE CURSOR statement`; 154 rows rest on the table-of-contents
        # label rather than on the h1.
        result = self.check(
            "topic:SSEPEK_13.0.0/sqlref/src/tpc/db2z_sql_allocatecursor.html"
            ";topic-id:statements-allocate-cursor;heading:ALLOCATE CURSOR",
            body("ALLOCATE CURSOR statement"),
        )
        self.assertEqual(result["verdict"], "exact")
        self.assertEqual(result["matched_on"], "toc-label")

    def test_a_chapter_number_the_topic_tree_does_not_carry_is_reported_as_such(self) -> None:
        result = self.check(
            "topic:SSLTBW_3.2.0/com.ibm.zos.v3r2.idai200/dalloc.htm"
            ";topic-id:commands-allocate;heading:Chapter 4. ALLOCATE",
            body("ALLOCATE"),
        )
        self.assertEqual(result["verdict"], "exact")
        self.assertEqual(result["matched_on"], "h1-without-chapter-number")

    def test_a_second_branch_publishing_the_identifier_is_accepted(self) -> None:
        result = self.check(
            "topic:SSEPEK_13.0.0/sqlref/src/tpc/db2z_sql_setaccel.html"
            ";topic-id:statements-set-current-accelerator;heading:SET CURRENT ACCELERATOR",
            body("SET CURRENT ACCELERATOR"),
        )
        self.assertEqual(result["verdict"], "exact")

    def test_an_identifier_the_tree_does_not_publish_is_retitled(self) -> None:
        result = self.check(
            "topic:SSEPEK_13.0.0/sqlref/src/tpc/db2z_sql_allocatecursor.html"
            ";topic-id:statements-invented;heading:ALLOCATE CURSOR",
            body("ALLOCATE CURSOR"),
        )
        self.assertEqual(result["verdict"], "retitled")
        self.assertEqual(result["published_topic_id"], ["statements-allocate-cursor"])

    def test_a_heading_neither_the_topic_nor_the_tree_carries_is_retitled(self) -> None:
        # Retitled means the path still resolves. It is not `missing`, because
        # the row's topic is there — what changed is what it is called.
        result = self.check(
            "topic:SSEPEK_13.0.0/sqlref/src/tpc/db2z_sql_allocatecursor.html"
            ";topic-id:statements-allocate-cursor;heading:ALLOCATE CURSOR (old name)",
            body("ALLOCATE CURSOR"),
        )
        self.assertEqual(result["verdict"], "retitled")
        self.assertEqual(result["served_heading"], "ALLOCATE CURSOR")
        self.assertEqual(result["last_modified"], "2026-01-28")

    def test_a_row_citing_a_table_row_matches_a_cell_of_that_row(self) -> None:
        result = self.check(jcl_locator("SCHEDULE", SCHEDULE_ROW), statements())
        self.assertEqual(result["verdict"], "exact")
        self.assertEqual(result["matched_on"], "table-row")

    def test_swapping_two_statements_ordinals_is_caught(self) -> None:
        # THE finding. SCHEDULE and SET each cite the other's body row. Every
        # label is still somewhere in the table and every topic, topic-id and
        # heading still resolves, so a scan of the whole table calls both rows
        # exact and a full regeneration passes ten gates with the inventory
        # citing the wrong evidence for two of its rows.
        for heading, cited, truly in (
            ("SCHEDULE", SET_ROW, SCHEDULE_ROW),
            ("SET", SCHEDULE_ROW, SET_ROW),
        ):
            with self.subTest(heading=heading):
                result = self.check(jcl_locator(heading, cited), statements())
                self.assertEqual(result["verdict"], "retitled")
                self.assertEqual(result["matched_on"], "table-row")
                self.assertEqual(result["row"], cited)
                self.assertEqual(result["heading_found_in_rows"], [truly])

    def test_swapping_the_two_command_statements_is_caught_on_both_halves(self) -> None:
        # The pair the ordinal could not tell apart. `JCL command` at row 2 was
        # already caught; `COMMAND` at row 1 was not, because `command` is a word
        # of row 1's `//  command` cell, so a reviewer saw one anomaly and would
        # have corrected the wrong row. Both halves now report, and each names
        # the ordinal its label really sits at.
        for heading, cited, truly in (
            ("JCL command", COMMAND_ROW, JCL_COMMAND_ROW),
            ("COMMAND", JCL_COMMAND_ROW, COMMAND_ROW),
        ):
            with self.subTest(heading=heading):
                result = self.check(jcl_locator(heading, cited), statements())
                self.assertEqual(result["verdict"], "retitled")
                self.assertEqual(result["matched_on"], "table-row")
                self.assertEqual(result["row"], cited)
                self.assertEqual(result["heading_found_in_rows"], [truly])

    def test_the_unswapped_pairs_are_exact_against_the_same_table(self) -> None:
        # The control for the two tests above: the fixture is not one that fails
        # whatever ordinal it is given.
        for heading, cited in (
            ("SCHEDULE", SCHEDULE_ROW),
            ("SET", SET_ROW),
            ("JCL command", JCL_COMMAND_ROW),
            ("COMMAND", COMMAND_ROW),
        ):
            with self.subTest(heading=heading):
                self.assertEqual(
                    self.check(jcl_locator(heading, cited), statements())["verdict"], "exact"
                )

    def test_no_label_of_the_twenty_survives_being_moved_to_another_ordinal(self) -> None:
        # The swap tests generalized: every one of the 380 wrong pairings is
        # reported. This is what "the ordinal discriminates" means, and it is
        # cheap enough to assert exhaustively rather than on the two pairs
        # somebody happened to think of.
        for truly, heading in enumerate(JCL_STATEMENT_LABELS, 1):
            for cited in range(1, len(JCL_STATEMENT_LABELS) + 1):
                if cited == truly:
                    continue
                with self.subTest(heading=heading, cited=cited):
                    result = self.check(jcl_locator(heading, cited), statements())
                    self.assertEqual(result["verdict"], "retitled")
                    self.assertEqual(result["heading_found_in_rows"], [truly])

    def test_a_label_that_two_rows_answer_to_is_not_credited_to_either(self) -> None:
        # A publication that stops telling its own rows apart takes the
        # discriminator with it. Reporting `exact` because the cited row is one
        # of the two would credit the citation with a choice nothing made.
        doubled = list(JCL_STATEMENTS)
        doubled.append(("// SET", "set, superseded spelling", "See the SET statement."))
        result = self.check(jcl_locator("SET", SET_ROW), statements(rows=doubled))
        self.assertEqual(result["verdict"], "retitled")
        self.assertEqual(result["matched_on"], "table-row-ambiguous")
        self.assertEqual(result["heading_found_in_rows"], [SET_ROW, len(doubled)])

    def test_every_committed_jcl_statement_locator_resolves_against_the_table(self) -> None:
        # End to end, on the locators the tree actually carries rather than on
        # ones the test wrote for itself: all 20 read `exact` on `table-row`.
        catalog = json.loads(
            (REPOSITORY / "conformance/subsystems/coverage/catalogs/jcl-jes2.json").read_text(encoding="utf-8")
        )
        unit = next(u for u in catalog["units"] if u["id"] == "jcl-statements")
        self.assertEqual(len(unit["rows"]), 20)
        for entry in unit["rows"]:
            with self.subTest(row=entry["id"]):
                result = LOCATORS.check(
                    entry, self.nodes, self.labels, self.tails, statements()
                )
                self.assertEqual(result["verdict"], "exact", result)
                self.assertEqual(result["matched_on"], "table-row")

    def test_a_table_citation_with_no_row_ordinal_is_not_resolved(self) -> None:
        # Dropping the discriminator must not read as a pass. Without it the 20
        # rows are interchangeable, and a check that cannot tell them apart
        # should say so rather than answer the question it can still answer.
        result = self.check(jcl_locator("SCHEDULE", None), statements())
        self.assertEqual(result["verdict"], "retitled")
        self.assertEqual(result["matched_on"], "table-row-uncited")

    def test_a_row_ordinal_past_the_end_of_the_table_is_reported_as_such(self) -> None:
        # Drift in the publication, not in the locator: the table lost rows.
        result = self.check(jcl_locator("SCHEDULE", 99), statements())
        self.assertEqual(result["verdict"], "retitled")
        self.assertEqual(result["matched_on"], "table-row-absent")
        self.assertEqual(result["table_body_rows"], 20)

    def test_a_row_ordinal_that_is_not_a_row_number_is_refused(self) -> None:
        for ordinal in ("0", "-1", "SCHEDULE"):
            with self.subTest(ordinal=ordinal):
                result = self.check(jcl_locator("SCHEDULE", ordinal), statements())
                self.assertEqual(result["matched_on"], "table-row-malformed")

    def test_a_row_citing_a_table_the_topic_no_longer_has_is_retitled(self) -> None:
        result = self.check(jcl_locator("SCHEDULE", SCHEDULE_ROW), body("JCL statements"))
        self.assertEqual(result["verdict"], "retitled")
        self.assertEqual(result["matched_on"], "table-absent")

    def test_a_cited_table_is_never_settled_by_the_shared_topic_heading(self) -> None:
        # All 20 rows share one topic, so a heading or label that agreed with
        # the topic's own title would let any of them past the row check.
        result = self.check(jcl_locator("JCL statements", SET_ROW), statements())
        self.assertEqual(result["verdict"], "retitled")
        self.assertEqual(result["matched_on"], "table-row")

    def test_a_topic_that_is_gone_and_whose_heading_is_nowhere_is_missing(self) -> None:
        result = self.check(
            "topic:SSEPEK_13.0.0/sqlref/src/tpc/db2z_sql_invented.html"
            ";topic-id:statements-invented;heading:INVENTED STATEMENT",
            DOCS.NotFound("https://www.ibm.com/x"),
        )
        self.assertEqual(result["verdict"], "missing")

    def test_a_topic_that_is_gone_but_whose_heading_is_unique_elsewhere_moved(self) -> None:
        result = self.check(
            "topic:SSEPEK_13.0.0/sqlref/src/tpc/db2z_sql_oldpath.html"
            ";topic-id:statements-allocate-cursor;heading:ALLOCATE CURSOR",
            DOCS.NotFound("https://www.ibm.com/x"),
        )
        self.assertEqual(result["verdict"], "moved")
        self.assertEqual(
            result["found_topic_path"],
            "SSEPEK_13.0.0/sqlref/src/tpc/db2z_sql_allocatecursor.html",
        )

    def test_a_product_key_bump_reads_as_moved_rather_than_as_missing(self) -> None:
        # The z/OS 3.3 baseline republishes the same book under a new product
        # key. 573 rows reading `missing` would look like an inventory that had
        # rotted rather than a publication that had been renumbered.
        result = self.check(
            "topic:SSLTBW_3.3.0/com.ibm.zos.v3r2.idai200/dalloc.htm"
            ";topic-id:commands-allocate;heading:Chapter 4. ALLOCATE",
            DOCS.NotFound("https://www.ibm.com/x"),
        )
        self.assertEqual(result["verdict"], "moved")
        self.assertEqual(result["reason"], "product-key")

    def test_an_unreachable_endpoint_is_skipped_and_never_missing(self) -> None:
        # This is the distinction the whole tool turns on: an endpoint we could
        # not reach has told us nothing about the row.
        result = self.check(
            "topic:SSEPEK_13.0.0/sqlref/src/tpc/db2z_sql_allocatecursor.html"
            ";topic-id:statements-allocate-cursor;heading:ALLOCATE CURSOR",
            DOCS.Unreachable("https://www.ibm.com/x", "http-503"),
        )
        self.assertEqual(result["verdict"], "skipped")
        self.assertEqual(result["reason"], "endpoint-unreachable")

    def test_a_row_located_some_other_way_is_skipped_with_its_reason(self) -> None:
        result = self.check("html-table:dfha8mf__eibfn_table;eibfn:0602", None)
        self.assertEqual(result["verdict"], "skipped")
        self.assertEqual(result["reason"], "not-a-topic-locator")
        self.assertIn("locator", result)


class CatalogLocatorTests(unittest.TestCase):
    """What the committed catalogs actually cite, read offline from the tree."""

    def locators(self) -> list[tuple[str, str, dict[str, str]]]:
        found = []
        for path in sorted((REPOSITORY / "conformance/subsystems/coverage/catalogs").glob("*.json")):
            document = json.loads(path.read_text(encoding="utf-8"))
            for unit in document.get("units", []):
                for entry in unit["rows"]:
                    found.append(
                        (document["subsystem"], unit["id"],
                         LOCATORS.components(entry["source_locator"]))
                    )
        return found

    def test_every_topic_locator_citing_a_table_cites_a_row_of_it(self) -> None:
        # The verifier refuses to resolve a table with no ordinal, so a locator
        # written without one would report `retitled` rather than pass quietly.
        # This says the same thing where it is cheap to say: in the catalogs.
        for subsystem, unit, parts in self.locators():
            if "topic" in parts and "table" in parts:
                with self.subTest(subsystem=subsystem, unit=unit, heading=parts.get("heading")):
                    self.assertIn("row", parts)

    def test_the_jcl_statement_rows_cite_twenty_distinct_ordinals(self) -> None:
        cited = [
            int(parts["row"])
            for subsystem, unit, parts in self.locators()
            if subsystem == "jcl-jes2" and unit == "jcl-statements"
        ]
        self.assertEqual(sorted(cited), list(range(1, 21)))
        self.assertEqual(cited, sorted(cited))

    def test_the_fixture_carries_the_labels_the_catalog_actually_cites(self) -> None:
        # Without this, JCL_STATEMENT_LABELS could be edited into agreement with
        # the table and the uniqueness assertion would go on passing over labels
        # nothing in the tree cites. The list is the catalog's, in row order.
        cited = [
            parts["heading"]
            for subsystem, unit, parts in self.locators()
            if subsystem == "jcl-jes2" and unit == "jcl-statements"
        ]
        self.assertEqual(cited, JCL_STATEMENT_LABELS)


class DestinationTests(unittest.TestCase):
    def test_a_report_inside_the_repository_is_refused(self) -> None:
        # Unbound JSON under conformance/subsystems/coverage is what broke this branch once.
        for tool in (LOCATORS, PINS):
            with self.subTest(tool=tool.__name__):
                with self.assertRaises(ValueError):
                    tool.outside_repository(REPOSITORY / "conformance/subsystems/coverage/audit.json")

    def test_a_report_outside_the_repository_is_allowed(self) -> None:
        for tool in (LOCATORS, PINS):
            with self.subTest(tool=tool.__name__):
                self.assertTrue(tool.outside_repository(Path("/tmp/audit.json")).is_absolute())


#: The pinned body of the one topic the pin tests use. Pinned on 2026-09-03,
#: which is the date the whole Db2 book carries.
PINNED = body("CREATE VIEW", "the pinned text", updated="2026-09-03")


class PinTests(unittest.TestCase):
    MANIFEST = {
        "content_url_template": DOCS.CONTENT_URL,
        "topic_manifest_digest": DOCS.manifest_digest(
            [{"topic_path": "a/first.html", "sha256": DOCS.digest(PINNED)}]
        ),
        "topic_count": 1,
        "total_bytes": len(PINNED),
        "topics": [
            {
                "topic_path": "a/first.html",
                "sha256": DOCS.digest(PINNED),
                "bytes": len(PINNED),
                "last_modified": "2026-09-03",
            }
        ],
    }

    def verify(self, served, again=None, sample=None):
        """Run the whole book against one served body and one re-read body.

        `again` is what the second read of a mismatching topic returns; None
        means the test asserts no second read happens, and a second read that
        does happen fails loudly rather than silently returning the first body.
        """
        reads = []

        def reread(path, template, cache=None):
            reads.append(path)
            if again is None:
                raise AssertionError(f"unexpected re-read of {path}")
            if isinstance(again, Exception):
                raise again
            return again

        original_topics, original_topic = PINS.docs_api.topics, PINS.docs_api.topic
        PINS.docs_api.topics = lambda paths, *_: [(p, served) for p in paths]
        PINS.docs_api.topic = reread
        try:
            state = PINS.verify_topics(self.MANIFEST, None, sample, 1)
        finally:
            PINS.docs_api.topics = original_topics
            PINS.docs_api.topic = original_topic
        state["reads"] = reads
        return state

    def test_the_pin_is_the_digest_over_the_whole_book(self) -> None:
        state = self.verify(PINNED)
        self.assertEqual(state["status"], "match")
        self.assertTrue(state["matches_pin"])

    def test_a_book_that_matches_is_never_read_twice(self) -> None:
        self.assertEqual(self.verify(PINNED)["reads"], [])

    def test_a_mismatch_is_read_again_before_it_is_recorded(self) -> None:
        # Measured, not assumed: four full re-reads of the 832 Db2 topics
        # reported 1, 7, 1 and 6 changed and never named the same topic twice.
        # A digest that does not survive a second look is not evidence.
        state = self.verify(body("CREATE VIEW", "a stale build", updated="2026-01-07"), PINNED)
        self.assertEqual(state["reads"], ["a/first.html"])
        self.assertEqual(state["changed"][0]["resolution"], "stale-read")
        self.assertEqual(state["changed"][0]["resolved_by"], "the re-read reproduces the pin")
        self.assertEqual(state["status"], "stale-read")

    def test_a_stale_read_that_does_not_go_away_is_still_a_stale_read(self) -> None:
        # The state on 2026-09-07: db2z_sql_createview served the 2026-01-07
        # build, 10 bytes short of its pin, on 13 consecutive reads. The
        # re-read alone does not settle it, and the date does — republication
        # moves that date forward, so a body older than the pin is an older
        # build being served rather than a newer one being published.
        stale = body("CREATE VIEW", "a stale build", updated="2026-01-07")
        state = self.verify(stale, stale)
        self.assertEqual(state["changed"][0]["resolution"], "stale-read")
        self.assertEqual(state["changed"][0]["reread"]["last_modified"], "2026-01-07")
        self.assertEqual(state["status"], "stale-read")
        self.assertFalse(state["matches_pin"])

    def test_a_stale_read_reports_the_digest_that_was_actually_served(self) -> None:
        # The report says what this run retrieved. Substituting the pin because
        # a second read produced it would make the digest a claim about the
        # book rather than a record of the request.
        stale = body("CREATE VIEW", "a stale build", updated="2026-01-07")
        state = self.verify(stale, PINNED)
        self.assertEqual(state["changed"][0]["sha256"], DOCS.digest(stale))
        self.assertEqual(state["changed"][0]["reread"]["sha256"], DOCS.digest(PINNED))

    def test_a_republished_topic_is_reported_with_both_dates(self) -> None:
        served = body("CREATE VIEW", "changed", updated="2026-10-01")
        state = self.verify(served, served)
        self.assertEqual(state["changed"][0]["resolution"], "republished")
        self.assertEqual(state["changed"][0]["pinned_last_modified"], "2026-09-03")
        self.assertEqual(state["changed"][0]["last_modified"], "2026-10-01")
        self.assertEqual(state["status"], "differs")

    def test_the_same_date_over_different_bytes_gets_its_own_verdict(self) -> None:
        # Neither staleness nor republication explains this one, so it must not
        # be filed under either. It is the case that would mean the endpoint
        # changed under a date that did not move.
        served = body("CREATE VIEW", "different", updated="2026-09-03")
        state = self.verify(served, served)
        self.assertEqual(state["changed"][0]["resolution"], "same-date-different-bytes")
        self.assertEqual(state["status"], "differs")

    def test_a_difference_with_no_dates_to_compare_is_not_excused(self) -> None:
        served = b"<h1 class='topictitle1'>CREATE VIEW</h1>"
        state = self.verify(served, served)
        self.assertEqual(state["changed"][0]["resolution"], "undated-difference")
        self.assertEqual(state["status"], "differs")

    def test_a_re_read_that_cannot_be_made_falls_back_to_the_dates(self) -> None:
        stale = body("CREATE VIEW", "a stale build", updated="2026-01-07")
        state = self.verify(stale, DOCS.Unreachable("https://www.ibm.com/x", "http-503"))
        self.assertEqual(state["changed"][0]["reread"], {"reason": "http-503"})
        self.assertEqual(state["changed"][0]["resolution"], "stale-read")

    def test_only_an_unexplained_difference_counts_as_one(self) -> None:
        stale = body("CREATE VIEW", "a stale build", updated="2026-01-07")
        state = self.verify(stale, stale)
        self.assertEqual(state["topics_stale_read"], 1)
        self.assertEqual(state["topics_unexplained"], 0)
        self.assertEqual(state["resolutions"], {"stale-read": 1})

    def test_an_unreachable_book_is_skipped_rather_than_reported_as_drift(self) -> None:
        error = DOCS.Unreachable("https://www.ibm.com/x", "URLError")
        state = self.verify(error)
        self.assertEqual(state["status"], "skipped")
        self.assertEqual(state["topics_unreachable"], 1)

    def test_a_sampled_run_never_claims_the_pin_matched(self) -> None:
        state = self.verify(PINNED, sample=1)
        self.assertEqual(state["status"], "sampled")
        self.assertNotIn("matches_pin", state)


class DateTests(unittest.TestCase):
    def test_the_direction_of_the_date_is_what_separates_the_two_cases(self) -> None:
        self.assertEqual(DOCS.compare_dates("2026-09-03", "2026-01-07"), "older")
        self.assertEqual(DOCS.compare_dates("2026-09-03", "2026-10-01"), "newer")
        self.assertEqual(DOCS.compare_dates("2026-09-03", "2026-09-03"), "same")

    def test_anything_that_is_not_a_pair_of_dates_is_undated(self) -> None:
        for pinned, served in (
            (None, "2026-09-03"),
            ("2026-09-03", None),
            ("2026-09-03", "Last Updated"),
            ("", ""),
        ):
            with self.subTest(pinned=pinned, served=served):
                self.assertEqual(DOCS.compare_dates(pinned, served), "undated")


class ExitStatusTests(unittest.TestCase):
    """Which verdicts fail a run, stated once where a reader can find it."""

    def test_a_stale_read_is_not_a_failure_and_a_republication_is(self) -> None:
        self.assertNotIn(PINS.STALE_READ, PINS.UNEXPLAINED)
        for name in (PINS.REPUBLISHED, PINS.SAME_DATE, PINS.UNDATED):
            with self.subTest(name=name):
                self.assertIn(name, PINS.UNEXPLAINED)


if __name__ == "__main__":
    unittest.main()
