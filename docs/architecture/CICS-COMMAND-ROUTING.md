# CICS command descriptor and semantic-family routing

Status: **Implemented**
Owner: **CICS provider maintainers**
Scope: **source-reviewed application-command contracts, generated registry shapes, and existing provider routes**
Applies from: **mainframe-env 0.9.0 development**

## Authorities

The readable identity authority is
[`command-descriptors.json`](../../conformance/0.9/cics/command-descriptors.json),
validated by
[`cics-command-descriptors.schema.json`](../../conformance/0.9/schemas/cics-command-descriptors.schema.json).
Its `application_catalog` contains the 263 mandatory `api-commands` row IDs,
official labels and two-byte EIB function codes. The separate `runtime`
collection retains 23 API operations and two explicit SPI compatibility
operations; it does not enlarge the application denominator or make the SPI
operations application-registry routes.

Objective command facts come from digest-pinned IBM CICS TS 6.x HTML, divided
into three disjoint source authorities:

- `sources-a` covers rows `0001`–`0088`: [map](../../conformance/0.9/cics/application-api-sources-a-map.json),
  [corpus](../../conformance/0.9/cics/application-api-sources-a-corpus.json),
  [projection](../../conformance/0.9/generated/cics-application-api-sources-a-candidates.json),
  and [review](../../conformance/0.9/cics/application-api-sources-a-review.json).
- `sources-b` covers rows `0089`–`0176`: [map](../../conformance/0.9/cics/application-api-sources-b-map.json),
  [corpus](../../conformance/0.9/cics/application-api-sources-b-corpus.json),
  [projection](../../conformance/0.9/generated/cics-application-api-sources-b-candidates.json),
  and [review](../../conformance/0.9/cics/application-api-sources-b-review.json).
- `sources-c` covers rows `0177`–`0263`: [map](../../conformance/0.9/cics/application-api-sources-c-map.json),
  [corpus](../../conformance/0.9/cics/application-api-sources-c-corpus.json),
  [projection](../../conformance/0.9/generated/cics-application-api-sources-c-candidates.json),
  and [review](../../conformance/0.9/cics/application-api-sources-c-review.json).

Each batch binds a topic manifest and extraction plan to content-addressed HTML
bodies in `$MAINFRAME_ENV_IBM_DOCS_CACHE`. Fresh bodies are obtained through the
repository browser-fetch bridge and the user's Chrome session; PDF is not a
source path. The projector emits structural facts and fragment hashes, while
the independent verifier reparses the pinned HTML before comparing the
projection. Reviews auto-accept objective matches, fail on source or
reprojection gaps, and retain row-and-dimension bounded ambiguities. There is no
human-only approval gate. Source projection and review alone grant no coverage,
semantic, execution or differential credit.

The semantic authority produced from all three accepted reviews is
[`cics-application-command-contracts.json`](../../conformance/0.9/generated/cics-application-command-contracts.json),
validated by its
[`schema`](../../conformance/0.9/schemas/cics-application-command-contracts.schema.json).
It freezes a 263-row contract across grammar, option legality and direction,
bounds, resource and capability intent, EIB/RESP/RESP2 and conditions,
applicability, effect, cancellation, audit and recovery. Its status is
`frozen-with-bounded-ambiguities`: all rows are closed as either source-resolved,
bounded, or not applicable, but bounded dimensions are not represented as
resolved. The contract explicitly has `execution_authority=false` and zero
coverage, semantic and differential credit.

The contract also binds one 121-name EIBRESP authority for dynamic
`HANDLE CONDITION` and `IGNORE CONDITION` clauses. Its early participant view
records 20 known mutating rows, 225 bounded-effect rows, one explicit UOW
boundary and 244 bounded-UOW rows. Those values describe current contract
certainty; they are not execution or conformance counts.

The same generator emits the compact
[`CICS application IR registry`](../../crates/foundation/mainframe-env-ir/src/generated/cics_application_registry.rs).
It contains all 263 API registry shapes with deterministic recognition,
option-shape, family, EIBFN and handler identities. Readiness is deliberately
split:

