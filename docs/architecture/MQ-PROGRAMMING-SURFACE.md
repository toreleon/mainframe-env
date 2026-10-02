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

## Private pointer-free raw layout projection

The same structure/status catalog retains private `@2` framing and adds an
independent `mainframe-env.mq-raw-layout-projection@1` projection. Its digest is
separate from the frozen original call/signature identity and the accepted
wire-options projection; neither historical digest nor canonical bytes changes.
The existing registry generator emits every field identity, width, offset and
reviewed initial observation. Initial observations are reference facts, never
missing-input defaults or scalar legality permits.

`mainframe-env-host-api::mq_raw_layout` captures complete MQOD1 (168 bytes),
MQMD1 (324), MQMD2 (364), GMO1 (72), PMO1 (128) and MQCNO1 (12) prefixes. Lengths derive from
the COBOL declarations and joined C byte/character types, not platform-dependent
CURRENT_LENGTH macros. Version, identifier and complete capacity are checked;
opaque identifiers and all other input fields remain exact. The embedding must
explicitly select normal/big or reversed/little integers and ASCII-compatible
or the existing owned CP037 identifier profile. Other encodings reject.
Character observations remain raw encoded bytes. CP037 is an owned embedding
choice, not a claim that MQ mandates CCSID37. MQMD.Encoding describes the body,
never the structure's encoding. Raw signed 32-bit observations are retained even
outside COBOL PIC S9(9); scalar use/writeback separately checks that reviewed range.

Writeback preflights a bounded field batch and the exact captured capacity/prefix
before one copy. Only explicit source-defined output fields may change. Omitted,
unchanged or undefined fields retain input bytes; suffix bytes stay untouched.
Actual observations and platform/model/single-queue applicability must come from
the trusted adapter/service. No status determines output synthesis or mutation
permission. PMO destination counts cannot be written on z/OS; GMO Signal1 remains
an opaque slot and SET_SIGNAL pointer behavior is unsupported. Conditional PUT
correlation/context/group output updates remain pending in this bounded substrate.
The API does not generate names, counts, message IDs, descriptor fields or expiry
clock scaling. Captured numeric Context is an alias observation, never HOBJ
authority. The eventual registry bridge must supply live handles, including the
actual retained HCONN for PUT1, independently of names or options.

`MqRawCapture::writeback_full_get_md` maps an actual complete returned MQMD1/2
value to the same generated output policy and one atomic prefix copy. It requires
GET applicability, the captured version and exact structure character profile;
StrucId/Version remain input-only. Every other common/extension output field is
retained without partial-descriptor narrowing. Numeric byte order comes from
the original capture, not the returned body Encoding/CCSID. Range, stale-prefix,
capacity and late-field failures leave every destination byte unchanged; suffix
bytes remain caller-owned. This helper performs no conversion, defaulting, status
inference, queue mutation or authority check. Diagnostic signed/opaque-byte
fixtures prove value preservation, not native semantic validity. Compiled
OPEN/GET forwarding, trusted catalog/profile binding and multi-argument final
reply writeback must still compose it with actual selected execution.
Sources: original MQGETrow0015 and supplemental baseline2026-09-12
`q097390_204–304`, `q097395_6–30/1498–1508`; source review earns zero credit.

Complete MQMD projection into the current typed message descriptor rejects with
`DescriptorRepresentationPending`: Report, MsgType, Feedback, body encoding/CCSID,
backout/reply/context/origin and OriginalLength lack lossless typed fields. The
additive `MqMdValue` and `MqFullMessage` boundary retains those observations with
explicit FullPut/FullPutOne/FullGet and FullPut/FullGot tags in the existing MQI
canonical authority. Complete GET binds copied bytes, DataLength, original
capacity/truncation and reviewed status without losing the returned MD on rejected
truncation. The existing private result replay codec keeps old storage@1 bytes
exact and uses strict storage@2 only for these full outputs, composing the owned
MD value codec and full host result digest. Full requests remain pending and the
selected provider route explicitly Unsupported until full-MD delivery/checkpoint,
policy and retained receipt integration. No automatic migration or new executable
permission follows from exact representation (see ADR0033 full-MD value). Raw structs
do not create an alternate effect journal or bypass canonical result bounds.

