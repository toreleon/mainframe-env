from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest


TOOL = Path(__file__).resolve().parents[1] / "verify_cics_application_sources.py"
spec = importlib.util.spec_from_file_location("verify_cics_application_sources", TOOL)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


HTML_TEMPLATE = """
<div><article>
<h1 id="topic">FOO</h1><div>
<section><h2 class="sectiontitle" id="foo__syntax">Syntax</h2>
<div class="syntaxdiagram" id="foo__diagram">
<h3 class="syntaxdiagram-title" id="foo__diagram_title">FOO</h3>
<svg class="syntaxdiagram"><g class="diagram">
<g class="boxed groupcomp"><g class="unboxed syntaxkwd">
<text class="syntaxkwd">FOO</text>
</g></g>
</g></svg></div></section>
<section><h2 class="sectiontitle" id="foo__options">Options</h2>
<dl><dt>ID(<span class="var">{marker}</span>)</dt><dd>{description}</dd></dl>
</section>
<section><h2 class="sectiontitle" id="foo__conditions">Conditions</h2>
<dl><dt>16 INVREQ</dt><dd>RESP2 values:
<dl><dt>1</dt><dd>The identifier is invalid.</dd></dl></dd></dl>
</section>
</div></article></div>
""".strip()


class Fixture:
    def __init__(
        self,
        directory: str,
        *,
        marker: str = "data-value",
        description: str = "Specifies the input identifier.",
    ) -> None:
        self.root = Path(directory)
        self.cache = self.root / "cache"
        self.cache.mkdir(parents=True)
        self.topic_path = "SSJL4D_6.x/reference-applications/commands-api/foo.html"
        self.body = HTML_TEMPLATE.format(marker=marker, description=description).encode()
        self.digest = module.sha256_bytes(self.body)
        self._write_inputs()
        self.snapshot = module.build_source_snapshot(self.root, self.cache)
        self.projection = self._projection(marker)
        self.write_projection()

    def _json(self, relative: Path, value: object) -> None:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    def _write_inputs(self) -> None:
        manifest = {
            "topics": [
                {
                    "topic_path": self.topic_path,
                    "sha256": self.digest,
                    "bytes": len(self.body),
                    "last_modified": "2026-09-10",
                }
            ]
        }
        mapping = {
            "rows": [
                {
                    "official_row": "test:api-commands:0001",
                    "label": "FOO",
                    "eibfn": "0001",
                    "state": "mapped",
                    "selection_kind": "exact",
                    "topics": [
                        {
                            "topic_path": self.topic_path,
                            "role": "primary",
                        }
                    ],
                }
            ]
        }
        self._json(module.MANIFEST_PATH, manifest)
        self._json(module.MAP_PATH, mapping)
        self._json(module.PLAN_PATH, {"section_exceptions": []})
        canonical, _ = module.cache_names(self.topic_path, self.digest)
        (self.cache / canonical).write_bytes(self.body)

    def _evidence(self, fact: module.ExpectedFact) -> dict[str, str]:
        topic = self.snapshot.topics[fact.topic_path]
        node = module.resolve_path(topic.document, fact.structural_path)
        return {
            "topic_path": fact.topic_path,
            "topic_sha256": "sha256:" + topic.sha256,
            "source_role": "primary-command",
            "section_id": module.section_id(node),
            "tag": node.tag,
            "structural_path": fact.structural_path,
            "fragment_sha256": module.fragment_sha256(node.text()),
        }

    def _projection(self, marker: str) -> dict:
        dimensions = {
            name: {"name": name, "state": "projected", "candidates": [], "issues": []}
            for name in ("syntax", "options", "operand-directions", "conditions", "execution-context")
        }
        for ordinal, fact in enumerate(sorted(self.snapshot.expected.values(), key=lambda item: item.key())):
            if fact.kind == "source-syntax":
                key = "diagram-0001"
            elif fact.kind == "source-option":
                key = fact.value["term"]
            else:
                key = fact.value["condition_stack"][-1]
            dimensions[fact.dimension]["candidates"].append(
                {
                    "candidate_id": f"candidate-{ordinal:04d}",
                    "kind": fact.kind,
                    "key": key,
                    "candidate_value": copy.deepcopy(fact.value),
                    "evidence": self._evidence(fact),
                }
            )
        option_fact = next(
            fact for fact in self.snapshot.expected.values() if fact.kind == "source-option"
        )
        dimensions["operand-directions"]["candidates"].append(
            {
                "candidate_id": "candidate-direction",
                "kind": "source-operand-direction",
                "key": "ID",
                "candidate_value": {
                    "type": "operand-direction",
                    "option": "ID",
                    "marker": marker,
                    "direction": "input" if marker == "data-value" else "unknown",
                },
                "evidence": self._evidence(option_fact),
            }
        )
        for dimension in dimensions.values():
            dimension["candidates"].sort(key=lambda item: item["candidate_id"])
        return {
            "target_subsystem": "cics.application-api",
            "rows": [
                {
                    "official_row": "test:api-commands:0001",
                    "label": "FOO",
                    "eibfn": "0001",
                    "row_state": "projected",
                    "dimensions": list(dimensions.values()),
                }
            ],
        }

    def write_projection(self) -> None:
        self._json(module.PROJECTION_PATH, self.projection)

    def candidate(self, kind: str) -> dict:
        return next(
            candidate
            for dimension in self.projection["rows"][0]["dimensions"]
            for candidate in dimension["candidates"]
            if candidate["kind"] == kind
        )

    def dimension(self, name: str) -> dict:
        return next(
            item for item in self.projection["rows"][0]["dimensions"] if item["name"] == name
        )


