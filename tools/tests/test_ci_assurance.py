import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

TOOL = Path(__file__).resolve().parents[1] / 'ci_assurance.py'
spec = importlib.util.spec_from_file_location('ci_assurance', TOOL)
ci = importlib.util.module_from_spec(spec); spec.loader.exec_module(ci)

class SelectionTests(unittest.TestCase):
    def test_runtime_compiler_store_and_workflow_paths_select_their_obligations(self):
        for path, required in {
            'crates/kernel/mainframe-env-interpreter/src/machine.rs': {'runtime','architecture'},
            'crates/kernel/mainframe-env-compiler/src/hir.rs': {'compiler','architecture'},
            'crates/stores/mainframe-env-store/src/durable.rs': {'store','runtime'},
            '.github/workflows/ci.yml': ci.ALL,
            'conformance/0.8/evidence/receipt.json': ci.ALL,
            'docs/contracts/effect-canonical-v1.md': ci.ALL,
            'docs/architecture/RUNTIME.md': ci.ALL,
        }.items():
            with self.subTest(path=path): self.assertTrue(required <= set(ci.obligations([path])))
    def test_shared_contracts_cannot_select_an_empty_or_partial_set(self):
        for path in ['crates/contracts/mainframe-env-host-api/src/request.rs','crates/foundation/mainframe-env-ir/src/lib.rs','Cargo.lock','Cargo.toml','conformance/spec/manifest.json']:
            self.assertEqual(set(ci.obligations([path])), ci.ALL)
    def test_prose_is_cheap_but_mixed_changes_are_not(self):
        for path in ['README.md','docs/research/market.md','docs/runbooks/start.md','docs/delivery/hardening/notes.md']:
            self.assertEqual(ci.obligations([path]), [])
            self.assertIn('runtime',ci.obligations([path,'crates/apps/server/src/main.rs']))
    def test_unknown_files_default_to_all_and_code_under_docs_is_not_prose(self):
        for path in ['new-system/input.xyz','docs/runbooks/check.py','docs/new-normative/spec.md']:
            self.assertEqual(set(ci.obligations([path])),ci.ALL)
    def test_renamed_or_deleted_normative_paths_still_trigger(self):
        # --no-renames supplies both paths. Deletions use the same obligation mapping.
        self.assertEqual(set(ci.obligations(['docs/contracts/OLD.md','docs/research/new.md'])),ci.ALL)
    def test_paths_cannot_escape_the_repository(self):
        for path in ['', '../Cargo.toml','/tmp/file','docs/../../x','x\x00y','docs\\x']:
            with self.assertRaises(ValueError): ci.obligations([path])
    @patch.object(ci,'identity',return_value={'candidate':'a'*40,'tree':'b'*40})
    def test_full_tiers_select_every_obligation(self,_):
        for event,ref in [('schedule','refs/heads/main'),('workflow_dispatch','refs/heads/main'),('push','refs/tags/mainframe-env-v0.8.2')]:
            p=ci.make_plan(Path('.'),{},event,ref)
            self.assertTrue(p['full']);self.assertTrue(p['msrv']);self.assertTrue(p['store'])
            self.assertTrue(set(ci.FULL)<=set(p['primary_gates']))
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

if __name__=='__main__':unittest.main()
