# Installed-call replay identity and recovery (#55)

## Baseline experiment, not just a source-derived risk

Product baseline: `0ccc90b9d5a26f97641af69b2e3875b0af6495e3`.
A test-only installation/router fixture committed a real provider-state record
before returning UnknownOutcome. After reopening SQLite, repeating the same
parent execution/run unit/effect occurrence with the process counter set to
98765 created a second business row: `online-call-effect-1:1` and
`online-call-effect-98765:1`. Thus this bounded experiment reproduces identity
instability and duplicate logical effects; it does not claim every provider
or every recovery entry point has the same behavior.

The fixture also reproduced #47's ordinary counter returning 1 then 1.
`remaining-baseline.patch` is the complete test-only delta applied to the pinned
baseline, and `remaining-baseline.log` records the executed observations.
Run it in a disposable worktree at that commit:

```
git apply /absolute/path/to/remaining-baseline.patch
cargo test --locked -p mainframe-env-server baseline_remaining::baseline_ -- --nocapture
```

## New protocol

Installed raw and batch-program calls derive their child request/execution/effect
identities from a versioned length-prefixed SHA-256 domain containing the parent
run unit, parent execution, and parent effect key. Attempt/counter/scheduling are
not new logical operations. Distinct occurrences require distinct effect keys.
All nested children inherit the parent's run unit, but have distinct execution
identities. The input fingerprint also binds sequence, principal/grants,
provider generations, bindings, caller artifact, program and input schema/bytes.

A CAS-created `cobol-call-replay@1` reservation precedes dispatch. Version 2 holds
the exact bounded successful response. A replay returns it without driving the
machine again. Pending, interrupted and concurrently active reservations are
UnknownOutcome, not permission to repeat a possible side effect. A failed result
write is also UnknownOutcome. Installed raw children now have durable journals.

## Compatibility and recovery boundary

This is at-most-once dispatch plus cached success, not automatic exactly-once
recovery. A crash after reservation and before dispatch can need reconciliation,
as can a known failed/cancelled child whose reservation remains pending. Use the
recorded child execution ID to inspect its journal/intents and provider business
state; never delete pending rows merely to make a retry run.

The `cobol-call-protocol@1` marker is established for new run units at attempt 1.
A later attempt without that marker, or an unknown marker version, fails closed.
Do not relabel a legacy recovery as attempt 1: counter-era in-flight run units
must be drained or explicitly reconciled under the old executable before upgrade.
Legacy raw calls have insufficient durable identity to reconstruct automatically.
No legacy digest/key is rewritten or guessed. Inline batch identity remains a
separate entry-point policy; this change covers installed raw/batch calls.

The #47 integration supplies run-owned retained state and atomic state/reply
publication. #57 versions effect-journal digest bytes independently of effect keys.
Keep those cross-layer tests in the final integrated candidate.

## Verification performed

Local Rust 1.98.0 (`88d9e12ae178fab0fb5cc050a94da85685d449ea`):
40 server tests passed; server all-target Clippy with warnings denied passed.
The tests include seven-child successful replay after SQLite reopen; conflicting
input rejection; attempt compatibility; concurrent first admission; and an actual
subprocess exiting with code 55 immediately after the committed business write,
before provider/child/parent completion. Four concurrent retries after unrelated
work retain exactly one business row and do not redispatch the effect.

The #49 capacity fault is now scoped to the execution performing the business
write (4/5 journal events), rather than exhausting capacity at an earlier newly
journaled child admission. All unknown receipts are checked and reconciled.
These are local/model tests, not licensed IBM equivalence or release acceptance.
