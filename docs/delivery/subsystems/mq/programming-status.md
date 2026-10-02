# IBM MQ — MQI programming surface progress

Subsystem: **mq**
Phase: **programming**
Target release: **0.15.0**

Status: **Implementation active**

`MQ-1503.zos-backout-policy` adds an explicit private live delivery candidate for
the reviewed MQ9.4 BackoutCount rule (`q097395_` lines1498–1508, supplemental
baseline2026-09-12; original MQBACK/MQGET rows0001/0015). Complete messages
actually removed into a syncpoint unit increment once and saturate at255.
Invalid signed counters refuse the whole unit; browse/rejected truncation,
staged puts and other units do not count. Every other MD/body/property byte and
the legacy partial/storage-only backout/cold projection remain exact. Existing
candidate quota checks precede adoption. This is a deterministic policy primitive,
not selected semantic admission, physical publication, HardenGetBackout crash
accuracy, a task-end producer, native MQMD initialization or new call acceptance.
Selected GET/BACK and final recovery must compose this rule before their native
backout observations can earn credit. All parent requirements remain active.
Five new policy and six unchanged full-storage regressions pass (11 focused
tests, zero ignored), including MD1/2, ASCII/owned CP037, persistent/nonpersistent,
exact repeated decision, invalid-counter mixed-unit rollback and finalization
quota refusal. Thirteen policy tests, actual guards/four mutants, module952/34/4/1,
fmt/docs/changelog pass. Diagnostic surrounding fields are preservation tests,
not native-valid MQI execution fixtures. Unchanged contract/dependency inputs
reuse exact earlier passes rather than new global/deny/CI claims.

`MQ-1506.selected-retention-dependencies` now protects selected recovery graphs
in the existing epoch-fenced core inventory. The owning full rich snapshot reader
validates marker/catalog/delivery/control/unit/receipt together, delegating old
and full storage outputs to the sole lossless result codec. Every retained
receipt execution/effect and unit logical execution/original CONNECT key stays
blocked, including empty/final units; malformed/unknown/orphaned state sets the
conservative unowned fence. No runtime, age, archival permit or new target is
created. Four provider and three server regressions plus 13 rich-reader, three
existing safety and one coordinator retention regressions pass (24 total, zero
ignored), with owned SQLite reopen, real core-row protection, stale epoch refusal
and actual guards/four row mutants/module937/fmt/docs/changelog. Original failed
compile/test receipts remain failed; their repaired inputs pass separately.
Dependency policy reuses only original d8 and all 32 unchanged inputs; unchanged
contract inputs retain the genuine global API-doc pass from `54bcfeae`, not a
new gate run. Selected retirement/post-persistence age, root task end, recovery,
actual full delivery, participant/IR/CardDemo/all26 still require implementation.

`MQ-1506.host-api-doc-repair` composes sealed worker `d3c7b274` with the
execution/store repair below. The genuine global API-documentation gate now
passes at exact counts compiler 70, coverage 386, execution 0, host 931 and
store 72. No limit increases, suppressions or new exemptions are introduced.
Three request-family groups move behind unchanged public reexports; independent
reconstruction preserves declaration/implementation tokens and literal bytes.
The actual request-module allowance decreases 2268 to 1701; all new children
stay below 1200. The combined checkout passes 233 host tests, zero ignored,
and required policy/row/module/fmt/docs gates. Historical failed receipts keep
their original candidate identities; this fresh repair is not a release waiver.

`MQ-1503.full-message-boundary` adds complete FullPut/FullPutOne/FullGet requests
and FullPut/FullGot observations to the existing 26-call authority, composing
every MQMD1/2 field and ordered properties in the sole canonical encoder.
GET binds actual copied prefix, DataLength, original capacity/truncation and
reviewed status, including returned MD on rejected truncation. The same replay
codec preserves exact old storage@1 and uses strict storage@2 only for full
outputs; historical reconstruction grants no live authority. Sealed worker
`f4bccf48`, ADR0033 and original rows `0015/0020/0021` define this value boundary.
Full requests remain explicitly pending/Unsupported before selected mutation;
source-bound per-call policy, delivery/checkpoint/receipt evolution and actual
PUT/GET/PUT1 execution remain required. This is not new executable call credit.

`MQ-1501.typed-connection-warning` now preserves the exact reviewed warning
`1/2002` and defined nonhistorical issued HCONN for compiled CONN/CONNX, reusing
the same ABI alias or recording a child's first observation without minting a
provider handle/UOW. Both calls capture bounded compiled ranges and recheck
original frame/profile/layout/bytes before one atomic output batch. Undefined
failed HCONN is retained; unusable/pending/historical/uncertain replies cannot
partially write or install an alias. Sealed worker `a3bca54f` and integrated
`e139bfd8`, original MQ9.4 rows `0008/0009` under
`ibm-mq-9.4-mqi-2026-08-31`, define this machine slice, not actual provider reuse
or installed-service/SAF acceptance.

`MQ-1505.selected-provider-identity`, sealed `78cdde60`, adds only a read-only
physical-Arc identity observation through the existing frozen host registry.
Equal descriptor/generation values cannot substitute another selected provider;
existing ready/missing refusals and independent invocation admission remain.
This necessary bridge composition check grants no lifecycle, SAF or UOW permit.

`MQ-1505.selected-connection-warning`, integrated `e85d389b`, now reuses the
actual unique live issued connection for ordinary nonshared CONN/CONNX under
the same logical owner, original CONNECT key, current unit and closed directory.
Mandatory original core/SAF/control checks and atomic receipt/audit/CAS remain;
exact reviewed `1/2002` performs no new allocation, delivery-clock advance or
pending-work change. Cached replay must still match the actual issued reply and
live authority. Manager composition passes 193 focused tests, zero ignored,
including Memory/SQLite late-CAS, audit rollback, uncertainty and incarnation
fences. Original baseline rows `0008/0009` distinguish a task from its subtasks.
Private provider fixtures do not prove the configured installed/public route.

`MQ-1506.execution-store-api-docs`, integrated `47e35251`, documents the
execution/participant and audited publication/CAS boundaries without changing
executable code. Exact missing-documentation ratchets decrease from execution
303/176 and store 98/95 to execution 0 and store 72; no ceiling increases or
exemptions are introduced. The original host failure 2546/1128 retains its
historical candidate identity; the separate host repair above supplies a fresh
passing combined global gate. Documentation still grants no licensed evidence
or release acceptance.

`MQ-1505.configured-installed-bridge` consumes sealed worker `b16013ecd`.
Deliberate privileged Rust setup opens only existing strict rich state and binds
the SAME physical provider/store/clock/control to genuine original compiled
artifact/core/winning CALL admission. Bounded root/frame maps retain original
same-task lineage. Ordinary `cobol.call@1` forwards its existing winning proof
through the same guarded session as batch input; preparation abort is once-only
and finish observes the untouched raw outcome before cursor/linkage mapping.
Real compiled nested/successive CALLs use the actual selected provider on Memory
and owned SQLite with mandatory recording/denying SAF, original core/receipts,
both provider and coordinator audits and actual lifecycle/outbox records.
Normal nonfinal returns retain prior connection/work; late CAS/audit/control
failure and cold incarnation fence without redispatch. Drop revokes transport
only, never deciding a durable UOW. ProductServer defaults remain legacy; no
startup normalization, operator permission, installed RACF/shared-participant,
typed checkpoint/task-end/recovery, CardDemo or all26 completion is claimed.
The strict fixture is reproducibly generated by the existing quiescent import
planner and grants setup credit only. Independent manager checks bind all22
worker paths,479 tested source inputs,125 receipts and five original source pins.
The composed candidate passes 56 MQ session/configured, three affected legacy
CALL and 12 replay regressions (71 focused tests, zero ignored); one separate
setup-only generator reproduces the exact fixture bytes. Thirteen policy tests,
canonical/provider guards, four actual row mutants, module950/34/4/1, fmt,
normal docs generation/check and changelog pass. Unchanged contract and32
dependency inputs reuse exact earlier passing receipts, not new global/deny/CI
runs. Historical worker failures remain attached to their original candidates.

`MQ-1501.installed-connx-observation` forwards the additive trusted CONNX profile
through the same exact-original Invocation and pre/post revocation guard as
profile/current-unit lookup. Encoding/profile observations are unchanged; older
embeddings remain Unsupported. Finish/abort/Drop suppress escaped observations,
including a concurrent late return; callback panic irreversibly revokes transport
as protected Unknown without cleanup/retry. This transport-only composition uses
the already reviewed row `0009`/CNO source boundary and selects no ABI, queue
manager, handle, SAF or UOW authority. The real configured producer remains the
next installed integration obligation.

The deliberately privileged Rust `MqTrustedBatchRuntime/Root/Frame` facet now
opens only existing strict rich state and retains one selected service, physical
store, mandatory SAF/clock and frozen limits. It preserves exact unbound original
roots and checked opaque same-task child lineage; it cannot attest application
bindings or fabricate an installed host proof. Original dispatch reuses core,
SAF/audit/UOW/CAS/receipt/replay authority. Preparation abort and normal nonfinal
child return are explicit and once-only; uncertainty fences/retains, and Drop
makes no durable decision. Worker `f80e9a12`, ADR0032 and original rows
`0001/0007/0008/0009/0012` define this provider slice. Its inherited batch-module
failure is not relabeled; the manager composes the separately committed downward
ratchet repair. Actual installed producer, deliberate normalization/selection,
final task-end/checkpoint/retention/participant/full26/CardDemo remain pending.

The complete MQMD VALUE primitive now retains every version-one/two signed
MQLONG and fixed character/byte field, with a separate explicit structure
character profile and no invented v2 fields. Projection uses the one generated
raw catalog; the new bounded strict value codec reuses existing canonical
primitives. Original effect/result/replay/storage/checkpoint bytes and the old
partial descriptor remain unchanged; partial projection still refuses pending.
Worker `71d66481`, ADR0033 and original rows `0015/0020/0021`, supplemental MQMD
`q097390_/q097395_/q091870_` and the point-layout scalar/encoding pins define this
slice. Full-message request/result/replay value composition is integrated above;
actual PUT/GET, context/SAF, delivery and durable evolution remain pending.
The worker's original global API-doc failure retains its candidate identity;
the separate repairs above pass a fresh combined gate without waiving or
relabeling that failure.

