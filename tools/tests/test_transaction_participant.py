import copy
import importlib.util
import json
import re
from pathlib import Path
import unittest


TOOL = Path(__file__).resolve().parents[1] / "generate_transaction_participant.py"
ROOT = TOOL.parent.parent
SPEC = importlib.util.spec_from_file_location("generate_transaction_participant", TOOL)
participant = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(participant)


class TransactionParticipantTests(unittest.TestCase):
    def setUp(self) -> None:
        self.contract = json.loads((ROOT / participant.CONTRACT_PATH).read_text())
        self.fixtures = json.loads((ROOT / participant.FIXTURE_PATH).read_text())

    def test_checked_in_contract_and_fixtures_validate(self) -> None:
        participant.validate_contract(self.contract)
        participant.validate_fixtures(self.fixtures, self.contract)
        self.assertEqual(
            (ROOT / participant.OUTPUT_PATH).read_text(),
            participant.render(self.contract),
        )

    def test_effect_or_lock_order_drift_is_rejected(self) -> None:
        for field in ["effect", "locks"]:
            mutated = copy.deepcopy(self.contract)
            mutated["ordering"][field].reverse()
            with self.assertRaises(participant.ContractError):
                participant.validate_contract(mutated)

    def test_future_provider_cannot_gain_unowned_capabilities(self) -> None:
        mutated = copy.deepcopy(self.contract)
        mutated["participants"][1]["status"] = "accepted"
        mutated["participants"][1]["capabilities"] = copy.deepcopy(
            mutated["participants"][0]["capabilities"]
        )
        with self.assertRaises(participant.ContractError):
            participant.validate_contract(mutated)

    def test_ims_preparation_is_optional_and_cannot_admit_or_hide_blockers(self) -> None:
        old = copy.deepcopy(self.contract)
        del old["participants"][2]["preparation"]
        participant.validate_contract(old)
        for field, replacement in [
            ("status", "accepted"),
            ("capabilities", self.contract["participants"][0]["capabilities"]),
            ("preparation", {"scope": "ims-local-database-provider-route"}),
            ("preparation", None),
        ]:
            mutated = copy.deepcopy(self.contract)
            mutated["participants"][2][field] = replacement
            with self.assertRaises(participant.ContractError):
                participant.validate_contract(mutated)
        for index in [0, 1, 3]:
            mutated = copy.deepcopy(self.contract)
            mutated["participants"][index]["preparation"] = mutated["participants"][2]["preparation"]
            with self.assertRaises(participant.ContractError):
                participant.validate_contract(mutated)
    def test_outcome_partition_cannot_hide_unknown_or_heuristic_state(self) -> None:
        mutated = copy.deepcopy(self.contract)
        outcomes = mutated["participants"][0]["capabilities"]["outcomes"]
        outcomes["reported"].remove("unknown-outcome")
        outcomes["not_produced"].remove("in-doubt")
        with self.assertRaises(participant.ContractError):
            participant.validate_contract(mutated)

    def test_fixture_cannot_claim_ownership_for_subordinate_dpl(self) -> None:
        mutated = copy.deepcopy(self.fixtures)
        case = next(
            case
            for case in mutated["cics_cases"]
            if case["id"] == "subordinate-dpl-rejected"
        )
        case["syncpoint_owner"] = "participant"
        with self.assertRaises(participant.ContractError):
            participant.validate_fixtures(mutated, self.contract)

    def test_fixture_reader_version_is_closed(self) -> None:
        mutated = copy.deepcopy(self.fixtures)
        mutated["reader_cases"][2] = {
            "id": "read-v2",
            "version": 2,
            "expected": "accepted",
        }
        with self.assertRaises(participant.ContractError):
            participant.validate_fixtures(mutated, self.contract)

    def test_frame_codec_declaration_matches_existing_provider_readers(self) -> None:
        source = (ROOT / "crates/providers/mainframe-env-cics/src/retention.rs").read_text()
        codecs = re.findall(r'const UOW_V\d_MAGIC: &\[u8; 5\] = b"(MECU\d)";', source)
        schemas = self.contract["participants"][0]["capabilities"]["schemas"]
        self.assertEqual(schemas["uow_read"], codecs)
        self.assertEqual(schemas["uow_write"], "MECU2")
        self.assertEqual(schemas["uow_frame_write"], "MECU3")

    def test_legacy_declaration_retains_its_version_one_shape(self) -> None:
        legacy = copy.deepcopy(self.contract)
        schemas = legacy["participants"][0]["capabilities"]["schemas"]
        del schemas["uow_frame_write"]
        schemas["uow_read"] = ["MECU1", "MECU2"]
        participant.validate_contract(legacy)
        self.assertIn("uow_frame_write: None", participant.render(legacy))

    def test_partial_or_unknown_frame_codec_metadata_is_rejected(self) -> None:
        for field, value in [("uow_frame_write", "MECU4"), ("uow_frame_write", None), ("uow_read", ["MECU1", "MECU2"])]:
            mutated = copy.deepcopy(self.contract)
            mutated["participants"][0]["capabilities"]["schemas"][field] = value
            with self.subTest(field=field, value=value), self.assertRaises(participant.ContractError):
                participant.validate_contract(mutated)
        mutated = copy.deepcopy(self.contract)
        del mutated["participants"][0]["capabilities"]["schemas"]["uow_frame_write"]
        with self.assertRaises(participant.ContractError):
            participant.validate_contract(mutated)


if __name__ == "__main__":
    unittest.main()
