"""Owned reconciled null-slot property layouts in the ONE MQ source catalog.

The fixed review digest closes offline artifacts; cache-backed review also joins
each source fragment and reproduces named declarations, never a native ABI claim.
"""
import hashlib
import json
from pathlib import Path
import re
import sys

import mq_raw_layout as raw
import mq_wire_options as wire

OUTPUT = Path('crates/contracts/mainframe-env-host-api/src/mq_raw_property/generated.rs')
PROJECTION_SHA = '76bcbc1a91aced75d2ca6c7fd3854593135481537d283d4e787d07f97b2c2fd8'
SCOPES = ('mq-programming-supplements', 'mq-property-sources',
          'mq-point-layout-sources', 'ibm-mq-9.4-mqi-2026-08-31')


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def load(root):
    raw.load(root)  # Historical @1/@2 and raw projection remain separately frozen.
    value = wire.read_json(root / wire.CATALOG).get('raw_property')
    if not isinstance(value, dict) or digest(value) != PROJECTION_SHA:
        raise ValueError('reviewed MQ raw property projection differs')
    if (value['schema_version'] != 'mainframe-env.mq-raw-property-projection@1'
            or value['profile'] != 'OwnedReconciledNullSlot4AsciiNormalV1'
            or value['semantic_execution_credit'] != 0
            or value['historical_catalog_sha256'] != wire.HISTORICAL_SHA):
        raise ValueError('MQ raw property identity/credit differs')
    topics = {}
    for source, scope in zip(value['sources'], SCOPES, strict=True):
        path = root / source['topic_manifest']['path']; manifest = wire.read_json(path)
        if (source['scope_id'] != scope or source['baseline_id'] != manifest['baseline_id']
                or source['topic_manifest']['sha256'] != hashlib.sha256(path.read_bytes()).hexdigest()
                or manifest['product'] != 'SSFKSJ_9.4.0'):
            raise ValueError('MQ raw property source binding differs')
        topics.update({t['topic_path']: t['sha256'] for t in manifest['topics']})
    for source in raw.locators(value):
        if (topics.get(source['topic_path']) != source['topic_sha256']
                or not 1 <= source['first_line'] <= source['last_line'] <= 4000):
            raise ValueError('MQ raw property source locator differs')
    for layout in value['layouts']:
        offset = 0; names = set()
        for field in layout['fields']:
            if (field['name'] in names or field['offset'] != offset
                    or field['kind'] not in ('long', 'null-slot', 'characters')
                    or field['width'] not in (4, 8)
                    or field['kind'] in ('long', 'null-slot') and field['width'] != 4):
                raise ValueError('MQ raw property field/order/width differs')
            offset += field['width']; names.add(field['name'])
        if offset != layout['prefix_bytes']:
            raise ValueError('MQ raw property complete prefix differs')
    return value


