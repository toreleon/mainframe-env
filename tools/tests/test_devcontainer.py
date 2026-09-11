"""Configuration contracts for the capped VS Code development workspace."""

import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class DevContainerTests(unittest.TestCase):
    def setUp(self):
        self.config = json.loads((ROOT / '.devcontainer/devcontainer.json').read_text())
        self.compose = (ROOT / '.devcontainer/compose.yaml').read_text()
        self.wrapper = (ROOT / 'docker/dev').read_text()
        self.entrypoint = (ROOT / 'docker/entrypoint.sh').read_text()
        self.cargo = (ROOT / 'docker/dev-bin/cargo').read_text()
        self.codex_config = (ROOT / 'docker/codex-container.toml').read_text()
        self.dockerfile = (ROOT / 'docker/toolchain.Dockerfile').read_text()

    def test_reuses_compose_services_and_never_stops_the_shared_stack_on_close(self):
        self.assertEqual(self.config['dockerComposeFile'],
                         ['../docker/compose.yaml', 'compose.yaml'])
        self.assertEqual(self.config['service'], 'dev')
        self.assertEqual(self.config['runServices'], ['postgres'])
        self.assertEqual(self.config['workspaceFolder'], '/workspace')
        self.assertEqual(self.config['shutdownAction'], 'none')
        self.assertFalse(self.config['overrideCommand'])

    def test_remote_user_and_tooling_are_explicit(self):
        self.assertEqual(self.config['remoteUser'], 'developer')
        self.assertFalse(self.config['updateRemoteUserUID'])
        remote_path = self.config['remoteEnv']['PATH']
        self.assertTrue(remote_path.startswith('/opt/mainframe-env/docker/dev-bin:'))
        settings = self.config['customizations']['vscode']['settings']
        self.assertFalse(settings['rust-analyzer.checkOnSave'])
        self.assertIn('vadimcn.vscode-lldb',
                      self.config['customizations']['vscode']['extensions'])
        self.assertIn('openai.chatgpt',
                      self.config['customizations']['vscode']['extensions'])
        self.assertTrue(settings['chatgpt.openOnStartup'])

    def test_compose_keeps_limits_cache_and_no_docker_socket(self):
        self.assertIn("MAINFRAME_ENV_DEV_CONTAINER: '1'", self.compose)
        self.assertIn('CODEX_HOME: /home/developer/.codex', self.compose)
        self.assertIn('dev-vscode-home:/home/developer', self.compose)
        self.assertIn('SYS_PTRACE', self.compose)
        self.assertNotIn('/var/run/docker.sock', self.compose)
        self.assertNotIn('/Users/', self.compose)
        base = (ROOT / 'docker/compose.yaml').read_text()
        for required in ('mem_limit: 6g', 'cpus: 3', 'pids_limit: 512',
                         'dev-cargo:/cache', 'dev-target:/target',
                         'ibm-docs:/ibm-docs'):
            self.assertIn(required, base)

    def test_codex_uses_docker_as_the_single_sandbox(self):
        self.assertIn('sandbox_mode = "danger-full-access"', self.codex_config)
        self.assertIn('approval_policy = "never"', self.codex_config)
        self.assertIn(
            'COPY docker/codex-container.toml /etc/codex/config.toml',
            self.dockerfile,
        )
        self.assertIn('mkdir -p "$dev_home/.codex"', self.entrypoint)

    def test_initializer_requires_the_exact_capped_socket(self):
        self.assertEqual(self.config['initializeCommand'],
                         '${localWorkspaceFolder}/docker/dev devcontainer-init')
        self.assertIn('incoming_docker_host="${DOCKER_HOST:-}"', self.wrapper)
        self.assertIn('[[ "$incoming_docker_host" == "$docker_socket" ]]', self.wrapper)
        self.assertIn('exec code --new-window "$repo"', self.wrapper)
        vscode = self.wrapper.split('  vscode)', 1)[1].split('  devcontainer-init)', 1)[0]
        self.assertNotIn('start_stack', vscode)
        self.assertIn('up -d --no-build --wait', vscode)

    def test_long_lived_entrypoint_releases_lock_and_cargo_reacquires_it(self):
        branch = self.entrypoint.split(
            'if [[ "${MAINFRAME_ENV_DEV_CONTAINER:-0}" == 1 ]]; then', 2
        )[-1].split('else', 1)[0]
        self.assertIn('flock -u 9', branch)
        self.assertIn('usermod --uid "$dev_uid" --gid "$dev_gid" developer',
                      self.entrypoint)
        self.assertIn('flock /cache/build.lock', self.cargo)
        self.assertIn('MAINFRAME_ENV_CARGO_LOCK_HELD', self.cargo)
        self.assertIn('/usr/local/cargo/bin/cargo', self.cargo)


if __name__ == '__main__':
    unittest.main()
