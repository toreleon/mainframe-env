from __future__ import annotations

import importlib.util
import json
import os
import unittest
from pathlib import Path
from types import ModuleType

TOOLS = Path(__file__).resolve().parents[1]
REPOSITORY = TOOLS.parents[3]
MANIFEST = REPOSITORY / "conformance/subsystems/jcl/generated/jcl-topic-manifest.json"
CATALOG = REPOSITORY / "conformance/subsystems/coverage/catalogs/jcl-jes2.json"


def load_module(name: str) -> ModuleType:
    path = TOOLS / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


JCL = load_module("extract_jcl_html_parameters")

#: The counts the catalog records, and the whole point of the depth rule.
EXPECTED = {
    "dd-parameters": 74,
    "exec-parameters": 19,
    "job-parameters": 35,
    "output-parameters": 76,
}


def node(label: str, href: str, *topics: dict[str, object]) -> dict[str, object]:
    return {"label": label, "href": href, "topicId": href.rsplit("/", 1)[-1], "topics": list(topics)}


def labelled(node: dict[str, object], depth: int = 0):
    """Every (depth, label) at or below a node, counted from that node."""
    yield depth, JCL.label_of(node)
    for child in JCL.children(node):
        yield from labelled(child, depth + 1)


# A table of contents shaped like the real one: a book node found by href, a
# childless duplicate of that href beside it, chapters with parameter children,
# `Syntax` and `Subparameter definition` grandchildren that no reader could
# take for a parameter whatever depth it reads at, and -- the actual trap --
# the deeper topics that DO end in the word `parameter`, which a descendant
# walk would report as three more of them.
BOOK_HREF = "P/c/abstract.htm"
TOC = {
    "toc": node(
        "product",
        "P/c/root.htm",
        node(
            "z/OS MVS JCL Reference",
            BOOK_HREF,
            node("Abstract", BOOK_HREF + "?pos=2"),
            node(
                "Chapter 12. DD statement",
                "P/c/ddst.htm",
                node("Description", "P/c/desc.htm"),
                node(
                    "ACCODE parameter",
                    "P/c/xddaccod.htm",
                    node("Syntax", "P/c/Syntax1.htm"),
                    node(
                        "Subparameter definition",
                        "P/c/Sub1.htm",
                        node("Effect of DCB=dsname parameter", "P/c/Eff1.htm"),
                    ),
                    node("Overrides", "P/c/Over1.htm"),
                    node("Relationship to other parameters", "P/c/Rel1.htm"),
                ),
                node(
                    "AMP parameter",
                    "P/c/xddamp.htm",
                    node("Examples of the AMP parameter", "P/c/Ex1.htm"),
                ),
            ),
            node(
                "Chapter 16. EXEC statement",
                "P/c/execst.htm",
                node("Description", "P/c/desc2.htm"),
                node("ACCT parameter", "P/c/xexacct.htm"),
                node("PROC and procedure name parameters", "P/c/xexproc.htm"),
            ),
            node("Comment statement", "P/c/comtst.htm"),
            node("IF/THEN/ELSE/ENDIF statement construct", "P/c/ifuse.htm"),
            node("XMIT JCL statement", "P/c/xmitst.htm"),
            node(
                "JES2 control statements",
                "P/c/j2st.htm",
                node("Description", "P/c/desc3.htm"),
                node("/*JOBPARM statement", "P/c/xj2jobpm.htm"),
                node("/*MESSAGE statement", "P/c/xj2mess.htm"),
            ),
        ),
    )
}
BOOK = TOC["toc"]["topics"][0]


class BookTests(unittest.TestCase):
    def test_the_book_is_found_by_href_and_not_by_label(self) -> None:
        self.assertIs(JCL.book(TOC, BOOK_HREF), BOOK)

    def test_the_childless_duplicate_of_the_href_is_not_the_book(self) -> None:
        found = JCL.book(TOC, BOOK_HREF)
        self.assertEqual(JCL.label_of(found), "z/OS MVS JCL Reference")
        self.assertEqual(len(JCL.children(found)), 7)

    def test_a_missing_book_fails_closed(self) -> None:
        with self.assertRaises(ValueError):
            JCL.book(TOC, "P/c/nowhere.htm")


