import importlib.util
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import Mock, patch

TOOL = Path(__file__).resolve().parents[1] / 'ci_assurance.py'
ROOT = TOOL.parent.parent
spec = importlib.util.spec_from_file_location('ci_assurance', TOOL)
ci = importlib.util.module_from_spec(spec); spec.loader.exec_module(ci)

class SelectionTests(unittest.TestCase):
    def test_runtime_compiler_store_and_pipeline_paths_select_their_obligations(self):
        for path, required in {
            'crates/kernel/mainframe-env-interpreter/src/machine.rs': {'runtime','architecture'},
            'crates/kernel/mainframe-env-compiler/src/hir.rs': {'compiler','architecture'},
            'crates/stores/mainframe-env-store/src/durable.rs': {'store','runtime'},
            'Jenkinsfile': ci.ALL,
            'tools/jenkins/disk_guard.py': ci.ALL,
            'conformance/subsystems/jes/evidence/receipt.json': ci.ALL,
            'docs/contracts/effect-canonical-v1.md': {ci.DOCS},
            'docs/architecture/RUNTIME.md': {ci.DOCS},
        }.items():
            with self.subTest(path=path): self.assertTrue(required <= set(ci.obligations([path])))
    def test_shared_contracts_cannot_select_an_empty_or_partial_set(self):
        for path in ['crates/contracts/mainframe-env-host-api/src/request.rs','crates/foundation/mainframe-env-ir/src/lib.rs','Cargo.lock','Cargo.toml','conformance/spec/manifest.json']:
            self.assertEqual(set(ci.obligations([path])), ci.ALL)
    def test_cics_pilot_manifest_review_fixture_observation_and_provider_paths_are_selected(self):
        for path, required in {
            'conformance/subsystems/cics/application/manifests/cics-file-uow-topics.json': ci.ALL,
            'conformance/subsystems/cics/application/cics/pilot-rule-review.json': ci.ALL,
            'conformance/subsystems/cics/application/cics/pilot-fixtures.json': ci.ALL,
            'conformance/subsystems/cics/application/cobol/move-rule-review.json': ci.ALL,
            'conformance/subsystems/cics/application/cobol/move-fixture.json': ci.ALL,
            'conformance/subsystems/cics/application/oracles/cics-licensed-differential.json': ci.ALL,
            'crates/tooling/mainframe-env-conformance/src/cics_pilot.rs': {'architecture','evidence','runtime'},
            'crates/tooling/mainframe-env-conformance/src/cobol_move_pilot.rs': {'architecture','evidence','runtime'},
            'crates/tooling/mainframe-env-conformance/src/cics_licensed.rs': {'architecture','evidence','runtime'},
            'crates/providers/mainframe-env-cics/src/service.rs': {'architecture','evidence','store','runtime','mutation'},
            'crates/contracts/mainframe-env-coverage/src/conformance.rs': ci.ALL,
            'docs/architecture/CONFORMANCE-IR.md': {ci.DOCS},
        }.items():
            with self.subTest(path=path):
                self.assertTrue(set(required) <= set(ci.obligations([path])))
    def test_markdown_selects_only_the_bounded_docs_gate_but_mixed_changes_are_not(self):
        for path in ['README.md','docs/research/market.md','docs/runbooks/start.md','docs/delivery/hardening/notes.md']:
            self.assertEqual(ci.obligations([path]), [ci.DOCS])
            self.assertIn('runtime',ci.obligations([path,'crates/apps/server/src/main.rs']))
    def test_unknown_files_default_to_all_and_code_under_docs_is_not_prose(self):
        for path in ['new-system/input.xyz','docs/runbooks/check.py']:
            self.assertEqual(set(ci.obligations([path])),ci.ALL)
        self.assertEqual(ci.obligations(['docs/new-normative/spec.md']), [ci.DOCS])
    def test_renamed_or_deleted_normative_paths_still_trigger(self):
        # --no-renames supplies both paths. Deletions use the same obligation mapping.
        self.assertEqual(ci.obligations(['docs/contracts/OLD.md','docs/research/new.md']), [ci.DOCS])
    def test_paths_cannot_escape_the_repository(self):
        for path in ['', '../Cargo.toml','/tmp/file','docs/../../x','x\x00y','docs\\x']:
            with self.assertRaises(ValueError): ci.obligations([path])
    @patch.object(ci,'identity',return_value={'candidate':'a'*40,'tree':'b'*40})
    def test_full_tiers_select_every_obligation(self,_):
        for event,ref in [('schedule','refs/heads/main'),('manual','refs/heads/main'),('tag','refs/tags/sandbox-checkpoint')]:
            p=ci.make_plan(Path('.'),{},event,ref)
            self.assertTrue(p['full']);self.assertTrue(p['msrv']);self.assertTrue(p['store'])
            self.assertTrue(set(ci.FULL)<=set(p['primary_gates']))
            self.assertIn('certification',p['primary_gates'])
            self.assertIn('supply-chain',p['primary_gates'])
            self.assertIn('cargo-deny',p['primary_gates'])
            self.assertIn('msrv',p['primary_gates'])
            self.assertIn('python-tooling-tests',p['primary_gates'])
            self.assertIn('api-docs',p['primary_gates'])

    @patch.object(ci,'identity',return_value={'candidate':'a'*40,'tree':'b'*40})
    def test_dependency_policy_blocks_even_a_prose_only_pull_request(self,_):
        with patch.object(ci.subprocess,'check_output',return_value=b'README.md\0'):
            plan=ci.make_plan(Path('.'),{},'pull_request','refs/pull/1/merge','c'*40)
        self.assertFalse(plan['build'])
        self.assertEqual(plan['primary_gates'],['supply-chain','cargo-deny','license-notices','docs'])

    def test_jenkins_enforces_dependency_policy_and_current_subsystem_checks(self):
        jenkins = (ROOT / 'Jenkinsfile').read_text()
        self.assertIn('--gate cargo-deny -- cargo deny check', jenkins)
        self.assertIn('--gate license-notices -- cargo xtask license-notices --check', jenkins)
        self.assertIn('--gate supply-chain', jenkins)
        self.assertIn('cargo +1.95.0 check --workspace --all-targets --all-features --locked', jenkins)
        self.assertNotIn('RELEASE_TAG', jenkins)
        self.assertNotIn('publish_release_assets.py', jenkins)
        self.assertIn('--gate conformance', jenkins)


    def test_jenkins_records_discovered_tooling_and_all_postgres_gates(self):
        root = Path(__file__).resolve().parents[2]
        pipeline = (root / 'Jenkinsfile').read_text()
        self.assertIn('--gate python-tooling-tests --expect-tests -- "$MAINFRAME_ENV_PYTHON" -B tools/run_tooling_tests.py', pipeline)
        self.assertIn('--gate api-docs -- "$MAINFRAME_ENV_PYTHON" -B tools/check_public_api_docs.py', pipeline)
        listed = subprocess.check_output(
            [root / 'tools/jenkins/postgres_parity.sh', 'list'], text=True
        ).splitlines()
        self.assertEqual(listed, [
            'postgres-move',
            'postgres-effect',
            'postgres-stale-effect-recovery',
            'postgres-online-resume',
            'postgres-atomic-invariants',
            'postgres-work-leases',
            'postgres-storage-profile',
            'postgres-artifact-read-versions',
            'postgres-readiness',
            'postgres-retention',
            'postgres-durable',
            'postgres-carddemo-restart',
        ])
        postgres = (root / 'tools/jenkins/postgres_parity.sh').read_text()
        self.assertIn(
            'writable_probe_requires_provider_state_dml_and_rolls_everything_back',
            postgres,
        )
        self.assertIn('postgres_artifact_read_versions_are_compatible_and_fail_closed', postgres)
        self.assertIn('--gate "$gate" --expect-tests --min-tests 1 --', postgres)
        self.assertIn('mountpoint -q "$state"', postgres)
        self.assertIn('state="$state/parity"', postgres)
        self.assertIn('"$workspace/.postgres/"*', postgres)
        self.assertIn('postgres_parity.sh list', pipeline)

    @patch.object(ci,'identity',return_value={'candidate':'a'*40,'tree':'b'*40})
    @patch.object(ci.subprocess,'check_output',return_value=b'docs/README.md\0')
    def test_markdown_only_plan_runs_docs_without_rust_build(self,_,__):
        plan=ci.make_plan(Path('.'),{},'pull_request','refs/pull/1/merge','b'*40)
        self.assertFalse(plan['build']);self.assertFalse(plan['msrv'])
        self.assertTrue(plan['docs'])
        self.assertEqual(plan['primary_gates'],['supply-chain','cargo-deny','license-notices','docs'])

    def test_jenkins_pull_request_context_uses_explicit_comparison_sha(self):
        context=ci.jenkins_context(Path('.'),{
            'JENKINS_URL':'http://127.0.0.1:8080/',
            'CHANGE_ID':'93',
            'BRANCH_NAME':'PR-93',
            'MAINFRAME_ENV_CI_BASE':'a'*40,
        })
        self.assertEqual(context,{
            'event':'pull_request',
            'ref':'refs/pull/93/merge',
            'base':'a'*40,
            'provider':'jenkins',
        })

    def test_jenkins_tag_and_timer_select_full_contexts(self):
        tag=ci.jenkins_context(Path('.'),{
            'JENKINS_HOME':'/capped/jenkins-home',
            'BRANCH_NAME':'sandbox-checkpoint',
            'TAG_NAME':'sandbox-checkpoint',
        })
        self.assertEqual((tag['event'],tag['ref'],tag['provider']),
                         ('tag','refs/tags/sandbox-checkpoint','jenkins'))
        timer=ci.jenkins_context(Path('.'),{
            'JENKINS_URL':'http://127.0.0.1:8080/',
            'BUILD_CAUSE':'TIMERTRIGGER',
            'BRANCH_NAME':'main',
        })
        self.assertEqual(timer['event'],'schedule')

    def test_jenkins_context_rejects_non_sha_comparison_base(self):
        with self.assertRaises(ValueError):
            ci.jenkins_context(Path('.'),{'JENKINS_HOME':'/capped'},requested_base='main~1')

    @patch.object(ci,'identity',return_value={'candidate':'a'*40,'tree':'b'*40})
    def test_merge_push_skips_msrv_but_manual_full_does_not(self,_):
        push=ci.make_plan(Path('.'),{'merge_commit':True},'push','refs/heads/main')
        full=ci.make_plan(Path('.'),{'merge_commit':True},'manual','refs/heads/main')
        self.assertFalse(push['msrv']);self.assertTrue(full['msrv'])
        self.assertNotIn('msrv',push['primary_gates'])
        self.assertIn('msrv',full['primary_gates'])
    @patch.object(ci,'identity',return_value={'candidate':'a'*40,'tree':'b'*40})
    def test_missing_diff_fails_closed_to_all_not_prose(self,_):
        p=ci.make_plan(Path('.'),{},'pull_request','refs/pull/1/merge')
        self.assertEqual(set(p['obligations']),ci.ALL)
    def test_missing_or_wrong_candidate_receipts_cannot_pass(self):
        with tempfile.TemporaryDirectory() as d:
            p={'candidate':'a'*40,'tree':'b'*40,'full':False}; root=Path(d)
            self.assertFalse(ci.summarize(p,root,['architecture-fast'])['selected_commands_passed'])
            ci.write_json(root/'architecture-fast.json',{'candidate':'c'*40,'tree':p['tree'],'status':'passed'})
            self.assertFalse(ci.summarize(p,root,['architecture-fast'])['selected_commands_passed'])
            ci.write_json(root/'architecture-fast.json',{'candidate':p['candidate'],'tree':p['tree'],'status':'passed'})
            report=ci.summarize(p,root,['architecture-fast'])
            self.assertTrue(report['selected_commands_passed']);self.assertFalse(report['release_acceptance'])
            self.assertEqual(report['unselected_full_gates'],ci.FULL)
    def test_empty_gate_list_is_not_full_assurance(self):
        p={'candidate':'a'*40,'tree':'b'*40,'full':True}
        self.assertFalse(ci.summarize(p,Path('.'),[])['selected_commands_passed'])
    def test_empty_gate_list_passes_an_explicit_no_build_plan(self):
        p={'candidate':'a'*40,'tree':'b'*40,'full':False,'build':False}
        report=ci.summarize(p,Path('.'),[])
        self.assertTrue(report['selected_commands_passed'])
        self.assertEqual(report['checks'],{})
    def test_real_git_diff_includes_deletions(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d)
            def git(*args): return subprocess.check_output(['git',*args],cwd=root,text=True).strip()
            git('init','-q');git('config','user.name','fixture');git('config','user.email','fixture@example.invalid')
            (root/'Cargo.toml').write_text('old');git('add','.');git('commit','-qm','old');base=git('rev-parse','HEAD')
            (root/'Cargo.toml').unlink();(root/'README.md').write_text('prose');git('add','-A');git('commit','-qm','new')
            p=ci.make_plan(root,{'pull_request':{'base':{'sha':base}}},'pull_request','refs/pull/1/merge')
            self.assertIn('Cargo.toml',p['paths']);self.assertEqual(set(p['obligations']),ci.ALL)
    @patch.object(ci,'identity',return_value={'candidate':'a'*40,'tree':'b'*40})
    @patch.object(ci.subprocess,'check_output',side_effect=lambda *args, **kw: '' if kw.get('text') else b'')
    def test_zero_selected_tests_never_receive_pass_credit(self,_,__):
        with tempfile.TemporaryDirectory() as d:
            code=ci.record(Path('.'),Path(d),'empty-test',[sys.executable,'-c','print("running 0 tests\\ntest result: ok. 0 passed; 0 failed;")'],True)
            self.assertNotEqual(code,0)
            self.assertEqual(json.loads((Path(d)/'empty-test.json').read_text())['status'],'failed')

    @patch.object(ci,'identity',return_value={'candidate':'a'*40,'tree':'b'*40})
    @patch.object(ci.subprocess,'check_output',side_effect=lambda *args, **kw: '' if kw.get('text') else b'')
    def test_tooling_runner_marker_receives_nonempty_test_credit(self,_,__):
        with tempfile.TemporaryDirectory() as d:
            code=ci.record(Path('.'),Path(d),'python-tooling-tests',[sys.executable,'-c',"print('tooling test result: ok. 7 executed; 1 skipped;')"],True)
            self.assertEqual(code,0)
            receipt=json.loads((Path(d)/'python-tooling-tests.json').read_text())
            self.assertEqual(receipt['observed_passed_tests'],7)

    @patch.object(ci,'identity',return_value={'candidate':'a'*40,'tree':'b'*40})
    @patch.object(ci.subprocess,'check_output',side_effect=lambda *args, **kw: '' if kw.get('text') else b'')
    def test_record_captures_jenkins_runner_identity(self,_,__):
        with tempfile.TemporaryDirectory() as d, patch.dict(ci.os.environ,{
            'JENKINS_URL':'http://127.0.0.1:8080/', 'NODE_NAME':'built-in',
            'JOB_NAME':'mainframe-env', 'BUILD_NUMBER':'7',
            'BUILD_URL':'http://127.0.0.1:8080/job/mainframe-env/7/'
        },clear=True):
            code=ci.record(Path('.'),Path(d),'runner',[sys.executable,'-c','pass'])
            runner=json.loads((Path(d)/'runner.json').read_text())['runner']
            self.assertEqual(code,0);self.assertEqual(runner['ci'],'jenkins')
            self.assertEqual((runner['job_name'],runner['build_number']),('mainframe-env','7'))

