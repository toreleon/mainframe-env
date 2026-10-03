import copy
import importlib.util
import json
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


if __name__ == "__main__":
    unittest.main()
