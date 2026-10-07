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

Original private PROGRAM security preparation produced 11 passing tests/70 fixture
iterations; SQLite helper preparation produced five passing tests; scoped
SYNCPOINT consumption produced six passing tests/30 scoped verdicts. These are
original scoped producers, not a full current-candidate suite. Current integration
checks must be recorded separately after reconciliation with main.

All public command behavior, complete lifecycle/authorization/concurrency,
quiesce/drain/restart matrices, broader backend compatibility and required
licensed differentials remain pending. No licensed runner is configured;
differential=pending, credit=0. The parent is not sealed from partial children.

## Declared integration ownership

SPI-1001.origin-main-sync: reconcile PR389 with origin/main
8acfd9875a25ecc3d10459abd2da9f12a490d1f3. Follow current subsystem paths,
framework documentation and public package baseline. Do not restore VERSION,
release/evidence management, version-number conformance directories or retired
release commands. Source product/API versions and schema wire identities remain
meaningful and must not be blindly removed.

Manager owns docs, tools, xtask, conformance schemas/catalogs/generators and the
integration index. CLI author 01a0ff99-3347-7380-a417-5b54c817e32e owns Rust
conflict reconciliation under crates/ only in a separate worktree from the same
merge inputs. Preserve upstream MQ/IMS/Db2/CardDemo changes and this PR's CICS
frame/storage/private PROGRAM semantics; compose existing authorities. No import
of unsealed borrowed-control work, new runtime admission, policy waiver, source
refresh, license execution, push/PR/merge commit by the author or extra workers.

Author focused compilation/proof and a reviewable whole owned diff precede manager
import. Manager resolves current-path references, regenerates owner-derived
files and runs affected tests plus mandatory policy/schema/format/docs/module
checks. Different CLI review checks the final integration resolution. Preserve
unrelated worktrees and required receipts externally; clean intended Cargo
targets after build/test/generator sequences. Only a passing synchronized
candidate is pushed to update draft PR389; no GitHub merge/release/deployment.
