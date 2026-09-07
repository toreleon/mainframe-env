"""Tests for the RACF topic reader.

The fixtures are markup, not extracted text lines. That is the point of the
change these tests cover: the retired reader was tested against strings that
looked like a PDF page, so its tests could only ever confirm that it parsed the
shape someone had imagined, and the two things it got wrong — a segment opened
inline, and a brace group that was not the command's own — were both invisible
at that level. Every fixture below is a fragment of the shape the reference
actually publishes.
"""

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


RACF = load_module("extract_racf_html_syntax")
FETCH = load_module("fetch_racf_topics")


ADDGROUP = """
<section class="section refsyn" role="region" aria-labelledby="addgroup__title__6">
<h2 class="sectiontitle" id="addgroup__title__6">Syntax</h2>
<p data-hd-otherprops="nohelp">For the key to the symbols used in the command
syntax diagrams, see <a href="xsyntax.htm">Syntax of RACF commands and operands</a>.
The complete syntax of the ADDGROUP command is:</p>
<div class="tablenoborder"><table summary="" role="presentation" class="defaultstyle">
<tbody>
<tr><td class="tdleft"><span class="ph" data-hd-otherprops="nohelp">[<span class="ph var">subsystem-prefix</span>]{ADDGROUP
| AG}</span></td></tr>
<tr><td class="tdleft"><div class="lines">[ CSDATA( [ <span class="ph var">custom-field-name</span> ] ) ]</div></td></tr>
<tr><td class="tdleft"><div class="lines">[ <u>TERMUACC</u> | NOTERMUACC ]</div></td></tr>
<tr><td class="tdleft"><div class="lines">[ UNIVERSAL ]</div></td></tr>
</tbody></table></div>
<p data-hd-otherprops="nohelp">For information on issuing this command as a
RACF TSO command, refer to <a href="trc.htm">RACF TSO commands</a>.</p>
</section>

<section class="section" role="region" aria-labelledby="addgroup__title__7">
<h2 class="sectiontitle" id="addgroup__title__7">Parameters</h2>
<dl class="parml">
<dt class="pt dlterm"><strong><em>group-name</em></strong></dt>
<dd class="pd">specifies the group.</dd>
<dt class="pt dlterm">AT([<em>node</em>].<em>userid</em>) | ONLYAT([<em>node</em>].<em>userid</em>)</dt>
<dd class="pd">directs the command.
  <dl class="parml">
  <dt class="pt dlterm">AT([node].userid ...)</dt><dd class="pd">runs it there too.</dd>
  <dt class="pt dlterm">ONLYAT([node].userid ...)</dt><dd class="pd">runs it only there.</dd>
  </dl>
</dd>
<dt class="pt dlterm">CICS</dt>
<dd class="pd">specifies the CICS segment.
  <dl class="parml">
  <dt class="pt dlterm">OPCLASS(<em>operator-class</em> ...)</dt><dd class="pd">the classes.</dd>
  <dt class="pt dlterm">XRFSOFF(FORCE | NOFORCE)</dt><dd class="pd">the sign-off option.</dd>
  </dl>
</dd>
<dt class="pt dlterm">UNIVERSAL</dt><dd class="pd">makes the group universal.</dd>
</dl>
</section>

<h2 class="sectiontitle">Examples</h2>
<dl class="parml"><dt class="pt dlterm">OPERATION</dt><dd>an example, not syntax.</dd></dl>
"""

#: SET is the standing regression. Its command has no abbreviation, and the
#: brace group in its Syntax section belongs to an operand.
SET = """
<section class="section refsyn"><h2 class="sectiontitle">Syntax</h2>
<table role="presentation"><tbody><tr><td>[<span>subsystem-prefix</span>]SET</td></tr>
<tr><td>[ AUTOAPPL( {SETONLY | NOSET} ) ]</td></tr></tbody></table>
</section>
"""

#: RACDCERT IMPORT opens with three operands in a single term.
IMPORT = """
<section class="section refsyn"><h2 class="sectiontitle">Syntax</h2>
<table role="presentation"><tbody><tr><td>RACDCERT IMPORT(TOKEN(<em>token-name</em>)
SEQNUM(<em>sequence-number</em>)) [ ID(<em>certificate-owner</em>) | SITE | CERTAUTH ]</td></tr>
</tbody></table></section>
<section class="section"><h2 class="sectiontitle">Parameters</h2>
<dl class="parml">
<dt class="pt dlterm" id="le-import__importtok">IMPORT
TOKEN(<span class="ph var">token-name</span>) SEQNUM(<span class="ph var">sequence-</span><span class="ph var">number</span>)</dt>
<dd class="pd">imports it.</dd>
<dt class="pt dlterm">ID(<em>certificate-owner</em>) | SITE | CERTAUTH</dt>
<dd class="pd">the owner.</dd>
</dl>
</section>
"""


