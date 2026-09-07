from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path
from types import ModuleType

TOOLS = Path(__file__).resolve().parents[1]


def load_module(name: str) -> ModuleType:
    path = TOOLS / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


GRAMMAR = load_module("extract_cobol_html_grammar")
COMPARE = load_module("compare_cobol_grammar")
FETCH = load_module("fetch_cobol_topics")


def svg(body: str) -> str:
    return (
        '<svg class="syntaxdiagram" xmlns="http://www.w3.org/2000/svg">'
        f"<g class='diagram'>{body}</g></svg>"
    )


def boxed(kind: str, value: str) -> str:
    return f"<g class='boxed syntax{kind}'><g class='text'><text class='syntax{kind}'>{value}</text></g></g>"


KEYWORD = boxed("kwd", "ADD")
OPERAND = boxed("var", "identifier-1")


class StructureTests(unittest.TestCase):
    def test_boxed_classes_split_keywords_from_operands(self) -> None:
        item = GRAMMAR.form("Format 1", [svg(KEYWORD + OPERAND)])
        self.assertEqual(
            item["main_line"],
            [
                {"kind": "keyword", "value": "ADD"},
                {"kind": "operand", "value": "identifier-1"},
            ],
        )

    def test_a_choice_keeps_its_first_branch_on_the_main_line(self) -> None:
        body = f"<g class='groupchoice'>{OPERAND}{boxed('var', 'literal-1')}</g>"
        item = GRAMMAR.form("Format 1", [svg(body)])
        self.assertEqual([entry["value"] for entry in item["main_line"]], ["identifier-1"])
        self.assertEqual(
            item["branches"],
            [{"kind": "operand", "value": "literal-1", "relation": "alternative"}],
        )

    def test_a_text_free_sibling_marks_its_group_optional(self) -> None:
        body = f"<g class=''><g class=''></g>{boxed('kwd', 'ROUNDED')}</g>"
        item = GRAMMAR.form("Format 1", [svg(body)])
        self.assertEqual(item["main_line"], [])
        self.assertEqual(
            item["branches"],
            [{"kind": "keyword", "value": "ROUNDED", "relation": "optional"}],
        )

    def test_an_outer_optional_survives_a_nested_choice(self) -> None:
        inner = f"<g class='groupchoice'>{boxed('kwd', 'ON')}{boxed('kwd', 'OFF')}</g>"
        body = f"<g class=''><g class=''></g>{inner}</g>"
        item = GRAMMAR.form("Format 1", [svg(body)])
        self.assertTrue(all(entry["relation"] == "optional" for entry in item["branches"]))

    def test_a_sequence_without_a_bypass_stays_on_the_main_line(self) -> None:
        body = f"<g class='groupseq'>{KEYWORD}{OPERAND}</g>"
        item = GRAMMAR.form("Format 1", [svg(body)])
        self.assertEqual(item["branches"], [])
        self.assertEqual(len(item["main_line"]), 2)

    def test_multi_line_labels_are_normalized(self) -> None:
        item = GRAMMAR.form("Format 1", [svg(boxed("kwd", "SIZE\nERROR"))])
        self.assertEqual(item["main_line"][0]["value"], "SIZE ERROR")

    def test_a_fragment_reference_is_neither_keyword_nor_operand(self) -> None:
        body = "<a class='boxed fragref'><g class='text'><text>when-phrase</text></g></a>"
        item = GRAMMAR.form("Format 1", [svg(body)])
        self.assertEqual(item["main_line"], [{"kind": "fragment", "value": "when-phrase"}])


