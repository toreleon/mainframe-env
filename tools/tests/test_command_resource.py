import errno
import importlib.util
import json
import os
from pathlib import Path
import signal
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch


ROOT = Path(__file__).resolve().parents[2]
TOOL = ROOT / 'tools' / 'command_resource.py'
spec = importlib.util.spec_from_file_location('owned_command_resource', TOOL)
resource = importlib.util.module_from_spec(spec)
spec.loader.exec_module(resource)
ci_spec = importlib.util.spec_from_file_location('resource_test_ci', ROOT / 'tools' / 'ci_assurance.py')
ci = importlib.util.module_from_spec(ci_spec)
ci_spec.loader.exec_module(ci)


class CommandResourceCustodyTests(unittest.TestCase):
    def invoke(self, directory, script, timeout=1, arguments=()):
        destination = Path(directory) / 'actual.resource.json'
        chunks = []
        closed = []
        code, error = ci._run_owned(
            [sys.executable, '-B', str(TOOL), str(destination),
             sys.executable, '-B', '-c', script, *arguments], Path(directory),
            lambda channel, data: chunks.append((channel, data)),
            timeout_seconds=timeout, max_output_bytes=65536, on_closed=closed.append)
        return code, error, closed, b''.join(data for _, data in chunks), destination

    def reap_fixture_children(self, born):
        # The serial test first proves ECHILD before birth. During its admitted
        # scope P_ALL can name only current kernel-owned fixture children; stdout
        # labels are supplementary and never the sole disposal authority.
        errors = []
        reaped = set()

        def retain_and_wait(observed, expected_pid=None):
            if observed is None:
                raise ValueError('fixture child exit remains unknown')
            pid = observed.si_pid
            if pid <= 1 or pid == os.getpid() or (expected_pid is not None and pid != expected_pid):
                raise ValueError('fixture kernel child identity differs')
            if observed.si_code not in (os.CLD_EXITED, os.CLD_KILLED, os.CLD_DUMPED):
                raise ValueError('fixture kernel child exit differs')
            status, _ = resource._wait_primary(pid)
            reaped.add(pid)
            expected = observed.si_status if observed.si_code == os.CLD_EXITED else -observed.si_status
            if os.waitstatus_to_exitcode(status) != expected:
                raise ValueError('fixture retained observation and wait differ')
            if os.waitstatus_to_exitcode(status) != -signal.SIGKILL:
                raise ValueError('fixture actual exit differs from group SIGKILL')

        # One faulty/unknown known-child observation cannot skip the remaining
        # known children. No signal or wait is issued from a reused procfs PID.
        for pid in sorted(set(born.values())):
            try:
                observed = os.waitid(os.P_PID, pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
                retain_and_wait(observed, pid)
            except Exception as problem:
                errors.append(type(problem).__name__ + ': ' + str(problem))
        for _ in range(resource.MAX_ADOPTED_CHILDREN + 1):
            try:
                observed = os.waitid(os.P_ALL, 0, os.WEXITED | os.WNOHANG | os.WNOWAIT)
            except ChildProcessError as problem:
                if problem.errno == errno.ECHILD:
                    return errors, reaped
                errors.append('fixture ECHILD admission failed: ' + str(problem))
                return errors, reaped
            except Exception as problem:
                errors.append('fixture child observation failed: ' + str(problem))
                return errors, reaped
            try:
                if len(reaped) >= resource.MAX_ADOPTED_CHILDREN:
                    raise ValueError('fixture child inventory exceeds 65536')
                retain_and_wait(observed)
            except Exception as problem:
                errors.append(type(problem).__name__ + ': ' + str(problem))
                return errors, reaped
        errors.append('fixture ECHILD remains unobserved')
        return errors, reaped

    def test_unavailable_subreaper_refuses_before_fork(self):
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(resource.os, 'getpid', return_value=70001), \
                patch.object(resource.os, 'getpgrp', return_value=70001), \
                patch.object(resource.os, 'getsid', return_value=70001), \
                patch.object(resource, '_subreaper', side_effect=OSError('unavailable')), \
                patch.object(resource.os, 'fork') as birth:
            with self.assertRaisesRegex(OSError, 'unavailable'):
                resource.run(Path(directory) / 'resource.json', ['never'])
            birth.assert_not_called()

    def test_unknown_destination_is_retained_without_birth(self):
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(resource.os, 'getpid', return_value=70001), \
                patch.object(resource.os, 'getpgrp', return_value=70001), \
                patch.object(resource.os, 'getsid', return_value=70001), \
                patch.object(resource.os, 'fork') as birth:
            destination = Path(directory) / 'resource.json'
            destination.write_bytes(b'unknown prior bytes')
            with self.assertRaisesRegex(ValueError, 'already exists'):
                resource.run(destination, ['never'])
            self.assertEqual(destination.read_bytes(), b'unknown prior bytes')
            birth.assert_not_called()

    def test_same_inode_corrupted_resource_cannot_be_retained_as_actual_waits(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / 'resource.json'
            owner = resource._parent_identity(destination.parent)
            sync = os.fsync

            def corrupt(fd):
                sync(fd)
                original = destination.read_bytes()
                destination.write_bytes(original.replace(b'"value": 7', b'"value": 8'))

            with patch.object(resource.os, 'fsync', side_effect=corrupt):
                with self.assertRaisesRegex(ValueError, 'bytes differ'):
                    resource._save_resource(destination, owner, {'value': 7})
            self.assertEqual(json.loads(destination.read_bytes()), {'value': 8})

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            parent = root / 'owner'
            parent.mkdir(mode=0o700)
            destination = parent / 'resource.json'
            owner = resource._parent_identity(parent)
            retained = root / 'retained-original'
            sentinel = b'unknown replacement receipt'
            sync = os.fsync

            def replace_parent(fd):
                sync(fd)
                parent.rename(retained)
                parent.mkdir(mode=0o700)
                destination.write_bytes(sentinel)

            with patch.object(resource.os, 'fsync', side_effect=replace_parent):
                with self.assertRaisesRegex(ValueError, 'parent changed'):
                    resource._save_resource(destination, owner, {'value': 7})
            self.assertEqual(destination.read_bytes(), sentinel)
            self.assertEqual((retained / 'resource.json').read_bytes(), b'{\n  "value": 7\n}\n')
            self.assertEqual(resource._parent_identity(retained), owner)

    def test_unknown_adopted_exit_cannot_be_reaped_or_claim_echild(self):
        for observed in [None,
                         SimpleNamespace(si_pid=0, si_code=os.CLD_EXITED),
                         SimpleNamespace(si_pid=os.getpid(), si_code=os.CLD_EXITED),
                         SimpleNamespace(si_pid=42, si_code=os.CLD_EXITED),
                         SimpleNamespace(si_pid=43, si_code=os.CLD_STOPPED)]:
            with self.subTest(observed=observed), \
                    patch.object(resource.os, 'waitid', return_value=observed), \
                    patch.object(resource, '_wait_primary') as wait:
                with self.assertRaises(ValueError):
                    resource._reap_adopted(42)
                wait.assert_not_called()
        for problem in (OSError(errno.EIO, 'unknown'),
                        ChildProcessError(errno.EIO, 'not ECHILD')):
            with self.subTest(problem=type(problem).__name__), \
                    patch.object(resource.os, 'waitid', side_effect=problem), \
                    patch.object(resource, '_wait_primary') as wait:
                with self.assertRaises(type(problem)):
                    resource._reap_adopted(42)
                wait.assert_not_called()
        for problem in (OSError(errno.EIO, 'primary unknown'),
                        ChildProcessError(errno.ECHILD, 'primary missing')):
            with self.subTest(primary_problem=type(problem).__name__), \
                    patch.object(resource.os, 'wait4', side_effect=problem) as wait:
                with self.assertRaises(type(problem)):
                    resource._wait_primary(42)
                wait.assert_called_once_with(42, 0)
        with patch.object(resource.os, 'wait4', return_value=(43, 0, Mock())):
            with self.assertRaisesRegex(ValueError, 'identity differs'):
                resource._wait_primary(42)

    def test_retained_adopted_exit_status_must_match_sole_wait(self):
        observation = SimpleNamespace(si_pid=43, si_code=os.CLD_KILLED, si_status=signal.SIGTERM)
        with patch.object(resource.os, 'waitid', return_value=observation), \
                patch.object(resource, '_wait_primary', return_value=(0, Mock())) as wait:
            with self.assertRaisesRegex(ValueError, 'retained exit'):
                resource._reap_adopted(42)
            wait.assert_called_once_with(43)
        with patch.object(resource.os, 'waitid', side_effect=ChildProcessError(errno.ECHILD, 'no children')):
            self.assertEqual(resource._reap_adopted(42), [])

    def test_actual_primary_success_keeps_original_wait4_usage_and_closure(self):
        with tempfile.TemporaryDirectory() as directory:
            code, error, closed, raw, destination = self.invoke(
                directory, 'import os,sys; assert sys.argv[1]==""; os.write(1,b"actual primary output")',
                arguments=('',))
            self.assertEqual((code, error, closed, raw), (0, None, [0], b'actual primary output'))
            row = json.loads(destination.read_bytes())
            self.assertEqual(row['schema_version'], 'external.linux-command-resource@1')
            self.assertEqual(row['child_exit_code'], 0)
            self.assertEqual(row['wrapper_exit_code'], 0)
            self.assertEqual(row['adopted_child_waits'], [])
            self.assertEqual(row['wrapper_deferred_signals'], [])
            self.assertIs(row['kernel_child_set_echild_observed'], True)
            self.assertEqual(row['group_closure_credit'], 0)
            self.assertGreater(row['child_pid'], 1)
            self.assertGreater(row['max_process_rss_kib'], 0)
            self.assertGreaterEqual(row['user_seconds'], 0)
            self.assertGreaterEqual(row['system_seconds'], 0)
            self.assertIn('not aggregate group peak', row['qualification'])

    def test_actual_primary_nonzero_exit_remains_failed(self):
        with tempfile.TemporaryDirectory() as directory:
            code, error, closed, raw, destination = self.invoke(directory, 'raise SystemExit(23)')
            self.assertEqual((code, error, closed, raw), (23, None, [23], b''))
            row = json.loads(destination.read_bytes())
            self.assertEqual((row['child_exit_code'], row['wrapper_exit_code']), (23, 23))
            self.assertIs(row['kernel_child_set_echild_observed'], True)

        with tempfile.TemporaryDirectory() as directory:
            # The actual born child cancels only its own parent wrapper, then
            # exits0. Cancellation must remain an actual failed wrapper result.
            script = 'import os,signal,time; os.kill(os.getppid(),signal.SIGTERM); time.sleep(.03)'
            code, error, closed, raw, destination = self.invoke(directory, script)
            row = json.loads(destination.read_bytes())
            self.assertEqual((code, error, closed, raw), (143, None, [143], b''))
            self.assertEqual((row['child_exit_code'], row['wrapper_exit_code']), (0, 143))
            self.assertEqual(row['wrapper_deferred_signals'], [signal.SIGTERM])
            self.assertIs(row['kernel_child_set_echild_observed'], True)
            self.assertEqual(row['adopted_child_waits'], [])
            self.assertEqual(row['group_closure_credit'], 0)
            # Existing legacy primary-exit joins intentionally refuse this row;
            # a future exact-helper wrapper-exit join may admit accounting only.
            self.assertNotEqual(row['child_exit_code'], code)

        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / 'actual.resource.json'
            chunks = []
            closed = []
            # This dedicated subprocess is the real already-born session leader.
            # It sends TERM only to itself after fsync completes, at the actual
            # snapshot/save boundary; no guessed child or released PGID is used.
            script = "import importlib.util,os,signal,sys\nspec=importlib.util.spec_from_file_location('actual_resource_finalization',sys.argv[1])\nresource=importlib.util.module_from_spec(spec)\nspec.loader.exec_module(resource)\nsync=resource.os.fsync\ndef cancel_during_save(fd):\n    sync(fd)\n    os.kill(os.getpid(),signal.SIGTERM)\nresource.os.fsync=cancel_during_save\nraise SystemExit(resource.main(sys.argv[2:]))\n"
            code, error = ci._run_owned(
                [sys.executable, '-B', '-c', script, str(TOOL), str(destination),
                 sys.executable, '-B', '-c', 'pass'], Path(directory),
                lambda channel, data: chunks.append((channel, data)),
                timeout_seconds=1, max_output_bytes=65536, on_closed=closed.append)
            self.assertEqual((code, error, closed), (125, None, [125]))
            self.assertEqual(b''.join(data for _, data in chunks),
                             b'owned resource witness failed: InterruptedError, errno=4\n')
            diagnostic = json.loads(destination.read_bytes())
            self.assertEqual((diagnostic['child_exit_code'], diagnostic['wrapper_exit_code']), (0, 0))
            self.assertEqual(diagnostic['group_closure_credit'], 0)
            # An already-written pre-finalization snapshot is diagnostic only:
            # neither legacy nor new wrapper-exit admission may relabel125 as0.
            self.assertNotEqual(diagnostic['child_exit_code'], code)
            self.assertNotEqual(diagnostic['wrapper_exit_code'], code)

    def test_actual_term_reaps_direct_and_adopted_children_without_timeout_pass(self):
        script = '''import json,os,time
grandchild=os.fork()
if grandchild==0:
    print(json.dumps({'role':'adopted','pid':os.getpid()}),flush=True)
    time.sleep(10)
    os._exit(0)
print(json.dumps({'role':'primary','pid':os.getpid()}),flush=True)
time.sleep(10)
'''
        with tempfile.TemporaryDirectory() as directory:
            code, error, closed, raw, destination = self.invoke(directory, script, timeout=.4)
            born = {row['role']: row['pid'] for row in map(json.loads, raw.splitlines())}
            self.assertEqual(set(born), {'primary', 'adopted'})
            self.assertEqual(code, 128 + signal.SIGTERM)
            self.assertIn('command deadline exceeded', error)
            self.assertEqual(closed, [code])
            row = json.loads(destination.read_bytes())
            self.assertEqual(row['child_pid'], born['primary'])
            self.assertEqual(row['child_exit_code'], -signal.SIGTERM)
            self.assertEqual(row['wrapper_deferred_signals'], [signal.SIGTERM])
            self.assertIs(row['kernel_child_set_echild_observed'], True)
            self.assertEqual([x['child_pid'] for x in row['adopted_child_waits']], [born['adopted']])
            self.assertEqual(row['adopted_child_waits'][0]['child_exit_code'], -signal.SIGTERM)
            for pid in born.values():
                self.assertFalse((Path('/proc') / str(pid)).exists())

    def test_actual_group_sigkill_still_refuses_and_test_fixture_reaps_only_its_children(self):
        # Private test-only adoption prevents deliberately killed wrapper children
        # from reaching PID1. It is not production group closure evidence.
        prior = resource._subreaper()
        born = {}
        admitted = False
        cleanup_errors = []
        try:
            # Refuse a test-parent preexisting child rather than reaping it.
            try:
                os.waitid(os.P_ALL, 0, os.WEXITED | os.WNOHANG | os.WNOWAIT)
            except ChildProcessError as problem:
                self.assertEqual(problem.errno, errno.ECHILD)
            else:
                self.fail('test parent child set is not empty before fixture birth')
            admitted = True
            resource._subreaper(True)
            script = "import json,os,signal,time\nsignal.signal(signal.SIGTERM,signal.SIG_IGN)\ngrandchild=os.fork()\nif grandchild==0:\n    print(json.dumps({'role':'adopted','pid':os.getpid()}),flush=True)\n    time.sleep(10)\n    os._exit(0)\nprint(json.dumps({'role':'primary','pid':os.getpid()}),flush=True)\ntime.sleep(10)\n"
            with tempfile.TemporaryDirectory() as directory:
                code, error, closed, raw, destination = self.invoke(directory, script, timeout=.4)
                for line in raw.splitlines():
                    row = json.loads(line)
                    born[row['role']] = row['pid']
                self.assertEqual(set(born), {'primary', 'adopted'})
                self.assertEqual(code, -signal.SIGKILL)
                self.assertIsNotNone(error)
                self.assertEqual(closed, [])
                self.assertFalse(destination.exists())
        finally:
            original_problem = sys.exc_info()[1]
            try:
                if admitted:
                    cleanup_errors, reaped = self.reap_fixture_children(born)
                    if not set(born.values()).issubset(reaped):
                        cleanup_errors.append('not all known fixture children were reaped')
            finally:
                # Restore even if observation, wait, assertion or reaping fails.
                try:
                    resource._subreaper(bool(prior))
                except Exception as problem:
                    cleanup_errors.append('fixture subreaper restoration failed: ' + str(problem))
            if cleanup_errors:
                cleanup_problem = AssertionError('; '.join(cleanup_errors))
                if original_problem is not None:
                    raise BaseExceptionGroup('fixture outcome and cleanup remain failed',
                                             [original_problem, cleanup_problem])
                raise cleanup_problem


if __name__ == '__main__':
    unittest.main()
