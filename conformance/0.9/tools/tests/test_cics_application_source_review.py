from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path
import sys
import unittest


TOOL = Path(__file__).resolve().parents[1] / "review_cics_application_sources.py"
spec = importlib.util.spec_from_file_location("review_cics_application_sources", TOOL)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


ZERO = "sha256:" + "0" * 64


def category(ids: list[str], *, issue: bool = False) -> dict:
    key = "issue_ids" if issue else "candidate_ids"
    return {
        "count": len(ids),
        key: ids,
        f"{key}_sha256": module.independent.category_digest(ids),
    }


def verification_report() -> dict:
    report = {
        "schema_version": module.independent.REPORT_SCHEMA,
        "target_version": "0.9.0",
        "status": "verified",
        "semantic_authority": False,
        "execution_authority": False,
        "automatic_registration": False,
        "coverage_credit": 0,
        "semantic_credit": 0,
        "differential_credit": 0,
        "verifier": {
            "name": "independent-cics-html-structural-verifier",
            "version": module.independent.VERIFIER_VERSION,
            "implementation_sha256": ZERO,
            "projection_loaded_after_source_derivation": True,
        },
        "inputs": {
            "source_map_sha256": ZERO,
            "topic_manifest_sha256": ZERO,
            "extraction_plan_sha256": ZERO,
            "candidate_projection_sha256": ZERO,
        },
        "counts": {
            "topics": 1,
            "rows": 1,
            "candidates": 1,
            "issues": 0,
            "unique_evidence_fragments": 1,
            "candidate_kinds": {
                "source-condition": 0,
                "source-context": 1,
                "source-operand-direction": 0,
                "source-option": 0,
                "source-syntax": 0,
            },
        },
        "structural_coverage": {
            "expected": 0,
            "projected": 0,
            "missing": 0,
            "extra": 0,
            "expected_by_kind": {
                "source-condition": 0,
                "source-option": 0,
                "source-syntax": 0,
            },
            "projected_by_kind": {
                "source-condition": 0,
                "source-option": 0,
                "source-syntax": 0,
            },
        },
        "applicability_coverage": {
            "expected": 0,
            "projected": 0,
            "missing": 0,
            "extra": 0,
        },
        "candidate_categories": {
            "verified": category(["candidate-1"]),
            "requires-reprojection": category([]),
            "product-ambiguity": category([]),
            "mismatch": category([]),
        },
        "issue_categories": {
            "verified": category([], issue=True),
            "requires-reprojection": category([], issue=True),
            "product-ambiguity": category([], issue=True),
            "mismatch": category([], issue=True),
        },
        "ambiguity_scope": [],
        "findings": [],
    }
    report["report_sha256"] = module.independent.report_digest(report)
    return report


def refresh(report: dict) -> None:
    report["report_sha256"] = module.independent.report_digest(report)