class DocumentTests(unittest.TestCase):
    def test_a_title_delimits_its_diagram(self) -> None:
        document = (
            '<h3 class="syntaxdiagram-title">Format 1: ADD</h3>' + svg(KEYWORD)
            + '<h3 class="syntaxdiagram-title">Format 2: ADD</h3>' + svg(OPERAND)
        )
        found = GRAMMAR.diagrams(document)
        self.assertEqual([title for title, _ in found], ["Format 1: ADD", "Format 2: ADD"])
        self.assertEqual([len(pieces) for _, pieces in found], [1, 1])

    def test_a_wrapped_diagram_joins_its_pieces_instead_of_counting_them(self) -> None:
        document = (
            '<h3 class="syntaxdiagram-title">Format 1: SORT</h3>' + svg(KEYWORD) + svg(OPERAND)
        )
        found = GRAMMAR.diagrams(document)
        self.assertEqual(len(found), 1)
        item = GRAMMAR.form(*found[0])
        self.assertEqual(len(item["main_line"]), 2)

    def test_a_container_with_extra_attributes_is_still_found(self) -> None:
        document = (
            '<div class="syntaxdiagram" data-hd-platform="hst">'
            '<h3 class="syntaxdiagram-title">Format 1: SORT</h3>' + svg(KEYWORD) + "</div>"
        )
        self.assertEqual(len(GRAMMAR.diagrams(document)), 1)

    def test_an_unrelated_svg_is_not_a_diagram(self) -> None:
        document = '<svg class="icon"><g class="boxed syntaxkwd"><text>NO</text></g></svg>'
        self.assertEqual(GRAMMAR.diagrams(document), [])

    def test_html_entities_do_not_defeat_the_parser(self) -> None:
        item = GRAMMAR.form("Format 1", [svg(boxed("kwd", "A&nbsp;B"))])
        self.assertEqual(item["main_line"][0]["value"], "A B")

    def test_phrase_diagrams_are_separated_from_statement_formats(self) -> None:
        self.assertEqual(GRAMMAR.kind_of("Format 1: ADD statement"), "format")
        self.assertEqual(GRAMMAR.kind_of("when-phrase Format"), "fragment")
        self.assertEqual(GRAMMAR.kind_of("converting-phrase Format 1"), "fragment")


class ProjectionTests(unittest.TestCase):
    ROW = {
        "id": "json-parse",
        "row_id": "row:0024",
        "label": "JSON PARSE statement",
        "forms": ["JSON PARSE identifier-1 INTO identifier-2"],
    }

    def test_format_titles_leaves_out_the_phrase_fragments(self) -> None:
        document = (
            '<h3 class="syntaxdiagram-title">Format</h3>'
            + svg(KEYWORD)
            + '<h3 class="syntaxdiagram-title">when-phrase Format</h3>'
            + svg(boxed("kwd", "WHEN"))
        )
        excluded: dict[tuple[str, str], int] = {}
        result = GRAMMAR.project(
            self.ROW, [("JSON PARSE statement", "any.html", document)], excluded
        )
        self.assertEqual(
            [item["title"] for item in result["forms"]], ["Format", "when-phrase Format"]
        )
        self.assertEqual(result["format_titles"], ["Format"])

    def test_a_declared_non_syntax_diagram_is_dropped(self) -> None:
        path, title = next(iter(GRAMMAR.NON_SYNTAX.items()))
        excluded: dict[tuple[str, str], int] = {}
        result = GRAMMAR.project(
            self.ROW, [(title, path, svg(boxed("kwd", "[0-9]")))], excluded
        )
        self.assertEqual(result["forms"], [])
        self.assertEqual(excluded, {(path, title): 1})

    def test_a_non_syntax_exclusion_needs_the_title_as_well_as_the_path(self) -> None:
        path = next(iter(GRAMMAR.NON_SYNTAX))
        document = '<h3 class="syntaxdiagram-title">Format</h3>' + svg(KEYWORD)
        excluded: dict[tuple[str, str], int] = {}
        result = GRAMMAR.project(self.ROW, [("Format", path, document)], excluded)
        self.assertEqual([item["title"] for item in result["forms"]], ["Format"])
        self.assertEqual(excluded, {})


