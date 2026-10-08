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



# These literal expectations are independent of the command owner's builders.
PUBLIC_OPERATIONS = {
    'submit': ('zos-jobs submit local-file /input/public-client.jcl', 'IBMUSER', 'TESTPASS', None, None),
    'owner-list': ('zos-jobs list jobs --owner IBMUSER --prefix PBCLNT01', 'IBMUSER', 'TESTPASS', None, None),
    'status': ('zos-jobs view job-status-by-jobid JOB00001', 'IBMUSER', 'TESTPASS', 'JOB00001', None),
    'files': ('zos-jobs list spool-files-by-jobid JOB00001', 'IBMUSER', 'TESTPASS', 'JOB00001', None),
    'content': ('zos-jobs view spool-file-by-id JOB00001 0', 'IBMUSER', 'TESTPASS', 'JOB00001', '0'),
    'bad-password': ('zos-jobs submit local-file /input/public-client.jcl', 'IBMUSER', 'WRONGPASS', None, None),
    'other-status': ('zos-jobs view job-status-by-jobid JOB00001', 'OTHERUSR', 'OTHERPASS', 'JOB00001', None),
    'other-files': ('zos-jobs list spool-files-by-jobid JOB00001', 'OTHERUSR', 'OTHERPASS', 'JOB00001', None),
    'other-content': ('zos-jobs view spool-file-by-id JOB00001 0', 'OTHERUSR', 'OTHERPASS', 'JOB00001', '0'),
    'other-owner-list': ('zos-jobs list jobs --owner IBMUSER --prefix PBCLNT01', 'OTHERUSR', 'OTHERPASS', None, None),
}
PUBLIC_SEEDS = {
    'identity/passwd': b'agent:x:1000:1000:public-client:/client-home:/bin/false\n',
    'identity/group': b'agent:x:1000:\n',
    'identity/nsswitch.conf': b'passwd: files\ngroup: files\nhosts: files\n',
    'client-home/.zowe.env.json': b'{}\n',
    'plugins/plugins.json': b'{}\n',
    'client-home/settings/imperative.json': b'{"overrides":{"CredentialManager":false},"credentialManagerOptions":{}}\n',
    'input/public-client.jcl': b'//PBCLNT01 JOB CLASS=A\n//STEP1 EXEC PGM=IEFBR14\n',
}
PUBLIC_ROLES = ('node-archive', 'node', 'zowe-archive', 'bubblewrap', 'loader', 'libdl',
                'libstdcxx', 'libm', 'libgcc', 'libpthread', 'libc', 'libnss_files')


class PublicClientCommandTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='public-command-')
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve()
        self.parent = self.base / 'commands'; self.parent.mkdir(mode=0o700)
        self.run = self.parent / 'action'
        self.tree = self.base / 'tree'; (self.tree / 'package/lib').mkdir(parents=True)
        (self.tree / 'package/lib/main.js').write_bytes(b'inert\n')
        self.files = {}
        for role in PUBLIC_ROLES:
            path = self.base / role; path.write_bytes(b'inert\n'); path.chmod(0o600)
            self.files[role] = path
        self.bindings = [f'{role}={path}' for role, path in self.files.items()]

    def arguments(self, **changes):
        return dict(profile_files=self.bindings, profile_tree=self.tree, run_dir=self.run,
                    action='submit', port='1234', timeout_seconds=1, **changes)

    def command(self, **changes):
        args = self.arguments(); args.update(changes)
        return ci.public_client_command(ROOT, **args)

    def cli(self, extra=(), prefix=()):
        argv = ['ci_assurance.py', *prefix, 'public-client-command', '--development-profile',
                'public-client-linux-x86_64', '--profile-tree', str(self.tree),
                '--run-dir', str(self.run), '--action', 'submit', '--port', '1234',
                '--timeout-seconds', '1']
        for binding in self.bindings: argv += ['--profile-file', binding]
        with patch.object(sys, 'argv', argv + list(extra)):
            return ci.main()

    def inert(self, launch=None):
        from contextlib import ExitStack
        stack = ExitStack(); self.addCleanup(stack.close)
        supply = ci._public_client_supply_chain()
        # Exercise the real lock, role grammar and development validator; only
        # their external byte readers are inert. Never admit the retained client.
        stack.enter_context(patch.object(supply, 'verify_profile_file', return_value={'sha256': 'a'*64, 'bytes': 6, 'mode': 0o600}))
        stack.enter_context(patch.object(supply, 'validate_node_archive'))
        stack.enter_context(patch.object(supply, 'zowe_archive_rows', return_value={}))
        stack.enter_context(patch.object(supply, 'validate_zowe_tree', return_value={'sha256': 'b'*64, 'files': 1, 'bytes': 6}))
        # POST keeps real checked-input contexts open; these existing inert
        # controls mock only their byte readers, not command/session admission.
        from contextlib import contextmanager
        @contextmanager
        def inert_input(path, pin):
            yield io.BytesIO(b'inert\n'), Mock(st_mode=0o600)
        stack.enter_context(patch.object(supply, 'checked_input', side_effect=inert_input))
        stack.enter_context(patch.object(supply, '_verify_profile_source',
            side_effect=lambda path, source, metadata, pin: supply.verify_profile_file(path, pin)))
        stack.enter_context(patch.object(ci, '_public_client_identity'))
        def complete(command, root, output, **kwargs):
            self.assertEqual(root, self.run / 'cwd')
            self.assertEqual(kwargs['env'], {})
            self.assertEqual(kwargs['max_output_bytes'], 131072)
            self.assertTrue(kwargs['separate_stderr'])
            output('stdout', b'\xff{partial\x00'); output('stderr', b'\x80warning\n')
            kwargs['on_reaped'](1)
            return 1, None
        spawn = stack.enter_context(patch.object(ci, '_run_owned', side_effect=launch or complete))
        return supply, spawn

    def test_ten_handwritten_action_vectors(self):
        for action, (operation, user, password, job, file) in PUBLIC_OPERATIONS.items():
            with self.subTest(action=action):
                self.assertEqual(ci._public_client_operation(action, '1234', job, file),
                    operation.split() + ['--host', '127.0.0.1', '--port', '1234', '--protocol', 'http',
                    '--user', user, '--password', password, '--completion-timeout', '5',
                    '--establish-connection-timeout', '5', '--response-format-json'])

    def test_unknown_and_surplus_actions_refuse(self):
        for action in ('version', 'shell', 'unknown', 'submit --help', ''):
            with self.subTest(action=action), self.assertRaises(ValueError):
                ci._public_client_operation(action, '1234', None, None)
        for action, (_, _, _, job, file) in PUBLIC_OPERATIONS.items():
            for bad_job, bad_file in ((None, file), (job, None), ('JOB00001', '0')):
                if (bad_job, bad_file) == (job, file): continue
                with self.subTest(action=action, job=bad_job, file=bad_file), self.assertRaises(ValueError):
                    ci._public_client_operation(action, '1234', bad_job, bad_file)

    def test_scalar_refusals(self):
        for port in ('0', '65536', '+1', '-1', '01', ' 1', '1\n', '١', '1.0', 1, True):
            with self.subTest(port=port), self.assertRaises(ValueError):
                ci._public_client_operation('submit', port, None, None)
        for job in ('JOB00000', 'JOB1', 'JOB000001', 'JOB100000000', 'JOB0000١', '../JOB00001', 'JOB00001/x', '--help', 'JOB00001\n'):
            with self.subTest(job=job), self.assertRaises(ValueError):
                ci._public_client_operation('status', '1', job, None)
        for file in ('-1', '64', '00', '+0', '٠', 'SP-1', '/tmp/a', '0\n'):
            with self.subTest(file=file), self.assertRaises(ValueError):
                ci._public_client_operation('content', '1', 'JOB00001', file)

    def test_canonical_scalar_boundaries(self):
        for port in ('1', '65535'):
            for job in ('JOB00001', 'JOB99999999'):
                for file in ('0', '63'):
                    result = ci._public_client_operation('content', port, job, file)
                    self.assertEqual(result[:5], ['zos-jobs', 'view', 'spool-file-by-id', job, file])
                    self.assertEqual(result[result.index('--port')+1], port)

    def test_timeout_refuses_before_allocation(self):
        self.inert()
        for timeout in (float('nan'), float('inf'), 0, -1, 10.01, True, '1', 10**400):
            with self.subTest(timeout=timeout), self.assertRaises(ValueError):
                self.command(timeout_seconds=timeout)
            self.assertFalse(self.run.exists())

    def test_cli_unknown_remainder_duplicate_and_profile_refuse(self):
        self.inert()
        for extra in (['--port', '1234'], ['--port=1234'], ['--action', 'status'],
                      ['--timeout-seconds', '1'], ['--run-dir', str(self.run)],
                      ['--profile-tree', str(self.tree)], ['--development-profile', 'other'],
                      ['--env', 'PATH=x'], ['--', 'echo'], ['echo'], ['--po', '1234']):
            with self.subTest(extra=extra), patch('sys.stderr', new=io.StringIO()), self.assertRaises(SystemExit):
                self.cli(extra)
            self.assertFalse(self.run.exists())

    def test_cli_selected_surface_invokes_one_transport(self):
        _, spawn = self.inert()
        self.assertEqual(self.cli(), 0); spawn.assert_called_once()

    def test_bindings_refuse_before_launch(self):
        supply, spawn = self.inert()
        for bindings in (self.bindings[:-1], self.bindings + [self.bindings[0]],
                         self.bindings[:-1] + ['extra=/tmp/x'], self.bindings[:-1] + ['libnss_files=relative']):
            with self.subTest(bindings=bindings), self.assertRaises((ValueError, supply.SupplyChainError)):
                self.command(profile_files=bindings)
            self.assertFalse(self.run.exists()); spawn.assert_not_called()

    def test_alternate_root_refuses_without_loading_other_policy(self):
        supply, spawn = self.inert()
        with patch.object(supply, 'validate_ci_lock', wraps=supply.validate_ci_lock) as validator:
            with self.assertRaises(ValueError): ci.public_client_command(self.base, **self.arguments())
            validator.assert_not_called(); spawn.assert_not_called()
        with patch('sys.stderr', new=io.StringIO()), self.assertRaises(SystemExit):
            self.cli(prefix=['--root', str(ROOT), '--root', str(ROOT)])

    def test_input_error_is_forwarded_from_actual_validator(self):
        supply, spawn = self.inert()
        with patch.object(supply, 'verify_profile_file', side_effect=supply.SupplyChainError('inert digest differs')):
            with self.assertRaisesRegex(supply.SupplyChainError, 'digest differs'): self.command()
        self.assertFalse(self.run.exists()); spawn.assert_not_called()

    def test_run_leaf_ownership_and_overlap_refuse(self):
        _, spawn = self.inert()
        existing = self.parent / 'existing'; existing.mkdir()
        linked = self.base / 'linked'; linked.symlink_to(self.parent, target_is_directory=True)
        for run in (existing, linked / 'new', self.tree / 'new', ROOT / 'new',
                    self.parent / '..' / 'new', Path('/'), Path.home(), self.parent / 'bad\nname'):
            with self.subTest(run=run), self.assertRaises(ValueError): self.command(run_dir=run)
        self.parent.chmod(0o777)
        with self.assertRaises(ValueError): self.command()
        self.parent.chmod(0o700); spawn.assert_not_called()

    def test_fixed_mount_environment_and_node_vector(self):
        _, spawn = self.inert(); self.assertEqual(self.command(), 0)
        expected = [str(self.files['bubblewrap']), '--unshare-user', '--uid', '1000', '--gid', '1000',
                    '--unshare-pid', '--die-with-parent', '--new-session', '--cap-drop', 'ALL', '--clearenv',
                    '--proc', '/proc', '--dev', '/dev', '--tmpfs', '/tmp', '--dir', '/etc', '--dir', '/opt/node/bin',
                    '--dir', '/lib64', '--dir', '/lib/x86_64-linux-gnu']
        mounts = [('node', '/opt/node/bin/node'), ('loader', '/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2'),
                  ('loader', '/lib64/ld-linux-x86-64.so.2'), ('libdl', '/lib/x86_64-linux-gnu/libdl.so.2'),
                  ('libstdcxx', '/lib/x86_64-linux-gnu/libstdc++.so.6'), ('libm', '/lib/x86_64-linux-gnu/libm.so.6'),
                  ('libgcc', '/lib/x86_64-linux-gnu/libgcc_s.so.1'), ('libpthread', '/lib/x86_64-linux-gnu/libpthread.so.0'),
                  ('libc', '/lib/x86_64-linux-gnu/libc.so.6'), ('libnss_files', '/lib/x86_64-linux-gnu/libnss_files.so.2')]
        for role, target in mounts: expected += ['--ro-bind', str(self.files[role]), target]
        for source, target in [('identity/passwd', '/etc/passwd'), ('identity/group', '/etc/group'),
                               ('identity/nsswitch.conf', '/etc/nsswitch.conf'), ('input', '/input')]:
            expected += ['--ro-bind', str(self.run / source), target]
        expected += ['--dir', '/opt/client/node_modules/@zowe', '--ro-bind',
                     str(self.tree / 'package'), '/opt/client/node_modules/@zowe/cli']
        for source, target in [('client-home', '/client-home'), ('plugins', '/plugins'), ('cwd', '/work')]:
            expected += ['--bind', str(self.run / source), target]
        expected += ['--chdir', '/work', '--setenv', 'PATH', '/opt/node/bin', '--setenv', 'TMPDIR', '/tmp',
                     '--setenv', 'ZOWE_CLI_HOME', '/client-home', '--setenv', 'ZOWE_CLI_PLUGINS_DIR', '/plugins',
                     '/opt/node/bin/node', '--no-addons', '--no-global-search-paths',
                     '/opt/client/node_modules/@zowe/cli/lib/main.js',
                     'zos-jobs', 'submit', 'local-file', '/input/public-client.jcl', '--host', '127.0.0.1',
                     '--port', '1234', '--protocol', 'http', '--user', 'IBMUSER', '--password', 'TESTPASS',
                     '--completion-timeout', '5', '--establish-connection-timeout', '5', '--response-format-json']
        self.assertEqual(spawn.call_args.args[0], expected)

    def test_exact_seeds_private_modes_and_fresh_capture(self):
        self.inert(); self.assertEqual(self.command(), 0)
        for name, data in PUBLIC_SEEDS.items():
            self.assertEqual((self.run / name).read_bytes(), data)
            self.assertEqual((self.run / name).stat().st_mode & 0o7777, 0o600)
        for name in ('identity', 'client-home', 'plugins', 'cwd', 'input', 'client-home/settings', 'client-home/logs'):
            self.assertEqual((self.run / name).stat().st_mode & 0o7777, 0o700)
        self.assertEqual(self.run.stat().st_mode & 0o7777, 0o700)
        for name in ('stdout.bin', 'stderr.bin', 'child-exit.txt', 'supervision-error.txt', 'input-lock.sha256'):
            self.assertEqual((self.run / name).stat().st_mode & 0o7777, 0o600)
        with self.assertRaises(ValueError): self.command()

    def test_raw_binary_nonzero_exit_is_transport_complete(self):
        self.inert(); self.assertEqual(self.command(), 0)
        self.assertEqual((self.run / 'stdout.bin').read_bytes(), b'\xff{partial\x00')
        self.assertEqual((self.run / 'stderr.bin').read_bytes(), b'\x80warning\n')
        self.assertEqual((self.run / 'child-exit.txt').read_bytes(), b'1\n')
        self.assertEqual((self.run / 'supervision-error.txt').read_bytes(), b'')

    def test_each_stream_boundary_and_overflow_prefix(self):
        for stream in ('stdout', 'stderr'):
            for extra in (0, 1):
                self.run = self.parent / f'{stream}-{extra}'
                def launch(command, root, output, **kwargs):
                    error = None
                    try: output(stream, b'\xff'*65536 + b'x'*extra)
                    except ValueError as problem: error = str(problem)
                    kwargs['on_reaped'](-15 if extra else 0)
                    return (-15 if extra else 0), error
                self.inert(launch)
                with self.subTest(stream=stream, extra=extra):
                    self.assertEqual(self.command(), extra)
                    self.assertEqual((self.run / (stream + '.bin')).read_bytes(), b'\xff'*65536)
                    self.assertEqual((self.run / 'child-exit.txt').read_bytes(), b'-15\n' if extra else b'0\n')
                    self.assertEqual(bool((self.run / 'supervision-error.txt').read_bytes()), bool(extra))

    def test_both_full_streams_fit_shared_limit(self):
        def launch(command, root, output, **kwargs):
            output('stdout', b'a'*65536); output('stderr', b'b'*65536)
            kwargs['on_reaped'](0); return 0, None
        self.inert(launch); self.assertEqual(self.command(), 0)
        self.assertEqual((self.run / 'stdout.bin').stat().st_size + (self.run / 'stderr.bin').stat().st_size, 131072)

    def test_no_actual_reap_never_manufactures_exit(self):
        for error in ('Popen failed', 'launcher wait failed'):
            self.run = self.parent / error.replace(' ', '-')
            self.inert(lambda *args, **kwargs: (127, error))
            self.assertEqual(self.command(), 1)
            self.assertEqual((self.run / 'child-exit.txt').read_bytes(), b'')
            self.assertIn(error.encode(), (self.run / 'supervision-error.txt').read_bytes())

    def test_actual_127_and_timeout_status_are_retained(self):
        for code, error, outer in ((127, None, 0), (-15, 'command deadline exceeded', 1)):
            self.run = self.parent / str(code)
            def launch(*args, **kwargs): kwargs['on_reaped'](code); return code, error
            self.inert(launch); self.assertEqual(self.command(), outer)
            self.assertEqual((self.run / 'child-exit.txt').read_bytes(), f'{code}\n'.encode())

    def test_postcheck_seed_membership_links_and_log_caps_fail(self):
        mutations = []
        for name in ('identity/passwd', 'input/public-client.jcl', 'client-home/settings/imperative.json', 'plugins/plugins.json'):
            mutations.append(lambda name=name: (self.run / name).write_bytes(b'changed'))
        mutations += [lambda: (self.run / 'client-home/team.json').write_bytes(b'{}'),
                      lambda: (self.run / 'cwd/helper').write_bytes(b'native'),
                      lambda: (self.run / 'plugins/link').symlink_to(self.files['node']),
                      lambda: (self.run / 'client-home/logs/zowe.log').write_bytes(b'x'*(1048576+1))]
        for index, mutate in enumerate(mutations):
            self.run = self.parent / f'mutation-{index}'
            def launch(*args, **kwargs):
                args[2]('stdout', b'partial'); mutate(); kwargs['on_reaped'](1); return 1, None
            self.inert(launch)
            with self.subTest(index=index):
                self.assertEqual(self.command(), 1)
                self.assertEqual((self.run / 'stdout.bin').read_bytes(), b'partial')
                self.assertTrue((self.run / 'supervision-error.txt').read_bytes())

    def test_two_normal_bounded_regular_logs_are_allowed(self):
        def launch(*args, **kwargs):
            for name in ('imperative.log', 'zowe.log'):
                (self.run / 'client-home/logs' / name).write_bytes(b'x'*1048576)
            kwargs['on_reaped'](0); return 0, None
        self.inert(launch); self.assertEqual(self.command(), 0)

    def test_fresh_validator_postcheck_changed_role_or_tree_fails(self):
        supply, spawn = self.inert()
        for owner in ('verify_profile_file', 'validate_zowe_tree'):
            self.run = self.parent / owner
            original = getattr(supply, owner).return_value
            def launch(*args, **kwargs):
                getattr(supply, owner).return_value = {**original, 'sha256': 'c'*64}
                kwargs['on_reaped'](0); return 0, None
            spawn.side_effect = launch
            self.assertEqual(self.command(), 1)
            getattr(supply, owner).return_value = original

    def test_lock_hash_change_and_postvalidator_failure_fail(self):
        supply, spawn = self.inert()
        original = ci._public_client_lock_bytes
        def read(root):
            data = original(root)
            if spawn.called: return data + b' '
            return data
        with patch.object(ci, '_public_client_lock_bytes', read): self.assertEqual(self.command(), 1)
        self.run = self.parent / 'post-input-error'
        def launch(*args, **kwargs):
            supply.verify_profile_file.side_effect = supply.SupplyChainError('input changed')
            kwargs['on_reaped'](0); return 0, None
        spawn.side_effect = launch
        self.assertEqual(self.command(), 1)

    def test_precheck_and_capture_write_errors_refuse(self):
        self.inert()
        with patch.object(ci, '_public_client_check_state', side_effect=OSError('state unavailable')):
            self.assertEqual(self.command(), 1)
        self.run = self.parent / 'write-error'
        original = ci.os.open
        def opening(path, *args, **kwargs):
            if path == self.run / 'stdout.bin': raise OSError('capture unavailable')
            return original(path, *args, **kwargs)
        with patch.object(ci.os, 'open', opening): self.assertEqual(self.command(), 1)

    def test_supervision_diagnostics_are_bounded_ascii_and_no_response_summary(self):
        def launch(*args, **kwargs):
            args[2]('stdout', b'test result: ok. 999 passed;\n')
            kwargs['on_reaped'](-15); return -15, '\u2603'*5000
        self.inert(launch)
        with patch('sys.stdout', new=io.StringIO()) as console:
            self.assertEqual(self.command(), 1); self.assertEqual(console.getvalue(), '')
        data = (self.run / 'supervision-error.txt').read_bytes()
        self.assertLessEqual(len(data), 4096); data.decode('ascii')

    def test_selected_empty_environment_ignores_poison_and_auxiliary_files(self):
        _, spawn = self.inert()
        (self.base / 'argv.json').write_bytes(b'["--bind","/","/"]')
        (self.base / 'settings.json').write_bytes(b'{"CredentialManager":"native"}')
        with patch.dict(os.environ, {'PATH': '/poison', 'LD_PRELOAD': '/poison', 'NODE_OPTIONS': '--require /poison',
                                     'ZOWE_CLI_HOME': '/poison', 'HOME': '/poison', 'CODEX_HOME': '/poison'}):
            self.assertEqual(self.command(), 0)
        self.assertEqual(spawn.call_args.kwargs['env'], {})
        self.assertNotIn(str(self.base / 'argv.json'), spawn.call_args.args[0])

    def test_run_raw_noncanonical_paths_and_missing_parent_refuse(self):
        _, spawn = self.inert()
        for run in ('relative', str(self.parent) + '//new', str(self.parent) + '/./new',
                    str(self.parent) + '/bad\x00name', str(self.parent) + '/bad\x7fname',
                    str(self.parent) + '/' + 'a'*4097, str(self.base / 'missing/new')):
            with self.subTest(run=run), self.assertRaises((ValueError, OSError)):
                self.command(run_dir=run)
        spawn.assert_not_called()

    def test_seed_precheck_refuses_modified_bytes_before_launch(self):
        _, spawn = self.inert()
        original = ci._prepare_public_client_run
        def prepare(run):
            original(run); (run / 'identity/passwd').write_bytes(b'changed')
        with patch.object(ci, '_prepare_public_client_run', prepare):
            self.assertEqual(self.command(), 1)
        spawn.assert_not_called()

    def test_raw_and_exit_write_failure_preserve_transport_failure(self):
        for name in ('stdout.bin', 'child-exit.txt'):
            self.run = self.parent / name
            _, spawn = self.inert()
            original = ci._public_client_exclusive
            def opening(path):
                output = original(path)
                if path.name == name:
                    broken = Mock(wraps=output)
                    broken.write.side_effect = OSError('write unavailable')
                    return broken
                return output
            def launch(command, root, output, **kwargs):
                error = None
                try:
                    output('stdout', b'partial')
                    kwargs['on_reaped'](17)
                except OSError:
                    error = 'exit capture failed' if name == 'child-exit.txt' else 'raw capture failed'
                return 17, error
            spawn.side_effect = launch
            with self.subTest(name=name), patch.object(ci, '_public_client_exclusive', opening):
                self.assertEqual(self.command(), 1)
            self.assertEqual((self.run / 'child-exit.txt').read_bytes(), b'')
            self.assertTrue((self.run / 'supervision-error.txt').read_bytes())

    def test_postcheck_all_seeds_modes_and_special_files_refuse(self):
        mutations = [lambda name=name: (self.run / name).write_bytes(b'changed') for name in PUBLIC_SEEDS]
        mutations += [lambda: (self.run / 'input/public-client.jcl').chmod(0o644),
                      lambda: (self.run / 'client-home').chmod(0o777),
                      lambda: os.mkfifo(self.run / 'client-home/logs/zowe.log'),
                      lambda: (self.run / 'client-home/logs/other.log').write_bytes(b'x'),
                      lambda: os.link(self.files['node'], self.run / 'client-home/logs/zowe.log')]
        for index, mutate in enumerate(mutations):
            self.run = self.parent / f'extra-state-{index}'
            def launch(*args, **kwargs): mutate(); kwargs['on_reaped'](0); return 0, None
            self.inert(launch)
            with self.subTest(index=index): self.assertEqual(self.command(), 1)

    def test_each_cli_scalar_duplicate_and_unknown_credential_surface_refuses(self):
        _, spawn = self.inert()
        for option, value in (('job-id', 'JOB00001'), ('file-id', '0'), ('development-profile', 'public-client-linux-x86_64')):
            with self.subTest(option=option), patch('sys.stderr', new=io.StringIO()), self.assertRaises(SystemExit):
                self.cli(['--' + option, value, '--' + option, value])
        for option in ('host', 'user', 'password', 'mount', 'settings', 'command', 'output', 'environment'):
            with self.subTest(option=option), patch('sys.stderr', new=io.StringIO()), self.assertRaises(SystemExit):
                self.cli(['--' + option, 'anything'])
        spawn.assert_not_called(); self.assertFalse(self.run.exists())

    def test_duplicate_exit_callback_refuses_and_preserves_first_status(self):
        def launch(*args, **kwargs):
            kwargs['on_reaped'](1)
            try: kwargs['on_reaped'](0)
            except ValueError: return 1, 'exit capture failed'
            return 0, None
        self.inert(launch); self.assertEqual(self.command(), 1)
        self.assertEqual((self.run / 'child-exit.txt').read_bytes(), b'1\n')


