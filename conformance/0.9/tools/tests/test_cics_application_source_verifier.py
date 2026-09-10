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
            "target_version": "0.9.0",
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


if __name__ == "__main__":
    unittest.main()