class TopicTests(unittest.TestCase):
    LOCATOR = (
        "topic:SS6SG3_6.5/lr/ref/rlpsadd.html"
        ";topic-id:statements-add-statement;heading:ADD statement"
    )

    def test_the_catalog_locator_supplies_the_topic_and_the_heading(self) -> None:
        row = {"id": "add", "source_locator": self.LOCATOR}
        self.assertEqual(FETCH.topic_path(row), "SS6SG3_6.5/lr/ref/rlpsadd.html")
        self.assertEqual(FETCH.heading(row), "ADD statement")

    def test_a_retired_page_locator_is_refused_rather_than_guessed(self) -> None:
        row = {"id": "add", "source_locator": "page:343;outline:ADD statement"}
        with self.assertRaises(ValueError):
            FETCH.topic_path(row)

    def test_the_table_of_contents_is_keyed_by_path_not_by_heading(self) -> None:
        toc = {"toc": {"label": "book", "href": "b.html", "topics": [
            {"label": "DELETE statement", "href": "SS6SG3_6.5/lr/ref/rlpsdele.html"},
            {"label": "DELETE statement", "href": "SS6SG3_6.5/lr/ref/rlcdsdel.html"},
        ]}}
        nodes = FETCH.locate(toc)
        row = {"id": "delete", "source_locator": "topic:SS6SG3_6.5/lr/ref/rlcdsdel.html"
               ";topic-id:statements-delete;heading:DELETE statement"}
        self.assertEqual(nodes[FETCH.topic_path(row)]["href"],
                         "SS6SG3_6.5/lr/ref/rlcdsdel.html")

    def test_a_statement_subtree_includes_its_format_topics(self) -> None:
        node = {
            "label": "SET statement",
            "href": "a.html",
            "topics": [
                {"label": "Format 1", "href": "b.html"},
                {"label": "Format 2", "href": "c.html", "topics": [
                    {"label": "Format 2 detail", "href": "d.html"}
                ]},
            ],
        }
        self.assertEqual(
            [label for label, _ in FETCH.subtree(node)],
            ["SET statement", "Format 1", "Format 2", "Format 2 detail"],
        )


class ComparisonTests(unittest.TestCase):
    def row(self) -> dict[str, object]:
        form = {
            "title": "Format 1: ADD statement",
            "kind": "format",
            "main_line": [
                {"kind": "keyword", "value": "ADD"},
                {"kind": "operand", "value": "identifier-1"},
            ],
            "branches": [
                {"kind": "keyword", "value": "ROUNDED", "relation": "optional"},
                {
                    "kind": "operand",
                    "value": "literal-1",
                    "relation": "alternative",
                    "alternative_to": "identifier-1",
                },
            ],
        }
        return {
            "id": "add",
            "row_id": "row:0002",
            "title": "ADD statement",
            "forms": [form],
            "format_titles": ["Format 1: ADD statement"],
            "catalog_forms": ["ADD operands TO targets"],
        }

    def test_source_keywords_absent_from_the_catalog_are_reported(self) -> None:
        result = COMPARE.compare(self.row())
        self.assertEqual(result["keywords_missing_from_catalog"], ["ROUNDED"])
        self.assertEqual(result["keywords_absent_from_source"], ["TO"])

    def test_catalog_placeholders_are_separated_from_source_operands(self) -> None:
        result = COMPARE.compare(self.row())
        self.assertEqual(result["catalog_placeholders"], ["operands", "targets"])
        self.assertEqual(result["source_operands"], ["identifier-1", "literal-1"])

    def test_branch_relations_are_projected_separately(self) -> None:
        result = COMPARE.compare(self.row())
        self.assertEqual(result["alternatives"], ["literal-1"])
        self.assertEqual(result["optionals"], ["ROUNDED"])

    def test_phrase_fragments_are_not_counted_against_the_catalog_forms(self) -> None:
        row = self.row()
        row["forms"].append(
            {
                "title": "when-phrase Format",
                "kind": "fragment",
                "main_line": [{"kind": "keyword", "value": "WHEN"}],
                "branches": [],
            }
        )
        result = COMPARE.compare(row)
        self.assertEqual(result["source_diagram_count"], 2)
        self.assertEqual(result["source_format_count"], 1)
        self.assertEqual(result["source_fragment_count"], 1)
        # A keyword the publication only reaches through a phrase fragment is
        # still a keyword of the statement.
        self.assertIn("WHEN", result["keywords_missing_from_catalog"])

    def test_fragment_placeholders_are_not_counted_as_keywords(self) -> None:
        row = self.row()
        row["forms"][0]["branches"].append(
            {"kind": "keyword", "value": "PHRASE 1", "relation": "optional"}
        )
        result = COMPARE.compare(row)
        self.assertNotIn("PHRASE 1", result["keywords_missing_from_catalog"])


if __name__ == "__main__":
    unittest.main()