class ChapterTests(unittest.TestCase):
    def test_a_chapter_is_titled_as_the_tree_publishes_it(self) -> None:
        found = JCL.chapters(BOOK)
        self.assertIn("DD statement", found)
        self.assertIn("EXEC statement", found)

    def test_a_missing_chapter_is_absent_rather_than_guessed(self) -> None:
        self.assertNotIn("JOB statement", JCL.chapters(BOOK))


class ParameterTests(unittest.TestCase):
    def test_only_direct_children_are_collected(self) -> None:
        chapter = JCL.chapters(BOOK)["DD statement"]
        names = [item["name"] for item in JCL.parameters(chapter)]
        self.assertEqual(names, ["ACCODE", "AMP"])

    def test_plural_parameter_titles_are_accepted(self) -> None:
        chapter = JCL.chapters(BOOK)["EXEC statement"]
        names = [item["name"] for item in JCL.parameters(chapter)]
        self.assertEqual(names, ["ACCT", "PROC AND PROCEDURE NAME"])

    def test_repeated_names_are_recorded_once(self) -> None:
        chapter = dict(JCL.chapters(BOOK)["DD statement"])
        chapter["topics"] = list(chapter["topics"]) + [
            node("ACCODE parameter", "P/c/xddaccod2.htm")
        ]
        names = [item["name"] for item in JCL.parameters(chapter)]
        self.assertEqual(names.count("ACCODE"), 1)

    def test_a_parameter_carries_the_topic_it_was_taken_from(self) -> None:
        chapter = JCL.chapters(BOOK)["DD statement"]
        self.assertEqual(JCL.parameters(chapter)[0]["topic_path"], "P/c/xddaccod.htm")


class DepthTrapTests(unittest.TestCase):
    r"""The depth rule, asserted with labels that can actually break it.

    What stood here asserted that `SYNTAX`, `SUBPARAMETER DEFINITION` and
    `OVERRIDES` are absent from `parameters()`. They are -- and they are absent
    from a reader that walks the whole subtree too, because `PARAMETER` is
    `^(.+?)\s+parameters?$` and no section heading in this book ends in the
    word. The assertion could not fail under the regression it was named for,
    so it read as protection and was none.

    The topics a descendant walk really admits are the cross-references and the
    examples, which do end in the word: 91 `Relationship to other parameters`
    across the four chapters, `Examples of the AMP parameter`, and at level 3
    `Effect of DCB=dsname parameter`. Against the served tree that reports 424
    parameters rather than 204, of which 150 are DD -- not the 451 that is just
    the DD subtree's level-2 node count. All three shapes are in the fixture,
    and the second test below is what keeps them there.
    """

    def test_a_deeper_topic_ending_in_the_word_is_not_a_parameter(self) -> None:
        chapter = JCL.chapters(BOOK)["DD statement"]
        names = [item["name"] for item in JCL.parameters(chapter)]
        self.assertNotIn("RELATIONSHIP TO OTHER", names)
        self.assertNotIn("EXAMPLES OF THE AMP", names)
        self.assertNotIn("EFFECT OF DCB=DSNAME", names)
        self.assertEqual(len(names), 2)

    def test_the_fixture_still_carries_what_makes_that_a_test(self) -> None:
        chapter = JCL.chapters(BOOK)["DD statement"]
        below = sorted(
            (depth, label)
            for depth, label in labelled(chapter)
            if depth > 1 and JCL.PARAMETER.match(label)
        )
        self.assertEqual(
            below,
            [
                (2, "Examples of the AMP parameter"),
                (2, "Relationship to other parameters"),
                (3, "Effect of DCB=dsname parameter"),
            ],
        )

    def test_a_section_heading_could_not_have_matched_at_any_depth(self) -> None:
        for heading in ("Syntax", "Subparameter definition", "Defaults", "Overrides"):
            self.assertIsNone(JCL.PARAMETER.match(heading), heading)


class SectionTests(unittest.TestCase):
    def test_syntax_and_subparameter_children_are_separated(self) -> None:
        chapter = JCL.chapters(BOOK)["DD statement"]
        parameter = JCL.children(chapter)[1]
        found = JCL.sections(parameter)
        self.assertEqual(found["syntax"], ["P/c/Syntax1.htm"])
        self.assertEqual(found["subparameter"], ["P/c/Sub1.htm"])

    def test_a_parameter_without_sections_reports_none(self) -> None:
        chapter = JCL.chapters(BOOK)["EXEC statement"]
        parameter = JCL.children(chapter)[1]
        self.assertEqual(JCL.sections(parameter), {"syntax": [], "subparameter": []})


