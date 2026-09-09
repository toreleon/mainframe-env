from pathlib import Path
import tempfile
import unittest

import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import check_enterprise_authorization as guard


class EnterpriseAuthorizationGuardTests(unittest.TestCase):
    def test_guard_passes_the_repository(self):
        guard.check()

    def test_execute_order_rejects_post_dispatch_authorization(self):
        source = """    pub fn execute(\n) {\n apply_request();\n authorizer.authorize();\n}\n    /// next\n"""
        body = guard.execute_body(source)
        self.assertGreater(body.index("authorizer.authorize("), body.index("apply_request("))


if __name__ == "__main__":
    unittest.main()
