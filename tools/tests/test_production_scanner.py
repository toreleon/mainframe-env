"""Independent production/test boundary fixtures for the assurance scanner."""

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "production_scanner", ROOT / "tools/check_typed_semantic_boundaries.py"
)
scanner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(scanner)


class ProductionScannerTests(unittest.TestCase):
    def test_inline_test_helper_cannot_hide_later_application_dispatch(self):
        source = '''impl Dispatcher {
    #[ cfg ( test ) ]
    fn fixture() { let identity = "CARDDEMO"; }
    fn execute(program: &str) -> bool { program == "COBTUPDT" }
}
'''
        production = scanner.production(source)
        self.assertNotIn("CARDDEMO", production)
        self.assertIn('program == "COBTUPDT"', production)
        with self.assertRaisesRegex(scanner.BoundaryError, "COBTUPDT"):
            scanner.reject(production, ["COBTUPDT"], "production application hardcode scan")

    def test_inner_test_attribute_excludes_only_its_module(self):
        source = '''mod fixtures {
    #![cfg(test)]
    const APPLICATION: &str = "CARDDEMO";
}
fn execute(program: &str) -> bool { program == "COBTUPDT" }
'''
        production = scanner.production(source)
        self.assertNotIn("CARDDEMO", production)
        self.assertIn('program == "COBTUPDT"', production)

    def test_exact_inline_attribute_keeps_later_dispatch(self):
        source = '''impl Dispatcher {
    #[cfg(test)] fn fixture() { let identity = "CARDDEMO"; }
    fn execute(program: &str) -> bool { program == "COBTUPDT" }
}'''
        production = scanner.production(source)
        self.assertNotIn("CARDDEMO", production)
        self.assertIn('program == "COBTUPDT"', production)

    def test_markers_in_literals_and_comments_preserve_the_entire_source(self):
        markers = [
            'const TEXT: &str = "#[cfg(test)] \\" quoted";',
            'const RAW: &str = r###"#![cfg(test)] "# }"###;',
            'const BYTES: &[u8] = br##"#[cfg(test)]"##;',
            '// #[cfg(test)]\n',
            '/* outer /* #![cfg(test)] */ still a comment */',
            '#[doc = "#[cfg(test)]"]',
        ]
        for marker in markers:
            with self.subTest(marker=marker):
                source = marker + '\nfn execute() { dispatch("COBTUPDT"); }\n'
                self.assertEqual(scanner.production(source), source)

    def test_nested_modules_keep_production_siblings_and_literal_identities(self):
        source = '''mod outer {
    #[cfg(test)] mod fixtures { fn helper() { dispatch("CARDDEMO"); } }
    mod inner {
        #[cfg(test)] fn fixture() { dispatch("PAUDBLOD"); }
        fn execute() { dispatch("COBTUPDT"); }
    }
    fn last() { dispatch("PSBPAUTB"); }
}'''
        production = scanner.production(source)
        self.assertNotIn("CARDDEMO", production)
        self.assertNotIn("PAUDBLOD", production)
        self.assertIn('dispatch("COBTUPDT")', production)
        self.assertIn('dispatch("PSBPAUTB")', production)

    def test_test_variant_cannot_remove_preceding_or_following_production(self):
        source = '''enum Routing {
    Production = 1,
    #[cfg(test)] Fixture,
}
fn execute() { dispatch("COBTUPDT"); }
'''
        production = scanner.production(source)
        self.assertIn("Production = 1", production)
        self.assertNotIn("Fixture", production)
        self.assertIn('dispatch("COBTUPDT")', production)

    def test_generic_test_function_keeps_later_dispatch(self):
        source = '''#[cfg(test)] fn fixture<A, B>(a: A, b: B) { dispatch("CARDDEMO"); }
fn execute() { dispatch("COBTUPDT"); }
'''
        production = scanner.production(source)
        self.assertNotIn("CARDDEMO", production)
        self.assertIn('dispatch("COBTUPDT")', production)

    def test_stacked_attributes_comments_and_raw_test_strings(self):
        source = '''#[doc = r##"attributes {;} #[cfg(test)]"##]
#[cfg(/* nested /* comment */ */ test)]
#[inline]
pub(crate) fn fixture() { let value = br###"}; /* #[cfg(test)] */"###; }
fn execute() { dispatch("COBTUPDT"); }
'''
        production = scanner.production(source)
        self.assertNotIn("fixture", production)
        self.assertNotIn("#[doc", production)
        self.assertNotIn("#[inline]", production)
        self.assertIn('dispatch("COBTUPDT")', production)

    def test_visibility_and_braced_initializers_stop_at_their_semicolon(self):
        source = '''#[cfg(test)] pub(crate) const FIXTURE: &str = { "CARDDEMO" };
#[cfg(test)] pub static SAMPLE: &str = { "PAUDBLOD" };
#[cfg(test)] pub type Sample = [u8; { 2 }];
#[cfg(test)] pub use fixtures::{First, Second};
#[cfg(test)] mod external_tests;
fn execute() { dispatch("COBTUPDT"); }
'''
        production = scanner.production(source)
        self.assertNotIn("CARDDEMO", production)
        self.assertNotIn("PAUDBLOD", production)
        self.assertNotIn("Sample", production)
        self.assertNotIn("fixtures::", production)
        self.assertNotIn("external_tests", production)
        self.assertIn('dispatch("COBTUPDT")', production)

    def test_characters_and_lifetimes_do_not_change_item_boundaries(self):
        source = r'''#[cfg(test)] fn fixture<'a>(input: &'a str) {
    let chars = ('\u{7d}', '\x7b', '\'', '}');
    dispatch("CARDDEMO");
}
fn execute<'a>(program: &'a str) -> bool { program == "COBTUPDT" }
'''
        production = scanner.production(source)
        self.assertNotIn("CARDDEMO", production)
        self.assertIn('program == "COBTUPDT"', production)

    def test_non_test_and_conditional_test_attributes_remain_visible(self):
        for attribute in [
            "#[cfg(not(test))]",
            '#[cfg(any(test, feature = "runtime"))]',
            "#[cfg_attr(test, inline)]",
            '#[doc = "#[cfg(test)]"]',
        ]:
            with self.subTest(attribute=attribute):
                source = attribute + '\nfn execute() { dispatch("COBTUPDT"); }'
                self.assertEqual(scanner.production(source), source)

    def test_file_level_inner_attribute_excludes_the_whole_file(self):
        source = '''//! Unit test fixtures.
#![cfg(test)]
const APPLICATION: &str = "CARDDEMO COBTUPDT";
'''
        production = scanner.production(source)
        self.assertNotIn("CARDDEMO", production)
        self.assertNotIn("COBTUPDT", production)

    def test_exclusion_keeps_line_and_character_coordinates(self):
        source = '''fn before() {}
#[cfg(test)] fn fixture() { dispatch("CARDDEMO"); }
fn execute() { dispatch("COBTUPDT"); }
'''
        production = scanner.production(source)
        self.assertEqual(len(production), len(source))
        self.assertEqual(production.count("\n"), source.count("\n"))
        self.assertEqual(production.index("COBTUPDT"), source.index("COBTUPDT"))

    def test_malformed_source_fails_closed(self):
        for source in [
            "#[cfg(test)] fn fixture() {",
            "#[cfg(test)]",
            'const TEXT: &str = "unterminated',
            'const TEXT: &str = r##"unterminated',
            "/* unterminated",
        ]:
            with self.subTest(source=source):
                with self.assertRaises(scanner.BoundaryError):
                    scanner.production(source)

    def test_malformed_delimiters_fail_closed_before_test_exclusion(self):
        for source in [
            "fn execute() {",
            "fn execute(] {}",
            "fn execute() { let value = [1, 2); }",
            "#[cfg(test)] fn fixture() { let value = [1, 2); }\n"
            'fn execute() { dispatch("COBTUPDT"); }',
            "fn execute() {} }",
        ]:
            with self.subTest(source=source):
                with self.assertRaisesRegex(scanner.BoundaryError, "Rust delimiter"):
                    scanner.production(source)

    def test_delimiter_validation_preserves_macro_tokens_and_literal_forms(self):
        source = r'''macro_rules! forward {
    ($value:expr) => { consume!([$value]); };
}
fn execute<'a>(program: &'a str) {
    let text = "{[(}]}";
    let bytes = b"[(}";
    let cstring = c"}])";
    let raw = r##"{[)\""##;
    let raw_bytes = br##"{[)\""##;
    let raw_cstring = cr##"{[)\""##;
    let chars = ('}', b']', '\u{7b}', '\x5b', '\'');
    /* ([{ /* ]}) */ */
    // )]}
    forward!(program);
}
'''
        self.assertEqual(scanner.production(source), source)

    def test_batch_cli_preserves_order_and_production_dispatch(self):
        with tempfile.TemporaryDirectory() as temporary:
            paths = [Path(temporary) / "first.rs", Path(temporary) / "second.rs"]
            paths[0].write_text(
                '#[cfg(test)] fn fixture() { dispatch("CARDDEMO"); }\n'
                'fn execute() { dispatch("COBTUPDT"); }', encoding="utf-8"
            )
            paths[1].write_text('fn second() {}', encoding="utf-8")
            result = subprocess.run(
                [sys.executable, "-B", str(ROOT / "tools/check_typed_semantic_boundaries.py"),
                 "--production-files"],
                input=json.dumps([str(path) for path in paths]),
                text=True, capture_output=True, check=True,
            )
            sources = json.loads(result.stdout)
            self.assertEqual(len(sources), 2)
            self.assertNotIn("CARDDEMO", sources[0])
            with self.assertRaisesRegex(scanner.BoundaryError, "COBTUPDT"):
                scanner.reject(sources[0], ["COBTUPDT"], "production application hardcode scan")
            self.assertEqual(sources[1], "fn second() {}")

    def test_batch_cli_refuses_missing_files_and_invalid_input(self):
        for paths in [["/missing-production-scanner-fixture.rs"], {}, [None]]:
            with self.subTest(paths=paths):
                result = subprocess.run(
                    [sys.executable, "-B", str(ROOT / "tools/check_typed_semantic_boundaries.py"),
                     "--production-files"],
                    input=json.dumps(paths), text=True, capture_output=True,
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
