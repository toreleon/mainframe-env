from __future__ import annotations

import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))

import verify_mq_licensed_differential as mq  # noqa: E402


def digest(label: str) -> str:
    return "sha256:" + hashlib.sha256(label.encode()).hexdigest()


class MqLicensedDifferentialTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.context = mq.verify_contract()
        cls.candidate = mq.CandidateIdentity("a" * 40, "b" * 40, True)

    def receipt_and_pins(self) -> tuple[dict, mq.ExternalPins]:
        service = {
            "product_name": "IBM MQ",
            "major_minor": "9.4",
            "vrmf": "9.4.0.1",
            "command_level": 940,
            "build_id": "unit-test-build",
            "installation_digest": digest("installation"),
        }
        pins = mq.ExternalPins(
            receipt_sha256=digest("external-receipt"),
            service_identity_sha256=mq.canonical_digest(service),
            environment_manifest_sha256=digest("environment"),
            authorization_grant_sha256=digest("authorization"),
            fixture_bundle_sha256=digest("fixture-bundle"),
            product_runner_sha256=digest("product-runner"),
            oracle_runner_sha256=digest("oracle-runner"),
        )
        observations = []
        for number, case in enumerate(self.context.fixtures["cases"], 1):
            if number % 5 == 0:
                completion = "MQCC_FAILED"
                reason = "MQRC_HCONN_ERROR"
                handle = "invalid"
                mutation = "no-mutation"
            elif number % 3 == 0:
                completion = "MQCC_WARNING"
                reason = "MQRC_TRUNCATED_MSG_ACCEPTED"
                handle = "unchanged"
                mutation = "pending"
            else:
                completion = "MQCC_OK"
                reason = "MQRC_NONE"
                handle = "valid"
                mutation = "committed"
            output_length = number if number % 2 == 0 else 0
            normalized = {
                "completion_code": completion,
                "reason_code": reason,
                "handle_state": handle,
                "mutation_state": mutation,
                "state_digest": digest(f"state-{case['call']}"),
                "effect_digest": digest(f"effect-{case['call']}"),
                "output_digest": digest(f"output-{case['call']}"),
                "output_length": output_length,
                "output_encoding": "binary" if output_length else "no-output",
                "status_digest": digest(f"status-{case['call']}"),
            }
            observations.append(
                {
                    "case_id": case["case_id"],
                    "row_id": case["row_id"],
                    "call": case["call"],
                    "source_topic_path": case["source_topic_path"],
                    "input_profile": case["input_profile"],
                    "fixture_bundle_member": case["fixture_bundle_member"],
                    "fixture_input_digest": digest(f"fixture-{case['call']}"),
                    "product": copy.deepcopy(normalized),
                    "oracle": copy.deepcopy(normalized),
                }
            )
        receipt = {
            "schema_version": "mainframe-env.mq-licensed-differential-receipt@1",
            "target_version": "0.15.0",
            "work_package": "MQ-1506.licensed-harness-contract",
            "source_identity": copy.deepcopy(self.context.source_identity),
            "licensed_service": service,
            "authorization": {
                "authorized": True,
                "authority": "licensed-environment-controller",
                "grant_id": "unit-test-grant",
                "grant_digest": pins.authorization_grant_sha256,
                "environment_manifest_digest": pins.environment_manifest_sha256,
                "capabilities": copy.deepcopy(self.context.adapter["required_capabilities"]),
            },
            "candidate": {
                "commit_sha": self.candidate.commit_sha,
                "tree_sha": self.candidate.tree_sha,
                "tracked_worktree_clean": True,
            },
            "contract": {
                "spec_digest": self.context.spec_digest,
                "verifier_digest": self.context.verifier_digest,
                "fixture_contract_digest": self.context.fixture_contract_digest,
                "fixture_bundle_digest": pins.fixture_bundle_sha256,
                "normalization_policy": "mq-normalized-observation@1",
                "runner_protocol": "mainframe-env.mq-licensed-runner@1",
            },
            "runners": {
                "product": {
                    "kind": "mainframe-env-owned",
                    "route": "owned-mq-provider",
                    "digest": pins.product_runner_sha256,
                    "native_ibm_fallback": False,
                },
                "oracle": {
                    "kind": "licensed-ibm-mq",
                    "route": "licensed-mqi-adapter",
                    "digest": pins.oracle_runner_sha256,
                },
            },
            "observations": observations,
            "attestation": {
                "receipt_id": "unit-test-structural-envelope",
                "run_id": "unit-test-run",
                "issued_at": "2026-09-22T00:00:00Z",
                "licensed_execution": True,
                "independent_product_and_oracle_runs": True,
                "proprietary_bytes_retained_in_receipt": False,
                "credentials_retained_in_receipt": False,
                "raw_output_retained_in_receipt": False,
                "historical_result": False,
                "synthetic_or_model_result": False,
            },
        }
        return receipt, pins

    def assert_rejected(self, receipt: dict, pins: mq.ExternalPins, text: str) -> None:
        with self.assertRaisesRegex(mq.VerificationError, text):
            mq.verify_receipt_document(receipt, self.context, pins, self.candidate)

    def test_contract_is_exact_and_absent_receipt_has_zero_credit(self) -> None:
        pending = mq.pending_result(self.context, mq.current_candidate())
        self.assertEqual(pending["status"], "pending-external-licensed-receipt")
        self.assertEqual(pending["differential"], {"credited_calls": 0, "denominator": 26})
        self.assertEqual(len(self.context.calls), 26)
        self.assertEqual(len(self.context.cases), 26)
        self.assertEqual(
            self.context.adapter["denominator"]["baseline_id"],
            "ibm-mq-9.4-mqi-2026-08-31",
        )
        self.assertEqual(
            self.context.adapter["denominator"]["reviewed_source_topics"],
            [
                {
                    "call": "MQCONNX",
                    "topic_path": "SSFKSJ_9.4.0/refdev/q101770_.html",
                    "sha256": "sha256:41e9da41eb766141814ba1b2c3dc9c649450d1ab6cc64d574165f239ea8ed633",
                    "review_method": "ibm_docs.py-read",
                    "coverage_credit": 0,
                },
                {
                    "call": "MQSTAT",
                    "topic_path": "SSFKSJ_9.4.0/refdev/q101920_.html",
                    "sha256": "sha256:4f19dab47ed3e325ec894cc74a8d88db265a7c27f8ff2a7c507f94cf4bedd1d9",
                    "review_method": "ibm_docs.py-read",
                    "coverage_credit": 0,
                },
                {
                    "call": "MQINQ",
                    "topic_path": "SSFKSJ_9.4.0/refdev/q101840_.html",
                    "sha256": "sha256:03e3347bbf16d2f8e3a9061e921dbfca7a3afd0fe3bc13418ebdf47bb652ce1b",
                    "review_method": "retained-html-sha256",
                    "coverage_credit": 0,
                },
            ],
        )
        self.assertEqual(
            self.context.adapter["denominator"]["unavailable_source_topics"],
            [],
        )

    def test_absent_receipt_gate_exits_nonzero_without_fabricating_pass(self) -> None:
        environment = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith("MAINFRAME_ENV_MQ_")
        }
        process = subprocess.run(
            [
                sys.executable,
                "-B",
                str(TOOLS / "verify_mq_licensed_differential.py"),
                "--check",
                "--require-pass",
            ],
            cwd=mq.REPOSITORY,
            env=environment,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        self.assertEqual(process.returncode, 1, process.stderr)
        result = json.loads(process.stdout)
        self.assertEqual(result["status"], "pending-external-licensed-receipt")
        self.assertEqual(result["differential"]["credited_calls"], 0)

    def test_untracked_repository_file_invalidates_candidate(self) -> None:
        with tempfile.TemporaryDirectory(prefix="mq-licensed-candidate-") as temporary:
            repository = Path(temporary)
            for arguments in [
                ["init", "--quiet"],
                ["config", "user.email", "tests@mainframe.invalid"],
                ["config", "user.name", "mainframe-env tests"],
            ]:
                subprocess.run(
                    ["git", *arguments],
                    cwd=repository,
                    check=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True,
                )
            (repository / "tracked.txt").write_text("tracked\n", encoding="utf-8")
            subprocess.run(
                ["git", "add", "tracked.txt"],
                cwd=repository,
                check=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            subprocess.run(
                ["git", "commit", "--quiet", "-m", "baseline"],
                cwd=repository,
                check=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            self.assertTrue(mq.current_candidate(repository).tracked_worktree_clean)

            (repository / "untracked-config.json").write_text("{}\n", encoding="utf-8")
            self.assertEqual(
                mq.git_output(repository, ["status", "--porcelain", "--untracked-files=no"]),
                "",
            )
            self.assertFalse(mq.current_candidate(repository).tracked_worktree_clean)

    def test_structural_test_envelope_covers_every_call(self) -> None:
        receipt, pins = self.receipt_and_pins()
        result = mq.verify_receipt_document(receipt, self.context, pins, self.candidate)
        self.assertEqual(result["differential"], {"credited_calls": 26, "denominator": 26})

    def test_m3_structure_topic_drift_is_rejected(self) -> None:
        original_load_json = mq.load_json

        def changed_catalog(path: Path, label: str | None = None) -> dict:
            value = original_load_json(path, label)
            if path == mq.REPOSITORY / mq.STRUCTURE_CATALOG_PATH:
                value["calls"][0]["topic_sha256"] = "0" * 64
            return value

        with mock.patch.object(mq, "load_json", side_effect=changed_catalog):
            with self.assertRaisesRegex(mq.VerificationError, "M3 call topics"):
                mq.verify_contract()

    def test_omitted_transition_mutant_is_killed(self) -> None:
        receipt, pins = self.receipt_and_pins()
        receipt["observations"].pop()
        self.assert_rejected(receipt, pins, "exactly 26")

    def test_generic_success_mutant_is_killed(self) -> None:
        receipt, pins = self.receipt_and_pins()
        observation = next(
            row for row in receipt["observations"] if row["product"]["completion_code"] == "MQCC_FAILED"
        )
        observation["product"]["completion_code"] = "MQCC_OK"
        observation["product"]["reason_code"] = "MQRC_NONE"
        self.assert_rejected(receipt, pins, "differential mismatch")

    def test_authorization_bypass_mutant_is_killed(self) -> None:
        receipt, pins = self.receipt_and_pins()
        receipt["authorization"]["authorized"] = False
        self.assert_rejected(receipt, pins, "not authorized")

    def test_forbidden_mutation_mutant_is_killed(self) -> None:
        receipt, pins = self.receipt_and_pins()
        receipt["observations"][0]["product"]["state_digest"] = digest("forbidden-mutation")
        self.assert_rejected(receipt, pins, "differential mismatch")

    def test_byte_encoding_mutant_is_killed(self) -> None:
        receipt, pins = self.receipt_and_pins()
        observation = next(
            row for row in receipt["observations"] if row["product"]["output_length"] > 0
        )
        observation["product"]["output_encoding"] = "utf-8"
        self.assert_rejected(receipt, pins, "differential mismatch")

    def test_raw_output_and_secret_shaped_fields_are_rejected(self) -> None:
        receipt, pins = self.receipt_and_pins()
        receipt["observations"][0]["product"]["raw_output"] = "not-allowed"
        self.assert_rejected(receipt, pins, "fields differ")
        receipt, pins = self.receipt_and_pins()
        receipt["authorization"]["credential"] = "not-allowed"
        self.assert_rejected(receipt, pins, "fields differ")

    def test_native_oracle_fallback_and_stale_identities_are_rejected(self) -> None:
        receipt, pins = self.receipt_and_pins()
        receipt["runners"]["product"]["native_ibm_fallback"] = True
        self.assert_rejected(receipt, pins, "owned provider")
        receipt, pins = self.receipt_and_pins()
        receipt["contract"]["spec_digest"] = digest("stale-spec")
        self.assert_rejected(receipt, pins, "identity drifted")

    def test_fixture_generator_is_fresh(self) -> None:
        process = subprocess.run(
            [sys.executable, "-B", str(TOOLS / "generate_mq_licensed_fixtures.py"), "--check"],
            cwd=mq.REPOSITORY,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertIn("cases=26", process.stdout)


if __name__ == "__main__":
    unittest.main()
