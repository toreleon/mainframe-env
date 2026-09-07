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


AMS = load_module("extract_ams_pdf_parameters")

CHAPTER_PAGE = [
    "Table 4. ALTER Attributes That Can be Altered (continued)",
    "ALT",
    "INDEX",
    "CLUS",
    "TER",
    "ALTER Parameters",
    "The ALTER command takes the following required and optional parameters.",
    "Required Parameters",
    "entryname",
    "    This names the entry to be altered.",
    "Optional Parameters",
    "ACCOUNT(account-info)",
    "    Account is supported only for SMS-managed VSAM or non-VSAM data sets.",
    "    account-info",
    "        Use this to change accounting information.",
    "    Abbreviation: ACCT",
    "ADDVOLUMES(volser)",
    "    Abbreviation: ADDVOL, AVOL",
    "ALTER Examples",
    "ACCOUNT",
    "    This heading is past the section and must not be collected.",
]


class SectionTests(unittest.TestCase):
    def test_the_table_tail_above_the_section_is_dropped(self) -> None:
        body = AMS.section_lines(CHAPTER_PAGE)
        self.assertNotIn("ALT", body)
        self.assertEqual(body[0], "Required Parameters")

    def test_the_examples_heading_closes_the_section(self) -> None:
        body = AMS.section_lines(CHAPTER_PAGE)
        self.assertNotIn("ALTER Examples", body)
        self.assertNotIn("    This heading is past the section and must not be collected.", body)

    def test_a_chapter_without_parameter_groups_yields_nothing(self) -> None:
        self.assertEqual(AMS.section_lines(["Chapter 34. VERIFY", "prose"]), [])

    def test_the_singular_group_heading_is_recognized(self) -> None:
        self.assertEqual(AMS.section_lines(["Required Parameter", "FILE(ddname)"])[0], "Required Parameter")


class HeadingTests(unittest.TestCase):
    def test_flush_left_uppercase_headings_are_parameters(self) -> None:
        names, _ = AMS.headings(AMS.section_lines(CHAPTER_PAGE), "ALTER")
        self.assertEqual(names, ["ACCOUNT", "ADDVOLUMES"])

    def test_indented_subparameters_and_prose_are_not_parameters(self) -> None:
        names, _ = AMS.headings(AMS.section_lines(CHAPTER_PAGE), "ALTER")
        self.assertNotIn("ACCT", names)

    def test_abbreviations_are_collected_from_their_own_line(self) -> None:
        _, abbreviations = AMS.headings(AMS.section_lines(CHAPTER_PAGE), "ALTER")
        self.assertEqual(abbreviations, ["ACCT", "ADDVOL", "AVOL"])

    def test_single_letter_values_are_not_parameters(self) -> None:
        names, _ = AMS.headings(["Optional Parameters", "AVGREC(U|K|M)", "U", "K", "M"], "ALLOCATE")
        self.assertEqual(names, ["AVGREC"])

    def test_the_running_head_repeating_the_command_is_skipped(self) -> None:
        names, _ = AMS.headings(["Optional Parameters", "ALTER", "OWNER(id)"], "ALTER")
        self.assertEqual(names, ["OWNER"])


class ChapterTests(unittest.TestCase):
    def test_only_command_named_chapters_are_taken(self) -> None:
        entries = [
            {"title": "Chapter 4. ALLOCATE", "depth": 0, "page": 62},
            {"title": "Chapter 5. ALTER", "depth": 0, "page": 86},
            {"title": "Chapter 1. Using Access Method Services", "depth": 0, "page": 33},
        ]
        self.assertEqual(sorted(AMS.chapters(entries)), ["ALLOCATE", "ALTER"])

    def test_a_two_word_command_chapter_is_kept(self) -> None:
        entries = [{"title": "Chapter 6. ALTER LIBRARYENTRY", "depth": 0, "page": 110}]
        self.assertIn("ALTER LIBRARYENTRY", AMS.chapters(entries))


if __name__ == "__main__":
    unittest.main()