The same delivery kernel now holds tagged partial/complete entries in homogeneous
queue profiles. Additive cold/live/delivery-row @2 projections share the existing
checkpoint validator, MD value codec and property vocabulary. A private explicit
pre-activation quiescent upgrade retains the unchanged rich marker identity and
composes marker/catalog/metadata CAS in one bounded transaction. Populated partial
queues retain their profile; full switches require drained queues, no pending
units/cursors and no selected control/runtime. Previously activated service
retirement needs an owner-approved seam. Ordinary deltas preserve the loaded
schema/profile, @1 bytes stay exact, old readers reject @2 and open never rewrites
state. This is durable storage preparation, not selected full PUT/GET admission,
operator authorization, context/GMT/ID policy, retention or participant acceptance.

Sources are the MQ9.4 original baseline `ibm-mq-9.4-mqi-2026-08-31`, rows
0006/0015/0019/0020/0021, the existing programming supplement baseline
`ibm-mq-9.4-programming-supplements-2026-09-12` (declarations q098100_, q097390_,
q096710_, q098650_ and their field/constant topics), and the separate
`ibm-mq-9.4-point-layout-sources-2026-09-12` elementary/COBOL/encoding topics
q093580_, q093600_, q093630_, q103960_. Exact source pins and fragment locators
remain in the single catalog. Offline checks validate artifact closure; optional
cache-backed generation reproduces the selected facts. Reference review provides
zero execution/licensed credit and no refresh or same-browser capture claim.

MQCNO1 is an additive private raw projection revision in that same catalog. Its
new frozen digest reconstructs and verifies the exact previous five-layout
projection; historical call/status/wire-option identities and canonical DTOs
remain unchanged. The generated prefix contains only StrucId, Version and
Options. Later offsets/pointers are outside VERSION1 and are never read or zeroed.
`decode_connx_default` admits numeric MQCNO_NONE (0) or HANDLE_SHARE_NONE (32)
only with an independently selected ordinary owned nonshared profile. It returns
the existing checked optional manager name, NonShared and ContractDefault.
Application options cannot select host topology, connection scope or authority.
MTS sharing defaults, client/fallback, implicit CICS and binding/security profiles
remain pending. Other recognized flags are unsupported and unknown bits reject;
platform-ignored flags are not silently ignored. Zero-valued aliases cannot
establish binding or reconnect configuration.

StrucId and Version are always input. Options has conditional binding output
semantics, which this input-only adapter explicitly leaves pending: observed
updates reject atomically and unchanged/undefined observations retain exact bytes.
No successful output, status or HCONN is fabricated. The manager still owns the
trusted producer profile, live alias registry and actual service/COBOL bridge.
Source context is original row0009 q101770_ and supplemental q091060_ (numeric
identities), q095410_ (C/COBOL declaration) and q095415_ (field/platform semantics),
under the baselines above. Exact pins and bounded fragment locators are in the
catalog. Conflicting CURRENT_VERSION declarations remain unresolved; this adapter
uses only independently corroborated VERSION1. This decoder earns no MQCONNX
execution or licensed credit and does not broaden the accepted replay schema.

## Authority boundary

The compiled machine's ordinary batch adapter now accepts MQCONNX VERSION1
through `MqMqiProgramFrame::connx_profile`, a read-only interpreter-owned profile
port that defaults to Unsupported. The embedding must independently select the
ordinary owned nonshared profile and explicit big-endian/ASCII-compatible
structure encoding; those are the actual compiled storage rules, not native
endianness, MQMD.Encoding or decoded Invocation bindings. Other encodings,
sharing/client/fallback/binding/security profiles and newer versions fail closed.
The first admitted profile is frozen and compared around lookup callbacks,
before original effect allocation and again before output writeback. Refusal,
panic, changed frame/profile and unusable post-dispatch replies preserve Unknown.