class PublicClientActionLayoutTests(unittest.TestCase):
    # Reuse only inert fixture setup/transport; expectations below are handwritten.
    setUp = PublicClientCommandTests.setUp
    arguments = PublicClientCommandTests.arguments
    command = PublicClientCommandTests.command
    inert = PublicClientCommandTests.inert

    def launch_mutation(self, mutate, expected):
        def launch(command, root, output, **kwargs):
            output('stdout', b'partial\x00'); output('stderr', b'MODULE_NOT_FOUND\xff')
            mutate(); kwargs['on_reaped'](1); return 1, None
        self.inert(launch)
        self.assertEqual(self.command(), expected)
        self.assertEqual((self.run / 'child-exit.txt').read_bytes(), b'1\n')
        self.assertEqual((self.run / 'stdout.bin').read_bytes(), b'partial\x00')
        self.assertEqual((self.run / 'stderr.bin').read_bytes(), b'MODULE_NOT_FOUND\xff')
        self.assertEqual(bool((self.run / 'supervision-error.txt').read_bytes()), bool(expected))

    def write_log(self, name, count, mode=0o600):
        path = self.run / 'client-home/logs' / name
        path.write_bytes(b'x' * count); path.chmod(mode)

    def test_exact_single_local_install_full_vector(self):
        _, spawn = self.inert(); self.assertEqual(self.command(), 0)
        expected = [str(self.files['bubblewrap']), '--unshare-user', '--uid', '1000', '--gid', '1000',
                    '--unshare-pid', '--die-with-parent', '--new-session', '--cap-drop', 'ALL', '--clearenv',
                    '--proc', '/proc', '--dev', '/dev', '--tmpfs', '/tmp', '--dir', '/etc', '--dir', '/opt/node/bin',
                    '--dir', '/lib64', '--dir', '/lib/x86_64-linux-gnu']
        for role, target in [('node', '/opt/node/bin/node'), ('loader', '/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2'),
                             ('loader', '/lib64/ld-linux-x86-64.so.2'), ('libdl', '/lib/x86_64-linux-gnu/libdl.so.2'),
                             ('libstdcxx', '/lib/x86_64-linux-gnu/libstdc++.so.6'), ('libm', '/lib/x86_64-linux-gnu/libm.so.6'),
                             ('libgcc', '/lib/x86_64-linux-gnu/libgcc_s.so.1'), ('libpthread', '/lib/x86_64-linux-gnu/libpthread.so.0'),
                             ('libc', '/lib/x86_64-linux-gnu/libc.so.6'), ('libnss_files', '/lib/x86_64-linux-gnu/libnss_files.so.2')]:
            expected += ['--ro-bind', str(self.files[role]), target]
        for source, target in [('identity/passwd', '/etc/passwd'), ('identity/group', '/etc/group'),
                               ('identity/nsswitch.conf', '/etc/nsswitch.conf'), ('input', '/input')]:
            expected += ['--ro-bind', str(self.run / source), target]
        expected += ['--dir', '/opt/client/node_modules/@zowe', '--ro-bind',
                     str(self.tree / 'package'), '/opt/client/node_modules/@zowe/cli']
        for source, target in [('client-home', '/client-home'), ('plugins', '/plugins'), ('cwd', '/work')]:
            expected += ['--bind', str(self.run / source), target]
        expected += ['--chdir', '/work', '--setenv', 'PATH', '/opt/node/bin', '--setenv', 'TMPDIR', '/tmp',
                     '--setenv', 'ZOWE_CLI_HOME', '/client-home', '--setenv', 'ZOWE_CLI_PLUGINS_DIR', '/plugins',
                     '/opt/node/bin/node', '--no-addons', '--no-global-search-paths',
                     '/opt/client/node_modules/@zowe/cli/lib/main.js',
                     'zos-jobs', 'submit', 'local-file', '/input/public-client.jcl', '--host', '127.0.0.1',
                     '--port', '1234', '--protocol', 'http', '--user', 'IBMUSER', '--password', 'TESTPASS',
                     '--completion-timeout', '5', '--establish-connection-timeout', '5', '--response-format-json']
        self.assertEqual(spawn.call_args.args[0], expected)
        self.assertEqual(spawn.call_args.kwargs['env'], {})

    def test_debug_log_zero_length_is_optional_valid_regular_file(self):
        self.launch_mutation(lambda: self.write_log('imperative_debug.log', 0), 0)

    def test_debug_log_exact_per_file_boundary_retains_failed_startup(self):
        self.launch_mutation(lambda: self.write_log('imperative_debug.log', 1048576), 0)

    def test_three_logs_exact_aggregate_boundary(self):
        def mutate():
            for name, count in [('imperative.log', 700000), ('zowe.log', 700000), ('imperative_debug.log', 697152)]:
                self.write_log(name, count)
        self.launch_mutation(mutate, 0)

    def test_three_logs_aggregate_plus_one_refuses(self):
        def mutate():
            for name, count in [('imperative.log', 700000), ('zowe.log', 700000), ('imperative_debug.log', 697153)]:
                self.write_log(name, count)
        self.launch_mutation(mutate, 1)
        self.assertIn(b'log total differs', (self.run / 'supervision-error.txt').read_bytes())

    def test_debug_log_per_file_plus_one_refuses(self):
        self.launch_mutation(lambda: self.write_log('imperative_debug.log', 1048577), 1)
        self.assertIn(b'inspection cap', (self.run / 'supervision-error.txt').read_bytes())

    def test_debug_log_names_outside_exact_membership_refuse(self):
        for index, name in enumerate(['client-home/logs/imperative_debug.log.1', 'client-home/logs/debug.log',
                                      'client-home/imperative_debug.log', 'cwd/imperative_debug.log']):
            self.run = self.parent / f'name-{index}'
            def mutate(name=name):
                path = self.run / name; path.write_bytes(b'x'); path.chmod(0o600)
            with self.subTest(name=name): self.launch_mutation(mutate, 1)

    def test_debug_log_symlink_hardlink_fifo_and_directory_refuse(self):
        for kind in ('symlink', 'hardlink', 'fifo', 'directory'):
            self.run = self.parent / kind
            def mutate(kind=kind):
                path = self.run / 'client-home/logs/imperative_debug.log'
                if kind == 'symlink': path.symlink_to(self.files['node'])
                elif kind == 'hardlink': os.link(self.files['node'], path)
                elif kind == 'fifo': os.mkfifo(path, 0o600)
                else: path.mkdir(mode=0o700)
            with self.subTest(kind=kind): self.launch_mutation(mutate, 1)

    def test_debug_log_writable_and_special_modes_refuse(self):
        for index, mode in enumerate((0o620, 0o602, 0o1600, 0o2600, 0o4600)):
            self.run = self.parent / f'mode-{index}-{mode:o}'
            with self.subTest(mode=mode):
                self.launch_mutation(lambda mode=mode: self.write_log('imperative_debug.log', 1, mode), 1)

    def test_debug_log_accepts_private_vendor_read_modes(self):
        for mode in (0o600, 0o640, 0o644):
            self.run = self.parent / f'read-mode-{mode:o}'
            with self.subTest(mode=mode):
                self.launch_mutation(lambda mode=mode: self.write_log('imperative_debug.log', 1, mode), 0)


