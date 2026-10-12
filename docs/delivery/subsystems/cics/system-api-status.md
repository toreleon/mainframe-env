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
PERFORM SSL and SPI0204 PERFORM STATISTICS. Their manifests and candidate
locators exist, but the required retained bodies are unavailable in the current
offline cache; reviewed exact label/form association remains incomplete.
No prefix or EIBFN shortcut resolves them.
Source review and generated case candidates are not licensed execution evidence.

### SPI-1001 PERFORM source/form joins (#413)

Status: **Blocked on three retained command bodies; negative controls implemented**.
The authorized source-only slice uses branch `codex/spi-perform-source-joins-413`
and base `a688045a` (origin/main). The base still marks all three joins
`candidate-unresolved`; none is already fixed. GitHub issue retrieval was
unavailable, so scope follows the supplied #413 contract. PR #393 overlap review
excludes `system-api-plan.md` and shared registries from this slice. The
documentation manifest is regenerated normally for this status update; its
shared path is routine generated overlap, with no reserved Foundation claim.

Offline searches and exact SHA-selected reads against
`MAINFRAME_ENV_IBM_DOCS_CACHE=$HOME/.cache/mainframe-env/ibm-docs` found no matching
body for any join. No retained HTML root is configured. Each join is blocked
independently under the missing-body rule; no source equivalence is assumed.
The product is IBM CICS TS 6.x, command-body baseline
`ibm-cics-ts-6x-spi-command-bodies-2026-09-12`, with official identities from
`ibm-cics-ts-6x-2026-08-31:spi-commands-unique`.

| Identity | Official label | Candidate source label/form | Disposition |
|---|---|---|---|
| SPI0201 / row 0201 | PERFORM SECURITY | PERFORM SECURITY REBUILD | Blocked: pinned body missing |
| SPI0203 / row 0203 | PERFORM SSL | PERFORM SSL REBUILD | Blocked: pinned body missing |
| SPI0204 / row 0204 | PERFORM STATISTICS | PERFORM STATISTICS RECORD | Blocked: pinned body missing |

Exact missing topics under
`SSJL4D_6.x/reference-system-programming/commands-spi/` and retained SHA-256 pins:

- SPI0201: `dfha8_performsecurity.html`,
  `830d0abe3e41d8ded0a29d9692afa52c2466dc4a70f2dccdb5af8b62aabf86f2`.
- SPI0203: `dfha8_performssl.html`,
  `5179c5ec352f12fd973f0e702bfd8cafa4c70b2b73ac9f69b4182638805daff9`.
- SPI0204: `dfha8_performstatistics.html`,
  `3732a301a401c5720c300be074876fe7f1b1392de3efb0011f6c19259b6b6130`.

Provision authorized matching bytes through the
[cache runbook](../../../runbooks/IBM-DOCS-CACHE.md), then review the exact
official cell, command heading and syntax form before promoting each join.
The mapper's focused tests now cover ambiguous/missing exact source labels,
wrong reviewed rows/hashes and missing retained bodies for all three identities.
These rejection controls preserve the unresolved state; they do not verify the
candidate equivalences. No join fix or generated authority change is claimed.

Focused checks use `python3 -B -m unittest discover -s tools/tests -p test_cics_system_source_map.py`
and `python3 -B -m unittest discover -s conformance/subsystems/cics/system/tools/tests`.
Canonical generator checks use
`python3 -B tools/generate_cics_source_map.py --family spi --check`, its FEPI
equivalent, and
`python3 -B conformance/subsystems/cics/system/tools/generate_spi1001_catalog.py --check`.
Without retained input bytes these validate committed authorities only.

SPI-1001 still requires these three exact joins and complete source-reviewed
grammar, option, condition, lifecycle/context and registry obligations. The
private projection remains 266 SPI/39 FEPI identities; the official denominator
remains 269 SPI/39 FEPI. Public admission is unchanged, with no SPI/FEPI runtime
and zero licensed credit. Integration regenerates the shared documentation
manifest after combining source changes, following the existing coordination
workflow.

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
