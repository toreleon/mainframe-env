import importlib.util
from pathlib import Path
import sys
import unittest

TOOL = Path(__file__).resolve().parents[1] / 'dataset_mutations.py'
spec = importlib.util.spec_from_file_location('dataset_mutations', TOOL)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


class ClassificationTests(unittest.TestCase):
    def transcript(self, failed=()):
        text = ''.join(f'test {name} ... {"FAILED" if name in failed else "ok"}\n'
                       for name in sorted(module.EXPECTED_TESTS))
        text += ''.join(f"thread '{name}' panicked at source.rs:1:1:\nassertion failed\n" for name in failed)
        text += 'test result: FAILED.\n' if failed else 'test result: ok.\n'
        return text

    def test_unchanged_success_is_surviving_not_killed(self):
        self.assertEqual(module.classify(0, self.transcript(), module.EXPECTED_TESTS), ('survived', []))

    def test_only_actual_assertion_failure_gets_credit(self):
        failing = sorted(module.EXPECTED_TESTS)[1:]
        self.assertEqual(module.classify(101, self.transcript(failing), module.EXPECTED_TESTS), ('killed', failing))

    def test_compile_failure_gets_no_credit(self):
        self.assertEqual(module.classify(101, 'error[E0425]: no such value\ncould not compile', module.EXPECTED_TESTS), ('invalid', []))

    def test_missing_or_empty_test_selection_gets_no_credit(self):
        self.assertEqual(module.classify(0, 'running 0 tests\ntest result: ok.\n', module.EXPECTED_TESTS), ('invalid', []))

    def test_timeout_gets_no_credit(self):
        self.assertEqual(module.classify(None, self.transcript(), module.EXPECTED_TESTS), ('timed_out', []))

    def test_signal_or_inconsistent_exit_gets_no_credit(self):
        self.assertEqual(module.classify(-9, self.transcript(), module.EXPECTED_TESTS), ('invalid', []))

    def test_anchors_are_unique_and_tests_remain_unchanged(self):
        source = (TOOL.parents[1] / module.SOURCE).read_text()
        for mutation in module.MUTATIONS:
            changed = module.apply_mutation(source, mutation)
            self.assertNotEqual(changed, source)
            self.assertEqual(changed.partition('#[cfg(test)]')[2], source.partition('#[cfg(test)]')[2])
            self.assertEqual(module.digest(source.encode()), module.digest((TOOL.parents[1] / module.SOURCE).read_bytes()))

    def test_missing_repeated_and_test_only_anchor_rejected(self):
        mutation = module.Mutation('test', 'test', 'ANCHOR', 'CHANGED')
        for source in ['nothing\n#[cfg(test)]\n', 'ANCHOR ANCHOR\n#[cfg(test)]\n', 'nothing\n#[cfg(test)]\nANCHOR']:
            with self.assertRaises(ValueError):
                module.apply_mutation(source, mutation)

    def test_production_only_handler_can_be_mutated_without_a_test_suffix(self):
        mutation = module.Mutation('test', 'test', 'ANCHOR', 'CHANGED')
        self.assertEqual(module.apply_mutation('before ANCHOR after', mutation), 'before CHANGED after')

    def test_cics_product_mutation_anchors_are_unique_and_scenarios_are_unchanged(self):
        scenarios = (TOOL.parents[1] / module.CICS_SCENARIOS).read_bytes()
        for mutation in module.CICS_MUTATIONS:
            source = (TOOL.parents[1] / mutation.source).read_text()
            changed = module.apply_mutation(source, mutation)
            self.assertNotEqual(changed, source)
            self.assertEqual(
                changed.partition('#[cfg(test)]')[2],
                source.partition('#[cfg(test)]')[2],
            )
            self.assertEqual(
                module.digest(scenarios),
                module.digest((TOOL.parents[1] / module.CICS_SCENARIOS).read_bytes()),
            )

    def test_cobol_move_product_mutation_anchor_is_unique_and_scenario_is_unchanged(self):
        source = (TOOL.parents[1] / module.COBOL_MOVE_SOURCE).read_text()
        scenarios = (TOOL.parents[1] / module.COBOL_MOVE_SCENARIOS).read_bytes()
        for mutation in module.COBOL_MOVE_MUTATIONS:
            changed = module.apply_mutation(source, mutation)
            self.assertNotEqual(changed, source)
            self.assertEqual(
                changed.partition('#[cfg(test)]')[2],
                source.partition('#[cfg(test)]')[2],
            )
            self.assertEqual(
                module.digest(scenarios),
                module.digest((TOOL.parents[1] / module.COBOL_MOVE_SCENARIOS).read_bytes()),
            )

    def test_typed_arithmetic_product_mutation_anchors_are_unique_and_scenario_is_unchanged(self):
        source = (TOOL.parents[1] / module.TYPED_ARITHMETIC_SOURCE).read_text()
        tests = source.partition('#[cfg(test)]')[2]
        for mutation in module.TYPED_ARITHMETIC_MUTATIONS:
            changed = module.apply_mutation(source, mutation)
            self.assertNotEqual(changed, source)
            self.assertEqual(
                changed.partition('#[cfg(test)]')[2],
                tests,
            )
            self.assertEqual(
                module.digest(tests.encode()),
                module.digest(
                    (TOOL.parents[1] / module.TYPED_ARITHMETIC_SOURCE)
                    .read_text()
                    .partition('#[cfg(test)]')[2]
                    .encode()
                ),
            )

    def test_corresponding_product_mutation_anchors_are_unique_and_tests_are_unchanged(self):
        source = (TOOL.parents[1] / module.COBOL_CORRESPONDING_SOURCE).read_text()
        tests = source.partition('#[cfg(test)]')[2]
        for mutation in module.COBOL_CORRESPONDING_MUTATIONS:
            changed = module.apply_mutation(source, mutation)
            self.assertNotEqual(changed, source)
            self.assertEqual(changed.partition('#[cfg(test)]')[2], tests)

    def test_selected_product_tests_match_the_recorded_expected_sets(self):
        arithmetic = (TOOL.parents[1] / module.TYPED_ARITHMETIC_SOURCE).read_text()
        arithmetic_names = {
            'machine::typed_decimal::tests::' + name
            for name in module.re.findall(
                r'#\[test\]\s*fn\s+([A-Za-z0-9_]+)',
                arithmetic.partition('#[cfg(test)]')[2],
            )
        }
        self.assertEqual(arithmetic_names, module.TYPED_ARITHMETIC_EXPECTED_TESTS)

        corresponding = (TOOL.parents[1] / module.COBOL_CORRESPONDING_SCENARIOS).read_text()
        corresponding_names = {
            'framework::tests::' + name
            for name in module.re.findall(
                r'#\[test\]\s*fn\s+(add_corresponding_[A-Za-z0-9_]+)',
                corresponding.partition('#[cfg(test)]')[2],
            )
        }
        self.assertEqual(
            corresponding_names,
            module.COBOL_CORRESPONDING_EXPECTED_TESTS,
        )

    def test_v4_inventory_replaces_the_obsolete_atomic_batch_mutant(self):
        self.assertEqual(module.SCHEMA_VERSION, 'mainframe-env.source-mutations@4')
        arithmetic_ids = {mutation.identity for mutation in module.TYPED_ARITHMETIC_MUTATIONS}
        self.assertEqual(len(arithmetic_ids), 7)
        self.assertNotIn('typed-decimal-write-before-batch-validates', arithmetic_ids)
        self.assertEqual(len(module.COBOL_CORRESPONDING_MUTATIONS), 3)
        self.assertEqual(
            sum(
                len(mutations)
                for mutations in (
                    module.MUTATIONS,
                    module.CICS_MUTATIONS,
                    module.COBOL_MOVE_MUTATIONS,
                    module.TYPED_ARITHMETIC_MUTATIONS,
                    module.COBOL_CORRESPONDING_MUTATIONS,
                )
            ),
            20,
        )


if __name__ == '__main__':
    unittest.main()
