"""Independent RFH2 facts and frozen historical projection mutants."""
import copy
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import generate_mq_mqi_registry as registry
import mq_rfh2_profile as profile
import mq_property_profile as properties
import mq_raw_layout as raw
import mq_wire_options as wire

class Rfh2ProfileTests(unittest.TestCase):
    def fixture(self, root):
        value=profile.load(registry.ROOT);p,_=properties.load(registry.ROOT);r=raw.load(registry.ROOT)
        paths={wire.CATALOG,Path(r['owned_character_table']['path'])}
        paths.update(Path(s['topic_manifest']['path']) for s in value['sources']+p['sources']+r['sources'])
        for path in paths:
            target=root/path;target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(registry.ROOT/path,target)

    def test_independent_header_constant_layout_and_type_facts(self):
        value=profile.load(registry.ROOT)
        facts={f['symbol']:f['decimal'] for f in value['facts']}
        self.assertEqual(facts,{'MQBMHO_VERSION_1':1,'MQBMHO_NONE':0,'MQBMHO_DELETE_PROPERTIES':1,
            'MQMHBO_VERSION_1':1,'MQMHBO_NONE':0,'MQMHBO_PROPERTIES_IN_MQRFH2':1,'MQMHBO_DELETE_PROPERTIES':2,
            'MQRFH_NONE':0,'MQRFH_VERSION_2':2,'MQRFH_STRUC_LENGTH_FIXED_2':36})
        self.assertEqual([(f['name'],f['offset'],f['width']) for f in value['fields']],
            [('STRUCID',0,4),('VERSION',4,4),('STRUCLENGTH',8,4),('ENCODING',12,4),
             ('CODEDCHARSETID',16,4),('FORMAT',20,8),('FLAGS',28,4),('NAMEVALUECCSID',32,4)])
        self.assertEqual({i['symbol']:i['value'] for i in value['identifiers']},
            {'MQBMHO_STRUC_ID':'BMHO','MQMHBO_STRUC_ID':'MHBO','MQRFH_STRUC_ID':'RFH ',
             'MQFMT_NONE':'        ','MQFMT_RF_HEADER_2':'MQHRF2  '})
        self.assertEqual(value['formatter']['inherited_ccsid']['decimal'],-2)
        self.assertEqual(value['formatter']['name_value_ccsid']['decimal'],1208)
        self.assertEqual({t['kind']:t['lexical'] for t in value['types']},
            {'ByteString':'bin.hex','Int8':'i1','Int16':'i2','Int32':'i4','Int64':'i8','String':'string'})

    def test_pin_number_offset_scope_credit_and_duplicate_mutants_fail_without_cache(self):
        mutations=[lambda v:v.__setitem__('semantic_execution_credit',1),
            lambda v:v['facts'][0].__setitem__('decimal',4),lambda v:v['facts'].pop(),
            lambda v:v['facts'].append(copy.deepcopy(v['facts'][0])),
            lambda v:v['fields'][1].__setitem__('offset',0),lambda v:v['fields'][1].__setitem__('width',8),
            lambda v:v['identifiers'][2].__setitem__('value','RFH2'),
            lambda v:v['sources'][0]['topic_manifest'].__setitem__('sha256','0'*64),
            lambda v:v['sources'][0].__setitem__('scope_id','foreign'),
            lambda v:v['facts'][0]['source'].__setitem__('topic_sha256','0'*64),
            lambda v:v['facts'][0]['source'].__setitem__('fragment_sha256','0'*64),
            lambda v:v['formatter']['inherited_ccsid'].__setitem__('decimal',0),
            lambda v:v.__setitem__('extra',True)]
        for mutation in mutations:
            with self.subTest(mutation=mutation),tempfile.TemporaryDirectory() as directory:
                root=Path(directory);self.fixture(root);path=root/wire.CATALOG;value=json.loads(path.read_text());mutation(value['rfh2_profile'])
                path.write_text(path.read_text().split('  "rfh2_profile": ')[0]+'  "rfh2_profile": '+
                    json.dumps(value['rfh2_profile'],indent=2).replace('\n','\n  ')+'\n}\n')
                with self.assertRaises(ValueError):profile.load(root)

    def test_generated_freshness_and_historical_identities_are_unchanged(self):
        self.assertEqual(wire.historical_sha(registry.ROOT/wire.CATALOG),'3dc77d004bd79ad7f6daa99ffb4a3fb958ff1d816cb6c33bc4b6bac794a23448')
        self.assertEqual(properties.digest(properties.load(registry.ROOT)[0]),'8da951d82502333755eee8f45447c2c17f699bbe436e93d3bde1c57a6d60dfd4')
        self.assertEqual(raw.digest(raw.load(registry.ROOT)),'2f6d7c679543891ccc768f099963813d981871d2abd35fa7b49aa4e70732ae6a')
        self.assertEqual(profile.render(registry.ROOT),(registry.ROOT/profile.OUTPUT).read_text())
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);self.fixture(root);self.assertEqual(profile.render(root),profile.render(registry.ROOT))
            value=profile.load(root);path=root/value['sources'][0]['topic_manifest']['path'];path.write_bytes(path.read_bytes()+b' ')
            with self.assertRaises(ValueError):profile.load(root)

if __name__=='__main__':unittest.main()
