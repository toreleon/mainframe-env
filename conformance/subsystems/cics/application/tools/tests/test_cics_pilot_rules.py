from __future__ import annotations

import hashlib
import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

TOOL = Path(__file__).resolve().parents[1] / "extract_cics_pilot_rules.py"
spec = importlib.util.spec_from_file_location("extract_cics_pilot_rules", TOOL)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


HTML = b"""<article><h1 id="title">Pilot</h1>
<h2 id="conditions">Conditions</h2>
<dl><dt>16 INVREQ</dt><dd>RESP2 values:<dl><dt>30</dt>
<dd>Only when no prior READ UPDATE exists, the request must not proceed.</dd></dl></dd></dl>
<h2 id="rules">Rules</h2>
<p>Do not mutate state unless authorization succeeds; see <a href="general.html">general rule</a>.</p>
</article>"""


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def manifest() -> dict:
    return {
        "baseline_id": "synthetic",
        "product": "synthetic-product",
        "topic_manifest_digest": "0" * 64,
        "topics": [
            {
                "topic_path": "synthetic/topic.html",
                "bytes": len(HTML),
                "sha256": sha(HTML),
            }
        ],
    }


def config() -> dict:
    return {
        "extractor_version": "test@1",
        "release": "synthetic",
        "sources": [
            {"topic_path": "synthetic/topic.html", "sections": ["conditions", "rules"]}
        ],
        "compile_rules": [
            {
                "id": "nested-condition",
                "topic_path": "synthetic/topic.html",
                "sections": ["conditions"],
                "all": ["Only when", "must not"],
                "interpretation": "reject without prior update context",
                "applicability": "synthetic",
            },
            {
                "id": "authorization",
                "topic_path": "synthetic/topic.html",
                "sections": ["rules"],
                "all": ["unless authorization"],
                "interpretation": "no mutation before authorization",
                "applicability": "synthetic",
            },
        ],
    }


class CicsPilotRuleTests(unittest.TestCase):
    def compile(self, cfg: dict | None = None) -> dict:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "topic.html").write_bytes(HTML)
            return module.compile_corpus(manifest(), cfg or config(), root)

    def test_nested_dita_conditions_negation_only_when_and_unless_are_preserved(self) -> None:
        result = self.compile()
        by_rule = {
            candidate["rule_ids"][0]: candidate for candidate in result["candidates"]
        }
        nested = by_rule["nested-condition"]
        self.assertTrue(nested["cues"]["negated"])
        self.assertTrue(nested["cues"]["only_when"])
        self.assertEqual(nested["cues"]["condition_stack"], ["16 INVREQ", "30"])
        authorization = by_rule["authorization"]
        self.assertTrue(authorization["cues"]["unless"])
        self.assertEqual(authorization["cues"]["references"], ["general.html"])

    def test_idless_paragraph_has_versioned_structural_locator_and_digest(self) -> None:
        result = self.compile()
        candidate = next(
            item for item in result["candidates"] if item["rule_ids"] == ["authorization"]
        )
        self.assertIn("p[1]", candidate["locator"])
        self.assertIn("fragment-sha256:", candidate["locator"])
        self.assertEqual(len(candidate["fragment_sha256"]), 64)

    def test_output_is_deterministic_and_retains_no_publication_text(self) -> None:
        left = self.compile()
        right = self.compile()
        self.assertEqual(left, right)
        rendered = json.dumps(left, sort_keys=True)
        self.assertNotIn("Only when no prior", rendered)
        self.assertEqual(left["coverage_credit"], 0)

    def test_missing_section_and_conflicting_rules_are_explicit(self) -> None:
        cfg = config()
        cfg["sources"][0]["sections"].append("missing")
        duplicate = dict(cfg["compile_rules"][1])
        duplicate["id"] = "authorization-conflict"
        duplicate["interpretation"] = "contradictory interpretation"
        cfg["compile_rules"].append(duplicate)
        result = self.compile(cfg)
        self.assertTrue(
            any(item.get("reason") == "missing-section" for item in result["inventory"])
        )
        self.assertGreater(result["totals"]["conflicting"], 0)

    def test_source_drift_blocks_projection(self) -> None:
        broken = manifest()
        broken["topics"][0]["sha256"] = "f" * 64
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "topic.html").write_bytes(HTML)
            with self.assertRaisesRegex(ValueError, "digest mismatch"):
                module.compile_corpus(broken, config(), root)


if __name__ == "__main__":
    unittest.main()
