"""Mutation checks for the provider-row guard's relocated production helpers."""

import importlib.util
from pathlib import Path
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("provider_rows", ROOT / "tools/check_provider_rows.py")
GUARD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GUARD)


class ProviderRowGuardTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        for provider in ("db2", "ims", "mq"):
            relative = Path(f"crates/providers/mainframe-env-{provider}/src/service.rs")
            self.copy(relative)
            if provider != "db2":
                self.copy(relative.with_suffix("") / "row_store.rs")

    def copy(self, relative):
        target = self.root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text((ROOT / relative).read_text())

    def mutate(self, provider, child, before, after):
        path = self.root / f"crates/providers/mainframe-env-{provider}/src/service"
        path = path / "row_store.rs" if child else path.with_suffix(".rs")
        text = path.read_text()
        self.assertIn(before, text)
        path.write_text(text.replace(before, after))

    def test_connected_production_helpers_pass(self):
        GUARD.check(self.root)

    def test_atomic_publication_missing_from_child_is_rejected(self):
        for provider in ("ims", "mq"):
            with self.subTest(provider=provider):
                self.mutate(provider, True, ".mutate_provider_states_atomic(", ".wrong_publication(")
                with self.assertRaisesRegex(ValueError, "mutate_provider_states_atomic"):
                    GUARD.check(self.root)
                self.mutate(provider, True, ".wrong_publication(", ".mutate_provider_states_atomic(")

    def test_disconnected_child_is_rejected(self):
        self.mutate("mq", False, "mod row_store;", "// disconnected row_store;")
        with self.assertRaisesRegex(ValueError, "mod row_store"):
            GUARD.check(self.root)

    def test_whole_state_write_in_child_is_rejected(self):
        self.mutate("ims", True, "use super::*;", "use super::*;\n// serde_json::to_vec(&state)")
        with self.assertRaisesRegex(ValueError, "whole-state persistence"):
            GUARD.check(self.root)


if __name__ == "__main__":
    unittest.main()
