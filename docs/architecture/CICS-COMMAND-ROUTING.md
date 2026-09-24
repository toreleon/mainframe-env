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
records 61 known mutating rows, 178 bounded-effect rows, one explicit UOW
boundary and 238 bounded-UOW rows. Those values describe current contract
certainty; they are not execution or conformance counts.

The same generator emits the compact
[`CICS application IR registry`](../../crates/foundation/mainframe-env-ir/src/generated/cics_application_registry.rs).
It contains all 263 API registry shapes with deterministic recognition,
option-shape, family, EIBFN and handler identities. Readiness is deliberately
split:

- 160 `typed-runtime` API routes, including the original `READ`, `REWRITE`, and
  `SYNCPOINT` routes and the reviewed incremental family slices;
- 0 `legacy-compatibility` API routes; and
- 103 `unready` rows that are recognized but fail explicitly as unsupported.

The now-empty raw compatibility set remains owned by the separate versioned
[`legacy-execution-options.json`](../../conformance/0.9/cics/legacy-execution-options.json)
catalog and its
[`schema`](../../conformance/0.9/schemas/cics-legacy-execution-options.schema.json).
It binds the logical 263-row application identity digest, not the physical
descriptor file, so runtime-readiness edits cannot invalidate frozen IBM
source receipts. The generator requires its route identities to match the
legacy API runtime set exactly and verifies every admitted option against a
current accepted source projection before emitting the registry.

