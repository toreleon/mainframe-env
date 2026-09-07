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

#: SET is the standing regression. Its command has no abbreviation, and no
#: brace group in its Syntax section is anchored on its keyword. The groups the
#: topic does carry — reproduced here — belong to the LIST and TRACE operands.
SET = """
<section class="section refsyn"><h2 class="sectiontitle">Syntax</h2>
<table role="presentation" class="defaultstyle"><tbody>
<tr><td class="tdleft">[<span class="ph var">subsystem-prefix</span>]SET</td></tr>
<tr><td class="tdleft"><div class="lines">[ LIST<span class="ph"
data-hd-otherprops="nohelp">( {</span>SYSTEM | JOBNAME(<span class="ph var">jobname</span>
...)<span class="ph" data-hd-otherprops="nohelp">} )</span> ]</div></td></tr>
<tr><td class="tdleft"><div class="lines">[ TRACE<span class="ph"
data-hd-otherprops="nohelp">( {</span>COUNT(<span class="ph var">number</span>) |
RESET<span class="ph" data-hd-otherprops="nohelp">} )</span> ]</div></td></tr>
</tbody></table>
</section>
<section class="section"><h2 class="sectiontitle">Parameters</h2>
<dl class="parml"><dt class="pt dlterm">LIST</dt><dd class="pd">lists it.</dd></dl>
</section>
"""

#: ADDSD carries the group the anchoring exists for. `{SET | SETONLY | NOSET}`
#: is ADDSD's own operand, defined under Parameters with all three restated
#: beneath it; `set.htm` contains neither SETONLY nor NOSET anywhere.
ADDSD = """
<section class="section refsyn"><h2 class="sectiontitle">Syntax</h2>
<p data-hd-otherprops="nohelp">The complete syntax of the ADDSD command is:</p>
<table summary="" role="presentation" class="defaultstyle cds--data-table"><tbody>
<tr><td class="tdleft"><span class="ph" data-hd-otherprops="nohelp">[<span
class="ph var">subsystem-prefix</span>]{ADDSD
| AD}</span></td></tr>
<tr><td class="tdleft"><div class="lines"><span class="ph"
data-hd-otherprops="nohelp">[ {</span>GENERIC | MODEL | TAPE<span class="ph"
data-hd-otherprops="nohelp">} ]</span></div></td></tr>
<tr><td class="tdleft"><div class="lines"><span class="ph"
data-hd-otherprops="nohelp">[ {</span><u>SET</u> | SETONLY | NOSET<span class="ph"
data-hd-otherprops="nohelp">} ]</span></div></td></tr>
</tbody></table></section>
<section class="section"><h2 class="sectiontitle">Parameters</h2>
<dl class="parml">
<dt class="pt dlterm">SET | SETONLY | NOSET</dt>
<dd class="pd">specifies whether the in-storage profiles are refreshed.
  <dl class="parml">
  <dt class="pt dlterm">SET</dt><dd class="pd">refreshes them.</dd>
  <dt class="pt dlterm">SETONLY</dt><dd class="pd">refreshes them only.</dd>
  <dt class="pt dlterm">NOSET</dt><dd class="pd">does not refresh them.</dd>
  </dl>
</dd>
</dl></section>
"""

#: The RACDCERT umbrella topic: a Syntax section that points at the function
#: subtopics, no table in it, and no Parameters section at all.
UMBRELLA = """
<section class="section refsyn"><h2 class="sectiontitle">Syntax</h2>
<p>For details about syntax and parameters for each RACDCERT function, see the
&ldquo;Syntax&rdquo; and &ldquo;Parameters&rdquo; subtopics of each RACDCERT
function.</p>
</section>
"""

#: ADDUSER's NETVIEW segment, three levels: the segment, its members, and the
#: values two of those members restate. Under the collapsed reading GENERAL,
#: GLOBAL, SPECIFIC, NO and YES were all reported as members of NETVIEW.
NETVIEW = """
<section class="section"><h2 class="sectiontitle">Parameters</h2>
<dl class="parml">
<dt class="pt dlterm">NETVIEW</dt>
<dd class="pd">specifies the NETVIEW segment.
  <dl class="parml">
  <dt class="pt dlterm">CTL</dt>
  <dd class="pd">whether a security check is performed.
    <dl class="parml">
    <dt class="pt dlterm">GENERAL</dt><dd class="pd">as for SPECIFIC, and more.</dd>
    <dt class="pt dlterm">GLOBAL</dt><dd class="pd">no checking is done.</dd>
    <dt class="pt dlterm"><span class="keyword kwd defkwd">SPECIFIC</span></dt>
    <dd class="pd">only started spans.</dd>
    </dl>
  </dd>
  <dt class="pt dlterm">MSGRECVR(NO | YES)</dt>
  <dd class="pd">whether unsolicited messages are received.
    <dl class="parml">
    <dt class="pt dlterm">NO</dt><dd class="pd">they are not.</dd>
    <dt class="pt dlterm">YES</dt><dd class="pd">they are.</dd>
    </dl>
  </dd>
  <dt class="pt dlterm">OPCLASS(<em>class</em> ...)</dt><dd class="pd">the classes.</dd>
  </dl>
</dd>
</dl>
</section>
"""