class StatementTests(unittest.TestCase):
    def test_a_label_resolves_by_adding_statement_case_folded(self) -> None:
        found = JCL.statements(BOOK, ["comment", "DD"])
        self.assertEqual(
            [item["topic_path"] for item in found], ["P/c/comtst.htm", "P/c/ddst.htm"]
        )

    def test_the_two_hand_rules_resolve_by_topic(self) -> None:
        found = JCL.statements(BOOK, ["IF/THEN/ELSE/ENDIF", "XMIT"])
        self.assertEqual([item["rule"] for item in found], ["topic", "topic"])
        self.assertEqual(
            [item["topic_path"] for item in found], ["P/c/ifuse.htm", "P/c/xmitst.htm"]
        )

    def test_an_unresolved_label_fails_closed(self) -> None:
        with self.assertRaises(ValueError):
            JCL.statements(BOOK, ["NOSUCH"])

    def test_the_jecl_chapter_drops_its_description_section(self) -> None:
        found = JCL.jecl_statements(BOOK, "P/c/j2st.htm")
        self.assertEqual(
            [item["label"] for item in found], ["/*JOBPARM statement", "/*MESSAGE statement"]
        )

    def test_a_missing_jecl_chapter_fails_closed(self) -> None:
        with self.assertRaises(ValueError):
            JCL.jecl_statements(BOOK, "P/c/nowhere.htm")


class BodyTests(unittest.TestCase):
    def test_a_code_block_keeps_its_column_layout(self) -> None:
        body = (
            '<pre class="codeblock"><code>DISP= ( [NEW] [,DELETE ] )\n'
            "        [OLD] [,KEEP   ]  \n</code></pre>"
        )
        self.assertEqual(
            JCL.codeblocks(body),
            ["DISP= ( [NEW] [,DELETE ] )\n        [OLD] [,KEEP   ]"],
        )

    def test_a_topic_without_a_code_block_yields_nothing(self) -> None:
        self.assertEqual(JCL.codeblocks("<p>CCSID= nnnnn</p>"), [])

    def test_only_outermost_definition_terms_are_subparameters(self) -> None:
        body = (
            "<dl><dt>ERASE|NOERASE</dt><dd><dl><dt>ERASE</dt><dd>x</dd>"
            "<dt>NOERASE</dt><dd>y</dd></dl></dd><dt>access-code</dt><dd>z</dd></dl>"
        )
        terms, recognised = JCL.subparameters(body)
        self.assertEqual(terms, ["ERASE|NOERASE", "access-code"])
        self.assertEqual(recognised, ["ERASE", "NOERASE"])

    def test_a_table_reports_its_body_rows(self) -> None:
        body = '<table id="t"><tr><th>a</th></tr><tr><td>b</td></tr></table>'
        self.assertEqual(JCL.table_body_rows(body, "t"), 1)

    def test_an_absent_table_is_none_rather_than_zero(self) -> None:
        self.assertIsNone(JCL.table_body_rows("<p>x</p>", "t"))


class NormalizeTests(unittest.TestCase):
    def test_the_catalog_suffix_is_stripped(self) -> None:
        self.assertEqual(JCL.normalize("ACCODE parameter"), "ACCODE")
        self.assertEqual(
            JCL.normalize("PROC and procedure name parameters"),
            "PROC AND PROCEDURE NAME",
        )

    def test_a_label_without_the_suffix_is_unchanged(self) -> None:
        self.assertEqual(JCL.normalize("SCHEDULE"), "SCHEDULE")


class CatalogTests(unittest.TestCase):
    def test_a_missing_unit_fails_closed(self) -> None:
        catalog = {"units": [{"id": "dd-parameters", "rows": []}]}
        with self.assertRaises(ValueError):
            JCL.catalog_names(catalog, "exec-parameters")

    def test_catalog_labels_are_normalized(self) -> None:
        catalog = {
            "units": [
                {"id": "dd-parameters", "rows": [{"label": "ACCODE parameter"}]}
            ]
        }
        self.assertEqual(JCL.catalog_names(catalog, "dd-parameters"), ["ACCODE"])


