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


LOCATORS = load_module("verify_outline_locators")

PAGES = {
    "ALLOCATE CURSOR": [1169],
    "ADD statement": [343],
    "DELETE": [40, 273],
}


def row(identifier: str, locator: str) -> dict[str, str]:
    return {"id": identifier, "source_locator": locator}


class LocatorTests(unittest.TestCase):
    def test_a_heading_on_its_recorded_page_is_exact(self) -> None:
        result = LOCATORS.check(row("r1", "pdf-page:1169;outline:ALLOCATE CURSOR"), PAGES)
        self.assertEqual(result["verdict"], "exact")

    def test_a_heading_on_a_different_page_is_moved(self) -> None:
        result = LOCATORS.check(row("r2", "pdf-page:900;outline:ADD statement"), PAGES)
        self.assertEqual(result["verdict"], "moved")
        self.assertEqual(result["found_pages"], [343])

    def test_a_heading_the_publication_does_not_carry_is_missing(self) -> None:
        result = LOCATORS.check(row("r3", "pdf-page:12;outline:INVENTED statement"), PAGES)
        self.assertEqual(result["verdict"], "missing")

    def test_a_repeated_heading_matches_any_of_its_pages(self) -> None:
        for page in (40, 273):
            result = LOCATORS.check(row("r4", f"pdf-page:{page};outline:DELETE"), PAGES)
            self.assertEqual(result["verdict"], "exact")

    def test_a_non_pdf_locator_is_reported_as_skipped_not_dropped(self) -> None:
        result = LOCATORS.check(row("r5", "html-table:dfha8mf__eibfn_table;eibfn:0602"), PAGES)
        self.assertEqual(result["verdict"], "skipped")
        self.assertIn("locator", result)

    def test_non_breaking_spaces_in_a_title_do_not_defeat_the_match(self) -> None:
        pages = LOCATORS.index([{"title": LOCATORS.clean("Chapter\xa04.\xa0 ALLOCATE"), "page": 63, "depth": 0}])
        result = LOCATORS.check(row("r6", "pdf-page:63;outline:Chapter 4. ALLOCATE"), pages)
        self.assertEqual(result["verdict"], "exact")


if __name__ == "__main__":
    unittest.main()
