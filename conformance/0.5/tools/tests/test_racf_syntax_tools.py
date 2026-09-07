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


class TextShapeTests(unittest.TestCase):
    def test_kerned_command_names_still_match(self) -> None:
        import re

        pattern = re.compile(r"\{\s*" + RACF.spaced("RVARY") + r"\s*(\||\})")
        self.assertTrue(pattern.search("[subsystem-prefix]{RV ARY | RV}"))
        self.assertTrue(pattern.search("[subsystem-prefix]{RVARY}"))

    def test_a_running_sentence_ends_the_block(self) -> None:
        self.assertTrue(RACF.is_prose("RACF denies access to the data set entirely"))

    def test_a_positional_operand_line_does_not_end_the_block(self) -> None:
        self.assertFalse(RACF.is_prose("profile-name-1"))
        self.assertFalse(RACF.is_prose("      [ ERASE ]"))


if __name__ == "__main__":
    unittest.main()
