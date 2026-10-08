import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
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


if __name__=='__main__':unittest.main()