class CandidateCleanlinessTests(unittest.TestCase):
    def exercise(self, *, before=None, during=None, ignored=False):
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory)
            root = workspace / 'repo'
            root.mkdir()
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            subprocess.run(['git', '-C', str(root), 'config', 'user.name', 'Fixture'], check=True)
            subprocess.run(['git', '-C', str(root), 'config', 'user.email', 'fixture@example.test'], check=True)
            (root / 'tracked.rs').write_text('initial\n')
            (root / '.gitignore').write_text('target/\n')
            subprocess.run(['git', '-C', str(root), 'add', '.'], check=True)
            subprocess.run(['git', '-C', str(root), 'commit', '-qm', 'fixture'], check=True)
            if before == 'tracked':
                (root / 'tracked.rs').write_text('dirty\n')
            elif before == 'untracked':
                (root / 'untracked.rs').write_text('unreviewed\n')
            if ignored:
                (root / 'target').mkdir()
                (root / 'target/cache').write_text('generated\n')
            marker = workspace / 'executed'
            # Restoring a tracked file during the command cannot legitimize dirty input.
            script = "from pathlib import Path; import sys; Path(sys.argv[1]).write_text('yes'); "
            if before == 'tracked':
                script += "Path('tracked.rs').write_text('initial\\n'); "
            if during == 'untracked':
                script += "Path('new-source.rs').write_text('unreviewed'); "
            script += "print('test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s')"
            output = workspace / 'output'
            with patch.object(ci.sys, 'stdout', Mock(buffer=io.BytesIO())), \
                    patch.object(ci.shutil, 'which', return_value=None):
                code = ci.record(root, output, 'candidate', [sys.executable, '-c', script, str(marker)], True, 1)
            return code, marker.exists(), json.loads((output / 'candidate.json').read_text())

    def test_dirty_tracked_input_refuses_before_a_command_can_restore_it(self):
        code, executed, receipt = self.exercise(before='tracked')
        self.assertNotEqual(code, 0)
        self.assertFalse(executed)
        self.assertFalse(receipt['candidate_unchanged'])
        self.assertEqual(receipt['observed_passed_tests'], 0)

    def test_untracked_input_refuses_before_execution(self):
        code, executed, receipt = self.exercise(before='untracked')
        self.assertNotEqual(code, 0)
        self.assertFalse(executed)
        self.assertEqual(receipt['status'], 'failed')

    def test_command_that_adds_untracked_source_cannot_pass(self):
        code, executed, receipt = self.exercise(during='untracked')
        self.assertNotEqual(code, 0)
        self.assertTrue(executed)
        self.assertEqual(receipt['observed_passed_tests'], 1)
        self.assertFalse(receipt['candidate_unchanged'])

    def test_clean_candidate_allows_ignored_build_artifacts_and_external_output(self):
        code, executed, receipt = self.exercise(ignored=True)
        self.assertEqual(code, 0)
        self.assertTrue(executed)
        self.assertTrue(receipt['candidate_unchanged'])
        self.assertEqual(receipt['observed_passed_tests'], 1)