class AutomaticCicsSourceReviewTests(unittest.TestCase):
    def test_applicability_coverage_bound_is_exactly_four_fields_per_batch_row(self) -> None:
        schema = json.loads(
            (module.ROOT / module.SCHEMA_PATH).read_text(encoding="utf-8")
        )
        properties = schema["$defs"]["applicability-coverage"]["properties"]
        self.assertEqual(
            {name: value["maximum"] for name, value in properties.items()},
            {"expected": 352, "projected": 352, "missing": 352, "extra": 352},
        )
        for batch in "abc":
            review = json.loads(
                (
                    module.ROOT
                    / module.source_batch(batch).review_path
                ).read_text(encoding="utf-8")
            )
            self.assertLessEqual(review["applicability_coverage"]["expected"], 352)

    def test_clear_independent_report_is_auto_accepted_without_human_fields(self) -> None:
        receipt = module.build_receipt(module.ROOT, report=verification_report())
        self.assertEqual(receipt["review_status"], "auto-accepted")
        self.assertEqual(receipt["counts"]["blocking_findings"], 0)
        self.assertTrue(receipt["review_authority"]["automatic_acceptance"])
        self.assertFalse(receipt["review_authority"]["manual_approval"])
        self.assertNotIn("reviewer_id", json.dumps(receipt))
        module.validate_receipt(receipt, copy.deepcopy(receipt))

    def test_reprojection_and_mismatch_block_the_ordinary_check(self) -> None:
        for candidate_category in (
            "requires-reprojection",
            "mismatch",
        ):
            with self.subTest(category=candidate_category):
                report = verification_report()
                report["candidate_categories"]["verified"] = category([])
                report["candidate_categories"][candidate_category] = category(["candidate-1"])
                report["status"] = (
                    "mismatch"
                    if candidate_category == "mismatch"
                    else "requires-reprojection"
                )
                report["findings"] = [
                    {
                        "candidate_id": "candidate-1",
                        "category": candidate_category,
                        "reason_code": "synthetic-blocker",
                    }
                ]
                refresh(report)
                receipt = module.build_receipt(module.ROOT, report=report)
                self.assertEqual(receipt["review_status"], "blocked")
                with self.assertRaisesRegex(module.ReviewError, "automatic source review is blocked"):
                    module.validate_receipt(receipt, copy.deepcopy(receipt))

    def test_exact_product_ambiguity_is_bounded_without_manual_approval(self) -> None:
        report = verification_report()
        report["candidate_categories"]["verified"] = category([])
        report["candidate_categories"]["product-ambiguity"] = category(["candidate-1"])
        report["status"] = "verified-with-bounded-ambiguities"
        report["findings"] = [
            {
                "candidate_id": "candidate-1",
                "category": "product-ambiguity",
                "reason_code": "target-equivalence-ambiguity",
            }
        ]
        report["ambiguity_scope"] = [
            {"official_row": "test:0001", "dimensions": ["execution-context"]}
        ]
        refresh(report)
        receipt = module.build_receipt(module.ROOT, report=report)
        self.assertEqual(receipt["review_status"], "auto-accepted-with-bounded-ambiguities")
        self.assertEqual(receipt["counts"]["blocking_findings"], 0)
        module.validate_receipt(receipt, copy.deepcopy(receipt))

    def test_missing_or_extra_source_fact_blocks_without_user_approval(self) -> None:
        for field in ("missing", "extra"):
            with self.subTest(field=field):
                report = verification_report()
                report["status"] = "requires-reprojection"
                report["structural_coverage"][field] = 1
                report["findings"] = [
                    {
                        "fact_sha256": ZERO,
                        "category": "requires-reprojection",
                        "reason_code": f"{field}-structural-fact",
                    }
                ]
                refresh(report)
                receipt = module.build_receipt(module.ROOT, report=report)
                self.assertEqual(receipt["review_status"], "blocked")
                self.assertGreater(receipt["counts"]["blocking_findings"], 0)

    def test_product_issue_is_compact_and_bounded(self) -> None:
        report = verification_report()
        report["status"] = "verified-with-bounded-ambiguities"
        report["counts"]["issues"] = 1
        report["issue_categories"]["product-ambiguity"] = category(["issue-1"], issue=True)
        report["findings"] = [
            {
                "issue_id": "issue-1",
                "official_row": "test:0001",
                "category": "product-ambiguity",
                "reason_code": "missing-exact-source",
            }
        ]
        report["ambiguity_scope"] = [
            {"official_row": "test:0001", "dimensions": ["execution-context"]}
        ]
        refresh(report)
        receipt = module.build_receipt(module.ROOT, report=report)
        self.assertEqual(receipt["review_status"], "auto-accepted-with-bounded-ambiguities")
        self.assertEqual(receipt["blocker_ids"], ["issue-1"])
        self.assertNotIn("candidate_ids", receipt["candidate_dispositions"]["verified"])
        self.assertLess(len(module.pretty(receipt)), 10000)

    def test_stale_receipt_and_manual_approval_fields_are_rejected(self) -> None:
        receipt = module.build_receipt(module.ROOT, report=verification_report())
        stale = copy.deepcopy(receipt)
        stale["counts"]["candidates"] = 2
        stale["review_sha256"] = module.review_digest(stale)
        with self.assertRaisesRegex(module.ReviewError, "stale"):
            module.validate_receipt(stale, receipt, require_accepted=False)

        manual = copy.deepcopy(receipt)
        manual["reviewer_id"] = "somebody"
        manual["review_sha256"] = module.review_digest(manual)
        with self.assertRaisesRegex(module.ReviewError, "approval fields are forbidden"):
            module.validate_receipt(manual, manual, require_accepted=False)

    def test_review_checker_does_not_depend_on_projection_implementation(self) -> None:
        source = TOOL.read_text(encoding="utf-8")
        self.assertNotIn("extract_cics_application_sources", source)
        self.assertNotIn("projector.", source)
        self.assertIn("verify_cics_application_sources", source)

    def test_report_digest_and_zero_credit_are_enforced(self) -> None:
        report = verification_report()
        report["coverage_credit"] = 1
        refresh(report)
        with self.assertRaisesRegex(module.ReviewError, "improperly grants coverage_credit"):
            module.build_receipt(module.ROOT, report=report)

        report = verification_report()
        report["report_sha256"] = ZERO
        with self.assertRaisesRegex(module.ReviewError, "report digest differs"):
            module.build_receipt(module.ROOT, report=report)


if __name__ == "__main__":
    unittest.main()
