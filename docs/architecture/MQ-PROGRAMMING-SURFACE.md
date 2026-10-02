# IBM MQ programming-surface ownership

Status: **Normative identity boundary; semantics are implemented incrementally**

Owner: `mainframe-env-host-api` contracts and `mainframe-env-mq` provider

Scope: MQI denominator, source provenance, host context and semantic authority

Applies from: mainframe-env 0.15.0

## Denominator and provenance

The immutable 0.2 official catalog defines 26 unique IBM MQ 9.4 MQI calls.
The pinned source call list displays 27 rows because it lists `MQMHBUF` twice.
That duplicate is retained as source provenance and never increments coverage.

`conformance/0.15/mq/source-call-list.json` records the 27 positions, their
normalized official rows, and the exact pinned call-list topic. The generated
`MqMqiCallIdentityDescriptor` registry joins those positions to the per-call
topic paths and SHA-256 pins in the immutable MQ topic manifest. The registry
is identity-only: it neither selects a handler nor advertises execution.

`conformance/0.15/mq/structure-status-catalog.json` adds the ordered,
source-bound signatures for the same 26 calls. The generated host API exposes
169 parameter descriptors with structure and version symbols, options,
selectors, completion and reason families, and handle roles. All 26 pinned
call topics have matching retained HTML, including the re-pinned MQINQ topic
`SSFKSJ_9.4.0/refdev/q101840_.html` (SHA-256
`03e3347bbf16d2f8e3a9061e921dbfca7a3afd0fe3bc13418ebdf47bb652ce1b`).
The MQBUFMH spelling anomaly is recorded in the catalog, while `MQHMSG` is
the sole published handle identity. The registry supplies identity data only.
The additive validator now checks source-bound ordered signatures, structure
identities and versions, option families and documented combinations without
registering a handler. The bounded numeric adapter below admits its reviewed
point-to-point subset; other numeric forms and execution remain pending.

## Authority boundary

`mainframe-env-mq` is the one owned semantic authority for queue managers,
objects, handles, messages, callbacks, properties, delivery and recovery.
Stable host contracts live in `mainframe-env-host-api`; application packages
provide topology; shared store, principal/SAF, canonical effect and UOW
contracts retain their existing ownership.

A native IBM MQ client can appear only behind an explicit licensed adapter and
profile. It is not an owned-simulator fallback, does not add simulator coverage,
and cannot act as both product and expectation in a differential test. A
commodity broker can be only a replaceable physical adapter after a reviewed
semantic-gap and failure matrix; its acknowledgements or transaction model do
not establish MQI compatibility.

## Host-owned syncpoint rule

The pinned `MQCMIT` and `MQBACK` topics restrict those calls on z/OS to batch,
including IMS batch DL/I. CICS applications use CICS syncpoint commands;
non-batch IMS applications use IMS coordination calls. `MQBEGIN` distinguishes
queue-manager-coordinated local and global units from externally coordinated
units and is invalid in an MQ client environment.

The typed call contract must therefore carry the execution context and
syncpoint owner. A forbidden context returns the exact MQ completion/reason
condition without mutating queue-manager state. `MQCMIT` must never be exposed
as a generic cross-subsystem commit.

The provider recognizes both the typed `mainframe-env.mq.host-context@1`
binding and the existing typed `mainframe-env.cics.execution-context@1`
binding. Direct `MQCMIT` and `MQBACK` calls are checked before authorization,
replay, lock acquisition, or queue-state access. CICS returns MQCC 2 / MQRC
2012; non-batch IMS and host-coordinator-owned contexts are rejected by the
same source-bound matrix. The existing CICS SYNCPOINT dispatch carries nested
and outer effect-origin bindings; MQ replay validation binds them to the exact
run, sequence, idempotency key, and outer effect before persistence. Missing,
contradictory, partial, or malformed provenance fails closed.

