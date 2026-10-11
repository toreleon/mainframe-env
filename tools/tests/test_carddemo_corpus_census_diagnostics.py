"""Inert corpus census diagnostic controls; no process or corpus execution."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

TOOL = Path(__file__).resolve().parents[1] / 'carddemo_corpus_campaign.py'
SPEC = importlib.util.spec_from_file_location('corpus_census_diagnostic_controls', TOOL)
campaign = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(campaign)


class CorpusCensusDiagnosticControls(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='corpus-census-control-')
        self.root = Path(self.temporary.name)
        self.evidence = self.root / 'evidence'; self.evidence.mkdir()
        self.copy = self.evidence / 'owned-corpus'; self.copy.mkdir()
        self.owner = campaign.Campaign.__new__(campaign.Campaign)
        self.owner.evidence = self.evidence
        self.owner.evidence_id = campaign.identity(self.evidence)
        self.owner.copy = self.copy
        self.owner.copy_id = campaign.identity(self.copy)
        self.owner.closed = True
        self.owner.proven_requirements = []
        self.owner.report = {'status': 'running', 'requirements': [], 'cleanup': {'status': 'pending'}}
        self.owner.verify_inputs = mock.Mock()  # No compiled/product producer in these controls.

    def tearDown(self):
        self.temporary.cleanup()  # This fixture owns disposal, not the refused campaign.

    def diagnostic_command_owner(self):
        # This is an inert callback control, never a process or corpus producer.
        from contextvars import ContextVar
        self.owner.repo = self.root
        self.owner.start = campaign.time.monotonic()
        self.owner.output_bytes = 0
        self.owner.report['commands'] = []
        self.owner.supervisor = mock.Mock()
        context = ContextVar('inert-corpus-census-diagnostics', default=None)
        self.owner.supervisor._LINUX_CENSUS_DIAGNOSTICS = context
        return context

    def test_failed_census_diagnostics_keep_exact_error_unknown_closure_and_copy(self):
        context = self.diagnostic_command_owner()
        parent = {'observations': [], 'omitted_observations': 0}
        parent_token = context.set(parent)
        error = 'owned group cleanup uncertain: owned group membership census observation deadline exceeded'
        trace = {'stage': 'scan_complete_deadline', 'owner_phase': 'final_cleanup',
                 'elapsed_seconds': .051, 'thread_cpu_seconds': .003,
                 'numeric_entries_per_scan': [2], 'stat_reads_started': 2,
                 'stat_reads_completed': 2, 'vanished_scans': 0,
                 'member_observed': False, 'leader_retained': True}
        def refuse(command, root, output, **options):
            self.assertEqual(command, ['inert'])
            self.assertTrue(0 < options['timeout_seconds'] <= campaign.COMMAND_SECONDS)
            self.assertEqual(options['max_output_bytes'], campaign.MAX_OUTPUT_BYTES)
            self.assertTrue(options['separate_stderr'])
            actual = context.get()
            self.assertIsNot(actual, parent)
            actual['observations'].append(dict(trace))
            actual['omitted_observations'] = 1
            output('stderr', b'actual retained stderr')
            return 1, error  # No closure callback: the real primitive refused it.
        self.owner.supervisor._run_owned.side_effect = refuse
        try:
            with self.assertRaisesRegex(ValueError, 'closure is not established'):
                self.owner.run('failed-census', ['inert'])
            self.assertIs(context.get(), parent)
            self.assertEqual(parent, {'observations': [], 'omitted_observations': 0})
        finally:
            context.reset(parent_token)
        row = json.loads((self.evidence / 'campaign.json').read_text())['commands'][0]
        self.assertEqual(row['exit_code'], 1)
        self.assertEqual(row['error'], error)
        self.assertEqual(row['status'], 'running')
        self.assertFalse(row['owned_group_closed'])
        self.assertEqual(row['census_diagnostics'], {
            'schema_version': 'private.carddemo.corpus-census-diagnostic@1',
            'observations': [trace], 'omitted_observations': 1,
            'scope': 'Bounded failed procfs observations only; no closure or test credit.',
        })
        self.assertEqual((self.evidence / '00-failed-census.stderr').read_bytes(),
                         b'actual retained stderr')
        self.assertEqual(self.owner.report['requirements'], [])
        with self.assertRaisesRegex(ValueError, 'closure'):
            self.owner.cleanup()
        self.assertTrue(self.copy.is_dir())

    def test_closed_nonzero_command_adds_no_census_diagnostic_and_restores_context(self):
        context = self.diagnostic_command_owner()
        parent = {'observations': [{'sentinel': 'parent diagnostic'}], 'omitted_observations': 2}
        parent_token = context.set(parent)
        def complete(command, root, output, **options):
            self.assertEqual(context.get(), {'observations': [], 'omitted_observations': 0})
            output('stderr', b'original refusal')
            options['on_closed'](1)
            return 1, None
        self.owner.supervisor._run_owned.side_effect = complete
        try:
            self.assertEqual(self.owner.run('closed-refusal', ['inert']),
                             (1, b'', b'original refusal'))
            self.assertIs(context.get(), parent)
        finally:
            context.reset(parent_token)
        row = json.loads((self.evidence / 'campaign.json').read_text())['commands'][0]
        self.assertEqual(row['closed_exit_code'], 1)
        self.assertTrue(row['owned_group_closed'])
        self.assertEqual(row['status'], 'closed')
        self.assertNotIn('census_diagnostics', row)
        self.assertEqual(parent, {'observations': [{'sentinel': 'parent diagnostic'}],
                                  'omitted_observations': 2})
        self.assertEqual(self.owner.report['requirements'], [])

    def test_supervisor_exception_restores_context_and_keeps_failed_observation(self):
        context = self.diagnostic_command_owner()
        parent = {'observations': [], 'omitted_observations': 0}
        parent_token = context.set(parent)
        trace = {'stage': 'stat_read', 'errno': 13}
        def fail(command, root, output, **options):
            context.get()['observations'].append(dict(trace))
            raise OSError('forced supervisor exception')
        self.owner.supervisor._run_owned.side_effect = fail
        try:
            with self.assertRaisesRegex(OSError, 'forced supervisor exception'):
                self.owner.run('raised', ['inert'])
            self.assertIs(context.get(), parent)
        finally:
            context.reset(parent_token)
        # The existing main error path saves this in-memory failed row; saving
        # it here exercises retention without admitting the inert command.
        self.owner.save()
        row = json.loads((self.evidence / 'campaign.json').read_text())['commands'][0]
        self.assertEqual(row['census_diagnostics']['observations'], [trace])
        self.assertFalse(row['owned_group_closed'])
        self.assertNotIn('exit_code', row)
        self.assertEqual(self.owner.report['requirements'], [])
        self.assertTrue(self.copy.is_dir())


if __name__ == '__main__':
    unittest.main()
