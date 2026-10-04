"""Fixed source facts, historical closure and independently chosen mutants."""
import copy
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import generate_mq_mqi_registry as registry
import mq_inquiry_local_type as inquiry
import mq_wire_options as wire


class InquiryLocalTypeTests(unittest.TestCase):
    def fixture(self, root):
        paths = {wire.CATALOG}
        paths.update(Path(s['topic_manifest']['path']) for s in inquiry.load(registry.ROOT)['sources'])
        for path in paths:
            target = root / path; target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(registry.ROOT / path, target)

    def test_independent_literal_symbols_numbers_hex_and_provenance(self):
        value = inquiry.load(registry.ROOT)
        self.assertEqual([(f['symbol'], f['decimal'], f['hexadecimal']) for f in value['facts']],
                         [('MQIA_Q_TYPE',20,'00000014'), ('MQQT_LOCAL',1,'00000001')])
        self.assertEqual((value['official_row'], value['source_positions']), ('0016',[16]))
        self.assertEqual(value['owned_limits'], {'integer_slots':256})
        self.assertEqual(value['semantic_execution_credit'],0)
        self.assertEqual(value['facts'][0]['source']['topic_sha256'],
                         '33468f1a42a59c46286c019ac8d8023970f65f90685cb7836dc77ffc3d630e2e')
        self.assertEqual(value['facts'][1]['source']['topic_sha256'],
                         'ac5de9d74f62635456e566bfcbba7706699686cef14580ab30aa22166b2fb1ba')

    def test_value_symbol_pin_locator_count_profile_credit_and_unknown_mutants(self):
        mutations = [
            lambda v:v['facts'][0].__setitem__('decimal',21),
            lambda v:v['facts'][0].__setitem__('hexadecimal','00000015'),
            lambda v:v['facts'][1].__setitem__('decimal',2),
            lambda v:v['facts'][1].__setitem__('symbol','MQQT_MODEL'),
            lambda v:v['facts'].append(copy.deepcopy(v['facts'][0])),
            lambda v:v['facts'].pop(),
            lambda v:v['facts'][0]['source'].__setitem__('topic_sha256','0'*64),
            lambda v:v['facts'][0]['source'].__setitem__('first_line',450),
            lambda v:v['facts'][0]['source'].__setitem__('fragment_sha256','0'*64),
            lambda v:v['sources'][0].__setitem__('scope_id','foreign'),
            lambda v:v['sources'][0]['topic_manifest'].__setitem__('sha256','0'*64),
            lambda v:v.__setitem__('schema_version','mainframe-env.mq-inquiry-local-type-projection@2'),
            lambda v:v.__setitem__('semantic_execution_credit',1),
            lambda v:v.__setitem__('profile','AllQueues'),
            lambda v:v.__setitem__('unknown',True),
            lambda v:v['pending'].pop(),
            lambda v:v['owned_limits'].__setitem__('integer_slots',257),
        ]
        for action in mutations:
            with self.subTest(action=action), tempfile.TemporaryDirectory() as directory:
                root=Path(directory); self.fixture(root); path=root / wire.CATALOG
                original=path.read_text(); value=json.loads(original)['inquiry_local_type']; action(value)
                path.write_text(original.split('  "inquiry_local_type": ')[0] + '  "inquiry_local_type": ' +
                                json.dumps(value,indent=2).replace('\n','\n  ')+'\n}\n')
                with self.assertRaises(ValueError): inquiry.load(root)

    def test_missing_duplicate_new_version_and_tampered_manifest_fail(self):
        for mode in ['missing','duplicate','version','manifest']:
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as directory:
                root=Path(directory); self.fixture(root); path=root / wire.CATALOG; original=path.read_text()
                if mode == 'missing':
                    path.write_text(original.split('  "inquiry_local_type": ')[0].removesuffix(',\n')+'\n}\n')
                    self.assertEqual(wire.historical_sha(path),wire.HISTORICAL_SHA)
                elif mode == 'duplicate':
                    path.write_text(original.replace('"inquiry_local_type": {','"inquiry_local_type": {}, "inquiry_local_type": {'))
                elif mode == 'version':
                    path.write_text(original.replace('mq-structure-status-catalog@2','mq-structure-status-catalog@3',1))
                else:
                    value=inquiry.load(root); path=root / value['sources'][2]['topic_manifest']['path']
                    path.write_bytes(path.read_bytes()+b' ')
                with self.assertRaises(ValueError): inquiry.load(root)

    def test_historical_projection_bytes_and_call_closure_stay_exact(self):
        for helper in [registry.wire,registry.raw_layout,registry.property_profile,
                       registry.rfh2_profile,registry.raw_property,inquiry]:
            self.assertEqual(helper.render(registry.ROOT),(registry.ROOT / helper.OUTPUT).read_text())
        self.assertEqual(wire.historical_sha(registry.ROOT / wire.CATALOG),
                         '3dc77d004bd79ad7f6daa99ffb4a3fb958ff1d816cb6c33bc4b6bac794a23448')
        self.assertEqual(registry._contract_digest(registry.load_contract()),
                         'sha256:8452faa699ae5ed8431605958196aabf52fc4d550cca7b3d485b26c0beb0c965')
        self.assertEqual(sum(len(r['source_positions']) for r in registry.load()),27)
        self.assertEqual(inquiry.digest(inquiry.load(registry.ROOT)),inquiry.PROJECTION_SHA)


if __name__ == '__main__':
    unittest.main()
