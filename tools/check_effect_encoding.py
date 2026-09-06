"""Reject regressions back to diagnostic effect digests or payload-size accounting."""
from pathlib import Path
import re

ROOT=Path(__file__).resolve().parents[1]

def check(root: Path) -> None:
    coordinator=(root/'crates/kernel/mainframe-env-interpreter/src/coordinator.rs').read_text()
    service=(root/'crates/contracts/mainframe-env-host-api/src/service.rs').read_text()
    canonical=(root/'crates/contracts/mainframe-env-host-api/src/canonical.rs').read_text()
    for source in (coordinator,service):
        if re.search(r'format!\s*\(\s*"[^"\n]*\?[^"\n]*"\s*,\s*(?:effect|request)\.(?:request|outcome)',source):
            raise ValueError('Debug is not a persisted effect representation or a payload budget')
    if 'canonical_request_digest(&effect.request)' not in coordinator or 'canonical_result_digest(&result.outcome)' not in coordinator:
        raise ValueError('coordinator must use versioned request/result encoders')
    if 'canonical_request_size(' not in service or 'canonical_result_size(' not in service:
        raise ValueError('host limits must use the canonical typed byte budget')
    if 'format!' in canonical or 'Debug' in canonical:
        raise ValueError('canonical production encoder must not depend on diagnostic formatting')
    durable=(root/'crates/stores/mainframe-env-store/src/durable.rs').read_text()
    for field in ('request_canonical_v1','result_canonical_v1','digest_format'):
        if field not in durable: raise ValueError(f'missing versioned persistence field: {field}')

if __name__=='__main__':
    check(ROOT)
    print('effect-encoding architecture guard: pass')