class PublicClientAdmissionTests(unittest.TestCase):
    def identity(self, **changes):
        from contextlib import ExitStack
        with ExitStack() as stack:
            stack.enter_context(patch.object(ci.sys, 'platform', changes.get('platform', 'linux')))
            stack.enter_context(patch.object(ci.platform, 'machine', return_value=changes.get('machine', 'x86_64')))
            for name in ('getuid', 'geteuid', 'getgid', 'getegid'):
                stack.enter_context(patch.object(ci.os, name, return_value=changes.get(name, 1000)))
            stack.enter_context(patch.object(ci.os, 'getgroups', return_value=changes.get('groups', [1000])))
            stack.enter_context(patch.object(Path, 'read_text', return_value=changes.get('status',
                'CapEff:\t0000000000000000\nCapPrm:\t0000000000000000\nCapAmb:\t0000000000000000\n')))
            return ci._public_client_identity()

    def test_platform_identity_and_groups_are_refusals(self):
        self.identity()
        for key, value in (('platform', 'darwin'), ('machine', 'aarch64'), ('getuid', 0),
                           ('geteuid', 0), ('getgid', 0), ('getegid', 0), ('groups', [1000, 0])):
            with self.subTest(key=key), self.assertRaises(ValueError): self.identity(**{key: value})

    def test_each_actual_capability_set_missing_or_nonzero_refuses(self):
        base = 'CapEff:\t0000000000000000\nCapPrm:\t0000000000000000\nCapAmb:\t0000000000000000\n'
        for name in ('CapEff', 'CapPrm', 'CapAmb'):
            for status in (base.replace(name + ':\t0000000000000000', name + ':\t0000000000000001'),
                           base.replace(name + ':\t0000000000000000\n', ''), base + name + ':\t0\n'):
                with self.subTest(name=name, status=status), self.assertRaises(ValueError): self.identity(status=status)

    def test_bounded_parent_diagnostic_escapes_controls_and_credentials(self):
        data = ci._public_client_diagnostic('TESTPASS WRONGPASS OTHERPASS\x00\x1b\n\u2603')
        self.assertEqual(data, b'[redacted] [redacted] [redacted]\\x00\\x1b\\n\\u2603\n')