The current 160 API routes are the only advertised application commands.
Conversation ALLOCATE lowers from selected COBOL to APPC mapped/MRO; GDS
ALLOCATE and GDS ASSIGN are registered for their assembler/C-only host routes
and retain six-byte RETCODE results without EXEC CICS conditions. A trusted
task ingress can install one durable principal facility; alternate ALLOCATE
routes cannot silently become the principal.
WAIT SIGNAL is a distinct typed principal-facility route. A trusted LU ingress
posts an ordered durable signal; the selected provider suspends until one is
pending, then consumes it with a replay receipt and applies source-defined
SIGNAL/EIBSIG behavior. Online suspension retains the task checkpoint for
event-driven reissue.
The twelve COUNTER and DCOUNTER routes share a versioned local pool authority
and bounded descriptor, compiler, interpreter, and provider children. See
[ADR-0014](../decisions/0014-named-counter-authority.md) for the atomic
state, tag, replay, and pool-rebuild boundaries.
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
the copied request before dispatch. Local DATALENGTH is retained as a distinct
numeric operand but deliberately does not shorten or validate that payload;
the source assigns it only to remote/dynamic transfer optimization. Channel,
remote DATALENGTH checking, input-message, remote-system, transaction, and
SYNCONRETURN forms remain compiler rejections until their separate contracts
are implemented.
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
RIDFLD without returning a record and admits the default-equivalent `GTEQ` or
exact-key `EQUAL` relation as an exclusive choice; READNEXT and READPREV require INTO and
model RIDFLD as the same input/output storage identity so the host-updated key
feeds the next browse request; ENDBR closes the resource browse. REQID/SYSID,
KEYLENGTH/LENGTH, SET, alternate RBA/RRN/XRBA and generic key modes, and
UPDATE/TOKEN/RLS locking remain explicit compiler rejections.
The typed keyed-mutation subset admits DELETE with either an explicit RIDFLD or
the record held by the task's latest `READ UPDATE` on that file. WRITE FILE
requires explicit FROM and RIDFLD data areas; REWRITE consumes the held record
identity and an explicit FROM area. Optional WRITE or REWRITE LENGTH is a
bounded literal, halfword-binary value, or matching `LENGTH OF` and selects the
exact record prefix before mutation. READ LENGTH is the same writable halfword
capacity on input and actual-record-length output. Optional READ, WRITE, or
explicit-key DELETE KEYLENGTH uses the same positive halfword forms against
RIDFLD and is validated against the durable definition. All use the same single
FILE/DATASET resource binding and typed mutation envelope. TOKEN correlation,
SYSID, generic and alternate record identities, WRITE MASSINSERT, and RLS
NOSUSPEND remain fail-closed. READ GTEQ uses a bounded transient dataset cursor
to select the equal key or first greater keyed record, and closes that cursor
before returning the record. READ GENERIC uses the same transient route with a
required positive partial KEYLENGTH and rejects a first-greater record whose key
does not share the requested prefix. Explicit EQUAL selects the same exact
complete- or generic-key route as the default relation and is mutually
exclusive with GTEQ. GTEQ may carry source-defined KEYLENGTH zero to select the
first keyed record, including when GENERIC is also present.
The typed local WRITEQ TD subset requires a bounded QUEUE selector and FROM
storage input, with optional numeric LENGTH or `LENGTH OF` that input. The
provider writes exactly the selected prefix under the request's mutation
identity, so retries compare the semantic record rather than ignored trailing
bytes. Typed local DELETEQ TD accepts only the bounded QUEUE selector, requires
update access to the same queue resource, and atomically removes the durable
queue plus its retained-byte accounting. A missing queue returns QIDERR 44/0.
Typed local READQ TD consumes the oldest durable record into exactly one
writable INTO area or SET pointer. INTO uses the optional halfword LENGTH as a
maximum and returns the original record length; omitted LENGTH uses the
compiler-resolved INTO extent. SET returns a checked virtual address to an
interpreter-owned copy of the complete record and participates in checkpoint
restore. Zero or truncated INTO reads consume and return LENGERR 22/0, while
negative length or insufficient SET capacity preserves the queue. Missing and
empty queues return QIDERR 44/0 and QZERO 23/0. Durable installed TDQUEUE
definitions select intrapartition or local extrapartition direction, enabled
and open state, record-size policy, and bounded per-queue capacity. Those
definitions drive exact DISABLED, INVREQ, NOTOPEN, LENGERR, NOSPACE, IOERR,
QIDERR, and QZERO outcomes across reopen. First registration preflights every
materialized compatibility-profile queue against the complete proposed
definition set and atomically rejects undeclared queues or retained data that
violates direction, fixed/maximum record size, record count, or byte limits.
WRITEQ TD, READQ TD, and DELETEQ TD accept SYSID only when it identifies the
current system; an unknown or unsupported remote name returns SYSIDERR 53/0
before authorization or queue mutation. NOSUSPEND/QBUSY, indoubt locking,
remote routing, and external data set integration remain deferred.
Typed `DOCUMENT CREATE` starts the document-control family on a bounded durable
authority owned by the issuing execution, run unit, and transaction. It returns
a deterministic 16-byte token and optional fullword DOCSIZE for empty, FROM,
TEXT, BINARY, FROMDOC, or registered-template content. Symbol-list length,
delimiter and unescaping rules are checked before persistence. Template use
performs a DOCTEMPLATE READ decision against the registered resource name.
The document row and canonical outer replay are one atomic store mutation, so
an unknown result replays the same token without duplicating content; rows
reload from SQLite and are deleted when their owning task ends. Operation tag
63, operand tags 132–147, option tag 84, and non-ASSIGN output tags 216–217 are
append-only. `DOCUMENT DELETE` uses tag 64 and a storage-backed 16-byte
DOCTOKEN input. It verifies task/transaction ownership and atomically deletes
the durable row with its replay record, freeing aggregate capacity immediately;
a missing token returns NOTFND 13/1. `DOCUMENT INSERT` uses operation tag 65 and the
shared document operands to append, insert after a bookmark, or replace a
bookmark interval. It applies source-specific conversion marks, copies
FROMDOC bookmarks, and keeps a bounded internal tagged retrieval form for
FROM round trips. Each insert updates the versioned document and effect replay
atomically. Document symbol names preserve case, and template expansion is
bounded before persistence. `DOCUMENT RETRIEVE` uses tag 66, input operand
tags 132/145/146, and document DATAONLY option tag 85. It copies tagged or
data-only content to a bounded application buffer, returns the exact required
LENGTH on a short buffer, and performs the supported CP037 character-set
conversions without changing document state. `DOCUMENT SET` uses tag 67 and
the shared token, symbol, value, list, delimiter and length operands. It
replaces case-sensitive symbol definitions atomically with the effect replay;
previously inserted bytes retain their materialized values. The unused
operand, option and output tags in the reserved ranges remain unassigned.
Typed `WAIT JOURNALNAME` uses the journal-control family and a single durable
`cics-journal-v1` authority shared with journal writes. A literal or
storage-backed name is normalized to 1–8 uppercase alphanumeric, `$`, `@`, or
`#` characters and authorized as `JOURNAL/CICS.JOURNAL.<name>`. An explicit
fullword `REQID` selects only a token created by the issuing task; omission
waits on the named journal's current buffer without inheriting the writer's
task ownership. Hardened output returns immediately, pending output suspends
and reissues through the coordinator (therefore retaining its cancellation and
deadline fence), and durable I/O, unknown-journal, and unavailable states map
to IOERR 17, JIDERR 43, and NOTOPEN 19. Denial maps to NOTAUTH 70. The bounded
authority and completion state survive SQLite reopen; WAIT itself is
non-mutating and therefore creates no independent mutation replay ledger.
Typed `WAIT JOURNALNUM` keeps the compatibility operation separate while mapping
numeric values 1–99 to `DFHJ01`–`DFHJ99` before the same SAF check and durable
wait. The explicit REQID remains task-owned; an omitted REQID uses the selected
journal's current buffer. Other numeric values and mixed name/number operand
shapes are rejected before the wait.
Typed `WRITE JOURNALNAME` requires a known 1–8 character journal, two-byte
JTYPEID, and FROM area. Optional fullword FLENGTH and halfword PFXLENG select
bounded data and PREFIX slices; invalid lengths return LENGERR 22 without an
append. The local durable writer authorizes `JOURNAL/CICS.JOURNAL.<name>` for
update, retains the exact record bytes and idempotency key, and returns a
fullword REQID only for deferred output. An explicit WAIT creates a hardened
record before returning. Deferred output remains pending until the trusted
local output worker acknowledges it; a numbered or named WAIT then observes
the same completion state. Two pending buffer slots enforce default suspension
or NOSUSPEND/NOJBUFSP 45 without appending a rejected record. Unknown,
disabled, and denied journals map to JIDERR 43, NOTOPEN 19, and NOTAUTH 70.
The `cics-journal-v1` namespace now writes codec version 2 while reading
version 1 WAIT state. Native System Logger/SMF transport and JOURNALMODEL
resolution remain outside this local execution boundary.
Typed `WRITE JOURNALNUM` uses a separate compatibility command identity and
maps numeric values 1–99 to `DFHJnn` before the same authorization and durable
writer. It reuses the reviewed WRITE JOURNALNAME record options and condition
path with a numeric selector. The pinned 0255 page identifies the compatibility
successor without an option diagram, so this local option mapping remains an
explicit bounded inference rather than a source-projected equivalence claim.


