from __future__ import annotations

import copy
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest


TOOL = Path(__file__).resolve().parents[1] / "verify_spi1001_source.py"
SPEC = importlib.util.spec_from_file_location("verify_spi1001_source", TOOL)
source = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = source
SPEC.loader.exec_module(source)


class Spi1001SourceAuthorityTests(unittest.TestCase):
    def test_committed_authority_closes_only_identity_facts(self) -> None:
        authority, catalog = source.validate_authority()
        self.assertEqual(authority["tables"]["spi"]["official_denominator"], 269)
        self.assertEqual(authority["tables"]["fepi"]["official_denominator"], 39)
        self.assertFalse(authority["projection_boundary"]["semantic_authority"])
        self.assertEqual(authority["coverage_credit"], 0)
        self.assertEqual(catalog["mandatory_rows"], 571)

    def test_parser_preserves_duplicate_label_eibfn_identities(self) -> None:
        body = b"""
        <table id="spi"><tr><th>Command</th><th>EIBFN code</th><th>Type</th></tr>
        <tr><td>INQUIRE X</td><td>12 34</td><td>SPI</td></tr>
        <tr><td>INQUIRE X</td><td>1236</td><td>SPI</td></tr></table>
        <table id="fepi"><tr><th>Command</th><th>EIBFN code</th><th>Type</th></tr>
        <tr><td>FEPI X</td><td>5678</td><td>FEPI</td></tr></table>
        """
        parsed = source.parse_command_tables(body, {"spi", "fepi"})
        table = {
            "interface": "SPI",
            "raw_rows": 2,
            "unique_command_labels": 1,
            "duplicate_labels": [
                {
                    "label": "INQUIRE X",
                    "catalog_eibfn": "1234",
                    "additional_source_eibfn": ["1236"],
                }
            ],
        }
        source.verify_table_projection(
            table,
            [("test:spi:0001", "INQUIRE X", "1234")],
            parsed["spi"],
        )

    def test_table_projection_rejects_changed_eibfn(self) -> None:
        table = {
            "interface": "FEPI",
            "raw_rows": 1,
            "unique_command_labels": 1,
            "duplicate_labels": [],
        }
        with self.assertRaisesRegex(source.SourceAuthorityError, "official catalog"):
            source.verify_table_projection(
                table,
                [("test:fepi:0001", "FEPI X", "1234")],
                [["FEPI X", "1236", "FEPI"]],
            )

    def test_table_projection_rejects_interface_mutation(self) -> None:
        table = {
            "interface": "SPI",
            "raw_rows": 1,
            "unique_command_labels": 1,
            "duplicate_labels": [],
        }
        with self.assertRaisesRegex(source.SourceAuthorityError, "interface"):
            source.verify_table_projection(
                table,
                [("test:spi:0001", "INQUIRE X", "1234")],
                [["INQUIRE X", "1234", "FEPI"]],
            )

    def test_table_projection_rejects_dropped_duplicate_identity(self) -> None:
        table = {
            "interface": "SPI",
            "raw_rows": 2,
            "unique_command_labels": 1,
            "duplicate_labels": [
                {
                    "label": "INQUIRE X",
                    "catalog_eibfn": "1234",
                    "additional_source_eibfn": ["1236"],
                }
            ],
        }
        with self.assertRaisesRegex(source.SourceAuthorityError, "raw row count"):
            source.verify_table_projection(
                table,
                [("test:spi:0001", "INQUIRE X", "1234")],
                [["INQUIRE X", "1234", "SPI"]],
            )

    def test_retained_source_digest_mutation_fails(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "topic.html"
            path.write_bytes(b"changed")
            with self.assertRaisesRegex(source.SourceAuthorityError, "digest differs"):
                source.verified_bytes(path, "sha256:" + "0" * 64, None)

    def test_semantic_boundary_mutation_fails_closed(self) -> None:
        authority, _ = source.validate_authority()
        mutated = copy.deepcopy(authority)
        mutated["projection_boundary"]["blocked_facts"].remove("conditions")
        with self.assertRaisesRegex(source.SourceAuthorityError, "dimensions drifted"):
            source.validate_projection_boundary(mutated)


if __name__ == "__main__":
    unittest.main()
