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
    def test_production_keeps_code_after_inline_cfg_test_method(self):
        source = (
            "impl Pilot {\n"
            "    #[cfg(test)]\n"
            "    fn helper(&self) { let text = \"} string\"; /* { comment */ }\n"
            "    fn execute(&self) { self.coordinator.execute(); }\n"
            "}\n"
        )
        result = typed_boundaries.production(source)
        self.assertIn("fn execute(&self)", result)
        self.assertNotIn("fn helper(&self)", result)

    def test_production_removes_trailing_cfg_test_module(self):
        source = (
            "fn execute() {}\n"
            "#[cfg(test)]\n"
            "mod tests { fn helper() { let text = \"{\"; // } comment\n"
            "} }\n"
        )
        self.assertIn("fn execute()", typed_boundaries.production(source))
        self.assertNotIn("mod tests", typed_boundaries.production(source))

    def test_production_removes_braceless_cfg_test_items_through_semicolon(self):
        source = (
            "#[cfg(test)]\nuse tests::Helper;\n"
            "fn middle() {}\n"
            "#[cfg(test)]\nconst SAMPLE: &str = \"};\";\n"
            "fn end() {}\n"
        )
        result = typed_boundaries.production(source)
        self.assertNotIn("use tests::Helper", result)
        self.assertNotIn("const SAMPLE", result)
        self.assertIn("fn middle()", result)
        self.assertIn("fn end()", result)

    def test_production_removes_stacked_attributes_with_cfg_test_item(self):
        source = (
            "#[cfg(test)]\n"
            "#[allow(dead_code)]\n"
            "#[doc = \"{;}\"]\n"
            "fn helper() { let brace = '}'; }\n"
            "fn execute() {}\n"
        )
        result = typed_boundaries.production(source)
        self.assertNotIn("#[allow(dead_code)]", result)
        self.assertNotIn("#[doc", result)
        self.assertNotIn("fn helper()", result)
        self.assertIn("fn execute()", result)

    def test_production_keeps_cfg_test_text_inside_literals_and_comments(self):
        source = (
            'const TEXT: &str = "#[cfg(test)]";\n'
            'const RAW: &str = r#"#[cfg(test)]"#;\n'
            '// #[cfg(test)]\n'
            '/* #[cfg(test)] */\n'
            "fn execute() {}\n"
        )
        self.assertEqual(typed_boundaries.production(source), source)

    def test_production_removes_cfg_test_const_fn_body_only(self):
        source = (
            "#[cfg(test)]\nconst fn helper() -> usize { 1 }\n"
            "fn execute() {}\n"
        )
        result = typed_boundaries.production(source)
        self.assertNotIn("const fn helper", result)
        self.assertIn("fn execute()", result)

    def test_reject_sees_forbidden_pattern_after_inline_cfg_test_item(self):
        source = (
            "#[cfg(test)]\nfn helper() {}\n"
            "fn execute() { forbidden_runtime_path(); }\n"
        )
        with self.assertRaisesRegex(typed_boundaries.BoundaryError, "forbidden_runtime_path"):
            typed_boundaries.reject(
                typed_boundaries.production(source),
                ["forbidden_runtime_path"],
                "typed runtime",
            )

    def test_cics_descriptor_registry_accepts_split_entries_and_rejects_missing_literal(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            descriptor = root / "crates/foundation/mainframe-env-ir/src/cics_descriptor"
            entries = descriptor / "executable_entries"
            entries.mkdir(parents=True)
            (descriptor / "executable_entries.rs").write_text(
                "pub const CICS_EXECUTABLE_DESCRIPTORS: [CicsExecutableDescriptor; 0] = [];\n"
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
                + '#[cfg(test)]\nconst SHADOW: &str = "namespace: \\"cics.file\\"";\n'
            )
            with self.assertRaisesRegex(
                typed_boundaries.BoundaryError,
                'typed CICS descriptor registry omits namespace: "cics.file"',
            ):
                typed_boundaries.check_cics_descriptor_entries(root)

    def test_descriptor_registry_may_be_declared_static(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            descriptor = root / "crates/foundation/mainframe-env-ir/src/cics_descriptor"
            descriptor.mkdir(parents=True)
            registry = descriptor / "executable_entries.rs"
            registry.write_text(
                "pub static CICS_EXECUTABLE_DESCRIPTORS: [CicsExecutableDescriptor; 0] = [];\n"
                'namespace: "cics.file"\n'
                'namespace: "cics.recovery"\n'
                "operation: CicsPlanOperation::Read\n"
                "operation: CicsPlanOperation::Rewrite\n"
                "operation: CicsPlanOperation::Syncpoint\n"
            )
            typed_boundaries.check_cics_descriptor_entries(root)
            registry.write_text(
                registry.read_text().replace(": [CicsExecutableDescriptor; 0] = [];", ": () = ();")
            )
            with self.assertRaisesRegex(
                typed_boundaries.BoundaryError,
                "typed CICS descriptor registry omits",
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