Typed `SPOOLCLOSE` captures an exact eight-character TOKEN and requires RESP or
NOHANDLE. The provider verifies task/principal ownership and `JESSPOOL` update
authority before one versioned state transition. KEEP returns either direction
to the available-input queue; DELETE purges it. When neither option is present,
an explicitly closed input report defaults to DELETE and an explicitly closed
output report defaults to KEEP. The same provider row retains a bounded request
digest/result ledger so a crash after state CAS but before the outer CICS replay
row cannot duplicate or reverse the close. The route uses EIBFN `5610`; open,
record transfer, implicit-close, and broader JES interface states are owned by
the following spool-control slices.

The diagnostics provider has a separate versioned `cics-diagnostics-v1`
authority for local trace and dump records. A trusted region adapter can set
`CicsTraceConfiguration`, register dump-code and MCT user-point definitions,
and inspect `CicsDiagnosticSnapshot`; no trace
destination is active by default. `CicsLimits` bounds retained entries,
aggregate encoded bytes, replay keys, and each captured payload. Command
handlers use this state through versioned compare-and-swap writes, so later
diagnostic commands can retain exact results across SQLite reopen and a
post-dispatch retry without adding state to the frozen CICS service root.
`ENTER TRACENUM` requires a halfword numeric identifier in 0–199 and accepts
an optional eight-character resource, source bytes, and halfword length in
0–4000. A normal entry needs the user flag and an active destination;
`EXCEPTION` always records to the internal destination even when it is off.
The retained entry records its selected destination set, exact bytes, run,
and principal. A `CICSDIAG` SAF update check precedes durable mutation.
`MONITOR` uses additive trusted MCT definitions keyed by entry name and point.
The supported local actions update one counter, start or stop one clock, or
move bytes from a checked COBOL pointer into a bounded user character field.
Clock actions read the host clock; MOVE uses the four-byte DATA2 length or the
registered default. Missing DATA2 returns INVREQ/6 after the move, matching
the source's successful-operation condition. Point, data, and definition
errors retain their distinct INVREQ secondary codes.
`DUMP TRANSACTION` stores a bounded `MECDMP01` section stream in the durable
diagnostics row. FROM and SEGMENTLIST sections retain exact task storage bytes;
other sections record only the local task and catalog state the provider owns.
Registered dump-code maximum and suppression rules are applied before
capture, and `CICSDIAG` authorization precedes every write. DUMPID uses a
durable run/count counter; a fresh provider instance advances the run number
on its first successful dump. System dump requests fail explicitly because
this provider has no SDUMP backend.
The standalone `DUMP` form uses the same bounded section codec and SAF check.
It can include the local dump-code table (`DCT`) and accepts an omitted code,
using the generic diagnostic resource in that case. Its exact CICS TS 6.x
behavior is source-gapped; this route promises only the documented local
capture and never claims a CICS dump dataset or system dump.
The local `TRACE` form requires one ON/OFF direction and at least one of USER,
SYSTEM, EI, or SINGLE. USER controls the user trace flag, SYSTEM the system
destination, EI the internal destination, and SINGLE arms one internal user
entry. The following `ENTER TRACENUM` consumes that one-shot flag. These
switches are durable and share the diagnostic replay row; the configured
`CICSDIAG` resource is checked before mutation. The exact CICS TS 6.x TRACE
command page is absent from the pinned local corpus, so these are explicit
local controls rather than a claim about every IBM trace facility.
The local `ENTER TRACEID` form stores an exact bounded payload with its
identifier, resource, entry name, issuing identity, selected destination, and
ACCOUNT/MONITOR/PERFORM flags. MONITOR retains the event bytes in the local
monitor text map; ACCOUNT and PERFORM increment their own durable event counts.
It consumes a TRACE SINGLE arm when present. The CICS TS 6.x target command
page and both committed compatibility bodies are absent locally, so these
event counts are expressly local diagnostic behavior, not a claim about IBM
MCT accounting or performance record layout.

