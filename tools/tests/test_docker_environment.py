import importlib.util
import json
import os
from pathlib import Path
import tempfile
import types
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]


def load(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'docker' / f'{name}.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


storage = load('storage')
deploy = load('deploy')


class DockerStorageTests(unittest.TestCase):
    def test_development_tmpdir_is_executable_target_storage(self):
        compose = (ROOT / 'docker' / 'compose.yaml').read_text()
        self.assertIn('TMPDIR: /target', compose)
        self.assertNotIn('TMPDIR: /tmp', compose)

    def test_rejects_large_filesystem_even_when_almost_empty(self):
        stats = types.SimpleNamespace(f_blocks=100_000_000_000, f_frsize=1,
                                      f_bavail=99_000_000_000)
        with tempfile.TemporaryDirectory() as temporary, patch.object(storage.os, 'statvfs', return_value=stats):
            with self.assertRaisesRegex(ValueError, 'capacity'):
                storage.usage(Path(temporary))

    def test_low_space_prevents_builds_but_allows_recovery_checks(self):
        stats = types.SimpleNamespace(f_blocks=32 * 1024**3, f_frsize=1,
                                      f_bavail=1024**3)
        with tempfile.TemporaryDirectory() as temporary, patch.object(storage.os, 'statvfs', return_value=stats):
            with self.assertRaisesRegex(ValueError, 'Reclaim caches'):
                storage.usage(Path(temporary))
            self.assertEqual(storage.usage(Path(temporary), reserve=0)['free_bytes'], 1024**3)

    def test_secret_initialization_preserves_passwords_and_rejects_symlinks(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            storage.create_secrets(root)
            password = (root / 'jenkins_password').read_bytes()
            storage.create_secrets(root)
            self.assertEqual((root / 'jenkins_password').read_bytes(), password)
            self.assertEqual((root / 'jenkins_password').stat().st_mode & 0o777, 0o600)
            (root / 'jenkins_password').unlink()
            (root / 'jenkins_password').symlink_to(root / 'admin_password')
            with self.assertRaisesRegex(ValueError, 'unsafe existing secret'):
                storage.create_secrets(root)

    def test_cleanup_rejects_host_or_persistent_data_paths(self):
        for path in ('/', '/state', '/releases', '/var/lib/postgresql', '/workspace'):
            with self.assertRaises(ValueError):
                storage.clean(Path('/cache'), Path(path))


class DockerDeploymentTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.binary = self.root / 'mainframe-env-server'
        self.binary.write_bytes(b'tested-binary')
        self.sha = 'a' * 40
        self.previous = 'b' * 40 + '-1'
        for target, name, value in ((deploy, 'ROOT', self.root), (deploy, 'BINARY', self.binary)):
            patcher = patch.object(target, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)
        environment = patch.dict(os.environ, MAINFRAME_ENV_CI_BRANCH='main', BUILD_NUMBER='2')
        environment.start()
        self.addCleanup(environment.stop)

    def setup_previous(self):
        (self.root / self.previous).mkdir()
        deploy.point(self.root, 'current', self.previous)
        deploy.point(self.root, 'last-good', self.previous)

    def command(self, *args):
        return self.sha if args[0] == 'git' else ''

    def test_success_promotes_exact_bytes_and_retains_previous(self):
        self.setup_previous()
        stale = self.root / ('c' * 40 + '-0')
        stale.mkdir()
        with patch.object(deploy, 'command', side_effect=self.command), patch.object(deploy, 'wait_healthy'):
            receipt = deploy.deploy()
        self.assertTrue(receipt['ready'])
        self.assertEqual((self.root / 'current' / self.binary.name).read_bytes(), b'tested-binary')
        self.assertEqual(os.readlink(self.root / 'previous'), self.previous)
        self.assertFalse(stale.exists())

    def test_unhealthy_candidate_restores_and_verifies_previous(self):
        self.setup_previous()
        with patch.object(deploy, 'command', side_effect=self.command), patch.object(
            deploy, 'wait_healthy', side_effect=[RuntimeError('unhealthy'), None]
        ) as readiness:
            with self.assertRaisesRegex(RuntimeError, 'unhealthy'):
                deploy.deploy()
        self.assertEqual(os.readlink(self.root / 'current'), self.previous)
        self.assertEqual(readiness.call_count, 2)
        self.assertFalse((self.root / (self.sha + '-2')).exists())

    def test_failed_initial_deploy_stops_service_and_removes_pointer(self):
        with patch.object(deploy, 'command', side_effect=self.command) as commands, patch.object(
            deploy, 'wait_healthy', side_effect=RuntimeError('unhealthy')
        ):
            with self.assertRaisesRegex(RuntimeError, 'unhealthy'):
                deploy.deploy()
        commands.assert_any_call('docker', 'stop', '--time', '30', deploy.CONTAINER)
        self.assertFalse((self.root / 'current').is_symlink())

    def test_interrupted_candidate_does_not_replace_last_known_good_fallback(self):
        self.setup_previous()
        interrupted = 'd' * 40 + '-3'
        (self.root / interrupted).mkdir()
        deploy.point(self.root, 'current', interrupted)
        with patch.object(deploy, 'command', side_effect=self.command), patch.object(
            deploy, 'wait_healthy', side_effect=[RuntimeError('unhealthy'), None]
        ):
            with self.assertRaisesRegex(RuntimeError, 'unhealthy'):
                deploy.deploy()
        self.assertEqual(os.readlink(self.root / 'current'), self.previous)

    def test_branch_label_alone_cannot_authorize_other_commit(self):
        with patch.object(deploy, 'command', side_effect=[self.sha, 'd' * 40]):
            with self.assertRaisesRegex(ValueError, 'origin/main'):
                deploy.deploy()
        self.assertFalse((self.root / 'current').is_symlink())

    def test_pointer_rejects_directory_escape(self):
        with self.assertRaises(ValueError):
            deploy.point(self.root, 'current', '../../outside')


if __name__ == '__main__':
    unittest.main()