class ManifestTests(unittest.TestCase):
    """The counts, asserted against the committed manifest rather than the TOC.

    The table of contents is 45 MB of publication bytes and lives outside the
    repository, so these run against what the fetch tool recorded. A reader
    that walked descendants would have written 150 DD parameter topics here,
    and 424 in total against the 204 the catalog carries.
    """

    @classmethod
    def setUpClass(cls) -> None:
        cls.manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
        cls.catalog = json.loads(CATALOG.read_text(encoding="utf-8"))

    def counts(self, role: str) -> dict[str, int]:
        found: dict[str, int] = {}
        for topic in self.manifest["topics"]:
            if topic["role"] == role:
                found[topic["unit"]] = found.get(topic["unit"], 0) + 1
        return found

    def test_each_unit_records_its_catalog_count_of_parameter_topics(self) -> None:
        self.assertEqual(self.counts("parameter"), EXPECTED)

    def test_the_parameter_denominator_is_two_hundred_and_four(self) -> None:
        self.assertEqual(sum(EXPECTED.values()), 204)
        self.assertEqual(self.manifest["role_counts"]["parameter"], 204)

    def test_the_statement_rosters_are_twenty_and_thirteen(self) -> None:
        self.assertEqual(
            self.counts("statement"), {"jcl-statements": 20, "jes2-jecl-statements": 13}
        )

    def test_the_sections_are_grandchildren_and_are_counted_apart(self) -> None:
        self.assertEqual(self.manifest["role_counts"]["syntax"], 199)
        self.assertEqual(self.manifest["role_counts"]["subparameter-definition"], 188)

    def test_every_topic_is_inside_the_pinned_baseline_at_the_pinned_digest(self) -> None:
        self.assertTrue(self.manifest["pinned_baseline"]["subset_of_pin"])
        self.assertEqual(self.manifest["pinned_baseline"]["absent_from_pin"], [])
        self.assertEqual(self.manifest["pinned_baseline"]["differing_from_pin"], [])

    def test_the_manifest_digest_follows_from_its_own_topic_list(self) -> None:
        import sys

        sys.path.insert(0, str(TOOLS.parents[2] / "tools"))
        import docs_api

        self.assertEqual(
            self.manifest["topic_manifest_digest"],
            docs_api.manifest_digest(self.manifest["topics"]),
        )

    def test_the_manifest_carries_no_publication_prose(self) -> None:
        for topic in self.manifest["topics"]:
            self.assertNotIn("<", json.dumps(topic))


class TableOfContentsTests(unittest.TestCase):
    """The same counts taken from the served tree, when a copy is on hand.

    `JCL_TOC` names a saved table of contents outside the repository. Without
    it these skip rather than pass vacuously: the manifest assertions above are
    what run unconditionally.
    """

    @classmethod
    def setUpClass(cls) -> None:
        path = os.environ.get("JCL_TOC")
        if not path or not Path(path).is_file():
            raise unittest.SkipTest("set JCL_TOC to a saved table of contents")
        cls.book = JCL.book(json.loads(Path(path).read_text(encoding="utf-8")))
        cls.catalog = json.loads(CATALOG.read_text(encoding="utf-8"))

    def test_direct_children_give_the_catalog_counts(self) -> None:
        found = JCL.chapters(self.book)
        counts = {
            unit: len(JCL.parameters(found[chapter])) for unit, chapter in JCL.UNITS.items()
        }
        self.assertEqual(counts, EXPECTED)

    def test_the_dd_subtree_is_deeper_than_the_reader_looks(self) -> None:
        chapter = JCL.chapters(self.book)["DD statement"]
        self.assertEqual(
            JCL.subtree_size(chapter), {"0": 1, "1": 75, "2": 451, "3": 39}
        )
        self.assertEqual(len(JCL.parameters(chapter)), 74)

    def test_the_source_order_matches_the_catalog_order(self) -> None:
        found = JCL.chapters(self.book)
        for unit, chapter in JCL.UNITS.items():
            names = [item["name"] for item in JCL.parameters(found[chapter])]
            self.assertEqual(names, JCL.catalog_names(self.catalog, unit), unit)


if __name__ == "__main__":
    unittest.main()