class PublicClientValidationCostTests(unittest.TestCase):
    def setUp(self):
        source = ROOT / 'tools/tests/test_supply_chain.py'
        fixture_spec = importlib.util.spec_from_file_location('cost_input_fixtures', source)
        module = importlib.util.module_from_spec(fixture_spec)
        fixture_spec.loader.exec_module(module)
        self.fixture = module.DevelopmentInputTests()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.supply = ci._public_client_supply_chain()
        self.lock = json.loads((ROOT / self.supply.CI_LOCK_PATH).read_text())
        self.lock['development_profiles']['public-client-linux-x86_64'] = self.fixture.profile
        self.lock_bytes = json.dumps(self.lock).encode()

    def command(self, run, launch=None):
        def complete(command, root, output, **kwargs):
            kwargs['on_reaped'](0)
            return 0, None
        with patch.object(self.supply, 'validate_ci_lock', return_value=self.lock), \
                patch.object(ci, '_public_client_lock_bytes', return_value=self.lock_bytes), \
                patch.object(ci, '_public_client_identity'), \
                patch.object(ci, '_run_owned', side_effect=launch or complete), \
                patch.object(ci.subprocess, 'Popen', side_effect=AssertionError('no child')):
            return ci.public_client_command(ROOT,
                profile_files=[f'{role}={path}' for role, path in self.fixture.files.items()],
                profile_tree=self.fixture.tree, run_dir=run, action='submit', port='1234', timeout_seconds=1)

    def test_only_pre_streaming_archive_parsers_and_every_action_starts_fresh(self):
        with patch.object(self.supply.tarfile, 'open', wraps=self.supply.tarfile.open) as opened:
            for name in ('one', 'two'):
                self.assertEqual(self.command(self.fixture.root / name), 0)
        modes = [call.kwargs.get('mode') for call in opened.call_args_list]
        self.assertEqual(modes, ['r|', 'r|', 'r|', 'r|'])

    def test_real_post_role_bytes_are_not_cached(self):
        def changed(command, root, output, **kwargs):
            self.fixture.files['node'].write_bytes(b'other node\n')
            kwargs['on_reaped'](0)
            return 0, None
        run = self.fixture.root / 'changed'
        self.assertEqual(self.command(run, changed), 1)
        self.assertIn(b'input postcheck failed', (run / 'supervision-error.txt').read_bytes())