Typed `SPOOLOPEN INPUT` requires a writable eight-character TOKEN, an
eight-character USERID value, optional one-character CLASS, and RESP or
NOHANDLE. USERID must share the issuing CICS APPLID's first four characters.
After JESSPOOL update authorization, the provider selects the first matching
available report in token order and durably binds it to the exact run and
principal. Only one input report can be open: another task receives SPOLBUSY
88/4 and the current owner receives 88/8. A missing or held-equivalent report
returns NOTFND 13/4 without mutation. The returned TOKEN is non-ASSIGN output
tag 208, and the selected route uses EIBFN `5602`.

Typed `SPOOLOPEN OUTPUT` requires writable TOKEN, destination USERID and NODE,
and RESP or NOHANDLE. Output creation is multi-threaded and allocates one
task-owned report with default class A, NOCC, PRINT, and maximum record length
32,760. CLASS and halfword RECORDLENGTH override the defaults; NOCC, ASA, and
MCC are exclusive, as are PRINT and PUNCH. NODE and USERID must both be `*`
for the local OUTDESCR override. A POINTER or POINTER-32 OUTDESCR is followed
through its address field to a bounded length-prefixed OUTPUT parameter string;
invalid pointers and malformed strings return INVREQ 16/52 and 16/44. A bad
NODE/USERID combination returns NODEIDERR 90/0, and RECORDLENGTH outside
0–32,760 returns LENGERR 22 with the supplied value. State and returned token
share one durable replay CAS; the compiled route uses EIBFN `5602`. Dynamic JES
allocation, macro return codes, and implicit end-of-task close are outside the
local spool boundary.

Typed `SPOOLREAD` requires the task-owned eight-character TOKEN, a writable
INTO area, fullword-binary MAXFLENGTH, and RESP or NOHANDLE. Optional writable
fullword TOFLENGTH receives the actual record length. A short transfer writes
the available prefix, returns LENGERR 22 with the omitted byte count in RESP2,
and retains the same record for retry. MAXFLENGTH above 32,760 returns LENGERR
22/0 without advancing. Successful reads advance the durable cursor; the first
read beyond the final record returns ENDFILE 20 and marks EOF, and subsequent
reads return INVREQ 16/12. Wrong ownership returns NOTOPEN 19/8, while a
task-owned output report returns NOTOPEN 19/12. The reply and cursor/EOF state
share one replay CAS. The selected compiled route uses EIBFN `5604`.