All five CONNX operands must be reference storage. The adapter checks MQCHAR48
manager naming, the real fixed group declaration and direct prefix member types
(also the one source-defined level01/level10 COPY wrapper),
offsets/widths against generated Cno1 descriptors, signed fullword outputs and
all input/output overlaps. CNO capacity is explicitly bounded to 1,024 bytes;
only its twelve-byte VERSION1 prefix is interpreted. Existing raw capture/decode
admits exact options0/32. The original ConnectExtended effect keeps existing
sequence/key/deadline/mutation/context and the separately admitted Invocation.
All input bytes, including CNO Options and suffix, are retained unchanged.
Captured layouts, views and bytes are rechecked before a preencoded bounded
atomic output batch. Only a usable provider-issued opaque token installs an
existing ABI alias. Failed reviewed status preserves undefined HCONN bytes.
Both compiled CONN and CONNX also copy the existing source-reviewed
WARNING/ALREADY_CONNECTED plus Connected shape as exact application CC/RC
1/2002. Only a nonhistorical provider-issued token can install an ABI alias;
an already-observed exact token reuses its alias. A child's first observation
does not allocate a connection or decide a UOW: its trusted provider alone
attests the prior live handle. Status-only warnings, special/historical handles
and other warning/failure output forms remain protected Unknown. The original
reviewed result and canonical identity are retained without OK normalization.
CONN now captures all four compiled argument layouts, views and bytes, checks
input/output overlaps, contains profile callbacks and rechecks the entire
capture before one preencoded atomic write batch. CONNX retains all five ranges,
the frozen independently selected ABI/profile and its final callback recheck.
Neither route writes undefined failed HCONN or fabricates conditional Options.
This warning rule is MQCONN row0008 q101760_ usage271/failed273 and MQCONNX
row0009 q101770_ return76–78/failed41–42 under the original baseline above.
No profile/options grant SAF, lifecycle or UOW authority; no conditional Options
output is fabricated. Sources are original row0009 q101770_ (signature8–18,
scope26–66), MQCONN row0008 q101760_ (manager14–46), and the supplemental
q091060_/q095410_/q095415_ pins above. Compiler-generated machine fixtures provide
no installed selected-provider, SAF or licensed acceptance. Existing
CONNECT/DISC/CMIT/BACK, canonical and checkpoint bytes stay unchanged; typed
source schema0/checkpoint refusal remains. The manager owns live session
forwarding, the actual trusted producer/service route and full26 acceptance.

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

The additive `MqWireFullGet`/`get_full` adapter retains the complete MQMD1/2
observation and explicit structure character encoding, while reusing this same
numeric GMO1 decoder and binding/unit rules. Its first profile is remove/no-wait,
optional accepted truncation, and explicit or platform-default syncpoint. GMO1
matches both nonzero binary MsgId and CorrelId; binary-zero fields are wildcards.
MD2 group/sequence/offset remain exact observations, not additional selectors.
All signed/fixed-byte fields survive, even diagnostic values that the selected
service subsequently refuses. There is no partial descriptor conversion, new
option namespace, outcome generation or handle authority. Native field legality,
configured queue defaults, actual SAF, original intent and provider admission
remain separate. Source: original MQGETrow0015 `q101830_21–48`, supplemental
`q096715_1269–1381` and `q097395_1389–1482` under baseline2026-09-12.
Compiled OPEN/GET forwarding and complete result writeback remain required;
this translator alone is not compiled/native full-call acceptance.

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

### Additive point-layout source scope

`mq-point-layout-sources` separately registers twelve retained MQ 9.4 topics under
`ibm-mq-9.4-point-layout-sources-2026-09-12`. Its topic-manifest@1 and the shared
0.15 registry bind exact bytes, topic-set digest, product and zero-credit scope.
MQOD and MQMO constants, CCSID, expiry, message type and priority constants,
elementary data types, COBOL declarations, structure alignment, COBOL COPY
conventions and binary/machine encoding references supply missing sources for
subsequent point-to-point layout review.
The existing 80-topic scope already pins MQOD/MQMD/MQGMO/MQPMO declarations and
fields, encoding, format, object-type and persistence references; those pins are
reused without duplication or re-hashing. Both scopes use the existing pinned TOC.

Registration is not numeric or layout admission. Later projections must use the
one structure/status catalog and explicit review; source presence cannot supply
defaults, permit unsupported forms or change pending reasons. Original call
rows `0006`, `0015`, `0019`, `0020`, `0021`, all 27 source positions, frozen
80-topic bindings and existing semantic/canonical identities remain unchanged.
No public ABI, handler, machine or service behavior is added. The archive run
remains in-progress without independent browser reproduction and predates MQINQ
issue337 re-pin; identity metadata supplies no freshness or same-snapshot claim.
Source and execution credit remain zero, with all ten reason declarations pending.

### Additive property source scope

