import importlib.util
from pathlib import Path
import tempfile
import unittest

TOOLS = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("check_retention_lifecycle", TOOLS / "check_retention_lifecycle.py")
retention = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(retention)


class RetentionProviderDeletionGuardTests(unittest.TestCase):
    def test_split_provider_deletion_shape_and_both_execution_bounds_are_guarded(self):
        relative = "crates/stores/mainframe-env-store/src/retention/provider_deletion.rs"
        source = (TOOLS.parent / relative).read_text()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            original_root = retention.ROOT
            retention.ROOT = root
            self.addCleanup(setattr, retention, "ROOT", original_root)
            path = root / relative
            path.parent.mkdir(parents=True)
            path.write_text(source)
            retention.check_provider_deletion(root)
            for fragment in [
                "pub(crate) fn validate_provider_deletion(",
                "ProviderRetentionDependency::CoreEffect",
                "ProviderRetentionDependency::CicsNested",
                "ProviderRetentionDependency::ProviderGraph",
                "required_executions.len() > 32",
                "required_executions.len() <= 32",
                "ProviderRetentionDependency::DirectProduct",
            ]:
                with self.subTest(fragment=fragment):
                    path.write_text(source.replace(fragment, "removed_contract"))
                    with self.assertRaisesRegex(ValueError, "retention contract is incomplete"):
                        retention.check_provider_deletion(root)


if __name__ == "__main__":
    unittest.main()
