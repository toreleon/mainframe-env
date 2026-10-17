"""Independent first-party byte/math oracle; never import a product implementation.

Source-only work: no Rust, application, native utility, or actual output input.
The local Cp037/default-PIC-X contract is explicitly qualified in the manifest.
"""
from pathlib import Path
import argparse
import base64
import collections
import hashlib
import json
import re

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--corpus', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
OUT = args.output
OUT.mkdir(parents=True, exist_ok=True)
CORPUS = args.corpus
PIN = '59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e'
BASE = 'c8758b2117c1111538a230dbbd3b4f1f9295fe47'
PREFIX = 'AWS.M2.CARDDEMO.'
CURRENT_DATE = '2022070600000000+0000'
PARM_DATE = '2022071800'
TIMESTAMP = '2022-07-06-00.00.00.000000'
SPACE = b'\x40'
POSITIVE = '{ABCDEFGHI'
NEGATIVE = '}JKLMNOPQR'
pins = {}
MANIFEST = Path(__file__).resolve().parents[1] / 'subsystems/jes/oracles/carddemo-financial-reference@1.json'
FROZEN_SOURCE_PINS = json.loads(MANIFEST.read_text())['source_pins']

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def b64(raw):
    return base64.b64encode(raw).decode('ascii')

def offered(relative):
    raw = (CORPUS / relative).read_bytes()
    assert sha(raw) == FROZEN_SOURCE_PINS[relative], relative
    pins[relative] = sha(raw)
    return raw

def cp(text):
    return text.encode('cp037', errors='strict')

def host(raw):
    return raw.decode('cp037', errors='strict').encode('utf8')

def records(relative, length):
    raw = offered(relative)
    assert len(raw) % length == 0
    return [raw[i:i + length] for i in range(0, len(raw), length)]

def signed(raw):
    text = raw.decode('cp037')
    last = text[-1]
    if last in POSITIVE:
        return int(text[:-1] + str(POSITIVE.index(last)))
    if last in NEGATIVE:
        return -int(text[:-1] + str(NEGATIVE.index(last)))
    assert text.isascii() and text.isdigit(), repr(text)
    return int(text)

def zoned(cents, width):
    digits = str(abs(cents)).zfill(width)
    assert len(digits) == width
    return cp(digits[:-1] + (NEGATIVE if cents < 0 else POSITIVE)[int(digits[-1])])

def edited(cents, sign, comma=True):
    # PIC +/-ZZZ,ZZZ,ZZZ.ZZ: fixed leading sign position, zero suppression
    # over nine integer positions and two visible fractional positions.
    if cents == 0:
        return ' ' * (15 if comma else 13)
    digits = f'{abs(cents) // 100:09d}'
    pattern = list(f'{digits[:3]},{digits[3:6]},{digits[6:]}') if comma else list(digits)
    significant = False
    for i, char in enumerate(pattern):
        if char.isdigit() and char != '0':
            significant = True
        if not significant:
            pattern[i] = ' '
    leading = '-' if cents < 0 else ('+' if sign == '+' else ' ')
    return leading + ''.join(pattern) + f'.{abs(cents) % 100:02d}'

