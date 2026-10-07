"""Independent reconciled prefix fixtures and fail-closed projection mutants."""
import copy
import hashlib
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import generate_mq_mqi_registry as registry
import mq_property_profile as properties
import mq_raw_layout as raw
import mq_raw_property as profile
import mq_rfh2_profile as rfh2
import mq_wire_options as wire


class RawPropertyTests(unittest.TestCase):
    def fixture(self, root):
        value = profile.load(registry.ROOT); previous = raw.load(registry.ROOT)
        paths = {wire.CATALOG, Path(previous['owned_character_table']['path'])}
        paths.update(Path(s['topic_manifest']['path']) for s in value['sources'])
        for path in paths:
            target = root / path; target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(registry.ROOT / path, target)

    def test_independent_complete_reconciled_members_and_widths(self):
        value = profile.load(registry.ROOT)
        charv, impo = value['layouts']
        self.assertEqual((charv['prefix_bytes'], impo['prefix_bytes']), (20, 60))
        self.assertEqual([(f['name'], f['offset'], f['width']) for f in charv['fields']],
                         [('VSPtr',0,4),('VSOffset',4,4),('VSBufSize',8,4),('VSLength',12,4),('VSCCSID',16,4)])
        self.assertEqual([(f['name'], f['offset'], f['width']) for f in impo['fields']],
                         [('StrucId',0,4),('Version',4,4),('Options',8,4),
                          ('RequestedEncoding',12,4),('RequestedCCSID',16,4),
                          ('ReturnedEncoding',20,4),('ReturnedCCSID',24,4),('Reserved1',28,4),
                          ('ReturnedName.VSPtr',32,4),('ReturnedName.VSOffset',36,4),
                          ('ReturnedName.VSBufSize',40,4),('ReturnedName.VSLength',44,4),
                          ('ReturnedName.VSCCSID',48,4),('TypeString',52,8)])
        self.assertEqual(impo['fields'][7]['kind'], 'characters')
        self.assertEqual(impo['fields'][8]['kind'], 'null-slot')
        self.assertFalse(impo['fields'][-1]['defined_standard_output'])
        self.assertEqual(value['vendor_copybook_equivalence'], 'unresolved')
        self.assertEqual(len(value['disagreements']), 6)

    def test_frozen_historical_and_existing_generated_projection_bytes(self):
        self.assertEqual(wire.historical_sha(registry.ROOT / wire.CATALOG),
                         '7ff640ea64ade031e55848bd6de0788b1d70471ea9a5a34df158e43390eb6332')
        for helper in [wire, raw, properties, rfh2]:
            self.assertEqual(helper.render(registry.ROOT), (registry.ROOT / helper.OUTPUT).read_text())
        self.assertEqual(registry._contract_digest(registry.load_contract()),
                         'sha256:8452faa699ae5ed8431605958196aabf52fc4d550cca7b3d485b26c0beb0c965')

    def test_missing_duplicate_foreign_fields_widths_offsets_outputs_and_pins_reject(self):
        mutations = [
            lambda v: v['layouts'][1]['fields'].pop(),
            lambda v: v['layouts'][1]['fields'].append(copy.deepcopy(v['layouts'][1]['fields'][0])),
            lambda v: v['layouts'][1]['fields'][7].__setitem__('kind','long'),
            lambda v: v['layouts'][1]['fields'][7].__setitem__('width',1),
            lambda v: v['layouts'][1]['fields'][-1].__setitem__('width',4),
            lambda v: v['layouts'][1]['fields'][8].__setitem__('offset',28),
            lambda v: v['layouts'][1]['fields'][8].__setitem__('name','PointerAuthority'),
            lambda v: v['layouts'][1]['fields'][-1].__setitem__('defined_standard_output',True),
            lambda v: v['layouts'][0].__setitem__('prefix_bytes',24),
            lambda v: v['layouts'][1].__setitem__('version',2),
            lambda v: v['layouts'][1].__setitem__('identifier','IPMO'),
            lambda v: v['sources'][0].__setitem__('scope_id','foreign'),
            lambda v: v['sources'][0]['topic_manifest'].__setitem__('sha256','0'*64),
            lambda v: v['identity']['version_source'].__setitem__('topic_sha256','0'*64),
            lambda v: v['identity']['version_source'].__setitem__('fragment_sha256','0'*64),
            lambda v: v['identity']['version_source'].__setitem__('first_line',1),
            lambda v: v.__setitem__('semantic_execution_credit',1),
            lambda v: v.__setitem__('vendor_copybook_equivalence','verified'),
            lambda v: v.__setitem__('type_string_policy','synthesize'),
            lambda v: v.__setitem__('profile','NativePointer'),
            lambda v: v['long'].__setitem__('width',8),
            lambda v: v['owned_limits'].__setitem__('group_bytes',2**64),
            lambda v: v.__setitem__('unknown',True),
        ]
        for mutate in mutations:
            with self.subTest(mutate=mutate), tempfile.TemporaryDirectory() as directory:
                root = Path(directory); self.fixture(root); path = root / wire.CATALOG
                original = path.read_text(); value = json.loads(original)['raw_property']; mutate(value)
                path.write_text(original.split('  "raw_property": ')[0] + '  "raw_property": ' +
                                json.dumps(value,indent=2).replace('\n','\n  ') + '\n}\n')
                with self.assertRaises(ValueError): profile.load(root)

    def test_tampered_manifest_bytes_and_missing_projection_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); self.fixture(root); value = profile.load(root)
            path = root / value['sources'][1]['topic_manifest']['path']; path.write_bytes(path.read_bytes()+b' ')
            with self.assertRaises(ValueError): profile.load(root)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); self.fixture(root); path=root / wire.CATALOG
            original = path.read_text(); prefix = original.split('  "raw_property": ')[0]
            path.write_text(prefix.removesuffix(',\n') + '\n}\n')
            with self.assertRaises(ValueError): profile.load(root)

    def test_duplicate_json_key_and_deterministic_generated_closure(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); self.fixture(root); path = root / wire.CATALOG
            text=path.read_text().replace('"raw_property": {','"raw_property": {}, "raw_property": {')
            path.write_text(text)
            with self.assertRaises(ValueError): profile.load(root)
        self.assertEqual(profile.render(registry.ROOT), (registry.ROOT / profile.OUTPUT).read_text())
        self.assertEqual(profile.digest(profile.load(registry.ROOT)), profile.PROJECTION_SHA)


if __name__ == '__main__':
    unittest.main()
