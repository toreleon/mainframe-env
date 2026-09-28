import importlib.util
from pathlib import Path
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
TOOL = ROOT / "tools" / "check_typed_semantic_boundaries.py"
SPEC = importlib.util.spec_from_file_location("check_typed_semantic_boundaries", TOOL)
typed_boundaries = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(typed_boundaries)


class TypedSemanticBoundaryTests(unittest.TestCase):
    def test_cics_descriptor_registry_accepts_split_entries_and_rejects_missing_literal(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            descriptor = root / "crates/foundation/mainframe-env-ir/src/cics_descriptor"
            entries = descriptor / "executable_entries"
            entries.mkdir(parents=True)
            (descriptor / "executable_entries.rs").write_text(
                "pub const CICS_EXECUTABLE_DESCRIPTORS: () = ();\n"
            )
            base = entries / "base_entries.rs"
            base.write_text(
                'namespace: "cics.file"\n'
                "operation: CicsPlanOperation::Read\n"
                "operation: CicsPlanOperation::Rewrite\n"
            )
            (entries / "recovery_entries.rs").write_text(
                'namespace: "cics.recovery"\n'
                "operation: CicsPlanOperation::Syncpoint\n"
            )
            typed_boundaries.check_cics_descriptor_entries(root)
            base.write_text(
                base.read_text().replace('namespace: "cics.file"', 'namespace: "missing.file"')
                + '#[cfg(test)]\nnamespace: "cics.file"\n'
            )
            with self.assertRaisesRegex(
                typed_boundaries.BoundaryError,
                'typed CICS descriptor registry omits namespace: "cics.file"',
            ):
                typed_boundaries.check_cics_descriptor_entries(root)

    def test_typed_cics_region_uses_current_legacy_helper_boundary(self):
        source = typed_boundaries.read(
            ROOT,
            "crates/kernel/mainframe-env-interpreter/src/machine/typed_cics.rs",
        ).split("#[cfg(test)]\nmod tests", 1)[0]
        region = typed_boundaries.between(
            source,
            "pub(super) fn execute(\n",
            "fn legacy_condition_policy(\n",
        )
        self.assertIn("retrieve::allocation_arguments(", region)
        typed_boundaries.reject(
            region,
            ["legacy_arguments", "\n    arguments("],
            "typed CICS runtime",
        )
        with self.assertRaises(typed_boundaries.BoundaryError):
            typed_boundaries.reject(
                region + "\n    arguments(machine);\n",
                ["\n    arguments("],
                "typed CICS runtime",
            )


if __name__ == "__main__":
    unittest.main()
