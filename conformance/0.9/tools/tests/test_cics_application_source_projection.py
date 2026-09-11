from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
import re
import sys
import unittest
from pathlib import Path


TOOL = Path(__file__).resolve().parents[1] / "extract_cics_application_sources.py"
spec = importlib.util.spec_from_file_location("extract_cics_application_sources", TOOL)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


SYNTAX_HTML = """
<article>
  <section>
    <h2 id="syntax" class="sectiontitle">Syntax</h2>
    <div class="syntaxdiagram">
      <h3 id="diagram" class="syntaxdiagram-title">ALLOCATE</h3>
      <svg class="syntaxdiagram">
        <g class="diagram">
          <g class="boxed groupcomp">
            <g class="unboxed syntaxkwd"><g><text class="syntaxkwd">ALLOCATE</text></g></g>
          </g>
          <g class="groupchoice">
            <g class="groupseq">
              <g class="boxed groupcomp"><g class="unboxed syntaxkwd">
                <g><text class="syntaxkwd">SYSID(</text></g>
                <g><text class="syntaxvar">systemname</text></g>
                <g><text class="syntaxdelim">)</text></g>
              </g></g>
              <g class="groupseq">
                <g></g>
                <g class="boxed groupcomp"><g class="unboxed syntaxkwd">
                  <g><text class="syntaxkwd">PROFILE(</text></g>
                  <g><text class="syntaxvar">name</text></g>
                  <g><text class="syntaxdelim">)</text></g>
                </g></g>
              </g>
            </g>
            <g class="boxed groupcomp"><g class="unboxed syntaxkwd">
              <g><text class="syntaxkwd">PARTNER(</text></g>
              <g><text class="syntaxvar">name</text></g>
              <g><text class="syntaxdelim">)</text></g>
            </g></g>
          </g>
        </g>
      </svg>
    </div>
  </section>
</article>
"""


class OrderedHtmlTests(unittest.TestCase):
    def test_mixed_inline_content_keeps_source_order(self) -> None:
        root = module.parse_html(
            '<dl><dt>PROFILE(<span class="var">name</span>)</dt></dl>'
        )
        term = next(node for node in module.walk(root) if node.tag == "dt")
        self.assertEqual(term.text(), "PROFILE( name )")
        self.assertLess(term.text().index("PROFILE"), term.text().index("name"))

    def test_void_element_does_not_capture_following_content(self) -> None:
        root = module.parse_html("<p>before<img src='x'>after<span>end</span></p>")
        paragraph = next(node for node in module.walk(root) if node.tag == "p")
        image = next(node for node in module.walk(root) if node.tag == "img")
        span = next(node for node in module.walk(root) if node.tag == "span")
        self.assertIs(span.parent, paragraph)
        self.assertEqual(image.children, [])
        self.assertEqual(paragraph.text(), "before after end")

    def test_structural_path_uses_ids_then_sibling_ordinals(self) -> None:
        root = module.parse_html(
            '<article><section><h2 id="options">Options</h2><p>a</p><p>b</p></section></article>'
        )
        paragraphs = [node for node in module.walk(root) if node.tag == "p"]
        self.assertTrue(module.structural_path(paragraphs[1]).endswith("section[1]/p[2]"))

    def test_duplicate_structural_locator_fails_closed(self) -> None:
        root = module.parse_html(
            '<article><section><p id="same">a</p><p id="same">b</p></section></article>'
        )
        with self.assertRaisesRegex(module.ProjectionError, "non-unique structural"):
            module._validate_unique_structural_paths(root, "synthetic/topic.html")


class SectionTests(unittest.TestCase):
    def test_only_a_direct_h2_sectiontitle_names_a_section(self) -> None:
        root = module.parse_html(
            """<section><div><h2 id="wrong" class="sectiontitle">Wrong</h2></div></section>
            <section><h2 id="right" class="sectiontitle extra">Options</h2></section>"""
        )
        sections = module.direct_sections(root)
        self.assertEqual(list(sections), ["Options"])
        self.assertEqual(sections["Options"].section_id, "right")

    def test_duplicate_section_title_fails_closed(self) -> None:
        root = module.parse_html(
            """<section><h2 id="a" class="sectiontitle">Options</h2></section>
            <section><h2 id="b" class="sectiontitle">Options</h2></section>"""
        )
        with self.assertRaisesRegex(module.ProjectionError, "duplicate section"):
            module.direct_sections(root)

    def test_idless_section_title_fails_closed(self) -> None:
        root = module.parse_html(
            '<section><h2 class="sectiontitle">Conditions</h2></section>'
        )
        with self.assertRaisesRegex(module.ProjectionError, "title or id"):
            module.direct_sections(root)