class PublicClientTreeAuthorityCommandTests(unittest.TestCase):
    setUp = PublicClientValidationCostTests.setUp
    command = PublicClientValidationCostTests.command

    def test_every_action_resolves_root_manifest_and_closing_root_per_phase(self):
        self.assertEqual(self.fixture.payloads, {
            'package/package.json': b'{"name":"@zowe/cli","version":"8.39.0"}',
            'package/lib/main.js': b'fixture-only\n', 'package/empty': b''})
        original = self.supply.canonical_input
        names = []
        def observed(path):
            if path.is_relative_to(self.fixture.tree):
                names.append(path.relative_to(self.fixture.tree).as_posix())
            return original(path)
        with patch.object(self.supply, 'canonical_input', side_effect=observed):
            for name in ('first', 'second'):
                self.assertEqual(self.command(self.fixture.root / name), 0)
        self.assertEqual(names, ['.', 'package/package.json', '.'] * 4)


class PublicClientSupervisorKeywordTests(unittest.TestCase):
    def supervise(self, *, env=None, callback=None, code=17, wait_error=None):
        from types import SimpleNamespace
        fd, writer = os.pipe(); os.close(writer)
        process = Mock(pid=987654321, stdout=os.fdopen(fd, 'rb'), stderr=None)
        events = []
        def wait(**kwargs):
            events.append('wait')
            if wait_error: raise wait_error
            return code
        process.wait.side_effect = wait
        def capture(value):
            self.assertEqual(events[-1], 'wait'); events.append(('capture', value))
            if callback: callback(value)
        with patch.object(ci, '_validate_limits'), patch.object(ci.subprocess, 'Popen', return_value=process) as spawn, \
                patch.object(ci.os, 'waitid', return_value=SimpleNamespace(si_pid=process.pid, si_code=os.CLD_EXITED, si_status=code)), \
                patch.object(ci, '_linux_group_has_members', return_value=False), patch.object(ci.os, 'killpg') as kill:
            result = ci._run_owned(['inert'], ROOT, lambda *args: None, timeout_seconds=1,
                                   env=env, on_reaped=capture)
        process.wait.assert_called_once_with(timeout=.5)
        kill.assert_not_called()
        return result, events, spawn

    def test_empty_and_copied_environment_and_default_compatibility(self):
        result, events, spawn = self.supervise(env={})
        self.assertEqual(spawn.call_args.kwargs['env'], {})
        env = {'PATH': '/fixed'}
        _, _, spawn = self.supervise(env=env)
        self.assertEqual(spawn.call_args.kwargs['env'], env)
        self.assertIsNot(spawn.call_args.kwargs['env'], env)
        _, _, spawn = self.supervise()
        self.assertNotIn('env', spawn.call_args.kwargs)

    def test_successful_wait_callback_exception_preserves_actual_code(self):
        def broken(code): raise OSError('exit file unavailable')
        (code, error), events, _ = self.supervise(callback=broken)
        self.assertEqual(code, 17); self.assertIn('exit capture failed', error)
        self.assertNotIn('wait failed', error); self.assertEqual(events, ['wait', ('capture', 17)])

    def test_failed_wait_never_calls_capture(self):
        (code, error), events, _ = self.supervise(wait_error=OSError('wait unavailable'))
        self.assertEqual(code, 127); self.assertIn('wait failed', error); self.assertEqual(events, ['wait'])

    def test_invalid_environment_refuses_before_popen(self):
        for env in ({'a=b': 'x'}, {'': 'x'}, {'x': '\x00'}, {'x\x00': 'a'}, {'x': 1}, {1: 'x'}, []):
            with self.subTest(env=env), patch.object(ci.subprocess, 'Popen') as spawn, self.assertRaises(ValueError):
                ci._run_owned(['inert'], ROOT, lambda *args: None, timeout_seconds=1, env=env)
            spawn.assert_not_called()

    def test_popen_failure_never_calls_reaped_callback(self):
        captured = Mock()
        with patch.object(ci, '_validate_limits'), patch.object(ci.subprocess, 'Popen', side_effect=OSError('spawn unavailable')):
            code, error = ci._run_owned(['inert'], ROOT, lambda *args: None, timeout_seconds=1,
                                       env={}, on_reaped=captured)
        self.assertEqual(code, 127); self.assertIn('spawn unavailable', error); captured.assert_not_called()


