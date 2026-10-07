from __future__ import annotations

import copy
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path
import tempfile
import unittest


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import verify_ims_licensed_differential as ims  # noqa: E402


def digest(label: str) -> str:
    return "sha256:" + hashlib.sha256(label.encode()).hexdigest()


class ImsLicensedDifferentialTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.context = ims.verify_contract()
        cls.candidate = ims.CandidateIdentity("a" * 40, "b" * 40, True)

    def receipt_and_pins(self) -> tuple[dict, dict[str, str]]:
        service = {
            "product_name": "IBM IMS", "major_minor": "15.6", "release": "15.6.0",
            "service_level": "test-service", "build_id": "test-build",
            "installation_digest": digest("installation"),
        }
        pins = {
            "receipt_sha256": digest("receipt"),
            "service_identity_sha256": ims.canonical_digest(service),
            "environment_manifest_sha256": digest("environment"),
            "authorization_grant_sha256": digest("authorization"),
            "fixture_bundle_sha256": digest("fixture-bundle"),
            "product_runner_sha256": digest("product-runner"),
            "oracle_runner_sha256": digest("oracle-runner"),
        }
        observations = []
        for case in self.context.cases:
            scenario = case["scenario_class"]
            normalized = {
                "outcome_class": scenario,
                "pcb_status": "  " if scenario == "completed" else "GE",
                "pcb_kind": "database",
                "position_state": "unchanged" if scenario == "rejected" else "advanced",
                "position_digest": digest("position-" + case["case_id"]),
                "mutation_state": "no-mutation" if scenario == "rejected" else "committed",
                "state_digest": digest("state-" + case["case_id"]),
                "effect_digest": digest("effect-" + case["case_id"]),
                "output_digest": digest("output-" + case["case_id"]),
                "output_length": 0,
                "output_encoding": "no-output",
            }
            observations.append({
                **{field: copy.deepcopy(case[field]) for field in (
                    "case_id", "row_id", "call_family", "source_locator",
                    "source_topic_path", "source_topic_sha256", "applicability_profiles",
                    "input_profile", "scenario_class", "fixture_bundle_member",
                )},
                "fixture_input_digest": digest("fixture-" + case["case_id"]),
                "product": copy.deepcopy(normalized),
                "oracle": copy.deepcopy(normalized),
            })
        receipt = {
            "schema_version": "mainframe-env.ims-licensed-differential-receipt@1",
            "target_subsystem": "ims.programming", "work_package": "IMS-1406.licensed-harness-contract",
            "source_identity": copy.deepcopy(self.context.source_identity),
            "licensed_service": service,
            "authorization": {
                "authorized": True, "authority": "licensed-environment-controller",
                "grant_id": "test-grant", "grant_digest": pins["authorization_grant_sha256"],
                "environment_manifest_digest": pins["environment_manifest_sha256"],
                "capabilities": copy.deepcopy(self.context.adapter["required_capabilities"]),
            },
            "candidate": {"commit_sha": self.candidate.commit_sha, "tree_sha": self.candidate.tree_sha, "tracked_worktree_clean": True},
            "contract": {
                "spec_digest": self.context.spec_digest,
                "verifier_digest": self.context.verifier_digest,
                "fixture_contract_digest": self.context.fixture_contract_digest,
                "fixture_bundle_digest": pins["fixture_bundle_sha256"],
                "normalization_policy": "ims-normalized-observation@1",
                "runner_protocol": "mainframe-env.ims-licensed-runner@1",
            },
            "runners": {
                "product": {"kind": "mainframe-env-owned", "route": "owned-ims-provider", "digest": pins["product_runner_sha256"], "native_ibm_fallback": False},
                "oracle": {"kind": "licensed-ibm-ims", "route": "licensed-dli-adapter", "digest": pins["oracle_runner_sha256"]},
            },
            "observations": observations,
            "attestation": {
                "receipt_id": "synthetic-test-envelope", "run_id": "unit-test",
                "issued_at": "2026-10-01T00:00:00Z", "licensed_execution": True,
                "independent_product_and_oracle_runs": True,
                "proprietary_bytes_retained_in_receipt": False,
                "credentials_retained_in_receipt": False,
                "raw_output_retained_in_receipt": False,
                "historical_result": False, "synthetic_or_model_result": False,
            },
        }
        return receipt, pins

    def assert_rejected(self, receipt: dict, pins: dict[str, str], message: str) -> None:
        with self.assertRaisesRegex(ims.VerificationError, message):
            ims.verify_receipt_document(receipt, self.context, pins, self.candidate)

    def test_contract_has_25_independent_rows_and_pending_credit(self) -> None:
        self.assertEqual(len(self.context.cases), 25)
        result = ims.pending_result(self.context, ims.current_candidate())
        self.assertEqual(result["status"], "pending-external-licensed-receipt")
        self.assertEqual(result["differential"], {"credited_families": 0, "denominator": 25})

    def test_structural_envelope_and_mutants(self) -> None:
        # This synthetic envelope tests only the parser. It is never written as
        # licensed evidence and cannot pass the CLI without external pins.
        receipt, pins = self.receipt_and_pins()
        self.assertEqual(ims.verify_receipt_document(receipt, self.context, pins, self.candidate)["differential"]["credited_families"], 25)
        mutants = []
        omitted = copy.deepcopy(receipt)
        omitted["observations"].pop()
        mutants.append((omitted, pins, "exactly 25"))
        duplicate = copy.deepcopy(receipt)
        duplicate["observations"][1] = copy.deepcopy(duplicate["observations"][0])
        mutants.append((duplicate, pins, "differs or order changed"))
        reordered = copy.deepcopy(receipt)
        reordered["observations"][0], reordered["observations"][1] = reordered["observations"][1], reordered["observations"][0]
        mutants.append((reordered, pins, "differs or order changed"))
        generic = copy.deepcopy(receipt)
        for observation in generic["observations"]:
            observation["product"]["outcome_class"] = "completed"
            observation["oracle"]["outcome_class"] = "completed"
            observation["product"]["pcb_status"] = "  "
            observation["oracle"]["pcb_status"] = "  "
        mutants.append((generic, pins, "outcome class differs"))
        bad_status = copy.deepcopy(receipt)
        bad_status["observations"][0]["product"]["pcb_status"] = "OKAY"
        mutants.append((bad_status, pins, "PCB status malformed"))
        bad_position = copy.deepcopy(receipt)
        bad_position["observations"][0]["product"]["position_state"] = "somewhere"
        mutants.append((bad_position, pins, "position state malformed"))
        stale_candidate = copy.deepcopy(receipt)
        stale_candidate["candidate"]["commit_sha"] = "c" * 40
        mutants.append((stale_candidate, pins, "candidate is stale"))
        stale_spec = copy.deepcopy(receipt)
        stale_spec["contract"]["spec_digest"] = digest("old-spec")
        mutants.append((stale_spec, pins, "spec or fixture identity is stale"))
        stale_source = copy.deepcopy(receipt)
        stale_source["source_identity"]["comparison_topic_sha256"] = digest("old-source")
        mutants.append((stale_source, pins, "source identity drifted"))
        wrong_release = copy.deepcopy(receipt)
        wrong_release["licensed_service"]["release"] = "14.6.0"
        mutants.append((wrong_release, pins, "release is not 15.6"))
        unauthorized = copy.deepcopy(receipt)
        unauthorized["authorization"]["authorized"] = False
        mutants.append((unauthorized, pins, "unauthorized"))
        tampered = copy.deepcopy(receipt)
        tampered["observations"][0]["product"]["position_digest"] = digest("tampered")
        mutants.append((tampered, pins, "differential mismatch"))
        for document, external, message in mutants:
            with self.subTest(message=message):
                self.assert_rejected(document, external, message)

    def test_external_path_and_pins_fail_closed(self) -> None:
        with self.assertRaisesRegex(ims.VerificationError, "missing"):
            ims.external_pins({})
        inside = ims.ROOT / ims.ADAPTER
        with self.assertRaisesRegex(ims.VerificationError, "inside candidate tree"):
            ims.read_external_receipt(inside, ims.ROOT, ims.file_digest(inside))
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "receipt.json"
            path.write_text(json.dumps({"test": True}))
            with self.assertRaisesRegex(ims.VerificationError, "external digest pin"):
                ims.read_external_receipt(path, ims.ROOT, digest("other"))
            link = Path(directory) / "linked.json"
            link.symlink_to(path)
            with self.assertRaisesRegex(ims.VerificationError, "non-symlink"):
                ims.read_external_receipt(link, ims.ROOT, ims.file_digest(path))

    def test_absent_receipt_is_pending_and_required_gate_fails(self) -> None:
        environment = {key: value for key, value in os.environ.items() if not key.startswith("MAINFRAME_ENV_IMS_")}
        process = subprocess.run(
            [sys.executable, "-B", str(ims.ROOT / ims.VERIFIER), "--check", "--require-pass"],
            cwd=ims.ROOT, env=environment, capture_output=True, text=True, check=False,
        )
        self.assertEqual(process.returncode, 1, process.stderr)
        self.assertEqual(json.loads(process.stdout)["differential"]["credited_families"], 0)
