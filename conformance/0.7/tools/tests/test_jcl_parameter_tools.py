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


JCL = load_module("extract_jcl_pdf_parameters")


def entry(title: str, depth: int, page: int = 0) -> dict[str, object]:
    return {"title": title, "depth": depth, "page": page}


OUTLINE = [
    entry("Chapter 12. DD statement", 0),
    entry("ACCODE parameter", 1),
    entry("Subparameter definition", 2),
    entry("AMP parameter", 1),
    entry("Examples of the AMP parameter", 2),
    entry("Chapter 16. EXEC statement", 0),
    entry("ACCT parameter", 1),
    entry("PROC and procedure name parameters", 1),
]


class ChapterTests(unittest.TestCase):
    def test_chapters_span_until_the_next_chapter(self) -> None:
        spans = JCL.chapters(OUTLINE)
        self.assertEqual(spans["DD statement"], (0, 5))
        self.assertEqual(spans["EXEC statement"], (5, 8))

    def test_a_missing_chapter_is_absent_rather_than_guessed(self) -> None:
        self.assertNotIn("JOB statement", JCL.chapters(OUTLINE))


class ParameterTests(unittest.TestCase):
    def test_only_first_level_parameter_entries_are_collected(self) -> None:
        names = [item["name"] for item in JCL.parameters(OUTLINE, (0, 5))]
        self.assertEqual(names, ["ACCODE", "AMP"])

    def test_plural_parameter_titles_are_accepted(self) -> None:
        names = [item["name"] for item in JCL.parameters(OUTLINE, (5, 8))]
        self.assertEqual(names, ["ACCT", "PROC AND PROCEDURE NAME"])

    def test_repeated_names_are_recorded_once(self) -> None:
        outline = OUTLINE + [entry("ACCODE parameter", 1)]
        names = [item["name"] for item in JCL.parameters(outline, (0, 5))]
        self.assertEqual(names.count("ACCODE"), 1)


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


if __name__ == "__main__":
    unittest.main()
