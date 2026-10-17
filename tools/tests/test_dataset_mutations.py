import importlib.util
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

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

    def test_test_only_declarations_do_not_hide_production_but_remain_protected(self):
        source = '#[cfg(test)]\nuse test_helpers::ANCHOR;\n#[cfg(test)]\nmod external_tests;\nfn product() { BODY }\n#[cfg(test)]\nmod tests { BODY }\n'
        mutation = module.Mutation('test', 'test', 'fn product() { BODY }', 'fn product() { CHANGED }')
        changed = module.apply_mutation(source, mutation)
        self.assertEqual(changed, source.replace('fn product() { BODY }', 'fn product() { CHANGED }'))
        with self.assertRaises(ValueError):
            module.apply_mutation(source, module.Mutation('test', 'test', 'ANCHOR', 'CHANGED'))
        with self.assertRaises(ValueError):
            module.apply_mutation(source, module.Mutation('test', 'test', 'external_tests', 'CHANGED'))

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
                module.split_test_code(changed)[1],
                module.split_test_code(source)[1],
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


class ExecutionOwnershipTests(unittest.TestCase):
    def test_execute_isolates_inherited_targets_and_records_actual_context(self):
        with tempfile.TemporaryDirectory() as directory:
            external = Path(directory)
            caller_target = external / 'caller-target'
            caller_target.mkdir()
            marker = caller_target / 'protected-artifact'
            marker.write_bytes(b'caller-owned')
            caller_build = external / 'caller-build'
            caller_build.mkdir()
            build_marker = caller_build / 'protected-intermediate'
            build_marker.write_bytes(b'caller-owned-intermediate')
            for inherited, inherited_build in ((str(caller_target), str(caller_build)),
                                               ('relative-caller-target', 'relative-caller-build')):
                with self.subTest(inherited=inherited, inherited_build=inherited_build):
                    receipt = {}
                    retained = external / 'receipt.json'

                    def save():
                        retained.write_text(json.dumps(receipt))

                    with module.owned_snapshot(receipt, save) as snapshot:
                        owner = module.claim_build_owner(snapshot)
                        receipt['build_owner'] = owner.verify()
                        actual = owner.verify()

                        def child(command, cwd, on_output, **arguments):
                            self.assertEqual(cwd, snapshot.path)
                            environment = arguments['env']
                            self.assertEqual(environment['CARGO_TARGET_DIR'], str(owner.target))
                            self.assertNotEqual(environment['CARGO_TARGET_DIR'], inherited)
                            self.assertEqual(environment['CARGO_BUILD_BUILD_DIR'], str(owner.build))
                            self.assertNotEqual(environment['CARGO_BUILD_BUILD_DIR'], inherited_build)
                            (Path(environment['CARGO_TARGET_DIR']) / 'mutant-artifact').write_bytes(b'mock')
                            (Path(environment['CARGO_BUILD_BUILD_DIR']) / 'mutant-intermediate').write_bytes(b'mock')
                            # These tests exercise ownership, never fabricate product-test credit.
                            on_output('stdout', b'mocked child; no product tests executed\n')
                            arguments['on_closed'](0)
                            return 0, None

                        with patch.dict(module.os.environ, {'CARGO_TARGET_DIR': inherited,
                                                           'CARGO_BUILD_BUILD_DIR': inherited_build,
                                                           'CARGO_BUILD_JOBS': '2'}), \
                                patch.object(module.supervisor, '_run_owned', side_effect=child) as run:
                            result = module.execute(owner, external / 'execution.log', 1)
                        run.assert_called_once()
                        self.assertEqual(result['working_directory'], actual['working_directory'])
                        self.assertEqual(result['cargo_target_directory'], actual['cargo_target_directory'])
                        self.assertEqual(result['cargo_build_directory'], actual['cargo_build_directory'])
                        self.assertEqual(result['environment']['CARGO_TARGET_DIR'], str(owner.target))
                        self.assertEqual(result['environment']['CARGO_BUILD_BUILD_DIR'], str(owner.build))
                        self.assertEqual(result['environment']['CARGO_BUILD_JOBS'], '2')
                        self.assertEqual(result['environment']['CARGO_INCREMENTAL'], '0')
                        self.assertTrue(result['ownership_verified_after'])
                        self.assertTrue(result['process_supervision']['group_closed'])
                        self.assertEqual(result['classification'], 'invalid')
                        self.assertEqual(result['killing_tests'], [])
                    cleanup = receipt['build_owner_cleanup']
                    self.assertTrue(cleanup['verified'])
                    self.assertFalse(module.os.path.lexists(snapshot))
                    self.assertFalse(module.os.path.lexists(owner.target))
                    self.assertFalse(module.os.path.lexists(owner.build))
                    self.assertEqual(json.loads(retained.read_text())['build_owner_cleanup'], cleanup)
                    self.assertEqual(marker.read_bytes(), b'caller-owned')
                    self.assertEqual(build_marker.read_bytes(), b'caller-owned-intermediate')

    def test_fresh_target_claim_refuses_existing_namespace_without_spawning(self):
        with tempfile.TemporaryDirectory() as directory:
            external = Path(directory)
            protected = external / 'protected-target'
            protected.mkdir()
            marker = protected / 'protected-artifact'
            marker.write_bytes(b'caller-owned')
            for namespace, kind in ((namespace, kind) for namespace in ('target', 'build')
                                    for kind in ('directory', 'file', 'symlink')):
                with self.subTest(namespace=namespace, kind=kind):
                    receipt = {}
                    with module.owned_snapshot(receipt, lambda: None) as snapshot:
                        target = snapshot / namespace
                        if kind == 'directory':
                            target.mkdir()
                        elif kind == 'file':
                            target.write_bytes(b'existing')
                        else:
                            target.symlink_to(protected, target_is_directory=True)
                        with patch.object(module.supervisor, '_run_owned') as run:
                            with self.assertRaises(FileExistsError):
                                module.claim_build_owner(snapshot)
                        run.assert_not_called()
                    self.assertTrue(receipt['build_owner_cleanup']['verified'])
                    self.assertEqual(marker.read_bytes(), b'caller-owned')

    def test_replaced_target_identity_refuses_before_child_spawn(self):
        receipt = {}
        with self.assertRaisesRegex(ValueError, 'cleanup was not verified'):
            with module.owned_snapshot(receipt, lambda: None) as snapshot:
                owner = module.claim_build_owner(snapshot)
                owner.target.rename(snapshot / 'replaced-target')
                owner.target.mkdir()
                with patch.object(module.supervisor, '_run_owned') as run:
                    with self.assertRaisesRegex(ValueError, 'identity changed'):
                        module.execute(owner, snapshot / 'unused.log', 1)
                run.assert_not_called()
        self.assertFalse(receipt['build_owner_cleanup']['verified'])
        # The test created both directories and launched no command. Verify the
        # original test-owned root before disposing of its retained namespace.
        self.assertEqual(module.directory_identity(snapshot.path), snapshot.identity)
        shutil.rmtree(snapshot.path)

    def test_exception_cleanup_retains_actual_owner_and_absence_receipt(self):
        with tempfile.TemporaryDirectory() as directory:
            retained = Path(directory) / 'receipt.json'
            receipt = {}
            with self.assertRaisesRegex(ValueError, 'baseline failed'):
                with module.owned_snapshot(receipt, lambda: retained.write_text(json.dumps(receipt))) as snapshot:
                    owner = module.claim_build_owner(snapshot)
                    receipt['build_owner'] = owner.verify()
                    raise ValueError('baseline failed')
            stored = json.loads(retained.read_text())
            self.assertEqual(stored['snapshot_owner'], owner.cwd_identity)
            self.assertEqual(stored['build_owner']['cargo_target_directory'], owner.target_identity)
            self.assertEqual(stored['build_owner']['cargo_build_directory'], owner.build_identity)
            self.assertTrue(stored['build_owner_cleanup']['verified'])
            self.assertFalse(module.os.path.lexists(snapshot))
            self.assertFalse(module.os.path.lexists(owner.target))
            self.assertFalse(module.os.path.lexists(owner.build))

    def test_unknown_group_closure_refuses_restoration_and_cleanup(self):
        receipt = {}
        with tempfile.TemporaryDirectory() as directory:
            retained = Path(directory) / 'receipt.json'
            with patch.object(module.shutil, 'rmtree') as remove:
                with self.assertRaisesRegex(ValueError, 'cleanup was not verified'):
                    with module.owned_snapshot(receipt, lambda: retained.write_text(json.dumps(receipt))) as snapshot:
                        owner = module.claim_build_owner(snapshot)
                        source = snapshot / 'source.rs'
                        source.write_text('MUTANT')
                        with patch.object(module.supervisor, '_run_owned', return_value=(0, 'owned group cleanup uncertain: fixture')):
                            result = module.execute(owner, Path(directory) / 'execution.log', 1)
                            self.assertEqual(result['classification'], 'invalid')
                            self.assertEqual(result['killing_tests'], [])
                            self.assertFalse(result['process_supervision']['group_closed'])
                            with self.assertRaisesRegex(ValueError, 'closure was not established'):
                                owner.restore(Path('source.rs'), 'BASE')
                            self.assertEqual(source.read_text(), 'MUTANT')
                remove.assert_not_called()
            stored = json.loads(retained.read_text())
            self.assertFalse(stored['build_owner_cleanup']['verified'])
            self.assertFalse(stored['build_owner_cleanup']['command_group_closed'])
            self.assertFalse(stored['build_owner_cleanup']['root_identity_verified_before_removal'])
            self.assertEqual(module.directory_identity(snapshot.path), snapshot.identity)
            # The mocked supervisor launched nothing; disposal is test-owned.
            shutil.rmtree(snapshot.path)

    def test_replaced_snapshot_cannot_receive_restoration_or_deletion(self):
        receipt = {}
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, 'cleanup was not verified'):
                with module.owned_snapshot(receipt, lambda: None) as snapshot:
                    owner = module.claim_build_owner(snapshot)
                    (snapshot / 'source.rs').write_text('MUTANT')
                    snapshot.path.rename(Path(directory) / 'original-owned-snapshot')
                    snapshot.path.mkdir()
                    replacement_identity = module.directory_identity(snapshot.path)
                    protected = snapshot / 'source.rs'
                    protected.write_text('replacement-owner')
                    with self.assertRaisesRegex(ValueError, 'snapshot identity changed'):
                        owner.write_source(Path('source.rs'), 'NEXT-MUTANT')
                    self.assertEqual(protected.read_text(), 'replacement-owner')
                    with self.assertRaisesRegex(ValueError, 'snapshot identity changed'):
                        owner.restore(Path('source.rs'), 'BASE')
            self.assertEqual(protected.read_text(), 'replacement-owner')
            self.assertFalse(receipt['build_owner_cleanup']['verified'])
            self.assertFalse(receipt['build_owner_cleanup']['root_identity_verified_before_removal'])
            # Separately prove this test-created replacement's own identity.
            self.assertEqual(module.directory_identity(snapshot.path), replacement_identity)
            shutil.rmtree(snapshot.path)

    @unittest.skipUnless(sys.platform == 'linux' and hasattr(os, 'fork'), 'Linux launched-group fixture')
    def test_real_descendant_timeout_closes_group_before_restore_and_cleanup(self):
        receipt = {}
        # Parent and child share an unbuffered pipe. Each identity record must
        # be one write below PIPE_BUF; multi-argument print can interleave.
        child_script = '''import os, signal, time
child = os.fork()
if child == 0:
    os.write(1, f'CHILD {os.getpid()} {os.getpgrp()}\\n'.encode())
    while True:
        for name in ('CARGO_TARGET_DIR', 'CARGO_BUILD_BUILD_DIR'):
            with open(os.path.join(os.environ[name], 'child-artifact'), 'w') as stream:
                stream.write(str(time.monotonic_ns()))
        time.sleep(.005)
def stop(signum, frame):
    pid, status = os.waitpid(child, 0)
    os.write(1, f'REAPED {pid} {status}\\n'.encode())
    raise SystemExit(0)
signal.signal(signal.SIGTERM, stop)
os.write(1, f'PARENT {os.getpid()} {os.getpgrp()}\\n'.encode())
while True:
    time.sleep(.01)
'''
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / 'descendant.log'
            fixture_receipt = Path(directory) / 'receipt.json'
            with module.owned_snapshot(receipt, lambda: fixture_receipt.write_text(json.dumps(receipt))) as snapshot:
                owner = module.claim_build_owner(snapshot)
                receipt['build_owner'] = owner.verify()
                (snapshot / 'source.rs').write_text('MUTANT')
                result = module.execute(owner, log, 1, [sys.executable, '-u', '-c', child_script])
                receipt['execution'] = result
                print(json.dumps({'fixture': 'real descendant diagnostic', 'execution': result,
                                  'process_trace': log.read_text()}, sort_keys=True))
                self.assertEqual(result['classification'], 'timed_out')
                self.assertEqual(result['killing_tests'], [])
                self.assertTrue(result['process_supervision']['group_closed'])
                self.assertTrue(result['ownership_verified_after'])
                self.assertLess(result['duration_seconds'], 3)
                raw = log.read_text()
                self.assertLess(len(log.read_bytes()), 512)
                identities = {parts[0]: list(map(int, parts[1:])) for parts in
                              (line.split() for line in raw.splitlines())}
                parent_pid, parent_group = identities['PARENT']
                child_pid, child_group = identities['CHILD']
                self.assertEqual(parent_group, parent_pid)
                self.assertEqual(child_group, parent_pid)
                self.assertEqual(identities['REAPED'][0], child_pid)
                for pid in (parent_pid, child_pid):
                    with self.assertRaises(ProcessLookupError):
                        os.kill(pid, 0)
                markers = [owner.target / 'child-artifact', owner.build / 'child-artifact']
                stable = [marker.read_bytes() for marker in markers]
                time.sleep(.025)
                self.assertEqual([marker.read_bytes() for marker in markers], stable)
                owner.restore(Path('source.rs'), 'BASE')
                self.assertEqual((snapshot / 'source.rs').read_text(), 'BASE')
            self.assertTrue(receipt['build_owner_cleanup']['verified'])
            self.assertTrue(receipt['build_owner_cleanup']['receipt_retained_before_removal'])
            self.assertEqual(json.loads(fixture_receipt.read_text()), receipt)
            # The retained test runner log carries actual bounded closure and
            # cleanup evidence; this Python fixture earns zero product credit.
            print(json.dumps({'fixture': 'real same-group descendant timeout',
                              'execution': result, 'process_trace': raw,
                              'cleanup': receipt['build_owner_cleanup'],
                              'product_tests_completed': 0}, sort_keys=True))

    def test_unverified_cleanup_is_retained_and_refuses_campaign_success(self):
        with tempfile.TemporaryDirectory() as directory:
            retained = Path(directory) / 'receipt.json'
            receipt = {}
            # Cleanup itself runs; simulate the verifier observing a remaining path.
            with patch.object(module.os.path, 'lexists', return_value=True):
                with self.assertRaisesRegex(ValueError, 'cleanup was not verified'):
                    with module.owned_snapshot(receipt, lambda: retained.write_text(json.dumps(receipt))) as snapshot:
                        module.claim_build_owner(snapshot)
            stored = json.loads(retained.read_text())
            self.assertFalse(stored['build_owner_cleanup']['verified'])
            self.assertFalse(module.os.path.lexists(snapshot))

    def test_failed_pre_cleanup_receipt_retention_preserves_owned_artifacts(self):
        with tempfile.TemporaryDirectory() as directory:
            retained = Path(directory) / 'receipt.json'
            receipt = {}
            calls = []

            def save():
                calls.append('save')
                if len(calls) == 1:
                    raise OSError('forced receipt retention failure')
                retained.write_text(json.dumps(receipt))

            with patch.object(module.shutil, 'rmtree') as remove:
                with self.assertRaisesRegex(ValueError, 'pre-cleanup receipt retention failed'):
                    with module.owned_snapshot(receipt, save) as snapshot:
                        owner = module.claim_build_owner(snapshot)
                        receipt['build_owner'] = owner.verify()
                        markers = [owner.target / 'retained-artifact', owner.build / 'retained-intermediate']
                        for marker in markers:
                            marker.write_bytes(b'owned evidence')
                remove.assert_not_called()
            self.assertEqual(len(calls), 2)
            stored = json.loads(retained.read_text())
            cleanup = stored['build_owner_cleanup']
            self.assertFalse(cleanup['receipt_retained_before_removal'])
            self.assertFalse(cleanup['root_identity_verified_before_removal'])
            self.assertFalse(cleanup['verified'])
            self.assertFalse(cleanup['snapshot_absent'])
            self.assertFalse(cleanup['target_absent'])
            self.assertFalse(cleanup['build_absent'])
            self.assertEqual([marker.read_bytes() for marker in markers], [b'owned evidence'] * 2)
            self.assertEqual(module.directory_identity(snapshot.path), stored['snapshot_owner'])
            # The fixture started no processes and still owns this exact root.
            shutil.rmtree(snapshot.path)


if __name__ == '__main__':
    unittest.main()