- 38 `typed-runtime` API routes, including the original `READ`, `REWRITE`, and
  `SYNCPOINT` routes and the reviewed incremental family slices;
- 0 `legacy-compatibility` API routes; and
- 225 `unready` rows that are recognized but fail explicitly as unsupported.

The now-empty raw compatibility set remains owned by the separate versioned
[`legacy-execution-options.json`](../../conformance/0.9/cics/legacy-execution-options.json)
catalog and its
[`schema`](../../conformance/0.9/schemas/cics-legacy-execution-options.schema.json).
It binds the logical 263-row application identity digest, not the physical
descriptor file, so runtime-readiness edits cannot invalidate frozen IBM
source receipts. The generator requires its route identities to match the
legacy API runtime set exactly and verifies every admitted option against a
current accepted source projection before emitting the registry.

The current 38 API routes are the only advertised application commands.
`ASKTIME ABSTIME` returns its packed-decimal destination and refreshes EIBDATE
and EIBTIME. Bare `ASKTIME` is a distinct route that refreshes only those two
packed-decimal EIB fields; it cannot manufacture an ABSTIME destination. Both
forms lower through distinct typed plan operations, while retained raw
ASKTIME artifacts remain readable by the compatibility interpreter.
The typed FORMATTIME route currently admits only its source-checked legacy
subset: packed ABSTIME input, valued DATESEP/TIMESEP, five explicit date
formats, TIME, MILLISECONDS, and common response options. Other official
FORMATTIME fields remain explicit compiler rejections until their output and
timezone contracts are implemented.
The typed DELAY route completes bare/default and literal-zero INTERVAL without
timer state. Valid positive packed literal INTERVAL values bind a hidden
task/statement identity, persist one versioned cycle and shared work item, and
reissue from the durable checkpoint only after lease-fenced due promotion.
Positive literal delays may bind a bounded REQID for other-task local CANCEL;
task teardown abandons their durable cycle and cancels work. `FOR` and `UNTIL`
also accept literal or resolved numeric HOURS/MINUTES/SECONDS and retain their
mode in append-only typed-plan tags. Packed TIME accepts an integer constant or
resolved packed numeric storage and uses the same absolute deadline path.
`FOR MILLISECS` accepts literal or resolved fullword values alone or with the
other units and retains millisecond precision through the durable deadline.
Packed INTERVAL accepts either a literal or resolved packed numeric value under
the existing plan tag. A due DELAY worker also resolves the bounded durable
online exchange by run-unit identity and resumes it without a client resume
request, including after the SQLite-backed product/store is reopened. PostgreSQL
restart wake and remote cancellation remain explicit gaps.
`ABEND` also lowers through a typed task plan: ABCODE is captured as a bounded
literal or a pre-resolved 1–4 character storage input, and CANCEL/NODUMP remain
distinct flags. Retained raw ABEND artifacts remain readable, but new
compilations do not carry their command text across the executable boundary.
`HANDLE ABEND` uses the same typed task dialect with a source label or bounded
program-name input and mutually exclusive CANCEL/RESET actions; its provider
continues to own authorization and durable active/canceled exit state.
The typed local LINK subset binds PROGRAM and an optional COMMAREA before
dispatch. COMMAREA is one input/output storage identity, so registering its
return destination cannot replace the captured request bytes. Optional LENGTH
accepts a literal, numeric storage, or matching LENGTH OF identity and truncates
the copied request before dispatch. Channel, DATALENGTH, input-message,
remote-system, transaction, and SYNCONRETURN forms remain compiler rejections
until their separate contracts are implemented.
The typed local XCTL subset binds the same PROGRAM and optional COMMAREA inputs,
but declares no COMMAREA output because control does not return to the caller.
Its complete provider result becomes a frame-replacing transfer to the selected
program. The same LENGTH forms select the replacement frame's copied COMMAREA
and therefore its EIBCALEN. Channel and input-message forms remain compiler
rejections until their separate contracts are implemented.
The typed local RETURN subset admits bare completion and an optional TRANSID;
COMMAREA is admitted only with TRANSID so the copied bytes have an owned durable
continuation identity. RETURN carries no COMMAREA output. Literal, numeric
storage, and matching `LENGTH OF` values select the captured prefix. Channel,
input-message, IMMEDIATE, ENDACTIVITY, higher-level, and DPL forms remain
fail-closed. All three commands bound explicit lengths before dispatch and map
invalid ranges or missing storage to LENGERR rather than reading beyond the
resolved data area.
Typed local START scheduling accepts packed INTERVAL/TIME and explicit
`AFTER`/`AT` unit forms from literals or numeric storage. Append-only plan tags
retain the mode and each present HOURS, MINUTES, or SECONDS component through
codec/checkpoint and interpreter request construction. Source-presence
semantics therefore survive runtime evaluation: a lone MINUTES may reach 5999
and lone SECONDS 359999, while any combined form narrows minutes and seconds to
59. The provider returns INVREQ 16 with response2 4, 5, or 6 for the respective
out-of-range component.
Typed local START no longer requires FROM. A request with no FROM,
RTRANSID, RTERMID, or QUEUE still creates its ordinary durable schedule/work
identity but carries no retrievable data. RETRIEVE consumes that exact ready
identity once and returns ENDDATA 29/0; the identical consumer request replays
the same result. Metadata-only START is data-bearing for this rule and returns
its requested metadata with length zero. LENGTH and FMH remain invalid without
FROM, so omission cannot manufacture a payload or function-management-header
state.
The typed local START/RETRIEVE data cycle also carries the bounded metadata
subset: START may supply RTRANSID, RTERMID, and QUEUE names, and RETRIEVE may
request exact-width writable destinations for any corresponding value.
ENVDEFERR is decided before one-time consumption when the producer omitted a
requested field. START FMH is persisted with the data record; RETRIEVE emits a
strict typed EIBFMH byte that the interpreter applies to its implicit EIB
state. RETRIEVE may alternatively use SET with mandatory LENGTH: the compiler
requires a pointer target, and the interpreter returns task-owned bytes through
its checked virtual-address model. Local START PROTECT writes no worker item
until an explicit successful SYNCPOINT commits its issuing run's
protected-pending records; explicit rollback removes those records. The typed
ABEND route also removes still-protected records before transferring or
terminating. CANCEL returns NOTFND while a protected row is uncommitted and
uses the ordinary pending-work cancellation fence after committing SYNCPOINT.
Normal machine completion and highest-level RETURN now apply the same commit
transition implicitly before task cleanup. Known execution failure applies
rollback deletion; scheduler/WAIT suspension does not finalize the task.
Terminal disconnect and idle timeout also apply rollback deletion through the
caller-held cleanup boundary. If durable execution reaches a terminal outcome
before product/CICS cleanup, recovery reconstructs the exact invocation from
the retained exchange, restores its checkpointed priority, and reloads the
durable CICS undo state before cleanup. `Completed` commits protected START;
cancelled, timed-out, failed, and dead-letter outcomes roll it back. A completed
handoff retains its already-applied RETURN finalization and is discarded without
committing twice.
When local START omits REQID, the provider derives one replay-stable
eight-character identifier and the interpreter writes it to implicit EIBREQID;
that value owns the same record and work identities as an explicit REQID. With
local NOCHECK, the same internal identity is generated for replay and worker
ownership but is deliberately not returned, leaving EIBREQID null. Remote
NOCHECK shipping remains deferred.
Local START USERID performs a `SURROGAT <userid>.DFHSTART` READ check under the
issuing principal before writing an interval or work row, but only after the
typed RACF principal-status route validates the requested non-login execution
identity. Unknown identities return USERIDERR 69/8, revoked identities return
69/19, and locked/indeterminate identities return 69/10; an unavailable
external-security interface returns INVREQ 16/18. A password-expired but
otherwise active identity remains valid for this non-login check. An accepted
explicit identity is stored as the future task principal; omission keeps the
issuer. Surrogate denial returns NOTAUTH 70/9 without mutation. Terminal
combinations and actual target-task creation remain deferred.
RETRIEVE WAIT durably checkpoints and reissues the same statement when no
eligible record exists; explicit execution re-entry after worker promotion
consumes through the ordinary one-time fence. Deadlock timeout, shutdown/AICB,
automatic wake, process-restart WAIT resume, terminal association, and automatic
task launch remain fail-closed or deferred.
The typed default-cursor file-browse subset binds STARTBR, READNEXT, READPREV,
and ENDBR to exactly one FILE/DATASET alias. STARTBR captures a writable
RIDFLD without returning a record and admits the default-equivalent `GTEQ`
relation; READNEXT and READPREV require INTO and
model RIDFLD as the same input/output storage identity so the host-updated key
feeds the next browse request; ENDBR closes the resource browse. REQID/SYSID,
KEYLENGTH/LENGTH, SET, alternate RBA/RRN/XRBA and other generic key modes, and
UPDATE/TOKEN/RLS locking remain explicit compiler rejections.
The typed keyed-mutation subset admits DELETE with either an explicit RIDFLD or
the record held by the task's latest `READ UPDATE` on that file. WRITE FILE
requires explicit FROM and RIDFLD data areas. Both use the same single
FILE/DATASET resource binding and typed mutation envelope. TOKEN correlation,
SYSID/length handling, generic and alternate record identities, WRITE
MASSINSERT, and RLS NOSUSPEND remain fail-closed.
The typed local WRITEQ TD subset requires a bounded QUEUE selector and FROM
storage input, with optional numeric LENGTH or `LENGTH OF` that input. The
provider writes exactly the selected prefix under the request's mutation
identity, so retries compare the semantic record rather than ignored trailing
bytes. Remote SYSID routing and TDQUEUE definition-state conditions remain
deferred.
The typed local BMS subset binds `RECEIVE MAP`, `SEND MAP`, and `SEND TEXT` to
the terminal family. Map names are prevalidated 1–7 character literals or
alpha/alphanumeric fields; a `RECEIVE MAP` MAPSET field may be eight bytes so
its runtime value can contain a valid name plus a trailing blank. `SEND MAP`
requires MAP, defaults MAPSET to MAP, and optionally captures FROM; `RECEIVE
MAP` requires MAP, applies the same MAPSET default, and optionally writes INTO;
`SEND TEXT` requires FROM. The provider uses the requested durable map
definition for terminal-fit validation and input-field normalization. SET
pointers, omitted-map AID-only receive, implicit symbolic map storage, explicit
length, paging, device and other terminal controls remain explicit compiler
rejections.
`CURSOR` and `FREEKB` are admitted and forwarded but are not yet modeled by the
terminal provider (`#210`). `ERASE` coincides with the provider's existing
full-screen replacement behavior (`#203`).
`PURGE MESSAGE` is a separate typed terminal mutation. Because this runtime has
no ACCUM or page-building route, its reachable full-BMS logical-message state
is empty: local purge succeeds idempotently without changing the already
displayed screen or current map. DPL use returns `INVREQ` 16/200. Deleting a
nonempty accumulated message and surfacing temporary-storage `TSIOERR` remain
unready until that logical-message authority exists.
The typed ASSIGN subset carries each of its 78 admitted context values as a
pre-resolved output binding under one bounded output-name authority. It retains
the existing 16-option maximum, exact receiver checks, partial-INVREQ behavior,
local/DPL matrix, EIBFN and provider semantics without carrying source command
text across the executable boundary. The other 35 generated ASSIGN semantic
options remain compiler rejections until their contexts are implemented.
The legacy route admits only the source-valid option subset whose behavior is
implemented by that raw handler. A catalog-known option outside that subset
fails explicitly before compatibility lowering instead of being silently
dropped. The pre-registry `DATASET` spelling remains an exact alias for `FILE`
on the existing file and browse operations; specifying both spellings fails as
an ambiguous resource selection.
`automatic_registration` remains false, the default handler is null, and an
unready row cannot reach a generic-success fallback. The application registry
does not accept or dispatch SPI or FEPI identities. A separate generated,
compiler-only compatibility descriptor admits exactly `INQUIRE PROGRAM` to the
pre-existing raw `Inquire` route; it is bound to SPI row `0155`, excluded from
the 263-row registry and its digest, and does not admit `SET FILE`, other
`INQUIRE` forms, or unknown options. The two retained SPI compatibility
operations otherwise remain confined to the separate legacy runtime
collection; SPI/FEPI completion belongs to 0.10.

