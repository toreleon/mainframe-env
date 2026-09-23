import importlib.util
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
TOOL = ROOT / "tools" / "check_typed_semantic_boundaries.py"
SPEC = importlib.util.spec_from_file_location("check_typed_semantic_boundaries", TOOL)
typed_boundaries = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(typed_boundaries)


class TypedSemanticBoundaryTests(unittest.TestCase):
    def test_typed_cics_region_uses_current_legacy_helper_boundary(self):
        source = typed_boundaries.production(
            typed_boundaries.read(
                ROOT,
                "crates/kernel/mainframe-env-interpreter/src/machine/typed_cics.rs",
            )
        )
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
