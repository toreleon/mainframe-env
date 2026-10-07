"""Independent private raw layout projection in the ONE MQ structure catalog."""

import hashlib
import json
from pathlib import Path
import re
import sys

import mq_wire_options as wire

OUTPUT = Path('crates/contracts/mainframe-env-host-api/src/mq_raw_layout/generated.rs')
PROJECTION_SHA = '8f3ca67fe72efe6cd4c6c044086de3e9f5d1feebe4390f8163709a5936cb633e'
PREVIOUS_PROJECTION_SHA = '328ced5efce5d0ca96f7984d13ad3bb168dcc8518097f4535601a5278ddaae91'
SCOPES = ('mq-programming-supplements', 'mq-point-layout-sources', 'ibm-mq-9.4-mqi-2026-08-31')


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def previous_projection(value):
    """Reconstruct exact reviewed five-layout artifact, never relabel its identity."""
    previous = json.loads(json.dumps(value))
    del previous['previous_projection_sha256']
    del previous['connx_input']
    previous['sources'].pop()
    previous['layouts'].pop()
    previous['identifier_sources'].pop()
    if digest(previous) != PREVIOUS_PROJECTION_SHA:
        raise ValueError('previous MQ raw layout projection differs')
    return previous


def locators(value):
    if isinstance(value, dict):
        if 'fragment_sha256' in value:
            yield value
        else:
            for child in value.values():
                yield from locators(child)
    elif isinstance(value, list):
        for child in value:
            yield from locators(child)


def load(root):
    wire.load(root)  # Preserves the historical call identity and frozen @2 wire projection.
    value = wire.read_json(root / wire.CATALOG)['raw_layout']
    if digest(value) != PROJECTION_SHA:
        raise ValueError('reviewed MQ raw layout projection differs')
    previous_projection(value)
    if (value['schema_version'] != 'mainframe-env.mq-raw-layout-projection@1'
            or value['semantic_execution_credit'] != 0
            or value['historical_catalog_sha256'] != wire.HISTORICAL_SHA
            or value['wire_options_projection_sha256'] != wire.PROJECTION_SHA):
        raise ValueError('MQ raw layout identity/credit differs')
    topics = {}
    for source, scope in zip(value['sources'], SCOPES, strict=True):
        path = root / source['topic_manifest']['path']
        manifest = wire.read_json(path)
        if (source['scope_id'] != scope or source['baseline_id'] != manifest['baseline_id']
                or source['topic_manifest']['sha256'] != hashlib.sha256(path.read_bytes()).hexdigest()
                or manifest['product'] != 'SSFKSJ_9.4.0'):
            raise ValueError('MQ raw layout manifest binding differs')
        for topic in manifest['topics']:
            topics[topic['topic_path']] = topic['sha256']
    for source in locators(value):
        if (topics.get(source['topic_path']) != source['topic_sha256']
                or not 1 <= source['first_line'] <= source['last_line'] <= 4000):
            raise ValueError('MQ raw layout locator/pin differs')
    for layout in value['layouts']:
        offset = 0
        names = set()
        for field in layout['fields']:
            if (field['name'] in names or field['offset'] != offset
                    or type(field['width']) is not int or not 1 <= field['width'] <= 48
                    or field['kind'] in ('long', 'alias', 'signal-slot') and field['width'] != 4):
                raise ValueError('MQ raw layout overlap/type/width differs')
            offset += field['width']; names.add(field['name'])
        if offset != layout['prefix_bytes'] or len(layout['identifier']) != 4:
            raise ValueError('MQ raw layout prefix differs')
    owned = value['owned_character_table']
    if hashlib.sha256((root / owned['path']).read_bytes()).hexdigest() != owned['sha256']:
        raise ValueError('owned embedding character profile differs')
    facts = value['connx_input']['facts']
    if len(facts) != 29 or [f['symbol'] for f in facts] != sorted({f['symbol'] for f in facts}):
        raise ValueError('MQCNO numeric fact count/order differs')
    for fact in facts:
        if (type(fact['decimal']) is not int or not 0 <= fact['decimal'] <= 2147483647
                or int(fact['hexadecimal'], 16) != fact['decimal']):
            raise ValueError('MQCNO numeric identity differs')
    return value