class SectionScopeTests(unittest.TestCase):
    def test_a_section_is_named_by_its_own_sectiontitle(self) -> None:
        self.assertEqual(
            [entry["title"] for entry in RACF.sections(ADDGROUP)],
            ["Syntax", "Parameters"],
        )

    def test_a_definition_list_outside_parameters_is_not_read(self) -> None:
        parameters = RACF.section_named(ADDGROUP, "Parameters")
        self.assertNotIn("OPERATION", parameters)

    def test_an_absent_section_is_reported_rather_than_guessed(self) -> None:
        self.assertIsNone(RACF.section_named(SET, "Parameters"))

    def test_a_repeated_heading_is_refused_not_resolved_by_taking_the_first(
        self,
    ) -> None:
        doubled = ADDGROUP + '<section><h2 class="sectiontitle">Parameters</h2></section>'
        with self.assertRaises(ValueError):
            RACF.section_named(doubled, "Parameters")

    def test_nested_sections_close_at_their_own_end_tag(self) -> None:
        nested = (
            '<section><h2 class="sectiontitle">Syntax</h2>'
            '<section><h2 class="sectiontitle">Inner</h2>inner</section>outer</section>'
        )
        self.assertIn("outer", RACF.section_named(nested, "Syntax"))


class AliasTests(unittest.TestCase):
    def test_the_brace_group_yields_the_documented_abbreviation(self) -> None:
        syntax = RACF.section_named(ADDGROUP, "Syntax")
        self.assertEqual(RACF.aliases(syntax, "ADDGROUP"), ["AG"])

    def test_a_brace_group_split_across_markup_and_lines_still_matches(self) -> None:
        # The reference sets `{ADDGROUP` and `| AG}` on separate source lines,
        # with a span boundary between them.
        self.assertIn("{ADDGROUP\n| AG}", ADDGROUP)
        self.assertEqual(
            RACF.aliases('{SETROPTS\n</span><span>| SETR}', "SETROPTS"), ["SETR"]
        )

    def test_a_brace_group_that_is_not_the_commands_own_is_not_an_alias(self) -> None:
        # The regression: an unanchored search returns SETONLY and NOSET, which
        # are values of the AUTOAPPL operand, not abbreviations of SET.
        syntax = RACF.section_named(SET, "Syntax")
        self.assertEqual(RACF.aliases(syntax, "SET"), [])

    def test_several_alternatives_are_all_returned(self) -> None:
        syntax = '<h2 class="sectiontitle">Syntax</h2>{PASSWORD | PW | PHRASE}'
        self.assertEqual(RACF.aliases(syntax, "PASSWORD"), ["PW", "PHRASE"])

    def test_a_lowercase_alternative_is_not_an_alias(self) -> None:
        self.assertEqual(RACF.aliases("{RVARY | operand-name}", "RVARY"), [])


class NameTests(unittest.TestCase):
    def test_a_term_naming_a_sequence_yields_every_keyword(self) -> None:
        self.assertEqual(
            RACF.names("IMPORT(TOKEN(token-name) SEQNUM(sequence-number))"),
            ["IMPORT"],
        )
        self.assertEqual(
            RACF.names("IMPORT TOKEN(token-name) SEQNUM(sequence-number)"),
            ["IMPORT", "TOKEN", "SEQNUM"],
        )

    def test_arguments_are_removed_before_the_whitespace_split(self) -> None:
        # Splitting first would cut `RESOWNER( userid or group-name )` into words.
        self.assertEqual(RACF.names("RESOWNER( userid or group-name )"), ["RESOWNER"])

    def test_an_alternation_still_yields_every_name(self) -> None:
        self.assertEqual(RACF.names("ID(owner) | SITE | CERTAUTH"),
                         ["ID", "SITE", "CERTAUTH"])

    def test_a_keyword_qualified_by_a_bracketed_option_is_recovered(self) -> None:
        self.assertEqual(
            RACF.names("INACTIVE [NOCLASSACT(class-namelist | *) (NOTAPE)]"),
            ["INACTIVE", "NOCLASSACT"],
        )

    def test_a_lowercase_placeholder_is_not_an_operand(self) -> None:
        self.assertEqual(RACF.names("group-name"), [])