#: ALTUSER's OPERPARM segment, five of the six levels that topic reaches. The
#: alternation term at the top restates itself; AUTH enumerates beneath a term
#: that does not restate the enumeration, and DOM restates it in its argument.
OPERPARM = """
<section class="section"><h2 class="sectiontitle">Parameters</h2>
<dl class="parml">
<dt class="pt dlterm">OPERPARM | NOOPERPARM</dt>
<dd class="pd">the OPERPARM segment.
  <dl class="parml">
  <dt class="pt dlterm">OPERPARM</dt>
  <dd class="pd">specifies it.
    <dl class="parml">
    <dt class="pt dlterm">AUTH | NOAUTH</dt>
    <dd class="pd">the authority.
      <dl class="parml">
      <dt class="pt dlterm">AUTH</dt>
      <dd class="pd">specifies it.
        <dl class="parml">
        <dt class="pt dlterm">MASTER</dt><dd class="pd">master authority.</dd>
        <dt class="pt dlterm">ALL</dt><dd class="pd">all of them.</dd>
        </dl>
      </dd>
      <dt class="pt dlterm">NOAUTH</dt><dd class="pd">removes it.</dd>
      </dl>
    </dd>
    <dt class="pt dlterm">DOM | NODOM</dt>
    <dd class="pd">the DOM authority.
      <dl class="parml">
      <dt class="pt dlterm">DOM(NORMAL | ALL | NONE)</dt>
      <dd class="pd">specifies it.
        <dl class="parml">
        <dt class="pt dlterm">NORMAL</dt><dd class="pd">the default.</dd>
        <dt class="pt dlterm">ALL</dt><dd class="pd">all of them.</dd>
        <dt class="pt dlterm">NONE</dt><dd class="pd">none of them.</dd>
        </dl>
      </dd>
      </dl>
    </dd>
    </dl>
  </dd>
  <dt class="pt dlterm">NOOPERPARM</dt><dd class="pd">removes it.</dd>
  </dl>
</dd>
</dl>
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
        self.assertIsNone(RACF.section_named(UMBRELLA, "Parameters"))

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
        # The regression, and the group it is really about. `{SET | SETONLY |
        # NOSET}` is in the ADDSD topic, where it is ADDSD's own operand; ADDSD
        # abbreviates to AD and nothing else.
        syntax = RACF.section_named(ADDSD, "Syntax")
        self.assertIn("{ SET | SETONLY | NOSET }", RACF.text_of(syntax))
        self.assertEqual(RACF.aliases(syntax, "ADDSD"), ["AD"])

    def test_the_group_naming_setonly_is_an_operand_of_addsd(self) -> None:
        # It is defined under ADDSD's Parameters with all three restated
        # beneath it, which is what an operand alternation looks like and what
        # an abbreviation list never does.
        terms = RACF.operands(RACF.section_named(ADDSD, "Parameters"))
        self.assertEqual(terms[0]["names"], ["SET", "SETONLY", "NOSET"])
        self.assertEqual(terms[0]["values"], ["SET", "SETONLY", "NOSET"])

    def test_the_set_command_has_no_group_of_its_own(self) -> None:
        # SET returns no alias, and the reason is that its own Syntax section
        # brace-groups two operands and never its keyword.
        syntax = RACF.section_named(SET, "Syntax")
        self.assertEqual(RACF.aliases(syntax, "SET"), [])
        text = RACF.text_of(syntax)
        self.assertIn("{ SYSTEM | JOBNAME( jobname ...) }", text)
        self.assertIn("{ COUNT( number ) | RESET }", text)
        self.assertNotIn("SETONLY", text)

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


def find(entries: list[dict[str, object]], term: str) -> dict[str, object]:
    for entry in RACF.descendants(entries):
        if entry["term"] == term:
            return entry
    raise AssertionError(f"no term {term!r} in the tree")


class NestingTests(unittest.TestCase):
    """The depth below the operand, which the collapsed reading threw away."""

    def setUp(self) -> None:
        self.netview = RACF.operands(RACF.section_named(NETVIEW, "Parameters"))
        self.operparm = RACF.operands(RACF.section_named(OPERPARM, "Parameters"))

    def test_the_tree_is_as_deep_as_the_list(self) -> None:
        self.assertEqual(RACF.nesting_depth(self.netview), 3)
        self.assertEqual(RACF.nesting_depth(self.operparm), 5)

    def test_a_grandchild_is_not_reported_against_the_operand(self) -> None:
        # The defect. GENERAL, GLOBAL and SPECIFIC belong to CTL and NO and YES
        # to MSGRECVR; all five were reported as members of the NETVIEW segment.
        segment = self.netview[0]
        self.assertEqual(segment["names"], ["NETVIEW"])
        self.assertEqual(
            segment["members"], ["CTL", "MSGRECVR", "OPCLASS"]
        )
        for name in ("GENERAL", "GLOBAL", "SPECIFIC", "NO", "YES"):
            self.assertNotIn(name, segment["members"])

    def test_a_term_below_the_operand_classifies_its_own_children(self) -> None:
        self.assertEqual(
            find(self.netview, "CTL")["members"], ["GENERAL", "GLOBAL", "SPECIFIC"]
        )

    def test_an_argument_that_states_its_alternatives_makes_them_values(self) -> None:
        # ALTUSER writes `CTL(GENERAL | GLOBAL | SPECIFIC)` for the same three
        # terms ADDUSER hangs under a bare `CTL`, so the argument is read too.
        msgrecvr = find(self.netview, "MSGRECVR(NO | YES)")
        self.assertEqual(msgrecvr["values"], ["NO", "YES"])
        self.assertEqual(msgrecvr["members"], [])
        self.assertEqual(
            RACF.alternatives("DOM(NORMAL | ALL | NONE)"),
            ["DOM", "NORMAL", "ALL", "NONE"],
        )

    def test_a_placeholder_argument_offers_nothing(self) -> None:
        self.assertEqual(RACF.alternatives("OPCLASS(operator-class ...)"), ["OPCLASS"])
        self.assertEqual(RACF.alternatives("MSCOPE(system-name ... | * | *ALL)"),
                         ["MSCOPE"])

    def test_an_enumeration_the_term_does_not_restate_reads_as_members(self) -> None:
        # The rule stating what it can see. `AUTH` lists MASTER and ALL beneath
        # itself without naming them in the term, so they are reported as
        # members of AUTH — not as a claim that AUTH is a segment, and no
        # longer as contents of OPERPARM.
        self.assertEqual(find(self.operparm, "AUTH")["members"], ["MASTER", "ALL"])
        self.assertEqual(self.operparm[0]["members"], [])
        self.assertEqual(self.operparm[0]["values"], ["OPERPARM", "NOOPERPARM"])

    def test_the_same_name_may_be_a_value_here_and_a_member_there(self) -> None:
        self.assertIn("ALL", find(self.operparm, "AUTH")["members"])
        self.assertIn("ALL", find(self.operparm, "DOM(NORMAL | ALL | NONE)")["values"])

    def test_a_term_with_no_parent_at_the_level_above_stops_the_run(self) -> None:
        # A `dl` that opens two levels at once leaves a term with no term to
        # belong to. Attaching it to the nearest one available would report a
        # reader defect as a fact about the publication. No topic of the 60
        # does this.
        skipped = (
            '<dl><dt>OMVS</dt><dd><dl><dl><dt>UID</dt><dd>x</dd></dl></dl></dd></dl>'
        )
        with self.assertRaises(ValueError):
            RACF.operand_tree(skipped)


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
            for term in RACF.descendants(surface["terms"])
            for name in term["names"] + term["values"] + term["members"]
        }
        self.assertNotIn("TERMUACC", reachable)
        self.assertIn("TERMUACC", surface["tokens"])

    def test_every_level_is_reachable_from_the_surface(self) -> None:
        # `reachable` above walks the tree, so a name the publication puts three
        # levels down cannot be mistaken for one the syntax table alone draws.
        surface = RACF.surface(NETVIEW, "ADDUSER")
        reachable = {
            name
            for term in RACF.descendants(surface["terms"])
            for name in term["names"] + term["values"] + term["members"]
        }
        self.assertIn("SPECIFIC", reachable)
        self.assertIn("YES", reachable)

    def test_a_topic_with_no_parameters_section_says_so(self) -> None:
        self.assertFalse(RACF.surface(UMBRELLA, "RACDCERT")["has_parameters"])


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