def verify_source(root, cache):
    value = load(root)
    sys.path.insert(0, str(root / 'conformance/tools')); import ibm_docs
    pins, tocs = ibm_docs.load_pins(); contents = {}
    needed = {s['topic_path'] for s in raw.locators(value)}
    for source in value['sources']:
        selected, selected_tocs = ibm_docs.select(pins, tocs, source['scope_id'], None)
        for toc in selected_tocs:
            ibm_docs.cached_toc(cache, toc)
        for pin in selected:
            if pin.topic in needed:
                contents[pin.topic] = ibm_docs.plain_text(ibm_docs.cached_body(cache, pin))
    for source in raw.locators(value):
        fragment = contents[source['topic_path']][source['first_line']-1:source['last_line']]
        if hashlib.sha256('\n'.join(fragment).encode()).hexdigest() != source['fragment_sha256']:
            raise ValueError('MQ raw property source fragment differs')
    # Reproduce the complete CHARV order from its coherent C declaration. MQPTR
    # width is deliberately replaced, not inferred from the host's pointer ABI.
    charv = contents['SSFKSJ_9.4.0/refdev/q094690_.html']
    declared = [m.group(1) for line in charv[61:66]
                if (m := re.match(r'MQ(?:PTR|LONG) (\w+);', line))]
    if declared != [f['name'] for f in value['layouts'][0]['fields']]:
        raise ValueError('MQCHARV complete declaration differs')
    impo = contents['SSFKSJ_9.4.0/refdev/q097210_.html']
    names = [m.group(1) for line in impo[70:83]
             if (m := re.match(r'MQ(?:CHAR\d*|LONG|CHARV) (\w+)', line))]
    projected = [f['name'] for f in value['layouts'][1]['fields'] if '.' not in f['name']]
    projected.insert(projected.index('TypeString'), 'ReturnedName')
    if names != projected:
        raise ValueError('MQIMPO reconciled declaration order differs')
    for layout in value['layouts']:
        for field in layout['fields']:
            if field['kind'] == 'long':
                s = field['sources'][0]
                line = contents[s['topic_path']][s['first_line']-1]
                if not line.startswith('MQLONG ' + field['name'].split('.')[-1] + ';'):
                    raise ValueError('MQ raw property long declaration differs')
            elif field['kind'] == 'characters':
                s = field['sources'][0]
                fragment = contents[s['topic_path']][s['first_line']-1:s['last_line']]
                if field['name'] == 'Reserved1':
                    widths = re.findall(r'\((\d+) byte', ' '.join(fragment))
                    if widths != [str(field['width'])]:
                        raise ValueError('MQIMPO reconciled Reserved1 width differs')
                elif not fragment[0].startswith(f"MQCHAR{field['width']} {field['name']};"):
                    raise ValueError('MQ raw property character declaration differs')
            elif field['kind'] == 'null-slot':
                s = field['sources'][0]
                fragment = contents[s['topic_path']][s['first_line']-1:s['last_line']]
                if (field['width'] != value['long']['width']
                        or 'COPY CMQCHRVV REPLACING POINTER BY ==BINARY PIC S9(9)==.' not in fragment):
                    raise ValueError('owned null-slot replacement corroboration differs')
    ident = value['identity']
    for key, expected in [('identifier_source', ['MQIMPO_STRUC_ID', '“IMPO”']),
                          ('version_source', ['MQIMPO_VERSION_1', '1', "X'00000001'"])]:
        s = ident[key]
        fragment = contents[s['topic_path']][s['first_line']-1:s['last_line']]
        if [line.removesuffix(' |') for line in fragment] != expected:
            raise ValueError('MQ raw property ID/version reproduction differs')


def render(root):
    value = load(root)
    out = ['// @generated by tools/generate_mq_mqi_registry.py; do not edit.\n',
           '/// Reviewed owned reconciliation digest; not vendor ABI or execution evidence.\n',
           f'pub const MQ_RAW_PROPERTY_PROJECTION_SHA256: &str = "{PROJECTION_SHA}";\n']
    for name, key in [('MAX_GROUP_BYTES', 'group_bytes'), ('MAX_BATCH_PLANS', 'batch_plans')]:
        out.append(f"pub(super) const {name}: usize = {value['owned_limits'][key]};\n")
    kinds = {'long': 'Long', 'characters': 'Characters', 'null-slot': 'NullSlot'}
    for layout in value['layouts']:
        out.append(f"const {layout['kind'].upper()}_FIELDS: &[MqRawPropertyField] = &[\n")
        for f in layout['fields']:
            out.append('    MqRawPropertyField {\n' +
                       f'        name: "{f["name"]}",\n' +
                       f'        kind: MqRawPropertyFieldKind::{kinds[f["kind"]]},\n' +
                       f'        offset: {f["offset"]},\n        width: {f["width"]},\n' +
                       f'        defined_standard_output: {str(f["defined_standard_output"]).lower()},\n' + '    },\n')
        out.append('];\n')
    out.append('pub(super) const LAYOUTS: &[MqRawPropertyLayout] = &[\n')
    for layout in value['layouts']:
        identifier = 'None' if layout['identifier'] is None else f'Some(*b"{layout["identifier"]}")'
        version = 'None' if layout['version'] is None else f'Some({layout["version"]})'
        out.append('    MqRawPropertyLayout {\n' +
                   f'        kind: MqRawPropertyKind::{layout["kind"]},\n' +
                   f'        prefix_bytes: {layout["prefix_bytes"]},\n' +
                   f'        identifier: {identifier},\n        version: {version},\n' +
                   f'        fields: {layout["kind"].upper()}_FIELDS,\n' + '    },\n')
    out.append('];\n')
    return ''.join(out)
