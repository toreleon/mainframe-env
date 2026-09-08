import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

TOOL = Path(__file__).resolve().parents[1] / 'jenkins' / 'disk_guard.py'
spec = importlib.util.spec_from_file_location('jenkins_disk_guard', TOOL)
guard = importlib.util.module_from_spec(spec); spec.loader.exec_module(guard)


class DiskGuardTests(unittest.TestCase):
    def test_verify_accepts_only_paths_on_and_below_capped_root(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            home = root / 'jenkins-home'; home.mkdir()
            cargo = root / 'cargo-home'
            usage = {'total_bytes': guard.HARD_LIMIT_BYTES, 'used_bytes': 1,
                     'free_bytes': guard.DEFAULT_MINIMUM_FREE_BYTES,
                     'used_percent': 1.0}
            with patch.object(guard, 'filesystem_usage', return_value=usage):
                result = guard.verify(root, [f'JENKINS_HOME={home}', f'CARGO_HOME={cargo}'], 0)
                self.assertEqual(result['hard_limit_bytes'], guard.HARD_LIMIT_BYTES)
                with self.assertRaises(ValueError):
                    guard.verify(root, ['WORKSPACE=/outside-the-capped-volume'], 0)

    def test_verify_rejects_a_large_filesystem_even_when_it_is_empty(self):
        with tempfile.TemporaryDirectory() as directory:
            usage = {'total_bytes': guard.HARD_LIMIT_BYTES + 1, 'used_bytes': 0,
                     'free_bytes': guard.HARD_LIMIT_BYTES + 1,
                     'used_percent': 0.0}
            with patch.object(guard, 'filesystem_usage', return_value=usage):
                with self.assertRaisesRegex(ValueError, 'reports .* bytes of capacity'):
                    guard.verify(Path(directory), [], 0)

    def test_prune_removes_only_known_cargo_caches_above_threshold(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve(); cargo = root / 'cargo-home'
            cache = cargo / 'registry' / 'cache'; cache.mkdir(parents=True)
            source = cargo / 'registry' / 'src'; source.mkdir(parents=True)
            keep = cargo / 'credentials.toml'; keep.write_text('keep')
            before = {'total_bytes': 100, 'used_bytes': 80, 'free_bytes': 20,
                      'used_percent': 80.0}
            after = {'total_bytes': 100, 'used_bytes': 10, 'free_bytes': 90,
                     'used_percent': 10.0}
            with patch.object(guard, 'filesystem_usage', side_effect=[before, after]):
                result = guard.prune(root, cargo, 75)
            self.assertFalse(cache.exists()); self.assertFalse(source.exists())
            self.assertTrue(keep.exists()); self.assertEqual(len(result['removed']), 2)


if __name__ == '__main__':
    unittest.main()