A second generated, compiler-only compatibility descriptor
(`toreleon/mainframe-env#177`) admits exactly a bare, non-MAP 3270-logical
`SEND FROM(...)` -- with optional `LENGTH`, `RESP`, `RESP2` and flags `ERASE`,
`NOHANDLE`, and no other options -- to the pre-existing raw `SendText` route
the legacy runtime has executed since 0.1.1. It is bound to application row
`0187`, whose own reviewed runtime operation (row `0192`, `SEND TEXT`) is
`SendText`; every sibling `SEND *` form (`MAP`, `TEXT`, `CONTROL`, `PAGE`,
`PARTNSET`) is an application discriminator and is excluded. Unlike the
`INQUIRE PROGRAM` route, an option outside this bounded shape does not fail
closed from the compatibility descriptor itself: it falls through to the
pre-existing 263-row registry route, so a real, catalog-known SEND option
(`CTLCHAR`, `WAIT`, `STRFIELD`, `CONVID`, ...) keeps failing with the
existing "handler is unready" diagnosis for row `0187` instead of a
fabricated "unknown option" from the compatibility route. This route changes
no readiness, advertising, count or credit: row `0187` stays `Unready` and
`advertised: false` in the 263-row registry, and the runtime is unchanged --
the legacy route already passes `LENGTH` into the `SendText` request exactly
the way `SEND TEXT` does, since both surface forms share
`CicsOperation::SendText` and `typed_cics::execute_legacy`. Folding row
`0187` into the reviewed runtime table itself is deferred until
`toreleon/mainframe-env#173` lets the source review re-run.

