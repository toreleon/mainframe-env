"""Fixed reviewed facts and independent tamper/compatibility expectations."""
import copy
import hashlib
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mq_wire_options as wire
import generate_mq_mqi_registry as registry


class MqWireOptionsTests(unittest.TestCase):
    def fixture(self, root):
        for path in [wire.CATALOG, wire.MANIFEST]:
            target = root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(registry.ROOT / path, target)

    def mutate(self, root, mutation):
        path = root / wire.CATALOG
        raw = path.read_text()
        value = json.loads(raw)['wire_options']
        mutation(value)
        path.write_text(raw.split('  "wire_options": ')[0] + '  "wire_options": ' +
                        json.dumps(value, indent=2).replace('\n', '\n  ') + '\n}\n')

    def test_exact_named_source_facts_not_generator_derived_expectations(self):
        value = wire.load(registry.ROOT)
        facts = {f['symbol']: f for f in value['facts']}
        self.assertEqual(len(facts), 102)
        for symbol, number in [('MQOO_INPUT_SHARED', 2), ('MQOO_OUTPUT', 16),
                               ('MQCO_DELETE_PURGE', 2), ('MQGMO_WAIT', 1),
                               ('MQGMO_NO_SYNCPOINT', 4), ('MQGMO_BROWSE_FIRST', 16),
                               ('MQGMO_ACCEPT_TRUNCATED_MSG', 64), ('MQPMO_NEW_MSG_ID', 64),
                               ('MQPMO_NEW_CORREL_ID', 128), ('MQOD_VERSION_1', 1),
                               ('MQMD_VERSION_2', 2), ('MQGMO_VERSION_4', 4), ('MQPMO_VERSION_3', 3)]:
            self.assertEqual(facts[symbol]['decimal'], number)
        self.assertEqual(facts['MQOO_RESOLVE_LOCAL_Q']['kind'], 'cpp-only')
        self.assertEqual(facts['MQOO_RESOLVE_LOCAL_Q']['decimal'], 262144)
        self.assertEqual(facts['MQOO_RESOLVE_LOCAL_TOPIC']['decimal'], 262144)
        self.assertEqual(facts['MQCO_NONE']['decimal'], 0)
        self.assertEqual(facts['MQCO_IMMEDIATE']['decimal'], 0)
        self.assertIsNone(facts['MQOD_VERSION_1']['hexadecimal'])

    def test_historical_signature_and_status_binding_remain_exact(self):
        self.assertEqual(wire.historical_sha(registry.ROOT / wire.CATALOG),
                         '7ff640ea64ade031e55848bd6de0788b1d70471ea9a5a34df158e43390eb6332')
        self.assertEqual(registry._contract_digest(registry.load_contract()),
                         'sha256:8452faa699ae5ed8431605958196aabf52fc4d550cca7b3d485b26c0beb0c965')
        self.assertEqual(registry.render_contract(), (registry.ROOT / registry.CONTRACT_OUTPUT_PATH).read_text())

    def test_pin_value_alias_version_locator_and_credit_mutants_fail(self):
        mutations = [
            lambda v: v['facts'][0].__setitem__('decimal', 999),
            lambda v: v['facts'][0].__setitem__('decimal', True),
            lambda v: v['facts'][0].__setitem__('decimal', 2147483648),
            lambda v: v['facts'][0].__setitem__('hexadecimal', 'ffffffff'),
            lambda v: v['facts'][0].__setitem__('topic_sha256', '0'*64),
            lambda v: v['facts'][0].__setitem__('topic_path', 'SSFKSJ_9.4.0/refdev/q999999_.html'),
            lambda v: v['facts'][0].__setitem__('first_line', 0),
            lambda v: v['facts'][0].__setitem__('fragment_sha256', '0'*64),
            lambda v: v['facts'].append(copy.deepcopy(v['facts'][0])),
            lambda v: v['facts'].pop(),
            lambda v: v['facts'][1].__setitem__('symbol', v['facts'][0]['symbol']),
            lambda v: v.__setitem__('semantic_execution_credit', 1),
            lambda v: v.__setitem__('scope_id', 'foreign'),
            lambda v: v['topic_manifest'].__setitem__('sha256', '0'*64),
            lambda v: v.__setitem__('baseline_id', 'foreign'),
            lambda v: v['corroboration'].pop(),
        ]
        for mutation in mutations:
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as d:
                root = Path(d);self.fixture(root);self.mutate(root, mutation)
                with self.assertRaises(ValueError): wire.load(root)

    def test_changed_historical_calls_cannot_be_rehashed_as_new_identity(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);self.fixture(root);p=root/wire.CATALOG
            p.write_text(p.read_text().replace('MQBACK', 'MQFORGED', 1))
            with self.assertRaisesRegex(ValueError, 'historical'): wire.load(root)

    def test_selected_manifest_tamper_is_not_hidden_by_fresh_render(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);self.fixture(root);p=root/wire.MANIFEST
            value=json.loads(p.read_text());value['topics'][0]['sha256']='0'*64
            p.write_text(json.dumps(value))
            with self.assertRaisesRegex(ValueError, 'source binding'): wire.load(root)

    def test_duplicate_json_fields_are_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);self.fixture(root);p=root/wire.CATALOG
            p.write_text(p.read_text().replace('"decimal": 1,', '"decimal": 1, "decimal": 1,', 1))
            with self.assertRaises(ValueError): wire.load(root)

    def test_unknown_root_field_cannot_hide_after_additive_projection(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);self.fixture(root);p=root/wire.CATALOG
            raw=p.read_text();p.write_text(raw[:-2] + ', "foreign": true\n}\n')
            with self.assertRaises(ValueError): wire.load(root)


if __name__ == '__main__': unittest.main()
