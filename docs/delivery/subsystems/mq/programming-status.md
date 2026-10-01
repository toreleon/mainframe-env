# IBM MQ — MQI programming surface progress

Subsystem: **mq**
Phase: **programming**
Target release: **0.15.0**

Status: **Implementation active**

- Completion branch: `codex/v015-completion`
- Completion-wave base: `2f5191be0ddd6a5aae32ce5f92aa94846b2f2c37`

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
