import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest

TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))
SPEC = importlib.util.spec_from_file_location("check_execution_route", TOOLS / "check_execution_route.py")
execution_route = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(execution_route)


class ExecutionRouteTests(unittest.TestCase):
    def test_production_drive_is_rejected(self):
        self.assertFalse(execution_route.check_source("pub fn run() { machine.drive(); }\n"))

    def test_terminal_test_probe_is_not_a_production_route(self):
        self.assertTrue(execution_route.check_source(
            'pub fn run() { coordinator.execute(); }\n#[cfg(test)]\nmod tests {\n'
            '    const TEXT: &str = "} not a brace";\n    fn probe() { machine.drive(); }\n}\n'
        ))

    def test_appended_production_and_interior_tests_are_not_exempt(self):
        source = "pub fn run() {}\n#[cfg(test)]\nmod tests { fn probe() { machine.drive(); } }\n"
        self.assertFalse(execution_route.check_source(source + "pub fn appended() { machine.drive(); }\n"))
        self.assertFalse(execution_route.check_source(source + "pub fn appended() {}\n"))

    def test_marker_inside_a_raw_string_does_not_hide_production(self):
        self.assertFalse(execution_route.check_source(
            'const TEXT: &str = r#"\n#[cfg(test)]\nmod tests { }\n"#;\npub fn run() { machine.drive(); }\n'
        ))

    def test_application_and_gateway_trees_are_both_checked(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for directory in ["crates/apps/app/src", "crates/gateways/gateway/src"]:
                path = root / directory / "lib.rs"
                path.parent.mkdir(parents=True)
                path.write_text("pub fn run() { machine.drive(); }\n")
            self.assertEqual(len(execution_route.check(root)), 2)

    def test_missing_source_root_fails_closed(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(OSError, "missing production source root"):
                execution_route.check(Path(temporary))


if __name__ == "__main__":
    unittest.main()