class IndependentCicsSourceVerifierTests(unittest.TestCase):
    def fixture(self, **kwargs) -> tuple[tempfile.TemporaryDirectory, Fixture]:
        directory = tempfile.TemporaryDirectory()
        return directory, Fixture(directory.name, **kwargs)

    def test_tool_has_no_projection_implementation_dependency(self) -> None:
        source = TOOL.read_text(encoding="utf-8")
        forbidden = "extract_cics_application_sources"
        self.assertNotIn(forbidden, source)
        self.assertNotIn("projector.", source)

    def test_dpl_applicability_is_derived_from_command_and_option_rows(self) -> None:
        document = module.parse_fragment(
            b"<table><tbody>"
            b"<tr><td>RECEIVE</td><td>all</td></tr>"
            b"<tr><td>FREE</td><td>all</td></tr>"
            b"<tr><td>LINK</td><td>INPUTMSG INPUTMSGLEN</td></tr>"
            b"<tr><td>START</td><td>TERMID where intersystem</td></tr>"
            b"<tr><td>ISSUE</td><td>ABEND SIGNAL</td></tr>"
            b"<tr><td>SEND</td><td>MAP TEXT</td></tr>"
            b"<tr><td>WAIT TERMINAL</td><td>all</td></tr>"
            b"</tbody></table>"
        )
        self.assertEqual(
            module.independently_derive_dpl("RECEIVE MAP", document),
            {"dpl_server": "allowed"},
        )
        self.assertEqual(
            module.independently_derive_dpl("RECEIVE", document)["dpl_server"],
            "restricted",
        )
        signal = module.independently_derive_dpl("ISSUE SIGNAL", document)
        self.assertEqual(signal["dpl_restriction"]["kind"], "conditional")
        self.assertEqual(
            signal["dpl_restriction"]["restriction_predicates"],
            ["principal-facility"],
        )
        self.assertEqual(
            module.independently_derive_dpl("ISSUE COPY", document),
            {"dpl_server": "allowed"},
        )
        self.assertEqual(
            module.independently_derive_dpl("READ", document),
            {"dpl_server": "allowed"},
        )
        send = module.independently_derive_dpl("SEND", document)["dpl_restriction"]
        self.assertEqual(send["kind"], "conditional")
        self.assertEqual(send["prohibited_options"], ["MAP", "TEXT"])
        self.assertEqual(send["restriction_predicates"], ["principal-facility"])
        self.assertEqual(send["restriction_match"], "any-of")
        wait = module.independently_derive_dpl("WAIT TERMINAL", document)[
            "dpl_restriction"
        ]
        self.assertEqual(wait["kind"], "conditional")
        self.assertEqual(wait["restriction_predicates"], ["principal-facility"])
        for distinct_command in (
            "FREE CHILD",
            "LINK ACQACTIVITY",
            "LINK ACQPROCESS",
            "LINK ACTIVITY",
            "RECEIVE PARTN",
            "START ATTACH",
            "START BREXIT",
        ):
            self.assertEqual(
                module.independently_derive_dpl(distinct_command, document),
                {"dpl_server": "allowed"},
            )

    def test_threadsafe_applicability_handles_catalog_file_aliases(self) -> None:
        document = module.parse_fragment(
            b"<section><ul><li>ABEND</li><li>READ *</li>"
            b"<li>DEQ (This command is threadsafe if LOCAL and non-threadsafe if GLOBAL.)</li>"
            b"<li>GET COUNTER and GET DCOUNTER</li>"
            b"<li>GET CONTAINER (CHANNEL)</li>"
            b"<li>REQUEST ENCYRPTPTKT</li>"
            b"<li>WEB READ FORMFIELD</li><li>WEB READ HTTPHEADER</li>"
            b"<li>WEB READ QUERYPARM</li><li>WEB WRITE HTTPHEADER</li>"
            b"</ul></section>"
        )
        read = module.independently_derive_threadsafe("READ FILE", document)
        self.assertEqual(read["threadsafe"], "conditional")
        self.assertEqual(
            read["threadsafe_condition"]["profile"],
            "file-control-storage-and-locality",
        )
        self.assertEqual(
            module.independently_derive_threadsafe("ABEND", document),
            {"threadsafe": "yes"},
        )
        self.assertEqual(
            module.independently_derive_threadsafe("DEQ", document)["threadsafe"],
            "conditional",
        )
        self.assertEqual(
            module.independently_derive_threadsafe("REQUEST ENCRYPTPTKT", document),
            {"threadsafe": "yes"},
        )
        for label in ("WEB READ", "WEB WRITE", "GET DCOUNTER", "GET CONTAINER"):
            self.assertEqual(
                module.independently_derive_threadsafe(label, document),
                {"threadsafe": "yes"},
            )
        self.assertEqual(
            module.independently_derive_threadsafe("ROUTE", document),
            {"threadsafe": "no"},
        )

    def test_dpl_prose_is_independently_derived(self) -> None:
        document = module.parse_fragment(
            b"<section><p>Any of the EXEC CICS WEB commands fail with a RESP2 "
            b"value of 1. WEB EXTRACT, EXTRACT TCPIP and EXTRACT CERTIFICATE "
            b"fail with a RESP2 value of 5. APPC commands listed are prohibited "
            b"only when they refer to the principal facility.</p>"
            b"<table><tr><td>CONNECT PROCESS</td><td>all</td></tr></table></section>"
        )
        section = next(node for node in document.descendants() if node.tag == "section")
        self.assertEqual(
            module.independently_derive_dpl("WEB CLOSE", section)["dpl_restriction"][
                "resp2"
            ],
            1,
        )
        self.assertEqual(
            module.independently_derive_dpl("WEB EXTRACT", section)["dpl_restriction"][
                "resp2"
            ],
            5,
        )
        connect = module.independently_derive_dpl("CONNECT PROCESS", section)
        self.assertEqual(connect["dpl_restriction"]["kind"], "conditional")

    def test_condition_symbol_uses_only_independently_loaded_response_codes(self) -> None:
        codes = {"INVREQ": 16, "LENGERR": 22, "NOTAUTH": 70}
        self.assertEqual(module.condition_symbol("NOTAUTH", 0, codes), "RESP-70-NOTAUTH")
        self.assertEqual(module.condition_symbol("UNKNOWN", 0, codes), "SYMBOL-UNKNOWN")

    def test_condition_metavariable_and_timer_default_are_independently_classified(self) -> None:
        document = module.parse_fragment(
            b"<svg><g><text class='syntaxkwd'>condition</text>"
            b"<text class='syntaxkwd'>today</text>"
            b"<text class='syntaxkwd'>NATLANG</text>"
            b"<text class='syntaxkwd'>'en'</text></g></svg>"
        )
        self.assertEqual(
            [(token.kind, token.value) for token in module.semantic_tokens(document)],
            [
                ("variable", "condition"),
                ("fragment", "today"),
                ("keyword", "NATLANG"),
                ("keyword", "'en'"),
            ],
        )

    def test_dynamic_condition_definition_has_independent_clause_shape(self) -> None:
        document = module.parse_fragment(
            b"<article><p>You cannot include more than sixteen conditions in the same command.</p>"
            b"<section><h2 class='sectiontitle' id='options'>Options</h2><dl>"
            b"<dt>condition(<span class='var'>label</span>)</dt>"
            b"<dd>Specifies the condition name and optional label.</dd>"
            b"</dl></section></article>"
        )
        definition = module.definition_inventory(module.sections(document)["Options"])[0]
        value = module.option_value(definition, document)
        self.assertEqual(value["term"], "CONDITION-NAME")
        self.assertEqual(value["dynamic_name_profile"], "cics-eibresp-condition-name@1")
        self.assertEqual(value["minimum_occurrences"], 1)
        self.assertEqual(value["maximum_occurrences"], 16)
        self.assertEqual(value["label_operand"], "optional")

    def test_language_option_and_trigger_facts_are_independently_hashed(self) -> None:
        language = module.parse_fragment(
            b"<p class='shortdesc'>APPC state (assembler-language and C programs only).</p>"
        )
        language_node = next(
            node for node in language.descendants() if node.tag == "p"
        )
        self.assertEqual(
            module.independent_language_profile(language_node),
            "assembler-and-c-only",
        )
        document = module.parse_fragment(
            b"<section><h2 class='sectiontitle' id='options'>Options</h2><dl>"
            b"<dt>LENGTH(data-area)</dt><dd>If you specify the SET option, the "
            b"LENGTH must be specified.</dd></dl></section>"
            b"<section><h2 class='sectiontitle' id='conditions'>Conditions</h2><dl>"
            b"<dt>16 INVREQ</dt><dd><dl><dt>1</dt><dd>The bounded trigger.</dd>"
            b"</dl></dd></dl></section>"
        )
        indexed = module.sections(document)
        option = module.definition_inventory(indexed["Options"])[0]
        self.assertEqual(
            module.option_value(option)["option_legality"], "bounded-prose"
        )
        condition = module.definition_inventory(indexed["Conditions"])[1]
        value = module.condition_value(condition, {"INVREQ": 16})
        self.assertEqual(len(value["trigger_fragment_sha256s"]), 1)
        self.assertRegex(value["trigger_fragment_sha256s"][0], r"^sha256:[0-9a-f]{64}$")

    def test_syntax_head_and_discriminators_are_independently_derived(self) -> None:
        tokens = [
            {"kind": "keyword", "value": "SEND MAP(", "relation": "required"},
            {"kind": "variable", "value": "name", "relation": "required"},
        ]
        head = module.independent_syntax_head(tokens, {"MAP"})
        self.assertEqual(head, "SEND")
        relation, discriminators = module.independent_syntax_identity(
            "SEND MAP", head, tokens
        )
        self.assertEqual(relation, "catalog-qualified")
        self.assertEqual(discriminators, [{"name": "MAP", "state": "present"}])
        self.assertEqual(
            module.independent_syntax_identity(
                "WAIT",
                "GDS WAIT",
                [{"kind": "keyword", "value": "GDS WAIT", "relation": "required"}],
            )[0],
            "documented-alias",
        )
        self.assertEqual(
            module.independent_syntax_identity(
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
            module.independent_syntax_identity(
                "ADDRESS SET", "ADDRESS", address_tokens
            )[1],
            [{"name": "SET", "state": "present"}],
        )

    def test_acquire_choice_branch_is_independently_partitioned(self) -> None:
        document = module.parse_fragment(
            b"<section><h2 id='s' class='sectiontitle'>Syntax</h2>"
            b"<div class='syntaxdiagram'><h3 id='d' class='syntaxdiagram-title'>ACQUIRE</h3>"
            b"<svg class='syntaxdiagram'><g class='diagram'><g class='groupcomp'>"
            b"<g class='boxed groupcomp'><g class='unboxed syntaxkwd'>"
            b"<g><text class='syntaxkwd'>ACQUIRE</text></g></g></g>"
            b"<g class='groupchoice'><g class='groupseq'>"
            b"<g class='boxed groupcomp'><g class='unboxed syntaxkwd'>"
            b"<g><text class='syntaxkwd'>PROCESS(</text></g>"
            b"<g><text class='syntaxvar'>data-value</text></g>"
            b"<g><text class='syntaxdelim'>)</text></g></g></g>"
            b"<g class='boxed groupcomp'><g class='unboxed syntaxkwd'>"
            b"<g><text class='syntaxkwd'>PROCESSTYPE(</text></g>"
            b"<g><text class='syntaxvar'>data-value</text></g>"
            b"<g><text class='syntaxdelim'>)</text></g></g></g></g>"
            b"<g class='boxed groupcomp'><g class='unboxed syntaxkwd'>"
            b"<g><text class='syntaxkwd'>ACTIVITYID(</text></g>"
            b"<g><text class='syntaxvar'>data-value</text></g>"
            b"<g><text class='syntaxdelim'>)</text></g></g></g>"
            b"</g></g></g></svg></div></section>"
        )
        diagram = module.diagrams(module.sections(document)["Syntax"])[0]
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
        activity = module.diagram_items_for_row(rows[0], rows, diagram)
        process = module.diagram_items_for_row(rows[1], rows, diagram)
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
        self.assertEqual(
            next(item for item in activity if item.tokens[0].value == "ACTIVITYID(").relation,
            "required",
        )

    def test_direct_choice_catalog_branch_is_promoted_to_required(self) -> None:
        document = module.parse_fragment(
            b"<section><h2 id='s' class='sectiontitle'>Syntax</h2>"
            b"<div class='syntaxdiagram'><h3 id='d' class='syntaxdiagram-title'>SUSPEND</h3>"
            b"<svg class='syntaxdiagram'><g class='diagram'><g class='groupcomp'>"
            b"<g class='unboxed syntaxkwd'><text class='syntaxkwd'>SUSPEND</text></g>"
            b"<g class='groupchoice'>"
            b"<g class='unboxed syntaxkwd'><text class='syntaxkwd'>ACQACTIVITY</text></g>"
            b"<g class='unboxed syntaxkwd'><text class='syntaxkwd'>ACQPROCESS</text></g>"
            b"<g class='unboxed syntaxkwd'><text class='syntaxkwd'>ACTIVITY(</text>"
            b"<text class='syntaxvar'>name</text><text class='syntaxdelim'>)</text></g>"
            b"</g></g></g></svg></div></section>"
        )
        diagram = module.diagrams(module.sections(document)["Syntax"])[0]
        rows = [
            {
                "official_row": "ibm-cics-ts-6x-2026-08-31:api-commands:0215",
                "label": "SUSPEND ACQACTIVITY",
                "selection_kind": "shared-page",
            },
            {
                "official_row": "ibm-cics-ts-6x-2026-08-31:api-commands:0216",
                "label": "SUSPEND ACQPROCESS",
                "selection_kind": "shared-page",
            },
            {
                "official_row": "ibm-cics-ts-6x-2026-08-31:api-commands:0217",
                "label": "SUSPEND ACTIVITY",
                "selection_kind": "shared-page",
            },
        ]
        selected = module.diagram_items_for_row(rows[2], rows, diagram)
        activity = next(
            item for item in selected if item.tokens[0].value == "ACTIVITY("
        )
        self.assertEqual(activity.relation, "required")
        self.assertNotIn(
            "ACQPROCESS", [token.value for item in selected for token in item.tokens]
        )

    def test_exact_source_derived_coverage_is_verified_and_deterministic(self) -> None:
        directory, fixture = self.fixture()
        self.addCleanup(directory.cleanup)
        first = module.verify(fixture.root, fixture.cache)
        second = module.verify(fixture.root, fixture.cache)
        self.assertEqual(first, second)
        self.assertEqual(first["structural_coverage"]["expected"], 4)
        self.assertEqual(first["structural_coverage"]["projected"], 4)
        self.assertEqual(first["structural_coverage"]["missing"], 0)
        self.assertEqual(first["structural_coverage"]["extra"], 0)
        self.assertEqual(first["candidate_categories"]["verified"]["count"], 5)
        self.assertEqual(first["candidate_categories"]["mismatch"]["count"], 0)
        self.assertEqual(first["status"], "verified")
        for field in ("coverage_credit", "semantic_credit", "differential_credit"):
            self.assertEqual(first[field], 0)

    def test_mutated_cache_body_fails_before_projection_is_loaded(self) -> None:
        directory, fixture = self.fixture()
        self.addCleanup(directory.cleanup)
        canonical, _ = module.cache_names(fixture.topic_path, fixture.digest)
        (fixture.cache / canonical).write_bytes(fixture.body + b" ")
        (fixture.root / module.PROJECTION_PATH).write_text("not JSON", encoding="utf-8")
        with self.assertRaisesRegex(module.VerificationError, "cached topic identity differs"):
            module.verify(fixture.root, fixture.cache)

    def test_locator_and_fragment_mutations_are_mismatches(self) -> None:
        for field, value, reason in (
            ("structural_path", "div[1]/article[9]", "unresolved-locator"),
            ("fragment_sha256", "sha256:" + "0" * 64, "fragment-digest-mismatch"),
        ):
            with self.subTest(field=field):
                directory, fixture = self.fixture()
                self.addCleanup(directory.cleanup)
                fixture.candidate("source-option")["evidence"][field] = value
                fixture.write_projection()
                report = module.verify(fixture.root, fixture.cache)
                self.assertEqual(report["candidate_categories"]["mismatch"]["count"], 1)
                self.assertIn(reason, {item["reason_code"] for item in report["findings"]})

    def test_changed_syntax_value_requires_reprojection(self) -> None:
        directory, fixture = self.fixture()
        self.addCleanup(directory.cleanup)
        syntax = fixture.candidate("source-syntax")
        syntax["candidate_value"]["tokens"][0]["value"] = "BAR"
        fixture.write_projection()
        report = module.verify(fixture.root, fixture.cache)
        self.assertIn(
            "structural-value-differs",
            {item["reason_code"] for item in report["findings"]},
        )
        self.assertEqual(report["candidate_categories"]["mismatch"]["count"], 1)
        self.assertEqual(report["structural_coverage"]["missing"], 0)

    def test_uppercase_nested_enum_is_not_an_operand_marker(self) -> None:
        document = module.parse_fragment(
            b'<dt>HOSTNAME<span class="var">HOSTNAME</span></dt>'
        )
        term = next(node for node in document.descendants() if node.tag == "dt")
        self.assertEqual(module.argument_markers(term), ["none"])

    def test_missing_option_is_detected_by_two_way_source_coverage(self) -> None:
        directory, fixture = self.fixture()
        self.addCleanup(directory.cleanup)
        fixture.dimension("options")["candidates"].clear()
        fixture.write_projection()
        report = module.verify(fixture.root, fixture.cache)
        self.assertEqual(report["structural_coverage"]["missing"], 1)
        self.assertIn(
            "missing-structural-fact",
            {item["reason_code"] for item in report["findings"]},
        )

    def test_intrinsic_sender_with_output_direction_is_a_mismatch(self) -> None:
        directory, fixture = self.fixture()
        self.addCleanup(directory.cleanup)
        fixture.candidate("source-operand-direction")["candidate_value"]["direction"] = "output"
        fixture.write_projection()
        report = module.verify(fixture.root, fixture.cache)
        self.assertIn(
            "intrinsic-input-direction-differs",
            {item["reason_code"] for item in report["findings"]},
        )
        self.assertEqual(report["candidate_categories"]["mismatch"]["count"], 1)

    def test_unexplained_data_area_direction_remains_product_ambiguous(self) -> None:
        directory, fixture = self.fixture(
            marker="data-area",
            description="The identifier associated with this request.",
        )
        self.addCleanup(directory.cleanup)
        report = module.verify(fixture.root, fixture.cache)
        self.assertIn(
            "direction-not-explicit",
            {item["reason_code"] for item in report["findings"]},
        )
        self.assertEqual(report["candidate_categories"]["product-ambiguity"]["count"], 1)
        self.assertEqual(report["status"], "verified-with-bounded-ambiguities")

    def test_issue_types_are_never_silently_verified(self) -> None:
        directory, fixture = self.fixture()
        self.addCleanup(directory.cleanup)
        issue_specs = (
            ("syntax", "source-gap"),
            ("options", "unmatched-row"),
            ("conditions", "conflicting-argument-kind"),
        )
        for ordinal, (dimension, code) in enumerate(issue_specs):
            fixture.dimension(dimension)["issues"].append(
                {
                    "issue_id": f"issue-{ordinal}",
                    "code": code,
                    "candidate_ids": [],
                }
            )
        fixture.write_projection()
        report = module.verify(fixture.root, fixture.cache)
        self.assertEqual(report["issue_categories"]["product-ambiguity"]["count"], 2)
        self.assertEqual(report["issue_categories"]["requires-reprojection"]["count"], 1)
        self.assertEqual(report["issue_categories"]["verified"]["count"], 0)

    def test_target_equivalence_scope_includes_every_affected_dimension(self) -> None:
        directory, fixture = self.fixture()
        self.addCleanup(directory.cleanup)
        fixture.dimension("syntax")["issues"].append(
            {
                "issue_id": "issue-equivalence",
                "code": "target-equivalence-ambiguity",
                "candidate_ids": [],
                "affected_dimensions": [
                    "syntax",
                    "options",
                    "operand-directions",
                    "conditions",
                ],
            }
        )
        fixture.write_projection()
        report = module.verify(fixture.root, fixture.cache)
        self.assertEqual(
            report["ambiguity_scope"],
            [
                {
                    "official_row": "test:api-commands:0001",
                    "dimensions": [
                        "conditions",
                        "operand-directions",
                        "options",
                        "syntax",
                    ],
                }
            ],
        )


class RepositoryAmbiguityScopeTests(unittest.TestCase):
    def test_cross_product_rows_scope_all_authority_bounded_dimensions(self) -> None:
        expected = {
            ("a", "ibm-cics-ts-6x-2026-08-31:api-commands:0056"),
            ("a", "ibm-cics-ts-6x-2026-08-31:api-commands:0065"),
            ("c", "ibm-cics-ts-6x-2026-08-31:api-commands:0220"),
        }
        dimensions = {"syntax", "options", "operand-directions", "conditions"}
        for batch, official_row in expected:
            with self.subTest(batch=batch, official_row=official_row):
                review = json.loads(
                    (module.REPOSITORY / module.source_batch(batch).review_path).read_text(
                        encoding="utf-8"
                    )
                )
                scope = {
                    item["official_row"]: set(item["dimensions"])
                    for item in review["ambiguity_scope"]
                }
                self.assertTrue(dimensions <= scope[official_row])

    def test_internal_cicsmessage_applicability_is_target_backed_not_applicable(self) -> None:
        batch = module.source_batch("a")
        projection = json.loads(
            (module.REPOSITORY / batch.projection_path).read_text(encoding="utf-8")
        )
        row = next(item for item in projection["rows"] if item["label"] == "CICSMESSAGE")
        facts = {
            key: value
            for dimension in row["dimensions"]
            for candidate in dimension["candidates"]
            for key, value in candidate.get("candidate_value", {})
            .get("applicability", {})
            .items()
        }
        self.assertEqual(
            facts,
            {
                "local_task": "not-applicable",
                "dpl_server": "not-applicable",
                "threadsafe": "not-applicable",
                "cobol": "not-applicable",
                "language_restriction": {"profile": "internal-only"},
            },
        )
        review = json.loads(
            (module.REPOSITORY / batch.review_path).read_text(encoding="utf-8")
        )
        self.assertNotIn(
            row["official_row"],
            {item["official_row"] for item in review["ambiguity_scope"]},
        )

    def test_compatibility_stubs_are_explicit_bounded_source_ambiguities(self) -> None:
        expected = {
            "b": {"ibm-cics-ts-6x-2026-08-31:api-commands:0133"},
            "c": {
                "ibm-cics-ts-6x-2026-08-31:api-commands:0236",
                "ibm-cics-ts-6x-2026-08-31:api-commands:0255",
            },
        }
        semantic = {"syntax", "options", "operand-directions", "conditions"}
        for batch_name, rows in expected.items():
            batch = module.source_batch(batch_name)
            projection = json.loads(
                (module.REPOSITORY / batch.projection_path).read_text(encoding="utf-8")
            )
            issues = {
                issue["official_row"]: issue
                for issue in projection["blocking_issues"]
                if issue["code"] == "compatibility-successor-not-equivalent"
            }
            review = json.loads(
                (module.REPOSITORY / batch.review_path).read_text(encoding="utf-8")
            )
            scope = {
                item["official_row"]: set(item["dimensions"])
                for item in review["ambiguity_scope"]
            }
            for row in rows:
                self.assertEqual(set(issues[row]["affected_dimensions"]), semantic)
                self.assertTrue(issues[row]["evidence"])
                self.assertTrue(semantic <= scope[row])


if __name__ == "__main__":
    unittest.main()
