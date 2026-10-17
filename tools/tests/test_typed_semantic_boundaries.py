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
    def test_linked_private_module_preserves_ownership_without_public_export(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory) / "product.rs"
            child = Path(directory) / "product" / "dispatch.rs"
            child.parent.mkdir()
            child.write_text('fn execute() {}\n#[cfg(test)] mod fixtures { const NAME: &str = "COBTUPDT"; }')
            parent.write_text("mod dispatch;")
            production = typed_boundaries.linked_module_production(parent, "dispatch")
            self.assertIn("fn execute()", production)
            self.assertNotIn("COBTUPDT", production)
            for source in (
                "// mod dispatch;",
                'const NAME: &str = "mod dispatch;";',
                "#[cfg(test)] mod dispatch;",
                "mod fixtures { mod dispatch; }",
                '#[path = "other.rs"] mod dispatch;',
            ):
                with self.subTest(source=source):
                    parent.write_text(source)
                    with self.assertRaises(typed_boundaries.BoundaryError):
                        typed_boundaries.linked_module_production(parent, "dispatch")

    def test_linked_production_requires_real_top_level_module_and_export(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory) / "product.rs"
            child = Path(directory) / "product" / "trust.rs"
            child.parent.mkdir()
            child.write_text('pub struct Authority;\n#[cfg(test)] mod tests { const KEY: &str = "SECRET"; }')
            parent.write_text("mod trust;\npub use trust::Authority;")
            self.assertIn("pub struct Authority", typed_boundaries.linked_production(parent, "trust", "Authority"))
            self.assertNotIn("SECRET", typed_boundaries.linked_production(parent, "trust", "Authority"))
            for source in (
                "pub use trust::Authority;",
                "mod trust;",
                "// mod trust;\npub use trust::Authority;",
                'const TEXT: &str = "mod trust;";\npub use trust::Authority;',
                "#[cfg(test)] mod trust;\npub use trust::Authority;",
                "mod trust;\n#[cfg(test)] pub use trust::Authority;",
                "mod nested { mod trust; }\npub use trust::Authority;",
                'fn fixture() { let text = "mod trust;"; }\npub use trust::Authority;',
                '/* mod trust; */\npub use trust::Authority;',
                '#[path = "other.rs"] mod trust;\npub use trust::Authority;',
            ):
                with self.subTest(source=source):
                    parent.write_text(source)
                    with self.assertRaises(typed_boundaries.BoundaryError):
                        typed_boundaries.linked_production(parent, "trust", "Authority")

    def test_linked_production_refuses_missing_child_and_path_identifiers(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory) / "product.rs"
            parent.write_text("mod trust;\npub use trust::Authority;")
            with self.assertRaises(FileNotFoundError):
                typed_boundaries.linked_production(parent, "trust", "Authority")
            for module, exported in (("../trust", "Authority"), ("trust", "Authority;")):
                with self.assertRaises(typed_boundaries.BoundaryError):
                    typed_boundaries.linked_production(parent, module, exported)

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