def statement_amount(cents, suppressed):
    integer = str(abs(cents) // 100).zfill(9)
    if suppressed:
        integer = integer.lstrip('0').rjust(9)
    return integer + f'.{abs(cents) % 100:02d}' + ('-' if cents < 0 else ' ')

def snapshot(name, rows, length, key=None):
    assert all(len(row) == length for row in rows), name
    return {'dataset': PREFIX + name, 'record_length': length, 'ccsid': 37,
            'key': None if key is None else {'offset': key[0], 'length': key[1]},
            'records': len(rows), 'raw_bytes': sum(map(len, rows)),
            'raw_sha256': sha(b''.join(rows)), 'raw_records_base64': list(map(b64, rows))}

def spool(name, rows):
    return {'program': name, 'scope': 'complete program DISPLAY rows only; no utility/JES log fabrication',
            'ccsid': 1208, 'records': len(rows), 'raw_records_base64': list(map(b64, rows)),
            'raw_joined_lf_sha256': sha(b'\n'.join(rows)), 'more': False}

def text_source(relative):
    return offered(relative).decode('ascii')

assert set(FROZEN_SOURCE_PINS) and len(FROZEN_SOURCE_PINS) == 36
# Every first-party byte read is checked against the immutable pinned-source SHA below.
# Historical source-model parent remains recorded; current repo is never imported or executed.
for relative in [
    'app/cbl/CBTRN02C.cbl', 'app/cbl/CBACT04C.cbl', 'app/cbl/CBTRN03C.cbl',
    'app/cbl/CBSTM03A.CBL', 'app/cbl/CBSTM03B.CBL',
    'app/cpy/CVACT01Y.cpy', 'app/cpy/CVACT03Y.cpy', 'app/cpy/CVTRA01Y.cpy',
    'app/cpy/CVTRA02Y.cpy', 'app/cpy/CVTRA03Y.cpy', 'app/cpy/CVTRA04Y.cpy',
    'app/cpy/CVTRA05Y.cpy', 'app/cpy/CVTRA06Y.cpy', 'app/cpy/CVTRA07Y.cpy',
    'app/cpy/COSTM01.CPY', 'app/cpy/CUSTREC.cpy',
    'app/jcl/POSTTRAN.jcl', 'app/jcl/INTCALC.jcl', 'app/jcl/TRANBKP.jcl',
    'app/jcl/COMBTRAN.jcl', 'app/jcl/TRANIDX.jcl', 'app/jcl/TRANREPT.jcl',
    'app/jcl/PRTCATBL.jcl', 'app/jcl/CREASTMT.JCL', 'app/proc/REPROC.prc',
    'app/ctl/REPROCT.ctl', 'app/jcl/TRANFILE.jcl',
]:
    text_source(relative)
seed_rows = {}
for name, length in [('ACCTDATA.PS', 300), ('TCATBALF.PS', 50), ('CARDXREF.PS', 50),
                     ('DISCGRP.PS', 50), ('DALYTRAN.PS', 350), ('DALYTRAN.PS.INIT', 350),
                     ('TRANTYPE.PS', 60), ('TRANCATG.PS', 60), ('CUSTDATA.PS', 500)]:
    seed_rows[name] = records('app/data/EBCDIC/' + PREFIX + name, length)

# Independently transcribed copybook offsets. Every unmodified byte stays exact.
acct = {row[:11]: row for row in seed_rows['ACCTDATA.PS']}
cat = {row[:17]: row for row in seed_rows['TCATBALF.PS']}
xref = {row[:16]: row for row in seed_rows['CARDXREF.PS']}
disc = {row[:16]: row for row in seed_rows['DISCGRP.PS']}
tran = {row[:16]: row for row in seed_rows['DALYTRAN.PS.INIT']}
assert len(acct) == len(seed_rows['ACCTDATA.PS']) == 50
assert len(cat) == len(seed_rows['TCATBALF.PS']) == 50
assert len(xref) == len(seed_rows['CARDXREF.PS']) == 50
assert len(disc) == len(seed_rows['DISCGRP.PS']) == 51
assert len(tran) == 1
assert all(len(row) == 350 for row in tran.values())
assert all(row[112:122] == SPACE * 10 for row in acct.values())
initial = [snapshot('ACCTDATA.VSAM.KSDS', sorted(acct.values()), 300, (0,11)),
           snapshot('TCATBALF.VSAM.KSDS', sorted(cat.values()), 50, (0,17)),
           snapshot('TRANSACT.VSAM.KSDS', sorted(tran.values()), 350, (0,16))]
unchanged = [snapshot('CARDXREF.VSAM.KSDS', sorted(xref.values()), 50, (0,16)),
             snapshot('DISCGRP.VSAM.KSDS', sorted(disc.values()), 50, (0,16)),
             snapshot('DALYTRAN.PS', seed_rows['DALYTRAN.PS'], 350),
             snapshot('TRANTYPE.VSAM.KSDS', sorted(seed_rows['TRANTYPE.PS']), 60, (0,2)),
             snapshot('TRANCATG.VSAM.KSDS', sorted(seed_rows['TRANCATG.PS']), 60, (0,6)),
             snapshot('CUSTDATA.VSAM.KSDS', sorted(seed_rows['CUSTDATA.PS']), 500, (0,9))]

# Source CBTRN02C 0100-TRANFILE-OPEN opens TRANSACT-FILE OUTPUT.
# Initial one-row snapshot above is preserved; the posted master starts empty.
tran.clear()

# CBTRN02C, including validation precedence and retained FILLER across failed
# category lookup + INITIALIZE (which excludes FILLER).
post_spool = [b'START OF EXECUTION OF PROGRAM CBTRN02C']
rejects, posting_ledger = [], []
cat_filler = SPACE * 22
reason_counts = collections.Counter()
for daily in seed_rows['DALYTRAN.PS']:
    reason, desc = 0, ''
    card = daily[262:278]
    amount = signed(daily[132:143])
    if card not in xref:
        reason, desc = 100, 'INVALID CARD NUMBER FOUND'
    else:
        account_key = xref[card][25:36]
        if account_key not in acct:
            reason, desc = 101, 'ACCOUNT RECORD NOT FOUND'
        else:
            account = acct[account_key]
            temporary = signed(account[78:90]) - signed(account[90:102]) + amount
            if signed(account[24:36]) < temporary:
                reason, desc = 102, 'OVERLIMIT TRANSACTION'
            if account[58:68] < daily[278:288]:
                reason, desc = 103, 'TRANSACTION RECEIVED AFTER ACCT EXPIRATION'
    if reason:
        reject = daily + cp(str(reason).zfill(4) + desc.ljust(76))
        assert len(reject) == 430
        rejects.append(reject)
        reason_counts[reason] += 1
        posting_ledger.append({'transaction_id_base64':b64(daily[:16]), 'accepted':False,
                               'reason':reason, 'raw_reject_base64':b64(reject)})
        continue
    transaction = daily[:304] + cp(TIMESTAMP) + SPACE * 20
    assert len(transaction) == 350 and transaction[:16] not in tran
    category_key = account_key + daily[16:22]
    if category_key in cat:
        old_category = cat[category_key]
        cat_filler = old_category[28:]
        new_category = category_key + zoned(signed(old_category[17:28])+amount,11) + cat_filler
    else:
        post_spool.append(b'TCATBAL record not found for key : ' + host(category_key) + b'.. Creating.')
        new_category = category_key + zoned(amount,11) + cat_filler
    cat[category_key] = new_category
    new_account = bytearray(account)
    new_account[12:24] = zoned(signed(account[12:24]) + amount,12)
    offset = 78 if amount >= 0 else 90
    new_account[offset:offset+12] = zoned(signed(account[offset:offset+12]) + amount,12)
    acct[account_key] = bytes(new_account)
    tran[transaction[:16]] = transaction
    posting_ledger.append({'transaction_id_base64':b64(daily[:16]), 'accepted':True,
                           'account_key_base64':b64(account_key), 'category_key_base64':b64(category_key),
                           'amount_cents':amount, 'raw_transaction_base64':b64(transaction)})
post_spool.extend([b'TRANSACTIONS PROCESSED :000000300',
                   b'TRANSACTIONS REJECTED  :' + str(len(rejects)).zfill(9).encode(),
                   b'END OF EXECUTION OF PROGRAM CBTRN02C'])
post_state = [snapshot('ACCTDATA.VSAM.KSDS', sorted(acct.values()),300,(0,11)),
              snapshot('TCATBALF.VSAM.KSDS', sorted(cat.values()),50,(0,17)),
              snapshot('TRANSACT.VSAM.KSDS', sorted(tran.values()),350,(0,16)),
              snapshot('DALYREJS.G0001V00',rejects,430)]

# CBACT04C: every category has a source DISPLAY, group miss has exactly two
# rows, each nonzero rate emits one full transaction. Account rewrite occurs
# only on a subsequent different key; EOF does not flush the final account.
int_spool = [b'START OF EXECUTION OF PROGRAM CBACT04C']
system_rows, interest_ledger = [], []
last_account, total_interest = None, 0
suffix = 0
for category_key in sorted(cat):
    category = cat[category_key]
    int_spool.append(host(category))
    account_key = category[:11]
    if account_key != last_account:
        if last_account is not None:
            old = acct[last_account]
            changed = bytearray(old)
            changed[12:24] = zoned(signed(old[12:24]) + total_interest,12)
            changed[78:90] = zoned(0,12)
            changed[90:102] = zoned(0,12)
            acct[last_account] = bytes(changed)
        total_interest = 0
        last_account = account_key
        account = acct[account_key]
        xrefs_for_account = [row for row in xref.values() if row[25:36] == account_key]
        assert len(xrefs_for_account) == 1
        account_xref = xrefs_for_account[0]
    rate_key = account[112:122] + category[11:17]
    if rate_key not in disc:
        int_spool.extend([b'DISCLOSURE GROUP RECORD MISSING',b'TRY WITH DEFAULT GROUP CODE'])
        rate_key = cp('DEFAULT'.ljust(10)) + category[11:17]
    assert rate_key in disc
    rate_hundredths = signed(disc[rate_key][16:22])
    category_cents = signed(category[17:28])
    if rate_hundredths != 0:
        numerator = category_cents * rate_hundredths
        monthly_cents = (abs(numerator) // 120000) * (-1 if numerator < 0 else 1)
        total_interest += monthly_cents
        suffix += 1
        transaction = cp(PARM_DATE + str(suffix).zfill(6) + '01' + '0005' + 'System'.ljust(10)
                         + ('Int. for a/c ' + account_key.decode('cp037')).ljust(100))
        transaction += zoned(monthly_cents,11) + cp('0'*9) + SPACE*(50+50+10)
        transaction += account_xref[:16] + cp(TIMESTAMP)*2 + SPACE*20
        assert len(transaction) == 350
        system_rows.append(transaction)
        interest_ledger.append({'category_key_base64':b64(category_key),
            'account_key_base64':b64(account_key),'balance_cents':category_cents,
            'rate_hundredths':rate_hundredths,'monthly_interest_cents':monthly_cents,
            'transaction_id_base64':b64(transaction[:16]),'raw_transaction_base64':b64(transaction)})
int_spool.append(b'END OF EXECUTION OF PROGRAM CBACT04C')
int_state = [snapshot('ACCTDATA.VSAM.KSDS',sorted(acct.values()),300,(0,11)),
             snapshot('TCATBALF.VSAM.KSDS',sorted(cat.values()),50,(0,17)),
             snapshot('TRANSACT.VSAM.KSDS',sorted(tran.values()),350,(0,16)),
             snapshot('SYSTRAN.G0001V00',system_rows,350)]

# The unchanged backup/combine JCL copies the whole OUTPUT-created master,
# then sorts the concatenation by raw ID; no initial row survives POSTTRAN.
backup1 = sorted(tran.values())
combined = sorted(backup1 + system_rows, key=lambda row:row[:16])
assert len({row[:16] for row in combined}) == len(combined)
backup2 = combined
selected = [row for row in backup2 if cp('2022-01-01') <= row[304:314] <= cp('2022-07-06')]
selected = sorted(selected,key=lambda row:row[262:278])
assert all(row[262:278] in xref for row in selected)

# CBTRN03C: frozen first-party literals and widths, including page/account
# delimiters and the source's retained last amount added once again at EOF.
report_rows, report_ledger = [], []
report_spool = [b'START OF EXECUTION OF PROGRAM CBTRN03C',
                b'Reporting from 2022-01-01 to 2022-07-06']
type_rows = {row[:2]:row for row in seed_rows['TRANTYPE.PS']}
category_rows = {row[:6]:row for row in seed_rows['TRANCATG.PS']}
name_header = ('DALYREPT'.ljust(38) + 'Daily Transaction Report'.ljust(41)
               + 'Date Range: '.ljust(12) + '2022-01-01' + ' to ' + '2022-07-06')
header1 = ('Transaction ID'.ljust(17) + 'Account ID'.ljust(12)
           + 'Transaction Type'.ljust(19) + 'Tran Category'.ljust(35)
           + 'Tran Source'.ljust(14) + ' ' + '        Amount'.ljust(16))
dash = cp('-'*133)
line_counter = page_total = account_total = grand_total = 0
current_card = None
page_totals, account_totals = [], []

def report_write(text):
    raw = cp(text) if isinstance(text,str) else text
    assert len(raw) <= 133
    report_rows.append(raw.ljust(133,SPACE))

def report_headers():
    global line_counter
    for row in [name_header,' '*133,header1,dash]:
        report_write(row)
        line_counter += 1

def report_account_total():
    global account_total, line_counter
    report_write('Account Total'.ljust(13) + '.'*84 + edited(account_total,'+'))
    report_write(dash)
    account_totals.append({'card_key_base64':b64(current_card),'amount_cents':account_total})
    account_total = 0
    line_counter += 2

def report_page_total():
    global page_total, grand_total, line_counter
    report_write('Page Total'.ljust(11) + '.'*86 + edited(page_total,'+'))
    report_write(dash)
    page_totals.append(page_total)
    grand_total += page_total
    page_total = 0
    line_counter += 2

for transaction in selected:
    report_spool.append(host(transaction))
    card = transaction[262:278]
    if card != current_card:
        if current_card is not None:
            report_account_total()
        current_card = card
    type_key, category_key = transaction[16:18], transaction[16:22]
    assert type_key in type_rows and category_key in category_rows
    if line_counter == 0:
        report_headers()
    if line_counter % 20 == 0:
        report_page_total()
        report_headers()
    amount = signed(transaction[132:143])
    page_total += amount
    account_total += amount
    detail = (transaction[:16] + SPACE + xref[card][25:36] + SPACE + type_key
              + cp('-') + type_rows[type_key][2:17] + SPACE + transaction[18:22]
              + cp('-') + category_rows[category_key][6:35] + SPACE + transaction[22:32]
              + SPACE*4 + cp(edited(amount,'-')) + SPACE*2)
    assert len(detail) == 114
    report_write(detail)
    line_counter += 1
    report_ledger.append({'transaction_id_base64':b64(transaction[:16]),
                          'card_key_base64':b64(card),'amount_cents':amount})
assert selected
report_spool.extend([b'TRAN-AMT '+host(selected[-1][132:143]),
                     b'WS-PAGE-TOTAL'+host(zoned(page_total,11))])
eof_retained_amount = signed(selected[-1][132:143])
page_total += eof_retained_amount
account_total += eof_retained_amount
report_page_total()
report_write('Grand Total'.ljust(11)+'.'*86+edited(grand_total,'+'))
report_spool.append(b'END OF EXECUTION OF PROGRAM CBTRN03C')

# PRTCATBL: preserve an explicit distinction between independently source-
# defined positive EDIT rows and the existing local negative-sign contract.
# No IBM signed-EDIT authority is inferred from the local implementation.
cat_print_rows, cat_print_ledger = [], []
for row in sorted(cat.values(),key=lambda r:(int(r[:11].decode('cp037')),
                                           r[11:13],int(r[13:17].decode('cp037')))):
    cents = signed(row[17:28])
    literal_digits = str(abs(cents)).zfill(11)
    assert len(literal_digits) == 11
    local_edit = ('-' if cents < 0 else '')+literal_digits[:9]+'.'+literal_digits[9:]
    projected = row[:11]+SPACE+row[11:13]+SPACE+row[13:17]+SPACE+cp(local_edit)+SPACE*9
    cat_print_rows.append(projected[:40].ljust(40,SPACE))
    cat_print_ledger.append({'category_key_base64':b64(row[:17]),'amount_cents':cents,
        'negative_format_authority': 'existing qualified local SORT contract only' if cents < 0 else None})

# CREASTMT's pinned OUTREC produces 328 bytes, then the offered fixed LRECL
# pads to 350. It excludes the last two process-timestamp characters and all
# original FILLER. The complete key is card+transaction; no equal-key tie.
trnx_rows = [row[262:278]+row[:262]+row[278:328]+SPACE*22
             for row in sorted(combined,key=lambda r:(r[262:278],r[:16]))]
assert all(len(row) == 350 for row in trnx_rows)
trnx_by_card = collections.defaultdict(list)
for row in trnx_rows:
    trnx_by_card[row[:16]].append(row)
assert len(trnx_by_card) <= 51 and max(map(len,trnx_by_card.values())) <= 10
assert sum(len(trnx_by_card[card]) for card in xref) == len(selected)

# CBSTM03A HTML 88 literals are independently frozen from the pinned source,
# including fixed-format continuation joins; no product parser is used.
html_literals = {
 'L01':'<!DOCTYPE html>', 'L02':'<html lang="en">', 'L03':'<head>',
 'L04':'<meta charset="utf-8">', 'L05':'<title>HTML Table Layout</title>',
 'L06':'</head>', 'L07':'<body style="margin:0px;">',
 'L08':'<table  align="center" frame="box" style="width:70%; font:12px Segoe UI,sans-serif;">',
 'LTRS':'<tr>', 'LTRE':'</tr>', 'LTDS':'<td>', 'LTDE':'</td>',
 'L10':'<td colspan="3" style="padding:0px 5px;background-color:#1d1d96b3;">',
 'L15':'<td colspan="3" style="padding:0px 5px;background-color:#FFAF33;">',
 'L16':'<p style="font-size:16px">Bank of XYZ</p>',
 'L17':'<p>410 Terry Ave N</p>', 'L18':'<p>Seattle WA 99999</p>',
 'L22-35':'<td colspan="3" style="padding:0px 5px;background-color:#f2f2f2;">',
 'L30-42':'<td colspan="3" style="padding:0px 5px;background-color:#33FFD1; text-align:center;">',
 'L31':'<p style="font-size:16px">Basic Details</p>',
 'L43':'<p style="font-size:16px">Transaction Summary</p>',
 'L47':'<td style="width:25%; padding:0px 5px; background-color:#33FF5E; text-align:left;">',
 'L48':'<p style="font-size:16px">Tran ID</p>',
 'L50':'<td style="width:55%; padding:0px 5px; background-color:#33FF5E; text-align:left;">',
 'L51':'<p style="font-size:16px">Tran Details</p>',
 'L53':'<td style="width:20%; padding:0px 5px; background-color:#33FF5E; text-align:right;">',
 'L54':'<p style="font-size:16px">Amount</p>',
 'L58':'<td style="width:25%; padding:0px 5px; background-color:#f2f2f2; text-align:left;">',
 'L61':'<td style="width:55%; padding:0px 5px; background-color:#f2f2f2; text-align:left;">',
 'L64':'<td style="width:20%; padding:0px 5px; background-color:#f2f2f2; text-align:right;">',
 'L75':'<h3>End of Statement</h3>', 'L78':'</table>', 'L79':'</body>', 'L80':'</html>',
}
# Read-back validates, rather than constructs, the independently transcribed
# expected HTML literals. Only the offered fixed-format source area is read.
html_readback = {}
literal_blocks = re.findall(r'88\s+HTML-([A-Z0-9-]+)(.*?)(?=\n\s*88\s+HTML-|\n\s*05\s+HTML-L11\.)',
                           text_source('app/cbl/CBSTM03A.CBL'),re.S)
for key, block in literal_blocks:
    value_started = False
    parts = []
    for line in block.splitlines():
        area = line[7:72] if len(line) >= 7 and line[:6].isspace() else line
        if not value_started:
            if 'VALUE ' not in area:
                continue
            remainder = area.split('VALUE ',1)[1]
            assert "'" in remainder
            literal = remainder.split("'",1)[1]
            value_started = True
        else:
            assert line[6:7] == '-', (key,line)
            literal = area.lstrip().removeprefix("'")
        if "'" in literal:
            parts.append(literal.split("'",1)[0])
            break
        parts.append(literal)
    html_readback[key] = ''.join(parts)
assert html_readback == html_literals, [(k,html_readback.get(k),v) for k,v in html_literals.items()
                                      if html_readback.get(k) != v]
statement_ps, statement_html, statement_ledger = [], [], []
customers = {row[:9]:row for row in seed_rows['CUSTDATA.PS']}

def fixed_html(text):
    raw = cp(text)
    assert len(raw) <= 100
    statement_html.append(raw.ljust(100,SPACE))

def emit_html(*keys):
    for key in keys:
        fixed_html(html_literals[key])

def delimiter(text, token):
    return text.split(token,1)[0]

for card in sorted(xref):
    x = xref[card]
    customer = customers[x[16:25]]
    account = acct[x[25:36]]
    name = ' '.join(delimiter(customer[a:b].decode('cp037'),' ')
                    for a,b in [(9,34),(34,59),(59,84)])+' '
    name = name.ljust(75)
    add1, add2 = customer[84:134].decode('cp037'),customer[134:184].decode('cp037')
    add3 = (' '.join(delimiter(customer[a:b].decode('cp037'),' ')
                     for a,b in [(184,234),(234,236),(236,239),(239,249)])+' ').ljust(80)
    acct_id = account[:11].decode('cp037').ljust(20)
    balance = statement_amount(signed(account[12:24]),False)
    fico = customer[329:332].decode('cp037').ljust(20)
    headers = ['*'*31+'START OF STATEMENT'+'*'*31,
               name+' '*5,add1+' '*30,add2+' '*30,add3,'-'*80,
               ' '*33+'Basic Details'.ljust(14)+' '*33,'-'*80,
               'Account ID         :'.ljust(20)+acct_id+' '*40,
               'Current Balance    :'.ljust(20)+balance+' '*47,
               'FICO Score         :'.ljust(20)+fico+' '*40,'-'*80,
               ' '*30+'TRANSACTION SUMMARY '+' '*30,'-'*80,
               'Tran ID         '+'Tran Details    '.ljust(51)+'  Tran Amount','-'*80]
    assert all(len(text) == 80 for text in headers)
    statement_ps.extend(map(cp,headers))
    emit_html('L01','L02','L03','L04','L05','L06','L07','L08','LTRS','L10')
    fixed_html('<h3>Statement for Account Number: '.ljust(34)+acct_id+'</h3>')
    emit_html('LTDE','LTRE','LTRS','L15','L16','L17','L18','LTDE','LTRE','LTRS','L22-35')
    fixed_html('<p style="font-size:16px">'+delimiter(name[:50],'  ')+'  </p>')
    for address in [add1,add2,add3]:
        fixed_html('<p>'+delimiter(address,'  ')+'  </p>')
    emit_html('LTDE','LTRE','LTRS','L30-42','L31','LTDE','LTRE','LTRS','L22-35')
    fixed_html('<p>Account ID         : '+delimiter(acct_id,'*')+'</p>')
    fixed_html('<p>Current Balance    : '+balance+'</p>')
    fixed_html('<p>FICO Score         : '+fico+'</p>')
    emit_html('LTDE','LTRE','LTRS','L30-42','L43','LTDE','LTRE','LTRS',
              'L47','L48','LTDE','L50','L51','LTDE','L53','L54','LTDE','LTRE')
    total, ids = 0, []
    for trnx in trnx_by_card[card]:
        transaction_id = trnx[16:32].decode('cp037')
        desc = trnx[48:97].decode('cp037')
        amount = signed(trnx[148:159])
        printed = statement_amount(amount,True)
        ps_detail = cp(transaction_id+' '+desc+'$'+printed)
        assert len(ps_detail) == 80
        statement_ps.append(ps_detail)
        emit_html('LTRS','L58')
        fixed_html('<p>'+delimiter(transaction_id,'*')+'</p>')
        emit_html('LTDE','L61')
        fixed_html('<p>'+delimiter(desc,'*')+'</p>')
        emit_html('LTDE','L64')
        fixed_html('<p>'+printed+'</p>')
        emit_html('LTDE','LTRE')
        total += amount
        ids.append(b64(trnx[16:32]))
    statement_ps.extend([cp('-'*80),cp('Total EXP:'+' '*56+'$'+statement_amount(total,True)),
                         cp('*'*32+'END OF STATEMENT'+'*'*32)])
    emit_html('LTRS','L10','L75','LTDE','LTRE','L78','L79','L80')
    statement_ledger.append({'card_key_base64':b64(card),'account_key_base64':b64(x[25:36]),
        'customer_key_base64':b64(x[16:25]),'transaction_ids_base64':ids,'total_cents':total})
assert len(statement_ps) == 50*19+len(selected)
assert len(statement_html) == 50*64+len(selected)*11

result = {
    'schema':'external.carddemo.financial-source-oracle@1',
    'corpus_commit':PIN,'base_commit':BASE,'source_pins':pins,
    'current_date':CURRENT_DATE,'timestamp':TIMESTAMP,'interest_parm':PARM_DATE,
    'authority':'independent pinned raw seed bytes and literal first-party program/JCL/copybook rules; no product encoder/parser or actual output input',
    'qualification':['actual compiled local route still required',
        'existing manual Cp037 PS source seed helper; no first-party install/workflow parity',
        'existing one-record ASCII REPROCT helper; full-source AMS comments remain unsupported',
        'existing DUSRSECJ inline Cp037 carrier gap remains separate and unresolved',
        'local Cp037 signed zoned/PIC-X initialization/numeric editing contract; no IBM licensed runtime equivalence',
        'TRANREPT equal-card order uses the declared local stable input order; DFSORT default ties are unspecified',
        'PRTCATBL negative EDIT sign-prefix/truncation fixture is explicitly local; independent IBM authority unavailable'],
    'initial_mutating_targets':initial,'unchanged_sources':unchanged,
    'posttran':{'return_code':4 if rejects else 0,'processed':300,'accepted':300-len(rejects),
        'rejected':len(rejects),'reason_counts':dict(reason_counts),'ledger':posting_ledger,
        'datasets':post_state,'display_spool':spool('CBTRN02C',post_spool)},
    'intcalc':{'return_code':0,'final_account_not_flushed_base64':b64(last_account),
        'final_account_unapplied_interest_cents':total_interest,'ledger':interest_ledger,
        'datasets':int_state,'display_spool':spool('CBACT04C',int_spool)},
    'backup_combine_selected':[
        snapshot('TRANSACT.BKUP.G0001V00',backup1,350),
        snapshot('TRANSACT.COMBINED.G0001V00',combined,350),
        snapshot('TRANSACT.VSAM.KSDS',combined,350,(0,16)),
        snapshot('TRANSACT.BKUP.G0002V00',backup2,350),
        snapshot('TRANSACT.DALY.G0001V00',selected,350)],
    'tranrept':{'return_code':0,'transaction_ledger':report_ledger,'page_totals_cents':page_totals,
        'account_totals':account_totals,'source_eof_retained_amount_added_cents':eof_retained_amount,
        'source_final_account_total_not_written_cents':account_total,'grand_total_cents':grand_total,
        'dataset':snapshot('TRANREPT.G0001V00',report_rows,133),
        'display_spool':spool('CBTRN03C',report_spool)},
    'prtcatbl':{'qualification':'negative EDIT formatting is only the independently stated existing local contract; no upstream signed EDIT claim',
        'ledger':cat_print_ledger,'backup':snapshot('TCATBALF.BKUP.G0001V00',sorted(cat.values()),50),
        'dataset':snapshot('TCATBALF.REPT',cat_print_rows,40)},
    'creastmt':{'qualification':'existing exact one-line malformed JCL correction; offered local compiled CALL/ALTER/default-PIC-X/numeric edit contracts',
        'table_card_groups':len(trnx_by_card),'table_max_transactions_per_card':max(map(len,trnx_by_card.values())),
        'source_jcl_outrec_unpadded_bytes':328,'source_jcl_outrec_fixed_pad_bytes':22,
        'ledger':statement_ledger,'datasets':[snapshot('TRXFL.SEQ',trnx_rows,350),
            snapshot('TRXFL.VSAM.KSDS',trnx_rows,350,(0,32)),
            snapshot('STATEMNT.PS',statement_ps,80),snapshot('STATEMNT.HTML',statement_html,100)],
        'display_spool':None,'spool_blocker':'independent offered PSA/TCB/TIOT pointer-image and DD/UCB oracle not yet captured; do not credit full spool from successful exit'},
    'native_runs':0,'application_credit':0,'observation_tokens':[],
}
(OUT/'financial-oracle.json').write_text(json.dumps(result,indent=2)+'\n')
(OUT/'source-checks.json').write_text(json.dumps({
    'source_pins':len(pins),'daily_inputs':len(seed_rows['DALYTRAN.PS']),
    'post_accepted':300-len(rejects),'post_rejected':len(rejects),
    'post_category_records':len(cat),'system_interest_records':len(system_rows),
    'combined_records':len(combined),'tranrept_selected_records':len(selected),
    'post_program_display_rows':len(post_spool),'interest_program_display_rows':len(int_spool),
    'report_records':len(report_rows),'report_program_display_rows':len(report_spool),
    'statement_ps_records':len(statement_ps),'statement_html_records':len(statement_html),
    'statement_table_card_groups':len(trnx_by_card),
    'statement_table_max_transactions_per_card':max(map(len,trnx_by_card.values())),
    'frozen_html_literals_read_back':len(html_readback),
    'legacy_sort_negative_edit_rows':sum(x['amount_cents'] < 0 for x in cat_print_ledger),
    'oracle_sha256':sha((OUT/'financial-oracle.json').read_bytes()),
    'native_runs':0,'application_credit':0},indent=2)+'\n')
print(json.dumps({'source_pins':len(pins),'accepted':300-len(rejects),'rejects':len(rejects),
                  'categories':len(cat),'interest_records':len(system_rows),'combined':len(combined),
                  'selected':len(selected),'native_runs':0,'application_credit':0}))

# This companion describes raw record comparisons only. It deliberately does
# not reuse the legacy dataset digest's u64 attribute/version framing.
def comparison(target, record_format):
    values = [base64.b64decode(value, validate=True)
              for value in target['raw_records_base64']]
    digest = hashlib.sha256()
    for value in values:
        digest.update(len(value).to_bytes(4, 'big'))
        digest.update(value)
    return {'dataset': target['dataset'], 'records': len(values),
            'attributes': {'organization': 'key-sequenced' if target['key'] else 'sequential',
                'record_format': record_format, 'logical_record_length': target['record_length'],
                'key_offset': target['key']['offset'] if target['key'] else None,
                'key_length': target['key']['length'] if target['key'] else None, 'ccsid': 37},
            'record_u32_framed_sha256': digest.hexdigest()}

assert sha((OUT / 'financial-oracle.json').read_bytes()) == json.loads(MANIFEST.read_text())['oracle_sha256']

comparisons = {
    'posttran-tcatbalf': comparison(result['posttran']['datasets'][1], 'fixed'),
    'tranrept-selected': comparison(result['backup_combine_selected'][4], 'fixed-blocked'),
    'tranrept-report': comparison(result['tranrept']['dataset'], 'fixed-blocked'),
}
assert comparisons == json.loads(MANIFEST.read_text())['comparisons']
(OUT / 'financial-reference-results.json').write_text(
    json.dumps(comparisons, indent=2) + '\n')