class DefinitionTests(unittest.TestCase):
    def setUp(self) -> None:
        root = module.parse_html(
            """<section><h2 id="conditions" class="sectiontitle">Conditions</h2>
            <dl><dt class="dthd">Condition</dt><dd class="ddhd">Meaning</dd>
              <dt>A(data-area)</dt><dd>first</dd><dd>second
                <dl><dt>16 INVREQ</dt><dd><dl><dt>30</dt><dd>reason</dd></dl></dd></dl>
              </dd>
              <dt>B</dt><dd>third</dd>
            </dl></section>"""
        )
        self.section = module.direct_sections(root)["Conditions"]

    def test_consecutive_descriptions_stay_with_their_preceding_term(self) -> None:
        groups = module.definition_groups(self.section)
        top = [group for group in groups if group.depth == 0]
        self.assertEqual([group.stack[-1] for group in top], ["A(data-area)", "B"])
        self.assertEqual(len(top[0].descriptions), 2)
        self.assertEqual(len(top[1].descriptions), 1)

    def test_nested_resp2_terms_retain_the_full_stack(self) -> None:
        groups = module.definition_groups(self.section)
        nested = next(group for group in groups if group.stack[-1] == "30")
        self.assertEqual(nested.stack, ("A(data-area)", "16 INVREQ", "30"))
        self.assertEqual(nested.depth, 2)

    def test_description_without_a_term_fails_closed(self) -> None:
        root = module.parse_html(
            '<section><h2 id="o" class="sectiontitle">Options</h2><dl><dd>x</dd></dl></section>'
        )
        with self.assertRaisesRegex(module.ProjectionError, "no preceding term"):
            module.definition_groups(module.direct_sections(root)["Options"])

    def test_plain_text_operand_survives_a_trailing_only_annotation(self) -> None:
        root = module.parse_html("<dl><dt>TOKEN(data-area) (RLS only)</dt></dl>")
        term = next(node for node in module.walk(root) if node.tag == "dt")
        self.assertEqual(module._argument_markers(term), ["data-area"])

    def test_condition_symbol_uses_only_pinned_response_codes(self) -> None:
        codes = {"INVREQ": 16, "LENGERR": 22, "NOTAUTH": 70}
        self.assertEqual(
            module._symbolic_condition_term("INVREQ", 0, codes), "RESP-16-INVREQ"
        )
        self.assertEqual(
            module._symbolic_condition_term("UNKNOWN", 0, codes), "SYMBOL-UNKNOWN"
        )


