import json
from pathlib import Path
import tempfile
import unittest

import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import check_public_api_docs as docs


class PublicApiDocsTests(unittest.TestCase):
    def test_policy_shape_is_exact_and_boolean_is_not_a_limit(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "policy.json"
            path.write_text(
                json.dumps(
                    {
                        "schema_version": docs.SCHEMA,
                        "packages": {"mainframe-env-store-api": True},
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaises(ValueError):
                docs.load_policy(path)

    def test_structured_rustdoc_diagnostics_are_counted_by_manifest(self):
        manifest = Path("/repo/crates/contracts/mainframe-env-store-api/Cargo.toml")
        lines = [
            json.dumps(
                {
                    "reason": "compiler-message",
                    "manifest_path": str(manifest),
                    "message": {
                        "level": "warning",
                        "code": {"code": "missing_docs"},
                    },
                }
            ),
            json.dumps(
                {
                    "reason": "compiler-message",
                    "manifest_path": str(manifest),
                    "message": {"level": "warning", "code": {"code": "dead_code"}},
                }
            ),
        ]
        counts, observed = docs.missing_doc_counts(
            lines, {"mainframe-env-store-api": manifest}
        )
        self.assertEqual(counts, {"mainframe-env-store-api": 1})
        self.assertEqual(observed, {"mainframe-env-store-api"})

    def test_ratchet_requires_reviewed_reduction_and_rejects_growth_or_missing_output(self):
        policy = {"mainframe-env-store-api": 2}
        self.assertEqual(
            docs.enforce(policy, {"mainframe-env-store-api": 2}, set(policy)), []
        )
        self.assertEqual(len(docs.enforce(policy, {"mainframe-env-store-api": 1}, set(policy))), 1)
        self.assertEqual(len(docs.enforce(policy, {"mainframe-env-store-api": 3}, set(policy))), 1)
        self.assertEqual(len(docs.enforce(policy, {}, set())), 1)


if __name__ == "__main__":
    unittest.main()