class OperandTests(unittest.TestCase):
    def setUp(self) -> None:
        self.terms = RACF.operands(RACF.section_named(ADDGROUP, "Parameters"))
        self.by_name = {
            tuple(term["names"]): term for term in self.terms if term["names"]
        }

    def test_depth_one_terms_are_the_operands(self) -> None:
        found: list[str] = []
        for term in self.terms:
            found.extend(term["names"])
        self.assertEqual(found, ["AT", "ONLYAT", "CICS", "UNIVERSAL"])

    def test_a_segment_keeps_its_members(self) -> None:
        cics = self.by_name[("CICS",)]
        self.assertEqual(cics["members"], ["OPCLASS", "XRFSOFF"])
        self.assertEqual(cics["values"], [])

    def test_an_alternative_restated_beneath_its_term_is_a_value(self) -> None:
        at = self.by_name[("AT", "ONLYAT")]
        self.assertEqual(at["values"], ["AT", "ONLYAT"])
        self.assertEqual(at["members"], [])

    def test_a_member_is_never_promoted_to_an_operand(self) -> None:
        found: list[str] = []
        for term in self.terms:
            found.extend(term["names"])
        self.assertNotIn("OPCLASS", found)

    def test_a_term_naming_three_operands_yields_all_three(self) -> None:
        # RACDCERT IMPORT opens `IMPORT TOKEN(...) SEQNUM(...)` in one `dt`,
        # hard-wrapped after the first word. Reading the term as a whole loses
        # all three names.
        terms = RACF.operands(RACF.section_named(IMPORT, "Parameters"))
        self.assertEqual(terms[0]["names"], ["IMPORT", "TOKEN", "SEQNUM"])
        self.assertEqual(
            terms[1]["names"], ["ID", "SITE", "CERTAUTH"]
        )

    def test_a_topic_without_definition_lists_contributes_nothing(self) -> None:
        self.assertEqual(RACF.operands("<p>prose only</p>"), [])


DELUSER = """
<section class="section refsyn"><h2 class="sectiontitle">Syntax</h2>
<p data-hd-otherprops="nohelp">The complete syntax of the DELUSER command is:</p>
<table summary="Syntax of the DELUSER command" class="defaultstyle cds--data-table">
<tbody><tr><td>[<span>subsystem-prefix</span>]{DELUSER | DU}</td></tr>
<tr><td>[ AT([<span>node</span>].<span>userid</span>) | ONLYAT([<span>node</span>].<span>userid</span>) ]</td></tr>
</tbody></table></section>
"""


class SyntaxTokenTests(unittest.TestCase):
    def test_the_syntax_table_is_read(self) -> None:
        tokens = RACF.syntax_tokens(RACF.section_named(ADDGROUP, "Syntax"))
        self.assertIn("TERMUACC", tokens)
        self.assertIn("NOTERMUACC", tokens)

    def test_a_syntax_table_without_the_presentation_role_is_still_read(self) -> None:
        # DELUSER, RACDCERT EXPORT, RACDCERT REKEY and RACPRMCK give their
        # syntax table a `summary` and no `role`. Requiring the role reads all
        # four as having no syntax at all.
        section = RACF.section_named(DELUSER, "Syntax")
        self.assertNotIn("presentation", section)
        self.assertEqual(RACF.syntax_tokens(section), ["DELUSER", "DU", "AT", "ONLYAT"])

    def test_a_syntax_section_with_no_table_yields_nothing(self) -> None:
        # The RACDCERT umbrella topic says to read the function subtopics.
        umbrella = '<section><h2 class="sectiontitle">Syntax</h2>' \
                   "<p>See the subtopics of each RACDCERT function.</p></section>"
        self.assertEqual(RACF.syntax_tokens(RACF.section_named(umbrella, "Syntax")), [])

    def test_the_boilerplate_paragraphs_are_dropped(self) -> None:
        # Both `nohelp` paragraphs name RACF, and the outro names TSO.
        tokens = RACF.syntax_tokens(RACF.section_named(ADDGROUP, "Syntax"))
        self.assertNotIn("TSO", tokens)
        self.assertNotIn("RACF", tokens)

    def test_a_lowercase_placeholder_is_not_a_token(self) -> None:
        tokens = RACF.syntax_tokens(RACF.section_named(ADDGROUP, "Syntax"))
        self.assertNotIn("custom-field-name", tokens)


class SurfaceTests(unittest.TestCase):
    def test_the_syntax_table_names_something_parameters_does_not(self) -> None:
        # TERMUACC is drawn and not defined; that is what `syntax_only` reports.
        surface = RACF.surface(ADDGROUP, "ADDGROUP")
        reachable = {
            name
            for term in surface["terms"]
            for name in term["names"] + term["values"] + term["members"]
        }
        self.assertNotIn("TERMUACC", reachable)
        self.assertIn("TERMUACC", surface["tokens"])

    def test_a_topic_with_no_parameters_section_says_so(self) -> None:
        self.assertFalse(RACF.surface(SET, "SET")["has_parameters"])


