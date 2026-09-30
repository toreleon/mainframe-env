from __future__ import annotations

from collections import Counter
import copy
import importlib.util
import io
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch


TOOL = (
    Path(__file__).resolve().parents[1]
    / "cache_cics_application_source_supplements.py"
)
spec = importlib.util.spec_from_file_location(
    "cache_cics_application_source_supplements", TOOL
)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


class CicsApplicationSourceSupplementTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.receipt = module.load_receipt()

    def mutated_receipt(self) -> dict:
        return copy.deepcopy(self.receipt)

    def refresh(self, receipt: dict) -> None:
        receipt["supplements_sha256"] = module.receipt_digest(receipt)

    def test_committed_receipt_is_exact_zero_credit_and_has_no_toc_claim(self) -> None:
        receipt = module.load_receipt()
        self.assertFalse(receipt["toc_claimed"])
        self.assertEqual(len(receipt["topics"]), 5)
        self.assertEqual(len(receipt["source_resolutions"]), 3)
        for field in (
            "semantic_credit",
            "execution_credit",
            "registration_credit",
            "coverage_credit",
            "differential_credit",
        ):
            self.assertEqual(receipt[field], 0)
        for topic in receipt["topics"]:
            self.assertFalse(topic["target_product_authority"])
            self.assertFalse(topic["semantic_authority"])
            self.assertEqual(topic["coverage_credit"], 0)

    def test_pin_identity_and_cache_keys_are_derived(self) -> None:
        pins = module.pins_from_receipt(self.receipt)
        self.assertEqual(len(pins), 5)
        for topic, pin in zip(self.receipt["topics"], pins):
            self.assertEqual(pin.topic, topic["topic_path"])
            self.assertEqual(pin.url, topic["content_url"])
            self.assertEqual(pin.key, topic["cache_key"])
            self.assertEqual(pin.scopes, ())

    def test_repin_keeps_original_capture_bound_to_historical_topics(self) -> None:
        historical = module.read_json(module.ROOT / module.HISTORICAL_RECEIPT_PATH)
        self.assertEqual(
            self.receipt["capture"]["identity_sha256"],
            module.identity_digest(historical["topics"]),
        )
        self.assertNotEqual(
            self.receipt["capture"]["identity_sha256"],
            module.identity_digest(self.receipt["topics"]),
        )
        receipt = self.mutated_receipt()
        receipt["repin"]["historical_receipt_sha256"] = "sha256:" + "0" * 64
        self.refresh(receipt)
        with self.assertRaisesRegex(module.SupplementError, "historical receipt binding"):
            module.validate_receipt(receipt)

    def test_toc_or_target_authority_spoof_is_rejected(self) -> None:
        receipt = self.mutated_receipt()
        receipt["toc_claimed"] = True
        self.refresh(receipt)
        with self.assertRaisesRegex(module.SupplementError, "toc_claimed"):
            module.validate_receipt(receipt)

        receipt = self.mutated_receipt()
        receipt["topics"][0]["target_authority_boundary"] = (
            "target-product-authority"
        )
        self.refresh(receipt)
        with self.assertRaisesRegex(module.SupplementError, "authority_boundary"):
            module.validate_receipt(receipt)

    def test_topic_set_url_cache_key_and_resolution_drift_are_rejected(self) -> None:
        mutations = (
            ("topic count", lambda value: value["topics"].pop()),
            (
                "content_url",
                lambda value: value["topics"][0].__setitem__(
                    "content_url", value["topics"][0]["content_url"] + "&extra=1"
                ),
            ),
            (
                "cache_key",
                lambda value: value["topics"][0].__setitem__(
                    "cache_key", "topic-" + "0" * 64 + "-sha256-" + "0" * 64 + ".html"
                ),
            ),
            (
                "source_resolutions",
                lambda value: value["source_resolutions"][1][
                    "supplemental_topic_sources"
                ].pop(),
            ),
        )
        for message, mutate in mutations:
            with self.subTest(message=message):
                receipt = self.mutated_receipt()
                mutate(receipt)
                self.refresh(receipt)
                with self.assertRaisesRegex(module.SupplementError, message):
                    module.validate_receipt(receipt)

    def test_stale_receipt_and_contract_bindings_are_rejected(self) -> None:
        receipt = self.mutated_receipt()
        receipt["supplements_sha256"] = "sha256:" + "0" * 64
        with self.assertRaisesRegex(module.SupplementError, "digest is stale"):
            module.validate_receipt(receipt)

        receipt = self.mutated_receipt()
        receipt["receipt_contract"]["schema"]["file_sha256"] = (
            "sha256:" + "0" * 64
        )
        self.refresh(receipt)
        with self.assertRaisesRegex(module.SupplementError, "schema binding is stale"):
            module.validate_receipt(receipt)

    def test_check_verifies_cached_heading_and_last_modified(self) -> None:
        bodies = [
            (
                f'<h1 class="topictitle1">{topic["heading"]}</h1>'
                f'<div id="lastModifiedDate">Last Updated {topic["last_modified"]}</div>'
            ).encode()
            for topic in self.receipt["topics"]
        ]
        with tempfile.TemporaryDirectory() as temporary, patch.object(
            module.ibm_docs, "cached_body", side_effect=bodies
        ) as cached:
            checked = module.check(module.ROOT, Path(temporary) / "cache")
        self.assertEqual(checked, self.receipt)
        self.assertEqual(cached.call_count, 5)

    def test_check_rejects_cached_body_metadata_drift(self) -> None:
        body = b'<h1 class="topictitle1">WRONG</h1>'
        with tempfile.TemporaryDirectory() as temporary, patch.object(
            module.ibm_docs, "cached_body", return_value=body
        ):
            with self.assertRaisesRegex(module.SupplementError, "heading differs"):
                module.check(module.ROOT, Path(temporary) / "cache")

    def test_import_reuses_streaming_cache_primitive_without_a_toc(self) -> None:
        with tempfile.TemporaryDirectory() as temporary, patch.object(
            module.ibm_docs,
            "import_cache",
            return_value=Counter(imported=5),
        ) as importer, patch.object(module, "check", return_value=self.receipt):
            result = module.import_receipt(
                module.ROOT, Path(temporary) / "cache", io.BytesIO(b"archive")
            )
        self.assertEqual(result["topics"], 5)
        pins, tocs = importer.call_args.args[2:]
        self.assertEqual(len(pins), 5)
        self.assertEqual(tocs, [])

    def test_import_failure_counts_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary, patch.object(
            module.ibm_docs,
            "import_cache",
            return_value=Counter(missing_expected=1),
        ), self.assertRaisesRegex(module.SupplementError, "import failed"):
            module.import_receipt(
                module.ROOT, Path(temporary) / "cache", io.BytesIO(b"archive")
            )

    def test_repository_cache_destination_is_rejected(self) -> None:
        with self.assertRaises(module.docs_api.InsideRepository):
            module.check(module.ROOT, module.ROOT)


if __name__ == "__main__":
    unittest.main()