The compiled typed adapter now recognizes original MQCONNX VERSION1 calls through
an additive, default-Unsupported trusted profile/encoding port. It checks real
compiled direct/COPY-wrapper CNO group layouts, all five reference ranges and
unchanged input bytes; only independently selected ordinary nonshared big-endian
ASCII-compatible storage is admitted. Exact issued-token aliases and bounded
atomic CONNX writeback reuse the existing machine authority. Original row `0009`
(`q101770_` signature and COBOL lines 220–228), row `0008` (`q101760_` name rules)
and supplemental `q091060_/q095410_/q095415_` define the source review. The worker
is sealed as `8fdbcd93`; its compiler fixtures are not installed-provider/SAF
acceptance. Guarded server forwarding and compiled warning handle writeback are
now separately integrated; actual selected service production, conditional
Options output and accepted typed checkpoints remain
separate composition work. Legacy canonical and checkpoint identities stay exact.

The existing reviewed-output contract now retains MQCONN/MQCONNX's exact
`MQCC_WARNING/MQRC_ALREADY_CONNECTED` pairing with its defined connection output.
Other warning/failure output pairings remain closed. The existing strict replay
codec preserves its full canonical identity but reconstructs only historical
non-executable handles; registry lifetime/owner checks remain independent.
Original MQ 9.4 rows `0008/0009`, MQCONN usage line 271 and MQCONNX return lines
76–78 define this source review. Selected provider reuse of its actual prior
connection and compiled warning writeback are separately integrated above.
The genuine configured installed route remains pending. No token,
duplicate connection, SAF permit or additional execution credit is fabricated.

Installed executable frames now forward read-only profile and current-unit
observations through one exact-Invocation, pre/post-callback revocation guard.
Panic revokes transport and returns protected Unknown without retry or cleanup;
finish, abort and Drop suppress escaped observations. Present inherited ordinary
batch MQ bindings are retained exactly; malformed or conflicting MQ contexts
reject before factory/dispatch, and only missing child context receives trusted
setup. Original parent/core/CALL provenance is never rewritten. Compiled-child
and genuine coordinator Memory/SQLite regressions cover CONNECT/CMIT/BACK/DISC,
but use fixture MQ/frame transport rather than an actual selected service/SAF.
Source baseline `ibm-mq-9.4-mqi-2026-08-31`, rows `0001/0007/0008/0009/0012`,
does not make these observations durable UOW or task-end authority.

An explicit private trusted-host context plane now admits an unchanged original
ordinary batch parent even when its MQ binding is absent. The opaque directory
freezes binding-only versus explicit mode; old routes never fall back. Present
malformed/conflicting MQ or CICS contexts, foreign/stale parent/probe and widened
child controls still fail closed. One bounded directory proof carries the exact
original Invocation and owner to strict effect admission, without rewriting
core/CALL, envelope, SAF, audit, UOW or replay identities. Same-task child-first
CONNECT, parent CMIT/BACK and cold-incarnation fences are covered on Memory and
SQLite private fixtures. Original MQ 9.4 rows `0001/0007/0008/0009/0012` remain
the source boundary. The actual installed same-service producer, public trusted
embedding bridge and shared participant acceptance remain separate obligations.

The checked raw-layout contract now includes MQCNO version 1's twelve-byte
StrucId/Version/Options prefix. Exact options zero or 32 decode only with a
separately supplied ordinary owned nonshared profile; options cannot select
host, sharing, security or binding authority. Conditional IBM Options output
remains pending, and writeback preserves omitted, undefined and suffix bytes.
The additive raw projection preserves the prior five-layout identity and all
original canonical/status/call identities. Original row `0009` and supplemental
`q091060_/q095410_/q095415_` under the pinned MQ 9.4 baselines define this
bounded source review. This is not executable CONNX or licensed call credit.
Installed memory/profile/alias routing and broader CNO versions remain required.

The installed server now supplies a private-constructor, non-Clone/non-Serde
admission observation only after validated artifact/catalog selection and winning
the original durable CALL reservation. It preserves the actual parent and
parentSome child, original core intent/running parent/CALL identity and frozen
physical store/control/host/artifact adapters. One explicit frame-session guard
invalidates transport before once-only abort/finish, observes every untouched raw
coordinator outcome before output mapping, and makes no durable decision on Drop.
Factory callbacks run outside setup locks; foreign physical store/control,
late provenance/control changes and uncertain notifications fail closed.
Fixtures prove actual compiled child/core/CALL dispatch, not an owned selected
MQ service or independent root/topology. The actual same-service host bridge
remains a composition obligation before public registration. Source baseline
`ibm-mq-9.4-mqi-2026-08-31`, rows `0008/0009/0012`, distinguishes actual task
end from child return; raw outcomes are not invented task-end authority.

Checked raw COBOL prefixes now retain every MQOD1/MQMD1/MQMD2/GMO1/PMO1
field byte and actual capacity, with explicit trusted integer/character encoding.
The independent raw-layout projection is generated from the existing single
structure/status catalog and hash-bound supplemental/layout sources, preserving
the original call, status, wire-option and canonical identities. Observed-field
writeback checks complete preflight before one bounded copy, preserving omitted,
undefined and suffix bytes; numeric aliases and Signal1 remain observations,
not executable handles or pointers. No defaults, names, counts or descriptors
are fabricated. Original rows `0006/0015/0019/0020/0021` and the
`ibm-mq-9.4-programming-supplements-2026-09-12` /
`ibm-mq-9.4-point-layout-sources-2026-09-12` pinned declarations are the source
boundary. Full typed MQMD representation, installed memory/alias routing and
real provider execution still require composition; this contract earns no
additional call or licensed execution credit.

The typed machine now emits MQCMIT/MQBACK for an independently admitted ordinary
batch/local-MQ frame. Its live-token UOW lookup is a read-only assertion, not
authority reconstructed from bindings or an application integer. Original
sequence/key/actor and the selected provider's logical-owner/control/CAS checks
remain unchanged. Reviewed status observations are copied exactly without local
durable decisions; mismatched units, changed frames and unusable post-dispatch
typed envelopes are protected Unknown before writeback. Legacy validation and
checkpoint bytes remain unchanged. Selected service wiring, shared participants
and typed recovery remain required.

Private same-task batch-child ownership now consumes an opaque admitted parent
lease and explicit host-supplied SAME TASK relationship. It retains the frozen
logical processing-unit origin across child references and validates existing
durable UOW ownership against that proof without changing `UnitOwner@1` bytes.
Child effects, retained core intent, SAF, audit and receipts keep the actual
child actor; return/abort retire only the appropriate volatile frame. Same
physical control/CAS, audited transaction and incarnation fences remain required.
Fixture provider tests are not an installed/public host producer. A checked
same-task child can now create the task's first default/nonshared connection.
Its original CONNECT key remains durable provenance while the admitted logical
root owns the UOW; its actual child actor still owns the effect, SAF, audit and
receipt. Normal nonfinal child return retains that connection, objects and work
for surviving admitted frames, without implicit disconnect or UOW decision.
Already-connected warning/output composition is integrated above; final task
end, configured abnormal/Unknown recovery, checkpoints, participants and full
26-call acceptance remain separate obligations. The source boundary is original baseline
`ibm-mq-9.4-mqi-2026-08-31`, rows `0008/0009/0012`; task excludes subtasks.

The additive `mq-point-layout-sources` scope now registers twelve hash-verified
retained MQ 9.4 layout/scalar/encoding topics independently of the frozen
80-topic programming supplements and original 27 call positions. Its baseline
is `ibm-mq-9.4-point-layout-sources-2026-09-12`; manifest SHA-256 is
`128e12e5a276b0b3613ce253f357918810b7f74c5351f8064caaea17d1f166fa`.
The shared reader and registry keep separate scope closure and zero credit.
This supplies sources for subsequent reviewed layout projections, not numeric
admission, wire execution, a new browser capture or licensed certification.

The complete-payload delivery storage feature is integrated in the existing
kernel: homogeneous partial/complete queue profiles, strict additive cold/live/
row @2 projections and a private pre-activation quiescent upgrade plan under the
unchanged rich marker/catalog/metadata fences. Old @1 partial bytes remain exact;
wrong profiles refuse before adoption. Actual selected full GET/PUT/PUT1, trusted
context/IDs/GMT/expiry policy, activated-service retirement and authorized operator
deployment/backup/rollback remain pending. No automatic rewrite or public-ready
claim is added. The composed candidate passes90 focused delivery/rich/import/
selected-retention/server/coordinator tests, zero ignored,13 tooling policy tests,
required guards/four actual namespace mutants, module945/34/4/1, fmt/docs/changelog.
Worker path/test-input/source and corrected stable-receipt identities were
independently verified; its original self-stdout inventory failure is preserved.
Only source registration/storage/private tests are credited to their boundaries;
full26/participant/CardDemo acceptance remains required and licensed0/26 skipped.

The independent `mq-property-sources` scope now registers twelve retained topics
for property names/restrictions, descriptor mapping, variable strings and property
option/structure/copy constants, baseline
`ibm-mq-9.4-property-sources-2026-09-12`, manifest SHA-256
`f1537d0ab7feba5c7260e5dced3e9878b5bf999b274e0254f888fc1d78d96ba7`.
Frozen original27/supplemental80/layout12 bindings remain unchanged. Hash-verified
archive metadata and retained bytes are not refreshed browser capture, semantic
admission or execution evidence; the in-progress archive's reproduction/freshness
caveats remain. Source and licensed execution credit stay zero. Actual selected
property transitions, associated-descriptor mapping and full26 gates remain.

The continuation's module-budget composition repair passes the global module
guard after unchanged validation, input-projection and conversation helpers are
split from the inherited server/application/IMS/CardDemo modules. IMS generic
tests move into the existing test-only directory; the MQ status generator's
comment header now matches the generator-owned-header policy. All inventory
changes lower exact counts or remove a no-longer-oversized MQ exemption; no
ceiling increases or new exemptions are introduced. The focused composed
regressions pass 53 tests with zero ignored. This is policy-gate repair, not
additional MQ call, participant, CardDemo-full or licensed execution credit.

Current user-directed acceptance exception (2026-10-02): the user explicitly
requested skipping the licensed IBM MQ differential gate after the missing
authorized oracle environment/receipt was reported. Do not request or run that
external gate for this continuation. Licensed execution remains **skipped, 0/26**,
not passed or certified. The original release contract and historical required
gate descriptions below are retained as provenance; this exception does not
waive owned execution, source, security, persistence, participant or CardDemo
checks, and no other acceptance requirement is reduced.

