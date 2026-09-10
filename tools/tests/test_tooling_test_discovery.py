from __future__ import annotations

import importlib.util
import io
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


TOOL = Path(__file__).resolve().parents[1] / "run_tooling_tests.py"
SPEC = importlib.util.spec_from_file_location("run_tooling_tests", TOOL)
assert SPEC is not None and SPEC.loader is not None
RUNNER = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = RUNNER
SPEC.loader.exec_module(RUNNER)


class ToolingTestDiscoveryTests(unittest.TestCase):
    def repository(self, files: dict[str, str], tracked: set[str]) -> Path:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        directory = Path(temporary.name)
        subprocess.run(["git", "init", "-q"], cwd=directory, check=True)
        for name, body in files.items():
            path = directory / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(body)
        subprocess.run(["git", "add", "--", *sorted(tracked)], cwd=directory, check=True)
        return directory

    def test_nested_tracked_python_and_shell_tests_are_discovered_without_a_directory_list(self):
        files = {
            "tools/tests/test_root.py": "import unittest\n",
            "conformance/1.7/tools/tests/test_nested.py": "import unittest\n",
            "conformance/1.8/tools/tests/parser_test.sh": "#!/usr/bin/env bash\ntrue\n",
            "tools/jenkins/helper.sh": "#!/usr/bin/env bash\ntrue\n",
            "docker/entrypoint.sh": "#!/usr/bin/env bash\ntrue\n",
            "docker/dev": "#!/usr/bin/env bash\ntrue\n",
            "docker/dev-bin/cargo": "#!/usr/bin/env bash\ntrue\n",
            "tests/test_not_tooling.py": "import unittest\n",
            "tools/tests/test_untracked.py": "import unittest\n",
        }
        tracked = set(files) - {"tools/tests/test_untracked.py"}
        root = self.repository(files, tracked)
        inventory = RUNNER.discover(root)
        self.assertEqual(
            inventory.python_tests,
            ("conformance/1.7/tools/tests/test_nested.py", "tools/tests/test_root.py"),
        )
        self.assertEqual(
            inventory.shell_tests,
            ("conformance/1.8/tools/tests/parser_test.sh",),
        )
        self.assertEqual(
            inventory.shell_tools,
            ("conformance/1.8/tools/tests/parser_test.sh", "docker/dev",
             "docker/dev-bin/cargo", "docker/entrypoint.sh", "tools/jenkins/helper.sh"),
        )

    def test_discovered_files_execute_and_report_nonempty_cases(self):
        files = {
            "tools/tests/test_ok.py": (
                "import unittest\n"
                "class Cases(unittest.TestCase):\n"
                "  def test_ok(self): self.assertTrue(True)\n"
                "  @unittest.skip('fixture')\n"
                "  def test_skip(self): pass\n"
                "if __name__ == '__main__': unittest.main()\n"
            ),
            "tools/tests/test_ok.sh": "#!/usr/bin/env bash\nset -euo pipefail\ntrue\n",
            "tools/jenkins/helper.sh": "#!/usr/bin/env bash\ntrue\n",
        }
        root = self.repository(files, set(files))
        inventory = RUNNER.discover(root)
        transcript = io.BytesIO()
        summary = RUNNER.run_inventory(root, inventory, transcript)
        self.assertEqual(summary.executed, 3)
        self.assertEqual(summary.skipped, 1)
        self.assertEqual(summary.failed_files, ())
        self.assertEqual(summary.shell_syntax_checks, 2)

    def test_zero_case_python_file_fails_closed(self):
        files = {"tools/tests/test_empty.py": "print('not a unittest suite')\n"}
        root = self.repository(files, set(files))
        summary = RUNNER.run_inventory(root, RUNNER.discover(root), io.BytesIO())
        self.assertIn("tools/tests/test_empty.py", summary.failed_files)


if __name__ == "__main__":
    unittest.main()
