"""One bounded numeric projection in the existing MQ structure catalog."""

import hashlib
import json
from pathlib import Path
import re
import sys

CATALOG = Path('conformance/0.15/mq/structure-status-catalog.json')
MANIFEST = Path('conformance/0.15/manifests/mq-programming-supplements-topics.json')
OUTPUT = Path('crates/contracts/mainframe-env-host-api/src/mq_wire_options/generated.rs')
HISTORICAL_SHA = '3dc77d004bd79ad7f6daa99ffb4a3fb958ff1d816cb6c33bc4b6bac794a23448'
# Immutable reviewed projection fixture; numeric authority remains the catalog.
PROJECTION_SHA = '4d55b1d825762ff4319215185fb240513ab97b0b9b8ff25192b1b0bab12b9d1c'


def read_json(path):
    def unique(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError('duplicate MQ wire JSON field')
            result[key] = value
        return result
    if path.stat().st_size > 2 * 1024 * 1024:
        raise ValueError('MQ wire input byte bound')
    return json.loads(path.read_text(), object_pairs_hook=unique)


def historical_sha(path):
    raw = path.read_bytes()
    catalog = read_json(path)
    keys = {'schema_version', 'target_version', 'work_package', 'baseline_id',
            'topic_manifest', 'source_call_list', 'unique_call_count',
            'verified_call_topic_count', 'missing_call_topic_count',
            'identity_authority', 'behavioral_coverage_credit', 'licensed_execution_credit', 'calls'}
    if 'wire_options' in catalog:
        keys.add('wire_options')
    if 'raw_layout' in catalog:
        keys.add('raw_layout')
    if 'property_profile' in catalog:
        keys.add('property_profile')
    if set(catalog) != keys:
        raise ValueError('historical MQ catalog root fields differ')
    if 'wire_options' in catalog:
        raw = raw.split(b'  "wire_options": ', 1)[0]
        if not raw.endswith(b'  ],\n'):
            raise ValueError('historical MQ catalog framing differs')
        raw = raw.removesuffix(b'  ],\n') + b'  ]\n}\n'
        raw = raw.replace(b'mq-structure-status-catalog@2', b'mq-structure-status-catalog@1', 1)
    result = hashlib.sha256(raw).hexdigest()
    if result != HISTORICAL_SHA:
        raise ValueError('historical MQ catalog identity differs')
    return result


def load(root):
    historical_sha(root / CATALOG)
    catalog = read_json(root / CATALOG)
    value = catalog['wire_options']
    digest = hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
    if digest != PROJECTION_SHA:
        raise ValueError('reviewed MQ wire projection identity differs')
    manifest = read_json(root / MANIFEST)
    if (value['topic_manifest'] != {'path': MANIFEST.as_posix(), 'sha256': hashlib.sha256((root / MANIFEST).read_bytes()).hexdigest()}
            or value['baseline_id'] != manifest['baseline_id']
            or value['scope_id'] != 'mq-programming-supplements'
            or value['historical_catalog_sha256'] != HISTORICAL_SHA
            or value['semantic_execution_credit'] != 0):
        raise ValueError('MQ wire source binding differs')
    topics = {row['topic_path']: row for row in manifest['topics']}
    facts = value['facts']
    if len(facts) != 102 or [f['symbol'] for f in facts] != sorted({f['symbol'] for f in facts}):
        raise ValueError('MQ wire fact count/order/uniqueness differs')
    for source in facts + value['corroboration']:
        if source['topic_path'] not in topics or topics[source['topic_path']]['sha256'] != source['topic_sha256']:
            raise ValueError('MQ wire topic pin differs')
        if not 1 <= source['first_line'] <= source['last_line'] <= 1600:
            raise ValueError('MQ wire fragment bound differs')
    for fact in facts:
        if (type(fact['decimal']) is not int or not 0 <= fact['decimal'] <= 2147483647
                or (fact['hexadecimal'] is not None and int(fact['hexadecimal'], 16) != fact['decimal'])
                or (fact['hexadecimal'] is None and fact['symbol'] != 'MQOD_VERSION_1')):
            raise ValueError('MQ wire signed numeric identity differs')
    return value


def verify_source(root, cache):
    """Reproduce only selected facts using the shared hash-checked reader."""
    value = load(root)
    sys.path.insert(0, str(root / 'conformance/tools'))
    import ibm_docs
    pins, tocs = ibm_docs.select(*ibm_docs.load_pins(), value['scope_id'], None)
    by_topic = {pin.topic: pin for pin in pins}
    for toc in tocs:
        ibm_docs.cached_toc(cache, toc)
    lines = {}
    for source in value['facts'] + value['corroboration']:
        topic = source['topic_path']
        if topic not in lines:
            lines[topic] = ibm_docs.plain_text(ibm_docs.cached_body(cache, by_topic[topic]))
        fragment = lines[topic][source['first_line'] - 1:source['last_line']]
        if hashlib.sha256('\n'.join(fragment).encode()).hexdigest() != source['fragment_sha256']:
            raise ValueError('MQ wire source fragment differs')
        if 'symbol' in source:
            expected = [source['symbol'], str(source['decimal'])]
            if source['hexadecimal'] is not None:
                expected.append("X'" + source['hexadecimal'] + "'")
            if [line.removesuffix(' |') for line in fragment] != expected:
                raise ValueError('MQ wire source numeric fact differs')


def render(root):
    value = load(root)
    out = ['// @generated by tools/generate_mq_mqi_registry.py; do not edit.\n',
           'pub(super) const PROJECTION_SHA256: &str =\n',
           f'    "{PROJECTION_SHA}";\n']
    for fact in value['facts']:
        out.append(f"pub(super) const {fact['symbol']}: i32 = {fact['decimal']};\n")
    for family in ('MQOO', 'MQCO', 'MQGMO', 'MQPMO'):
        names = [f['symbol'] for f in value['facts'] if f['symbol'].startswith(family + '_') and f['kind'] != 'version']
        out.append(f"pub(super) const {family}_KNOWN: i32 = " + '\n    | '.join(names) + ';\n')
    out.append('pub(super) const NUMERIC_IDENTITIES: &[(&str, i32)] = &[\n')
    for fact in value['facts']:
        symbol = fact['symbol']
        line = f'    ("{symbol}", {symbol}),\n'
        if len(line.strip()) > 64:
            line = f'    (\n        "{symbol}",\n        {symbol},\n    ),\n'
        out.append(line)
    out.append('];\n')
    return ''.join(out)
