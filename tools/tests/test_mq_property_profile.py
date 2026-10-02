"""Independent property constants/default expectations and exact closure mutants."""
import copy
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import generate_mq_mqi_registry as registry
import mq_property_profile as profile
import mq_raw_layout as raw
import mq_wire_options as wire

class PropertyProfileTests(unittest.TestCase):
    def fixture(self,root):
        value,_=profile.load(registry.ROOT); layouts=raw.load(registry.ROOT)
        paths={wire.CATALOG,Path(layouts['owned_character_table']['path'])}
        paths.update(Path(s['topic_manifest']['path']) for s in value['sources']+layouts['sources'])
        for path in paths:
            target=root/path;target.parent.mkdir(parents=True,exist_ok=True)
            shutil.copyfile(registry.ROOT/path,target)

    def test_fixed_numeric_profile_and_descriptor_defaults(self):
        value,md=profile.load(registry.ROOT);facts={f['symbol']:f['decimal'] for f in value['facts']}
        expected={'MQCMHO_VERSION_1':1,'MQCMHO_DEFAULT_VALIDATION':0,'MQCMHO_NO_VALIDATION':1,
                  'MQCMHO_VALIDATE':2,'MQDMHO_NONE':0,'MQDMPO_DEL_FIRST':0,
                  'MQIMPO_INQ_FIRST':0,'MQIMPO_QUERY_LENGTH':4,'MQSMPO_SET_FIRST':0,
                  'MQPD_VERSION_1':1,'MQPD_SUPPORT_OPTIONAL':1,'MQCOPY_DEFAULT':22,
                  'MQCOPY_NONE':0,'MQENC_NATIVE':785,'MQTYPE_NULL':2,'MQTYPE_BYTE_STRING':8,
                  'MQTYPE_INT8':16,'MQTYPE_INT16':32,'MQTYPE_INT32':64,'MQTYPE_LONG':64,
                  'MQTYPE_INT64':128,'MQTYPE_STRING':1024,'MQTYPE_AS_SET':0}
        for symbol,number in expected.items(): self.assertEqual(facts[symbol],number,symbol)
        self.assertEqual({i['symbol']:i['value'] for i in value['identifiers']}['MQPD_STRUC_ID'],'PD  ')
        self.assertEqual(value['descriptor_excluded_fields'],['StrucId','Version'])
        self.assertEqual(md['version'],1);self.assertEqual(md['prefix_bytes'],324)
        fields={f['name']:f for f in md['fields']}
        self.assertEqual(fields['Encoding']['initial']['kind'],'environment')
        self.assertEqual(fields['Expiry']['initial']['value'],-1)
        self.assertEqual(fields['Priority']['initial']['value'],-1)
        self.assertEqual(fields['Persistence']['initial']['value'],2)
        self.assertEqual({w['kind']:w['bytes'] for w in value['value_widths']},
            {'Boolean':4,'ByteString':None,'Int8':1,'Int16':2,'Int32':4,'Int64':8,'Float32':4,'Float64':8,'String':None,'Null':0})

    def test_mutated_pins_facts_widths_membership_credit_and_duplicate_reject(self):
        mutations=[lambda v:v.__setitem__('semantic_execution_credit',1),
            lambda v:v['facts'][0].__setitem__('decimal',17),
            lambda v:v['facts'][0].__setitem__('hexadecimal','FFFFFFFF'),
            lambda v:v['facts'].append(copy.deepcopy(v['facts'][0])),
            lambda v:v['facts'].pop(),
            lambda v:v['facts'][0].__setitem__('symbol',v['facts'][1]['symbol']),
            lambda v:v['facts'][0]['source'].__setitem__('fragment_sha256','0'*64),
            lambda v:v['facts'][0]['source'].__setitem__('topic_sha256','0'*64),
            lambda v:v['sources'][0]['topic_manifest'].__setitem__('sha256','0'*64),
            lambda v:v['sources'][0].__setitem__('scope_id','foreign'),
            lambda v:v['identifiers'][0].__setitem__('value','BAD!'),
            lambda v:v['value_widths'][0].__setitem__('bytes',8),
            lambda v:v.__setitem__('descriptor_layout','Md2'),
            lambda v:v['descriptor_excluded_fields'].remove('Version'),
            lambda v:v.__setitem__('unknown',True)]
        for mutation in mutations:
            with self.subTest(mutation=mutation),tempfile.TemporaryDirectory() as directory:
                root=Path(directory);self.fixture(root);path=root/wire.CATALOG;value=json.loads(path.read_text())
                mutation(value['property_profile'])
                # Preserve exact historical prefix while replacing only the private projection.
                path.write_text(path.read_text().split('  "property_profile": ')[0]+
                    '  "property_profile": '+json.dumps(value['property_profile'],indent=2).replace('\n','\n  ')+'\n}\n')
                with self.assertRaises(ValueError):profile.load(root)

    def test_unmounted_sources_still_validate_artifact_closure_and_old_identities(self):
        self.assertEqual(wire.historical_sha(registry.ROOT/wire.CATALOG),
            '3dc77d004bd79ad7f6daa99ffb4a3fb958ff1d816cb6c33bc4b6bac794a23448')
        self.assertEqual(registry._contract_digest(registry.load_contract()),
            'sha256:8452faa699ae5ed8431605958196aabf52fc4d550cca7b3d485b26c0beb0c965')
        self.assertEqual(raw.digest(raw.load(registry.ROOT)),'2f6d7c679543891ccc768f099963813d981871d2abd35fa7b49aa4e70732ae6a')
        self.assertEqual(profile.render(registry.ROOT),(registry.ROOT/profile.OUTPUT).read_text())
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);self.fixture(root)
            self.assertEqual(profile.render(root),profile.render(registry.ROOT))
            value,_=profile.load(root);path=root/value['sources'][0]['topic_manifest']['path']
            path.write_bytes(path.read_bytes()+b' ')
            with self.assertRaises(ValueError):profile.load(root)

if __name__=='__main__':unittest.main()
