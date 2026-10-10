"""Differential controls for PGID tokenization; no real process census or launch."""
import ast
from contextlib import contextmanager
import importlib.util
from pathlib import Path
import random
from types import SimpleNamespace
import unittest
from unittest.mock import patch

TOOL = Path(__file__).resolve().parents[1] / 'ci_assurance.py'
spec = importlib.util.spec_from_file_location('ci_stat_parser_controls', TOOL)
ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci)


def reference(raw):
    # Preserved full-tokenization behavior before the bounded allocation change.
    return int(raw.rsplit(b')', 1)[1].split()[2])


def production_expression():
    tree = ast.parse(TOOL.read_bytes())
    function = next(node for node in tree.body if isinstance(node, ast.FunctionDef)
                    and node.name == '_linux_group_has_members')
    assignments = [node for node in ast.walk(function) if isinstance(node, ast.Assign)
                   and any(isinstance(target, ast.Name) and target.id == 'group'
                           for target in node.targets)]
    if len(assignments) != 1:
        raise AssertionError('actual scanner has no unique PGID assignment')
    return compile(ast.Expression(assignments[0].value), str(TOOL), 'eval')


def outcome(function, raw):
    try:
        return ('value', function(raw))
    except (IndexError, ValueError) as error:
        return ('error', type(error).__name__, str(error))


class LinuxStatParserDifferentialTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        code = production_expression()
        cls.parse = staticmethod(lambda raw: eval(code, {'raw': raw}))

    def equal(self, raw):
        self.assertEqual(outcome(self.parse, raw), outcome(reference, raw))

    def test_comm_parentheses_all_ascii_whitespace_and_integer_forms(self):
        whitespace = [b' ', b'\t', b'\n', b'\r', b'\v', b'\f', b' \t\r\n']
        for separator in whitespace:
            for group in [b'987654321', b'1', b'0', b'-2', b'+3', b'0004', b'1_000']:
                for comm in [b'name', b'a ) name', b') ( nested )']:
                    self.equal(b'42 (' + comm + b')' + separator + b'Z' + separator
                               + b'1' + separator + group + separator + b'0' * 4096)

    def test_short_malformed_group_and_ignored_tail_match_prior_behavior(self):
        for raw in [b'', b')', b') S', b') S 1', b'42 (unterminated S 1 1',
                    b'42 (a) S 1 not-a-number 0', b'42 (a) S 1 12\x00 0',
                    b'42 (a) S 1 1 2 3) ignored last closing delimiter',
                    b'42 (a) S 1 1 \x00\xff malformed unparsed tail',
                    b'42 (a) S 1 1' + b' ' * 131072]:
            self.equal(raw)

    def test_deterministic_arbitrary_bytes_and_tokenized_tails_preserve_results(self):
        generator = random.Random(0xC175)
        alphabet = b' \t\n\r\v\f0123456789_+-AZ()\x00\xff'
        for index in range(8192):
            tail = bytes(generator.choice(alphabet) for _ in range(generator.randrange(0, 192)))
            raw = tail if index % 2 else b'42 (a ) name) S 1 ' + tail
            self.equal(raw)

    def scan(self, raw, pgid=987654321):
        @contextmanager
        def entries(path):
            self.assertEqual(path, '/proc')
            yield iter([SimpleNamespace(name='42', path='/proc/42')])
        with patch.object(ci.os, 'scandir', entries), \
                patch.object(ci, '_linux_stat_bytes', return_value=raw) as read:
            result = ci._linux_group_has_members(pgid)
        read.assert_called_once_with('/proc/42/stat')
        return result

    def test_actual_scanner_preserves_member_zombie_and_complete_negative_results(self):
        for state in [b'S', b'R', b'Z']:
            for group, expected in [(987654321, True), (1, False), (-2, False)]:
                raw = b'42 (a ) name) ' + state + b' 1 ' + str(group).encode() + b' 0 ' * 65536
                self.assertEqual(self.scan(raw), expected)

    def test_actual_scanner_malformed_group_cannot_earn_negative_result(self):
        for raw in [b'', b'42 (a) S 1', b'42 (a) S 1 bad 0', b'42 (a) S 1 12\x00 0']:
            with self.subTest(raw=raw), self.assertRaisesRegex(OSError, '^invalid Linux process-group metadata$'):
                self.scan(raw)


if __name__ == '__main__':
    unittest.main()
