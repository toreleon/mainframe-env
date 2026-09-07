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


RACF = load_module("extract_racf_pdf_syntax")

ADDSD = [
    "[subsystem-prefix]{ADDSD | AD}",
    "      (profile-name-1 [/password] ...)",
    "      [ ADDCATEGORY( category-name ... )]",
    "      [ AT([ node].userid ... ) | ONLYAT([ node].userid ... )]",
    "      [ ERASE ]",
]

SEGMENTED = [
    "[subsystem-prefix]{ADDUSER | AU}",
    "      [ CICS(",
    "      [ OPCLASS( OPERATOR-CLASS ... )]",
    "      [ XRFSOFF( FORCE | NOFORCE ]",
    "      )]",
    "      [ CLAUTH( class-name ... ) | NOCLAUTH ]",
]


class AliasTests(unittest.TestCase):
    def test_the_braced_alternative_is_an_alias(self) -> None:
        self.assertEqual(RACF.aliases_of(ADDSD[0], "ADDSD"), ["AD"])

    def test_a_command_without_a_brace_group_has_no_alias(self) -> None:
        self.assertEqual(RACF.aliases_of("RDELETE profile", "RDELETE"), [])


class OperandTests(unittest.TestCase):
    def test_bracketed_names_are_top_level_operands(self) -> None:
        top, nested = RACF.operands_of(ADDSD)
        self.assertEqual(top, ["ADDCATEGORY", "AT", "ONLYAT", "ERASE"])
        self.assertEqual(nested, [])

    def test_segment_members_stay_nested(self) -> None:
        top, nested = RACF.operands_of(SEGMENTED)
        self.assertEqual(top, ["CICS", "CLAUTH", "NOCLAUTH"])
        self.assertEqual(nested, ["OPCLASS", "XRFSOFF", "NOFORCE"])

    def test_an_unbalanced_syntax_line_does_not_leak_into_later_operands(self) -> None:
        top, _ = RACF.operands_of(SEGMENTED)
        self.assertIn("CLAUTH", top)

    def test_a_bracket_close_line_ends_the_segment(self) -> None:
        self.assertTrue(RACF.SEGMENT_CLOSE.match(")]"))
        self.assertTrue(RACF.SEGMENT_CLOSE.match("]]"))
        self.assertFalse(RACF.SEGMENT_CLOSE.match("[ ERASE ]"))

    def test_segment_openers_tolerate_spacing_variants(self) -> None:
        self.assertTrue(RACF.SEGMENT_OPEN.match("[ KERB("))
        self.assertTrue(RACF.SEGMENT_OPEN.match("[ ENCRYPT ("))
        self.assertTrue(RACF.SEGMENT_OPEN.match("[ OMVS[("))
        self.assertFalse(RACF.SEGMENT_OPEN.match("[ MODEL( dsname)]"))


FOOTNOTED = [
    "[subsystem-prefix]{ADDSD | AD}",
    "        [ ADDCATEGORY(category-name ...) ]",
    "2 More information about ALTER authority and how to limit it can be found in",
    "in discrete profiles in z/OS Security Server RACF Security Administrator's Guide.",
    "        [ ERASE ]",
    "Parameters",
    "        [ NOTAPARAMETER ]",
]


class BlockBoundaryTests(unittest.TestCase):
    def test_a_footnote_at_a_page_break_does_not_truncate_the_block(self) -> None:
        block, _, _ = RACF.find_block(["\n".join(FOOTNOTED)], "ADDSD")
        top, _ = RACF.operands_of(block)
        self.assertEqual(top, ["ADDCATEGORY", "ERASE"])

    def test_the_next_section_heading_still_ends_the_block(self) -> None:
        block, _, _ = RACF.find_block(["\n".join(FOOTNOTED)], "ADDSD")
        top, _ = RACF.operands_of(block)
        self.assertNotIn("NOTAPARAMETER", top)

    def test_sustained_prose_ends_the_block(self) -> None:
        page = "\n".join(
            ["[subsystem-prefix]{ADDSD | AD}", "        [ ERASE ]"]
            + ["RACF denies access to the data set entirely" for _ in range(20)]
            + ["        [ TOOLATE ]"]
        )
        block, _, _ = RACF.find_block([page], "ADDSD")
        top, _ = RACF.operands_of(block)
        self.assertEqual(top, ["ERASE"])


class TextShapeTests(unittest.TestCase):
    def test_kerned_command_names_still_match(self) -> None:
        page = "[subsystem-prefix]{RV ARY | RV}\n        [ ACTIVE ]"
        block, _, _ = RACF.find_block([page], "RVARY")
        self.assertEqual(RACF.aliases_of(block[0], "RVARY"), ["RV"])

    def test_a_command_with_no_alias_brace_is_still_located(self) -> None:
        page = "[subsystem-prefix]RACLINK\n        [ ID(userid) ]"
        found = RACF.find_block([page], "RACLINK")
        self.assertIsNotNone(found)
        top, _ = RACF.operands_of(found[0])
        self.assertEqual(top, ["ID"])

    def test_a_running_sentence_ends_the_block(self) -> None:
        self.assertTrue(RACF.is_prose("RACF denies access to the data set entirely"))

    def test_a_positional_operand_line_does_not_end_the_block(self) -> None:
        self.assertFalse(RACF.is_prose("profile-name-1"))
        self.assertFalse(RACF.is_prose("      [ ERASE ]"))


if __name__ == "__main__":
    unittest.main()
