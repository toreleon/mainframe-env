# IBM MQ — MQI programming surface progress

Subsystem: **mq**
Phase: **programming**
Target release: **0.15.0**

Status: **Implementation active**

- Completion branch: `codex/mq-v015-continuation`
- Continuation-wave base: `213ed878` (current main after PR #381 merged)

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