class TestFloorTests(unittest.TestCase):
    def record_log(self, log, *, exit_code=0, min_tests=260, expect_tests=False,
                   candidate_changed=False, candidate_dirty=False, via_cli=False):
        identities = [{'candidate': 'a' * 40, 'tree': 'b' * 40}] * 2
        if candidate_changed:
            identities[1] = {'candidate': 'c' * 40, 'tree': 'd' * 40}
        process = Mock(stdout=io.BytesIO(log), wait=Mock(return_value=exit_code))
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(ci, 'identity', side_effect=identities), \
                patch.object(ci.subprocess, 'Popen', return_value=process), \
                patch.object(ci.subprocess, 'check_output',
                             return_value=b' M tracked.py' if candidate_dirty else b''), \
                patch.object(ci.shutil, 'which', return_value=None), \
                patch.object(ci.sys, 'stdout', Mock(buffer=io.BytesIO())):
            output = Path(directory)
            if via_cli:
                arguments = ['ci_assurance.py', '--root', str(ROOT), 'record',
                             '--output', str(output), '--gate', 'tests', '--expect-tests',
                             '--min-tests', str(min_tests), '--', 'fixture-runner']
                with patch.object(ci.sys, 'argv', arguments):
                    code = ci.main()
            else:
                code = ci.record(ROOT, output, 'tests', ['fixture-runner'],
                                 expect_tests, min_tests=min_tests)
            return code, json.loads((output / 'tests.json').read_text())

    def test_workspace_floor_refuses_259_actual_passes(self):
        log = (b'running 300 tests\n'
               b'test result: ok. 259 passed; 0 failed; 41 ignored; 0 measured; '
               b'0 filtered out; finished in 0.05s\n')
        code, receipt = self.record_log(log)
        self.assertNotEqual(code, 0)
        self.assertEqual(receipt['status'], 'failed')
        self.assertEqual(receipt['observed_passed_tests'], 259)

    def test_workspace_floor_accepts_exactly_260_actual_passes(self):
        log = (b'test result: ok. 260 passed; 0 failed; 100 ignored; 0 measured; '
               b'10 filtered out; finished in 0.05s\n')
        code, receipt = self.record_log(log)
        self.assertEqual(code, 0)
        self.assertEqual(receipt['status'], 'passed')
        self.assertEqual(receipt['observed_passed_tests'], 260)
        self.assertEqual(receipt['minimum_passed_tests'], 260)
        self.assertFalse(receipt['full_assurance_credit'])
        self.assertEqual(receipt['licensed_credit'], 0)

    def test_floor_sums_multiline_binary_totals_without_ignored_credit(self):
        log = (b'test result: ok. 100 passed; 0 failed; 200 ignored; 0 measured; '
               b'0 filtered out; finished in 0.01s\n'
               b'Running integration tests\n'
               b'\x1b[32mtest result: ok. 160 passed; 0 failed; 0 ignored; 0 measured; '
               b'0 filtered out; finished in 0.02s\x1b[0m\n'
               b'test result: ok. 0 passed; 0 failed; 4 ignored; 0 measured; '
               b'0 filtered out; finished in 0.00s\n')
        code, receipt = self.record_log(log)
        self.assertEqual(code, 0)
        self.assertEqual(receipt['observed_passed_tests'], 260)

    def test_floor_refuses_zero_ignored_only_and_malformed_output(self):
        for log in [
            b'',
            b'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; '
            b'0 filtered out; finished in 0.00s\n',
            b'test result: ok. 0 passed; 0 failed; 260 ignored; 0 measured; '
            b'0 filtered out; finished in 0.00s\n',
            b'test result: ok. 260 passed;\n',
            b'test result: ok. many passed; 0 failed;\n',
            b'fixture says test result: ok. 260 passed; 0 failed; 0 ignored; '
            b'0 measured; 0 filtered out; finished in 0.00s\n',
        ]:
            with self.subTest(log=log):
                code, receipt = self.record_log(log)
                self.assertNotEqual(code, 0)
                self.assertEqual(receipt['status'], 'failed')

    def test_floor_refuses_failed_or_malformed_summary_after_valid_passes(self):
        passed = (b'test result: ok. 260 passed; 0 failed; 0 ignored; 0 measured; '
                  b'0 filtered out; finished in 0.01s\n')
        for tail in [
            b'test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; '
            b'0 filtered out; finished in 0.01s\n',
            b'test result: ok. 1 passed; 1 failed; 0 ignored; 0 measured; '
            b'0 filtered out; finished in 0.01s\n',
            b'test result: ok. broken passed;\n',
        ]:
            with self.subTest(tail=tail):
                code, receipt = self.record_log(passed + tail)
                self.assertNotEqual(code, 0)
                self.assertEqual(receipt['status'], 'failed')
        code, receipt = self.record_log(passed, exit_code=1)
        self.assertEqual(code, 1)
        self.assertEqual(receipt['status'], 'failed')

    def test_floor_preserves_candidate_binding(self):
        log = (b'test result: ok. 260 passed; 0 failed; 0 ignored; 0 measured; '
               b'0 filtered out; finished in 0.01s\n')
        for change in [{'candidate_changed': True}, {'candidate_dirty': True}]:
            with self.subTest(change=change):
                code, receipt = self.record_log(log, **change)
                self.assertNotEqual(code, 0)
                self.assertFalse(receipt['candidate_unchanged'])

    def test_floor_excludes_skipped_tooling_tests(self):
        for log, expected_count, expected_code in [
            (b'tooling test result: ok. 260 executed; 1 skipped;\n', 259, 1),
            (b'tooling test result: ok. 260 executed; 260 skipped;\n', 0, 1),
            (b'tooling test result: ok. 261 executed; 1 skipped; '
             b'3 python files; 1 shell test files; 4 shell syntax checks\n', 260, 0),
        ]:
            with self.subTest(log=log):
                code, receipt = self.record_log(log)
                self.assertEqual(code, expected_code)
                self.assertEqual(receipt['observed_passed_tests'], expected_count)

    def test_floor_cli_rejects_insufficient_output_and_accepts_threshold(self):
        for log, expected_code in [
            (b'test result: ok. 259 passed; 0 failed; 1 ignored; 0 measured; '
             b'0 filtered out; finished in 0.01s\n', 1),
            (b'test result: ok. 260 passed; 0 failed; 1 ignored; 0 measured; '
             b'0 filtered out; finished in 0.01s\n', 0),
        ]:
            with self.subTest(log=log):
                code, receipt = self.record_log(log, via_cli=True)
                self.assertEqual(code, expected_code)
                self.assertEqual(receipt['minimum_passed_tests'], 260)
                self.assertTrue(receipt['requires_nonempty_tests'])

    def test_invalid_floor_refuses_before_running_command(self):
        for minimum in [0, -1, True, 1.5, '260']:
            with self.subTest(minimum=minimum), \
                    patch.object(ci.subprocess, 'Popen') as process:
                with self.assertRaises(ValueError):
                    ci.record(ROOT, Path('.'), 'tests', ['fixture-runner'],
                              min_tests=minimum)
                process.assert_not_called()

    def test_optional_floor_preserves_focused_nonempty_tests(self):
        log = (b'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; '
               b'0 filtered out; finished in 0.01s\n')
        code, receipt = self.record_log(log, min_tests=None, expect_tests=True)
        self.assertEqual(code, 0)
        self.assertTrue(receipt['requires_nonempty_tests'])
        self.assertNotIn('minimum_passed_tests', receipt)

    def test_jenkins_requires_floor_only_for_existing_workspace_tests(self):
        pipeline = (ROOT / 'Jenkinsfile').read_text()
        self.assertIn('--gate tests --expect-tests --min-tests 260 -- cargo test '
                      '--workspace --all-features --locked --no-fail-fast', pipeline)
        self.assertEqual(pipeline.count('--min-tests'), 1)


