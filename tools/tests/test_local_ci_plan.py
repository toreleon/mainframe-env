"""Real Git histories exercise local CI selection without running expensive gates."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('local_ci_plan', ROOT / 'docker/ci_plan.py')
local = importlib.util.module_from_spec(spec)
spec.loader.exec_module(local)


class LocalPlanTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.git('init', '-q')
        self.git('config', 'user.name', 'fixture')
        self.git('config', 'user.email', 'fixture@example.invalid')
        registry = self.root / 'docs/documentation-registry.json'
        registry.parent.mkdir()
        registry.write_text(json.dumps({'normative_documents': [
            'docs/CHARTER.md', 'docs/delivery/VERIFICATION-STRATEGY.md',
        ]}))
        self.base = self.commit('README.md', 'initial')

    def git(self, *args):
        return subprocess.check_output(['git', *args], cwd=self.root, text=True,
                                       stderr=subprocess.DEVNULL).strip()

    def commit(self, path, body='changed'):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(body)
        self.git('add', '-A')
        self.git('commit', '-qm', 'fixture')
        return self.git('rev-parse', 'HEAD')

    def assert_runtime(self, plan):
        self.assertTrue(plan['build'])
        self.assertTrue(plan['msrv'])
        self.assertTrue(plan['store'])
        self.assertEqual(plan['primary_gates'], local.POLICY + local.RUNTIME)
        self.assertFalse(plan['full'])
        self.assertEqual(plan['licensed_credit'], 0)
        local.validate(self.root, plan)

    def test_prose_selects_policy_docs_and_tooling_without_runtime_or_deploy(self):
        self.commit('docs/runbooks/example.md')
        plan = local.plan(self.root, 'auto', self.base)
        self.assertFalse(plan['build'])
        self.assertFalse(plan['msrv'])
        self.assertFalse(plan['store'])
        self.assertEqual(plan['primary_gates'], local.POLICY)
        self.assertIn('cargo-deny', plan['primary_gates'])
        self.assertIn('license-notices', plan['primary_gates'])
        local.validate(self.root, plan)

    def test_no_changes_can_skip_runtime_but_not_policy(self):
        plan = local.plan(self.root, 'auto', self.base)
        self.assertEqual(plan['paths'], [])
        self.assertFalse(plan['build'])
        self.assertEqual(plan['primary_gates'], local.POLICY)

    def test_runtime_mode_keeps_every_local_gate_for_prose(self):
        self.commit('README.md', 'prose')
        self.assert_runtime(local.plan(self.root, 'runtime', self.base))

    def test_missing_or_unavailable_success_cannot_narrow_checks(self):
        self.commit('README.md', 'prose')
        for base in (None, '1' * 40):
            with self.subTest(base=base):
                self.assert_runtime(local.plan(self.root, 'auto', base))

    def test_nonancestor_success_cannot_narrow_checks(self):
        other = self.commit('README.md', 'other history')
        self.git('checkout', '-q', '--detach', self.base)
        self.commit('docs/runbooks/new.md')
        self.assert_runtime(local.plan(self.root, 'auto', other))

    def test_invalid_success_reference_is_rejected(self):
        for base in ('main', '--help', '0' * 40):
            with self.assertRaises(ValueError):
                local.plan(self.root, 'auto', base)

    def test_failed_runtime_commit_is_still_selected_after_prose_commit(self):
        self.commit('crates/apps/mainframe-env-server/src/main.rs', 'failing runtime')
        self.commit('README.md', 'prose after failure')
        plan = local.plan(self.root, 'auto', self.base)
        self.assertIn('crates/apps/mainframe-env-server/src/main.rs', plan['paths'])
        self.assert_runtime(plan)

    def test_normative_prose_and_unknown_inputs_require_runtime(self):
        for path in ('docs/contracts/ABI.md', 'docs/decisions/0010.md',
                     'docs/architecture/RUNTIME.md', 'docs/compatibility/COBOL.md',
                     'docs/releases/record.md', 'docker/ci.sh', 'new/input.xyz'):
            with self.subTest(path=path):
                base = self.git('rev-parse', 'HEAD')
                self.commit(path)
                self.assert_runtime(local.plan(self.root, 'auto', base))

    def test_deleted_code_is_not_hidden_by_added_prose(self):
        base = self.commit('crates/providers/provider/src/lib.rs')
        (self.root / 'crates/providers/provider/src/lib.rs').unlink()
        self.commit('docs/runbooks/removed.md')
        self.assert_runtime(local.plan(self.root, 'auto', base))

    def test_registered_normative_document_outside_architecture_requires_runtime(self):
        self.commit('docs/delivery/VERIFICATION-STRATEGY.md')
        self.assert_runtime(local.plan(self.root, 'auto', self.base))

    def test_registry_changes_cannot_downgrade_normative_checks(self):
        self.commit('docs/documentation-registry.json', json.dumps({
            'normative_documents': ['docs/OTHER.md'],
        }))
        self.commit('docs/delivery/VERIFICATION-STRATEGY.md')
        self.assert_runtime(local.plan(self.root, 'auto', self.base))

    def test_missing_or_malformed_normative_registry_cannot_narrow_checks(self):
        registry = self.root / 'docs/documentation-registry.json'
        registry.unlink()
        self.commit('README.md', 'missing registry')
        self.assert_runtime(local.plan(self.root, 'auto', self.base))
        base = self.commit('docs/documentation-registry.json', '{}')
        self.commit('README.md', 'malformed unchanged registry')
        self.assert_runtime(local.plan(self.root, 'auto', base))

    def test_plan_cannot_be_reused_for_another_candidate_or_rewritten_to_skip(self):
        self.commit('crates/apps/server/src/main.rs')
        plan = local.plan(self.root, 'auto', self.base)
        modified = copy.deepcopy(plan)
        modified['build'] = False
        modified['primary_gates'] = local.POLICY
        with self.assertRaises(ValueError):
            local.validate(self.root, modified)
        self.commit('README.md', 'new candidate')
        with self.assertRaisesRegex(ValueError, 'different candidate'):
            local.validate(self.root, plan)

    def test_uncommitted_or_untracked_sources_cannot_hide_behind_commit_identity(self):
        plan = local.plan(self.root, 'auto', self.base)
        (self.root / 'README.md').write_text('uncommitted')
        with self.assertRaisesRegex(ValueError, 'clean committed'):
            local.validate(self.root, plan)
        self.git('restore', 'README.md')
        (self.root / 'untracked.rs').write_text('untracked source')
        with self.assertRaisesRegex(ValueError, 'clean committed'):
            local.plan(self.root, 'auto', self.base)

    def test_failure_stops_later_gates_and_clears_old_success_receipts(self):
        plan = local.plan(self.root, 'auto', self.base)
        output = self.root / '.git/receipts'
        output.mkdir()
        for gate in local.COMMANDS:
            local.ci.write_json(output / (gate + '.json'), {
                'candidate': plan['candidate'], 'tree': plan['tree'], 'status': 'passed',
            })
        with patch.object(local.ci, 'record', return_value=7) as record:
            self.assertEqual(local.check(self.root, output, plan), 7)
        self.assertEqual(record.call_count, 1)
        summary = json.loads((output / 'summary.json').read_text())
        self.assertFalse(summary['selected_commands_passed'])
        self.assertEqual(summary['checks']['docs']['status'], 'not-run')
        self.assertFalse((output / 'tests.json').exists())

    def test_scoped_run_produces_timings_without_full_or_oracle_credit(self):
        plan = local.plan(self.root, 'auto', self.base)
        output = self.root / '.git/receipts'

        def record(root, destination, gate, command, expect_tests):
            local.ci.write_json(destination / (gate + '.json'), {
                'candidate': plan['candidate'], 'tree': plan['tree'], 'status': 'passed',
                'duration_seconds': 0.2,
            })
            return 0

        with patch.object(local.ci, 'record', side_effect=record):
            self.assertEqual(local.check(self.root, output, plan), 0)
        summary = json.loads((output / 'summary.json').read_text())
        self.assertTrue(summary['selected_commands_passed'])
        self.assertEqual(summary['duration_seconds'], 1)
        self.assertEqual(summary['tier'], 'local-docs')
        self.assertFalse(summary['release_acceptance'])
        self.assertEqual(summary['licensed_credit'], 0)


if __name__ == '__main__':
    unittest.main()