def verify_source(root, caches):
    """Reproduce selected hash-bound fragments and declarations, offline only."""
    value = load(root)
    sys.path.insert(0, str(root / 'conformance/tools'))
    import ibm_docs
    all_pins, all_tocs = ibm_docs.load_pins()
    contents = {}
    for source in value['sources']:
        scope = source['scope_id']
        pins, tocs = ibm_docs.select(all_pins, all_tocs, scope, None)
        cache = caches[scope]
        for toc in tocs:
            ibm_docs.cached_toc(cache, toc)
        needed = {s['topic_path'] for s in locators(value)}
        for pin in pins:
            if pin.topic in needed:
                contents[pin.topic] = ibm_docs.plain_text(ibm_docs.cached_body(cache, pin))
    for source in locators(value):
        fragment = contents[source['topic_path']][source['first_line']-1:source['last_line']]
        if hashlib.sha256('\n'.join(fragment).encode()).hexdigest() != source['fragment_sha256']:
            raise ValueError('MQ raw layout source fragment differs')
        if 'symbol' in source:
            expected = [source['symbol'], str(source['decimal']), "X'" + source['hexadecimal'] + "'"]
            if [line.removesuffix(' |') for line in fragment] != expected:
                raise ValueError('MQCNO source numeric fact differs')
    for layout in value['layouts']:
        lines = contents[layout['c_declaration']['topic_path']]
        declaration = layout['c_declaration']
        declared = [match.group(1) for line in lines[declaration['first_line']-1:declaration['last_line']]
                    if (match := re.match(r'MQ(?:CHAR\d*|BYTE\d+|LONG|HOBJ) (\w+);', line))]
        if declared != [field['name'] for field in layout['fields']]:
            raise ValueError('MQ raw layout complete declaration order differs')
        for field in layout['fields']:
            c = lines[field['c_line']-1]
            cobol = lines[field['cobol_line']-1]
            ctype = {'long': 'MQLONG', 'alias': 'MQHOBJ', 'signal-slot': 'MQLONG',
                     'bytes': f"MQBYTE{field['width']}", 'characters': f"MQCHAR{field['width']}"}[field['kind']]
            if not c.startswith(f"{ctype} {field['name']};"):
                raise ValueError('MQ raw layout C field identity differs')
            picture = 'S9(9) BINARY' if field['kind'] in ('long', 'alias', 'signal-slot') else f"X({field['width']})"
            if cobol != f"15 {layout['family']}-{field['name'].upper()} PIC {picture}.":
                raise ValueError('MQ raw layout COBOL width/order differs')
            initial = field['initial']
            if initial['kind'] == 'long':
                s = initial['source']
                fragment = lines[s['first_line']-1:s['last_line']]
                if fragment[-1] != f"{initial['value']} |":
                    raise ValueError('MQ raw layout initial scalar observation differs')
        # The separate constants topic supplies the exact blank-padded identifier.
        identifier_source = next(s for s in value['identifier_sources']
                                 if layout['family'] + '_STRUC_ID |' in
                                 contents[s['topic_path']][s['first_line']-1:s['last_line']])
        fragment = contents[identifier_source['topic_path']][identifier_source['first_line']-1:identifier_source['last_line']]
        identifiers = [line.strip('“” |').replace('¬', ' ') for line in fragment if line.startswith('“')]
        if identifiers != [layout['identifier']]:
            raise ValueError('MQ raw layout identifier fact differs')


