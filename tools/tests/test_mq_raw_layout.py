"""Independent fixed layout expectations and negative source closure mutants."""
import copy
import difflib
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mq_raw_layout as raw
import mq_wire_options as wire
import generate_mq_mqi_registry as registry


class MqRawLayoutTests(unittest.TestCase):
    def fixture(self, root):
        value = raw.load(registry.ROOT)
        for path in [wire.CATALOG, wire.MANIFEST, Path(value['sources'][1]['topic_manifest']['path']), Path(value['owned_character_table']['path'])]:
            target = root / path; target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(registry.ROOT / path, target)

    def test_fixed_reviewed_widths_offsets_and_initial_values(self):
        value = raw.load(registry.ROOT)
        layouts = {l['kind']: l for l in value['layouts']}
        self.assertEqual([(l['kind'], l['prefix_bytes']) for l in value['layouts']],
                         [('Od1',168),('Md1',324),('Md2',364),('Gmo1',72),('Pmo1',128)])
        fields = {f['name']: f for f in layouts['Md2']['fields']}
        for name,offset,width,kind in [('MsgId',48,24,'bytes'),('CorrelId',72,24,'bytes'),
                                      ('AccountingToken',208,32,'bytes'),('GroupId',324,24,'bytes'),
                                      ('OriginalLength',360,4,'long')]:
            self.assertEqual((fields[name]['offset'],fields[name]['width'],fields[name]['kind']),(offset,width,kind))
        self.assertEqual(fields['MsgType']['initial']['value'],8)
        self.assertEqual(fields['Expiry']['initial']['value'],-1)
        self.assertEqual(fields['Persistence']['initial']['value'],2)
        self.assertEqual(fields['Encoding']['initial']['kind'],'environment')
        self.assertEqual(layouts['Gmo1']['fields'][4]['kind'],'signal-slot')
        self.assertEqual(layouts['Pmo1']['fields'][4]['kind'],'alias')

    def test_historical_contract_and_wire_projection_stay_identical(self):
        self.assertEqual(wire.historical_sha(registry.ROOT/wire.CATALOG),
                         '3dc77d004bd79ad7f6daa99ffb4a3fb958ff1d816cb6c33bc4b6bac794a23448')
        self.assertEqual(registry._contract_digest(registry.load_contract()),
                         'sha256:8452faa699ae5ed8431605958196aabf52fc4d550cca7b3d485b26c0beb0c965')
        self.assertEqual(registry.render_contract(),(registry.ROOT/registry.CONTRACT_OUTPUT_PATH).read_text())
        self.assertEqual(wire.render(registry.ROOT),(registry.ROOT/wire.OUTPUT).read_text())

    def test_exact_layout_pin_type_width_offset_output_and_credit_mutants_reject(self):
        mutations = [
            lambda v: v['layouts'][0].__setitem__('prefix_bytes',184),
            lambda v: v['layouts'][0].__setitem__('version',2),
            lambda v: v['layouts'][0].__setitem__('identifier','OD\0\0'),
            lambda v: v['layouts'][1]['fields'][11].__setitem__('kind','characters'),
            lambda v: v['layouts'][1]['fields'][11].__setitem__('width',23),
            lambda v: v['layouts'][1]['fields'][11].__setitem__('offset',49),
            lambda v: v['layouts'][0]['fields'].append(copy.deepcopy(v['layouts'][0]['fields'][0])),
            lambda v: v['layouts'][2]['fields'].pop(),
            lambda v: v['layouts'][2]['fields'][2]['initial'].__setitem__('value',1),
            lambda v: v['layouts'][3]['fields'][4].__setitem__('writeback','get'),
            lambda v: v['layouts'][4]['fields'][5].__setitem__('writeback','get-put'),
            lambda v: v['layouts'][0]['c_declaration'].__setitem__('topic_sha256','0'*64),
            lambda v: v['layouts'][0]['c_declaration'].__setitem__('first_line',1),
            lambda v: v['layouts'][0]['c_declaration'].__setitem__('fragment_sha256','0'*64),
            lambda v: v['sources'][1]['topic_manifest'].__setitem__('sha256','0'*64),
            lambda v: v['sources'][1].__setitem__('scope_id','foreign'),
            lambda v: v.__setitem__('semantic_execution_credit',1),
            lambda v: v['elementary'].__setitem__('long_bytes',8),
            lambda v: v.__setitem__('unknown',True),
        ]
        for mutation in mutations:
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as directory:
                root=Path(directory);self.fixture(root);p=root/wire.CATALOG;old=p.read_text();v=json.loads(old)['raw_layout'];mutation(v)
                p.write_text(old.split('  "raw_layout": ')[0]+'  "raw_layout": '+json.dumps(v,indent=2).replace('\n','\n  ')+'\n}\n')
                with self.assertRaises(ValueError): raw.load(root)

    def test_tampered_manifests_and_owned_profile_reject_without_source_cache(self):
        for which in ['manifest','profile']:
            with tempfile.TemporaryDirectory() as directory:
                root=Path(directory);self.fixture(root);value=raw.load(root)
                relative=value['sources'][1]['topic_manifest']['path'] if which=='manifest' else value['owned_character_table']['path']
                p=root/relative;p.write_bytes(p.read_bytes()+b' ')
                with self.assertRaises(ValueError): raw.load(root)

    def test_generated_descriptors_are_deterministic_and_cache_independent(self):
        self.assertEqual(raw.render(registry.ROOT),(registry.ROOT/raw.OUTPUT).read_text())
        self.assertEqual(raw.render(registry.ROOT),raw.render(registry.ROOT))


if __name__ == '__main__': unittest.main()
