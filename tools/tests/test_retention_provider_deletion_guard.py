import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

TOOLS = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("check_retention_lifecycle", TOOLS / "check_retention_lifecycle.py")
retention = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(retention)


class RetentionProviderDeletionGuardTests(unittest.TestCase):
    def test_ims_selected_output_and_backout_remain_before_publication(self):
        original = Path.read_text
        execution_path = retention.ROOT / "crates/providers/mainframe-env-ims/src/service/execution.rs"
        for marker in ("Result<feedback::ExecutionOutput, HostProblem>",
                       "application_backout::settle_database_call(",
                       "undefined_length: output.undefined_length"):
            with self.subTest(marker=marker):
                def altered(path, *args, **kwargs):
                    source = original(path, *args, **kwargs)
                    return source.replace(marker, "removed_contract") if path == execution_path else source
                with patch.object(Path, "read_text", altered):
                    with self.assertRaises(ValueError):
                        retention.check_provider_codecs(retention.ROOT)

    def test_split_ims_pipeline_retains_replay_refresh_guard(self):
        original = Path.read_text
        execution_path = retention.ROOT / "crates/providers/mainframe-env-ims/src/service/execution.rs"

        def altered(path, *args, **kwargs):
            source = original(path, *args, **kwargs)
            if path == execution_path:
                return source.replace("fn refresh_replay(", "fn removed_replay_refresh(")
            return source

        retention.check_provider_codecs(retention.ROOT)
        with patch.object(Path, "read_text", altered):
            with self.assertRaisesRegex(ValueError, "fn refresh_replay"):
                retention.check_provider_codecs(retention.ROOT)

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