The independent `mq-property-sources` scope registers exactly twelve retained
topics under `ibm-mq-9.4-property-sources-2026-09-12`: property names
`SSFKSJ_9.4.0/develop/q022940_.html`, restrictions `q022950_.html`, descriptor
mapping `q022960_.html`, and
`SSFKSJ_9.4.0/refdev/q091110_.html` copy constants; it also pins MQCHARV and field
details, MQCMHO/MQDMHO/MQDMPO/MQIMPO/MQPD/MQSMPO option/structure constants needed
by the five-call property profile. The existing shared reader
and registry bind exact manifest bytes, topic-set digest and product/version.
Frozen call, supplemental and point-layout manifests are unchanged. This is
source registration only: property-name mapping, numeric admission and actual
selected execution need subsequent source review and tests. Source/coverage
credit is zero; the archive remains in-progress without independent browser
reproduction, predates the MQINQ re-pin and establishes no freshness or snapshot
equivalence. No publication bodies or refreshed sources are retained in Git.

### Additive recovery-policy source scope

The independent `mq-recovery-policy-sources` scope registers exactly
`SSFKSJ_9.4.0/refdev/q103230_.html` (HardenGetBackout) under
`ibm-mq-9.4-recovery-policy-sources-2026-09-12`. Its source-owner pin is bound to
the exact existing archive publication metadata, official MQ 9.4 content
endpoint, pinned TOC locator and matching raw HTML. The retained topic path was
checked first; the existing hash-addressed archive supplies the absent retained
file. The pinned queue-attribute list `q102970_.html`, reached from the pinned
MQMD BackoutCount topic `q097395_.html`, supplies the exact incoming link.
Publication `last_modified` comes from verified HTML `lastModifiedDate`, not
capture time. The shared reader and registry bind the manifest bytes and single
topic-set digest. Original call, supplemental, layout and property manifests
and the 26-call/27-position denominator remain unchanged.

This is source registration only. HardenGetBackout defaults, BackoutCount crash
accuracy, abnormal/final task-end policy and recovery semantics require later
owned source review and execution proof. Semantic authority and coverage credit
are zero. The archive remains in-progress without independent browser
reproduction, predates the MQINQ issue337 re-pin and establishes no freshness or
snapshot equivalence. No publication bodies or refreshed sources are in Git.

### Additive RFH2 source scope

The independent `mq-rfh2-sources` scope registers fifteen retained topics under
`ibm-mq-9.4-rfh2-sources-2026-09-12`, product `SSFKSJ_9.4.0`: BMHO/MHBO/RFH
constants, RFH2 declarations and field details, the mapping overview and eight
detailed property-mapping topics, and the JMS-header padding/lexical reference.
The shared topic-manifest
and registry authorities bind exact hashes, bytes and the existing pinned TOC.
The retained topic path is checked first; matching archive bytes supply absent
files. Publication dates come from verified `lastModifiedDate` markup, not fetch
time. Versioned endpoints, TOC headings and archive metadata establish bounded
source identity; missing independent HTML product attributes are not invented.

This is source registration only for original MQBUFMH row `0003` and MQMHBUF
row `0018` (source positions `3`, `18`, `25`). Original 27-topic, supplemental
80-topic, layout 12-topic, property 12-topic and recovery single-topic manifests,
26-call denominator, 1030 status declarations and ten pending reasons remain
unchanged. Numeric/layout/descriptor/option projection and actual RFH2 conversion
require subsequent explicit review and execution. Semantic authority and coverage
credit are zero. The archive remains in-progress, predates the MQINQ issue337
re-pin and has no independent browser reproduction, freshness or same-snapshot
claim. No publication bodies or refreshed sources are retained in Git.

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

### Installed admission and explicit frame session

The server's configured installed-batch factory receives the privately
constructed, nonserializable `InstalledBatchAdmission` after artifact validation
and the winning existing CALL reservation. It observes actual parent/child
linkage, catalog/version and validated artifact/compiler/interface provenance,
the original enclosing ProgramCall and retained parent canonical intent/running
execution, and the frozen physical store/control/host/artifact-store references.
Typed admission rejects absent/stale/mismatched original observations and
rechecks pending CALL/catalog and live monotonic controls around callbacks.
Legacy routing remains unchanged when no typed factory is configured. A direct
scheduler entry lacking that original core occurrence stays pending/Unsupported.

