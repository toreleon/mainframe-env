import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

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
            'conformance/0.8/evidence/receipt.json': ci.ALL,
            'docs/contracts/effect-canonical-v1.md': {ci.DOCS},
            'docs/architecture/RUNTIME.md': {ci.DOCS},
        }.items():
            with self.subTest(path=path): self.assertTrue(required <= set(ci.obligations([path])))
    def test_shared_contracts_cannot_select_an_empty_or_partial_set(self):
        for path in ['crates/contracts/mainframe-env-host-api/src/request.rs','crates/foundation/mainframe-env-ir/src/lib.rs','Cargo.lock','Cargo.toml','conformance/spec/manifest.json']:
            self.assertEqual(set(ci.obligations([path])), ci.ALL)
    def test_cics_pilot_manifest_review_fixture_observation_and_provider_paths_are_selected(self):
        for path, required in {
            'conformance/0.9/manifests/cics-file-uow-topics.json': ci.ALL,
            'conformance/0.9/cics/pilot-rule-review.json': ci.ALL,
            'conformance/0.9/cics/pilot-fixtures.json': ci.ALL,
            'conformance/0.9/cobol/move-rule-review.json': ci.ALL,
            'conformance/0.9/cobol/move-fixture.json': ci.ALL,
            'conformance/0.9/oracles/cics-licensed-differential.json': ci.ALL,
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
        for event,ref in [('schedule','refs/heads/main'),('manual','refs/heads/main'),('tag','refs/tags/mainframe-env-v0.8.2')]:
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

    def test_jenkins_and_offline_release_bundle_enforce_license_distribution(self):
        jenkins=(ROOT/'Jenkinsfile').read_text()
        self.assertIn('--gate cargo-deny -- cargo deny check',jenkins)
        self.assertIn('--gate license-notices -- cargo xtask license-notices --check',jenkins)
        self.assertIn('--gate supply-chain',jenkins)
        self.assertIn('--gate msrv',jenkins)
        self.assertIn('cargo +1.95.0 check --workspace --all-targets --all-features --locked',jenkins)
        self.assertIn("command -v cargo-deny",jenkins)
        self.assertIn("mainframe-env-release-ed25519-pkcs8",jenkins)
        self.assertIn('MAINFRAME_ENV_RELEASE_INVOCATION_ID="${BUILD_URL:',jenkins)
        for required in [
            'config/release-attestation-policy.json',
            'conformance/standards/cyclonedx/1.6/bom-1.6.schema.json.gz.b64',
            'docs/architecture/RELEASE-BUILDER.md',
            'docs/contracts/RELEASE-BUILD-V1.md',
        ]:
            self.assertIn(required,jenkins)
        bundle=(ROOT/'tools/package_offline_cargo_bundle.sh').read_text()
        for required in ['LICENSE','NOTICE','LICENSES/ICU.txt']:
            self.assertIn(f'"$root/{required}"',bundle)
        policy=(ROOT/'deny.toml').read_text()
        self.assertIn('crate = "decnumber-sys@=0.1.6"',policy)
        self.assertIn('allow = ["ICU"]',policy)

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
            'BRANCH_NAME':'mainframe-env-v0.8.2',
        })
        self.assertEqual((tag['event'],tag['ref'],tag['provider']),
                         ('tag','refs/tags/mainframe-env-v0.8.2','jenkins'))
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

if __name__=='__main__':unittest.main()
