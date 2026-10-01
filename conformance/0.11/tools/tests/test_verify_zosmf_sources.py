from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


REPOSITORY = Path(__file__).resolve().parents[4]
MODULE_PATH = REPOSITORY / "conformance/0.11/tools/verify_zosmf_sources.py"
SPEC = importlib.util.spec_from_file_location("verify_zosmf_sources", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
verify = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(verify)


class ZosmfSourceNormalizationTests(unittest.TestCase):
    def catalog(self) -> dict:
        return json.loads(verify.CATALOG.read_text(encoding="utf-8"))

    def test_complete_catalog_closes_every_family_heading_and_route_authority(self) -> None:
        self.assertEqual(
            verify.validate_structure(),
            {
                "families": 27,
                "headings": 189,
                "source_rows": 216,
                "operations": 278,
                "route_variants": 352,
                "obligations": 1727,
                "official_routes": 23,
                "custom_routes": 7,
            },
        )

    def test_missing_heading_is_rejected_instead_of_reducing_the_denominator(self) -> None:
        mutated = self.catalog()
        mutated["headings"].pop()
        with self.assertRaisesRegex(verify.CatalogError, "189 headings"):
            verify.validate_structure(catalog_override=mutated)

    def test_mutation_cannot_advertise_an_unowned_generated_route(self) -> None:
        mutated = self.catalog()
        operation = next(
            item for item in mutated["operations"] if item["publication"]["state"] == "withheld"
        )
        operation["publication"]["new_route_advertised"] = True
        with self.assertRaisesRegex(verify.CatalogError, "advertises a new route"):
            verify.validate_structure(catalog_override=mutated)

    def test_retained_body_digest_mismatch_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "body.html"
            path.write_bytes(b"changed")
            with self.assertRaisesRegex(verify.CatalogError, "digest differs"):
                verify.read_verified_body(path, "0" * 64, len(b"changed"))


if __name__ == "__main__":
    unittest.main()
