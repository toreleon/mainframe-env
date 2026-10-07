"""What the reserved-word reader must get right about the appendix.

The list this reader emits decides which COBOL words a program may not use as
a data-name, so a parse that quietly drops a column, or quietly folds
`Potential reserved words` into the other two, would either reserve words the
compiler must accept or release words it must refuse. Neither shows up as an
error at extraction time. These tests fix the table's shape and the partition
so that a change to the reader has to say so.

The fixtures are the appendix's own shape written out by hand: a single
four-column `tbody` whose rows carry one `X`, and the symbol rows whose word
cell is the operator followed by the prose that names it.
"""

from __future__ import annotations

import importlib.util
import json
import unittest
from pathlib import Path
from types import ModuleType

TOOLS = Path(__file__).resolve().parents[1]
REPOSITORY = Path(__file__).resolve().parents[6]


def load_module(name: str) -> ModuleType:
    path = TOOLS / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


RESERVED = load_module("extract_cobol_reserved_words")


def table(rows: str) -> str:
    return (
        "<table><thead><tr><th>Word</th><th>Reserved</th>"
        "<th>Standard only</th><th>Potential reserved words</th></tr></thead>"
        f"<tbody>{rows}</tbody></table>"
    )


def row(*cells: str) -> str:
    return "<tr>" + "".join(f"<td>{cell}</td>" for cell in cells) + "</tr>"


class ReservedTableTests(unittest.TestCase):
    def test_each_column_becomes_its_own_list(self) -> None:
        body = table(
            row("MOVE", "X", "", "")
            + row("REPORT", "", "X", "")
            + row("END-ACCEPT", "", "", "X")
        )
        words = RESERVED.reserved_words(body)
        self.assertEqual(words["reserved"], ["MOVE"])
        self.assertEqual(words["standard_only"], ["REPORT"])
        self.assertEqual(words["potential"], ["END-ACCEPT"])

    def test_a_symbol_row_keeps_the_symbol_and_drops_the_prose(self) -> None:
        body = table(row("** Arithmetic operator - exponentiation", "X", "", ""))
        words = RESERVED.reserved_words(body)
        self.assertEqual(words["symbol_rows"], ["**"])
        self.assertEqual(words["reserved"], [])

    def test_a_word_marked_in_two_columns_is_refused(self) -> None:
        with self.assertRaises(ValueError):
            RESERVED.reserved_words(table(row("MOVE", "X", "X", "")))

    def test_a_word_marked_in_no_column_is_refused(self) -> None:
        with self.assertRaises(ValueError):
            RESERVED.reserved_words(table(row("MOVE", "", "", "")))

    def test_a_repeated_word_is_refused(self) -> None:
        with self.assertRaises(ValueError):
            RESERVED.reserved_words(
                table(row("MOVE", "X", "", "") + row("MOVE", "X", "", ""))
            )

    def test_a_row_that_is_not_four_cells_is_refused(self) -> None:
        with self.assertRaises(ValueError):
            RESERVED.reserved_words(table(row("MOVE", "X", "")))

    def test_a_topic_with_no_table_is_refused(self) -> None:
        with self.assertRaises(ValueError):
            RESERVED.reserved_words("<p>Reserved words</p>")


class ContextSensitiveTests(unittest.TestCase):
    def test_a_word_governing_two_constructs_is_listed_once(self) -> None:
        body = table(
            row("NAME", "XML GENERATE statement")
            + row("NAME", "JSON GENERATE statement")
            + row("CYCLE", "EXIT statement")
        )
        self.assertEqual(RESERVED.context_sensitive_words(body), ["CYCLE", "NAME"])


class CommittedListTests(unittest.TestCase):
    """The file in the tree, read as a document rather than re-derived.

    Re-deriving it needs the pinned topics, which are not in the repository, so
    what is checkable offline is that the committed list is internally
    consistent and says what the compiler's generator relies on it saying.
    """

    def setUp(self) -> None:
        path = REPOSITORY / "conformance/subsystems/cobol/structure/cobol/reserved-words.json"
        self.document = json.loads(path.read_text(encoding="utf-8"))

    def test_the_three_columns_do_not_overlap(self) -> None:
        columns = [set(self.document[name]) for name in RESERVED.COLUMNS]
        for index, first in enumerate(columns):
            for second in columns[index + 1 :]:
                self.assertEqual(first & second, set())

    def test_every_word_is_shaped_like_a_cobol_word(self) -> None:
        for column in RESERVED.COLUMNS:
            for word in self.document[column]:
                self.assertIsNotNone(RESERVED.WORD.fullmatch(word), word)

    def test_it_carries_no_coverage_and_retains_no_publication_bytes(self) -> None:
        self.assertEqual(self.document["coverage_credit"], 0)
        self.assertFalse(self.document["source"]["retained_in_repository"])

    def test_the_words_that_left_the_grammar_union_are_absent_from_it(self) -> None:
        # These are the 24 words `cargo xtask cobol-language` now filters out of
        # `grammar_keywords`. If a future re-extraction found any of them under
        # `Reserved` or `Standard only`, the filter would put it back and
        # `MOVE NAME TO DEST` would stop compiling again.
        undefinable = set(self.document["reserved"]) | set(self.document["standard_only"])
        for word in (
            "ATTRIBUTE",
            "ATTRIBUTES",
            "BYTES",
            "CODEPAGE",
            "CYCLE",
            "ELEMENT",
            "ENCODING",
            "IGNORE",
            "IGNORING",
            "INDICATING",
            "INITIALIZED",
            "KEPT",
            "LOC",
            "NAME",
            "NAMESPACE",
            "NAMESPACE-PREFIX",
            "NONNUMERIC",
            "PARAGRAPH",
            "PARSE",
            "PARTIAL",
            "PREVIOUS",
            "VALIDATING",
            "WAIT",
            "XML-DECLARATION",
        ):
            self.assertNotIn(word, undefinable)

    def test_the_words_the_forms_rely_on_are_present(self) -> None:
        undefinable = set(self.document["reserved"]) | set(self.document["standard_only"])
        for word in ("MOVE", "ADD", "THRU", "ALSO", "ANY", "VALUE", "DATA", "FILE", "LINE"):
            self.assertIn(word, undefinable)


if __name__ == "__main__":
    unittest.main()
