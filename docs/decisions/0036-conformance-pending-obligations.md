# ADR 0036: explicit pending Conformance IR obligations

- Status: Proposed (bounded driver foundation)
- Owner: Conformance and MQ maintainers
- Date: 2026-10-03
- Applies from: mainframe-env current subsystem contracts
- Scope: existing shared Conformance IR v1 only

The shared compiler requires a case for every mandatory obligation/gate. A future
required profile could previously be left outside the row specification or given
a fake executable case. Neither describes the full mandatory surface safely.

An optional bounded `pending_reason` identifier on an obligation declares that
its executable binding is still missing. It remains mandatory for all declared
gates; no case may bind it, no verdict can satisfy it, and the existing derived
ledger reports Pending. Absence preserves the previous strict binding closure.
This changes neither applicability nor the six gate meanings. It is not permission
to mark missing supported behavior non-applicable or to count Unsupported as pass.
Old readers fail closed on the new field. Existing specs without it are unchanged.

MQ initially declares all 26 unique rows, retaining original 27 source positions,
with a pending complete-profile obligation. Separately bound finite selected
cases may demonstrate actual obligation progress without completing those rows.
The driver uses the product's selected runtime and shared coordinator/store;
its fixture setup and typed consumer do not prove installed/native/JES execution.
Source pins, source-only recognition and private test declarations grant no credit.

The finite observation counts occurrence receipts and finds a matching attributed
success audit for each original. It does not independently decode every receipt,
prove atomic receipt/audit linkage, or distinguish both audit layers. SQLite
orderly close/reopen checks MQ rows, not cold restore, a complete core graph or
process-crash recovery. Sixteen executions and two conditioned reuses share two
physical backend transcripts; all 26 complete-profile rows remain Pending.

Focused finite runs emit ordinary verdicts and a partial shared ledger. A full
completion request must reject remaining pending obligations. Removing a pending
reason requires real cases/independent expectations and current candidate evidence;
it cannot be cleared by a status edit or copying an old receipt.

MQ fixture identities bind the actual typed Consumer emission source separately
from independent expected output and physical empty setup. The source artifact is
harness input, not compiled COBOL or installed provenance. Registration refuses
a stale emission-source digest; observations refuse stale input/expectation/setup
identities. Whole-tree candidate identity retains its existing code binding.

SAF observations retain every actual ordered original/replay decision delegated
to the same backend's RacfService, including the separately denied fresh PUT.
Shared execution/run/invocation attribution is checked on every synchronous scope;
each observation retains sequence, original key, call, phase, principal, resource,
intent and actual decision. Deduplicated name sets cannot establish this evidence.
The fixed observation budget remains unchanged. These are private harness policy
facts, not installed/JES/native authentication or full-profile acceptance.

The finite ASCII structure source delegates to the Foundation checked encoder
with a 48-byte limit. Invalid characters or width are Unsupported; allocation
failure is ResourceExhausted. This does not add CP037, GMT or JES context support.