`MqSyncpointCall`, `MqHostEnvironment`, and `MqSyncpointOwner` define the shared
direct-call applicability matrix for `MQBACK`, `MQBEGIN`, and `MQCMIT`. Batch,
IMS batch DL/I, and other queue-manager-owned bindings admit these calls. CICS
and non-batch IMS reject application commit/backout; MQ client bindings also
reject `MQBEGIN`. An external coordinator rejects all three direct calls with
MQCC 2 / MQRC 2012. The current provider route enforces this matrix for direct
commit and backout in z/OS batch, IMS batch DL/I, CICS, IMS, MQI client and
other bindings. `MQBEGIN` has no executable public request route yet.

## Object lifecycle kernel

`mainframe-env-mq::object` owns bounded, case-sensitive object names and typed
definitions for queue managers, local, alias, remote, and model queues, topics,
subscriptions, and processes. Its catalog resolves aliases and remote routes
deterministically with cycle and depth rejection. Model instances have explicit
owner and close rules. A strict versioned snapshot codec rejects noncanonical
names, corrupt rows, and unsupported schema versions before restoration.

The existing queue service uses the same name rule for definitions, lookups,
request queue selectors, and trigger programs. It removes permitted trailing
blanks or a null ending significant data, preserves case, and rejects leading
or embedded blanks before durable mutation. The service persists the typed
catalog in the shared provider-row store and routes the compatibility queue
operations through local queues and aliases, including atomic migration from
the legacy queue-only manifest. Dynamic, remote, topic, subscription, process,
distribution-list and MQINQ execution remain fail-closed or pending.

## Frozen object and message request contracts

The host API now exposes a bounded, source-bound MQOPEN/MQCLOSE vocabulary for
object lookup, access, context, dynamic names and close lifecycle. It also
exposes bounded message descriptors, typed properties, identifier selection,
browse/wait/truncation, grouping/segmentation, distribution outcomes and
explicit duplicate or unknown states. These are non-executable contracts:
unsupported forms and pending provider authorities stay explicit, and their
presence grants no behavioral or licensed coverage.

## Reviewed completion wire identities

The existing completion/reason catalog now uses private source-projection schema
`mainframe-env.mq-completion-reason-catalog@2`. Its additive wire projection
reviews `MQCC_OK=0`, `MQCC_WARNING=1` and `MQCC_FAILED=2` from
`SSFKSJ_9.4.0/refdev/q090560_.html` in the separately pinned
`ibm-mq-9.4-programming-supplements-2026-09-12` scope. Decimal and eight-digit
hexadecimal identities agree and fit the API's signed 32-bit MQCC and signed SQL
integer representation; no SQL storage or general structure layout changes.
`MQCC_UNKNOWN=-1` is recorded as excluded from ordinary reviewed call returns.
MQCMIT catalog row `0007` corroborates the MQLONG output role; MQCBC field
context remains callback input and never creates a return pair for MQCB_FUNCTION.

`MqCompletion::wire_number` and `from_wire_number` map these three identities.
`MqReviewedStatus::wire_pair` emits their MQCC with the already admitted reason;
`from_wire_pair` applies the existing call-specific reason admission. Unknown,
negative or out-of-range completion values, reason aliases needing explicit
symbols, pending collisions and callback notifications fail closed. All ten
reason declarations remain pending and all 1,030 pairs retain their source pins.

The generator validates the unchanged original `@1` call-return artifact digest
by reconstructing its exact JSON representation. `MQ_STATUS_CATALOG_SHA256`
continues to bind that identity in existing canonical status bytes; the additive
completion projection has its own source digest. Canonical encoders, outcome
forms and old golden bytes are unchanged. Offline checks validate artifact
closure; cache-backed generator/verifier checks independently reproduce the
selected constants and corroborating fragments before comparison. This maps
identities only and does not calculate runtime results or register an ABI.
The archive provenance remains in-progress, without independent browser
reproduction and predating the MQINQ re-pin; no freshness, same-snapshot,
behavioral, licensed or execution claim follows from this review.