class SyntaxTests(unittest.TestCase):
    def setUp(self) -> None:
        root = module.parse_html(SYNTAX_HTML)
        self.diagram = module.syntax_diagrams(module.direct_sections(root)["Syntax"])[0]

    def test_keyword_variable_and_delimiter_leaves_remain_distinct(self) -> None:
        items = module.syntax_items(self.diagram)
        sysid = next(item for item in items if item.tokens[0].value == "SYSID(")
        self.assertEqual(
            [(token.kind, token.value) for token in sysid.tokens],
            [("keyword", "SYSID("), ("variable", "systemname"), ("delimiter", ")")],
        )

    def test_dynamic_condition_and_timer_default_are_not_literal_keywords(self) -> None:
        root = module.parse_html(
            """<svg><g>
            <text class="syntaxkwd">condition</text>
            <text class="syntaxkwd">today</text>
            <text class="syntaxkwd">NATLANG</text>
            <text class="syntaxkwd">'en'</text>
            </g></svg>"""
        )
        tokens = module._semantic_tokens(root)
        self.assertEqual(
            [(token.kind, token.value) for token in tokens],
            [
                ("variable", "condition"),
                ("fragment", "today"),
                ("keyword", "NATLANG"),
                ("keyword", "'en'"),
            ],
        )

    def test_required_optional_and_alternative_relations_are_structural(self) -> None:
        relations = {
            item.tokens[0].value: item.relation for item in module.syntax_items(self.diagram)
        }
        self.assertEqual(relations["ALLOCATE"], "required")
        self.assertEqual(relations["SYSID("], "required")
        self.assertEqual(relations["PROFILE("], "optional")
        self.assertEqual(relations["PARTNER("], "alternative")
        profile = next(
            item for item in module.syntax_items(self.diagram)
            if item.tokens[0].value == "PROFILE("
        )
        partner = next(
            item for item in module.syntax_items(self.diagram)
            if item.tokens[0].value == "PARTNER("
        )
        self.assertNotEqual(
            module.syntax_group_path(profile.tokens[0], self.diagram),
            module.syntax_group_path(partner.tokens[0], self.diagram),
        )
        self.assertEqual(module.syntax_piece_ordinal(profile.tokens[0], self.diagram), 1)

    def test_compound_command_prefix_is_not_an_operand_option(self) -> None:
        node = module.Node("g", {})
        item = module.SyntaxItem(
            "required",
            (
                module.SyntaxToken("keyword", "SET", node),
                module.SyntaxToken("keyword", "ASSOCIATION", node),
                module.SyntaxToken("keyword", "USERCORRDATA(", node),
                module.SyntaxToken("variable", "data-value", node),
            ),
            node,
        )
        self.assertEqual(
            module._syntax_option_name(item, {"USERCORRDATA"}), "USERCORRDATA"
        )

    def test_multiple_svg_pieces_share_one_diagram_title(self) -> None:
        body = SYNTAX_HTML.replace("</svg>", "</svg><svg class='syntaxdiagram'><g class='diagram'><g class='boxed groupcomp'><g><text class='syntaxkwd'>SECOND</text></g></g></g></svg>")
        root = module.parse_html(body)
        diagrams = module.syntax_diagrams(module.direct_sections(root)["Syntax"])
        self.assertEqual(len(diagrams), 1)
        self.assertEqual(len(diagrams[0].pieces), 2)

    def test_groupcomp_with_multiple_options_does_not_merge_their_operands(self) -> None:
        body = SYNTAX_HTML.replace(
            '<g class="boxed groupcomp">\n            <g class="unboxed syntaxkwd"><g><text class="syntaxkwd">ALLOCATE</text></g></g>\n          </g>',
            """<g class="boxed groupcomp">
              <g class="unboxed syntaxkwd"><text class="syntaxkwd">FIRST(</text>
                <text class="syntaxvar">data-area</text><text class="syntaxdelim">)</text></g>
              <g class="unboxed syntaxkwd"><text class="syntaxkwd">SECOND(</text>
                <text class="syntaxvar">data-value</text><text class="syntaxdelim">)</text></g>
            </g>""",
        )
        root = module.parse_html(body)
        diagram = module.syntax_diagrams(module.direct_sections(root)["Syntax"])[0]
        items = module.syntax_items(diagram)
        by_keyword = {item.tokens[0].value: item for item in items}
        self.assertEqual(
            [token.value for token in by_keyword["FIRST("].tokens],
            ["FIRST(", "data-area", ")"],
        )
        self.assertEqual(
            [token.value for token in by_keyword["SECOND("].tokens],
            ["SECOND(", "data-value", ")"],
        )

    def test_svg_without_a_title_fails_closed(self) -> None:
        root = module.parse_html(
            '<section><h2 id="s" class="sectiontitle">Syntax</h2><div class="syntaxdiagram"><svg class="syntaxdiagram"></svg></div></section>'
        )
        with self.assertRaisesRegex(module.ProjectionError, "stable identity"):
            module.syntax_diagrams(module.direct_sections(root)["Syntax"])

    def test_shared_asktime_syntax_does_not_leak_abstime_to_bare_row(self) -> None:
        root = module.parse_html(
            """<section><h2 id="s" class="sectiontitle">Syntax</h2>
            <div class="syntaxdiagram"><h3 id="d" class="syntaxdiagram-title">ASKTIME</h3>
            <svg class="syntaxdiagram"><g class="diagram">
              <g class="boxed groupcomp"><g class="unboxed syntaxkwd">
                <g><text class="syntaxkwd">ASKTIME</text></g>
              </g></g>
              <g class="groupseq"><g></g><g class="boxed groupcomp">
                <g class="unboxed syntaxkwd">
                  <g><text class="syntaxkwd">ABSTIME(</text></g>
                  <g><text class="syntaxvar">data-area</text></g>
                  <g><text class="syntaxdelim">)</text></g>
                </g>
              </g></g>
            </g></svg></div></section>"""
        )
        diagram = module.syntax_diagrams(module.direct_sections(root)["Syntax"])[0]
        rows = [
            {
                "official_row": "ibm-cics-ts-6x-2026-08-31:api-commands:0009",
                "label": "ASKTIME",
                "selection_kind": "shared-page",
            },
            {
                "official_row": "ibm-cics-ts-6x-2026-08-31:api-commands:0010",
                "label": "ASKTIME ABSTIME",
                "selection_kind": "shared-page",
            },
        ]
        bare = module._syntax_items_for_row(rows[0], rows, diagram)
        abstime = module._syntax_items_for_row(rows[1], rows, diagram)
        self.assertNotIn("ABSTIME(", [token.value for item in bare for token in item.tokens])
        self.assertIn("ABSTIME(", [token.value for item in abstime for token in item.tokens])

    def test_shared_acquire_prunes_the_complete_sibling_choice_branch(self) -> None:
        root = module.parse_html(
            """<section><h2 id="s" class="sectiontitle">Syntax</h2>
            <div class="syntaxdiagram"><h3 id="d" class="syntaxdiagram-title">ACQUIRE</h3>
            <svg class="syntaxdiagram"><g class="diagram"><g class="groupcomp">
              <g class="boxed groupcomp"><g class="unboxed syntaxkwd">
                <g><text class="syntaxkwd">ACQUIRE</text></g></g></g>
              <g class="groupchoice">
                <g class="groupseq">
                  <g class="boxed groupcomp"><g class="unboxed syntaxkwd">
                    <g><text class="syntaxkwd">PROCESS(</text></g>
                    <g><text class="syntaxvar">data-value</text></g>
                    <g><text class="syntaxdelim">)</text></g></g></g>
                  <g class="boxed groupcomp"><g class="unboxed syntaxkwd">
                    <g><text class="syntaxkwd">PROCESSTYPE(</text></g>
                    <g><text class="syntaxvar">data-value</text></g>
                    <g><text class="syntaxdelim">)</text></g></g></g>
                </g>
                <g class="boxed groupcomp"><g class="unboxed syntaxkwd">
                  <g><text class="syntaxkwd">ACTIVITYID(</text></g>
                  <g><text class="syntaxvar">data-value</text></g>
                  <g><text class="syntaxdelim">)</text></g></g></g>
              </g>
            </g></g></svg></div></section>"""
        )
        diagram = module.syntax_diagrams(module.direct_sections(root)["Syntax"])[0]
        rows = [
            {
                "official_row": "ibm-cics-ts-6x-2026-08-31:api-commands:0002",
                "label": "ACQUIRE ACTIVITYID",
                "selection_kind": "shared-page",
            },
            {
                "official_row": "ibm-cics-ts-6x-2026-08-31:api-commands:0003",
                "label": "ACQUIRE PROCESS",
                "selection_kind": "shared-page",
            },
        ]
        activity = module._syntax_items_for_row(rows[0], rows, diagram)
        process = module._syntax_items_for_row(rows[1], rows, diagram)
        activity_tokens = [token.value for item in activity for token in item.tokens]
        process_tokens = [token.value for item in process for token in item.tokens]
        self.assertEqual(
            [value for value in activity_tokens if value.endswith("(")],
            ["ACTIVITYID("],
        )
        self.assertEqual(
            [value for value in process_tokens if value.endswith("(")],
            ["PROCESS(", "PROCESSTYPE("],
        )
        activity_item = next(
            item for item in activity if item.tokens[0].value == "ACTIVITYID("
        )
        self.assertEqual(activity_item.relation, "required")