LINUX_FENCE_AVAILABLE = (sys.platform == 'linux' and callable(getattr(os,'waitid',None))
                         and all(hasattr(os,name) for name in ['P_PID','WEXITED','WNOHANG','WNOWAIT'])
                         and Path('/proc/self/stat').is_file())


@unittest.skipUnless(LINUX_FENCE_AVAILABLE,'Linux non-reaping wait/procfs controls unavailable; zero credit')
class CommandSupervisionTests(unittest.TestCase):
    def setUp(self):
        try:
            ci._validate_limits(1,None)
        except ValueError as problem:
            self.skipTest(f'Linux native fence unavailable: {problem}; zero credit')

    def owned(self, script, *, timeout=0.3, limit=4096, separate=False, callback=None):
        chunks = []
        def observe(stream, data):
            chunks.append((stream, data))
            if callback:
                callback(stream, data)
        started = time.monotonic()
        code, error = ci._run_owned(
            [sys.executable, '-B', '-c', script], ROOT, observe,
            timeout_seconds=timeout, max_output_bytes=limit, separate_stderr=separate)
        self.assertLess(time.monotonic() - started, 5)
        return code, error, chunks

    def recorded(self, script, *, timeout=0.3, limit=4096, minimum=None, via_cli=False):
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(ci, 'identity', return_value={'candidate':'a'*40, 'tree':'b'*40}), \
                patch.object(ci.subprocess, 'check_output', return_value=b''), \
                patch.object(ci.shutil, 'which', return_value=None), \
                patch.object(ci.sys, 'stdout', Mock(buffer=io.BytesIO())):
            output = Path(directory)
            command = [sys.executable, '-B', '-c', script]
            if via_cli:
                argv = ['ci_assurance.py', '--root', str(ROOT), 'record', '--output', directory,
                        '--gate', 'bounded', '--timeout-seconds', str(timeout),
                        '--max-output-bytes', str(limit), '--', *command]
                with patch.object(ci.sys, 'argv', argv):
                    code = ci.main()
            else:
                code = ci.record(ROOT, output, 'bounded', command, minimum is not None, minimum,
                                 timeout_seconds=timeout, max_output_bytes=limit)
            return code, json.loads((output/'bounded.json').read_bytes()), (output/'bounded.log').read_bytes()

    def test_native_positive_closed_stdin_and_exact_merged_bytes(self):
        code, error, chunks = self.owned(
            "import os; assert os.read(0,1)==b''; os.write(1,b'A'); os.write(2,b'B'); os.write(1,b'C')",
            timeout=1)
        self.assertEqual(code, 0)
        self.assertIsNone(error)
        self.assertTrue(all(stream=='stdout' for stream,data in chunks))
        self.assertEqual(b''.join(data for _, data in chunks), b'ABC')

    def test_native_separate_streams_keep_labels_and_per_stream_bytes(self):
        code, error, chunks = self.owned(
            "import os; os.write(2,b'error'); os.write(1,b'output')", timeout=1, separate=True)
        self.assertEqual(code, 0)
        self.assertIsNone(error)
        self.assertEqual(b''.join(data for stream,data in chunks if stream=='stdout'), b'output')
        self.assertEqual(b''.join(data for stream,data in chunks if stream=='stderr'), b'error')

    def test_native_silent_process_cannot_outlive_deadline(self):
        code, error, chunks = self.owned('import time; time.sleep(1.5)')
        self.assertNotEqual(code, 0)
        self.assertIn('deadline', error)
        self.assertEqual(chunks, [])

    def test_native_no_newline_partial_bytes_survive_timeout(self):
        code, error, chunks = self.owned("import os,time; os.write(1,b'partial'); time.sleep(1.5)")
        self.assertNotEqual(code, 0)
        self.assertIn('deadline', error)
        self.assertEqual(b''.join(data for _,data in chunks), b'partial')

    def test_native_closed_pipes_do_not_skip_exit_deadline(self):
        code, error, chunks = self.owned('import os,time; os.close(1); os.close(2); time.sleep(1.5)')
        self.assertNotEqual(code, 0)
        self.assertIn('deadline', error)
        self.assertEqual(chunks, [])

    def test_native_exact_byte_boundary_passes_but_one_more_fails(self):
        for length in [64,65]:
            with self.subTest(length=length):
                code, error, chunks = self.owned(f"import os; os.write(1,b'x'*{length})", limit=64, timeout=1)
                self.assertEqual(b''.join(data for _,data in chunks), b'x'*64)
                if length==64:
                    self.assertEqual(code, 0); self.assertIsNone(error)
                else:
                    self.assertIn('output limit', error)

    def test_native_both_streams_share_ceiling_without_newlines(self):
        code, error, chunks = self.owned(
            "import os; os.write(1,b'x'*40); os.write(2,b'y'*40)", limit=64, timeout=1)
        self.assertIn('output limit', error)
        self.assertEqual(b''.join(data for _,data in chunks), b'x'*40+b'y'*24)

    def test_native_ignored_term_requires_kill_and_launcher_wait(self):
        code, error, chunks = self.owned(
            "import os,signal,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); "
            "os.write(1,str(os.getpid()).encode()); time.sleep(1.5)")
        self.assertEqual(code, -signal.SIGKILL)
        self.assertIn('deadline', error)
        pid = int(b''.join(data for _,data in chunks))
        with self.assertRaises(ProcessLookupError): os.kill(pid,0)

    def test_native_forked_child_is_terminated_and_reaped_by_launcher(self):
        script = """import os,signal,time
child=os.fork()
if child==0:
    time.sleep(1.5)
    os._exit(0)
def stop(signum,frame):
    os.waitpid(child,0)
    raise SystemExit(0)
signal.signal(signal.SIGTERM,stop)
os.write(1,str(child).encode())
time.sleep(1.5)
"""
        code, error, chunks = self.owned(script)
        self.assertEqual(code, 0)
        self.assertIn('deadline', error)
        child = int(b''.join(data for _,data in chunks))
        with self.assertRaises(ProcessLookupError): os.kill(child,0)

    def test_native_observer_failure_and_cancellation_retain_partial_bytes(self):
        for problem in [OSError('log failed'), KeyboardInterrupt()]:
            with self.subTest(problem=type(problem).__name__):
                def fail(stream,data): raise problem
                code, error, chunks = self.owned(
                    "import os,time; os.write(1,b'saved'); time.sleep(1.5)", callback=fail)
                self.assertNotEqual(code,0)
                self.assertIsNotNone(error)
                self.assertEqual(b''.join(data for _,data in chunks), b'saved')

    def test_native_bounded_mode_restores_signal_handlers(self):
        previous = {sig:signal.getsignal(sig) for sig in [signal.SIGINT,signal.SIGTERM]}
        self.owned('pass', timeout=1)
        self.assertEqual({sig:signal.getsignal(sig) for sig in previous}, previous)

    def test_invalid_limits_refuse_before_popen(self):
        for timeout in [0,-1,True,float('nan'),float('inf'),1e300,10**1000,'1']:
            with self.subTest(timeout=str(timeout)[:40]), patch.object(ci.subprocess,'Popen') as spawn:
                with self.assertRaises(ValueError):
                    ci.record(ROOT, Path('.'), 'bounded', ['never'], timeout_seconds=timeout)
                spawn.assert_not_called()
        for limit in [0,-1,True,1.5,float('inf'),sys.maxsize+1,'10']:
            with self.subTest(limit=limit), patch.object(ci.subprocess,'Popen') as spawn:
                with self.assertRaises(ValueError):
                    ci.record(ROOT, Path('.'), 'bounded', ['never'], timeout_seconds=1, max_output_bytes=limit)
                spawn.assert_not_called()

    def test_output_option_requires_deadline_before_popen(self):
        with patch.object(ci.subprocess,'Popen') as spawn:
            with self.assertRaises(ValueError):
                ci.record(ROOT,Path('.'),'bounded',['never'],max_output_bytes=64)
            spawn.assert_not_called()

    def test_native_cli_options_and_unchanged_receipt_shape(self):
        code, receipt, log = self.recorded("import os; os.write(1,b'raw\\x00bytes')", timeout=1, via_cli=True)
        self.assertEqual(code,0)
        self.assertEqual(log,b'raw\x00bytes')
        self.assertEqual(receipt['schema_version'],'mainframe-env.ci-command@1')
        self.assertFalse(receipt['full_assurance_credit'])
        self.assertEqual(receipt['licensed_credit'],0)
        self.assertNotIn('timeout_seconds', receipt)
        self.assertNotIn('max_output_bytes', receipt)

    def test_native_summary_prefix_cannot_pass_after_overflow(self):
        summary=b'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n'
        code, receipt, log = self.recorded(
            f'import os; os.write(1,{summary!r}); os.write(2,b"x"*5000)', timeout=1, limit=256, minimum=1)
        self.assertNotEqual(code,0)
        self.assertEqual(receipt['status'],'failed')
        self.assertEqual(receipt['observed_passed_tests'],1)
        self.assertIn('output limit',receipt['error'])
        self.assertEqual(log,summary+b'x'*(256-len(summary)))

    def test_native_bounded_floor_counts_complete_summaries_and_no_skips(self):
        summary=b'tooling test result: ok. 2 executed; 1 skipped;\n'
        for minimum in [1,2]:
            with self.subTest(minimum=minimum):
                code, receipt, log = self.recorded(f'import os; os.write(1,{summary!r})', timeout=1, minimum=minimum)
                self.assertEqual(receipt['observed_passed_tests'],1)
                self.assertEqual(code,0 if minimum==1 else 1)
                self.assertEqual(log,summary)

    def test_native_partial_raw_record_is_not_replaced_by_timeout_error(self):
        code, receipt, log = self.recorded("import os,time; os.write(2,b'raw partial'); time.sleep(1.5)")
        self.assertNotEqual(code,0)
        self.assertEqual(log,b'raw partial')
        self.assertIn('deadline',receipt['error'])

    def test_nonposix_bounded_mode_refuses_before_launch(self):
        with patch.object(ci.os,'name','nt'), patch.object(ci.subprocess,'Popen') as spawn:
            with self.assertRaises(ValueError):
                ci._run_owned(['never'],ROOT,lambda stream,data:None,timeout_seconds=1)
            spawn.assert_not_called()

    def test_zero_exit_with_remaining_group_is_failure_and_only_owned_group_is_signalled(self):
        read_fd, write_fd = os.pipe()
        os.close(write_fd)
        process = Mock(pid=987654321, stdout=os.fdopen(read_fd,'rb'), stderr=None)
        process.wait.return_value = 0
        live = True
        signals = []
        def killpg(group,sig):
            nonlocal live
            self.assertEqual(group,process.pid)
            if not live: raise ProcessLookupError()
            if sig:
                signals.append(sig)
                live=False
        state=Mock(si_pid=process.pid,si_code=os.CLD_EXITED,si_status=0)
        def observe(kind,pid,flags):
            if pid==os.getpid(): raise ChildProcessError()
            return state
        with patch.object(ci.subprocess,'Popen',return_value=process), \
                patch.object(ci.os,'waitid',side_effect=observe), \
                patch.object(ci,'_linux_group_has_members',side_effect=lambda group:live), \
                patch.object(ci.os,'killpg',side_effect=killpg):
            code,error=ci._run_owned(['fixture'],ROOT,lambda stream,data:None,timeout_seconds=1)
        self.assertEqual(code,0)
        self.assertIn('group',error)
        self.assertEqual(signals,[signal.SIGTERM])
        process.wait.assert_called()
        self.assertTrue(process.stdout.closed)

    def test_wait_failure_cannot_be_relabelled_zero_exit(self):
        read_fd,write_fd=os.pipe(); os.close(write_fd)
        process=Mock(pid=987654321,stdout=os.fdopen(read_fd,'rb'),stderr=None)
        process.wait.side_effect=OSError('wait failed')
        state=Mock(si_pid=process.pid,si_code=os.CLD_EXITED,si_status=0)
        def observe(kind,pid,flags):
            if pid==os.getpid(): raise ChildProcessError()
            return state
        with patch.object(ci.subprocess,'Popen',return_value=process), \
                patch.object(ci.os,'waitid',side_effect=observe), \
                patch.object(ci,'_linux_group_has_members',return_value=False), \
                patch.object(ci.os,'killpg',side_effect=ProcessLookupError()):
            code,error=ci._run_owned(['fixture'],ROOT,lambda stream,data:None,timeout_seconds=1)
        self.assertNotEqual(code,0)
        self.assertIn('wait',error)
        self.assertTrue(process.stdout.closed)

    def test_popen_failure_is_failed_record_with_no_test_credit(self):
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(ci,'identity',return_value={'candidate':'a'*40,'tree':'b'*40}), \
                patch.object(ci.subprocess,'check_output',return_value=b''), \
                patch.object(ci.shutil,'which',return_value=None), \
                patch.object(ci.subprocess,'Popen',side_effect=OSError('missing executable')):
            code=ci.record(ROOT,Path(directory),'bounded',['absent'],True,1,timeout_seconds=1)
            receipt=json.loads((Path(directory)/'bounded.json').read_bytes())
        self.assertNotEqual(code,0)
        self.assertIn('missing executable',receipt['error'])
        self.assertEqual(receipt['observed_passed_tests'],0)

    def test_native_launcher_zero_exit_with_inherited_pipe_cannot_pass(self):
        # Linux test-only adoption lets the test reap the orphan it deliberately
        # creates. Production owns POSIX groups, not a global child subreaper.
        import ctypes
        libc = ctypes.CDLL(None, use_errno=True)
        prior = ctypes.c_int()
        if not hasattr(libc,'prctl') or libc.prctl(37,ctypes.byref(prior),0,0,0)!=0:
            self.skipTest('Linux subreaper query unavailable; zero native orphan credit')
        if libc.prctl(36,1,0,0,0)!=0:
            self.skipTest('Linux subreaper adoption unavailable; zero native orphan credit')
        child = None
        try:
            code,error,chunks = self.owned("""import os,time
child=os.fork()
if child==0:
    time.sleep(1.5)
    os._exit(0)
os.write(1,str(child).encode())
os._exit(0)
""", timeout=1)
            child = int(b''.join(data for _,data in chunks))
            self.assertEqual(code,0)
            self.assertIn('group',error)
            waited,status = os.waitpid(child,0)
            self.assertEqual(waited,child)
            self.assertTrue(os.WIFSIGNALED(status))
            self.assertIn(os.WTERMSIG(status),[signal.SIGTERM,signal.SIGKILL])
            with self.assertRaises(ProcessLookupError): os.kill(child,0)
            child=None
        finally:
            if child is not None:
                os.waitpid(child,0)
            self.assertEqual(libc.prctl(36,prior.value,0,0,0),0)

    def test_native_signal_cancellation_keeps_receipt_partial_bytes_and_reaps(self):
        previous=signal.getsignal(signal.SIGTERM)
        script="import os,signal,time; os.write(1,str(os.getpid()).encode()); os.kill(os.getppid(),signal.SIGTERM); time.sleep(1.5)"
        code,receipt,log=self.recorded(script,timeout=1)
        self.assertNotEqual(code,0)
        self.assertEqual(receipt['status'],'failed')
        self.assertIn('cancelled',receipt['error'])
        with self.assertRaises(ProcessLookupError): os.kill(int(log),0)
        self.assertIs(signal.getsignal(signal.SIGTERM),previous)

    def test_native_continuous_no_newline_saturation_is_bounded(self):
        code,error,chunks=self.owned("import os\nwhile True: os.write(2,b'x'*4096)",limit=512,timeout=1)
        self.assertNotEqual(code,0)
        self.assertIn('output limit',error)
        self.assertLess(len(error),500)
        self.assertEqual(b''.join(data for _,data in chunks),b'x'*512)

    def test_native_fragmented_and_final_no_newline_summaries_keep_floor(self):
        summary=b'tooling test result: ok. 3 executed; 1 skipped;'
        script=f"import os,time; os.write(1,{summary[:9]!r}); time.sleep(.02); os.write(1,{summary[9:]!r})"
        code,receipt,log=self.recorded(script,timeout=1,minimum=2)
        self.assertEqual(code,0)
        self.assertEqual(receipt['observed_passed_tests'],2)
        self.assertEqual(log,summary)

    def test_native_truncated_no_newline_summary_has_zero_credit(self):
        summary=b'tooling test result: ok. 3 executed; 1 skipped;'
        code,receipt,log=self.recorded(f'import os; os.write(1,{summary!r})',timeout=1,limit=20,minimum=1)
        self.assertNotEqual(code,0)
        self.assertEqual(receipt['observed_passed_tests'],0)
        self.assertEqual(log,summary[:20])

    def test_native_oversized_line_after_valid_summary_does_not_weaken_floor(self):
        summary=b'tooling test result: ok. 1 executed; 0 skipped;\n'
        script=f'import os; os.write(1,{summary!r}); os.write(1,b"test result:"+b"x"*66000+b"\\n")'
        code,receipt,log=self.recorded(script,timeout=1,limit=70000,minimum=1)
        self.assertNotEqual(code,0)
        self.assertEqual(receipt['status'],'failed')
        self.assertEqual(receipt['observed_passed_tests'],1)
        self.assertIsNotNone(receipt['error'])
        self.assertEqual(log,summary+b'test result:'+b'x'*66000+b'\n')

    def test_caller_or_broadcast_group_is_never_probed_or_signalled(self):
        for pid in [0,-1,1,os.getpid(),os.getpgrp()]:
            with self.subTest(pid=pid):
                read_fd,write_fd=os.pipe(); os.close(write_fd)
                process=Mock(pid=pid,stdout=os.fdopen(read_fd,'rb'),stderr=None)
                process.wait.return_value=0
                with patch.object(ci.subprocess,'Popen',return_value=process), patch.object(ci.os,'killpg') as kill:
                    code,error=ci._run_owned(['fixture'],ROOT,lambda stream,data:None,timeout_seconds=1)
                self.assertIsNotNone(error)
                self.assertIn('unsafe',error)
                kill.assert_not_called()
                process.wait.assert_called()
                self.assertTrue(process.stdout.closed)

    def test_native_separate_streams_still_share_one_output_ceiling(self):
        code,error,chunks=self.owned("import os; os.write(1,b'x'*50); os.write(2,b'y'*50)",
                                     limit=64,timeout=1,separate=True)
        self.assertIn('output limit',error)
        self.assertEqual(sum(len(data) for _,data in chunks),64)
        self.assertTrue(all(data == (b'x' if stream=='stdout' else b'y')*len(data) for stream,data in chunks))


