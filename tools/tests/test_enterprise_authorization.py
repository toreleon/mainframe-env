from pathlib import Path
import tempfile
import unittest

import sys
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import check_enterprise_authorization as guard


class EnterpriseAuthorizationGuardTests(unittest.TestCase):
    def test_guard_passes_the_repository(self):
        guard.check()

    def test_execute_order_rejects_post_dispatch_authorization(self):
        source = """    pub fn execute(\n) {\n apply_request();\n authorizer.authorize();\n}\n    /// next\n"""
        body = guard.execute_body(source)
        self.assertGreater(body.index("authorizer.authorize("), body.index("apply_request("))

    def test_split_ims_pipeline_cannot_omit_authorization(self):
        original = Path.read_text
        execution_path = guard.ROOT / "crates/providers/mainframe-env-ims/src/service/execution.rs"

        def altered(path, *args, **kwargs):
            source = original(path, *args, **kwargs)
            if path == execution_path:
                return source.replace("authorizer.authorize(", "removed_authorization(")
            return source

        with patch.object(Path, "read_text", altered):
            with self.assertRaisesRegex(ValueError, "ims execute omits enterprise authorization"):
                guard.check()

    def test_unowned_u_guard_cannot_precede_exact_replay(self):
        original = Path.read_text
        execution_path = guard.ROOT / "crates/providers/mainframe-env-ims/src/service/execution.rs"

        def altered(path, *args, **kwargs):
            source = original(path, *args, **kwargs)
            if path == execution_path:
                marker = "generic::gsam::reject_unowned_length("
                source = source.replace(marker, "removed_unowned_guard(")
                source = source.replace("        let replay_key =", marker + "\n        let replay_key =")
            return source

        with patch.object(Path, "read_text", altered):
            with self.assertRaisesRegex(ValueError, "unowned U guard hides retained replay"):
                guard.check()


if __name__ == "__main__":
    unittest.main()