- Completion branch: `codex/mq-v015-continuation`
- Continuation-wave base: `213ed878` (current main after PR #381 merged)
- Subsequent base integration: `f0727cf8` (accepted Db2 PR #385); MQ production
  inputs remain unchanged. Documentation registry/navigation preserve both
  subsystems' distinct proposed ADR filenames, including shared numeric prefixes.
  Fresh dependency policy is required for the imported Db2 manifest/lock edges;
  unchanged MQ execution receipts retain their original candidate identities.

This recovery ports lost commits `afe2b893`, `6026c4b9`, `2b7968e3`,
`01eb5ebb`, `83223a6f`, and `972fab3a` onto current main. The first adds the identity-only registry for all 26 unique IBM MQ
9.4 MQI calls, preserves all 27 displayed source-list positions (including the
second `MQMHBUF` row), and wires the source schema, generator, tests, and docs.
The registry does not register handlers or grant behavioral or licensed credit.
The two later commits add the typed syncpoint applicability matrix and reject
direct CICS commit/backout before MQ UOW mutation while retaining the attested
internal CICS SYNCPOINT dispatch.
The fourth adds the source-bound structure/status catalog and generated
read-only host contract descriptors.
The fifth recovers the owned MQ-1502 object catalog, deterministic resolution,
model queue lifecycle, and strict restart snapshot codec. Issue #342 fixes the
existing queue service's case folding and leading blank acceptance. These are
kernel and service regressions, not executable MQI call coverage.
The sixth adds the MQ-1506 licensed differential adapter, independent fixture
index, external receipt schema, fail-closed verifier, and mutant tests. No
licensed run or receipt was recovered.

The completion branch now also carries sealed slices for the bounded MQ handle
registry, source-bound call-shape validator, typed object-catalog service
integration, volatile message-handle properties, point-to-point delivery and
pub/sub. The object service persists one catalog authority in the existing
provider-row store, routes local queues and aliases through it, migrates the
legacy queue-only manifest atomically, and keeps model, remote, topic and
pre-created dynamic execution fail-closed until their request contracts exist.
The three new executable kernels are pure provider authorities; public MQI
routing and durable service integration remain deliberately unclaimed.

## Source review

The denominator authority is baseline `ibm-mq-9.4-mqi-2026-08-31`, catalog
`conformance/0.2/catalogs/mq.json`, and manifest
`conformance/0.2/manifests/mq-topics.json`. The call list is pinned at
`SSFKSJ_9.4.0/refdev/q101650_.html`, SHA-256
`24025a9f40dfc613b8243fef16f902a94794a9fbe517730ae1b6c489a338009f`.
The 26 other call topics are pinned in the same manifest. Matching retained
HTML was checked in the offline corpus; publication bytes remain outside Git.

Issue #337 re-pins `SSFKSJ_9.4.0/refdev/q101840_.html` (`MQINQ`) to SHA-256
`03e3347bbf16d2f8e3a9061e921dbfca7a3afd0fe3bc13418ebdf47bb652ce1b`
(112,180 bytes) after a Chrome check of the live page (owner-approved re-pin). The page metadata says
`Last Updated: 2026-09-10`, which is the manifest's `last_modified` value.
The manifest's `repin` block binds the superseded hash and manifest identity.
The configured retained HTML root has no MQ scope. All 27 manifest topics and
the pinned TOC were therefore verified by digest in the owner-provided raw
archive at `/Users/tore/Library/Caches/mainframe-env/ibm-docs-archive/raw`,
materialized into bounded temporary reader caches, and reviewed with
`ibm_docs.py search` and `read`. No network or browser refresh was used, and no
semantic or licensed credit is inferred from retained publication bytes.

The syncpoint matrix was reviewed against the same IBM MQ 9.4 baseline and
catalog rows `0001` (`MQBACK`), `0002` (`MQBEGIN`), and `0007` (`MQCMIT`). Their
verified retained topics are `SSFKSJ_9.4.0/refdev/q101690_.html` (SHA-256
`9550bf98c66f47f1d61943e0dbb7ab89043d0db1a3918ea3a4314dccf7182c86`),
`SSFKSJ_9.4.0/refdev/q101700_.html` (SHA-256
`142301ff6012e8ca253539cba7d2c7fcc0d60801b24caff5a56954c5d47a03d8`),
and `SSFKSJ_9.4.0/refdev/q101750_.html` (SHA-256
`590f32c213d129d6937c253f048ccdb9c5cbd963ca2d3310cf5672dcf68c42f4`).

MQ-1502 name handling uses the same baseline, catalog row `0008` (`MQCONN`),
and retained `SSFKSJ_9.4.0/refdev/q101760_.html` (SHA-256
`fa0cdd2c5e19326dfb91e5ad0b921fd47a1a3a918682e13c4ff5e36c2ba40347`).
The configured reader cache lacks the pinned MQ scope; the matching retained
HTML was read locally and verified against its manifest hash. This is source
review only and grants no conformance or licensed credit.

## Work-package ledger

| Slice | State | Scope |
|---|---|---|
| MQ-1501.call-denominator | Recovered | 26 identities, 27 source positions, deterministic generated registry |
| MQ-1501.syncpoint-owner-cics | Recovered | Direct CICS commit/backout rejection; attested CICS participant dispatch |
| MQ-1501.syncpoint-context-contract | Recovered | Typed MQBACK/MQBEGIN/MQCMIT applicability matrix |
| MQ-1501.host-context | Complete slice | Typed trusted host binding covers z/OS batch, IMS batch DL/I, CICS, IMS, MQI client and other bindings; forbidden direct commit/backout is rejected before mutation while the attested CICS participant path remains available |
| MQ-1501.structure-status-catalog | Recovered | Source-bound MQI parameter, structure/version, option/selector, completion/reason and handle identities; 169 parameters across 26 calls, one reviewed source-spelling anomaly |
| MQ-1501.structures-options | Partial | Source-bound shape/family validation is sealed; numeric legality and executable structure behavior remain pending |
| MQ-1501.handle-registry | Complete slice | Bounded typed HCONN/HOBJ/HSUB/HMSG ownership, sharing, generation, epoch, release and cascade contract |
| MQ-1501.structure-option-validator | Complete slice | Source-bound ordered signature, structure identity/version, option-family and documented combination validation; execution remains unsupported |
| MQ-1502.object-kernel | Recovered | Typed object catalog, resolution, model instance lifecycle, strict snapshot codec |
| MQ-1502.service-name-fix | Recovered | Case-sensitive queue service names and malformed name rejection before mutation (#342) |
| MQ-1502.object-service-integration | Complete slice | One typed catalog authority in existing provider rows; local/alias routing, authorization-before-mutation, CAS, SQLite reopen and queue-only migration |
| MQ-1502.object-route-contract | Complete slice | Source-bound, bounded MQOPEN/MQCLOSE object, access, modifier, dynamic-name and close-lifecycle request vocabulary; unsupported and pending execution forms stay explicit |
| MQ-1503.message-contract | Complete slice | Source-bound bounded descriptor, property, selector, browse/wait/truncation, group/segment, distribution and uncertainty vocabulary; no handler registration or execution credit |
| MQ-1506.licensed-harness-contract | Recovered | External IBM MQ 9.4 receipt contract and 26-call fixture index; no licensed execution |
| MQ-1503.message-handle-kernel | Complete slice | Registry-owned volatile HMSG lifetime, bounded typed properties and deterministic private buffer conversion |
| MQ-1503.point-to-point-kernel | Complete slice | Bounded local delivery, browse/cursors, selection, truncation, grouping/segmentation, syncpoint/backout, expiry, uncertainty and strict restart snapshot |
| MQ-1504.pubsub-kernel | Complete slice | Catalog-backed subscriptions, callback control, retained publication requests, trigger/delivery uncertainty, syncpoint staging and strict durable restart |
| MQ-1505 recovery and public-route integration | Pending | Shared host request/result boundary, Memory/SQLite service wiring, SAF/status mapping, dead-letter/retry/recovery and licensed execution |

Existing typed host routes support only the earlier open, get, put, put-one,
close, commit and rollback compatibility operations. The remaining catalog
identities are not executable. Unsupported forms must be rejected before
side effects. Licensed IBM MQ 9.4 differential credit remains **0/26**.
The object catalog is wired to the existing queue compatibility route for local
queues and aliases. The current request cannot express dynamic names, close
modes, remote routing or topic operations, so those forms remain explicit
fail-closed behavior. MQINQ behavior is not implemented.

The licensed verifier requires an external authorized receipt bound to the
current clean Git candidate, pinned source and service identities, independent
fixture bundle, and distinct product and oracle runners. It rejects malformed,
partial, or stale receipts before granting call credit. MQINQ now uses the M1
manifest re-pin; all 26 call topics have retained HTML identities. The
owner-provided raw archive supplied digest-matching topics and TOC to bounded
temporary reader caches, so the completion lanes repeated `ibm_docs.py search`
and `read` without a network refresh.

The structure/status catalog is bound to the pinned MQ 9.4 topic manifest and
the 27-row source list. All 26 call topics were verified against retained HTML;
the MQINQ signature uses the issue #337 re-pin. Its generated host contract is
read-only and carries no behavioral or licensed credit. The MQBUFMH source
spelling `MQHMQSG` is retained only as reviewed provenance; the published
message-handle identity is `MQHMSG`, corroborated by
`SSFKSJ_9.4.0/refdev/q101710_.html` (SHA-256
`8a94879a9c9f2e18ddb2171b0dd5ea477ebaf26684a760c9e1e31931f2084156`)
and `SSFKSJ_9.4.0/refdev/q101780_.html` (SHA-256
`66cf482573408e227aec23ec2219cd52bf7bf879591f33acb04dc3f33fb386db`).

## Completed foundation completion wave

The completion manager runs at most three isolated Codex CLI workers alongside
the manager. Workers use `gpt-6-sol` with `xhigh` reasoning, goal mode, approval
bypass and fast mode disabled. Each lane starts from the completion-wave base,
reads only digest-verified retained IBM HTML through `ibm_docs.py`, owns the
paths below, adds a unique `changes/unreleased/*.toml` fragment, runs focused
checks, cleans its Cargo target, and seals one feature commit.

| Lane | Slice | Owned paths |
|---|---|---|
| A | `MQ-1501.handle-registry` | Sealed and integrated as `111752c7` |
| B | `MQ-1502.object-service-integration` | Sealed and integrated as `8b3a3b62` |
| C | `MQ-1501.structure-option-validator` | Re-sealed after the expected facade overlap and integrated as `9bd63bdc` |

The integrated candidate passed the host API, MQ provider, object lifecycle,
typed object service, SQLite/memory provider-row, generator, formatting and
changelog checks. The optional PostgreSQL move-contract case remains skipped
because no disposable `MAINFRAME_ENV_TEST_POSTGRES_URL` is configured.

## Completed contract-freeze wave

The next three isolated lanes retain the same `gpt-6-sol`, `xhigh`, goal-mode,
approval-bypass and fast-disabled settings. They complete prerequisites before
point-to-point, pub/sub and recovery integration starts.

| Lane | Slice | Owned paths |
|---|---|---|
| D | `MQ-1501.host-context-enforcement` | Sealed and integrated as `d0c95899` |
| E | `MQ-1502.object-route-contract` | Sealed and integrated as `ae9a7c48` |
| F | `MQ-1503.message-contract` | Reconciled across the expected facade overlap, re-sealed, and integrated as `dc8c655e` |

Shared request enums, canonical encoders, provider service, status/dossier and
generated docs remain manager-owned unless a lane proves its declared new
module cannot be expressed without the minimal facade hook. Lanes E and F share
only the expected facade registration, which the manager will reconcile and
re-seal during integration. No lane may register handlers or claim behavioral
or licensed coverage.

The reconciled candidate passed all 100 host-API tests. The object-route and
message contracts are non-executable: their pending dispositions remain
binding, and neither changes the 0/26 licensed differential count.

## Completed executable-kernel wave

The three isolated lanes started from contract-freeze candidate `dc8c655e` plus
the manager-owned declaration commit. They retained `gpt-6-sol`, `xhigh`, goal
mode, approval bypass and fast mode disabled. Each lane repeated bounded offline
`ibm_docs.py search` and `read` against digest-verified retained HTML, owned only
its new provider module plus the minimal facade line and unique change fragment,
ran focused tests and required gates, cleaned its Cargo target, and sealed one
feature commit. The shared service, request/canonical encoders, status, dossier
and generated docs remained manager-owned.

| Lane | Slice | Exact source rows and semantic scope | Backend/public-route disposition |
|---|---|---|---|
| G | `MQ-1503.message-handle-kernel` | `0003` MQBUFMH, `0010` MQCRTMH, `0013` MQDLTMH, `0014` MQDLTMP, `0017` MQINQMP, `0018` MQMHBUF (both displayed provenance positions), and `0023` MQSETMP; bounded HMSG lifetime, typed property mutation/inquiry/deletion and buffer conversion | Reconciled and integrated as `6d55a2d9`; provider kernel plus `MqHandleRegistry`, with public route, durable-store and licensed credit still pending. |
| H | `MQ-1503.point-to-point-kernel` | `0015` MQGET, `0020` MQPUT and `0021` MQPUT1; put/get/browse/remove-under-cursor, identifier selection, truncation, grouping/segmentation, distribution, syncpoint/backout and explicit duplicate/unknown outcomes | Re-sealed across the shared facade and integrated as `71607bcb`; public host route and Memory/SQLite store integration remain pending. |
| I | `MQ-1504.pubsub-kernel` | `0004` MQCB, `0005` callback invocation, `0011` MQCTL, `0025` MQSUB and `0026` MQSUBRQ; bounded topic/subscription, callback-control, asynchronous delivery, request-publication, trigger and duplicate/unknown states | Re-sealed across the shared facade and integrated as `aa149bdc`; public host route, SAF/store integration and licensed credit remain pending. |

All three lanes use the typed host-context applicability already enforced by
MQ-1501. Mutating transitions are bounded and fail before partial mutation; H
and I carry their syncpoint/backout and restart obligations in the kernel slice
instead of deferring them to MQ-1505. The integrated provider suite passed 59
unit tests, four object-lifecycle tests and six object-service integration tests.
Formatting, changelog, docs, registry, licensed-contract and dependency-policy
checks passed. The licensed verifier remains `pending-external-licensed-receipt`
at 0/26 because the required external IBM MQ 9.4 receipt and exact external pins
are absent. The next implementation step is one manager-owned additive public
request/result boundary before service integration.

## Integrated continuation wave

The manager uses Codex CLI directly, with at most three isolated workers plus
the manager. This wave uses `gpt-6.1-sol`, high reasoning, goal mode, approval
bypass and fast mode disabled. Previous wave settings above are historical.
Workers read digest-verified retained HTML from the owner's raw archive through
the repository offline reader; they do not refresh sources or spawn workers.

| Lane | Declared slice | Exclusive implementation ownership |
|---|---|---|
| J | `MQ-1501.mqi-request-boundary` | Additive typed MQI request/result vocabulary and canonical encoding in host API new modules, with minimal facade/encoder hooks and crate-private opaque-handle identity projection; all 26 identities stay source-bound, unsupported wire/status details explicit. Shared `HostRequest` dispatch and providers remain manager-owned. |
| K | `MQ-1502.object-inquiry-kernel` | New bounded provider object-inquiry module, exact catalog-backed attributes supported by pinned MQINQ, with minimal facade export; unsupported selectors and MQSET remain explicit pending, no second catalog. |
| L | `MQ-1505.delivery-checkpoint-kernel` | Delivery module and child codec modules only: strict live checkpoint preserving pending operations versus existing restart/backout snapshot policy, bounds, recovery and atomic malformed-input rejection. No private durable journal or provider service. |

Each lane owns a unique change fragment, focused regression tests and a sealed
feature commit. Facade-only overlaps are reconciled and re-sealed by the manager.
Service, shared dispatch enums, documentation, evidence and public capability
registration remain manager-owned. These prerequisites do not grant licensed
credit or imply that all 26 calls are publicly executable.

J binds all 26 catalog identities and requires bounded, distinct typed inputs,
deterministic canonical identities, malformed/capacity rejection, and explicit
pending wire/result forms. K binds rows `0016` MQINQ, `0019` MQOPEN and `0022`
MQSET: catalog identity versus resolved identity, selector order/duplicates,
access applicability, output capacities and rejection without mutation. L binds
rows `0001` MQBACK, `0007` MQCMIT, `0015` MQGET, `0020` MQPUT and `0021` MQPUT1:
pending-operation retention, explicit resume versus cold recovery, monotonic
identities, no duplicate commit/backout, expiry and atomic bounds/corrupt-state
rejection. All are internal contracts/kernels, with backend/public-route and
licensed gates pending, not excluded.

The manager also owns `MQ-1501.shared-handle-kernel`, limited to message-handle
and pub/sub provider modules, new focused tests and a unique change fragment.
Rows `0008` MQCONN, `0010` MQCRTMH, `0012` MQDISC and `0025` MQSUB require one
connection/handle authority across both kernels: property and subscription
tokens share a registry, disconnect/unit/epoch retirement is coherent, and stale
or foreign tokens fail without mutation. No numeric/wire or public-route claims
are added. Shared lifecycle semantics remain those of the frozen registry.

The manager declares the review repair `MQ-1501.handle-access-guard`: scoped
registry/message-kernel access, lifetime-only registry observation, and bounded
reclamation of properties, bindings and callbacks after low-level retirement.
Owned paths are host-API `mq_handles.rs`, provider `message_handle.rs`,
`pubsub.rs`, `pubsub/lifecycle.rs`, facade exports, focused shared-handle tests,
the existing shared-kernel ADR and a unique fragment. Row `0012` MQDISC and
`0010` MQCRTMH retain the same source pins and special-handle semantics.
Generated documentation is manager-owned. No provider route or licensed claim
is introduced by this repair.

A separate review repair `MQ-1504.cics-callback-scope` owns pub/sub callback
control/dispatch matching, focused task-isolation regressions and a unique
fragment. Special default connections must retain registry-defined CICS task
identity, rather than sharing control state because their symbolic Hconn values
are equal. This is private-kernel isolation, not acceptance of CICS MQOP_START.

### Integrated commits and review outcome

| Slice | Integrated feature commit | Outcome |
|---|---|---|
| `MQ-1501.shared-handle-kernel` | `acc3ad11` | One connection/slot authority for message properties and subscriptions. |
| J: `MQ-1501.mqi-request-boundary` | `8a6f4f9e` | All 26 typed call identities, distinct canonical request/result domains and exact issued-token identities; dispatch remains pending. |
| K: `MQ-1502.object-inquiry-kernel` | `6076f9bc` | Catalog-backed bounded inquiry; unreviewed selectors and MQSET remain pending. |
| L: `MQ-1505.delivery-checkpoint-kernel` | `298e1040` | Live resume retains pending work/cursors/final decisions; cold restart retains its separate policy. |
| `MQ-1501.handle-access-guard` | `50dd6a92` | Direct registry/kernel retirement cannot leave a dispatchable stale callback; live unassociated/in-use properties are preserved. |
| `MQ-1504.cics-callback-scope` | `532159c4` | Default-connection controls isolate host/process/task units; issued shared connections retain one control. |

The independent read-only CLI review found no actionable inquiry/checkpoint
defects and identified the inherited CICS default-Hconn scope leak. Three
baseline regressions reproduced that leak before repair. Manager review also
closed direct-accessor retirement cleanup. Callback-state observation now takes
an owner, validates the connection and reports stale/cross-owner errors instead
of silently presenting stopped state; scoped accessors return dereference
guards rather than naked mutable references. See ADR 0028 for compatibility.

Verification selected from this diff passed: 124 host-API unit and nine
integration tests after canonical/handle integration; the final MQ suite's 83
unit, ten CICS scope, four object-lifecycle, six object-service and eleven shared
handle tests (114 total); and 13 licensed-verifier mutant tests. Formatting,
changelog, docs freshness, MQI registry, licensed-contract structure,
effect-encoding and exact-path feature-seal checks passed. Dependency policy
passed with unchanged dependencies. Each sequence cleaned its checkout target;
receipts remain outside disposable targets and Git.

The broader architecture-fast attempt stopped at the missing pinned CICS
`SSJL4D_6.x/applications/designing/dfhp37p.html` in its configured cache; the
expected retained topic-path file is also absent. No architecture-fast pass is
claimed. Optional global module-budget verification found an unchanged batch
baseline of 7,404 production lines against its 7,402 ceiling; new production
modules meet the 1,200-line limit. Supplemental strict Clippy found unchanged
MQ warnings; a strict-Clippy pass is not claimed. Unavailable unrelated evidence
was not refreshed or repeatedly retried.

### Remaining parent acceptance

These feature commits are bounded prerequisites, not completed parent work
packages or release acceptance. Public MQI dispatch/ABI registration, trusted
context and SAF/audit, Memory/SQLite durable service wiring, shared participant
acceptance, full structure/options/status/selector mappings and CardDemo exact
execution remain required. Unsupported forms stay explicit rather than generic
success. Licensed MQ 9.4 verification remains
`pending-external-licensed-receipt` at **0/26** without the external receipt and
exact external pins. Harness structure and mutant tests are not licensed runs.
The next manager-owned step is trusted public dispatch and service composition
through existing provider-row/effect/UOW authorities, without a private journal.

## Active service-integration wave

The next three CLI lanes start from `070e45e8` and retain `gpt-6.1-sol`, high
effort, goal mode, bypass and fast mode off. They must not advertise the new
MQI surface before dependency identities, participant binding and integrated
service proofs pass. The existing public legacy service remains unchanged
until its queue authority and the rich delivery authority are reconciled.

| Lane / parent slice | Exclusive ownership | Source rows / required proof |
|---|---|---|
| M / `MQ-1501.mqi-effect-admission` | New provider `mqi_admission.rs` and child tests, private facade hook, unique fragment; reuse existing trusted host-context decoder via a minimal crate-private hook only. No HostRequest variant or provider registration. | `0001`, `0002`, `0007`, `0008`, `0009`, `0004`, `0005`, `0011`: invocation/run/principal/grant, trusted owner/context, mutation sequence/key, finite deadlines/live cancellation and exact forbidden syncpoint disposition before state access. Caller assertions cannot mint trusted identity. |
| N / `MQ-1505.delivery-provider-rows` | Delivery checkpoint/row codec child modules and minimal delivery hooks, focused Memory/SQLite backend tests, unique fragment. No legacy service edits or new dispatcher/journal. | `0001`, `0007`, `0015`, `0020`, `0021`: bounded per-queue/UOW/final-decision metadata projections, strict restore, atomic row CAS/delta failure, restart/backout and fencing. The existing provider-state store is the physical adapter; a private whole-manager blob is forbidden. |
| O / `MQ-1501.completion-reason-catalog` | Normative completion/reason catalog, focused generator/schema/verifier/tests and generated host status modules with minimal facade hook, unique fragment. No service/state/context edits. | All `0001`–`0026`, preserving callback-function status non-applicability and 27 source positions: exact source-reviewed call-specific completion/reason pairs, numeric/symbolic consistency, strict unknown rejection and deterministic generation. Source projection is not execution credit. |

The manager owns dependency-consumption audit, shared HostRequest/canonical
integration, service composition/migration, early participant acceptance,
documentation/ADR and public route advertisement. These lanes build the actual
admission, status and durable boundaries needed for that integration; they do
not replace the required 26-call final outcome with private-kernel completion.
Existing replay, audit, canonical effect and transaction authorities must be
reused. Parent work packages and all official execution/differential gates
remain in progress.

The manager additionally owns `MQ-1505.row-envelope-reuse`, a mechanical
crate-private visibility hook for the existing MQ `ObjectRow<T>` envelope and
encoder in `service.rs`, plus a unique fragment. No payload, schema, namespace,
decoder, state transition or public API changes. Lane N consumes that single
codec instead of copying it; the hook is integrated before its worker resumes.

The hook's verification exposed a fixture allocation race: two parallel SQLite
tests can observe the same nanosecond clock value within one process and collide
on their temporary directory. `MQ-1502.sqlite-fixture-isolation` owns only the
object-service integration fixture/helper and a unique fragment; atomic bounded
directory allocation must preserve existing paths rather than deleting them.
This repair changes no product semantics or licensed/official coverage.

Lane M may make the existing retention `origin_for` function and its minimal
returned origin view crate-private, without changing that parser or its rules.
This scoped read-only hook reuses exact nested/outer CICS attestation checks;
application MQI admission must not gain coordinator authority merely because
bindings are present. No participant capability is advertised by this helper.

The affected MQ service's size audit found 1,477 baseline production lines
against its registered 1,331 non-growing ceiling (1,480 after the visibility
hook). `MQ-1505.service-row-codec-module` therefore owns a mechanical extraction
of existing row persistence/projection helpers into one child module, preserving
their bytes, CAS behavior and service facade. It must bring the affected service
below its existing ceiling, without raising exemptions or changing schemas.
The same slice updates `tools/check_provider_rows.py` and focused guard mutants
to inspect the linked MQ row child as well as the service, preserving both
required atomic-write checks and whole-state-serialization rejection.

### Consumed dependency identities

The accepted COBOL execution integration is merged PR #6, merge
`c7a07a93d0980173338cb26b85224e65d26e945e`, accepted candidate
`ba0694b12409b190acfae62fc50c01b289b7cdbc`, tree
`a0d6d6334db66f3392db44c3bb2e82ce0c590e66`. The accepted RACF/SAF integration is
merged PR #4, merge `b4f8fc70d320c52576667a7887312c16f9b22e68`, accepted candidate
`aa6debaf4952f31c15d83668c7b210834caa847d`, tree
`6b5975fdc7920d1664ec28674b5f66a7d6bb6f72`. Both merged authorities are ancestors
of this continuation; each accepted candidate has the same tree as its merge.
The scoped approvals recorded in COBOL execution status (2026-09-02, licensed
0/153 pending) and RACF security status (2026-09-01, licensed 0/48 pending)
remain historical dependency dispositions, not an MQ licensed waiver.

On clean continuation candidate `23c6200144c7416ee05e091409dd058d9f31ddf3`, the
consumed contracts passed `cargo xtask cobol-exit --check`,
`cargo xtask racf-catalog --check`, and focused RACF/SAF local conformance:
48/48 for each recognized, validated, executed, conditioned and recovered gate.
The COBOL exit checks structural closure and owned malformed/limit/recovery and
prior-artifact compatibility; it is not a fresh licensed execution campaign.
Receipts retain their actual candidate outside Git, and the target was cleaned.

### Next shared effect boundary

The manager declares `MQ-1501.host-effect-contract`: additive typed MQI
`HostRequest`/`HostResult` framing through the existing canonical streaming
authority, exact mutation/effect occurrence binding and bounded validation.
It covers existing catalog rows `0001`–`0026` as a non-executable contract only,
preserving callback notification's non-application-call role and every existing
legacy golden byte. Owned paths are host request/MQ child records, canonical MQ
child/framing, facade, focused host contract/golden tests, the canonical contract
document and one unique fragment. Existing oversized request and canonical
modules must be reduced within their registered ceilings, not granted new
exemptions. No provider advertisement, accepted participant or official
execution credit is introduced. Public service composition remains separately
required with trusted identity, SAF/audit and durable backend proof.

### Integrated service-boundary prerequisites

| Slice | Integrated feature | Bounded outcome |
|---|---|---|
| `MQ-1502.sqlite-fixture-isolation` | `2ed0dca6` | Exclusive fixture allocation remains unique at one clock tick; existing directories are never removed to claim ownership. |
| `MQ-1505.row-envelope-reuse` | `aabc91fc` | One existing object-row codec is reusable within the crate, with unchanged bytes. |
| `MQ-1505.service-row-codec-module` | `23c62001` | Mechanical helper extraction reduces service production lines to 1,092, below its unchanged 1,331 ceiling; linked-row guard mutants preserve the atomic/serialization checks. |
| M: `MQ-1501.mqi-effect-admission` | `a979b828` | Borrowed invocation/effect identity, trusted-owner comparison and live controls precede service validation; origin decoding does not select a coordinator. |
| O: `MQ-1501.completion-reason-catalog` | `cfde0373` | All 26 calls/27 source positions retain 1,020 source-consistent pairs and ten pending declarations; callback notification has no ordinary return table. |
| N: `MQ-1505.delivery-provider-rows` | `157f84a9` | Queue/UOW/decision/cursor rows, finite metadata and exact catalog/generation/fence identities use one existing checkpoint validator and object-row codec. |

Verification on the actual integration identities is separate: `a979b828`
passed all 130 MQ tests (98 unit, ten CICS scope, four object lifecycle, seven
object service and eleven shared handle); `cfde0373` passed 127 host-API unit
and nine integration tests, tooling mutants and the compiled status schema;
`157f84a9` passed all 34 affected delivery tests, including real Memory/SQLite
atomic failure and reopen. Each feature's HEAD seal, formatting, changelog and
documentation freshness passed. Provider-row and canonical-effect guards
passed; unchanged dependency-policy results are retained rather than repeated.
Each sequence cleaned its intended Cargo target and kept receipts outside Git.

The status table reproduces the pinned offline return sections, not execution
outcomes or independently licensed observations. Conflicting decimal/hex,
missing numbers, conflicting symbol numbers and malformed source spelling
remain pending. Numeric completion-code mapping remains pending because these
call pages name completion classes without declaring their numeric values.
No publication body is committed, no network refresh occurred, and no official
row or licensed numerator is increased by these contract/kernel proofs.

### Next isolated service-composition lanes

`MQ-1501.host-effect-contract` is integrated as `7ed1ed79`, re-sealed after
reconciling the reviewed-status result arm. The integration passed 142 host-API
unit and nine integration tests, documentation/changelog/formatting and the
canonical-effect guard. Original-effect extraction is immutable; all typed
occurrences retain exact replay identity. Request/canonical parents now meet
their unchanged line ceilings. No public handler or licensed credit is added.

The retained M lane next owns `MQ-1501.mqi-original-effect-binding`: the private
admission module/children, focused tests, narrow canonical contract prose and
one fragment. Replace metadata-only projections and independently supplied
envelope/mutation inputs with the actual validated typed host occurrence; bind
the full canonical host request identity and result call/limits/capacities to
that original borrow. Trusted lifecycle owner minting, SAF, durable dispatch,
returned-handle authority and public registration remain manager-owned. No
standalone MQI digest may replace the shared journal identity.

The manager owns `MQ-1501.trusted-lifecycle-directory`: a new private volatile
host lifecycle directory and tests, one facade hook, the handle registry's
process-termination primitive, ADR 0030 and one fragment. Opaque host-minted
process/frame leases map already-admitted invocations to non-reused numeric
owners; application envelopes cannot select them. Explicit CICS child admission
preserves the parent task, IMS syncpoint retirement advances its owner epoch,
and process termination retires shared handles. This is not host attestation,
durable UOW identity, a SAF permit, public dispatch or a new lifecycle journal.

After its sealed quiescent import, lane N next owns
`MQ-1505.rich-service-state-reader`: a private strict v1/v2 stored-authority
union and reader child, minimal service/row-codec hooks, affected Memory/SQLite
tests, narrow provider-row documentation and one fragment. A single bounded
physical MQ-prefix snapshot must select either legacy or rich authority, never
merge competing queues or use missing/corrupt state as an empty fallback.
Retained replay rows keep their existing bytes/versions and retention authority.
Public selection, runtime/UOW owner maps and audit/effect composition stay with
the manager. Normalized legacy reads must reuse the existing row authority;
rich decoding reuses the frozen delivery validator, not another runtime codec.

The manager-owned lifecycle directory is sealed as `de16ca03`: nine provider
lifecycle regressions and eight affected host registry tests passed, with
unchanged module ceilings, canonical guard, formatting/changelog and repaired
ADR navigation/docs freshness. The mandatory docs check initially identified
the absent navigation entry; that exact registration was repaired before seal.
No public caller, durable UOW or licensed evidence is supplied by the directory.

Lane N's quiescent import is sealed at `1c6a8fc6` before manager integration.
Its 77 focused Memory/SQLite service/delivery/object-service tests and four row
guard mutants passed. All legacy logical publications now advance the small
manifest CAS dependency, preventing either migration/writer race ordering from
publishing stale state. The import preserves exact legacy replay bytes/versions;
non-quiescent migration, actual v2 service selection and audit/effect composition
remain required. Disjoint stale legacy writers conservatively conflict; no
whole-state blob or automatic mutation redispatch is introduced.

The manager delegates the declared `MQ-1501.host-effect-contract` to retained
lane M. Lanes N and O receive the following disjoint next slices, keeping three
CLI workers plus manager, `gpt-6.1-sol`, high effort, goal mode/bypass and fast off.

`MQ-1505.audited-provider-publication` (lane O) owns a minimal additive shared
store boundary, Memory/SQLite implementations and focused tests, linked child
modules/extraction hooks, ADR 0029, `DURABLE-STORAGE-PROFILE.md` and one fragment.
Lane N exclusively owns the narrow `PROVIDER-ROW-PERSISTENCE-V1.md` edits.
The publication boundary must assert the exact existing live canonical
coordinator intent inside the same physical transaction as provider object,
UOW/replay-result rows and their typed audit. It must use existing audit/effect
codecs and touched-row rollback, never a private MQ audit/journal or whole-store
clone. Core result/lifecycle/outbox completion remains coordinator-owned. All
CAS, stale/recovered intent, audit capacity/identity and payload/row failures
must roll back the whole publication. PostgreSQL execution remains pending if
no disposable environment is configured; no new participant is accepted here.
This is infrastructure for the same source-bound MQ mutations, not new IBM
language semantics or a reason to review unrelated publications.

`MQ-1505.legacy-delivery-import` (lane N) owns a private linked legacy import
planner, minimal service hook and manifest-CAS dependency, focused Memory/SQLite
tests, narrow row-compatibility documentation and one fragment. Source rows
`0001`, `0007`, `0015`, `0020`, `0021` retain their pinned MQ baseline. The first
bounded import accepts only validated quiescent legacy state; live handles/UOWs
are rejected unchanged, never discarded or implicitly backed out. It must
preserve exact queue order/body/IDs, catalog/trigger identity and retained replay
bytes/versions/references through the existing delivery/checkpoint authority.
The composable batch CAS-fences the legacy manifest/catalog/queue identities,
initializes the existing rich row family and retires the legacy queue authority
with an explicit versioned marker. Every old writer must CAS the small manifest
dependency so a concurrent new pending-row insertion cannot evade migration's
fence. Both race orderings must have one winner with no partial state/audit.
This plan is not automatically applied during open and does not advertise a
public upgrade before the manager supplies the v2 reader and single service
selection. Non-quiescent conversion remains explicit pending work. The manager
retains status/ADR coordination, trusted owner/UOW minting, service selection,
SAF/audit composition, participant acceptance and public route proof.

### Integrated host and storage composition boundaries

| Slice | Integrated feature | Exact bounded proof |
|---|---|---|
| `MQ-1501.host-effect-contract` | `7ed1ed79` | 142 host unit plus nine integration tests; full typed host framing, immutable occurrence extraction and original sequence/key. Reviewed-status result arm reconciled and re-sealed. |
| `MQ-1501.trusted-lifecycle-directory` | `de16ca03` | Nine private lifecycle plus eight host registry tests; non-reused leases/owners, explicit CICS inheritance, IMS epoch and exact process retirement. No host attestation or durable UOW owner is supplied. |
| `MQ-1505.legacy-delivery-import` | `cb7d9eb5` | 36 service plus seven object-service tests and four row-guard mutants on integration; worker's separate 34 delivery regressions retain their original candidate. Small manifest CAS fences every legacy publication. |
| `MQ-1505.audited-provider-publication` | `8e8d02da` | 17 publication, five affected Memory/SQLite mutation and eight store-API checks on integration; exact retained live canonical intent and running execution fence rows plus typed audit in one physical transaction. |
| `MQ-1501.mqi-original-effect-binding` | `de24f3cc` | 30 admission/result regressions on integration; one original host occurrence, full shared canonical request/result identity and original call/limits/copied-capacity checks. |

All feature HEAD seals and required affected formatting, changelog and docs
freshness checks passed. Canonical/provider-row guards passed where applicable.
The manager reconciled the ADR 0029/0030 registry/navigation overlap and
regenerated derived documentation; no worker prose or source pin was discarded.
Every sequence cleaned its own checkout target, preserving external receipts.
No dependency input changed; prior policy passes are explicitly reused.

Audited publication deliberately does not finalize core results/outbox or
deduplicate disjoint batches under one unchanged intent. The actual service
must supply its CAS-protected UOW/replay dependency and remove sequential old
writers before atomic claims. PostgreSQL publication remains default-unsupported
and has no execution credit. Result preflight alone cannot validate returned
handles/UOW state or erase uncertainty after dispatch; the actual service must
map uncertainty through the existing shared effect authority. Missing additional
property/conversion required-length output forms remain pending, not invented.

### Single-snapshot reader and independent composition review

The manager integrates lane N's sealed `311e0511` reader without changing its
production bytes. It captures one bounded physical `mq-` snapshot and returns
exactly one legacy or rich authority. Public legacy opening is unchanged; no
automatic migration or public MQI selection occurs. Rich identity must match
both the import marker and delivery metadata, with historical source versions
remaining lower bounds. Replay records retain exact bytes/versions and existing
retention validation; numeric replay handles are not issued opaque tokens.
Separate 64 MiB legacy/replay and rich-row budgets plus a 128 MiB aggregate
accept valid imported combined footprints, including an exact-ceiling source
whose replacement marker is larger. The manager's 49 service regressions pass;
affected object-service, row guards, formatting, docs/changelog and exact seal
receipts accompany the integrated candidate rather than relabeling worker logs.

Lane M's read-only review found no actionable defect in the other authors'
lifecycle/import/audited-publication boundaries through clean `ef02809d`.
Incoming reader integration was explicitly excluded. It ran no new diagnostics
or tests and did not independently approve its own original-effect feature.
The external report binds exact path hashes and remaining service/participant
proof obligations. Root's restart clarification records that process-local
directory counters require a separate durably retained registry epoch advance.

Lane O completed a bounded offline inventory of 80 retained supplemental topic
candidates, all matching metadata hashes/byte counts and pinned TOC headings.
There are 31 priority prerequisites, 40 additional structure/constant topics,
and nine reason topics covering ten unchanged pending declarations. All remain
unreviewed for semantics; no independent browser-reproduction or freshness claim
is made. This inventory changes neither official source pins nor execution credit.

The current three retained CLI lanes use `gpt-6.1-sol`, high effort, fast off,
goal mode/bypass and no nested workers or orchestration skill:

- N owns `MQ-1505.rich-state-publication-fence`: the same rich authority's bounded
  ordinary delta and explicit checked persisted fence plan, exact marker/meta/
  catalog CAS, no automatic recovery decision or adoption before full commit.
- O owns `MQ-1501.supplemental-source-pins`: a separate zero-credit 0.15 manifest
  and shared-reader registry extension for the exact 80 inventory topics. The
  immutable 0.2 call baseline and pending semantic states remain unchanged.
- M owns `MQ-1505.original-effect-core-intent-binding`: original service admission
  bound to a real retained coordinator intent and the same physical PlatformStore
  audited transaction. No fabricated intent, private journal, SAF permission,
  UOW minting, public selection or core completion is supplied by this boundary.

O sealed source registration at `86d2b18d` (80 topics, manifest SHA-256
`7960f3118465521a55c541af376c100001feab5d086ec2a0ebe482339d7d7d8a`);
the manager integrates the same source/reader/registry bytes after selected
service commit `830f0164`. Fresh integration runs cover the 27 reader and ten
xtask topic-manifest tests, plus exact offline selected-scope reproduction and
formatting/docs/changelog/seal gates. Worker receipts remain separately bound.
Registration grants no semantic or execution credit. O now owns
`MQ-1501.completion-wire-mapping`: explicit semantic
review of the three MQCC numeric values from supplemental topic `q090560_`,
baseline `ibm-mq-9.4-programming-supplements-2026-09-12`, supporting original
call rows `0001`–`0026`. It extends the existing normative status catalog/schema,
generator/verifier and typed status API, with narrow status tooling/tests/docs.
Existing symbolic canonical bytes, call-page reason provenance, all ten pending
reason declarations, callback notification role and 26/27 denominator remain
unchanged. Source reproduction, negative mutation/schema checks, admitted and
pending pair conversions plus canonical goldens are required before sealing.
This mapping does not calculate outcomes or advertise handlers/ABI readiness.

N exclusively owns the rich-state/delivery-row helpers and provider-row contract;
O owns supplemental manifests/shared reader/xtask registry and cache runbook;
M owns the new private intent-binding module and narrow effect-replay note.
Minimal facade hooks and generated-doc overlaps are reconciled by the manager.

The manager's next `MQ-1505.selected-service-authority` slice replaces the
service's legacy-only mutex with one discriminated legacy/rich authority and
adds a private strict selected opener. It derives provider/core views from the
same PlatformStore Arc, requires an authorizer and trusted clock, and blocks
legacy sequential service operations on a selected instance. Existing public
legacy constructors/bytes/migration stay unchanged. Rows `0001`, `0007`, `0008`,
`0009`, `0015`, `0020`, `0021` retain their pinned context/lifecycle/delivery
authorities; the state selection adds no IBM status or wire constants. Memory
and SQLite strict v1/v2 selection, physical reopen, corruption/mixed-state and
wrong-fence refusal without writes, mandatory policy dependencies, same-store
views, rejected legacy bypasses and affected legacy regressions are required.
The manager owns service.rs, new service_selection.rs/tests, ADR 0031 and its
registry/navigation, one fragment and freshness. Other lanes must not edit this
union. No automatic conversion, result/replay publication, UOW owner, host
attestation, accepted participant or public MQI readiness is claimed by selection.

This selected-authority implementation passed 55 service regressions (six new
selection tests) and seven affected object-service integration tests on the
manager candidate. Memory and SQLite exercise v1/v2, populated/empty queues,
exact records/replay preservation and physical reopen. Selected legacy calls and
legacy provider registration are blocked; old constructors/registrations retain
their previous behavior. Initial empty-install test input was invalid and hit
the existing capacity guard; the repaired test uses a valid definition and proves
the selected route refusal. Row guard/four mutants, formatting, docs/changelog
and affected module ceilings (service 1120/1331, selection 115/1200) passed.
No dependencies, schema or canonical bytes changed. The strict opener stores one
authority and grants no connection, UOW, mutation, recovery or SAF permission.

The manager integrates N's sealed `720d0b21` bounded publication/fence plans
against this selected service union. Marker, metadata and catalog CAS are one
composed audited batch; fence-only changes preserve live member and replay bytes.
The ordinary/fence race has one winner in both orders. Fresh integration covers
the affected service (including selection), delivery and object-service tests,
row guard/mutants and required freshness/seal gates. Audited-store production
bytes are unchanged; its 17 worker regressions retain their separate candidate.
The plan is not a UOW decision, recovery permit or automatic adoption authority.

N next owns `MQ-1505.typed-result-replay-codec`: a private bounded strict storage
projection for actual typed non-handle MQI outputs and their full shared canonical
result identity. It preserves pending/unknown/duplicate outcomes and refuses
opaque handle outputs without live/historical authority. No public token
reconstruction, new journal/pruner, reader namespace allowance or readiness is
introduced. Minimal pure delivery-codec reuse must retain existing cold/live
bytes. Actual receipt/CAS/core-reference/retention and historical handle replay
remain manager obligations, not omitted v0.15 scope.

The manager integrates M's final sealed `af2e56bc` original-effect/core-intent
binding. It recomputes digest, capability and origin from the immutable original,
observes the real retained intent/execution on the same borrowed PlatformStore,
and retains monotonic observation time even after an expired boundary. Exact
audit identity and bounded MQ-owned mutations pass to the existing audited
transaction; its core completion/outbox remains coordinator-owned. Fifteen
final worker boundary tests and 30 admission regressions retain their original
candidate; fresh manager integration runs those affected tests and exact gates.
Earlier unpublished 27698b56 was superseded by the summary-substitution repair.
Nested CICS composition, actual SAF/queue/UOW validation and receipt deduplication
remain pending. No fabricated intent or replacement store can enter the binding.

O's sealed `6f28c25a` completion wire mapping is integrated separately. The
supplemental `q090560_` table binds MQCC_OK/WARNING/FAILED to 0/1/2; UNKNOWN -1
is not an ordinary call return. The original call catalog is reconstructed
exactly so its canonical status digest and all golden bytes stay unchanged.
Fourteen tooling tests, the host API suite and two schema/freshness checks are
fresh integration gates; selected source reproduction uses only the reviewed
MQCC/context fragments. The worker's independent full call/source reproduction
retains its actual candidate. All ten reason declarations remain pending.

O next performs read-only independent selected/publication/intent composition
review, excluding its own status/pin work. It binds committed path identities and
locates actual production host/coordinator call sites for the executable critical
path; no unchanged suite is rerun merely for a review. The next implementation
units must compose the existing service rather than mint parallel queue authority.

The manager owns actual selected service/ABI/host admission, separately durable
UOW ownership, SAF/security context and audit/replay composition, participant
minimum acceptance and public capability proof. All owned 26-call gates and
CardDemo remain required. Licensed differentials are explicitly user-skipped
for this continuation, with zero licensed credit. The clean `eb9483bc` licensed gate
reported missing external receipt/pins and pending 0/26; that older candidate
receipt is not relabeled as evidence for these newer commits. No release or
parent work package is complete.

## Selected-flow composition wave

All lanes start from the committed `adc911f0` integration candidate, except N's
already active non-handle codec on its recorded `576e236f` plus rich-publication
prerequisite. Direct CLI workers use `gpt-6.1-sol`, high effort, default service
tier and fast mode off. At most three workers run beside the manager; no nested
workers or orchestration skill are used. The licensed-only user exception above
applies to every lane.

| Slice / owner | Scope and contract ownership | Required focused acceptance |
|---|---|---|
| `MQ-1505.selected-operation-publication` / M | Actual private selected service flow and independently durable UOW/operation ownership under the existing sole mutex; reuse catalog, registry, lifecycle, delivery, original core intent and audited store publication. Own new service operation/UOW/receipt modules and narrow selected reader/authority integration, not host result encoding or machine/server routing. | Source-reviewed ordinary batch CONNECT/OPEN/PUT/PUT1/GET/CMIT/BACK/CLOSE/DISC first-flow composition, actual HCONN and resolved SAF checks, Memory/SQLite operation collision/replay and late CAS/audit/quota rollback, pending-work preservation and physical reopen; trusted host and handle-replay dependencies remain explicit until integrated. |
| `MQ-1505.lossless-reviewed-output` / O | Additive reviewed completion plus exact typed output in the existing host result contract and original-request preflight. Preserve every old encoding and status inventory. Own host result definition/validation/encoding and narrow admission-result preflight, not the service or N's storage codec. | Exact warning/failure GET descriptor/buffer/required-length observations, request/call/status/output coherence, copied-capacity bounds, pending/unknown separation, old canonical goldens and focused host/admission regressions. |
| `MQ-1505.typed-result-replay-codec` / N | Previously declared non-handle strict codec; refuse reconstruction of opaque authority. No concurrent result enum changes are consumed from another worker's dirty checkout. | Exact full-host canonical identity and storage round trip; malformed/duplicate/missing/unknown fields and bounds; existing delivery snapshot bytes unchanged. The manager must extend the codec deliberately for O's additive result after both features seal. |

These units compose the first real selected flow; they do not reduce the final
26-call denominator. The manager owns the actual trusted program host/ABI route
and shared handoff integration. Production `ProductServer` still opens the legacy
service, and selected legacy provider registration is deliberately empty. Do not
switch it merely because these private primitives compile. The host must issue
context/lifecycle provenance before constructing the original typed effect;
equal application bindings are not that provenance. PUT1 still requires a live
HCONN. Durable UOW ownership is not a caller's integer or volatile lease ID.

The read-only composition review bound exact selected/rich/core commits and found
no actionable defect within their stated primitive guarantees. It did not execute
a combined public route. The next composed route must prove one audited physical
publication, adoption after success, coordinator-owned completion, exact Completed
receipt replay and fenced Unknown resolution. Warning/error output, opaque handle
replay, nested CICS/IMS ownership and all remaining applicable contexts are not
waived by a successful first ordinary batch flow. Public readiness, participant
acceptance and full v0.15 completion remain unclaimed.

The manager's `MQ-1501.typed-machine-connection-route` owns the first original
typed machine effects for source rows `0008` MQCONN and `0012` MQDISC and an
explicit installed-batch host-admission hook. A trusted configured factory, not
application binding bytes, supplies frame ownership before effect construction.
The ABI map translates only already-issued opaque connection tokens and never
mints registry authority. Required checks cover exact original canonical effect,
signature/storage/writeback validation, foreign/stale wire numbers, changed
frame identity, uncertainty and old-route compatibility. Unsupported contexts
and durable checkpoint/handle replay remain pending; the old snapshot schema
must not silently lose this new volatile mapping. This prepares the actual host
route without switching ProductServer registration or claiming MQ execution.

This bounded handoff is implemented with nine focused interpreter regressions
and five installed-batch admission regressions passing on the current candidate,
with no failures or ignored tests. The compiled installed-program test uses real
Memory/SQLite core journals and checks retained original Intent identity before
test-provider dispatch; its fake MQ provider is not owned service/SAF evidence.
Explicit BY REFERENCE is accepted; BY VALUE/CONTENT and other known unimplemented
MQI signatures fail before effect construction. Unusable post-dispatch replies
remain UnknownOutcome without application writeback. Legacy MQ method bytes are
unchanged apart from child-module visibility, and the changed interpreter stays
below its existing 11,940-production-line ceiling. Canonical/provider-row guards,
four provider-row guard mutants, formatting, documentation and changelog checks
pass. The previously diagnosed unrelated batch-service module ratchet violation
(7,404 versus 7,402) is not waived or represented as a passing global module gate.
Dependency policy reuses the d8a026ab receipt only after verifying its receipt
hash and all 32 unchanged manifest/toolchain/policy input identities.

## Strict typed-result storage integration

`MQ-1505.typed-result-replay-codec` is integrated from sealed worker `0c6426e1`.
It preserves complete non-handle typed results, descriptors, buffers, exact
required/copied lengths, properties and outcome distinctions using the existing
message projection. Storage binds the full shared HostResult digest and original
MQI limits; duplicate, missing, unknown, malformed and over-budget input fails
closed before typed allocation. Callback notification storage cannot admit a
public callback effect. Opaque handle outputs remain explicitly unsupported in
this slice; no registry token is deserialized or reconstructed. Old delivery
restart/checkpoint bytes and default policies remain unchanged.

On this composed manager candidate, 15 codec, 34 delivery and five installed-batch
handoff regressions pass, with no failures or ignored tests. The worker's other
host-contract receipts remain separately bound to its original candidate.
Dependency policy is reused only under the same verified 32-input identity
proof described above. No provider receipt row, service mutation, journal,
public route or full-v0.15 acceptance is implied by this pure storage feature.

The next declared isolated CLI slices are `MQ-1505.historical-handle-result-replay`
(N: strict historical identity plus all-entry registry rejection and checked
lookup of existing live entries, existing codec and ADR0033) and
`MQ-1501.point-to-point-wire-options` (O: source-pinned numeric option/version
projection inside the existing normative catalog and checked constructors to
existing typed requests). Neither owns M's selected service/UOW publication or
the manager's machine/server route. Historical reconstruction cannot mint live
authority; caller option bits cannot mint UOW/SAF or erase unsupported modes.
Only sealed commits will be consumed, with generated/facade overlaps reconciled
and final composed checks retained under their actual candidate identity.

## Historical handle storage integration

`MQ-1505.historical-handle-result-replay` is integrated from sealed `8023d2cb`.
Issued CONNECT/OPEN/dynamic/message/subscription outputs preserve their original
canonical identity and metadata as historical observations. All registry entry
and mutation paths refuse historical tokens before lookup, including coincident
live slot identities. Checked resolution is read-only lookup of an exact existing
owner/role/connection/epoch entry, never allocation or resurrection. The machine
also refuses historical CONNECT output before any live ABI alias or writeback.
The caller must independently attest retained receipt/core occurrence and current
host frame before resolving; observation or canonical equality is not permission.
Historical symbolic Default/Unassociated results remain explicitly unsupported.

The composed candidate passes 13 handle, 21 codec, 11 shared-handle, ten CICS
callback-scope, ten typed-machine and five installed-batch tests (70 total, zero
failures/ignored). Worker89-test receipts retain their own candidate identity.
Documentation conflicts preserve both manager ADR0032 and worker ADR0033 plus
accepted Db2 decision paths; normal generation reconciles their manifest. No
source refresh or licensed execution is performed. Reviewed baseline/catalog
rows are `0008/0009/0010/0012/0019/0025` under
`ibm-mq-9.4-mqi-2026-08-31`, with exact pins in ADR0033 and the worker handoff.
Durable cold registry incarnation, actual receipt authority and selected route
acceptance remain required; this identity-only feature supplies none of them.

N's next isolated section is a read-only installed-batch trusted-producer design
review against sealed `1da5490c`. It owns external design/identity receipts only,
not M's service/UOW publication, O's numeric options or the manager's production
host bridge. Actual parentSome topology, real artifact provenance, same-store
admission, cancellation/probe, frame cleanup and durable recovery must compose;
the review cannot turn binding parsing or a test provider into attestation.

## Lossless reviewed output integration

`MQ-1505.lossless-reviewed-output` integrates sealed worker `4b7f5582` and
deliberately extends the existing strict codec, not a second result codec or
status inventory. An additive canonical tag preserves reviewed completion/reason
plus the exact output. GET OK/NONE, both reviewed truncation warnings and
no-message/wait-expiry failure observations retain descriptors, copied bytes,
required lengths and properties. Original request/mode/capacity must still pass
provider preflight. Unrepresented conversion/property/per-destination and other
status forms stay pending; the all-26 scope is not reduced.

The composed candidate passes 155 host unit plus nine integration, 33 original
admission/result, 23 codec, 11 typed-machine and five installed-batch tests
(236 total, no failures/ignored). New codec regressions cover required fields,
extra data, corrupted and coherent-invalid digests, warning/failure payloads,
SQL cursor bounds and historical/special connection handling. The machine may
copy validated reviewed OK/NONE connection/disconnection outputs, but cannot
turn a historical token into a live alias. Full original result tags/digests
remain unchanged in the core journal; local writeback is not receipt rewriting.
Old canonical goldens and old storage bytes are preserved.

Source review uses original baseline rows `0015/0020/0021` and supplemental
MQGMO `SSFKSJ_9.4.0/refdev/q096715_.html` (pinned programming-supplements baseline),
with exact four pins in the worker's source receipt and reviewed architecture.
All offline source credit remains zero. Actual selected-service publication,
host attestation, cold incarnation/checkpoint/retention, participant and CardDemo
acceptance remain required; only the licensed oracle is human-skipped.

## Trusted producer review and next guard repairs

N's read-only `MQ-1501.installed-batch-host-producer-design` is complete against
sealed `1da5490c`, with external identity/source/design receipts and no new
execution claims. It found two concrete safety gaps: factory installation can
race runtime publication, and an exported typed-source MachineSnapshot can
restore into a fresh unbound destination while silently dropping admission and
aliases. Existing same-instance refusal and coordinator checkpoint=None do not
cover that direct cross-instance API. Both findings are repaired by the bounded
state-guard integration below; real producer and typed recovery remain required.

The next isolated N feature is `MQ-1501.typed-frame-state-guards`, based on sealed
manager `3f758db2`. It owns narrow server setup serialization/typed control freeze
and interpreter source-snapshot refusal plus focused legacy compatibility tests.
It must preserve existing legacy behavior and bytes, cannot invent an accepted
typed checkpoint schema or serialize executable handles, and does not own M's
selected publication/UOW/receipt or O's numeric options. The manager retains
actual admitted parent/artifact/original-call proof, same-store service bundle,
processing-unit topology, explicit frame-session cleanup and public integration.

M now has the sealed historical and reviewed-output prerequisites and manager's
bounded existing-codec compatibility delta. Only committed sources are consumed;
its earlier service receipts keep their actual pre-integration identity. Fresh
composed service publication/replay checks are required before sealing that lane.
All 26-call, security, persistence, recovery, participant and CardDemo gates stay
active; the sole licensed oracle skip remains zero-credit.

## Checked point-to-point numeric options

`MQ-1501.point-to-point-wire-options` integrates sealed worker `736e5cda`.
Checked signed MQLONG options and reviewed MQOD1/GMO1/PMO1 plus ungrouped MQMD1/2
construct existing OPEN/CLOSE/GET/PUT/PUT1 requests without granting registry,
SAF, cursor or UOW authority. Unknown bits and illegal combinations fail;
recognized unrepresented versions, context, property and asynchronous modes
remain pending. Trusted bindings must attest queue defaults even for CLOSE zero,
supply independently admitted UOW/cursor state and explicitly convert finite
milliseconds to clock ticks. GET defaults follow the queue-manager platform.
PUT1 retains its actual opaque HCONN. This pure adapter is not service execution.

The existing normative structure/status catalog adds a private version-two
wire-options projection (102 facts, ten corroborating locators), retaining the
exact reconstructed version-one hash and all original call/status canonical
identities. Reviewed sources are original rows `0006/0015/0019/0020/0021` and
the pinned programming-supplements baseline's MQOO/MQCO/MQGMO/MQPMO, MQOD/MQMD
and field-detail topics; exact pins are recorded in the architecture and worker
handoff. Missing unpinned MQOD/MQMO constant tables are not guessed or refreshed.
Raw structure layouts, further descriptor fields and all-26 option coverage
remain required.

The composed candidate passes 114 Rust MQ host tests and 33 tooling tests with
no failures/ignored. The manager discovered its shell previously selected
system Python 3.9.6; fresh tooling, source reproduction, canonical/provider-row
guards, four row tests, schemas and registry checks now run with the pinned
Python 3.12.13. Documentation generation/checks and changelog validation use the
same corrected PATH. Older Python receipts retain their actual interpreter and
candidate identities; they are not relabeled. Rust 1.98 results are unchanged.
Dependency policy reuse verifies the original receipt and all 32 unchanged
inputs. The unrelated batch module overage remains unwaived. Only the licensed
oracle is skipped with zero credit; full v0.15 acceptance remains incomplete.

## Typed frame setup and source-snapshot guards

`MQ-1501.typed-frame-state-guards` integrates sealed worker `7f7f9b04`.
A common setup mutex serializes factory/control/runtime publication, prechecks
all runtime fields and freezes typed setup. It preserves the legacy first
control installation after runtime construction. External factory callbacks run
after the mutex is released. A typed source's direct snapshot exports diagnostic
schema zero, rejected before restore can mutate any destination, including a
fresh unbound machine and the manual binary projection. No accepted typed
checkpoint schema or executable-token serialization is introduced. Legacy
schema 12 and checkpoint codec bytes are unchanged.

The composed candidate passes 13 typed-machine, two legacy checkpoint, ten
server admission/setup and four legacy control tests (29 total, no failures or
ignored tests). Fresh guards, four provider-row tests, formatting, docs and
changelog checks use pinned Python/Rust. Original baseline rows `0008/0012`
remain the reviewed source boundary; source review gives zero execution credit.
Worker receipts retain their original candidate identity. The actual admitted
parent/artifact/call proof, same-service host bridge, lifecycle disposition,
selected SAF/UOW/replay composition and full typed recovery remain required.

## Private selected operation and durable-owner composition

`MQ-1505.selected-operation-publication` integrates sealed worker `55de3f87`.
Ordinary batch CONNECT/OPEN/PUT/PUT1/GET/CMIT/BACK/CLOSE/DISC compose under the
existing sole service mutex and same physical PlatformStore. Original core
intent/running execution, live opaque frame, resolved SAF resources, durable
current-owner/control CAS, delivery/catalog/marker state, insert-only exact
occurrence receipt and typed audit publish atomically. Candidate adoption follows
the entire transaction; late CAS/audit/quota failure preserves pending work.
Unknown publication/reply fences the runtime. Core completion/outbox remains
coordinator-owned, and shared transactions remain explicitly unsupported here.

Durable UOW IDs are allocated from bounded retained control, not caller integers
or volatile directory leases. Every empty/final owner is retained; the reader
checks the complete finite allocated prefix. Cold activation advances durable
incarnation before exposing any new connection. Replay rechecks the exact
physical control record, original receipt/core identity, current frame and SAF
before resolving an exact existing live entry; it never resurrects a handle.
The repaired stale-runtime regression proves a newer incarnation fences old
cached connection replay on both Memory and SQLite.

NoWait GET preserves source-reviewed OK/NONE, warning 2079/2080 and failed 2033
outputs through the existing lossless codec. True wait scheduling, generated PUT
descriptor fields, broader options/versions and remaining calls stay pending.
The composed candidate passes 25 selected-operation, 21 rich-state, six selection,
33 admission, 23 codec and 15 core-binding regressions (123 total, no failures or
ignored tests). Worker prior 277/52 receipts retain their own source candidates.
Reviewed original rows are `0001/0006/0007/0008/0009/0012/0015/0019/0020/0021`,
plus pinned supplemental MQGMO field details; source review earns zero credit.

The private root-only host entry still rejects the actual installed parentSome
child. A real admitted parent/artifact/call proof and same-service topology/session
bridge must compose before public registration. Pending owners survive cold
restart without reassignment; no retention deletion or participant protocol is
invented. Full security/recovery/participant/CardDemo/all-26 acceptance remains
required, with only the licensed oracle human-skipped at zero credit.

## Inherited module-ratchet repair

The manager extracted three unchanged batch terminal-problem projection helpers
into a bounded child module. Their production bodies are byte-identical apart
from child visibility; no IBM semantic behavior changes. Batch service production
size falls from 7,404 to 7,360, and its exact reviewed inventory ratchets downward
from 7,402 to 7,360 rather than raising the ceiling. The child has 51 production
lines. Earlier overage findings retain their actual pre-repair candidates.

A bounded inventory comparison also exposed other existing mismatches: product
assembly, package-v2, IMS service and CardDemo conformance exceed their recorded
counts; several previously extracted modules need lower-count inventory refresh.
Those untouched inputs are not waived, and this batch repair is not a passing
global module gate or full-release acceptance. They require distinct scoped
repairs with verification before the final v0.15 candidate can pass that gate.