def render(root):
    value = load(root)
    # Reuse the owned CP037 mapping, without adding a second character codec/dependency.
    raw = (root / value['owned_character_table']['path']).read_text()
    table = re.search(r'const CP037_TO_LATIN1: \[u8; 256\] = \[(.*?)\];', raw, re.S)
    mapping = [int(x, 16) for x in re.findall(r'0x[0-9a-f]+', table.group(1))]
    if len(mapping) != 256 or len(set(mapping)) != 256:
        raise ValueError('owned CP037 profile is not bijective')
    out = ['// @generated by tools/generate_mq_mqi_registry.py; do not edit.\n',
           f'pub const MQ_RAW_LAYOUT_PROJECTION_SHA256: &str = "{PROJECTION_SHA}";\n',
           f"pub(super) const MAX_PREFIX: usize = {max(l['prefix_bytes'] for l in value['layouts'])};\n",
           f"pub(super) const COBOL_LONG_MIN: i32 = {value['elementary']['cobol_long_min']};\n",
           f"pub(super) const COBOL_LONG_MAX: i32 = {value['elementary']['cobol_long_max']};\n"]
    kinds = {'long': 'Long', 'alias': 'Alias', 'signal-slot': 'SignalSlot', 'bytes': 'Bytes', 'characters': 'Characters'}
    policies = {'input': 'Input', 'get': 'Get', 'get-put': 'GetPut', 'dynamic-open': 'DynamicOpen',
                'put-count-other': 'PutCountOther', 'single-queue-put': 'SingleQueuePut',
                'pending-output': 'PendingOutput'}
    for layout in value['layouts']:
        out.append(f"const {layout['kind'].upper()}_FIELDS: &[MqRawFieldDescriptor] = &[\n")
        for field in layout['fields']:
            initial = field['initial']
            default = f"MqRawInitialValue::Long({initial['value']})" if initial['kind'] == 'long' else 'MqRawInitialValue::' + initial['kind'].title()
            out.extend(['    MqRawFieldDescriptor {\n', f"        name: {json.dumps(field['name'])},\n",
                        f"        kind: MqRawFieldKind::{kinds[field['kind']]},\n",
                        f"        offset: {field['offset']},\n", f"        width: {field['width']},\n",
                        f"        initial: {default},\n",
                        f"        writeback: WritebackPolicy::{policies[field['writeback']]},\n", '    },\n'])
        out.append('];\n')
    out.append('pub(super) const LAYOUTS: &[MqRawLayoutDescriptor] = &[\n')
    for layout in value['layouts']:
        ident = layout['identifier']
        cp = [mapping.index(ord(c)) for c in ident]
        out.extend(['    MqRawLayoutDescriptor {\n',f"        kind: MqRawLayoutKind::{layout['kind']},\n",
                    f"        version: {layout['version']},\n",f"        prefix_bytes: {layout['prefix_bytes']},\n",
                    f'        ascii_identifier: *b"{ident}",\n',f"        cp037_identifier: {cp},\n",
                    f"        fields: {layout['kind'].upper()}_FIELDS,\n",'    },\n'])
    out.append('];\n')
    for fact in value['connx_input']['facts']:
        out.append(f"pub(super) const {fact['symbol']}: i32 = {fact['decimal']};\n")
    names = [f['symbol'] for f in value['connx_input']['facts'] if f['kind'] == 'option']
    out.append('pub(super) const MQCNO_KNOWN: i32 = ' + '\n    | '.join(names) + ';\n')
    out.append('pub(super) const MQCNO_ADMITTED: &[i32] = &[' + ', '.join(value['connx_input']['admitted_options_symbols']) + '];\n')
    out.append('pub(super) const MQCNO_NUMERIC_IDENTITIES: &[(&str, i32)] = &[\n')
    for fact in value['connx_input']['facts']:
        symbol = fact['symbol'];out.append(f'    ("{symbol}", {symbol}),\n')
    out.append('];\n')
    return ''.join(out)