[`tools/generate_cics_descriptors.py`](../../tools/generate_cics_descriptors.py)
deterministically writes the provider descriptors, host-API identity table,
263-row contract, compact IR registry, and the isolated compiler-only
`INQUIRE PROGRAM` and bare-`SEND` legacy compatibility descriptors. `--check`
compares all generated outputs without writing. Schema, freshness, digest and
module-boundary checks run under `cargo xtask architecture-fast --check`;
hand-editing a generated artifact or changing an authority without
regeneration fails the gate.

CIC-901 is therefore an incremental, non-release architecture boundary. It
freezes source-backed contracts and fail-closed registry shape so CIC-902 and
later work can implement vertical semantic families on the shared runtime. It
does not claim that 263 commands execute, satisfy conformance, pass licensed
differentials, or make 0.9.0 release-ready.

## Existing runtime families

| Family | Owns |
|---|---|
| `task-control` / `handle-state` | task context, durable HANDLE state, ASSIGN, RETRIEVE, ABEND, and pseudo-conversation RETURN |
| `time` | ASKTIME clock acquisition and FORMATTIME conversion |
| `program-control` | program inquiry, LINK, and XCTL |
| `terminal-control` / `terminal-run` | BMS and text send/receive behavior plus terminal-task lifecycle cleanup |
| `file-control` | file status, keyed I/O, and browse behavior |
| `queue-control` | transient-data queue writes |
| `recovery` | SYNCPOINT coordination, rollback, and subsystem unit-of-work completion |
| `interval-control` | bounded local START scheduling/cancellation plus zero, relative, and absolute DELAY |

