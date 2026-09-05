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


if __name__ == '__main__':
    unittest.main()