class ProductArtifactAdmissionOwnerTests(unittest.TestCase):
    def fixture(self, root):
        paths = {
            "model": "crates/contracts/mainframe-env-store-api/src/model.rs",
            "cobol": "crates/apps/mainframe-env-server/src/cobol.rs",
            "artifact": "crates/apps/mainframe-env-server/src/cobol/artifact.rs",
            "product": "crates/apps/mainframe-env-server/src/product.rs",
            "machine": "crates/apps/mainframe-env-server/src/product/online_machine.rs",
        }
        sources = {
            "model": "pub struct ExecutableArtifactMetadata;\n"
                     "pub struct ArtifactRecord { pub executable: Option<ExecutableArtifactMetadata> }\n",
            "cobol": "fn execute() {}\n",
            "artifact": "pub(crate) fn admit_executable_artifact() { ValidatedArtifact::read(); }\n",
            "product": "mod online_machine;\nimpl ProductServer { fn launch() { self.run_online_exchange(); } }\n",
            "machine": "impl ProductServer { fn run_online_exchange() {\n"
                       "    let executable = admit_executable_artifact(&record)?;\n"
                       "    ReferenceMachine::from_binary(executable.payload(), invocation, limits);\n"
                       "    machine.restore_checkpoint(&checkpoint);\n"
                       "} }\n",
        }
        actual = {}
        for key, relative in paths.items():
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(sources[key])
            actual[key] = path
        return actual, sources

    def test_actual_plain_online_owner_satisfies_original_admission_predicates(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            typed_boundaries.check_product_artifact_admission(root)

    def test_original_metadata_admission_and_raw_payload_refusals_remain_live(self):
        cases = [
            ("model", "pub struct ExecutableArtifactMetadata", "pub struct MissingMetadata"),
            ("model", "pub executable: Option<ExecutableArtifactMetadata>", "pub executable: bool"),
            ("artifact", "pub(crate) fn admit_executable_artifact", "fn missing_admission"),
            ("artifact", "ValidatedArtifact::read", "unchecked_payload"),
            ("machine", "admit_executable_artifact(&record)?", "unchecked_payload(&record)?"),
        ]
        for key, old, new in cases:
            with self.subTest(owner=key, refused=old), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                paths, sources = self.fixture(root)
                paths[key].write_text(sources[key].replace(old, new))
                with self.assertRaises(typed_boundaries.BoundaryError):
                    typed_boundaries.check_product_artifact_admission(root)
        for key in ("cobol", "product", "machine"):
            with self.subTest(raw_payload_owner=key), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                paths, sources = self.fixture(root)
                paths[key].write_text(
                    sources[key] + "fn bypass() { ReferenceMachine::from_binary(\n"
                    "                &record.payload, invocation, limits); }\n"
                )
                with self.assertRaises(typed_boundaries.BoundaryError):
                    typed_boundaries.check_product_artifact_admission(root)

    def test_only_an_actual_production_private_owner_can_supply_admission(self):
        for declaration in (
            "// mod online_machine;",
            'const SHADOW: &str = "mod online_machine;";',
            "#[cfg(test)] mod online_machine;",
            "mod nested { mod online_machine; }",
            '#[path = "other.rs"] mod online_machine;',
        ):
            with self.subTest(declaration=declaration), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                paths, _ = self.fixture(root)
                paths["product"].write_text(declaration)
                with self.assertRaises(typed_boundaries.BoundaryError):
                    typed_boundaries.check_product_artifact_admission(root)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths, _ = self.fixture(root)
            paths["machine"].unlink()
            with self.assertRaises(FileNotFoundError):
                typed_boundaries.check_product_artifact_admission(root)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths, _ = self.fixture(root)
            paths["machine"].write_text(
                "fn unchecked() {}\n#[cfg(test)] fn shadow() { admit_executable_artifact(&record)?; }\n"
            )
            with self.assertRaises(typed_boundaries.BoundaryError):
                typed_boundaries.check_product_artifact_admission(root)


    def flat_fixture(self, root):
        paths, sources = self.fixture(root)
        sources["product"] = sources["product"].replace("mod online_machine;\n", "") + sources["machine"]
        paths["product"].write_text(sources["product"])
        paths["machine"].unlink()
        return paths, sources

    def test_genuine_flat_owner_without_child_satisfies_original_admission(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths, _ = self.flat_fixture(root)
            self.assertFalse(paths["machine"].exists())
            typed_boundaries.check_product_artifact_admission(root)

    def test_flat_owner_retains_all_five_admission_and_both_raw_payload_refusals(self):
        cases = [
            ("model", "pub struct ExecutableArtifactMetadata", "pub struct MissingMetadata"),
            ("model", "pub executable: Option<ExecutableArtifactMetadata>", "pub executable: bool"),
            ("artifact", "pub(crate) fn admit_executable_artifact", "fn missing_admission"),
            ("artifact", "ValidatedArtifact::read", "unchecked_payload"),
            ("product", "admit_executable_artifact(&record)?", "unchecked_payload(&record)?"),
        ]
        for key, old, new in cases:
            with self.subTest(owner=key, refused=old), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                paths, sources = self.flat_fixture(root)
                paths[key].write_text(sources[key].replace(old, new))
                with self.assertRaises(typed_boundaries.BoundaryError):
                    typed_boundaries.check_product_artifact_admission(root)
        for key in ("cobol", "product"):
            with self.subTest(raw_payload_owner=key), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                paths, sources = self.flat_fixture(root)
                paths[key].write_text(
                    sources[key] + "fn bypass() { ReferenceMachine::from_binary(\n"
                    "                &record.payload, invocation, limits); }\n"
                )
                with self.assertRaises(typed_boundaries.BoundaryError):
                    typed_boundaries.check_product_artifact_admission(root)

    def test_unlinked_or_shadow_child_cannot_supply_admission(self):
        for declaration in (
            "",
            "/* mod online_machine; */",
            'const SHADOW: &str = r#"mod online_machine;"#;',
            '#[doc = "mod online_machine;"] fn unrelated() {}',
            "#[cfg(test)] mod online_machine;",
            "mod nested { mod online_machine; }",
        ):
            with self.subTest(declaration=declaration), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                paths, sources = self.fixture(root)
                paths["product"].write_text(
                    declaration + "\n" + sources["product"].replace("mod online_machine;\n", "")
                )
                with self.assertRaises(typed_boundaries.BoundaryError):
                    typed_boundaries.check_product_artifact_admission(root)

    def test_declared_nonplain_file_owner_is_refused_even_with_valid_flat_body(self):
        for declaration in (
            '#[path = "other.rs"] mod online_machine;',
            '#[cfg(feature = "shadow")] mod online_machine;',
            "pub mod online_machine;",
            "pub(crate) mod online_machine;",
            "mod online_machine; mod online_machine;",
            'mod online_machine; #[path = "other.rs"] mod online_machine;',
        ):
            with self.subTest(declaration=declaration), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                paths, sources = self.flat_fixture(root)
                paths["product"].write_text(declaration + "\n" + sources["product"])
                # A genuine file declaration cannot fall back to the admitted flat body.
                paths["machine"].write_text(sources["machine"])
                with self.assertRaises(typed_boundaries.BoundaryError):
                    typed_boundaries.check_product_artifact_admission(root)

    def test_declared_missing_file_cannot_fall_back_to_valid_flat_owner(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths, sources = self.flat_fixture(root)
            paths["product"].write_text("mod online_machine;\n" + sources["product"])
            with self.assertRaises(FileNotFoundError):
                typed_boundaries.check_product_artifact_admission(root)


if __name__ == "__main__":
    unittest.main()