## Reviewed status with typed output

The additive `MqMqiOutcome::ReviewedOutput { status, output }` binds the existing
call-specific `MqReviewedStatus` to the existing `MqMqiOutput`. Its distinct
canonical name preserves every older variant's bytes; `ReviewedStatus` remains
a status-only observation. This is a bounded output contract, not runtime
calculation, queue mutation permission, SAF or returned-handle authority.

MQGET catalog row `0015`, `SSFKSJ_9.4.0/refdev/q101830_.html` (SHA-256
`290b8af3acbe4a87f007ab9e3b67d0a797f835066118c9c6150ff0570e430b62`),
lines 21–48 requires descriptor, copied buffer and original DataLength even on
truncation. Its lines 65–125 place both truncation reasons under MQCC_WARNING.
The separately pinned programming-supplements topic
`SSFKSJ_9.4.0/refdev/q096715_.html` (SHA-256
`a1c3fa0544e420f8bc1ce1dfeffb891df435e85a64108c378ac59a48dbd14af3`),
lines 674–686, distinguishes accepted removal/browse advance from rejected
retention without browse advance. Both baselines retain their exact pins;
supplement registration itself grants no semantics or execution credit.

Reviewed GET output admits OK/NONE with a complete message,
WARNING/TRUNCATED_MSG_ACCEPTED with accepted removed/browsed truncation,
WARNING/TRUNCATED_MSG_FAILED with rejected retained truncation, and
FAILED/NO_MSG_AVAILABLE with no-message/wait-expired observation. The constructor
and provider preflight bind call, supported default options, mode, wait,
truncation choice, capacity and exact copied length to the original request.
Required length can exceed capacity within explicit limits. Rejected truncation
reports no new cursor. The full descriptor, properties, expiry, identifiers and
copied bytes are encoded; an empty message remains distinct from no message.

Existing OK/NONE output classes remain shape-checked, with reviewed PUT/PUT1
requiring an actual accepted observation rather than pending/unknown/duplicate.
Those descriptor input/output roles are retained from rows `0020` and `0021`,
`q101880_` and `q101890_`. Reviewed distribution output is unsupported because
this payload lacks per-destination return pairs. The existing syncpoint-only
FAILED/ENVIRONMENT_ERROR no-output shape remains applicable. Other warning or
failed payloads, conversion-dependent lengths and absent property size-reporting
forms fail closed; status-only or explicit pending observations remain available.
No reason name authorizes mutation, and no numeric alias bypasses reviewed
symbol admission. All 1,030 declarations and ten pending reasons are unchanged.

## Checked point-to-point numeric intent

The private unreleased structure catalog uses additive schema
`mainframe-env.mq-structure-status-catalog@2`. Its `wire_options` projection
contains 102 reviewed numeric identities with exact topic/fragment hashes and
bounded line locators under `ibm-mq-9.4-programming-supplements-2026-09-12`.
MQOO `q092100_`, MQCO `q091070_`, MQGMO `q091510_`, MQPMO `q092190_`,
MQMD `q091870_` and MQOD `q098100_` supply constants and version facts.
MQGMO `q096715_`, MQPMO `q098655_` and MQOD `q098105_` supply field context.
Original call rows `0006`, `0015`, `0019`, `0020`, `0021` remain bound to
`q101740_`, `q101830_`, `q101870_`, `q101880_`, `q101890_` respectively.
MQOD1 is the only numerically established MQOD version in this selected scope;
higher version numbers and MQMO constants are not inferred from symbol names.

`mq_wire_options` accepts signed numeric inputs, checks signed 32-bit MQLONG
range, and produces existing typed OPEN/CLOSE/GET/PUT/PUT1 intents only for
local queues, MQOD1/GMO1/PMO1 and basic ungrouped MQMD1/2. It preserves the
three input modes, browse versus removal, finite wait conversion, truncation,
identifier selection and independently admitted local syncpoint. PUT1 retains
actual HCONN. NEW_MSG_ID clears only the requested ID to select the existing
generator intent; NEW_CORREL_ID remains pending. No ID or outcome is generated.
The returned structures are not wire layouts and do not reconstruct opaque handles.