class PublicClientNativeKeywordTests(unittest.TestCase):
    def test_native_empty_environment_excludes_parent_poison(self):
        output = []
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {
                'PUBLIC_CLIENT_POISON': 'parent', 'NODE_OPTIONS': 'poison', 'PATH': '/poison',
                'LD_PRELOAD': '/poison', 'ZOWE_CLI_HOME': '/poison', 'ZOWE_CLI_PLUGINS_DIR': '/poison'}):
            result = ci._run_owned([sys.executable, '-B', '-c',
                'import os,sys; sys.stdout.buffer.write(b"empty" if not set(os.environ) & '
                '{"PUBLIC_CLIENT_POISON","NODE_OPTIONS","PATH","LD_PRELOAD","ZOWE_CLI_HOME","ZOWE_CLI_PLUGINS_DIR"} else b"poison")'],
                Path(directory), lambda stream, data: output.append(data), timeout_seconds=2, env={})
        self.assertEqual(result, (0, None)); self.assertEqual(b''.join(output), b'empty')

    def test_native_postwait_callback_has_actual_status(self):
        captured = []
        with tempfile.TemporaryDirectory() as directory:
            result = ci._run_owned([sys.executable, '-B', '-c', 'raise SystemExit(23)'], Path(directory),
                lambda *args: None, timeout_seconds=2, on_reaped=captured.append)
        self.assertEqual(result, (23, None)); self.assertEqual(captured, [23])



class PublicClientParserDiagnosticTests(unittest.TestCase):
    # Fixed expectations are independent of argparse's error text and production
    # diagnostic helpers. Untrusted values never belong in selected diagnostics.
    PARSE_REFUSAL = b'public client arguments refused\n'
    VALUE_REFUSAL = b'public client refused\n'

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='public-parser-controls-')
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve()
        self.run = self.base / 'never-created'
        self.required = ['public-client-command', '--development-profile', 'public-client-linux-x86_64',
            '--profile-file', 'node=/inert/not-read', '--profile-tree', '/inert/not-read-tree',
            '--run-dir', str(self.run), '--action', 'submit', '--port', '1', '--timeout-seconds', '1']

    def invoke(self, arguments, *, expected=PARSE_REFUSAL, program='ci_assurance.py'):
        from contextlib import ExitStack
        supply = ci._public_client_supply_chain()
        with ExitStack() as stack:
            guards = [stack.enter_context(patch.object(owner, name, side_effect=AssertionError('refusal reached ' + name)))
                for owner, name in ((supply, 'validate_ci_lock'), (supply, 'validate_development_inputs'),
                                   (ci, '_public_client_identity'), (ci, '_prepare_public_client_run'),
                                   (ci, '_run_owned'), (ci.subprocess, 'Popen'))]
            stderr = stack.enter_context(patch('sys.stderr', new=io.StringIO()))
            stdout = stack.enter_context(patch('sys.stdout', new=io.StringIO()))
            stack.enter_context(patch.object(sys, 'argv', [program, *arguments]))
            try:
                status = ci.main()
            except SystemExit as stopped:
                status = stopped.code
            self.assertIn(status, (1, 2))
            for guard in guards: guard.assert_not_called()
            self.assertFalse(self.run.exists())
            self.assertEqual(list(self.base.iterdir()), [])
            self.assertEqual(stdout.getvalue(), '')
            diagnostic = stderr.getvalue().encode('utf-8')
            self.assertLessEqual(len(diagnostic), 4096)
            self.assertTrue(all(byte == 10 or 32 <= byte < 127 for byte in diagnostic))
            self.assertEqual(diagnostic, expected)
            return diagnostic

    def replaced(self, option, value):
        args = list(self.required)
        args[args.index(option) + 1] = value
        return args

    def test_oversized_unknown_argument_and_remainder_are_fixed_refusals(self):
        for suffix in (['UNTRUSTED-' + 'x'*8192], ['--unknown=' + 'x'*8192],
                       ['--', 'UNTRUSTED-' + 'x'*8192]):
            with self.subTest(shape=suffix[0][:20]): self.invoke(self.required + suffix)

    def test_control_bytes_unicode_and_program_name_never_reach_stderr(self):
        for token in ('UNTRUSTED-\x1b[31mRED', 'UNTRUSTED-\x00', 'UNTRUSTED-\n\r\t', 'UNTRUSTED-\x7f',
                      'UNTRUSTED-\u2603', '--unknown=\x1b[2J\n'):
            with self.subTest(token=repr(token)):
                self.invoke(self.required + [token], program='untrusted-program-\x1b[2J')

    def test_forbidden_password_values_are_never_echoed(self):
        for value in ('TESTPASS', 'PRIVATE-PARSER-SENTINEL', 'PRIVATE-' + 'x'*8192, 'PRIVATE-\x1b[2J\n'):
            for suffix in (['--password', value], ['--password=' + value]):
                with self.subTest(shape=suffix[0][:30], value_length=len(value)):
                    self.invoke(self.required + suffix)

    def test_every_duplicate_scalar_has_one_fixed_error(self):
        for option, value in (('--development-profile', 'public-client-linux-x86_64'), ('--profile-tree', '/inert'),
                              ('--run-dir', str(self.run)), ('--action', 'submit'), ('--port', '1'),
                              ('--timeout-seconds', '1'), ('--job-id', 'JOB00001'), ('--file-id', '0')):
            suffix = [option, value]
            if option in ('--job-id', '--file-id'): suffix *= 2
            with self.subTest(option=option): self.invoke(self.required + suffix)

    def test_malformed_typed_values_have_one_fixed_error(self):
        for value in ('INVALID-FLOAT', 'PRIVATE-' + 'x'*8192, 'PRIVATE-\x1b[2J', '\u2603', 'TESTPASS'):
            with self.subTest(value_length=len(value)):
                self.invoke(self.replaced('--timeout-seconds', value))

    def test_missing_option_values_and_required_fields_are_fixed(self):
        for suffix in (['--timeout-seconds'], ['--action'], ['--profile-file'], ['--port']):
            with self.subTest(option=suffix[0]): self.invoke(self.required + suffix)
        for option in ('--port', '--timeout-seconds', '--run-dir'):
            args = list(self.required); index = args.index(option); del args[index:index + 2]
            with self.subTest(missing=option): self.invoke(args)

    def test_parent_errors_before_and_after_selected_subparser_are_fixed(self):
        for prefix in (['--unknown=' + 'x'*8192], ['--unknown=PRIVATE-\x1b[2J'],
                       ['--password=PRIVATE-PARSER-SENTINEL'], ['--root', '--root', str(ROOT)],
                       ['--root', str(ROOT), '--root', str(ROOT)]):
            with self.subTest(prefix=prefix[0][:30]): self.invoke(prefix + self.required)

    def test_public_value_refusals_are_generic_before_validator_or_state(self):
        for option, value in (('--action', 'PRIVATE-\x1b[2J'), ('--port', 'PRIVATE-' + 'x'*8192),
                              ('--port', '01'), ('--timeout-seconds', 'nan')):
            with self.subTest(option=option):
                self.invoke(self.replaced(option, value), expected=self.VALUE_REFUSAL)
        self.invoke(['--root', str(self.base / ('PRIVATE-' + 'x'*8192)), *self.required], expected=self.VALUE_REFUSAL)
        self.invoke(['--root', str(self.base), *self.required], expected=self.VALUE_REFUSAL)

    def test_selected_help_keeps_fixed_useful_syntax(self):
        with patch.object(sys, 'argv', ['untrusted-program-\x1b[2J', 'public-client-command', '--help']), \
                patch('sys.stdout', new=io.StringIO()) as output, patch('sys.stderr', new=io.StringIO()) as error, \
                patch.object(ci, 'public_client_command') as command, self.assertRaises(SystemExit) as stopped:
            ci.main()
        self.assertEqual(stopped.exception.code, 0); command.assert_not_called()
        text = output.getvalue()
        self.assertIn('usage: ci_assurance.py public-client-command', text)
        for option in ('--profile-file', '--run-dir', '--action', '--port', '--timeout-seconds', '--job-id', '--file-id'):
            self.assertIn(option, text)
        self.assertNotIn('untrusted-program', text); self.assertNotIn('\x1b', text)
        self.assertEqual(error.getvalue(), '')

    def test_other_modes_keep_standard_parse_diagnostics_with_literal_public_mode(self):
        cases = [
            (['plan', '--output', '/inert', '--event', 'public-client-command'], "invalid choice: 'public-client-command'"),
            (['record', '--output', '/inert', '--gate', 'inert', '--timeout-seconds', 'PRIVATE-FLOAT',
              '--', 'public-client-command'], "invalid float value: 'PRIVATE-FLOAT'"),
            (['summary', '--plan', '/inert', '--directory', '/inert', '--output', '/inert',
              '--unknown', 'public-client-command', 'PRIVATE-\x1b[2J'], 'PRIVATE-\x1b[2J'),
        ]
        for args, expected in cases:
            with self.subTest(mode=args[0]), patch.object(sys, 'argv', ['ci_assurance.py', *args]), \
                    patch('sys.stderr', new=io.StringIO()) as error, self.assertRaises(SystemExit) as stopped:
                ci.main()
            self.assertEqual(stopped.exception.code, 2)
            self.assertIn('usage:', error.getvalue()); self.assertIn(expected, error.getvalue())
            self.assertFalse(self.run.exists())

    def test_record_remainder_and_global_root_value_do_not_select_public_errors(self):
        for prefix in ([], ['--root', 'public-client-command']):
            args = [*prefix, 'record', '--output', '/inert', '--gate', 'inert', '--',
                    'public-client-command', '--password', 'PRIVATE-PARSER-SENTINEL']
            with self.subTest(prefix=prefix), patch.object(sys, 'argv', ['ci_assurance.py', *args]), \
                    patch('sys.stderr', new=io.StringIO()) as error, patch.object(ci, 'record', return_value=0) as record:
                self.assertEqual(ci.main(), 0)
            self.assertEqual(record.call_args.args[3], ['public-client-command', '--password', 'PRIVATE-PARSER-SENTINEL'])
            self.assertEqual(error.getvalue(), '')