Typed `SPOOLWRITE` requires a task-owned output TOKEN, a storage-backed FROM
area, and RESP or NOHANDLE. Optional fullword FLENGTH selects a prefix of FROM;
when omitted, the source area's full length is used. LINE is the default and
PAGE marks an AFP page record; the source-backed choice excludes both flags
at once. FLENGTH outside 1–32,760 returns LENGERR 22/0 without mutation.
When the requested record exceeds the output report's RECORDLENGTH, the bounded
prefix is appended and LENGERR 22 reports the omitted byte count in RESP2.
Writing an input report returns NOTOPEN 19/16; wrong task ownership returns
NOTOPEN 19/8. A JOB card with USER= on the INTRDR destination requires the
task user's SURROGAT read authority for `job_user.SUBMIT`; a denial returns
NOTAUTH 70/1 before append. The record and reply share one durable replay CAS,
and the compiled route uses EIBFN `5606`.

Typed local GETMAIN routes SET plus exactly one FLENGTH or compatibility LENGTH
and optional INITIMG through the storage-control family. FLENGTH uses signed
fullword input; LENGTH uses unsigned halfword input and the source-defined
65,520-byte ceiling. The interpreter contributes its remaining virtual
frame/byte capacity, applies the returned initialized bytes to a checkpointed
virtual base, and writes a checked POINTER or POINTER-32 address. Zero or
over-limit length clears SET with LENGERR 22/1; unavailable capacity returns
default-ignored NOSTG 42/2. LENGTH selects below-line compatibility without
exposing or claiming a native address. Native addresses, key/share/executable
attributes, and 64-bit forms remain deferred. Typed FREEMAIN accepts exactly one
DATAPOINTER or DATA slot. The interpreter proves that the pointer value or
DATA area's current virtual-storage view names the start of a live allocation
owned by the current task, applies the replay-bound release intent, checkpoints
the freed identity, and excludes released bytes and frames from current
capacity accounting. Invalid, static, unassigned, or repeated release returns
INVREQ 16/1; key/shared/load ownership remains deferred.
GETMAIN64 is a separate typed Core-MIR operation for a checked non-LE
AMODE(64) caller, never an alias of GETMAIN or a COBOL source form. The
ownership and checkpoint boundary is recorded in
[ADR-0012](../decisions/0012-checked-amode64-storage-boundary.md). The
invocation binds the caller ABI and TASKDATAKEY, FLENGTH is fullword, and SET64
requires an eight-byte pointer slot. The interpreter owns a monotonic virtual
arena with distinct above-bar, LOC24, and LOC31 address ranges; allocation
bytes, guard-zone charges, attributes, and cursors survive checkpoint v11.
The provider returns a replay-bound allocation specification and exact
conditions, while the interpreter creates the address. SHARED remains
fail-closed until cross-task storage can be durable. The selected-route test
uses a compiled layout scaffold translated to the distinct typed IR identity;
it does not claim an assembler source frontend or native executable memory.
FREEMAIN64 is a distinct typed operation over the same checked arena. It
requires the same invocation ABI and exactly one DATAPOINTER or DATA operand.
DATAPOINTER reads an eight-byte pointer slot; DATA uses an explicit binding
from a declared area to a live allocation, never the area's stored bytes as a
pointer. The binding and allocation survive checkpoint v12. The provider
returns a replay-bound release intent, which the interpreter applies only
when the returned address matches the pending request. A freed identity stays
stale after restart and its charged bytes and frame leave the task budget.
Source-defined INVREQ 16/1 rejects invalid ownership or pointer identity;
INVREQ 16/2 rejects user-key release of CICS-key storage. The checked route
has no COBOL-source or native assembler execution claim.
The typed local BMS subset binds `RECEIVE MAP`, `SEND MAP`, and `SEND TEXT` to
the terminal family. Map names are prevalidated 1–7 character literals or
alpha/alphanumeric fields; a `RECEIVE MAP` MAPSET field may be eight bytes so
its runtime value can contain a valid name plus a trailing blank. `SEND MAP`
requires MAP, defaults MAPSET to MAP, and optionally captures FROM. With an
explicit FROM, LENGTH accepts a bounded literal, halfword-binary value, or
matching `LENGTH OF` and selects that exact prefix before symbolic-map
formatting. MAPONLY rejects FROM/LENGTH and selects only the initialized map
defaults. DATAONLY requires explicit symbolic FROM bytes, ignores map defaults,
applies supplied field attributes, and preserves an existing attribute for
`X'00'`. `SEND TEXT` requires FROM and accepts the same three LENGTH forms; an
out-of-range runtime value returns LENGERR 22/0 without changing the screen.
`RECEIVE MAP` requires MAP, applies the same MAPSET default, and optionally
writes INTO. The provider validates the canonical request shape and uses the
requested durable map definition for
terminal-fit validation and input-field normalization. SET pointers,
omitted-map AID-only receive, implicit symbolic map storage, RECEIVE length,
paging, device and other terminal controls remain explicit compiler
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
The typed ASSIGN subset carries each of its 92 admitted context values as a
pre-resolved output binding under one bounded output-name authority. It retains
the existing 16-option maximum, exact receiver checks, partial-INVREQ behavior,
local/DPL matrix, EIBFN and provider semantics without carrying source command
text across the executable boundary. The append-only BMS-overflow outputs
`DESTCOUNT`, `LDCMNEM`, `LDCNUM`, `PAGENUM`, and `PARTNPAGE` return `INVREQ`
16/2 locally because no executable BMS route establishes overflow state, and
the source-defined DPL restriction returns 16/200; both paths preserve their
receivers. `RETURNPROG` is admitted only for a local highest-level frame, where
the source value is eight blanks; the provider refreshes the current parent
identity from each trusted invocation and fails closed for LINK-child and DPL
lineage until it owns a durable caller stack. `INVOKINGPROG` also requires the
trusted durable program-entry marker to identify the local initial frame and
return eight blanks; XCTL targets, linked children, and DPL fail closed until
their caller identities are durable.
`TERMPRIORITY` returns the
terminal definition's source-default halfword zero independently of current
task priority; it follows the local nonterminal 16/5 and DPL 16/200 condition
matrix. `LANGINUSE` maps the runtime's unoverridden English language default to
the source-defined three-byte `ENU` in local and DPL contexts. `INPUTMSGLEN`
reads the bounded last normalized terminal-input byte length from durable
session codec `MECSB`; RECEIVE consumes the payload without erasing that
context, and no input returns halfword zero in local and DPL execution.
`INPARTN` preserves its one- or two-byte receiver and returns 16/2 before any
map is positioned, 16/5 without a terminal, and 16/200 in DPL; a positioned-map
request fails closed until input-partition state exists. New terminal sessions
allocate a durable unique four-character virtual-terminal identifier;
`FACILITY` returns it and `NETNAME` follows the pinned TERMINAL default by
padding the same name to eight bytes. Local nonterminal use returns 16/5,
FACILITY is DPL-prohibited at 16/200, and DPL NETNAME fails closed until remote
terminal identity is propagated. Historical sessions without an identifier
remain readable but do not acquire a fabricated one. Because no client network
endpoint is retained, local `TNADDR` returns the source-defined 39 blanks for
an unresolved address; nonterminal use returns 16/5 and DPL fails closed until
remote endpoint context exists. The other 21 generated
ASSIGN semantic options remain compiler rejections until
their contexts are implemented.
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
| `queue-control` | transient-data queue writes and local queue deletion |
| `recovery` | SYNCPOINT coordination, rollback, and subsystem unit-of-work completion |
| `interval-control` | bounded local START scheduling/cancellation with facility-less or virtual-terminal target launch plus zero, relative, and absolute DELAY |
| `storage-control` | bounded task-local virtual storage allocation and release |
| `journal-control` | durable named-journal output state and task synchronization |

| `spool-control` | durable CICS spool report open/read/write/close lifecycle |

This table describes the runtime families in the typed operation
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

The typed CICS effect-plan wire contract is
`mainframe-env.cics-effect-plan@2` (`MCEP`, big-endian version 2). Operation,
operand, option, and output identity tags each occupy a big-endian `u16` in v2.
All previously assigned numeric tags retain their values, including reserved
gaps and the ASSIGN output ranges. Counts, operand value-kind bytes, storage
slots, and condition bytes retain their v1 layout. The encoder emits only
canonical v2 plans, sorting named operands and outputs and rejecting duplicate
identities. The decoder accepts canonical v1 bytes for retained artifacts and
canonical v2 bytes; it rejects unsupported versions, unknown tags, truncation,
trailing bytes, and noncanonical ordering. Re-encoding a decoded v1 plan
migrates it to v2. This wire migration does not change command semantics or
the reviewed registry readiness split.

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
