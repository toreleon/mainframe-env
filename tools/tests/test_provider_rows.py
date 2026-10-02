"""Keep the row guard effective across the MQ service/module split."""

from pathlib import Path
import shutil
import tempfile
import unittest

from tools.check_provider_rows import check


ROOT = Path(__file__).resolve().parents[2]
MQ_SERVICE = Path("crates/providers/mainframe-env-mq/src/service.rs")
MQ_ROWS = MQ_SERVICE.with_name("service_rows.rs")


class ProviderRowsTests(unittest.TestCase):
    def fixture(self):
        temporary = tempfile.TemporaryDirectory(prefix="provider-rows-")
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        for path in (
            MQ_SERVICE,
            MQ_ROWS,
            Path("crates/providers/mainframe-env-ims/src/service.rs"),
            Path("crates/providers/mainframe-env-db2/src/service.rs"),
        ):
            target = root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / path, target)
        return root

    def test_linked_row_module_passes(self):
        check(self.fixture())

    def test_missing_atomic_publication_in_child_fails(self):
        root = self.fixture()
        path = root / MQ_ROWS
        path.write_text(path.read_text().replace(".mutate_provider_states_atomic(", ".not_atomic("))
        with self.assertRaisesRegex(ValueError, "mutate_provider_states_atomic"):
            check(root)

    def test_unlinked_child_cannot_supply_evidence(self):
        root = self.fixture()
        path = root / MQ_SERVICE
        path.write_text(path.read_text().replace("mod rows;", "mod other_rows;"))
        with self.assertRaisesRegex(ValueError, "linked row module"):
            check(root)

    def test_whole_state_persistence_in_child_fails(self):
        root = self.fixture()
        path = root / MQ_ROWS
        path.write_text(path.read_text() + "\nfn bad() { serde_json::to_vec(&state); }\n")
        with self.assertRaisesRegex(ValueError, "whole-state persistence"):
            check(root)


if __name__ == "__main__":
    unittest.main()