class PublicClientParentRootTests(unittest.TestCase):
    def refuse_hidden_root(self, second_option):
        from contextlib import ExitStack
        supply = ci._public_client_supply_chain()
        with tempfile.TemporaryDirectory(prefix='public-parent-root-') as directory:
            parent = Path(directory).resolve()
            tree = parent / 'tree'; tree.mkdir(mode=0o700)
            run = parent / 'never-created'
            args = ['ci_assurance.py', '--root', 'public-client-command', second_option, str(ROOT),
                    'public-client-command', '--development-profile', 'public-client-linux-x86_64',
                    '--profile-tree', str(tree), '--run-dir', str(run), '--action', 'submit',
                    '--port', '1', '--timeout-seconds', '1']
            for role in ('node-archive', 'node', 'zowe-archive', 'bubblewrap', 'loader', 'libdl',
                         'libstdcxx', 'libm', 'libgcc', 'libpthread', 'libc', 'libnss_files'):
                path = parent / role; path.write_bytes(b'inert\n'); path.chmod(0o600)
                args += ['--profile-file', f'{role}={path}']
            with ExitStack() as stack:
                command = stack.enter_context(patch.object(ci, 'public_client_command', return_value=0))
                guards = [stack.enter_context(patch.object(owner, name, side_effect=AssertionError('unexpected ' + name)))
                    for owner, name in ((supply, 'validate_ci_lock'), (supply, 'validate_development_inputs'),
                                       (ci, '_prepare_public_client_run'), (ci, '_run_owned'), (ci.subprocess, 'Popen'))]
                error = stack.enter_context(patch('sys.stderr', new=io.StringIO()))
                output = stack.enter_context(patch('sys.stdout', new=io.StringIO()))
                stack.enter_context(patch.object(sys, 'argv', args))
                try:
                    status = ci.main()
                except SystemExit as stopped:
                    status = stopped.code
                for guard in guards: guard.assert_not_called()
                self.assertFalse(run.exists())
                diagnostic = error.getvalue().encode('utf-8')
                self.assertLessEqual(len(diagnostic), 4096)
                self.assertTrue(all(byte == 10 or 32 <= byte < 127 for byte in diagnostic))
                # The inert transport stub exposes actual parser admission on
                # the initial candidate without any validator or native work.
                self.assertEqual((status, command.call_count, diagnostic, output.getvalue()),
                                 (2, 0, b'public client arguments refused\n', ''))

    def test_mode_valued_root_cannot_hide_second_exact_root(self):
        self.refuse_hidden_root('--root')

    def test_mode_valued_root_cannot_hide_second_abbreviated_root(self):
        self.refuse_hidden_root('--r')


if __name__=='__main__':unittest.main()