class SupplementBoundaryTests(unittest.TestCase):
    def test_uppercase_nested_enum_is_not_an_operand_marker(self) -> None:
        root = module.parse_html("<dl><dt>HOSTNAME</dt></dl>")
        term = next(node for node in module.walk(root) if node.tag == "dt")
        self.assertEqual(module._argument_markers(term), ["none"])

    def test_dpl_table_does_not_prefix_match_distinct_command_families(self) -> None:
        root = module.parse_html(
            """<table><tbody>
            <tr><td>FREE</td><td>all</td></tr>
            <tr><td>LINK</td><td>INPUTMSG INPUTMSGLEN</td></tr>
            <tr><td>RECEIVE</td><td>all</td></tr>
            <tr><td>START</td><td>TERMID where intersystem</td></tr>
            <tr><td>SEND</td><td>MAP TEXT</td></tr>
            <tr><td>WAIT TERMINAL</td><td>all</td></tr>
            </tbody></table>"""
        )
        table = next(node for node in module.walk(root) if node.tag == "table")
        for label in (
            "FREE CHILD",
            "LINK ACQACTIVITY",
            "RECEIVE MAP",
            "RECEIVE PARTN",
            "START ATTACH",
            "START BREXIT",
        ):
            self.assertEqual(
                module._dpl_server_applicability(label, table),
                {"dpl_server": "allowed"},
            )
        self.assertEqual(
            module._dpl_server_applicability("SEND MAP", table)["dpl_server"],
            "restricted",
        )
        send = module._dpl_server_applicability("SEND", table)["dpl_restriction"]
        self.assertEqual(send["kind"], "conditional")
        self.assertEqual(send["prohibited_options"], ["MAP", "TEXT"])
        self.assertEqual(send["restriction_predicates"], ["principal-facility"])
        self.assertEqual(send["restriction_match"], "any-of")
        wait = module._dpl_server_applicability("WAIT TERMINAL", table)[
            "dpl_restriction"
        ]
        self.assertEqual(wait["kind"], "conditional")
        self.assertEqual(wait["restriction_predicates"], ["principal-facility"])

    def test_threadsafe_source_typo_has_one_narrow_catalog_alias(self) -> None:
        root = module.parse_html("<ul><li>REQUEST ENCYRPTPTKT</li></ul>")
        item = next(node for node in module.walk(root) if node.tag == "ul")
        self.assertEqual(
            module._threadsafe_applicability(
                "REQUEST ENCRYPTPTKT", item, "mapped"
            ),
            {"threadsafe": "yes"},
        )

    def test_threadsafe_web_family_requires_every_pinned_form(self) -> None:
        root = module.parse_html(
            """<ul><li>WEB READ FORMFIELD</li><li>WEB READ HTTPHEADER</li>
            <li>WEB READ QUERYPARM</li><li>WEB WRITE HTTPHEADER</li></ul>"""
        )
        item = next(node for node in module.walk(root) if node.tag == "ul")
        self.assertEqual(
            module._threadsafe_applicability("WEB READ", item, "mapped"),
            {"threadsafe": "yes"},
        )
        self.assertEqual(
            module._threadsafe_applicability("WEB WRITE", item, "mapped"),
            {"threadsafe": "yes"},
        )
        self.assertEqual(
            module._threadsafe_applicability("CICSMESSAGE", item, "source-gap"),
            {"threadsafe": "not-applicable"},
        )

    def test_dpl_prose_and_principal_facility_are_structured(self) -> None:
        root = module.parse_html(
            """<section><p>Any of the EXEC CICS WEB commands fail with a RESP2
            value of 1. WEB EXTRACT, EXTRACT TCPIP and EXTRACT CERTIFICATE fail
            with a RESP2 value of 5. APPC commands listed are prohibited only
            when they refer to the principal facility.</p><table><tbody>
            <tr><td>CONNECT PROCESS</td><td>all</td></tr>
            <tr><td>RECEIVE</td><td>all</td></tr>
            </tbody></table></section>"""
        )
        section = next(node for node in module.walk(root) if node.tag == "section")
        web = module._dpl_server_applicability("WEB CLOSE", section)
        extract = module._dpl_server_applicability("WEB EXTRACT", section)
        connect = module._dpl_server_applicability("CONNECT PROCESS", section)
        self.assertEqual(web["dpl_restriction"]["resp2"], 1)
        self.assertEqual(extract["dpl_restriction"]["resp2"], 5)
        self.assertEqual(connect["dpl_restriction"]["kind"], "conditional")
        self.assertEqual(
            connect["dpl_restriction"]["restriction_predicates"],
            [
                "principal-facility",
                "connect-process-principal-facility-error-is-dpl",
            ],
        )

    def test_conditional_threadsafe_profile_preserves_predicates(self) -> None:
        root = module.parse_html("<ul><li>READ *</li><li>ABEND</li></ul>")
        item = next(node for node in module.walk(root) if node.tag == "ul")
        read = module._threadsafe_applicability("READ FILE", item, "mapped")
        self.assertEqual(read["threadsafe"], "conditional")
        self.assertEqual(
            read["threadsafe_condition"]["profile"],
            "file-control-storage-and-locality",
        )
        self.assertIn(
            "file-nsr", read["threadsafe_condition"]["not_threadsafe_when"]
        )
        self.assertEqual(
            module._threadsafe_applicability("ABEND", item, "mapped"),
            {"threadsafe": "yes"},
        )

    def test_language_profiles_are_exact_and_bounded(self) -> None:
        gds = module.parse_html(
            "<p class='shortdesc'>APPC basic conversation "
            "(assembler-language and C programs only).</p>"
        )
        node = next(module.walk(gds))
        self.assertEqual(module._language_profile(node), "assembler-and-c-only")
        self.assertEqual(
            module._language_applicability("assembler-and-c-only")["cobol"],
            "not-applicable",
        )

    def test_readq_ts_dependency_is_bounded_from_description_fragment(self) -> None:
        root = module.parse_html(
            "<section><h2 class='sectiontitle' id='options'>Options</h2><dl>"
            "<dt>LENGTH(data-area)</dt><dd>If you specify the SET option, the "
            "LENGTH must be specified.</dd></dl></section>"
        )
        section = next(node for node in module.walk(root) if node.tag == "section")
        group = module.definition_groups(
            module.Section("Options", "options", section)
        )[0]
        self.assertEqual(len(module._option_legality_descriptions(group)), 1)

    def test_syntax_head_excludes_valued_catalog_discriminator(self) -> None:
        cases = (
            (
                "ACQUIRE ACTIVITYID",
                [
                    {"kind": "keyword", "value": "ACQUIRE", "relation": "required"},
                    {"kind": "keyword", "value": "ACTIVITYID(", "relation": "required"},
                ],
                {"PROCESSTYPE", "ACTIVITYID"},
                "ACQUIRE",
                [{"name": "ACTIVITYID", "state": "present"}],
            ),
            (
                "SEND MAP",
                [{"kind": "keyword", "value": "SEND MAP(", "relation": "required"}],
                {"MAP"},
                "SEND",
                [{"name": "MAP", "state": "present"}],
            ),
            (
                "WRITE FILE",
                [{"kind": "keyword", "value": "WRITE FILE(", "relation": "required"}],
                {"FILE"},
                "WRITE",
                [{"name": "FILE", "state": "present"}],
            ),
        )
        for label, tokens, options, head, discriminators in cases:
            actual = module._syntax_head(tokens, options)
            self.assertEqual(actual, head)
            self.assertEqual(
                module._syntax_identity(label, actual, tokens)[1], discriminators
            )
        self.assertEqual(
            module._syntax_identity(
                "ACQUIRE ACTIVITYID",
                "ACQUIRE",
                [
                    {"kind": "keyword", "value": "ACQUIRE", "relation": "required"},
                    {"kind": "keyword", "value": "ACTIVITYID(", "relation": "alternative"},
                ],
            )[1],
            [],
        )
        address_tokens = [
            {
                "kind": "keyword",
                "value": "ADDRESS",
                "relation": "required",
                "group_path": "groupcomp[1]",
            },
            {
                "kind": "keyword",
                "value": "SET(",
                "relation": "required",
                "group_path": "groupchoice[2]/groupseq[1]/groupcomp[1]",
            },
            {
                "kind": "keyword",
                "value": "USING(",
                "relation": "required",
                "group_path": "groupchoice[2]/groupseq[1]/groupcomp[2]",
            },
            {
                "kind": "keyword",
                "value": "SET(",
                "relation": "alternative",
                "group_path": "groupchoice[2]/groupseq[2]/groupcomp[1]",
            },
            {
                "kind": "keyword",
                "value": "USING(",
                "relation": "alternative",
                "group_path": "groupchoice[2]/groupseq[2]/groupcomp[2]",
            },
        ]
        self.assertEqual(
            module._syntax_identity("ADDRESS SET", "ADDRESS", address_tokens)[1],
            [{"name": "SET", "state": "present"}],
        )
        self.assertEqual(
            module._syntax_identity(
                "WAIT",
                "GDS WAIT",
                [{"kind": "keyword", "value": "GDS WAIT", "relation": "required"}],
            )[0],
            "documented-alias",
        )

    def test_supplement_aliasing_is_rejected(self) -> None:
        candidate = {
            "kind": "source-syntax",
            "candidate_value": {
                "tokens": [{"kind": "keyword", "value": "TRACENUM"}]
            },
            "evidence": {
                "topic_path": "SSNAQ8_11.1.0/reference-api/r_enter.html"
            },
        }
        with self.assertRaisesRegex(module.ProjectionError, "aliases a distinct"):
            module._reject_supplement_alias(
                "ibm-cics-ts-6x-2026-08-31:api-commands:0065",
                "SSNAQ8_11.1.0/reference-api/r_enter.html",
                [candidate],
                ["TRACENUM"],
            )

    def test_wrong_direct_supplement_product_is_rejected(self) -> None:
        plan = module.read_json(module.ROOT / module.PLAN_PATH)
        plan = copy.deepcopy(plan)
        row = next(
            item
            for item in plan["source_resolutions"]
            if item["label"] == "DUMP"
        )
        row["direct_supplement_topic"] = (
            "SSXJAJ_14.1.0/com.ibm.faultanalyzer.doc_14.1/idiug166.html"
        )
        plan["extraction_sha256"] = module.canonical_digest(
            plan, module.PLAN_DOMAIN, "extraction_sha256"
        )
        with self.assertRaisesRegex(module.ProjectionError, "direct-product bindings"):
            module.validate_plan(plan)

    def test_supplement_receipt_hash_drift_is_rejected(self) -> None:
        plan = module.read_json(module.ROOT / module.PLAN_PATH)
        plan = copy.deepcopy(plan)
        plan["inputs"]["supplements"]["file_sha256"] = "sha256:" + "0" * 64
        plan["extraction_sha256"] = module.canonical_digest(
            plan, module.PLAN_DOMAIN, "extraction_sha256"
        )
        module.validate_plan(plan)
        with self.assertRaisesRegex(module.ProjectionError, "input identity"):
            module._input_files(module.ROOT, plan)