The returned `InstalledMqFrameSession` provides the existing machine frame plus
explicit preparation abort and raw-outcome finish. One server guard disables
executable use if the session's physical store/control differs from frozen
setup, even when all copied records or control observations match. It disables
executable transport before either once-only notification; callback failure or
panic preserves UnknownOutcome. Drop disables use without MQDISC/commit/backout,
parent-state mutation or a cleanup queue. Child return, Condition, suspension or
transfer does not establish task end. Protected CALL/core/replay retention stays
with its existing protocol. See [ADR 0032](../decisions/0032-mq-program-machine-frame.md).

Admission provenance is not a host-root/lifecycle lease or SAF/public readiness.
The selected factory must independently bind the SAME service and physical
store, actual admitted parent/process/task, current incarnation, original effect
publication and task/UOW disposition. Equal stored rows or binding/owner bytes
cannot substitute for that authority. The compiler/published-child/coordinator
fixtures verify server transport with fixture frames/providers; they do not
prove a real selected service, compiled scheduler producer or participant.
Actual host producer, all applicable 26-call contexts, durable typed checkpoint,
SAF/UOW/replay/recovery and participant/CardDemo acceptance remain required.

### Selected message properties

The private selected service admits a finite ordinary nonshared
z/OS batch/queue-manager profile for MQCRTMH, MQSETMP, MQINQMP, MQDLTMP and
MQDLTMH. Its checked `Property` request preserves explicit encoding/CCSID,
version-one default option flags, exact ASCII names, complete MQPD1 and
null/bytes/UTF-8/signed-integer values. It supports exact `Root.MQMD.Field`
case for the common MQMD1 fields, excluding StrucId/Version. Each issued HMSG
owns the associated default descriptor in its existing registry entry; setting
or resetting a field preserves its full fixed width. Stored descriptor scalars
are observations, not PUT legality, expiry-clock conversion, context permission
or a SAF principal. MQMD2/MQMDE extensions remain unsupported in this profile.

An exclusive message candidate holds the actual registry and touched property
entry through publication under the existing selected mutex. Prospective HMSG
observations are permanently historical; abort preserves generations/counters,
and only known adoption returns the issued live token with the committed full
host-result identity. The original retained core intent, logical current unit,
physical store, live controls and mandatory MQUOW/CURRENT resource authorizer
remain independent requirements. Provider rows, the insert-only result receipt
and typed audit publish in one transaction; no property journal or scheduler is
introduced. Host publication uses the existing success-audit convention even
when its exact reviewed MQ result is a defined failure observation. SAF denial
or authorizer failure publishes an audit-only decision, without property changes.

INQMP observations retain MQPD, type, actual returned Encoding/CCSID, returned
name prefix/VSLength, copied value prefix and complete DataLength. Short name
and short value failures retain their exact reviewed FAILED reason pairs;
simultaneously short buffers remain unsupported because precedence is not
reviewed. Absent properties retain the call-specific FAILED inquiry or WARNING
delete result. Undefined nonstring returned CCSID stays absent. New observations
use the sole replay codec's strict storage@3; existing storage@1/@2 and canonical
forms keep their bytes. Readers lacking @3 must refuse it, not down-convert it.

Property handles and associated payloads are volatile. Durable receipts preserve
observations and retained core/UOW dependencies, but cold reopen has no issued
handle or adopted cache proof and cannot revive properties or aliases. A
postpublication UnknownOutcome fences the runtime and retains the receipt;
it grants no redispatch/backout/adoption decision. Legacy KernelV1 conversion
rejects these reviewed entries rather than discard MQPD/CCSID/descriptor fields.
BUFMH/MHBUF RFH2 conversion, wildcard/cursor/conversion/context/special-connection
forms, durable payload integration and installed/compiler/participant/CardDemo
acceptance remain required follow-up work; these private fixtures grant no
official 26-call or public readiness credit.

The ONE structure/status catalog owns the independent private property source
projection; historical call/status/wire/raw projection identities remain frozen.
Sources are original rows0010/0013/0014/0017/0023 in
`ibm-mq-9.4-mqi-2026-08-31`, the pinned programming supplements, and
`ibm-mq-9.4-property-sources-2026-09-12` (including q022960 descriptor properties).
The explicit constants define MQPD_SUPPORT_OPTIONAL=1 and MQCOPY_DEFAULT=22;
the pinned MQPD structure table's inconsistent zero columns are recorded in
the projection and are not substituted for those definitions. This review
changes none of the ten pending reason declarations and earns zero execution
credit. Archive work remains in progress; no freshness or browser reproduction
claim follows from these offline pinned reads.

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