This table describes the eight families already present in the 40-operation runtime
collection. The 263-row application registry also assigns every row a
deterministic future family owner, but that assignment is routing shape rather
than an executable handler. `CicsService::invoke_run` selects an existing
runtime descriptor first and routes on its family. Each existing runtime family
has a reviewed implementation module under
`crates/providers/mainframe-env-cics/src/handlers/`; no command is dispatched by
an ad hoc keyword match in the service monolith. The service retains the shared
session/run state, authorization boundary, durable compare-and-swap primitives,
common condition/response machinery, and bounded codecs used across families.
The accepted `retention.rs` sibling owns provider-lifecycle codecs and
dependency descriptions; it is deliberately outside the command-family layer
and cannot become an alternate dispatch path.

The compiler/runtime boundary is governed by
[ADR-0011](../decisions/0011-typed-language-hir-and-semantic-ir.md). A migrated
COBOL CICS family resolves static command identity, options, resource bindings,
and output destinations before execution, then emits the existing owned typed
request through the execution coordinator. The provider never parses COBOL HIR
or source syntax, and the migration cannot introduce a parallel CICS provider,
store, unit-of-work protocol, or condition authority.

Execution-context facts that are not command operands travel in bounded,
versioned invocation bindings. `mainframe-env.cics.execution-context@1`
currently distinguishes local execution, DPL with `SYNCONRETURN`, DPL without
syncpoint ownership, and `EXECUTIONSET=DPLSUBSET`. The recovery handler uses
that context before changing UOW state; CIC-905 owns populating it from a
future public DPL route. It is not embedded in source tokens or inferred from a
successful transport call.