class LinkAndDigestTests(unittest.TestCase):
    def test_one_hop_links_are_canonical_unique_and_same_origin(self) -> None:
        source = "SSJL4D_6.x/reference-applications/commands-api/source.html"
        root = module.parse_html(
            """<a href="context.html#x">one</a><a href="context.html">two</a>
            <a href="/docs/en/SSJL4D_6.x/reference-applications/common.html?x=1">common</a>
            <a href="https://example.invalid/no.html">foreign</a><a href="#local">local</a>"""
        )
        self.assertEqual(
            module.one_hop_anchors(root, source),
            [
                "SSJL4D_6.x/reference-applications/commands-api/context.html",
                "SSJL4D_6.x/reference-applications/common.html",
            ],
        )
        self.assertEqual(
            module.linked_reference("context.html#exact", source),
            (
                "SSJL4D_6.x/reference-applications/commands-api/context.html",
                "exact",
            ),
        )

    def test_one_hop_target_anchor_must_resolve_exactly_once(self) -> None:
        root = module.parse_html(
            '<article><section><h2 id="target" class="sectiontitle">Target</h2></section></article>'
        )
        target = module.exact_target_fragment(root, "target", "synthetic/topic.html")
        self.assertEqual(target.tag, "h2")
        duplicate = module.parse_html(
            '<article><p id="target">a</p><p id="target">b</p></article>'
        )
        with self.assertRaisesRegex(module.ProjectionError, "not unique"):
            module.exact_target_fragment(duplicate, "target", "synthetic/topic.html")

    def test_dita_topic_id_anchor_resolves_to_unique_article_root(self) -> None:
        root = module.parse_html(
            '<article><h1 id="topic__title__1" class="topictitle1">Topic</h1>'
            '<section><h2 id="topic__section" class="sectiontitle">Part</h2></section>'
            "</article>"
        )
        target = module.exact_target_fragment(root, "topic", "synthetic/topic.html")
        self.assertEqual(target.tag, "article")

    def test_canonical_digest_omits_only_the_digest_field(self) -> None:
        left = {"b": 2, "a": 1, "projection_sha256": "old"}
        right = {"a": 1, "projection_sha256": "new", "b": 2}
        self.assertEqual(
            module.canonical_digest(left, module.OUTPUT_DOMAIN, "projection_sha256"),
            module.canonical_digest(right, module.OUTPUT_DOMAIN, "projection_sha256"),
        )

    def test_fragment_hash_is_domain_separated(self) -> None:
        self.assertNotEqual(
            module.fragment_sha256("same"),
            module.hashlib.sha256(b"same").hexdigest(),
        )

    def test_evidence_hash_is_reproducible_from_its_exact_locator_node(self) -> None:
        root = module.parse_html('<article><p id="fragment">exact text</p></article>')
        node = next(item for item in module.walk(root) if item.attrs.get("id") == "fragment")
        evidence = module.make_evidence(
            "SSJL4D_6.x/reference/example.html",
            "0" * 64,
            "one-hop-context",
            "__topic__",
            node,
            {"max_normalized_fragment_bytes": 1024, "max_locator_bytes": 1024},
        )
        self.assertEqual(
            evidence["fragment_sha256"],
            "sha256:" + module.fragment_sha256(module.normalized_fragment(node)),
        )

    def test_symbolic_projection_does_not_copy_definition_prose(self) -> None:
        prose = "SET or NODATA option specified"
        self.assertEqual(module._symbolic_option_stack([prose]), ["SET"])
        self.assertNotIn(prose, module._symbolic_option_stack([prose]))
        condition = module._symbolic_condition_term(
            "Only when a prior request exists", 1
        )
        self.assertRegex(condition, r"^FRAGMENT-[0-9A-F]{12}$")

    def test_argument_markers_split_alternatives_and_ignore_annotations(self) -> None:
        self.assertEqual(
            module._markers_in_text("ptr-ref | data-area"),
            ["data-area", "ptr-ref"],
        )
        self.assertEqual(module._markers_in_text("poolname"), ["name"])
        self.assertTrue(module._non_operand_annotation("VSAM KSDS only"))

    def test_identity_collision_fails_closed(self) -> None:
        left = {"candidate_id": "same", "value": 1}
        right = {"candidate_id": "same", "value": 2}
        with self.assertRaisesRegex(module.ProjectionError, "identity collision"):
            module._coalesce_by_identity(
                [left, right], "candidate_id", "candidate"
            )


class RepositoryProjectionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.root = TOOL.parents[3]
        cls.output_path = cls.root / module.OUTPUT_PATH
        cls.output = json.loads(cls.output_path.read_text(encoding="utf-8"))
        cls.manifest = json.loads(
            (cls.root / module.MANIFEST_PATH).read_text(encoding="utf-8")
        )
        cls.supplements = json.loads(
            (cls.root / module.SUPPLEMENTS_PATH).read_text(encoding="utf-8")
        )

    def test_frozen_projection_identity_counts_and_zero_credit_are_exact(self) -> None:
        self.assertEqual(self.output["status"], "candidate")
        self.assertEqual(self.output["review_status"], "unreviewed")
        self.assertFalse(self.output["semantic_authority"])
        self.assertFalse(self.output["automatic_registration"])
        self.assertEqual(self.output["coverage_credit"], 0)
        self.assertEqual(self.output["differential_credit"], 0)
        fixed_counts = {
            key: self.output["counts"][key]
            for key in (
                "rows",
                "dimension_records",
                "blocking_issues",
                "projected_dimensions",
                "declared_absent_dimensions",
                "source_gap_dimensions",
                "unmatched_dimensions",
                "conflicting_dimensions",
                "source_backed_not_applicable_dimensions",
                "syntax_diagrams",
                "syntax_svg_parts",
                "top_level_option_terms",
                "top_level_condition_terms",
            )
        }
        self.assertEqual(
            fixed_counts,
            {
                "rows": 88,
                "dimension_records": 440,
                "blocking_issues": 304,
                "projected_dimensions": 422,
                "declared_absent_dimensions": 14,
                "source_gap_dimensions": 0,
                "unmatched_dimensions": 0,
                "conflicting_dimensions": 0,
                "source_backed_not_applicable_dimensions": 4,
                "syntax_diagrams": 111,
                "syntax_svg_parts": 112,
                "top_level_option_terms": 895,
                "top_level_condition_terms": 344,
            },
        )

    def test_committed_projection_guard_passes_without_source_cache(self) -> None:
        checked = module.check_committed(self.root)
        self.assertEqual(checked["projection_sha256"], self.output["projection_sha256"])

    def test_projection_digest_and_input_file_identities_recompute(self) -> None:
        self.assertEqual(
            self.output["projection_sha256"],
            module.canonical_digest(
                self.output, module.OUTPUT_DOMAIN, "projection_sha256"
            ),
        )
        for item in self.output["inputs"].values():
            self.assertEqual(
                item["file_sha256"], module.file_sha256(self.root / item["path"])
            )
        self.assertEqual(
            self.output["extractor"]["implementation_sha256"],
            module.file_sha256(Path(module.__file__)),
        )
        self.assertEqual(
            self.output_path.read_text(encoding="utf-8"), module.pretty(self.output)
        )

    def test_eibresp_condition_authority_and_dynamic_clauses_are_exact(self) -> None:
        authority = self.output["condition_name_authority"]
        pairs = [[item["name"], item["code"]] for item in authority["conditions"]]
        self.assertEqual(len(pairs), 121)
        self.assertEqual(pairs, sorted(pairs))
        self.assertEqual(authority["allowed_names"], [name for name, _ in pairs])
        self.assertEqual(
            hashlib.sha256(
                json.dumps(pairs, separators=(",", ":")).encode("utf-8")
            ).hexdigest(),
            "33e16a2f60928dbd2168e1e3ad6e5e5b441fdca1c493c55b493d6452bf4d27e0",
        )
        by_name = dict(pairs)
        self.assertEqual(
            {name: by_name[name] for name in ("NORMAL", "ERROR", "BUSY")},
            {"NORMAL": 0, "ERROR": 1, "BUSY": 128},
        )
        self.assertEqual(
            set(range(129)) - set(by_name.values()),
            {67, 68, 73, 74, 76, 77, 78, 79},
        )

        batch_b_path = (
            self.root
            / "conformance/0.9/generated/cics-application-api-sources-b-candidates.json"
        )
        batch_b = json.loads(batch_b_path.read_text(encoding="utf-8"))
        rows = {row["label"]: row for row in batch_b["rows"]}
        for label, label_operand in (
            ("HANDLE CONDITION", "optional"),
            ("IGNORE CONDITION", "forbidden"),
        ):
            syntax = next(
                candidate["candidate_value"]
                for dimension in rows[label]["dimensions"]
                if dimension["name"] == "syntax"
                for candidate in dimension["candidates"]
                if candidate["kind"] == "source-syntax"
            )
            self.assertEqual(syntax["syntax_head"], label)
            condition = next(
                token for token in syntax["tokens"] if token["value"] == "condition"
            )
            self.assertEqual(condition["kind"], "variable")
            dynamic = next(
                candidate["candidate_value"]
                for dimension in rows[label]["dimensions"]
                if dimension["name"] == "options"
                for candidate in dimension["candidates"]
                if candidate["candidate_value"].get("dynamic_name_profile")
            )
            self.assertEqual(dynamic["term"], "CONDITION-NAME")
            self.assertEqual(dynamic["minimum_occurrences"], 1)
            self.assertEqual(dynamic["maximum_occurrences"], 16)
            self.assertEqual(dynamic["label_operand"], label_operand)

        timer = next(row for row in self.output["rows"] if row["label"] == "DEFINE TIMER")
        timer_tokens = [
            token
            for dimension in timer["dimensions"]
            for candidate in dimension["candidates"]
            if candidate["kind"] == "source-syntax"
            for token in candidate["candidate_value"]["tokens"]
            if token["value"].casefold() == "today"
        ]
        self.assertTrue(timer_tokens)
        self.assertTrue(all(token["kind"] == "fragment" for token in timer_tokens))

    def test_every_candidate_traces_to_a_pinned_topic_and_unique_identity(self) -> None:
        pins = {item["topic_path"]: item["sha256"] for item in self.manifest["topics"]}
        supplement_by_path = {
            item["topic_path"]: item for item in self.supplements["topics"]
        }
        pins.update(
            {
                item["topic_path"]: item["sha256"]
                for item in self.supplements["topics"]
            }
        )
        candidate_ids: set[str] = set()
        candidate_count = 0
        source_links = 0
        target_fragments = 0
        for row in self.output["rows"]:
            self.assertEqual(
                [item["name"] for item in row["dimensions"]], list(module.DIMENSIONS)
            )
            row_topics = {item["topic_path"]: item["topic_sha256"] for item in row["topics"]}
            for dimension in row["dimensions"]:
                for candidate in dimension["candidates"]:
                    candidate_count += 1
                    self.assertNotIn(candidate["candidate_id"], candidate_ids)
                    candidate_ids.add(candidate["candidate_id"])
                    evidence = candidate["evidence"]
                    self.assertEqual(
                        evidence["topic_sha256"], "sha256:" + pins[evidence["topic_path"]]
                    )
                    self.assertEqual(row_topics[evidence["topic_path"]], pins[evidence["topic_path"]])
                    self.assertRegex(evidence["fragment_sha256"], r"^sha256:[0-9a-f]{64}$")
                    product = evidence["topic_path"].split("/", 1)[0]
                    self.assertEqual(evidence["source_product"], product)
                    self.assertEqual(
                        evidence["target_authority_boundary"],
                        supplement_by_path.get(evidence["topic_path"], {}).get(
                            "target_authority_boundary", module.TARGET_AUTHORITY
                        ),
                    )
                    source_links += candidate["key"].startswith("one-hop-source-")
                    target_fragments += candidate["key"].startswith("one-hop-target-")
        self.assertEqual(candidate_count, self.output["counts"]["candidates"])
        self.assertEqual((source_links, target_fragments), (257, 257))

    def test_blockers_are_exact_and_dimension_local(self) -> None:
        top = {item["issue_id"]: item for item in self.output["blocking_issues"]}
        local = {
            issue["issue_id"]: issue
            for row in self.output["rows"]
            for dimension in row["dimensions"]
            for issue in dimension["issues"]
        }
        self.assertEqual(top, local)
        counts = {}
        for issue in top.values():
            counts[issue["code"]] = counts.get(issue["code"], 0) + 1
        self.assertEqual(
            counts,
            {
                "target-equivalence-ambiguity": 2,
                "prose-option-legality-not-structured": 266,
                "context-predicate-not-structured": 11,
                "shared-condition-applicability-unresolved": 25,
            },
        )

    def test_gap_rows_are_reprojected_without_aliasing_target_commands(self) -> None:
        by_number = {
            row["official_row"].rsplit(":", 1)[-1]: row for row in self.output["rows"]
        }
        internal = by_number["0027"]
        self.assertEqual(internal["row_state"], "projected")
        self.assertEqual(
            [dimension["state"] for dimension in internal["dimensions"]],
            [
                "source-backed-not-applicable",
                "source-backed-not-applicable",
                "source-backed-not-applicable",
                "source-backed-not-applicable",
                "projected",
            ],
        )
        for number in ("0056", "0065"):
            row = by_number[number]
            self.assertEqual(row["row_state"], "projected")
            self.assertEqual(
                {dimension["state"] for dimension in row["dimensions"]},
                {"projected"},
            )
            issues = [
                issue
                for dimension in row["dimensions"]
                for issue in dimension["issues"]
                if issue["code"] == "target-equivalence-ambiguity"
            ]
            self.assertEqual(len(issues), 1)
            self.assertEqual(issues[0]["code"], "target-equivalence-ambiguity")
            self.assertEqual(
                issues[0]["affected_dimensions"],
                ["syntax", "options", "operand-directions", "conditions"],
            )
        trace = by_number["0065"]
        target_context = (
            "SSJL4D_6.x/reference-applications/commands-api/"
            "dfhp4_entertracenum.html"
        )
        for dimension in trace["dimensions"]:
            for candidate in dimension["candidates"]:
                if candidate["evidence"]["topic_path"] == target_context:
                    self.assertEqual(dimension["name"], "execution-context")
                    self.assertEqual(candidate["kind"], "source-context")

    def test_supplements_never_expand_one_hop_links(self) -> None:
        supplement_paths = {
            item["topic_path"] for item in self.supplements["topics"]
        }
        for row in self.output["rows"]:
            for dimension in row["dimensions"]:
                for candidate in dimension["candidates"]:
                    if candidate["evidence"]["topic_path"] in supplement_paths:
                        self.assertFalse(candidate["key"].startswith("one-hop-"))

    def test_asktime_shared_page_split_has_no_abstime_leakage(self) -> None:
        rows = {
            row["official_row"].rsplit(":", 1)[-1]: row for row in self.output["rows"]
        }
        bare = rows["0009"]
        abstime = rows["0010"]
        self.assertEqual(bare["dimensions"][1]["state"], "declared-absent")
        self.assertEqual(bare["dimensions"][2]["state"], "declared-absent")
        bare_syntax = next(
            candidate["candidate_value"]
            for candidate in bare["dimensions"][0]["candidates"]
            if candidate["kind"] == "source-syntax"
        )
        abstime_syntax = next(
            candidate["candidate_value"]
            for candidate in abstime["dimensions"][0]["candidates"]
            if candidate["kind"] == "source-syntax"
        )
        self.assertNotIn("ABSTIME", [token["value"] for token in bare_syntax["tokens"]])
        self.assertIn(
            {"name": "ABSTIME", "state": "absent"},
            bare_syntax["identity_discriminators"],
        )
        self.assertIn(
            {"name": "ABSTIME", "state": "present"},
            abstime_syntax["identity_discriminators"],
        )

    def test_acquire_options_and_directions_follow_the_selected_syntax_branch(self) -> None:
        rows = {
            row["official_row"].rsplit(":", 1)[-1]: row for row in self.output["rows"]
        }
        expected = {
            "0002": {"ACTIVITYID"},
            "0003": {"PROCESS", "PROCESSTYPE"},
        }
        for number, names in expected.items():
            row = rows[number]
            options = {
                candidate["key"]
                for dimension in row["dimensions"]
                for candidate in dimension["candidates"]
                if candidate["kind"] == "source-option"
            }
            directions = {
                candidate["key"]
                for dimension in row["dimensions"]
                for candidate in dimension["candidates"]
                if candidate["kind"] == "source-operand-direction"
            }
            self.assertEqual(options, names)
            self.assertEqual(directions, names)

    def test_generated_candidate_values_contain_no_publication_prose_fields(self) -> None:
        forbidden = {"text", "html", "quote", "excerpt", "description", "interpretation"}
        symbolic = re.compile(r"^[A-Z0-9][A-Z0-9:_-]{0,127}$")

        def inspect(value):
            if isinstance(value, dict):
                self.assertFalse(forbidden & set(value))
                for child in value.values():
                    inspect(child)
            elif isinstance(value, list):
                for child in value:
                    inspect(child)

        inspect(self.output)
        for row in self.output["rows"]:
            for dimension in row["dimensions"]:
                for candidate in dimension["candidates"]:
                    value = candidate["candidate_value"]
                    if value["type"] == "option":
                        self.assertRegex(value["term"], symbolic)
                        for item in value["stack"]:
                            self.assertRegex(item, symbolic)
                    elif value["type"] == "condition":
                        for item in value["condition_stack"]:
                            self.assertRegex(item, symbolic)


if __name__ == "__main__":
    unittest.main()
