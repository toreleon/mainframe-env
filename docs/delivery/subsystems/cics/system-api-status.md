# CICS — SPI and FEPI progress

Subsystem: **cics**
Phase: **system-api**

Status: **Source-backed private preparation implemented; public SPI/FEPI execution and licensed acceptance pending**

## Current scope

The official denominator is 269 SPI and 39 FEPI command identities. The private,
non-routing grammar projection covers 266 SPI and 39 FEPI identities, 5,082
operands, 8,947 case candidates and 62 numeric domains/217 records across 18
family contracts. Source/projection counts earn no runtime credit: 0/269 SPI,
0/39 FEPI accepted. SPI-1001 and cics.system-api remain incomplete.

The application dependency remains incomplete. CICSMESSAGE, GETNEXT TIMER and
ISSUE COPY stay recorded as deferred/unready, including CICSMESSAGE internal
execution obligations. Public SPI/FEPI routes are not admitted from preparation.

## Implemented private preparation

- Pinned SPI and FEPI command-topic maps, command-body manifests, source form
  locators, family grammar/constraints, operand extents and numeric CVDA domains.
- Existing shared logical CICS/COBOL program frame, selected-call provenance,
  scoped storage/member reservation and terminal checkpoint prerequisites.
- Private named-PROGRAM status observation, configured command/resource security
  checks and SQLite reopen fixture preparation; source catalog row 0155.
- Scoped READ/REWRITE/SYNCPOINT application contract consumption and canonical
  conformance ledger export. This proof is limited to three application rows.

These extend existing authorities; no shadow dispatcher/resource store or generic
success handler is introduced. Deployed SAF policy, trusted issuer/namespace,
typed output receiver and propagation of current cancellation/deadlines remain
prerequisites. Direct helper or component tests do not close public acceptance.

## Source authority

Official catalog: ibm-cics-ts-6x-2026-08-31, SPI unique rows/FEPI identities.
Body baselines: ibm-cics-ts-6x-spi-command-bodies-2026-09-12 and
ibm-cics-ts-6x-fepi-command-bodies-2026-09-12. Retained matching publication
bodies remain external; committed manifests carry only locators/hashes.

PROGRAM row spi-commands-unique:0155 uses
SSJL4D_6.x/reference-system-programming/commands-spi/dfha8_inquireprogram.html,
SHA-256 e3d8ed4c069bd26822c6373b35278ec126591e2efaf020b8fbcf8b811845f20f.
Security reference uses baseline
ibm-cics-ts-6x-application-api-sources-b-2026-09-10 and topic
SSJL4D_6.x/reference-security/command-security-resource-reference.html,
SHA-256 57539dc40fa06aa78da3b435a22c1955fa750d30a47f865341d9bc6e78417c8d,
PROGRAM rows 306–310. Deployment classes/access/prefix policy remain configured.

Three row/body joins remain unresolved: SPI0201 PERFORM SECURITY, SPI0203
PERFORM SSL and SPI0204 PERFORM STATISTICS. Bodies exist, but reviewed exact
label/form association is incomplete. No prefix or EIBFN shortcut resolves them.
Source review and generated case candidates are not licensed execution evidence.

## Checks and remaining obligations

Private PROGRAM security and SQLite helper preparation retain their original
bounded scope. The [PR #389 integration review](https://github.com/toreleon/mainframe-env/pull/389)
verified 35 focused runtime tests, including 18 LINK attestations, seven transfer
intents and six compiled SYNCPOINT consumption/ledger cases with a fresh
same-builder export. The remaining four tests cover retained IMS/MQ behavior.
These are scoped results on the reviewed integration inputs, not full CICS
application or SPI/FEPI acceptance.

All public command behavior, complete lifecycle/authorization/concurrency,
quiesce/drain/restart matrices, broader backend compatibility and required
licensed differentials remain pending. No licensed runner is configured;
differential=pending, credit=0. The parent is not sealed from partial children.

## Integration status

PR #389 is merged. The source preparation, existing-authority conflict
reconciliation, cache tooling and review repairs are present on main. The review
corrected AMS source citations, removed three unused IMS/MQ implementation/test
copies and removed two redundant effect-limit conversions. This integration
preserves upstream MQ/IMS/Db2/CardDemo authorities and the public package baseline;
it does not complete SPI-1001 or admit public SPI/FEPI execution.

Source product/API versions and schema wire identities remain meaningful.
Current subsystem paths own conformance inputs; release/version history and
execution receipts remain outside the repository management model.

## Cache provisioning status

`conformance/tools/ibm_docs_snapshot.py` implements bounded deterministic external
snapshot packing/import for existing pinned source bytes. Corruption, bounds,
path and conflict tests cover the tool. Provisioning grants no grammar, runtime,
conformance or licensed credit. Publication bodies remain outside this repository;
unavailable scopes remain unavailable.

The cache repository is now private owner storage at revision
`058ee688e8d7b6702dd5088d5cfdcb5651d64608`. It provides the canonical
`cics-retained-20261007.tar.gz` and equivalent inspection ZIP. Both contain the
same 849 entries; the TAR matches the unchanged committed archive pin. An
actual LFS download and offline import verify 848 topics and one TOC in the
current workspace. Cache availability grants no runtime or licensed credit.
Developers without access provision their own authorized matching bytes.

The earlier metadata-only revision and synthetic transport checks retain their
original scope; they no longer describe current storage availability. Source
provisioning and runtime acceptance remain distinct.

Application review supplements pinned outside the shared registered scopes are
not included in this snapshot. Existing verified supplement cache entries remain
required for the architecture/source-review gates.

The current workspace lacks the application-review body
`SSJL4D_6.x/applications/designing/dfhp37p.html`; source freshness remains
unavailable here. Supply authorized matching cache bytes through the
[cache runbook](../../../runbooks/IBM-DOCS-CACHE.md) before claiming that gate
passed. No ordinary review refresh or source waiver is implied.

## SPI-1001.cache-storage-binding

Status: **Complete (provisioning metadata only)**. This non-semantic slice updates
the existing external snapshot storage revision and provisioning instructions after the owner uploads
the canonical TAR and equivalent inspection ZIP to private Git LFS storage.
The archive SHA-256, bytes, 18 scope IDs, source pins and zero-credit fields remain
unchanged. Acceptance is an exact archive/hash/content comparison, shared offline
import, focused snapshot regression tests, existing source-scope verification,
documentation/changelog and dependency-policy checks. No publication bytes enter
this repository and no public runtime or licensed acceptance is claimed.

The archive comparison and offline import pass, with 849 identical TAR/ZIP
entries and 848 verified topics plus one TOC. All 20 focused snapshot regressions,
registered SPI/FEPI scope checks, formatting, documentation/subsystem/changelog
checks, dependency policy and diff review pass. The unchanged source pins retain
their original identity; no runtime parent is complete from these checks.