BOOK = "SSLTBW_3.2.0/com.ibm.zos.v3r2.icha400/abstract.htm"
SYNTAX = "SSLTBW_3.2.0/com.ibm.zos.v3r2.icha400/cmdsyn.htm"


def family_node(index: int) -> dict[str, object]:
    return {"label": f"CMD{index:02d} (a command)", "href": f"c{index}.htm",
            "topicId": f"cmd{index}"}


def tree(children: list[dict[str, object]]) -> dict[str, object]:
    return {
        "toc": {
            "label": "z/OS",
            "topics": [
                # The abstract is filed twice, and only one of the two is the
                # book: `?pos=2` is a navigation disambiguator.
                {"label": "z/OS Security Server RACF Command Language Reference",
                 "href": BOOK,
                 "topics": [{"label": "RACF command syntax", "href": SYNTAX,
                             "topics": children}]},
                {"label": "Abstract", "href": BOOK + "?pos=2", "topics": []},
            ],
        }
    }


class SelectionTests(unittest.TestCase):
    def test_the_book_is_the_node_carrying_the_syntax_child(self) -> None:
        document = tree([family_node(i) for i in range(FETCH.FAMILY_COUNT)])
        self.assertEqual(len(FETCH.book_node(document)["topics"]), 1)

    def test_a_family_is_keyed_by_the_first_token_of_its_label(self) -> None:
        node = {"label": "PASSWORD or PHRASE (Specify user password)",
                "href": "passwrd.htm"}
        self.assertEqual(FETCH.keyword_of(node), "PASSWORD")

    def test_exactly_thirty_four_families_are_accepted(self) -> None:
        document = tree([family_node(i) for i in range(FETCH.FAMILY_COUNT)])
        self.assertEqual(len(FETCH.families(document)), FETCH.FAMILY_COUNT)

    def test_a_short_family_list_stops_the_tool(self) -> None:
        # 34 is an immutable denominator, so a book that publishes 33 is a
        # republication to review, never a manifest to write.
        document = tree([family_node(i) for i in range(33)])
        with self.assertRaises(FETCH.SelectionError):
            FETCH.families(document)

    def test_repeated_family_keywords_stop_the_tool(self) -> None:
        children = [family_node(i) for i in range(FETCH.FAMILY_COUNT - 1)]
        children.append(dict(family_node(0), href="duplicate.htm"))
        document = tree(children)
        with self.assertRaises(FETCH.SelectionError):
            FETCH.families(document)

    def test_a_navigation_position_is_stripped_from_a_topic_path(self) -> None:
        self.assertEqual(FETCH.strip_position(BOOK + "?pos=2"), BOOK)

    def test_only_the_racdcert_function_subtopics_are_taken(self) -> None:
        node = {
            "label": "RACDCERT (Manage RACF digital certificates)",
            "href": "radcertg.htm",
            "topics": [
                {"label": "Examples of controlling access to RACDCERT functions "
                          "using the FACILITY class", "href": "xconracdfc.htm"},
                {"label": "Examples of controlling the use of the RACDCERT command "
                          "using the RDATALIB class", "href": "xconracdrd.htm"},
            ] + [
                {"label": f"RACDCERT F{index:02d} (a function)",
                 "href": f"le-{index}.htm", "topicId": f"f{index}"}
                for index in range(FETCH.RACDCERT_FUNCTION_COUNT)
            ],
        }
        functions = FETCH.racdcert_functions(node)
        self.assertEqual(len(functions), FETCH.RACDCERT_FUNCTION_COUNT)
        self.assertNotIn("Examples", [entry["keyword"] for entry in functions])

    def test_a_function_is_keyed_by_the_word_after_racdcert(self) -> None:
        self.assertEqual(
            FETCH.keyword_of_function({"label": "RACDCERT ADDRING (Add key ring)"}),
            "ADDRING",
        )


class DestinationTests(unittest.TestCase):
    def test_publication_bytes_may_not_be_written_into_the_tree(self) -> None:
        with self.assertRaises(ValueError):
            FETCH.outside_repository(FETCH.REPOSITORY / "conformance" / "0.5" / "x")

    def test_a_path_outside_the_tree_is_accepted(self) -> None:
        self.assertTrue(FETCH.outside_repository(Path("/tmp/racf-topics")).is_absolute())


if __name__ == "__main__":
    unittest.main()
