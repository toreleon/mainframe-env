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


GRAMMAR = load_module("extract_cobol_pdf_grammar")
COMPARE = load_module("compare_cobol_grammar")


class Stream:
    """Minimal /ToUnicode stand-in for CMap parsing."""

    def __init__(self, data: bytes) -> None:
        self._data = data

    def get_object(self) -> "Stream":
        return self

    def get_data(self) -> bytes:
        return self._data


def token(text: str, x: float, y: float, italic: bool = False) -> dict[str, object]:
    return {
        "text": text,
        "x": x,
        "y": y,
        "x_end": x + 6.0 * len(text),
        "size": GRAMMAR.DIAGRAM_SIZE,
        "italic": italic,
        "bold": False,
    }


class CMapTests(unittest.TestCase):
    def test_bfchar_entries_are_decoded(self) -> None:
        data = (
            b"begincmap 2 beginbfchar\n<0003><0041>\n<0004><0042>\nendbfchar\nendcmap"
        )
        table = GRAMMAR.to_unicode({"/ToUnicode": Stream(data)})
        self.assertEqual(table, {3: "A", 4: "B"})

    def test_bfrange_entries_expand_across_the_range(self) -> None:
        data = b"beginbfrange\n<0010><0012><0041>\nendbfrange"
        table = GRAMMAR.to_unicode({"/ToUnicode": Stream(data)})
        self.assertEqual(table, {16: "A", 17: "B", 18: "C"})

    def test_bfrange_arrays_map_each_code_separately(self) -> None:
        data = b"beginbfrange\n<0020><0021>[<0058><005A>]\nendbfrange"
        table = GRAMMAR.to_unicode({"/ToUnicode": Stream(data)})
        self.assertEqual(table, {32: "X", 33: "Z"})

    def test_a_font_without_a_cmap_yields_no_table(self) -> None:
        self.assertEqual(GRAMMAR.to_unicode({}), {})


class LayoutTests(unittest.TestCase):
    def test_rows_group_tokens_sharing_a_baseline(self) -> None:
        rows = GRAMMAR.rows_of(
            [token("ADD", 10, 100), token("TO", 60, 100), token("ROUNDED", 40, 85)]
        )
        self.assertEqual([[item["text"] for item in row] for row in rows],
                         [["ADD", "TO"], ["ROUNDED"]])

    def test_a_wide_vertical_gap_starts_a_new_rail_line(self) -> None:
        rows = GRAMMAR.rows_of(
            [token("ADD", 10, 200), token("ROUNDED", 10, 188), token("SIZE", 10, 150)]
        )
        lines = GRAMMAR.rail_lines(rows)
        self.assertEqual(len(lines), 2)
        self.assertEqual(lines[0][0][0]["text"], "ADD")
        self.assertEqual(lines[1][0][0]["text"], "SIZE")

    def test_a_token_under_a_main_item_is_an_alternative(self) -> None:
        main = [token("CORRESPONDING", 50, 100)]
        entry = GRAMMAR.attach(main, token("CORR", 52, 88))
        self.assertEqual(entry["relation"], "alternative")
        self.assertEqual(entry["alternative_to"], "CORRESPONDING")

    def test_a_token_in_a_main_line_gap_is_optional(self) -> None:
        main = [token("ADD", 10, 100)]
        entry = GRAMMAR.attach(main, token("ROUNDED", 300, 88))
        self.assertEqual(entry["relation"], "optional")
        self.assertNotIn("alternative_to", entry)

    def test_operand_style_selects_the_operand_kind(self) -> None:
        self.assertEqual(
            GRAMMAR.node(token("identifier-1", 0, 0, italic=True)),
            {"kind": "operand", "value": "identifier-1"},
        )
        self.assertEqual(
            GRAMMAR.node(token("to", 0, 0)), {"kind": "keyword", "value": "TO"}
        )


class DiagramSelectionTests(unittest.TestCase):
    def test_a_band_without_rails_is_not_a_diagram(self) -> None:
        tokens = [token("MOVE", 10, 100), token("A", 60, 100)]
        self.assertEqual(GRAMMAR.split_diagrams(tokens, []), [])

    def test_a_railed_band_is_kept(self) -> None:
        tokens = [token("MOVE", 10, 100), token("A", 60, 100)]
        rails = [{"x0": 5.0, "y0": 100.0, "x1": 200.0, "y1": 100.0}]
        self.assertEqual(len(GRAMMAR.split_diagrams(tokens, rails)), 1)

    def test_a_figure_callout_without_vocabulary_is_rejected(self) -> None:
        self.assertFalse(GRAMMAR.is_diagram([token("1", 0, 0), token("2", 20, 0)]))
        self.assertTrue(GRAMMAR.is_diagram([token("MOVE", 0, 0), token("2", 20, 0)]))

    def test_a_single_token_band_is_rejected(self) -> None:
        self.assertFalse(GRAMMAR.is_diagram([token("CONTINUE", 0, 0)]))


class ComparisonTests(unittest.TestCase):
    def row(self) -> dict[str, object]:
        form = {
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

    def test_fragment_placeholders_are_not_counted_as_keywords(self) -> None:
        row = self.row()
        row["forms"][0]["branches"].append(
            {"kind": "keyword", "value": "PHRASE 1", "relation": "optional"}
        )
        result = COMPARE.compare(row)
        self.assertNotIn("PHRASE 1", result["keywords_missing_from_catalog"])


if __name__ == "__main__":
    unittest.main()
