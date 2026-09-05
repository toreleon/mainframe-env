# #47 — last-used installed COBOL program instances

## Reproduction and supported semantics

The test-only baseline experiment in `remaining-baseline.patch` applied to
`0ccc90b9d5a26f97641af69b2e3875b0af6495e3` returns **1, 1** from two installed
CALLs of the same ordinary working-storage counter. The corrected path returns
**1, 2**, including through a compiled parent CALL and actual linkage copy-back.

IBM's *Comparison of WORKING-STORAGE and LOCAL-STORAGE* documents retained
working-storage, per-call local-storage initialization with VALUE, INITIAL,
CANCEL, and termination of the owning run unit:
<https://www.ibm.com/docs/en/cobol-zos/6.4.0?topic=data-comparison-working-storage-local-storage>.
The accessible public reference is 6.4; the repository's declared target remains
6.5. No licensed 6.5 comparison campaign or equivalence credit is claimed here.
Unspecified LOCAL-STORAGE initial values are not credited as defined semantics.

The compiler now writes an explicit `program_lifecycle` MIR config attribute.
Header classification uses the language lexer, not substring matches in literals
or procedure/data text. A separate last-used-state codec retains working/file
storage roots, appropriate ALTER targets, working/file indexes, file status,
dataset cursors and deterministic random state. New machine entry retains fresh
LOCAL-STORAGE, linkage inputs, PC, call stack, output and condition status.
Retained bytes are validated before installation and replace only the persistent
initializers, including overlapping storage views and subordinate data.

## Ownership, atomicity and restart

Instances are keyed by **principal + run unit + program**, pinned to the installed
artifact. A bounded per-run inventory (at most 256 names) and active count use CAS
alongside the instance reservation. Different run units do not share mutable
instances. Active/reentrant or interrupted instances fail closed, never reset.
The ready state, active-count decrement and exact CALL reply are committed in one
`put_provider_states_atomic` operation. A failed final write leaves both the
instance busy and the reply pending; there is no partial publication or repeated
increment. SQLite reopening preserves last-used state and replay identities.

CANCEL atomically invalidates inactive selected instances and writes its own
idempotent receipt. Replaying an old CANCEL cannot erase state produced by a
subsequent call. Artifact replacement needs CANCEL before a new logical call;
old cached replies keep their original meaning.

Terminal inline batch, installed batch and completed online entrypoints release
last-used instances. Suspended or uncertain runs are not released. Embedders
using raw CALL directly must invoke `DefaultProgramRouter::finish_run_unit` at
their terminal boundary. The terminal marker is durable and prevents accidental
reuse of the run ID. Cleanup uses the bounded run inventory, not a truncated
cross-run store listing. Replay receipts remain for deduplication and are subject
to a separate deployment retention policy.

## Explicit compatibility boundary

Fresh runs use `cobol-call-protocol@2`. Runs admitted by #55's replay-only v1
protocol have no authoritative last-used state and must be drained/reconciled,
not continued with freshly initialized working-storage. Unmarked legacy retries
also remain rejected. Legacy MIR without lifecycle metadata must be recompiled
before installed raw CALL; no ordinary/INITIAL classification is guessed.

This implementation deliberately rejects unsupported retained lifecycles:
RECURSIVE/COMMON/nested program definitions, EXTERNAL/GLOBAL data, explicit ENTRY
aliases, dynamic/unbounded or pointer-bearing layouts and managed object calls.
It does not silently approximate them with fresh state. INITIAL programs with
files and CANCEL/termination of instances with open files are rejected rather
than faking implicit provider file closure. A reached STOP RUN inside a raw
subprogram, a live SQL/sort cursor, or other unsupported return-time runtime state
leaves the call pending/instance busy and reports uncertainty; top-level batch
STOP RUN remains supported. These cases require a separately implemented
cross-provider lifecycle, not an automatic retry or automatic state deletion.

## Verification

Run with the repository-pinned Rust toolchain:

```sh
cargo test --locked -p mainframe-env-compiler -p mainframe-env-interpreter -p mainframe-env-server
cargo clippy --locked -p mainframe-env-compiler -p mainframe-env-interpreter -p mainframe-env-server --all-targets -- -D warnings
```

Regressions cover ordinary 1→2; exact duplicate replay without increment;
INITIAL 1→1; working/local/linkage separation; compiled parent CALL/CANCEL/CALL;
idempotent CANCEL; nested calls; four simultaneous isolated run units; SQLite
reopen; ready-state/reply atomic failure; artifact replacement; unsupported active
CANCEL and lifecycle declarations; and rejection of replay-only legacy runs.
The #48/#49/#55 cancellation, uncertainty and real abrupt-process-exit tests are
executed with this state owner. Candidate SHA/tree, commands and timings belong
in the PR/workflow receipt. This document is not release acceptance.
