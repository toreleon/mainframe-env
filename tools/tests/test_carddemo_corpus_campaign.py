"""Owned-copy/closure refusal controls only; no product or corpus acceptance."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

TOOL = Path(__file__).resolve().parents[1] / 'carddemo_corpus_campaign.py'
SPEC = importlib.util.spec_from_file_location('corpus_campaign_controls', TOOL)
campaign = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(campaign)


class OwnedCopyControls(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='corpus-control-')
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

    def test_copy_includes_git_and_exact_all_file_bytes_without_links(self):
        source = self.root / 'source'; source.mkdir()
        (source / '.git').mkdir(); (source / '.git/HEAD').write_bytes(b'committed-object-id')
        (source / 'record').write_bytes(bytes(range(256)))
        entries, total = campaign.scan(source)
        campaign.copy_complete(source, self.copy, entries, self.owner.verify)
        self.assertEqual(campaign.scan(self.copy), (entries, total))
        self.assertNotEqual((source / 'record').stat().st_ino, (self.copy / 'record').stat().st_ino)

    def test_same_count_source_change_after_manifest_refuses_copy(self):
        source = self.root / 'source'; source.mkdir(); (source / 'record').write_bytes(b'original')
        entries, _ = campaign.scan(source)
        (source / 'record').write_bytes(b'changed!')
        with self.assertRaisesRegex(ValueError, 'source changed'):
            campaign.copy_complete(source, self.copy, entries, self.owner.verify)

    def test_initial_copy_replaced_root_refuses_next_file_and_directory_writes(self):
        for next_kind in ['file', 'directory']:
            with self.subTest(next_kind=next_kind), tempfile.TemporaryDirectory(prefix='copy-replacement-') as temporary:
                root = Path(temporary); source = root / 'source'; source.mkdir()
                (source / 'first').write_bytes(b'first copied bytes')
                if next_kind == 'file':
                    (source / 'next').write_bytes(b'must not enter replacement')
                else:
                    (source / 'next').mkdir()
                entries, _ = campaign.scan(source)
                target = root / 'target'; target.mkdir(); claimed = campaign.identity(target)
                displaced = root / 'retained-original'; calls = 0
                def guarded_copy_write():
                    nonlocal calls
                    calls += 1
                    if calls == 4:  # The first file's mkdir/open/chmod already completed.
                        self.assertEqual((target / 'first').read_bytes(), b'first copied bytes')
                        target.rename(displaced); target.mkdir()
                        (target / 'sentinel').write_bytes(b'replacement bytes')
                    if campaign.identity(target) != claimed:
                        raise ValueError('owned namespace identity changed')
                with self.assertRaisesRegex(ValueError, 'identity changed'):
                    campaign.copy_complete(source, target, entries, guarded_copy_write)
                self.assertEqual(calls, 4)
                self.assertFalse((target / 'next').exists())
                self.assertEqual(list(target.iterdir()), [target / 'sentinel'])
                self.assertEqual((target / 'sentinel').read_bytes(), b'replacement bytes')
                self.assertEqual((displaced / 'first').read_bytes(), b'first copied bytes')

    def test_source_links_are_refused_including_directory_links(self):
        source = self.root / 'source'; source.mkdir()
        (source / 'outside').symlink_to(self.copy, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'link or special'):
            campaign.scan(source)

    def test_original_copy_byte_bound_refuses_sparse_oversized_file_before_read(self):
        source = self.root / 'source'; source.mkdir()
        with (source / 'too-large').open('wb') as stream:
            stream.truncate(campaign.MAX_COPY_BYTES + 1)
        with self.assertRaisesRegex(ValueError, 'byte bound'):
            campaign.scan(source)

    def test_original_entry_bound_refuses_one_too_many_real_entries(self):
        source = self.root / 'source'; source.mkdir()
        for i in range(campaign.MAX_COPY_ENTRIES + 1):
            (source / str(i)).touch()
        with self.assertRaisesRegex(ValueError, 'entry bound'):
            campaign.scan(source)

    def test_replaced_namespace_refuses_mutant_and_restore_write_preserving_sentinel(self):
        displaced = self.evidence / 'retained-original'; self.copy.rename(displaced)
        self.copy.mkdir(); (self.copy / 'record').write_bytes(b'sentinel')
        for offered in [b'mutant', b'original']:
            with self.assertRaisesRegex(ValueError, 'identity changed'):
                self.owner.write('record', offered)
        self.assertEqual((self.copy / 'record').read_bytes(), b'sentinel')
        with self.assertRaisesRegex(ValueError, 'identity changed'):
            self.owner.cleanup()
        self.assertTrue(displaced.is_dir()); self.assertTrue(self.copy.is_dir())

    def test_unknown_closure_refuses_writes_and_cleanup_without_removing_namespace(self):
        (self.copy / 'record').write_bytes(b'original'); self.owner.closed = False
        with self.assertRaisesRegex(ValueError, 'closure'):
            self.owner.write('record', b'mutant')
        with self.assertRaisesRegex(ValueError, 'closure'):
            self.owner.cleanup()
        self.assertEqual((self.copy / 'record').read_bytes(), b'original')

    def test_required_retention_failure_preserves_real_owned_copy(self):
        self.owner.proven_requirements = list(campaign.REQUIREMENTS)  # Synthetic completed context only.
        (self.copy / 'artifact').write_bytes(b'actual retained bytes')
        self.owner.save = mock.Mock(side_effect=OSError('forced receipt retention failure'))
        with self.assertRaisesRegex(OSError, 'retention'):
            self.owner.cleanup()
        self.assertTrue(self.copy.is_dir())
        self.assertEqual((self.copy / 'artifact').read_bytes(), b'actual retained bytes')
        self.assertEqual(self.owner.report['requirements'], [])

    def test_final_absence_receipt_is_saved_only_after_actual_owned_disposal(self):
        self.owner.proven_requirements = list(campaign.REQUIREMENTS)  # Synthetic completed context only.
        (self.copy / 'artifact').write_bytes(b'retained evidence')
        self.owner.cleanup()
        self.assertFalse(self.copy.exists())
        result = json.loads((self.evidence / 'campaign.json').read_text())
        self.assertEqual(result['cleanup']['status'], 'removed-and-absence-verified')
        self.assertEqual(result['requirements'], campaign.REQUIREMENTS)

    def test_skipped_actual_campaign_cannot_delete_copy_or_bind_requirements(self):
        with self.assertRaisesRegex(ValueError, 'incomplete'):
            self.owner.cleanup()
        self.assertTrue(self.copy.is_dir())
        self.assertEqual(self.owner.report['requirements'], [])

    def test_zero_exit_without_closure_notification_retains_actual_result_and_copy(self):
        self.owner.repo = self.root
        self.owner.start = campaign.time.monotonic()
        self.owner.output_bytes = 0
        self.owner.report['commands'] = []
        self.owner.supervisor = mock.Mock()
        self.owner.supervisor._run_owned.return_value = (0, None)
        with self.assertRaisesRegex(ValueError, 'closure'):
            self.owner.run('fake-zero-exit', ['unused'])
        result = json.loads((self.evidence / 'campaign.json').read_text())
        self.assertEqual(result['commands'][0]['exit_code'], 0)
        self.assertFalse(result['commands'][0]['owned_group_closed'])
        self.assertTrue(self.copy.exists())
        self.assertEqual(result['requirements'], [])


if __name__ == '__main__':
    unittest.main()