class CommandSupervisionReviewTests(unittest.TestCase):
    def setUp(self):
        if not LINUX_FENCE_AVAILABLE and self._testMethodName != 'test_unsupported_platform_or_missing_waitnowait_refuses_before_launch':
            self.skipTest('Linux lifetime/native controls unavailable; zero credit')
        if self._testMethodName != 'test_unsupported_platform_or_missing_waitnowait_refuses_before_launch':
            try:
                ci._validate_limits(1,None)
            except ValueError as problem:
                self.skipTest(f'Linux lifetime fence unavailable: {problem}; zero credit')

    def test_dirty_real_candidate_replaces_old_success_log_before_any_launch(self):
        import hashlib
        old=b'tooling test result: ok. 260 executed; 0 skipped;\n'
        with tempfile.TemporaryDirectory() as directory:
            workspace=Path(directory)
            root=workspace/'repo'; root.mkdir()
            subprocess.run(['git','init','-q',str(root)],check=True)
            subprocess.run(['git','-C',str(root),'config','user.name','Fixture'],check=True)
            subprocess.run(['git','-C',str(root),'config','user.email','fixture@example.test'],check=True)
            (root/'tracked').write_text('clean\n')
            subprocess.run(['git','-C',str(root),'add','.'],check=True)
            subprocess.run(['git','-C',str(root),'commit','-qm','fixture'],check=True)
            for dirty in ['tracked','untracked']:
                with self.subTest(dirty=dirty):
                    (root/dirty).write_text('dirty\n')
                    output=workspace/'output'; output.mkdir(exist_ok=True)
                    log=output/'bounded.log'; log.write_bytes(old)
                    launches=[]
                    original_spawn=subprocess.Popen
                    def spawn(command,**kwargs):
                        if command[0]!='git':
                            launches.append(command)
                            raise AssertionError('dirty candidate must not launch a command')
                        return original_spawn(command,**kwargs)
                    with patch.object(ci.subprocess,'Popen',side_effect=spawn), \
                            patch.object(ci.shutil,'which',return_value=None):
                        code=ci.record(root,output,'bounded',['never'],True,1,timeout_seconds=1)
                    receipt=json.loads((output/'bounded.json').read_bytes())
                    self.assertEqual(launches,[])
                    self.assertNotEqual(code,0)
                    self.assertEqual(receipt['status'],'failed')
                    self.assertEqual(receipt['observed_passed_tests'],0)
                    self.assertEqual(log.read_bytes(),(receipt['error']+'\n').encode())
                    self.assertEqual(receipt['log_sha256'],hashlib.sha256(log.read_bytes()).hexdigest())
                    (root/'tracked').write_text('clean\n')
                    if dirty=='untracked': (root/'untracked').unlink()

    def test_log_open_failure_replaces_prior_bytes_without_launch(self):
        with tempfile.TemporaryDirectory() as directory:
            output=Path(directory); log=output/'bounded.log'
            log.write_bytes(b'old successful command\n')
            original_open=Path.open
            def open_file(path,mode='r',*args,**kwargs):
                if path==log and mode=='wb': raise OSError('current log open failed')
                return original_open(path,mode,*args,**kwargs)
            with patch.object(ci,'identity',return_value={'candidate':'a'*40,'tree':'b'*40}), \
                    patch.object(ci.subprocess,'check_output',return_value=b''), \
                    patch.object(ci.shutil,'which',return_value=None), \
                    patch.object(Path,'open',open_file), patch.object(ci.subprocess,'Popen') as spawn:
                code=ci.record(ROOT,output,'bounded',['never'],timeout_seconds=1)
                spawn.assert_not_called()
            receipt=json.loads((output/'bounded.json').read_bytes())
            self.assertNotEqual(code,0)
            self.assertEqual(log.read_bytes(),b'current log open failed\n')
            self.assertEqual(receipt['status'],'failed')

    def lifetime(self, *, members=False, wait_error=None, observation_error=None,
                 callback_error=False, membership_error=None, inherited_pipe=False):
        from types import SimpleNamespace
        read_fd,write_fd=os.pipe()
        os.write(write_fd,b'actual partial')
        if not inherited_pipe: os.close(write_fd)
        process=Mock(pid=987654321,stdout=os.fdopen(read_fd,'rb'),stderr=None)
        events=[]
        reaped=False
        live=members
        def poll():
            nonlocal reaped
            events.append('poll-reap'); reaped=True
            return 0
        def observe(kind,pid,flags):
            if pid==os.getpid():
                events.append('capability-probe')
                raise ChildProcessError()
            events.append('observe')
            self.assertEqual(kind,os.P_PID); self.assertEqual(pid,process.pid)
            self.assertTrue(flags & os.WNOWAIT)
            self.assertFalse(reaped)
            if observation_error: raise observation_error
            return SimpleNamespace(si_pid=pid,si_code=os.CLD_EXITED,si_status=0)
        def group(pgid):
            events.append('group-probe')
            self.assertEqual(pgid,process.pid); self.assertFalse(reaped)
            if membership_error: raise membership_error
            return live
        def kill(pgid,sig):
            nonlocal live
            events.append('group-probe' if sig==0 else 'group-signal')
            self.assertEqual(pgid,process.pid)
            # Model immediate reuse after any reaping operation. Looking up the
            # same numeric PGID cannot restore ownership of this unrelated group.
            if reaped: raise OSError('unrelated reused group must not be touched')
            if sig: live=False
        def wait(**kwargs):
            nonlocal reaped
            events.append('wait'); reaped=True
            if wait_error: raise wait_error
            return 0
        process.poll.side_effect=poll
        process.wait.side_effect=wait
        chunks=[]
        def output(stream,data):
            chunks.append(data)
            if callback_error: raise OSError('observer failed')
        try:
            with patch.object(ci.subprocess,'Popen',return_value=process), \
                    patch.object(ci.os,'waitid',side_effect=observe), \
                    patch.object(ci,'_linux_group_has_members',side_effect=group,create=True), \
                    patch.object(ci.os,'killpg',side_effect=kill):
                result=ci._run_owned(['fixture'],ROOT,output,timeout_seconds=.15)
        finally:
            process.stdout.close()
            if inherited_pipe: os.close(write_fd)
        self.assertNotIn('poll-reap',events)
        self.assertEqual(events.count('wait'),1)
        self.assertTrue(all(event not in ['observe','group-probe','group-signal'] for event in events[events.index('wait')+1:]),events)
        return result,events,chunks

    def test_retained_leader_alone_is_success_and_wait_is_last_group_operation(self):
        (code,error),events,chunks=self.lifetime()
        self.assertEqual(code,0); self.assertIsNone(error)
        self.assertIn('observe',events)
        self.assertEqual(b''.join(chunks),b'actual partial')

    def test_actual_remaining_members_fail_and_signals_precede_only_wait(self):
        (code,error),events,chunks=self.lifetime(members=True)
        self.assertEqual(code,0)
        self.assertIn('group',error)
        self.assertIn('group-signal',events)
        self.assertEqual(b''.join(chunks),b'actual partial')

    def test_failed_wait_never_uses_released_numeric_group(self):
        for problem in [OSError('wait failed'),subprocess.TimeoutExpired('fixture',.5),KeyboardInterrupt()]:
            with self.subTest(problem=type(problem).__name__):
                (code,error),events,chunks=self.lifetime(wait_error=problem)
                self.assertNotEqual(code,0)
                self.assertIn('wait',error)

    def test_callback_failure_finishes_signalling_before_wait(self):
        (code,error),events,chunks=self.lifetime(callback_error=True)
        self.assertIn('observer failed',error)
        self.assertEqual(b''.join(chunks),b'actual partial')

    def test_lost_child_fence_refuses_all_further_group_access(self):
        (code,error),events,chunks=self.lifetime(observation_error=ChildProcessError('lost child fence'))
        self.assertIn('lost child fence',error)
        self.assertNotIn('group-probe',events)
        self.assertNotIn('group-signal',events)

    def test_unsupported_platform_or_missing_waitnowait_refuses_before_launch(self):
        for platform,waitid in [('darwin',getattr(os,'waitid',None)),('linux',None)]:
            with self.subTest(platform=platform), patch.object(ci.sys,'platform',platform), \
                    patch.object(ci.os,'waitid',waitid,create=True), patch.object(ci.subprocess,'Popen') as spawn:
                with self.assertRaises(ValueError):
                    ci._run_owned(['never'],ROOT,lambda stream,data:None,timeout_seconds=1)
                spawn.assert_not_called()

    def test_inherited_pipe_deadline_retains_leader_until_all_group_work_finishes(self):
        (code,error),events,chunks=self.lifetime(inherited_pipe=True)
        self.assertEqual(code,0)
        self.assertIn('deadline',error)
        self.assertIn('group-signal',events)
        self.assertEqual(b''.join(chunks),b'actual partial')

    def test_uncertain_membership_fails_before_single_wait(self):
        (code,error),events,chunks=self.lifetime(membership_error=OSError('incomplete membership census'))
        self.assertIn('incomplete membership census',error)
        self.assertIn('group-signal',events)

    def test_linux_census_excludes_only_retained_leader_and_detects_zombie_member(self):
        from contextlib import contextmanager
        from types import SimpleNamespace
        @contextmanager
        def scan(root):
            self.assertEqual(root,'/proc')
            yield iter([SimpleNamespace(name='987654321',path='/proc/987654321'),
                        SimpleNamespace(name='42',path='/proc/42'),SimpleNamespace(name='self')])
        for group,expected in [(987654321,True),(1,False)]:
            with self.subTest(group=group), patch.object(ci.os,'scandir',scan), \
                    patch.object(Path,'read_bytes',return_value=f'42 (a ) name) Z 1 {group} 0'.encode()) as read:
                self.assertEqual(ci._linux_group_has_members(987654321),expected)
                self.assertEqual(read.call_count,1)

    def test_missing_membership_stat_is_unavailable_not_empty(self):
        from contextlib import contextmanager
        from types import SimpleNamespace
        @contextmanager
        def scan(root): yield iter([SimpleNamespace(name='42',path='/proc/42')])
        with patch.object(ci.os,'scandir',scan),patch.object(Path,'read_bytes',side_effect=FileNotFoundError('vanished')):
            with self.assertRaises(OSError): ci._linux_group_has_members(987654321)

    def test_autoreaping_sigchld_refuses_before_launch(self):
        with patch.object(ci.signal,'getsignal',return_value=signal.SIG_IGN),patch.object(ci.subprocess,'Popen') as spawn:
            with self.assertRaises(ValueError):
                ci._run_owned(['never'],ROOT,lambda stream,data:None,timeout_seconds=1)
            spawn.assert_not_called()

    def test_native_escaped_session_survives_deadline_without_group_signal(self):
        import ctypes
        libc=ctypes.CDLL(None,use_errno=True)
        prior=ctypes.c_int()
        if not hasattr(libc,'prctl') or libc.prctl(37,ctypes.byref(prior),0,0,0)!=0:
            self.skipTest('Linux subreaper query unavailable; zero native escape credit')
        if libc.prctl(36,1,0,0,0)!=0:
            self.skipTest('Linux subreaper adoption unavailable; zero native escape credit')
        child=None
        signals=[]
        original=ci.os.killpg
        def kill(group,sig):
            signals.append((group,sig))
            return original(group,sig)
        try:
            with patch.object(ci.os,'killpg',side_effect=kill):
                code,error,chunks=CommandSupervisionTests().owned("""import os,time
read,write=os.pipe()
child=os.fork()
if child==0:
    os.close(read)
    os.setsid()
    os.write(1,str(os.getpid()).encode())
    os.write(write,b'ready')
    os.close(write)
    time.sleep(.8)
    os._exit(0)
os.close(write)
os.read(read,1)
os._exit(0)
""",timeout=.15)
            child=int(b''.join(data for _,data in chunks))
            self.assertEqual(code,0)
            self.assertIn('deadline',error)
            self.assertEqual(os.getpgid(child),child)
            self.assertTrue(all(group!=child for group,sig in signals))
            waited,status=os.waitpid(child,0)
            self.assertEqual(waited,child)
            self.assertEqual(os.waitstatus_to_exitcode(status),0)
            with self.assertRaises(ProcessLookupError): os.kill(child,0)
            child=None
        finally:
            if child is not None: os.waitpid(child,0)
            self.assertEqual(libc.prctl(36,prior.value,0,0,0),0)


if __name__=='__main__':unittest.main()