The integration-owned `MqWireBindings` port supplies queue-manager platform,
already admitted unit/cursor, clock conversion and independently checked queue
defaults. Zero option words require confirmation that cluster/read-ahead,
property and put response defaults are represented. Missing configuration,
unit or cursor fails closed. Queue-manager z/OS defaults select local syncpoint;
distributed defaults select no syncpoint, while browse stays outside syncpoint.
External coordination remains pending. This port grants no SAF or mutation permit.
Service/coordinator and live registry checks remain mandatory after conversion.

Unknown/sign/overflow values and supported illegal combinations are rejected.
Recognized unrepresented context, properties, message handles, async response,
distribution and higher structure versions stay explicitly pending. Nonempty
selection under cursor remains pending because the current kernel applies
selection there, unlike the reviewed source. Nonzero unused wait fields and
unbounded waits are outside the strict subset. Unsupported defaults cannot
silently become `ContractDefault`; that value is emitted only after conversion.

The generator reconstructs and verifies the exact historical `@1` catalog hash
`3dc77d004bd79ad7f6daa99ffb4a3fb958ff1d816cb6c33bc4b6bac794a23448`.
Existing signature descriptors, status bindings, all 1,030 reason declarations,
ten pending reasons and canonical request/result bytes retain their identities.
Only the new projection has a new digest. Older strict catalog readers must
explicitly support `@2`; no retained effect migration or automatic replay occurs.
Cache-backed checks reproduce the selected facts through the shared offline
reader; cache-free checks bind their artifact closure. The supplemental archive
remains in-progress without independent browser reproduction, predates the MQINQ
re-pin, and establishes no freshness or same-snapshot claim. The adapter is not
a public handler or execution/participant/licensed acceptance claim. Remaining
all-26 structure/option/ABI/service integration stays required.

## Coverage boundary

### Historical handle observation

The strict private typed-result storage codec can preserve issued handle outputs
through the fixed-field `MqHandleObservation` projection. Only observations have
Serde; opaque executable tokens retain private construction. Decoded tokens have
an irreversible historical disposition, while exact canonical identity/bytes
remain unchanged. Every registry access rejects that disposition, including
lifetime observation. Canonical equality does not mean authority equality.
Readonly resolution can return an already-existing exact-owner/role/connection/
epoch entry only after the caller independently proves the retained receipt/core
occurrence and current admitted frame. It never connects, allocates or resurrects.
Historical Default/Unassociated connection reconstruction remains unsupported;
replay cannot acquire a current CICS task's default. Cold exposure still requires
service-owned persisted epoch advancement. See
[ADR 0033](../decisions/0033-mq-historical-handle-observation.md); no service,
SAF, receipt/retention, UOW or public readiness is supplied by this pure boundary.

### Coverage identity

The MQ-1506 licensed adapter at
`conformance/0.15/oracles/mq-licensed-differential.json` binds the 26-call
denominator to independent fixture identities and a bounded external receipt.
Its verifier requires an authorized IBM MQ 9.4 environment, exact service and
candidate identities, distinct product and oracle runners, normalized
digest-only observations, and one observation per call. An absent or rejected
receipt grants zero differential credit. The in-repository fixture index and
mutant tests validate the contract; they are not licensed execution evidence.

Catalog and generated-registry checks prove only:

- the 26-call normalized denominator;
- the exact 27-row source provenance;
- official row, label, topic-path and topic-digest joins; and
- deterministic generated identity bytes.

Behavioral credit requires independently bound Conformance IR obligations and
verdicts for each applicable recognized, validated, executed, conditioned,
recovered and differential gate. Missing licensed or source evidence remains
pending; it is never inferred from registry presence or broad workload success.
