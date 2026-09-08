from __future__ import annotations

import hashlib
import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

TOOL = Path(__file__).resolve().parents[1] / "extract_cobol_move_rules.py"
spec = importlib.util.spec_from_file_location("extract_cobol_move_rules", TOOL)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)

HTML = b"""<article><h1>MOVE</h1><p>Conversion and editing apply.</p>
<table id="valid"><tr><th>Sender</th><th>Numeric-edited</th></tr>
<tr><td>Numeric integer sending item</td><td>Yes</td></tr></table></article>"""


class CobolMoveRuleTests(unittest.TestCase):
    def test_shared_fragment_workflow_extracts_structural_table_rows(self) -> None:
        manifest = {
            "baseline_id": "synthetic",
            "product": "synthetic",
            "topic_manifest_digest": "0" * 64,
            "topics": [
                {
                    "topic_path": "synthetic/move.html",
                    "bytes": len(HTML),
                    "sha256": hashlib.sha256(HTML).hexdigest(),
                }
            ],
        }
        config = {
            "extractor_version": "test@1",
            "release": "synthetic",
            "sources": [
                {"topic_path": "synthetic/move.html", "sections": ["__lead__"]}
            ],
            "compile_rules": [
                {
                    "id": "numeric-to-edited",
                    "topic_path": "synthetic/move.html",
                    "sections": ["__lead__"],
                    "all": ["Numeric integer", "Yes"],
                    "interpretation": "numeric integer to edited is valid",
                    "applicability": "synthetic",
                }
            ],
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "move.html").write_bytes(HTML)
            left = module.compile_corpus(manifest, config, root)
            right = module.compile_corpus(manifest, config, root)
        self.assertEqual(left, right)
        self.assertEqual(left["totals"]["candidates"], 1)
        self.assertEqual(left["candidates"][0]["tag"], "tr")
        self.assertIn("tr[2]", left["candidates"][0]["locator"])
        self.assertNotIn("Numeric integer sending item", str(left))
        self.assertEqual(left["coverage_credit"], 0)

    def test_digest_drift_blocks_projection(self) -> None:
        manifest = {
            "baseline_id": "synthetic",
            "product": "synthetic",
            "topic_manifest_digest": "0" * 64,
            "topics": [
                {
                    "topic_path": "synthetic/move.html",
                    "bytes": len(HTML),
                    "sha256": "f" * 64,
                }
            ],
        }
        config = {
            "extractor_version": "test@1",
            "release": "synthetic",
            "sources": [
                {"topic_path": "synthetic/move.html", "sections": ["__lead__"]}
            ],
            "compile_rules": [],
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "move.html").write_bytes(HTML)
            with self.assertRaisesRegex(ValueError, "digest mismatch"):
                module.compile_corpus(manifest, config, root)


if __name__ == "__main__":
    unittest.main()