For a DPL invocation that owns the syncpoint, the bounded
`mainframe-env.cics.syncpoint.remote-outcome@1` binding records whether the
remote system is commit-capable or unable to commit. The latter drives the UOW
into rollback, persists the rolled-back terminal state, backs out local
recoverable work, and raises `ROLLEDBACK` with RESP 82. The binding is rejected
outside a `dpl-synconreturn` context; CIC-905 owns populating both bindings from
the eventual public DPL transport.

## Change contract

Adding an identity to the application projection requires a reviewed change to
the pinned official denominator and regeneration. It never updates
`CicsOperation`, dispatch or coverage by itself.

Adding or changing an executable typed CICS command requires one reviewable
change that:

1. updates the readable descriptor catalog and its official row binding;
2. regenerates the Rust descriptor module;
3. adds semantics to the named family module, keeping it below the hard
   1,200-production-line limit; and
4. adds focused condition, authorization, durability, and recovery tests
   appropriate to the operation.

A new semantic family changes the accepted module inventory and requires an ADR
amendment. Generated descriptors never contain behavior, and handler modules
are never generator-owned. [ADR-0010](../decisions/0010-rust-module-review-budgets.md)
records the hard limits and exact legacy ceiling policy.

## Verification

```bash
python3 -B tools/generate_cics_descriptors.py --check
python3 -B tools/generate_cics_source_map.py --batch all --check
python3 -B conformance/0.9/tools/fetch_cics_application_sources.py --batch all --cache $MAINFRAME_ENV_IBM_DOCS_CACHE --check
python3 -B conformance/0.9/tools/extract_cics_application_sources.py --batch a --cache $MAINFRAME_ENV_IBM_DOCS_CACHE --check
python3 -B conformance/0.9/tools/extract_cics_application_sources.py --batch b --cache $MAINFRAME_ENV_IBM_DOCS_CACHE --check
python3 -B conformance/0.9/tools/extract_cics_application_sources.py --batch c --cache $MAINFRAME_ENV_IBM_DOCS_CACHE --check
python3 -B conformance/0.9/tools/verify_cics_application_sources.py --batch a --cache $MAINFRAME_ENV_IBM_DOCS_CACHE
python3 -B conformance/0.9/tools/verify_cics_application_sources.py --batch b --cache $MAINFRAME_ENV_IBM_DOCS_CACHE
python3 -B conformance/0.9/tools/verify_cics_application_sources.py --batch c --cache $MAINFRAME_ENV_IBM_DOCS_CACHE
python3 -B conformance/0.9/tools/review_cics_application_sources.py --batch a --cache $MAINFRAME_ENV_IBM_DOCS_CACHE --check
python3 -B conformance/0.9/tools/review_cics_application_sources.py --batch b --cache $MAINFRAME_ENV_IBM_DOCS_CACHE --check
python3 -B conformance/0.9/tools/review_cics_application_sources.py --batch c --cache $MAINFRAME_ENV_IBM_DOCS_CACHE --check
python3 -B -m unittest tools.tests.test_cics_descriptors tools.tests.test_cics_source_map tools.tests.test_module_boundaries
cargo test -p mainframe-env-host-api -p mainframe-env-cics --all-features --locked
cargo xtask architecture-fast --check
```
