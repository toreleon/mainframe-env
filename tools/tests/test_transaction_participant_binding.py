import importlib.util
from pathlib import Path
import shutil
import tempfile
import unittest


TOOL = Path(__file__).resolve().parents[1] / "check_transaction_participant.py"
ROOT = TOOL.parent.parent
SPEC = importlib.util.spec_from_file_location("check_transaction_participant", TOOL)
binding = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(binding)


class TransactionParticipantBindingTests(unittest.TestCase):
    def fixture(self, root: Path) -> None:
        for relative in [
            "conformance/0.16/contracts/transaction-participant.json",
            "crates/providers/mainframe-env-ims/tests/participant_contract.rs",
            "crates/kernel/mainframe-env-interpreter/src/coordinator.rs",
            "crates/providers/mainframe-env-cics/src/handlers/recovery.rs",
            "crates/providers/mainframe-env-db2/src/service.rs",
            "crates/providers/mainframe-env-ims/src/service.rs",
            "crates/providers/mainframe-env-mq/src/service.rs",
        ]:
            target = root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, target)

    def test_checked_in_bindings_pass(self) -> None:
        binding.check(ROOT)

    def test_private_cics_rejection_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            path = root / "crates/providers/mainframe-env-cics/src/handlers/recovery.rs"
            path.write_text(
                path.read_text().replace(
                    "fn validate_syncpoint_owner",
                    'const PRIVATE: &str = b"dpl-without-synconreturn" | '
                    'b"dpl-executionset-subset";\nfn validate_syncpoint_owner',
                )
            )
            with self.assertRaises(ValueError):
                binding.check(root)

    def test_pending_provider_binding_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            path = root / "crates/providers/mainframe-env-db2/src/service.rs"
            path.write_text(
                path.read_text().replace(
                    "#[cfg(test)]",
                    "transaction_participant_contract_v1\n#[cfg(test)]",
                    1,
                )
            )
            with self.assertRaises(ValueError):
                binding.check(root)

    def test_ims_preparation_without_restart_test_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            path = root / "crates/providers/mainframe-env-ims/tests/participant_contract.rs"
            path.write_text(path.read_text().replace("std::process::Command::new", "removed_restart"))
            with self.assertRaises(ValueError):
                binding.check(root)


if __name__ == "__main__":
    unittest.main()
