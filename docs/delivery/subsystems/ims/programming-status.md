# IMS — DB / TM programming surface progress

Subsystem: **ims**
Phase: **programming**
Target release: **0.14.0**

Status: **Proposed**

## IMS-1401.public-ssa-navigation (bounded slice implemented)

Parent: IMS-1401. Candidate base: `4040bfef`, incorporating manager commits
`2f6a8d8e` (per-PCB sensitivity), `2f70b82f` (witnessed local UOW fences),
and `6f3dd74b` (equivalent baseline lint repairs), after CardDemo `587bf70e`.
The consumed dependency identities
below and integrated checkpoint/participant-preparation/CardDemo repairs remain
in force. Scope: additive bounded raw display-code SSA navigation request on the
existing host provider and selected signed-package database route; catalog
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0005/:0006`.
Local obligations: exact named/one-based offset binary predicates, relations and
Boolean grouping, concatenated hierarchy keys, supported command semantics,
status/output/position after successful and unsuccessful navigation, hold/update,
malformed/context/limit/authorization rejection without mutation, canonical replay,
rollback and Memory/SQLite reopen. Host contracts own the additive DTO/encoding;
IMS metadata, engine, generic service, SAF and shared row/effect/UOW authorities
retain their existing ownership. No new engine, store, coordinator, durable schema,
recovery DTO, secondary access path, PCB map, TM or shared IR registry is owned here.
No dependency/runtime is added. Legacy `ImsRequest` canonical bytes remain frozen.
GSAM RSA and embedded all-family dispatch are subsequent slices. Source basis:
IMS 15.6 `ibm-ims-15.6-programming-contracts-2026-09-11`, pinned
`ims_ssas.htm`, `ims_ssacodingrules.htm`, `ims_ssas_cmdcodes.htm`,
`ims_cmdcodref.htm`, `ims_ccmdcode.htm`, `ims_gnghncall.htm`, `ims_currentpos.htm`.
Exact retained/root and SHA-archive bytes are verified offline and read with
`ibm_docs.py`; no refresh or publication bodies enter Git. Local tests and this
feature seal grant zero official row/verdict or licensed credit; parent remains
open. Acceptance: fail-first public-provider tests, focused host/provider/selected
product regressions, strict scoped Clippy, formatting, affected catalog/schema
checks and mandatory dependency/docs/changelog gates. Existing unrelated aggregate
architecture cache/module blockers are reported without repeated campaigns.

### Bounded route and explicit remaining equivalence classes

The additive `HostRequest::ImsNavigation(ImsNavigationRequest)` accepts raw
display-code SSA syntax with exact binary comparative values, execution context,
and the unchanged legacy request envelope. GU/GN/GNP/GHU/GHN/GHNP use the existing
selected metadata, parser field resolver, generic provider, engine selection,
per-PCB sensitivity/position/hold helpers, SAF, canonical replay and atomic row
publication. The signed-package entry point shares the existing publication
fences. Named fields, one-based `O` offsets, all parsed binary relations, uniform
conjunction (`&`, `*`, `#`) and uniform disjunction (`+`, `|`) are implemented.
`C` selects exact concatenated physical hierarchy keys, `D` returns the marked
ancestors plus the lowest segment, and one `P` establishes parentage at the
marked occurrence. Key-only segments produce no data. Independent selected DB
PCBs retain separate position/hold state, and authorization targets the requested
PCB's database before observation or replay. GSAM is explicitly unsupported.

Source-backed classes still unimplemented in this route:

- Mixed OR/AND and independent/dependent grouping precedence are not established
  by the pinned coding-rules topic. Expressions mixing OR and conjunction fail
  `Unsupported`; nested Boolean expressions are not accepted by the parser.
- Command codes `A/F/G/L/M/N/Q/R/S/U/V/W/Z`, including numbered subset pointers,
  fail `Unsupported`. The command reference's literal dash/null placeholder
  remains a parser rejection; empty command slots in `*()` are supported.
- Multiple `P` operands, DEDB `D/P`, and all MSDB command codes fail explicitly.
  Source-backed first/last/current positioning, subset pointer, locking/enqueue,
  path-update and segment-sensitive command-code equivalence is still pending.
- `C` resolves only a physical hierarchy whose levels have sequence keys. It
  does not establish logical-child virtual concatenated keys, unkeyed levels,
  secondary-index selection/ordering or secondary-maintenance equivalence.
- Raw update/path-update operands, GSAM RSA and embedded all-family dispatch
  remain subsequent slices. Existing broader engine positioning/locking limits
  remain visible; this slice does not establish full IBM navigation equivalence.

Catalog rows remain
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0005/:0006`; there is no shared
Conformance IR binding, official row/verdict, licensed or full-parent credit.
The programming topic-set SHA-256 is
`a0ab40de8ec8c01a5cd43d4cab98f04c932f4b0b161e544da1d7d83aaac05594`.
Additional positioning and sensitivity topics read offline are
`ims_gnpghnpcall.htm`, `ims_ssacodingformats.htm`, and `ims_processingoptions.htm`.

Integration exceptions are bounded: extracting the selected DB adapter from
`product.rs`, exporting two existing metadata/applicability helpers inside the
IMS service, adding the exhaustive host/canonical enum arm, and documenting the
additive effect contract. TM, recovery, secondary-index, UOW and PCB-map owners'
modules are unchanged. No schema migration is introduced. Existing canonical
IMS golden vectors remain stable; the new variant has its own frozen vector.
Downgrade requires stopping new SSA dispatch and preserving completed replay
receipts; the integrated per-PCB/UOW base's drain/reconciliation requirements
still apply. Reverting this feature does not revert those manager repairs.

### Local verification and wider gate failures

Fail-first public-provider regression returned `Unsupported` before connection.
The final focused route has **11 passing public SSA regressions**, including all
six Get/Hold operations, exact bytes/status/position, unsuccessful navigation,
malformed/context/limits/SAF rejection, independent selected PCBs, replay conflict,
hold/replace, rollback and Memory/SQLite fresh reopen. The host suite passed
106 unit and 9 integration tests, preserving the legacy IMS golden encoding;
the provider suite passed 117 tests before the final position assertions/test;
the selected signed-package suite passed 9 tests. Strict host/IMS Clippy uses
`--all-targets --no-deps -- -D warnings` without suppression. IMS catalog,
assurance matrix, schemas, shared spec, offline dependency policy and changelog
checks pass. The matrix repair changes only 14 stale local-test file locators
from `generic.rs` to `generic/tests/mod.rs`, preserving rows/cases/credit.

The wider server strict Clippy run fails on 11 existing diagnostics in COBOL
replay, CICS token, dataset capability selection and retention/test helpers;
none is suppressed or counted as passing. The aggregate module-size guard also
fails against stale pre-existing ceilings: `product.rs` is reduced from 6291 to
6240 production lines (ceiling 6008). Minimal required enum/dispatch integration
adds 6 lines to the existing canonical encoder (base 3070, ceiling 3046), 5 to
the host request module (base 2336, ceiling 2300), and 48 to the existing IMS
service (base 2109, ceiling 1959). New bounded modules remain below 1200 lines.
These wider failures are retained for manager integration, not hidden by a
baseline refresh. They do not establish full-parent gate or promotion success.
All receipts are outside Git and disposable Cargo targets under the worker's
`v014-completion-20261002/IMS-1401.public-ssa-navigation` receipt directory.

## 2026-10-02 nonlicensed continuation

The user explicitly excluded licensed certification for this continuation.
This is an implementation-work scope decision, not a licensed pass, a release
promotion, or a reduction of the official 25-row denominator. Differential
credit remains **0/25**. The resumed manager candidate starts from
`213ed878ec138bdb2914330db6613559bffc5a86` on
`codex/v014-nonlicensed-completion-20261002` and uses isolated CLI feature
lanes with at most four workers. Each lane uses `gpt-6.1-sol`, high effort,
fast mode off, offline pinned source review and feature completion seals.

The read-only acceptance audit found zero IMS rows/cases in the shared
Conformance IR. The earlier **25/25 local handler mappings are not official
nonlicensed gate passes**. Several historical pending paragraphs were already
superseded by later implementations; they are retained as their original slice
record, not the current remaining-work authority.

Current work and sequencing:

- `IMS-1405.basic-checkpoint-boundary` is sealed at `d5fef29b`; the generic
  checkpoint commits undo and clears position, with four focused regressions.
- `IMS-1406.participant-binding` is integrated at `a130d1ac` as **preparation
  only**, retaining pending/null accepted capabilities. Public-provider and
  four-process SQLite restart tests do not replace the missing coordinator
  fencing, live controls, audit, retention and compatibility obligations.
- `IMS-1406.carddemo-corpus-package-route` migrates the actual clean corpus at
  pinned commit `59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e`, not merely synthetic
  CardDemo-shaped definitions. It must prove signed selected metadata, exact
  load/unload outputs, replay/rollback and Memory/SQLite reopen.
- `IMS-1403.local-uow-isolation` fences competing same-database writers and
  makes whole-image backout fail closed when retained safety cannot be proved.
- `IMS-1403.pcb-sensitivity` repairs request-PCB selection, independent
  positions and targetless GN/GNP sensitivity without protected-data leakage.
- `IMS-1405.application-recovery-dispatch` connects the existing recovery
  authority to bounded public typed calls; real PCB reposition and actual TM
  backout remain required where applicable, not synthetic-row substitutes.

Subsequent owned closure still includes public SSA/command-code/GSAM RSA and
embedded call routing, reviewed secondary access paths and STAT observations,
accepted early IMS participation, independently specified shared IR
obligations/driver/verdicts, and the applicable restart/compatibility/scale
matrix. The manager must integrate feature commits before running the complete
affected-subsystem exit checks on one unchanged candidate. Final mixed-resource
syncpoint closure stays in 0.16; licensed certification is excluded from this
run and remains pending rather than silently passed.

Offline cache resolution found the former CICS `dfhp37p.html` blocker at its
exact retained pin and assembled 509 verified reader entries. The aggregate
architecture gate still requires eight absent pinned CICS bodies (two batch-A
topics and six supplements). No refresh was authorized or performed. These
source-cache findings carry zero semantic credit, and the unchanged aggregate
failure is not repeatedly rerun. Focused architecture/security/effect/storage/
retention guards and dependency policy passed on the audited base; their old
receipts are not relabeled as new-candidate acceptance.

### Consumed dependency acceptance identities

The continuation consumes the released implementation baselines below, not
the starting-branch SHAs in their historical status paragraphs. The annotated
tags resolve to these exact commits and Git trees; the published Apple ARM64
evidence archive SHA-256 values match the GitHub release asset digests. Each
archive retains `manifest.json` and `provenance.intoto.json`; its source-tree
digest also matches the annotated tag. This is provenance review of existing
acceptance, not a rerun or attestation of the current IMS candidate.

| Dependency | Accepted commit / Git tree | Published receipt SHA-256 | Approval / disposition |
|---|---|---|---|
| COBOL 0.4 | `4a50a4e66f08b9cb5d293fb276cfbd52424b07fc` / `92ba4ce0855c02b6157d08fedac2cae51977a972` | `df6b835b57ce586927dafc4e1866e030b04f08929a18eca10de13f52738983b8` | [Explicit 2026-09-02 approval](../cobol/execution-status.md), licensed-pending implementation; [published release](https://github.com/toreleon/mainframe-env/releases/tag/mainframe-env-v0.4.0) |
| RACF/SAF 0.5 | `bd5e8ecd211b7da4f3e18dfcfc807352d0ebd2e8` / `7e89e90c372a8bb1ca63a7b06c3c744e1a85e8d9` | `c0089130b19d524c7f47a7e3dc301392369ffb0e97d045d5c4b9817c214bfec7` | [User-approved 2026-09-01 policy](../racf/security-status.md), licensed-pending implementation; [published release](https://github.com/toreleon/mainframe-env/releases/tag/mainframe-env-v0.5.0) |
| Dataset/VSAM/AMS 0.6 | `ca3c061adaa63af71eefe8ee494b7c523c3e5540` / `a0dbae4c52a0e3aea28a3ea4bf2e1e8297d2d360` | `3665fa112264b470a2c8a09fe3e767d6ca68f7a9f9972df5850855743e9e5cba` | [Approved 2026-09-01 policy](../dataset/data-status.md), [local certification record](../../../../conformance/0.6/evidence/dataset-certification.json); [published release](https://github.com/toreleon/mainframe-env/releases/tag/mainframe-env-v0.6.0) |

The receipts are the assets named
`mainframe-env-0.{4,5,6}.0-aarch64-apple-darwin-release-evidence.tar.gz`
on those releases. Licensed obligations remain respectively **0/153**,
**0/48** and **0/36**, owned by CER-1702 / 0.17 certification; their scoped
historical approvals are not a blanket approval of later subsystem claims.
Affected consumption regressions are the actual COBOL/JCL CardDemo load/unload
route, SAF denial-before-observation/mutation, and shared provider-row / durable
Memory and SQLite contracts. The current feature sections identify their exact
candidate-specific checks; the final integrated exit run is still pending.

## IMS-1406.verification-lint-repair (verification infrastructure slice)

The continuation's IMS/host/conformance Clippy diagnostics found three
unchanged baseline lint failures: nested MQ callback-option validation,
manual membership checking, and a redundant must-use annotation on an already
must-use iterator. This slice applies equivalent predicate/style repairs only;
it changes no IBM semantic, validation outcome, oracle admission or licensed
credit. No unrelated source lookup or licensed run is required. Acceptance is
focused MQ validation and local harness tests, strict affected-package Clippy,
format, changelog/docs and dependency policy. The manager owns the two existing
source files, this section, a unique fragment and routine docs manifest.

## IMS-1405.basic-checkpoint-boundary (implemented repair slice)

Parent: IMS-1405. Candidate base: `213ed878ec138bdb2914330db6613559bffc5a86`
on `codex/v014-nonlicensed-completion-20261002`. This slice repairs the existing
metadata-selected `ImsService::execute` checkpoint boundary, not a new store,
coordinator, raw DL/I operand parser, or mixed-resource syncpoint. The accepted
host, SAF, provider-row CAS, replay, and durable storage authorities remain
unchanged. Relevant catalog context is
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0002` (basic CHKP).
Local obligations are commit-before-new-undo, loss of current/parent/hold
position, exact replay without a second commit, checkpoint-capacity rejection
without mutation, and authorization of every database whose undo is committed.
Memory and SQLite close/reopen are the affected backends. These regressions
grant no official Conformance IR or licensed credit; raw I/O PCB and implicit
message-delivery obligations remain separate unfinished integration work.

Offline `ibm_docs.py search` and `read` passed against the retained verified
`ims-1405-topic-cache` for baseline
`ibm-ims-15.6-recovery-utilities-2026-09-11`, topic
`SSEPH2_15.6.0/com.ibm.ims156.doc.apr/ims_basicchkpcall.htm`,
`sha256:1029208c47f44b8a0144472c8127767b18140c57be70850fdfa00da83ef1743a`
(11,733 bytes). The source states that basic CHKP commits database changes
and loses database position. The current generic handler instead retains undo
and position; focused fail-first regressions reproduced that discrepancy.
The user's 2026-10-02 exclusion of licensed certification does not convert
source review or local handler coverage into official row passes.

The repaired generic checkpoint clears the current, parent and held position,
commits both existing undo maps for its run, and stores the post-checkpoint
session with the normal replay result in the shared atomic row publication.
Authorization covers every pending database, not only the scheduled PCB's
database. Exact replay returns the recorded checkpoint without committing a
later unit of work. Four focused regressions pass, including denial/capacity
no-mutation and a fresh SQLite connection. Scoped strict IMS Clippy,
formatting and diff checks pass. No schema migration is needed; old checkpoint
and session rows remain readable. A prior binary retains its old CHKP behavior,
so rollback of the binary does not retain this corrected semantic guarantee.

## IMS-1403.organization-logical-closure (implementation candidate)

Parent: IMS-1403. Candidate: `codex/v014-organization-closure`; accepted
0.4/0.5/0.6 host, security, and storage authorities remain in force. Scope:
the 13 organization identities in `ims-call-applicability-rules.json`, generic
GU/GN/GNP and hold/update/insert/load routes, and declared logical parent/child
pairs from `mainframe-env.ims-metadata@1`. INDEX and PSINDEX are typed index
databases rather than application data PCBs. The selected `ImsService` route,
Memory and SQLite `ProviderStateStore`, and their CAS, replay, undo, and SAF
contracts own publication. Catalog obligations are the applicable IMS 15.6
`dli-call-families` rows 0004 (DLET), 0005 (GU/GN/GNP), 0006
(GHU/GHN/GHNP), 0008/0009 (ISRT/LOAD), and 0015 (REPL); unrelated TM,
recovery-publication and licensed differential
rows remain pending. Tests must cover each organization class, exact position,
paired/unpaired links, failure, corruption, rollback, CAS and SQLite reopen.

Source basis: `ibm-ims-15.6-database-contracts-2026-09-11` and
`ibm-ims-15.6-metadata-contracts-2026-09-11`. Retained content-addressed HTML
was verified against the manifest and parsed with `ibm_docs.py` for
`ims_damdball.htm` (`a27054fc946043db089c678b85c42423fdcb6b219db016bd8c6a14d2aea6ba7c`),
`ims_hisamdb.htm` (`c9ae318dce4e56d6300babb016648ab5141972d1c49ea3fa052b561f05cee4d0`),
`ims_processinglogicalrelationships.htm` (`ae5f2c81859d6631c28380eaa7d6744735b16ba99c9337cdd701f9d54bc0bfee`),
`ims_lchildstmt.htm` (`518642c5f4fdbddb5f8a9ebc35dc609e5aeffa16b4951e99c2e655e285fcb9f8`),
and `ims_dbdstmt.htm` (`ce1b4b70aa0b5803d25e5d72a71004cffb071361fcbc93a658bd3ce4e17cf7cd`).
Offline `search` and `read` were attempted but the configured cache lacks the
verified IMS TOC; no network refresh is authorized. This source review earns no
licensed or conformance credit.

The candidate accepts all 13 pinned organizations through the generic metadata
route. INDEX/PSINDEX reject application data PCB scheduling with `AC`; the other
organizations use bounded hierarchy ordering, holds and updates as applicable.
Logical child insertion resolves one declared parent through parent-segment
field qualifiers. Child navigation returns the linked parent's current data;
paired parent deletion cascades to linked children, while an unpaired live child
fences parent deletion. The child image carries optional occurrence links, so
pre-slice images remain readable; binary rollback after creating links requires
retaining this reader because the older reader cannot maintain them. This slice
does not add a store, lock, coordinator or migration runner.

Focused verification on the current candidate: 65 IMS library tests, 12 TM
integration tests and doc tests pass, including all organizations, failed
resolution/no mutation, paired and unpaired deletion, rollback, store-capacity
failure, corruption, replay and SQLite reopen. Formatting, dependency policy,
changelog, docs and IMS catalog checks pass. Scoped Clippy passes with only
`clippy::collapsible_if` and `clippy::bool_assert_comparison` excepted for
unchanged recovery code; strict Clippy reports those two pre-existing warnings
in `recovery/utilities.rs` and `recovery/runtime_tests.rs`. The broader
`architecture-fast` check passes its transaction, effect, row persistence,
storage, authorization and retention guards, then stops at an unrelated missing
CICS cached topic `SSJL4D_6.x/applications/designing/dfhp37p.html`. No CICS
cache repair or network refresh belongs to this slice. These local tests do not
claim Conformance IR row verdicts or licensed differential credit.

## IMS-1401.remaining-call-families (implemented slice)

This bounded slice covers catalog rows 0001, 0003, 0007, 0011–0014, and
0022: INIT/ACCEPT, DEQ, GSCD, POS, INIT/QUERY, INIT/REFRESH, and STAT.
The public route is `ImsService` and `ims_providers`; the existing metadata,
session, replay, provider-row store, enterprise authorization, and effect/UOW
contracts remain authoritative. Memory and SQLite are the restart backends.
The typed runtime covers both INIT/ACCEPT rows, INIT/QUERY, INIT/REFRESH,
DEQ, GSCD, POS, and STAT. It validates row-qualified call sites against the
complete generated profile before replay or execution, and resolves returned
PCB statuses through the generated status authority. Q-class reservations,
DEDB area positions, SCD/PST addresses, and bounded buffer-pool statistics
reside in the existing IMS provider row store. Application and resource names
identify resources only. Qualified POS searches a retained DEDB engine image;
an empty metadata-only DEDB yields GE. The database-engine expansion for all
13 organizations is a separate work package.

Focused IMS and host-API suites pass, including Memory/SQLite restart, replay,
authorization-before-observation, malformed/forbidden-context no-mutation,
and source-corrected POS/GSCD applicability regressions. These unit tests do
not grant licensed differential or full 25-family conformance credit.

The generated applicability profiles were corrected from retained IMS 15.6 HTML:
`POS` accepts an optional single qualified or unqualified SSA only in online
DEDB contexts; `GSCD` is batch-only and accepts I/O or DB PCB; full-function
`DEQ` uses the I/O PCB in database contexts. The reviewed source is IMS 15.6
baseline `ibm-ims-15.6-dli-2026-08-31`, comparison topic
`SSEPH2_15.6.0/com.ibm.ims156.doc.apg/ims_comparingexecdlicmdsanddlicalls.htm`
at `sha256:ce4a179eac0f18ed4ff71bed9ca576b31bcbbdb076d305dbcbf942e26ce13e30`;
the programming-contracts baseline `ibm-ims-15.6-programming-contracts-2026-09-11`
pins `ims_dlicallfunctions.htm` at
`sha256:cc1b7fb49795e5bf1b928fdd4e4ec211cd4e807a2e4c1ff737e029f282817866`.
The retained IMS 15.6 HTML archive was separately hash-verified for
`ims_hinitcall.htm` (`a48c4bbf78f6e6dcb6f12b60b3f1822178e3267012fa8c2c90531d8f6c52dacd`),
`ims_acceptcmd.htm` (`e2a063a48f212ee305c67af3deb3caa9153062c7810a13501f10092a789b055a`),
`ims_querycmd.htm` (`dc5f2e3f4ea03214fe1da6e8cc59036a5855462046c86bb5932f9894d4a3e01e`),
`ims_refreshcmd.htm` (`9e73a1664f230ff6b757860d2ed7672d95096c28c35dbaf941bbf808ede100f3`),
`ims_deqcall.htm` (`ece3fcc632ba0b1cfa68836d8ab30cadf8081afbdb7d626ef26cc4d337728990`),
`ims_gscdcall.htm` (`541467790889e582607ff3a5e4cbe8bc5a9b730af6ce468330beaebe0a512bd7`),
`ims_poscall.htm` (`d2ccc24ca342defff492996928fe60c6cdd505a07ad0babdec8e4d4577e8057b`),
and `ims_statcall.htm` (`cc777a81e45ccaedc704c8319a9f9b6a6eb7b30521b00b110cd371491eb5fb51`).
These retained direct topics are reference data, not licensed execution evidence.

## IMS-1406.carddemo-coverage-closure (local slice)

Candidate: `codex/v014-carddemo-closure`, using the accepted 0.4 host ABI,
0.5 SAF, and 0.6 shared storage contracts. The official denominator remains
the 25 `dli-call-families` rows in `ibm-ims-15.6-dli-2026-08-31`.
`conformance/0.14/ims/assurance-matrix.json` now binds each row to its reviewed
applicability profiles, a typed runtime handler, executable
local tests, and explicit PCB status, position, mutation, and output observation
states. The checker rejects missing, duplicate, stale, name-dispatch, and
non-executable bindings. The product's selected-package IMS database route uses
the existing signed package selection, metadata publication, and generic IMS
service. Memory and SQLite CardDemo-shaped package tests cover signature
rejection, installation, database and TM execution, rollback, and reopen.

This is local regression evidence only. Executable local handler coverage is
**25/25** after binding the eight system-family rows to `ImsSystemCall` and its
Memory restart/replay regression. Exact licensed per-row observations and full
CardDemo corpus migration from legacy job definitions remain open. The external
licensed IMS 15.6 differential is pending, and coverage credit is **0/25**.
Final mixed-resource syncpoint closure remains in 0.16. The next executable
step is to migrate the pinned CardDemo IMS corpus job bindings onto the selected
generic package route, then run the independent licensed bundle on a sealed
candidate.

Focused verification: the eight IMS package tests and three assurance-checker
tests passed; `ims-assurance-matrix`, `ims-catalog`, `changelog`, `docs`, and
`cargo deny check` passed. `architecture-fast --check` passed its earlier
guards but stopped at `cics-sources-a-auto-review` because the offline cache
lacks `SSJL4D_6.x/applications/designing/dfhp37p.html`. This unrelated cache
gap was not refreshed. The IMS reader's verified TOC remains unavailable.

Offline source review used `ibm_docs.py search` and `read`; both reported the
missing verified IMS TOC in the configured reader cache. The user-specified
retained SHA-256 HTML archive was verified and parsed with the repository's
`ibm_docs.py` plain-text parser. Relevant IMS 15.6 pins: comparison table
`ims_comparingexecdlicmdsanddlicalls.htm` at
`ce4a179eac0f18ed4ff71bed9ca576b31bcbbdb076d305dbcbf942e26ce13e30`
(`ibm-ims-15.6-programming-contracts-2026-09-11`); current position
`ims_currentpos.htm` at
`07aafcddb9b30591eef1da5ad50bfe8bc3c1e9b9e9e51b56b27d39bf6adf5673`;
DB PCB mask `ims_imsdbdbpcbmask.htm` at
`699a551e0c2804db26725d0997be3b1f9fdc91379a69f490c8e509d76fcc61b3`;
PSB database PCB `ims_psbgendlipcbstmt.htm` at
`0dad54edd1a9940ca9a6988e06412836ff35a706cc2e1f7fbd36eb14fa02cfba`
(`ibm-ims-15.6-metadata-contracts-2026-09-11`); TM ISRT
`ims_isrtcalltm.htm` at
`9b0bd68473b41b3614641637776047ba148e2f28d9d5133ec0d78cf931560a31`
(`ibm-ims-15.6-tm-contracts-2026-09-11`); ROLS `ims_rolscall.htm` at
`b7e15d0c110d3296eac11d895326b3ef48ac913fd682b94b312aa6c59ad14af5`
(`ibm-ims-15.6-recovery-utilities-2026-09-11`). These source reads grant no
licensed or behavioral credit.

## IMS-1406.licensed-harness-contract (bounded slice)

Parent: IMS-1406. Scope: contract preparation for exactly the 25 official
`dli-call-families` rows in baseline `ibm-ims-15.6-dli-2026-08-31`; no runtime
route or public behavior changes. The fixture index binds each row, including
repeated INIT, ISRT, CHKP, and XRST spellings, to its reviewed applicability
profiles and the pinned IMS 15.6 comparison topic. External input bundles,
authorized licensed IMS execution, exact service/environment/runner/fixture
digest pins, and a current clean candidate receipt are mandatory for credit.
The bounded normalization compares two-byte PCB status, position and state
digests, mutation, and output. The local generator, verifier, schema gate, and
mutation tests validate only harness plumbing. Differential credit remains
**0/25, pending-external-licensed-receipt**; parent IMS-1406 and the 0.14 exit
gate remain open. No credentials, raw licensed outputs, or receipts are kept in
Git. Next step: an authorized external IBM IMS 15.6 run against the sealed
candidate and independently prepared pinned fixture bundle, followed by the
`--require-pass` verifier gate.

Source review: `ibm_docs.py search/read` could not complete because the IMS TOC
is unavailable in the configured offline reader cache. The retained comparison
HTML at `SSEPH2_15.6.0/com.ibm.ims156.doc.apg/ims_comparingexecdlicmdsanddlicalls.htm`
was independently checked at 16,046 bytes and
`sha256:ce4a179eac0f18ed4ff71bed9ca576b31bcbbdb076d305dbcbf942e26ce13e30`
and parsed with `ibm_docs.py` plain-text rules. The database status-table topic
was likewise checked at 75,709 bytes against its programming-manifest pin.
For catalog rows 0002 and 0016/0025, retained basic CHKP and XRST topics in
`ibm-ims-15.6-recovery-utilities-2026-09-11` matched respectively
`sha256:1029208c47f44b8a0144472c8127767b18140c57be70850fdfa00da83ef1743a`
and `sha256:aff46320869b8910ab9916011c8d722a5e044f9e33970d2996e0501204046cb6`
and were parsed offline.
These offline source checks grant zero licensed or behavioral credit.

Recovery branch: `codex/v014-ims-tm-runtime`, stacked on IMS I7
(`codex/v014-ims-database`, `42885f39`). Earlier slices reconstructed source
scopes from lost commits `a6c3ad61` and `3b6ce193`.

## Recovered identity catalog

The IMS 15.6 baseline `ibm-ims-15.6-dli-2026-08-31` pins 25 mandatory
`dli-call-families` rows in `conformance/0.2/catalogs/ims.json`.
The comparison topic is
`SSEPH2_15.6.0/com.ibm.ims156.doc.apg/ims_comparingexecdlicmdsanddlicalls.htm`
at `sha256:ce4a179eac0f18ed4ff71bed9ca576b31bcbbdb076d305dbcbf942e26ce13e30`.
`cargo xtask ims-catalog` generates typed IR identities and retains all 30
call-name and 30 command-name memberships. Repeated spellings remain separate
official rows. The registry does not claim execution or handler readiness.

## Recovered programming sources

`conformance/0.14/manifests/ims-programming-contracts-topics.json` pins 26
IMS 15.6 programming topics (574,527 bytes) for SSA, command codes,
PCB/status, get/position, processing options and call-family review. Its
topic-set digest is
`sha256:a0ab40de8ec8c01a5cd43d4cab98f04c932f4b0b161e544da1d7d83aaac05594`.
The added C-command topic is
`SSEPH2_15.6.0/com.ibm.ims156.doc.apr/ims_ccmdcode.htm` at
`sha256:038fcdaa210493f4c423ae2b0bc7fbbbf8bd381146a8ad25cf7615d28174d65a`.
All 26 pinned body hashes match local retained HTML. The configured offline
reader's IMS TOC is unverified, so search/read cannot yet display those topics.
This source registration grants zero semantic, conformance, differential or
licensed credit.

## Recovered database and TM sources

`conformance/0.14/manifests/ims-database-contracts-topics.json` pins 20 IMS
15.6 database organization, variable-segment, index, logical-relationship, and
mutation topics (110,015 bytes). Its baseline is
`ibm-ims-15.6-database-contracts-2026-09-11` and its topic-set digest is
`sha256:3b819e69f7ce608d66bf1b8f195556047024333374e6f77413af29822960e38b`.

`conformance/0.14/manifests/ims-tm-contracts-topics.json` separately pins ten
IMS 15.6 TM call, I/O PCB result, scheduling, and conversation topics (155,700
bytes). Its baseline is `ibm-ims-15.6-tm-contracts-2026-09-11` and its
topic-set digest is
`sha256:dac590371f5b7be6747c7280598ae9db075ea0f7c59473a2fcd8b9c9d8c0cd36`.
The 26-topic programming manifest remains byte-identical. All 30 added body
hashes and byte counts match retained local HTML. The IMS TOC is absent from
the configured offline cache and local corpus, so the offline reader cannot
complete a scope status or read check. These source pins grant zero semantic,
conformance, differential, or licensed credit.

## Recovered SSA contract

The reviewed Draft 2020-12 rules and `cargo xtask ims-catalog` generated host
API preserve 17 command-code identities, five subset-pointer forms, 18
relational encodings, and five Boolean encodings. The bounded display-code
parser handles fixed-width names, null command slots, named and O-code offset
fields, binary field values, and C-code concatenated keys using supplied DBD
length metadata. Malformed or unsupported forms return explicit errors before
any IMS execution; this slice adds no execution path. The rules cite seven
topic paths and hashes and bind five rule groups to individual sources in
`conformance/0.14/ims/ssa-rules.json`.

## Recovered TM call contracts

Lost worker `f3ec3946` (manager import `886002cd`) supplies typed, bounded
transaction definitions, segmented input messages, I/O and alternate PCB calls,
conversation actions, and four exact core TM statuses. Validation rejects
malformed names, duplicate definitions, limit violations, and reviewed CPI-C and
Fast Path call restrictions before any TM side effect. The source baseline is
`ibm-ims-15.6-tm-contracts-2026-09-11` in
`conformance/0.14/manifests/ims-tm-contracts-topics.json` (topic-set digest
`sha256:dac590371f5b7be6747c7280598ae9db075ea0f7c59473a2fcd8b9c9d8c0cd36`).
The matching retained HTML was verified by byte count and SHA-256. Source
mapping by rule group:

| Contract group | Pinned topic path and SHA-256 |
| --- | --- |
| GU/GN applicability | `SSEPH2_15.6.0/com.ibm.ims156.doc.apr/ims_gucall.htm` `cbd8bd8a4e59418d990c40a3e46458019c504daf529ca45dae036fce51e16edb`; `SSEPH2_15.6.0/com.ibm.ims156.doc.apr/ims_gncall.htm` `57a3bca4ea20a4e38d3b33e056a154be734e21b06b045785f94510787e83f153` |
| ISRT, PURG, CHNG and PCB routing | `SSEPH2_15.6.0/com.ibm.ims156.doc.apr/ims_isrtcalltm.htm` `9b0bd68473b41b3614641637776047ba148e2f28d9d5133ec0d78cf931560a31`; `SSEPH2_15.6.0/com.ibm.ims156.doc.apr/ims_purgcall.htm` `3b6414156c4c76da888278ff17a1a3d8ed9cedfee1068373f11c46588719e0a6`; `SSEPH2_15.6.0/com.ibm.ims156.doc.apr/ims_chngcall.htm` `ec4c7891b7d19dde573319846411c7af40170e90842c3663e59ab85df0999acc` |
| Scheduling, message PCB and statuses | `SSEPH2_15.6.0/com.ibm.ims156.doc.ccg/ims_tm_plan_terminals_msgsched.htm` `38a382955cc478134c1345691d29fd1943511032c08418e4f3ca215aafff5a56`; `SSEPH2_15.6.0/com.ibm.ims156.doc.apg/ims_resultsofmessageiopcb.htm` `76fb85f8fb2741324ea42681a80bc2c3b3687341c7f4f32db25d2a227c323d69`; `SSEPH2_15.6.0/com.ibm.ims156.doc.mc/compcodes/ims_dlistatuscodestables_messagecalls.htm` `c18deaa4db069bc24064071bb2ca50be736dde1ea8a324ca452d546df58d2955` (programming baseline) |
| Conversation and SPA | `SSEPH2_15.6.0/com.ibm.ims156.doc.apg/ims_codingconversations.htm` `9d429a6ed6e73e1f747b1059c93264761e14bf72cc3ee3aa567d53e1853c84c2`; `SSEPH2_15.6.0/com.ibm.ims156.doc.apg/ims_conversationstate.htm` `999cb001b8c66c2623668b3143cf1dd16b7287cbb8655555d776190a26f4fb64`; `SSEPH2_15.6.0/com.ibm.ims156.doc.apg/ims_conversationrecovery.htm` `5afd0e6ec7fd527047ff0ecbb30be819cfb11fc9efa894adae232299c9fbc6c3` |

The numeric limits and execution identities are local safeguards, not IBM
compatibility claims. The contract slice added no runtime or conformance credit;
the separate recovered runtime foundation is described below.

## Recovered PCB and status contract

Lost worker `e89d9ca6` (manager import `ec8212c4`) supplies Draft 2020-12
rules for four PCB masks and 205 status-context memberships across 162 distinct
two-byte codes. `cargo xtask ims-catalog` validates the rules against the pinned
IMS 15.6 programming manifest and generates typed host API descriptors. The
database, GSAM, I/O, and alternate masks preserve field order, width, status
offset, and execution-context applicability. Bounded layout and status lookup
reject unsupported context, width, code, and PCB-kind combinations before any
IMS side effect. The four existing core TM statuses now validate through the
same generated message-status registry while retaining their bounded TM API.

The seven rule groups cite their individual topic paths and SHA-256 hashes in
`conformance/0.14/ims/pcb-status-rules.json`: four mask topics and the database,
system-service, and message status-table topics. Each cited body matches the
retained local HTML and the 26-topic programming manifest. The configured
offline reader still lacks a verified IMS TOC, so its `search` and `read` cannot
display these topics. This slice grants no conformance, differential, licensed,
or execution credit.

## IMS-1401 call applicability and diagnostics

`IMS-1401.call-applicability` freezes a validation-only matrix for all 25
official comparison-table rows. Its single reviewed rules file defines 25
bounded profiles spanning the five execution contexts, four PCB kinds, 13
pinned organizations, processing-option classes, and SSA/RSA forms. The IMS
catalog generator derives row-qualified call/command spellings from the
immutable 0.2 catalog and emits typed descriptors and nine stable diagnostic
codes. Repeated INIT, ISRT, CHKP, and XRST spellings cannot select a row by
themselves. Each candidate must match one whole profile, preventing allowed
values from separate profiles being combined into an unreviewed call site.

The reviewed distinctions include DEDB-only POS and subset-pointer forms,
MSDB command-code exclusion, GSAM GN/GU/ISRT and GU's returned RSA,
full-function STAT, database PROCOPT capabilities, and the full-function
versus Fast Path DEQ PCB split. Raw PROCOPT and SSA bytes must first pass
their existing metadata/parser authorities; this matrix accepts only their
bounded classes. The rules digest is
`sha256:c7b2abd93786a7ebcc1e1211456c2d08ea6f299b8cf352c7e2c8822906f63598`.
Eight focused host-contract tests cover every row/profile, call and command
representatives, bounded pairwise classes, and forbidden cross-profile cases;
the 53 existing host API unit tests and six IMS generator tests pass. Generator
freshness, formatting, docs, strict host API Clippy, and xtask Clippy with the
unrelated CICS pilot dead-code warning excepted pass. Dependency policy still
reports the unchanged locked `rustls 0.23.43` advisory `RUSTSEC-2026-0285`;
bans, licenses, and sources pass. Neither finding is introduced here.

The source basis is IMS 15.6 baseline `ibm-ims-15.6-dli-2026-08-31`, with
the comparison topic at
`sha256:ce4a179eac0f18ed4ff71bed9ca576b31bcbbdb076d305dbcbf942e26ce13e30`,
DL/I functions at
`sha256:cc1b7fb49795e5bf1b928fdd4e4ec211cd4e807a2e4c1ff737e029f282817866`,
processing options at
`sha256:bb549e17230c1990ac0b5b5f0386512493bcd5552b3b4762bd7fc4436a923dd4`,
SSA command codes at
`sha256:d95a2d5c1dc735e2abf4b4bba206c704ee0d5104c47ff63afa848705f06084de`,
and GSAM retrieval/insertion at
`sha256:8dcb020dc55ed37c1cdf304e3cc7f6c0a724edb2868c910f4055e81ce2bd2ae1`.
All cited paths are checked against the pinned programming or database
manifests; the retained raw GSAM HTML was hash-verified and parsed offline.
This contract does not route calls or claim recognized, executed, conditioned,
recovered, differential, or licensed evidence.

## Recovered database engine foundation

Lost worker `9d154ac2` (manager import `590775b3`) supplies an isolated,
bounded, in-memory hierarchy engine. It supports HDAM/HIDAM/HISAM/SHISAM
segment order, qualified GU/GN/GNP-style navigation, caller-owned position and
parentage, hold-gated replace and physical subtree delete, variable segment
lengths, atomic secondary indexes, stale-hold fencing, and append-only GSAM.
Rejected duplicate keys, malformed data, index conflicts, key changes and stale
holds leave the image unchanged. The engine exposes typed internal errors and
does not return PCB status codes or register a host execution path. Its API
remains an isolated runtime projection; the I6 metadata catalog and existing SSA/PCB
contracts remain the authorities for published metadata and call parsing.

The source basis is `ibm-ims-15.6-dli-2026-08-31` in
`conformance/0.14/manifests/ims-programming-contracts-topics.json` and
`ibm-ims-15.6-database-contracts-2026-09-11` in
`conformance/0.14/manifests/ims-database-contracts-topics.json`. Relevant
retained bodies include `ims_gughucall.htm`
(`sha256:0a9b433d9e38e58c125232a94147acd309e5bbbd7baf3c8cb900a3e8fdf6c8b9`),
`ims_gnghncall.htm`
(`sha256:063ff108614ee13694ea7df2f7b39647447059590162614da56de2aa2eb49cb4`),
`ims_gnpghnpcall.htm`
(`sha256:6daaf5929bf3640a6d4ab17ea97d81328e60b26eb38c32b2c991c77d41592762`),
`ims_varlengthseg.htm`
(`sha256:a565890e0371da0ccb641c071f7f6696cf21d0efc89b66f4f9bdadbb44f3a01c`),
`ims_replacecall.htm`
(`sha256:55778b03e47f21e92fb967995ec54b7ff1903c7886b98bdb1394d6d85e8fd23a`)
and `ims_issuedeletecall.htm`
(`sha256:f40ecf698e4817f47ca8a4abd5214baa880476393c4f765a5c215051acdc6a23`).
The retained HTML matches pinned hashes. The configured offline reader cannot
verify the IMS TOC, so its search/read commands remain unavailable. This
source review and local tests grant no conformance, differential or licensed
credit. Persistence, recovery, PCB status mapping, host routing and TM runtime
remain outside the database engine slice.

## Recovered TM runtime foundation

Lost worker `c0d139bb` supplies bounded TM queue admission, ordered work
claims, GU/GN message delivery, fixed and modifiable alternate PCB routing,
ISRT/PURG output groups, conversational continue/switch/end, cancellation,
timeout, rollback, TERM, idempotent replay, and admission-gap repair. The
provider stores version-one catalog, message, session, conversation, outbound,
and replay rows through `ProviderStateStore`; `WorkStore` owns work, the logical
clock, and fenced leases. The existing authorizer checks PSB, transaction, and
destination access before protected transitions. Focused Memory and SQLite
tests cover these boundaries, including reopen of cursor, output, and
conversation state.

The port adds QD decoding through the generated message/I/O PCB status
registry and returns `ResourceExhausted` when an output segment buffer reaches
its local limit. It does not return QF for queue capacity: the pinned message
status topic assigns QF to invalid segment length. These adaptations have
fail-first tests. The source baseline remains
`ibm-ims-15.6-tm-contracts-2026-09-11` in
`conformance/0.14/manifests/ims-tm-contracts-topics.json` and the message
status table at
`SSEPH2_15.6.0/com.ibm.ims156.doc.mc/compcodes/ims_dlistatuscodestables_messagecalls.htm`
(`sha256:c18deaa4db069bc24064071bb2ca50be736dde1ea8a324ca452d546df58d2955`).
The retained topic bodies match the manifest hashes; the configured offline
reader still lacks a verified IMS TOC.

The `IMS-1404.application-dispatch` work package connects the generic TM
runtime to a selected signed application generation. Its changed boundary is
the v2 package `ims_tm` section, `ProductServer`'s public `ims_tm_*` API, and
the existing `TmService` catalog, queue, and work lease; it does not add a
scheduler or a second store. The section validates PSB and alternate PCB
references against package IMS metadata and binds selector and artifact to a
signed program entry. Publication and rollback retain prior definitions, and
admitted message/session/conversation rows keep their original package binding.
The candidate uses Memory and SQLite `ProviderStateStore`/`WorkStore` backends.

The reviewed source baseline is `ibm-ims-15.6-tm-contracts-2026-09-11`:
`ims_gucall.htm`, `ims_gncall.htm`, `ims_isrtcalltm.htm`, `ims_purgcall.htm`,
`ims_chngcall.htm`, `ims_tm_plan_terminals_msgsched.htm`, and the pinned I/O PCB
and conversation topics in the TM manifest. The bounded public slice concerns
the message-processing portions of `ibm-ims-15.6-dli-2026-08-31:dli-call-families:0005`
(GU/GN), `:0008` (ISRT), and `:0024` (TERM). PURG and CHNG are supplemental TM
calls here. Authorization, replay, fence, output, and continuation conditions
are internal regressions; no official row is complete. The retained raw HTML and IMS TOC
verified through offline `ibm_docs.py search/read` for this work package.
Database integration, multi-node recovery, conformance, differential,
licensed, and release credit remain pending. The version remains Proposed.

## Remaining work

The lost `ca0c3ddf` and `da05fe4f` metadata source and contract commits
(manager imports `99bc8bd5` and `52584adb`) are recovered as an additive
`mainframe-env.ims-metadata@1` Draft 2020-12 schema and typed provider
validator. The `ims-metadata-contracts` manifest pins ten IMS 15.6
DBDGEN/PSBGEN topics (3,035,971 bytes) under
`ibm-ims-15.6-metadata-contracts-2026-09-11`, with topic-set digest
`sha256:847166149ae438081effe2f09e7a492b9417217d285d48944bafc3b17cb671d2`.
Each pinned body matches retained local HTML. The configured offline reader
lacks the shared IMS TOC, so its topic status, search and read checks remain
unavailable. The manifest has zero coverage credit and no semantic authority.

The contract models DBD organization/version, segments and fields, secondary
indexes, logical relationships, database and alternate-terminal PCBs, PSB
database level, and ordered SENSEG paths. Validation checks names, references,
hierarchy, options, offsets and configured bounds before producing a digest.
The schema records topic paths and hashes per rule group. Schema acceptance
and mutation checks run in `cargo xtask ims-catalog --check` using xtask's
existing `jsonschema` dependency; the provider's Rust validation tests remain
in its crate. The metadata contract does not change the installed-definition
wire shape.

Database host execution, TM application dispatch, and broader recovery remain
for later slices. This work grants no conformance, differential, licensed, or
execution credit.

## IMS metadata package publication

`IMS-1402.package-publication` adds optional `ims_metadata` to signed application
v2 packages. Absent metadata keeps the prior wire form and package identity;
present metadata is validated within package bounds and contributes a separate
identity domain before staging. Publication uses the verified selected package
generation and the existing serialized application publication state. The IMS
provider retains at most 64 package-bound metadata generations per application
through `ProviderStateStore` and atomically advances its selected-generation
row. Exact replay is idempotent, conflicting generations fail closed, an absent
metadata selection clears the previous selection, interrupted `applying` state
recovers, and rollback restores a retained generation before package selection.

Focused application, host-contract, IMS-provider, and server tests cover
validation before mutation, replay, failure, restart, and rollback. The SQLite
reopen case verifies metadata clearing and retained generation restoration.
This slice publishes metadata only; it does not claim DL/I execution, licensed
differential, or release certification.

## IMS-1405 recovery and utilities lane

`IMS-1405.source-corpus` pins a separate, zero-credit scope of 20 retained
IMS 15.6 HTML topics (325,103 bytes) plus the shared IMS TOC. Its baseline is
`ibm-ims-15.6-recovery-utilities-2026-09-11` and topic-set digest is
`sha256:7418508fe1db54bbc8374fc1b8ea4cfd45a3e9aceff75f9734b75cb0afd4bde8`.
All body and TOC hashes reproduced in a flat offline reader cache assembled
from the retained raw SHA archive. The 23 shared reader tests and coverage
check pass. The raw archive lacks HTTP Last-Modified values, which the new
manifest records. Exact basic/symbolic CHKP, XRST, LOG, SETS/SETU, ROLS,
ROLL/ROLB, load, reorganization and database/log recovery topics were read
through `ibm_docs.py`; no source body is copied into Git. This separate cache
does not change the earlier configured-cache findings for other source scopes.

The lane binds frozen `ibm-ims-15.6-dli-2026-08-31:dli-call-families` rows
`:0002` (basic CHKP), `:0010` (LOG), `:0016`/`:0025` (XRST contexts),
`:0017` (ROLL/ROLB), `:0018` (ROLS), `:0020` (SETS), `:0021` (SETU), and
`:0023` (symbolic CHKP). Load, extract, reorganization, and database/log
recovery utilities are supplemental, not official denominator rows.

The next isolated feature slices are `IMS-1405.recovery-contracts`,
`IMS-1405.checkpoint-log-backout`, and `IMS-1405.utility-transitions`.
They must prove bounded images and user areas; CHKP commit with loss of DB
position; XRST's attempted reposition with honest PCB status; named restart
selection without mutation on missing or corrupt images; ordered LOG records;
SETS/SETU and ROLS/ROLL/ROLB backout scope; no duplicate replay; and real
staged utility transitions with digest and corruption checks. Post-dispatch
uncertainty remains `UnknownOutcome` until authoritative fenced observation.
The shared `ProviderStateStore`, effect journal, and UOW contract remain the
storage, replay, and commit authorities. No private store, lock service,
coordinator, scheduler, migration runner, public route, or official gate
credit is introduced by this source slice.

`IMS-1405.recovery-contracts` adds an isolated provider-side version-one
checkpoint image and bounded call contracts. Symbolic CHKP requires prior XRST,
is limited to batch/BMP and seven user areas, and records PCB keys as restart
attempt inputs rather than claiming restored position. Basic CHKP rejects
symbolic user areas. LOG admits codes `A0` through `FF` with a configured
payload bound that fits the two-byte length form. Restart selection, SETS/SETU
point kinds, and utility plans use closed typed forms. Images bind the full
request and committed database identity under a deterministic SHA-256 digest;
unknown versions and modified bytes fail before publication. These contracts
do not dispatch a call, commit a UOW, or grant an official coverage gate.

`IMS-1405.checkpoint-log-backout` adds a version-fenced recovery session row on
the accepted `ProviderStateStore`, not a new persistence service. CHKP seals a
checkpoint with the committed database digest and clears intermediate points;
the caller must publish that proposal with its actual database/UOW commit in
one atomic shared-store batch. LOG creates an ordered, hash-linked record.
XRST is once per execution generation: normal start is explicit, named/last
selection verifies the image, and a supplied database adapter proposes each
qualified-GU PCB update with its observed status. No position is reported as
reestablished without such an attempt. `LAST` applies only to BMP; a 14-byte
timestamp selector is validated but currently returns `Unsupported` because
no authentic timestamp index exists in this isolated engine.

SETS records up to nine bounded intermediate points; repeating a token cancels
later points. SETU with unsupported PCB/external participation reports a
nonfunctional warning while SETS rejects. ROLS restores only explicitly
tracked IMS database and non-express message rows through the shared atomic
batch, retains express messages, returns saved user data, and resets the
caller-visible position obligation. ROLL/ROLB restore the prior-commit
baseline, with termination distinguished. Resource capture requires the caller
to hold the existing shared UOW/lock authority and predeclare all touched
rows; this isolated slice does not acquire locks or route host calls.

Canonical effect intent can be checked before publication; terminal effect
result and uncertain reconciliation remain with the execution coordinator.
A post-dispatch infrastructure failure returns `UnknownOutcome`, never a
negative replay assertion. Focused Memory and SQLite tests cover restart,
replay, corruption, CAS races, atomic failure, message scope, and intent
preconditions. The public IMS route and official row-gate credit remain pending.

`IMS-1405.utility-transitions` adds bounded, typed utility images and log
streams on the same `ProviderStateStore` CAS authority. Initial load validates
the existing deterministic database engine's definition, hierarchy, keys and
indexes, stages an image, then atomically publishes an active generation.
Extract reads the verified active image. Reorganization rebuilds canonical
record order and indexes before publishing a new generation, rejecting GSAM;
it does not claim physical IMS data-set layout equivalence. Database recovery
replays a contiguous digest-linked typed update log over a verified image copy
for one data set, and can replace damaged active bytes only at their exact
shared-store version. It cannot repair application-logic damage or infer
missing log records.

The separate typed DFSULTR0-style log projection handles DUP (including an
explicit truncation LSN and non-usable error markers), REP of every marked
block, CLS of a verified unclosed online input, and read-only PSB membership
reporting. A damaged DUP without an LSN or replacement plan fails closed;
healthy DUP needs neither. Staged database/log outputs remain invisible until
atomic CAS publication; tampered stages, sequence gaps, changed active versions
and invalid hierarchy leave active bytes unchanged. Memory and SQLite tests
cover restart, replay, corruption and conflict. The contracts do not parse
licensed OLDS/SLDS/WADS layouts, dispatch live utility jobs, or attach this
projection to the public database execution route. `UtilityPlan.database`
names the log data set for a `LogRecovery` plan. Work scheduling, canonical
effect terminalization and UOW ownership remain with existing authorities.
The pinned source basis is
`ibm-ims-15.6-recovery-utilities-2026-09-11` in
`conformance/0.14/manifests/ims-recovery-utilities-contracts-topics.json`:
`ims_dfsurdb0.htm`, `ims_logrecovery.htm`, `ims_reorgutil.htm`, and
`ims_hdreorgunload.htm`/reload among the 20 verified topics. This isolated
slice grants no official, differential or licensed gate credit.

## IMS-1406 assurance matrix slice

`IMS-1406.assurance-matrix` is a bounded local assurance slice over the existing
IMS host service, isolated database engine, TM service, recovery session and
utilities, and package metadata publication. Its inputs are the currently
implemented typed provider routes on Memory and SQLite `ProviderStateStore`,
the accepted execution/UOW and SAF contracts, and the frozen
`ibm-ims-15.6-dli-2026-08-31:dli-call-families` catalog. The matrix binds
focused executable tests to SAF denial before caller-visible data or mutation,
malformed/limit rejection, CAS conflicts, replay, process reopen/corruption,
bounded scale, package rollback, and explicit unknown outcomes. Database and
TM execution lanes remain separate; this slice owns only additive tests, the
matrix checker, and this status. No new public route, state authority, runtime
semantic branch, or backend is introduced.

The applicable local gates are matrix identity/schema validation, focused
provider and package regressions, `ims-catalog`, docs, changelog, architecture,
format and clippy. Source review uses the verified IMS 15.6 database, TM,
metadata, programming, and recovery topic manifests; offline HTML is reference
data only. The matrix records pending public database-engine integration,
licensed IMS differential, mixed-resource closure, and full 25-family gates.
This slice gives no official row-gate or licensed credit. The next executable
step is to run these checks on the finished candidate and seal only this slice.

## IMS-1405.public-recovery-bridge

Parent: `IMS-1405`. This slice joins the staged checkpoint/log/backout and
load/extract/reorganization/database-recovery contracts to the generic IMS
database row used by `ImsService` and `ims_providers`. It adds no official
catalog row: the applicable call obligations remain
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0002/:0010/:0016/:0017/:0018/:0020/:0021/:0023/:0025`;
utility transitions are supplemental. The execution context is a selected
signed IMS metadata/package generation with a matching installed DBD, an
authorized IMS database resource, and a canonical effect/UOW intent for
mutation. The affected public route is metadata-driven DL/I database access;
the changed provider boundary is version-fenced image publication and recovery
proposal application. Memory and SQLite reopen, route visibility, corruption,
CAS conflicts, replay, and post-dispatch uncertainty are the local gates.
Licensed differential, remaining call families, and CardDemo remain pending
under their existing milestones. The source baseline is
`ibm-ims-15.6-recovery-utilities-2026-09-11`:
`ims_dfsurdb0.htm` `sha256:c16a37146a59fb6c09f908005035c32a0e974495304ea146f3b27d9fa4965cbb`,
`ims_reorgutil.htm` `sha256:db6c2060b5c0c652fe4638ca092c4455bee3deaa7eee6f898ee3c247ba7d1fa4`,
`ims_loaddb.htm` `sha256:15abf89b0d2e0f9c13e9adf225f08ab5e6552bbfb05e9b64456fd6e5e4d920d1`,
and `ims_rolscall.htm` `sha256:b7e15d0c110d3296eac11d895326b3ef48ac913fd682b94b312aa6c59ad14af5`.
The reviewed boundary reuses the existing generic object row, CAS store,
enterprise SAF resource, and effect/UOW contract; the isolated utility row is
staging evidence only.

The bridge is implemented on the current worktree candidate. The selected
package row and live database object version are CAS-fenced with publication;
the former isolated active utility namespace has no writer or reader in the
new route. Focused Memory and SQLite tests prove normal DL/I visibility,
checkpoint/log publication, ROLS image restoration, damaged-row recovery,
stage and receipt corruption, version conflicts, replay, SAF denial, and a committed write
whose lost acknowledgement returns `UnknownOutcome`. The current IMS package
suite passed 61 unit and 12 integration tests. Package clippy, format, IMS
catalog, IMS assurance matrix, changelog, docs, and dependency policy passed.
The broad runtime architecture command passed its provider-row, effect,
authorization, retention, and durable-storage guards but stopped at an
unrelated CICS source freshness check: retained
`SSJL4D_6.x/applications/designing/dfhp37p.html` is missing. No refresh was
requested or performed; that broad gate is not claimed as a pass.

## IMS-1406.carddemo-corpus-package-route (implemented local slice)

Parent: IMS-1406. Candidate base: `213ed878`; this checkout consumes the
accepted host ABI, SAF and shared store/UOW owners recorded in the COBOL 0.4,
RACF 0.5 and dataset 0.6 progress/evidence authorities. This local integration
profile does not promote those dependencies or claim licensed evidence.
The exact clean CardDemo input is commit
`59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e`, tree
`a1253e31c839f78d1f185b01771ba956da63b005`. All eight IMS definition assets,
the reached DBD/PSB projections, controller program sources and actual
LOADPADB/UNLDPADB JCL are bound into a signed v2 package. Selection,
publication and rollback remain owned by `mainframe-env-application` and
`ProductServer`; metadata, database images, positions and replay by
`ImsService`; SAF by RACF; effects/UOW and persistence by the existing host,
coordinator and `ProviderStateStore`. Tooling owns independent expectations.

Selected routes: public z/OSMF job submission with signed batch controllers,
`ProductServer::ims_execute_selected`, and the existing scoped `ims_providers`
host route. Backends: Memory and file-backed SQLite, including dropping and
reopening the SQLite store. Obligations for this integration profile are exact
segment/status/parent bytes, ordered load/unload and mutations, no mutation on
denial/malformed/replay conflict, package signature/selection/replay/rollback,
and retained data/checkpoint/replay on reopen. Official catalog context is
`ibm-ims-15.6-dli-2026-08-31:dli-call-families` rows
0002 (CHKP), 0004 (DLET), 0005 (GU/GN/GNP), 0006 (hold), 0008/0009
(ISRT/LOAD), 0015 (REPL), and 0024 (TERM). These are supplemental profile
regressions, with no official obligation verdict or row credit: **0/25 pending**.
Licensed certification is excluded by the user; mixed-resource closure stays
in v0.16 and release promotion is outside this slice.

Source review: offline `ibm_docs.py search/read` used IMS 15.6 programming,
database and metadata manifests. Relevant topics are `ims_gughucall.htm`
(`0a9b433d9e38e58c125232a94147acd309e5bbbd7baf3c8cb900a3e8fdf6c8b9`),
`ims_replacecall.htm`
(`55778b03e47f21e92fb967995ec54b7ff1903c7886b98bdb1394d6d85e8fd23a`),
`ims_fieldstmt.htm`
(`94455fbdc9d5f7404c89c0fd73a813fee237fb851fc0d2348a96ba1ce9d36fcb`),
and `ims_psbgensensegstmt.htm`
(`baf8ed5e7ad87faf1ba02801da3479385ba5b4b5fc74474ca051d32d3014ebf4`).
The retained path root lacks these bodies; their SHA archive copies match
manifest hashes and byte lengths and the reader TOC verifies. No network
refresh occurred. This is source review, with zero execution credit.

Boundary decision: reuse signed packages and generic database execution, with
no added dependency or runtime. The existing batch load controller emits the
public two-level `ImsLoadImage`, whereas the generic route accepts explicit
record images. A bounded provider adapter is necessary to resolve that image
from the installed metadata, rejecting ambiguous hierarchies; no application
identity selects behavior. This is the declared exception to tooling-only
ownership. Acceptance: focused corpus/profile regressions, carddemo-ims,
format, docs, changelog and dependency policy. The scoped implementation and local verification are complete; seal this
slice only, then hand it to the integration manager.

The first runtime attempt exposed an unchanged batch parser restriction:
UNLDPADB's exact 14-field `DLI,...,N` launch was rejected before controller
dispatch. The bounded batch launch adapter now shares control validation and
selection, accepting omitted controls and the local disabled-DBRC form only;
enabled or unknown supplied options remain unsupported. This requires the
batch facade, program validator, service delegate and a bounded launch module.
The reviewed source scopes lack the exact DFSRRC00 operational-parameter topic;
offline search reported no matching verified topic. The local profile does
not claim DBRC operational equivalence. The load topic
`ibm-ims-15.6-recovery-utilities-2026-09-11:ims_loaddb.htm`
(`15abf89b0d2e0f9c13e9adf225f08ab5e6552bbfb05e9b64456fd6e5e4d920d1`)
was read offline. Extracting the two tooling/batch functions requires lowering
their exact module-budget inventory counts; no ceiling is increased and the
assurance matrix/checker is unchanged.

The migrated gate installs no legacy IMS definition and no empty Db2 package.
Signed package selection supplies the reached two DBDs, three PSBs and three
PCBs. The package also retains all eight definition sources, all eight IMS
COBOL sources, both actual JCL assets, both source-bound controller entries and
four compiled EXEC DLI artifacts. Existing public JES load/unload controllers
operate on the generic image. Independent expectations compare every ordered
root/child/parent byte, exact GN/GNP/GU results, Get Hold before update, mutation,
compiled typed-DLI output, and the existing hierarchy and spool golden hashes.
Invalid segment insertion now expects the existing generic route's exact `AT`
status and zero mutation rather than the legacy harness's infrastructure error.
The pinned programming database-status table permits AT for ISRT:
`ims_dlistatuscodestables_databasecalls.htm`
(`2b41e1415ac50690e8ace7761263ef304a0ffd512e6d0beee088356fa439aa7f`).
This source-backed status membership is not licensed differential evidence.

Memory and independent file-backed SQLite adapter reopen pass exact package
selection, database/checkpoint preservation and replay checks. Malformed
bulk images and conflicting insert/load identities preserve committed bytes;
interactive UOW rollback and exact retry preserve the image. A separate
subprocess seed/reopen regression verifies metadata/package identity, exact
hierarchy, both index lookups, checkpoint and non-redispatched insert/load
receipts after the seed process exits. This process boundary avoids relying on
retained host-runtime references from an in-process shutdown. Package signature
rejection, publication replay, second-generation publication and retained
first-generation rollback pass on both backends.

Focused verification passes the corpus gate, three load-adapter tests, generic
IMS regressions, batch IMS tests, signed IMS package tests, the process-exit
regression, warnings-denied IMS/batch Clippy, IMS catalog, changelog, format and
dependency policy. Receipts are outside Git and Cargo targets. Broad conformance
Clippy stops at the unchanged `licensed_harness.rs:slot_ids` double-must-use
lint. The broad module guard stops at the unchanged server `product.rs` count
(6,291 versus its recorded 6,008); this slice only lowers its two touched
oversized counts and keeps every new module below 1,200 production lines.
The touched generic provider's unchanged inline tests are moved to its existing
test directory so its production module also stays below the limit. This is
an additional provider scope exception for the module contract, with no runtime
change. Neither unrelated finding is represented as a passing gate. The docs
manifest is regenerated for this appended section. The manager owns integration;
licensed certification remains excluded and **0/25 pending**, mixed-resource
closure stays in v0.16, and no release promotion is claimed.

## IMS-1403.pcb-sensitivity (local implementation slice, 2026-10-02)

Parent: IMS-1403. Entry candidate: `213ed878ec138bdb2914330db6613559bffc5a86`.
The independent current acceptance audit is the external
`v014-completion-20261002/acceptance-audit/audit-report.md`. This slice repairs
the existing typed `ImsRequest` / `ImsService` / `ims_providers` metadata route;
it adds no request shape, store, lock service, coordinator, or participant claim.
IBM-observable selection and sensitivity remain provider-owned semantics.
The existing host ABI, enterprise SAF and provider-row/CAS/replay authorities
are consumed; their historical licensed-pending dispositions remain unchanged.

Exact catalog identities are `ibm-ims-15.6-dli-2026-08-31:dli-call-families:0005`
(GU/GN/GNP), `:0006` (GHU/GHN/GHNP), and selected-PCB hold/update interactions
with `:0004` (DLET), `:0008` (ISRT) and `:0015` (REPL). Basic `:0002` CHKP
and local rollback must invalidate every PCB position; the manager owns the
basic checkpoint regression. Mandatory local obligations: select the requested
DB PCB and SAF database before observation; independent position, parentage and
hold state; explicit insensitive/invalid-PCB/no-read-option conditions with no
protected output or database mutation; targetless GN/GNP data sensitivity;
exact success, AC/AM/GE/GB/GP failure positions; replay without advancement;
rollback, historical readers, corrupt-map rejection and fresh-connection reopen.
Contexts are the existing full-function DB call route with typed DB PCBs and
SENSEG/PROCOPT metadata. Non-DB PCBs and unavailable key-feedback output remain
explicitly unsupported, not required-operation passes. Secondary/composite
access paths, SSA operands and recovery operands are separate slices.

Backends: Memory and SQLite, including a newly opened SQLite connection.
Owners: generic PCB/read/resource resolution, bounded PCB helper/tests, minimal
Session field/reader changes. Manager-authorized shared integration is necessary
in the system database-call observer: status and Q reservations currently use
the scheduled PCB and must use the requested PCB's database and position.
CHKP must retain the manager's commit-before-saving behavior and clear the whole
position map; rollback and database-image resets must clear affected positions.

Source contexts: IMS 15.6 `ibm-ims-15.6-programming-contracts-2026-09-11`:
`ims_imsdbdbpcbmask.htm` (699a551e0c2804db26725d0997be3b1f9fdc91379a69f490c8e509d76fcc61b3),
`ims_gughucall.htm` (0a9b433d9e38e58c125232a94147acd309e5bbbd7baf3c8cb900a3e8fdf6c8b9),
`ims_gnghncall.htm` (063ff108614ee13694ea7df2f7b39647447059590162614da56de2aa2eb49cb4),
`ims_gnpghnpcall.htm` (6daaf5929bf3640a6d4ab17ea97d81328e60b26eb38c32b2c991c77d41592762),
`ims_currentpos.htm` (07aafcddb9b30591eef1da5ad50bfe8bc3c1e9b9e9e51b56b27d39bf6adf5673).
Metadata baseline `ibm-ims-15.6-metadata-contracts-2026-09-11`:
`ims_psbgendlipcbstmt.htm` (0dad54edd1a9940ca9a6988e06412836ff35a706cc2e1f7fbd36eb14fa02cfba),
`ims_psbgensensegstmt.htm` (baf8ed5e7ad87faf1ba02801da3479385ba5b4b5fc74474ca051d32d3014ebf4).
Each manifest SHA and byte count matched the retained raw archive; repository
plain-text parser reads and `ibm_docs.py search/read` also used the existing
ims-1403/ims-1402 reader caches. No network refresh or whole-cache audit.
The pinned status explanation index links the retained IMS 15.6 `msgs/ac.htm`
(46e5eedc042e9d0bd38cb19888095e6ba12a53125ee965794fb6470e8bc7b30b),
`msgs/am.htm` (42b40782940d170e9a7b10d090a22be7ec6470d38ec9ac349c386df14c31e88f),
`msgs/gb.htm` (727e6569e1d5eb64cf06269901547ce0fbeb5649646ad5e7237a4f06a4b050f0),
and `msgs/gp.htm` (2de6e4ba34084fe32a0a0b493f06b11f7ad420d5c37a20b5adaeb0c1e3c6ef8d).
These bounded supplemental archive identities were hash-verified and parsed
offline; they are reference review, not registered execution evidence.

Before implementation: add failing two-PCB and explicit/targetless restricted
SENSEG tests, then repair and verify focused backend/security/replay/reader
regressions, strict package Clippy/fmt and affected mandatory gates. Official
credit remains **0/25** for every gate; local tests and correct forbidden-context
rejections do not close official required execution. Licensed differential and
mixed-resource closure remain pending outside this slice.

Implementation: every DB request selects `request.pcb` for metadata, SAF,
navigation and mutation-position binding. `session.pcb` and `session.position`
retain their historical scheduled-PCB meaning; the bounded `pcb_positions` map
contains only other DB PCBs. Missing historical maps mean no position on those
other PCBs. Readers reject duplicate/noncanonical/zero/out-of-range/alternate
PCB keys, an entry duplicating the scheduled PCB, malformed position/hold
shapes and live dangling occurrences. Historical checkpoint positions may
reference older images, but their map identities and intrinsic shapes are
validated. These remain existing v1 object rows, not a new store or schema
reader. New Q reservations retain an optional PCB identity; historical entries
without it retain their scheduled-PCB interpretation.

Rollback restriction: stop admission and drain sessions, checkpoints, UOWs and
Q reservations before running an older binary, or restore a consistent
pre-upgrade backup including all provider/replay/checkpoint references. Older
readers ignore additive map/PCB fields and cannot preserve other-PCB positions;
live writer downgrade is unsupported. Do not delete maps to simulate migration.

The minimal engine integration supplies a visibility predicate to the existing
navigation selection authority, rather than moving through hidden occurrences
and accidentally retaining their position or parentage on failure. Absent
explicit SENSEG names return AC; incompatible processing options return AM;
both preserve position and emit no protected data. Targetless GN/GNP skip
insensitive and key-only segments. Explicit key-only retrieval establishes
position and suppresses segment bytes; key-feedback output has no existing
typed result field and remains pending, without official execution credit.
GNP parent-qualification mismatch returns GE with unchanged position, while
absent parentage or a target at/above the parent remains GP.

The authorized observer integration binds status, Q reservation location and
modified/current flags to the selected PCB. CHKP retains the manager-specified
pending-undo removal, clears every PCB position and Q reservation, and stores
the post-CHKP session. Basic checkpoint tests remain manager-owned. Rollback
and image replacement clear the affected maps; deletion preserves an unrelated
same-database PCB position while rejecting dangling occurrences. Existing
logical-route fixtures were corrected to explicitly request parent PCB 2;
they had previously relied on the scheduled-PCB selection defect.

Additional source: `ibm-ims-15.6-recovery-utilities-2026-09-11`,
`ims_basicchkpcall.htm` (1029208c47f44b8a0144472c8127767b18140c57be70850fdfa00da83ef1743a):
commit changes and lose position. The position-reset integration also affects
`:0009` (LOAD) under the existing administrative route; no new utility behavior
or utility conformance claim is made.

Verification on the unchanged runtime diff: fail-first tests reproduced both
selection and hidden-segment defects; focused typed-provider Memory/SQLite
tests cover holds, replay, failure positions, SAF-before-observation, rollback,
fresh SQLite connections and historical/corrupt readers. The full IMS package
passes, as do strict scoped Clippy (`--no-deps -- -D warnings`), formatting,
IMS catalog/matrix, spec, changelog, dependency/license/supply-chain policy and
execution/effect/provider-row/durable/security/retention/participant guards.
External command receipts are in `v014-completion-20261002/ims-pcb-sensitivity`.

Aggregate limits are explicit: dependency-inclusive Clippy stops on unchanged
MQ host-API warnings. `architecture-fast` stops on the configured CICS reader
locator for `SSJL4D_6.x/fundamentals/connections/dfht1c0079.html`; the retained
root and SHA archive lack the expected manifest body at 3,031 bytes and
`cb6ff139ab0b7bb2832dc03dd4f53672181657f11d0740a417aab84819aaf254`,
so that exact CICS snapshot is unavailable in the specified local caches.
A separate module-boundary diagnostic finds the unchanged batch service at
7,404 lines against its 7,402-line ceiling; the entry IMS service also already
exceeds its recorded ceiling. No unrelated cache refresh, ratchet weakening or
global suite was performed. The shared official IMS conformance selector still
rejects because its product driver registry is absent; rejection earns no pass.
This slice does not close those manager-owned aggregate gates, the IMS-1403
parent, the 0.14 exit gate, licensed certification or mixed-resource recovery.

## IMS-1403.local-uow-isolation (verified bounded local slice)

Parent: IMS-1403. Candidate: this isolated `v014-cache-audit-20261002`
checkout; consumed baseline `213ed878ec138bdb2914330db6613559bffc5a86`
is the independent acceptance audit candidate. Existing execution, host ABI,
SAF, provider-row CAS, durable storage, retention and participant authorities
remain authoritative; the shared IMS participant descriptor is owned elsewhere.
This slice owns generic database local undo/publication, mutating load and
logical-cascade fences, utility bridge fences and focused Memory/SQLite tests.
It does not change checkpoint semantics, CardDemo adapters, host shapes or
the shared participant contract. The manager's CHKP change commits undo and
resets position; isolation must release its fence when that undo disappears.

Applicable catalog identities are IMS 15.6
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0004/:0006/:0008/:0009/:0015`,
with CHKP :0002 as an integration dependency. Local obligations cover two-run
and two-service same-database rollback safety, fail-closed contention,
authorization/malformed no-mutation, atomic CAS/failure/replay, retained undo
readers and fresh SQLite connections. Interactive local UOWs retain undo;
batch mutation's existing immediate commit remains in scope for fencing.
No official obligation verdict, licensed credit, mixed-resource closure or
release claim is granted. The minimum repair will reuse atomic shared-store
row CAS; it will not add a lock service, coordinator or store.

Source review: offline `ibm_docs.py search/read` with the existing verified
`ims-1405-topic-cache` read recovery baseline
`ibm-ims-15.6-recovery-utilities-2026-09-11`, `ims_rolscall.htm`
(`b7e15d0c110d3296eac11d895326b3ef48ac913fd682b94b312aa6c59ad14af5`)
and `ims_basicchkpcall.htm`
(`1029208c47f44b8a0144472c8127767b18140c57be70850fdfa00da83ef1743a`).
Whole-image local backout must never erase another committed writer; host
contention/uncertainty is distinct from IBM PCB status. Before semantic implementation, the retained/raw archive was also hash-verified
and parsed with the repository reader for programming baseline
`ibm-ims-15.6-programming-contracts-2026-09-11`: `ims_gughucall.htm`
(`0a9b433d9e38e58c125232a94147acd309e5bbbd7baf3c8cb900a3e8fdf6c8b9`),
`ims_gnghncall.htm`
(`063ff108614ee13694ea7df2f7b39647447059590162614da56de2aa2eb49cb4`),
and `ims_processingoptions.htm`
(`bb549e17230c1990ac0b5b5f0386512493bcd5552b3b4762bd7fc4436a923dd4`).
Recovery `ims_rollcall.htm`
(`01a33e88387636985ef575bdd7bdb99e0d3ce6a794dcf227d2f1e75c843ec61f`)
was read in the verified reader cache. Missing programming bodies in that
reader were resolved from the matching external archive; no source was
repinned or fetched.

The implementation retains whole-image undo only while its run owns the
database and declared logical dependencies. Shared database-row CAS advances
on image publication and undo acquisition/release, including an unchanged
image at commit or the manager's checkpoint boundary. A second scan rejects
a mixed database/undo observation; sessions refresh before position mutation.
Load, paired cascade and utility/recovery publication use the same ownership
and CAS fences. Contention returns host `IdempotencyConflict`; a stale or
unwitnessed backout and a lost publication acknowledgement remain
`UnknownOutcome`. Neither becomes a success PCB status or automatic redispatch.
This conservative database/dependency scope is broader than IBM record locks;
it does not prove IBM read isolation, scheduling/wait behavior or lock granularity.

New generic undo values use `mainframe-env.ims-local-undo@2` inside the existing
v1 object envelope/namespace and carry exact post-image SHA-256 witnesses plus
bounded digests of images observed under that UOW's ownership. A bridge
savepoint backout updates the current witness while retaining original undo
and ownership until common local settlement. A proposed image outside those
witnesses fails unknown, including a prior UOW's savepoint after another
writer committed. This is a local safety fence, not full recovery-call closure.
The reader preserves historical plain image maps without rewriting or
promoting their evidence. Existing active legacy undo can be read, but
unproven mutation/commit/backout fails unknown and requires explicit
reconciliation. Drain older writers before upgrade. Old binaries cannot read
active v2 undo: downgrade requires settling/draining those UOWs with this
reader or restoring a verified compatible backup, retaining checkpoint, replay
and recovery references. There is no SQL migration or new retention target.
The participant descriptor owner must declare the v2 writer and retained
plain-map reader; no shared participant acceptance is claimed here.

Four initial deterministic two-run/two-service tests failed on Memory and
SQLite before the repair. Affected-package verification now passes 86 unit
tests, 12 integration tests and zero doc tests, including 16 new isolation
cases. They cover the retained pre-fix committed-B image, legacy/schema
rejection, authorization, malformed load, batch and ordinary mutators, CAS
winners and stale rollback, lost acknowledgement/replay, logical cascade,
utility contention, intermediate savepoint/old-UOW rejection and fresh SQLite
connections. The CHKP test exercises
the manager's intended state publication (undo removal, position reset,
checkpoint retention), not an independently changed checkpoint handler.
Strict scoped Clippy with dependency linting excluded, workspace format,
dependency policy, IMS catalog/assurance, shared spec, changelog and the
execution/effect/provider-row/storage/SAF/retention/participant guards pass.
Dependency-inclusive Clippy stopped at unchanged MQ host validation warnings.
The aggregate architecture gate passed its earlier guards and stopped at the
unchanged CICS `dfhp37p.html` reader-cache gap; no unrelated cache provisioning
or audit was performed and that gate is not claimed as passed. The historical
0.4/0.5/0.6 licensed-pending dispositions remain as recorded; the audit's
missing exact acceptance-provenance mapping is not invented by this slice.

Next integration step: merge this exact sealed slice with the manager-owned
CHKP and participant changes, run the selected public CHKP regression there,
and resolve aggregate baseline gates in their owners. No official IMS row
credit, licensed differential, CardDemo certification, mixed-resource closure
or v0.14 release completion is asserted.

## IMS-1405.application-recovery-dispatch (bounded LOG slice)

Parent: IMS-1405; remains Proposed. Starting candidate is `a130d1ac`, including
manager repair `d5fef29b`. The audit at external
`worker-receipts/v014-completion-20261002/acceptance-audit/audit-report.md`
identifies nine recovery families with no application call adapter. This slice
implements only typed LOG dispatch for a selected, installed generic PSB/database
in DB batch, through the public host provider and existing canonical intent,
RecoverySession and database utility bridge. It does not admit the pending IMS
participant into a shared UOW. The existing recovery row is the log authority;
the live database/session/undo rows must remain unchanged by LOG.

Exact catalog scope: `ibm-ims-15.6-dli-2026-08-31:dli-call-families:0010`.
Mandatory local obligations: owned bounded LOG code/text validation (A0–FF,
including empty/binary text); selected-generation/PSB/database binding; DB-batch
CALL context and Batch invocation validation; mandatory typed SAF before
observation/mutation; exact canonical intent ownership/sequence/digest and
no active recovery lease; ordered persistent log append; exact replay without
duplicate append; rejection/no mutation for invalid/unsupported/unauthorized
requests and missing/conflicting intents; atomic failure/CAS; unchanged database,
PCB position and undo; log survival across local rollback, selected-generation
rollback, and SQLite child-process restart; explicit lost-ack UnknownOutcome.
Expectations are handwritten and invoke the owned public host route on Memory
and SQLite. These obligations are supplemental until the manager binds shared
Conformance IR; no official row passes or licensed credit are claimed.

Owners: additive host-api recovery DTO, HostRequest/HostResult validation and
canonical encoders/exports; additive IMS application recovery adapter and minimal
registration; selected server host composition; focused host/provider route tests.
Reuse the shared ProviderStateStore/IdempotencyStore and existing utility bridge;
no new store, coordinator, dependency, resource-position row, or timestamp index.
DB/DC, DBCTL, DCCTL, TM batch, command syntax and raw language LL/ZZ/AIB framing
remain unsupported in this slice, although LOG is source-applicable there.
PSB work-area and physical log block-size parity remain pending. Provider limits
are bounded local operational limits, not evidence of those IBM physical sizes.

Source: verified offline `ibm_docs.py search/read`, IMS 15.6 baseline
`ibm-ims-15.6-recovery-utilities-2026-09-11`,
`SSEPH2_15.6.0/com.ibm.ims156.doc.apr/ims_logcall.htm`,
`sha256:5106e045b54bca8564d5fc91065ca0dff7b58da4f25d8e09fd11b07d2aada5ca`.
The 20-topic recovery reader cache and TOC verify; no refresh is requested.
LOG writes caller information to the log using an I/O PCB; it neither commits
database work nor repositions a database PCB. The typed projection carries code
and text, excluding raw language framing rather than silently discarding it.

Required local checks: fail-first selected public-route tests, focused host/IMS
tests and server composition compile/tests, strict scoped Clippy, fmt, IMS
catalog, shared spec integrity, affected architecture/security/effect/row/durable
guards, docs/changelog and dependency policy, followed by an exact-path slice
seal/check and one local completion commit. Receipts stay outside Git/target.

Remaining recovery obligations: typed application basic/symbolic CHKP with atomic
real UOW commit and position loss; XRST normal/named/LAST/timestamp selectors and
real qualified-GU attempts on actual session position maps; SETS/SETU and
ROLS/ROLL/ROLB over fenced real database and actual TM rows, express-message
scope and termination/status; all other applicable contexts, raw framing,
participant admission/fencing/audit/retention/backup-restore and shared official
IR bindings. The manager's repaired basic CHKP route is preserved, not relabeled
as this adapter. IMS official credit remains absent; licensed remains 0/25;
mixed-resource closure and parent/release completion remain pending.

The consumed host/SAF/storage ancestry includes `b0258ebb` (accepted integrated
COBOL candidate) and `b4f8fc70` (security/dataset authorities), both verified
ancestors of this candidate. Approval and licensed-pending dispositions remain
in the COBOL execution, RACF security and dataset data status records and
`conformance/0.6/evidence/dataset-certification.json`; this worker does not
rerun or relabel those historical receipts. The current pending IMS participant
preparation at `a130d1ac` remains pending, with no descriptor change.

The I/O PCB success/status basis is additionally
`ibm-ims-15.6-programming-contracts-2026-09-11`,
`SSEPH2_15.6.0/com.ibm.ims156.doc.apg/ims_specifyingiopcb.htm`,
`sha256:d85982dd9c913de3bd6e387717c5e830e24c795b4dad51347f251a898ff18ced`.
The recovery reader lacks that programming body and the retained topic-path file
is absent. Its exact raw-archive body was hash-verified and parsed offline by
the repository's `ibm_docs.plain_text`; the publication is available, not guessed.
No source body, network refresh or whole-cache audit is part of this change.

The implementation uses separate owned logical LOG DTOs and a separate I/O
status result. Both server host compositions register the additive adapter;
the old `ims_providers` constructor still supports its original request surface.
The database/session/undo snapshots and real GN after GU remain unchanged by
LOG. Recovery rows stay `mainframe-env.ims-recovery-session@1`; new run addresses
bind selection/run/principal and effect addresses bind execution/key/sequence.
Provider-row quotas and recovery limits bound retained state. This slice adds
no automatic recovery-log pruning or physical IMS log format claim.

There is no SQL or existing row-schema migration. New and prior canonical
variants have independent golden compatibility tests. Older binaries can read
the unchanged v1 recovery rows but cannot dispatch the new host variants; stop
admission and drain/reconcile new effects before binary rollback. Preserve
recovery rows, selected metadata, canonical journals and audits together during
backup. Coherent backup/restore, retention expiry, concurrent recovery-lease
mutation fencing and participant admission remain unclosed obligations. Checking
an already claimed effect is an admission rejection, not that concurrent fence.

Focused host/IMS and selected signed-package/coordinator tests pass, including
Memory/SQLite failure, CAS, corruption, real position, rollback, replay and
separate SQLite processes. The independent canonical goldens cover new forms
and prior IMS request/result bytes. Strict IMS Clippy passes. Host/server Clippy
diagnostic review finds no warning or error on changed lines, without lint
allowances; whole-package strict commands remain blocked by unchanged MQ
validation and server COBOL/CICS/retention test lints, recorded in the external
handoff. No unrelated lint repair belongs to this slice. The scoped execution,
effect, provider-row, storage, authorization, retention and pending-participant
guards, IMS catalog, spec integrity, formatting and dependency policy pass.
The spec check is integrity evidence only; no new official IMS binding exists.
The final docs/changelog and exact-path seal/check remain the packaging steps;
the external handoff records their executed results and the completion commit.
## IMS-1406.module-boundary-repair (declared infrastructure slice, 2026-10-02)

Entry HEAD: `66e8bce0`. This slice moves unchanged provider-row persistence
helpers, product subsystem facades, and unchanged host dispatch/IMS records into
bounded owned modules. It preserves
public methods, shared storage/CAS authority, publication ordering and retained
schemas. No IBM semantics, dependency policy, licensed admission or coverage
credit changes. The existing oversized-module ceilings must only ratchet down;
new modules remain below the 1,200-production-line limit. Integrate later SSA
selected-route additions into this boundary without retaining duplicate methods.

The first global module check found three additional pre-existing shared
boundary violations in application preflight, interpreter accessors and MQ row
persistence. Move those unchanged helpers into their existing owner boundaries
and remove or lower exemptions; do not raise a limit. The first combined strict
Clippy run found eleven existing server warnings. Apply equivalent expressions,
test-module placement and an internal boxed decoded receipt, without changing
serialized receipt bytes, authorization, replay or retention policy. These are
required verification-infrastructure repairs, not new subsystem semantics.
The complete inventory scan also found one non-exempt TM service above 1,200
lines. Move its unchanged conversational helpers into a bounded child module;
keep settlement, message and shared work-store behavior intact.
Strict MQ verification then exposed four existing equivalent-expression/test
initialization warnings. Repair those without changing message bytes, delivery
contracts or limits; run the affected delivery cases and retain earlier passing
row-persistence results rather than repeat an unchanged full package suite.

Verification: affected IMS and product route regressions, module-boundary guard,
warnings-denied scoped Clippy, formatting, dependency policy, docs and changelog.
Preserve receipts outside targets and clean the intended Cargo target after the
sequence. Seal only this infrastructure feature, not the parent IMS milestone.

Executed verification: the global module guard passes across 795 production
modules. Application package tests (15), MQ row/package tests (59 unit plus 10
integration), the changed machine-output regression (1), TM runtime tests (8),
server replay tests (12), retention safety tests (3), signed IMS package tests
(10), changed MQ delivery cases (11), and compiled VERIFY TOKEN (1) pass.
The earlier host (104 unit plus 11 integration) and IMS (107 unit plus 27
integration) results cover the moved host/provider helpers; those inputs were
not changed again. Strict five-package all-target Clippy with `--no-deps`
and `-D warnings`, formatting, dependency policy, changelog and regenerated
docs checks pass. Preserve the initial module/lint failures in the external
`v014-completion-20261002/module-boundary-*.log` receipts; the final gate log
records the repairs, and all sequences ended with Cargo cleanup. This gives
no new official or licensed IMS credit and does not close the parent release.

### Public SSA manager integration

After `aaf3c2a7`, selected SSA dispatch is folded into the existing bounded
`product/ims.rs`, the additive canonical arm into `canonical/dispatch.rs`, and
original host validation/provider factory helpers move unchanged into bounded
child modules. Both LOG and SSA use the same factory and retain their distinct
canonical identities. Request/service module ceilings ratchet lower; no removed
helper or duplicated selected method remains. Original worker receipts and
aggregate failures remain historical. Combined-candidate checks and its exact
re-seal are recorded separately in `v014-completion-20261002/ssa-integration.log`,
not relabeled worker evidence.
The local assurance checker also follows the moved `ImsOperation` enum to its
new source file; handler names, executable-test requirements and zero-credit
disposition are unchanged. Its initial stale-path failure and passing runtime
checks are retained in `ssa-integration-initial-failure.log`.
## IMS-1403.secondary-access-paths (bounded local slice, 2026-10-02)

Parent: IMS-1403; entry HEAD: `2f6a8d8e`. The external acceptance audit is
`v014-completion-20261002/acceptance-audit/audit-report.md`. Preserve integrated
checkpoint, selected-PCB sensitivity, UOW ownership and provider-row CAS repairs.
This slice owns engine index definition/validation, composite source bytes,
source-to-self/ancestor target resolution, maintenance and selected secondary
navigation through the existing provider. It adds no engine, store, coordinator,
host request/canonical shape, SSA parser or rich-predicate authority.

Inspection correction: this entry's metadata DB PCB has no `secondary_index`
field (the legacy database definition does). Minimal shared edits add the
optional PCB selector, its schema/reference validation and constructor defaults;
absent selectors retain existing serialization and metadata digest identities.
Engine descriptor extensions likewise omit default fields. Selected nonroot
target processing requires restructured hierarchy/alias metadata unavailable in
this shape and is rejected before installation; index maintenance/lookup can
resolve an ancestor target without equating source with target. Fast Path
secondary processing is outside this full-function slice and rejected explicitly.

Catalog rows: `ibm-ims-15.6-dli-2026-08-31:dli-call-families:0005/:0006`
(GU/GN/GNP/Get Hold), `:0008/:0015/:0004` (ISRT/REPL/DLET), with existing
local commit/rollback/checkpoint and replay contracts as recovery guardrails.
Mandatory local obligations: ordered concatenated field bytes; target occurrence
resolution; independent primary/secondary PCB position/parentage/hold; indexed
qualification using the XDFLD name; maintenance on primary and selected updates;
exact GE/GB/GP/AK and validation failures; no mutation on invalid definitions,
authorization/context rejection or conflict; replay/rollback/CAS and fresh
Memory/SQLite reopen. Regressions must fail before implementation. Owners are
bounded database and generic helper/test modules, minimal metadata/schema and
shared constructor edits, this status, one fragment and routine docs manifest.

Source review uses hash-verified offline IMS 15.6 database-contracts baseline
`ibm-ims-15.6-database-contracts-2026-09-11`, topics
`ims_secondaryindexlogicalrelationships.htm` (e924f4e2c9c336d4ecdc8a6e2ccf4ff0369b0ab2332788ca015e215c14d5bd53),
`ims_ssassecondaryindex.htm` (4b3a1ee3cc0eabbbb4984d23a0eb60fe93be50887e1853643e0f382710139900),
`ims_howhierrstruc_fullfunction.htm` (8535859c6dfc8e9683b34d307535bc524321cdc445bb6b94c1143d8d0206d1ca),
and `ims_howseindexmaint.htm` (910d3494d8be6d834eb24844d86153ce675f67566fd49a455056d8da230cb086),
plus metadata PCB/XDFLD and programming positioning topics below. Retained
topic paths are absent for the four database topics; exact SHA archive bytes and
reader cache match the committed hashes/counts and repository parser reads.
No refresh or publication bodies in Git; source review earns zero credit.

Acceptance: focused selected-provider/engine/index/PCB/security/replay/rollback
regressions on Memory and SQLite; strict scoped no-deps Clippy, fmt, affected
metadata/catalog/schema generators, dependency policy, docs/changelog and
applicable architecture guards. External receipts remain outside Cargo targets.
Seal only this ID with target 0.14.0. Parent, official and licensed completion
remain pending; organization admission alone does not prove its semantics.

Implementation: the existing engine stores source-occurrence pointer identities
under concatenated search bytes and resolves each pointer to the declared self
or ancestor target. Search lists are bounded to five fields / 240 bytes and
validate ancestry, references, duplicate fields and XDFLD/physical-field name
collisions before admission. Insert, replace, subtree delete and restore share
that construction. A partial composite fails before image mutation; historical
single optional-field descriptors retain their prior behavior. Selected sequences
require search fields available at the source's minimum length. The selected
physical-root sequence uses the same hierarchy/matching/hold authority, with a
bounded per-PCB index/source cursor to distinguish repeated target pointers.
Indexed target replacement/search-byte changes lose selected parentage;
selected target insertion/deletion returns AM without changing the image.

Shared integration is limited to the optional metadata PCB field/schema,
constructor defaults in host/server/participant/CardDemo fixtures, one logical
delete position initializer, generic PCB reader validation and metadata
publication admission. Existing `ImsRequest`, canonical effects, checkpoint
commit/reset, PCB sensitivity, undo and provider-row atomic/CAS owners remain.
The assurance checker found fourteen pre-existing stale generic-test file
locators after the integrated test extraction; these now name the existing
`service/generic/tests/mod.rs`. No case, operation mapping, count, credit or
checker criterion changes. The audit's limits on what those local bindings
actually prove remain applicable.

Compatibility: valid historical metadata without selectors, single-field index
descriptors, images and plain positions preserve exact serialization/identity.
Extended descriptors write `source_field` instead of the old required `field`:
old engine readers therefore reject instead of ignoring extension fields and
misinterpreting composite keys/targets. New readers accept both bounded forms.
Prior metadata/position readers reject selector/cursor fields. Drain older
writers and preserve a coherent pre-feature backup of selected and retained
metadata/package generations, images, sessions, checkpoints, undo and replay
references before admitting extensions. Prior-binary rollback restores that
backup and referenced artifacts; stripping fields from live rows is forbidden.
Previously metadata-only admissions of impossible shapes are not reinterpreted
as executable access paths. These are local compatibility tests, not a complete
backup/restore or mixed-resource certification claim.

Additional reviewed pins: `ibm-ims-15.6-metadata-contracts-2026-09-11`,
`ims_psbgendlipcbstmt.htm` (0dad54edd1a9940ca9a6988e06412836ff35a706cc2e1f7fbd36eb14fa02cfba),
`ims_xdfldstmt.htm` (1954204faddc7fc6f55e5942342c3edbd2114781c136e4c0276794dc1818fce8);
`ibm-ims-15.6-programming-contracts-2026-09-11`,
`ims_gnpghnpcall.htm` (6daaf5929bf3640a6d4ab17ea97d81328e60b26eb38c32b2c991c77d41592762)
and `ims_currentpos.htm` (07aafcddb9b30591eef1da5ad50bfe8bc3c1e9b9e9e51b56b27d39bf6adf5673).
The exact SHA archive bodies and repository parser verified all eight selected
pins. `ibm_docs.py search/read` used the existing 1403/1402/1401 reader caches.
The external `secondary-access-paths/offline-sources.json` retains only bounded
identities, hashes, counts and zero-credit source-review provenance.

Local verification passes focused engine, selected-provider/generic/PCB,
metadata, participant, context and signed-package compatibility tests, fresh
Memory/SQLite readers, replay/rollback/CAS and denial-before-observation/mutation.
The conformance consumer compiles. Strict IMS-only Clippy uses
`--all-targets --no-deps -- -D warnings`; formatting, catalog generator/check,
shared spec, deny, changelog, license/supply-chain and execution/effect/row/
durable/security/retention/participant guards pass. Receipts remain in the
external `v014-completion-20261002/secondary-access-paths` directory. Zero-test
filtered binaries supply no credit. Runtime checks are pre-seal local runs,
not relabeled official candidate receipts.

Aggregate blockers are unchanged: selecting the whole shared host-API package
for strict Clippy finds MQ `mq_validation.rs` collapsible-if/manual-contains
lints; the module guard finds server `product.rs` at 6,291 versus its 6,008-line
ceiling. Touched production modules remain below 1,200 lines. `architecture-fast`
stops on CICS `SSJL4D_6.x/fundamentals/connections/dfht1c0079.html`: the committed
3,031-byte pin `cb6ff139ab0b7bb2832dc03dd4f53672181657f11d0740a417aab84819aaf254`
is absent at the retained topic path and SHA archive path. No refresh or gate
weakening occurs. Nonroot secondary restructuring, Fast Path processing,
NULLVAL/exits, SUBSEQ/pointer user data, rich SSA operands, PostgreSQL-specific
selected-path execution, official and licensed gates remain outside this slice.
The manager owns integration and aggregate closure; no parent completion,
organization-wide equivalence, release promotion or licensed credit is claimed.

### Secondary access manager integration

Integrate the existing UOW image witnesses and selected-PCB helpers rather than
replace their publication authority. The source review of
`ims_ssassecondaryindex.htm` (pin `4b3a1ee3...`) requires loss of parentage when
the selected XDFLD changes, not on every target REPL. Add a failing unchanged
index/ancestor-target parentage regression before repairing that edge. The rich
SSA route must reject an unimplemented selected-secondary combination before
mutation instead of silently performing a primary-order read. Add a public
regression for that honest boundary; a rich SSA/index bridge remains pending.
Receipts and exact integration re-seal stay separate from original worker runs.
The first combined run passed 129 unit cases and failed three inherited fixture
assumptions. Update unchanged-XDFLD parentage expectations and force the session
CAS race after the shared UOW refresh, before atomic publication. Do not remove
the conflict/no-database-mutation assertion or weaken fresh-reader behavior.

The repaired integrated runtime passes 132 IMS unit tests and 27 integration
tests, six selected metadata tests and 11 signed IMS package tests. Strict
three-package Clippy, the global module ratchet, IMS assurance and docs checks
pass in `secondary-integration-fixed.log`; that receipt is not the original
worker candidate. `secondary-integration-red.log` preserves both intended
runtime failures before repair. A test-only store shim forces the session CAS
race after refresh and before the real atomic store batch on both backends.
The metadata/schema and local feature seal remain zero-credit preparation for
official IR and licensed equivalence. Rich SSA with a selected secondary index
remains explicitly unsupported, not a successful primary-order substitute.
## IMS-1406.q-reservation-write-fence (declared bounded slice, 2026-10-02)

Parent: IMS-1406. Entry HEAD: `2f70b82f` (integrated UOW isolation, PCB
sensitivity, checkpoint and CardDemo repairs). Consumes the recorded released
0.4/0.5/0.6 dependency identities above and the existing host ABI, SAF,
canonical effect, durable storage, provider-row/CAS and replay authorities.
Owns a bounded reservation helper and minimal system/generic/isolation/utility
call-site integration; SSA navigation, secondary-index projection and application
LOG/recovery DTOs remain other workers' scope. No private lock/store/coordinator.

Catalog: `ibm-ims-15.6-dli-2026-08-31:dli-call-families:0003` (DEQ),
`:0005/:0006` (Q Get/Get Hold), `:0004/:0015` (DLET/REPL), `:0008/:0009`
(insertion/load publication) and `:0002` (checkpoint release integration).
Public routes are typed `ImsService::execute` / `ims_providers` and the existing
utility image publisher, full-function metadata and retained legacy reservations.
Mandatory local obligations: deterministic two-run/two-service fail-first on
Memory and file SQLite; ordinary updates, record granularity and cascades;
bulk/utility invalidation; acquisition/write CAS races and stale-row refresh;
owner update and per-PCB current/modified tracking; DEQ rules; checkpoint,
commit/rollback/termination release; authorization/no mutation, replay, retained
readers and SQLite restart. Host contention must not become an invented IBM
PCB result. Inspect pending-undo integrity reads separately from PROCOPT O and
record remaining obligations. No official row credit, parent seal, licensed
campaign, network refresh, push, PR or mixed-resource closure is authorized.

Acceptance: focused failing/passing regressions, strict scoped Clippy
`--no-deps -- -D warnings`, fmt, deny, affected IMS/spec/shared architecture
guards, docs/changelog; exact allowlist slice seal and check. Preserve external
receipts and clean Cargo targets after each command sequence. Next step:
offline pinned Q/DEQ/update/syncpoint review and deterministic reproduction.

Implementation: `service/system/reservations.rs` is a bounded helper on the
existing SystemState. Database refresh includes the authoritative system row
before undo verification; Q acquisition/release/current/modified changes
advance the existing database-row CAS, including unchanged image bytes. The
same atomic publication contains session, reservation, undo and replay changes.
Two concurrent acquisitions/writes cannot both publish observations from the
same database version. Acquisition also rejects another run's pending undo.
Image publication compares existing segment identities, versions, hierarchy and
bytes: a root Q fences its database record, a dependent Q fences that segment,
and deletion/cascades cannot remove a reserved dependent. Other roots and
unreserved sibling segments remain writable subject to the pre-existing
database/dependency UOW fence. Bulk image load and utility replacement reject
active reservations, including the owner's, because they can reassign occurrence
identities. A utility staged before a reservation/release must be restaged at
the new CAS version. No new private lock service or store is introduced.

Owner mutation marks the affected reservation modified even through another
PCB or a cascade; reacquisition cannot clear that mark. Successful navigation
within the same database record retains the position-required flag. Requested
PCB identity and historical missing-PCB scheduled-PCB fallback are preserved.
The existing observer releases reservations on CHKP, commit, rollback and
termination. Exact replay neither reacquires a released Q nor releases a new
reservation on replay of an older settlement. Commit/rollback authorization
now includes reserved databases even when there is no pending undo. The legacy
location reader and legacy write/load route receive minimal matching fences;
their other historical semantics are unchanged. Contentious updates return
host `IdempotencyConflict`; lost publication acknowledgement stays
`UnknownOutcome`, with retained authoritative replay and no automatic redispatch.
MSDB Q is explicitly unsupported without publishing data or position changes.

Source review is offline. The existing verified `ims-1403-topic-cache` supports
`ibm_docs.py search/read` for programming scope; the retained path root lacks
these exact bodies, so the raw SHA archive was checked against manifest hashes
and byte counts and read with the repository `plain_text` parser. Exact sources:

| Baseline / topic | SHA-256 |
|---|---|
| `ibm-ims-15.6-programming-contracts-2026-09-11`, `ims_comparingcmdcodesandopts.htm` | `eec570be49de991fe17b672c5e81dc4a83733d66267d3e0b561502e8820854ec` |
| Same programming baseline, `ims_gughucall.htm` | `0a9b433d9e38e58c125232a94147acd309e5bbbd7baf3c8cb900a3e8fdf6c8b9` |
| Same programming baseline, `ims_processingoptions.htm` | `bb549e17230c1990ac0b5b5f0386512493bcd5552b3b4762bd7fc4436a923dd4` |
| `ibm-ims-15.6-database-contracts-2026-09-11`, `ims_replacecall.htm` | `55778b03e47f21e92fb967995ec54b7ff1903c7886b98bdb1394d6d85e8fd23a` |
| Same database baseline, `ims_issuedeletecall.htm` | `f40ecf698e4817f47ca8a4abd5214baa880476393c4f765a5c215051acdc6a23` |
| `ibm-ims-15.6-recovery-utilities-2026-09-11`, `ims_basicchkpcall.htm` | `1029208c47f44b8a0144472c8127767b18140c57be70850fdfa00da83ef1743a` |

The archive's IMS 15.6 topic metadata also supplies bounded supplemental
identities: `ims_qcmdcode.htm` (9,960 bytes,
`e5af5bd0b7a2a631e0db6600fc0464428bda15cb1d5364b74cfe5fc7b8559eae`),
`ims_reservingsegments.htm` (3,791 bytes,
`4d3967707752564dcc2193e8003341bfdd63a2a2d5358ba8d5f9db210830fbbe`),
`ims_lockqcommand.htm` (1,863 bytes,
`e9127601febd8f76a8e5a36d3f761ac62be74dcbd67deefb2ed3c45aabe92f2d`),
and the previously recorded `ims_deqcall.htm` (17,885 bytes,
`ece3fcc632ba0b1cfa68836d8ab30cadf8081afbdb7d626ef26cc4d337728990`).
All four exact archive bodies hash-verified and parsed locally. Direct Q and
DEQ topics are absent from the registered manifests, so exact reader requests
remain unavailable there; these supplements are not silently promoted to
registered pins or execution evidence. The registered comparison topic binds
Q/LOCKED to update protection; supplemental Q topics distinguish root-record
versus dependent-segment scope and DEQ's class, modified and position exceptions.
No body is copied into Git, refreshed, or treated as licensed evidence.

Integrity-read inspection remains a separate obligation. Programming
`ims_processingoptions.htm` and supplemental `ims_reservingsegments.htm` say
that integrity readers must not observe another program's uncommitted altered
data; GO is the explicit read-without-integrity exception. The current generic
read route restores the live database image and selects by SENSEG/PROCOPT but
does not consult pending undo for ordinary Get/Get Hold. G and GO therefore do
not yet have distinct pending-undo visibility fences. This write repair does
not supply full transaction isolation, ordinary read/update lock coexistence,
root-Q entry restrictions, shared Q holders, waiting/deadlock scheduling, MAXQ
call accounting, physical block/CI data-sharing granularity, or complete Fast
Path DEQ buffer/FW semantics. Those source-backed obligations stay pending;
the existing conservative database-wide undo fence is also broader than IBM
record locks. Mixed-resource ownership, recovery-lease epochs, audit composition
and licensed equivalence retain their shared participant blockers.

Manager integration preserves the rich SSA/selected-secondary boundary and
the existing witnessed UOW publisher. Q checks therefore use the same common
fresh-call observer and atomic row changes, not an independent reservation map.
Move the unchanged legacy load/unload helpers into `service/legacy_load.rs` so
the new common fencing hooks lower, rather than enlarge, the frozen facade
budget. Original worker checks remain historical; combined verification and
exact re-seal are recorded separately in `q-integration.log` and its seal log.
The combined runtime passes 18 reservation tests (including child processes),
23 selected-PCB/secondary tests, 11 rich SSA tests and 10 application LOG tests;
strict IMS Clippy, the global module ratchet and IMS assurance/docs checks pass.
Legacy reservation/load checks exercise the extracted load helper through the
public route. These are current integrated checks, not official row coverage.

Compatibility: the v1 system/object rows, reservation key shape, optional PCB
field, canonical request/replay domains and namespace names are unchanged.
No SQL migration, new durable schema, or eager reader rewrite is introduced.
Earlier reservations without PCB identity remain readable and fenced. This
cannot undo updates already admitted by an older binary or relabel historical
successful replay receipts. Stop admission and drain/reconcile sessions,
reservations and UOWs before upgrade or binary downgrade; alternatively restore
a consistent compatible backup with all database/checkpoint/replay references.
An older writer does not enforce this CAS/reservation protocol, so mixed writer
versions and live downgrade with active reservations are unsupported. The
inherited v2-undo downgrade limits still apply.

Verification receipts are external at
`v014-completion-20261002/ims-q-reservation-write-fence`. The corrected
fail-first receipt shows REPL bypass on all four Memory/file-SQLite two-run and
two-service cases (the initial two-service attempt instead exposed a stale
Get-Hold CAS failure). Passing scoped regressions exercise the public
`ims_providers` write route, root/dependent and logical-cascade scope,
authorization and malformed no-mutation, modified/PCB and class rules,
settlement replay, forced acquisition/write CAS ordering, atomic failure,
unknown acknowledgement, retained missing-PCB readers, and three separate
SQLite seed/resume/verify processes. An earlier candidate's full IMS package
suite passed (121 unit, 17 integration and zero doc tests). The final runtime
candidate passes all 18 focused reservation regressions and strict scoped
Clippy; the added legacy/context checks also cover identical-byte REPL,
root/dependent acquisition overlap and same-record DEQ position retention.
A three-level case permits foreign insertion/deletion below a Q-reserved
dependent while root Q still fences that descendant publication; engine-owned
child-list changes alone do not modify the reserved dependent segment.
Final affected policy/generated gates are recorded below when complete. No licensed,
PostgreSQL, CardDemo-full, global cache audit, or release campaign is run.

Small shared integration needs: retain the original provider-row helper names
when the manager extracts `service/rows.rs`; this slice calls them unchanged.
The service facade grows only by reservation refresh/check/authorization/load
integration. Its entry count was already 2,109 versus the 1,959-line recorded
ceiling; the manager owns that split, and this slice does not raise the ceiling
or perform the extraction. New production modules remain below 1,200 lines.
The routine docs manifest must reflect this appended section. No official row
passes, IMS-1406 parent completion or full-minor seal is claimed.

The IMS assurance matrix initially failed because the integrated generic-test
split left 14 local-gate locators at `service/generic.rs`. This slice corrects
only those paths to the executable tests in `service/generic/tests/mod.rs`;
test names, source/catalog bindings, schema, pending dispositions and credit
remain unchanged. The final production counts are helper 381, system 1,036,
generic 934, isolation 376, utility bridge 581 and facade 2,122. The repository
module ratchet stops first at unchanged server `product.rs` (6,291 versus
6,008); the facade's pre-existing IMS ceiling issue remains for the manager's
separate extraction. No budget is raised and that global gate is not passed.

Final scoped acceptance: 18 reservation tests pass on the final runtime
candidate; 14 existing PCB tests and two existing system-family tests passed
before the final helper-only child-list refinement. Commands:
`cargo test -p mainframe-env-ims reservation_ -- --nocapture`,
`cargo test -p mainframe-env-ims service::generic::tests::pcb_tests`,
`cargo test -p mainframe-env-ims system_families_replay_and_restart`,
`cargo clippy -p mainframe-env-ims --all-targets --no-deps -- -D warnings`,
`cargo fmt --all -- --check`, `cargo deny --offline check`,
`cargo xtask ims-catalog --check`, `cargo xtask ims-assurance-matrix --check`,
`cargo xtask spec --check` (134 policy tests) and
`cargo xtask changelog --check`. The execution-route, effect-encoding,
provider-row, storage-profile, enterprise-authorization, retention-lifecycle
and transaction-participant Python guards pass, as does
`python3 -B tools/generate_transaction_participant.py --check`.
Cargo runs use the pinned toolchain, the authorized PATH prefix and
`CARGO_NET_OFFLINE=true`; deny uses retained advisory data without refresh.

Final documentation/sealing command receipt: `final-docs-seal.log` under the
external slice receipts directory. The routine generator/check is
`cargo xtask docs` / `cargo xtask docs --check`. The seal uses the exact staged
file allowlist with `cargo xtask work-package-seal --id
IMS-1406.q-reservation-write-fence --target-version 0.14.0` and the same paths
with `--check` after local commit. It is a bounded slice seal, not an official
row or parent seal. `git diff --check` and Cargo cleanup complete each sequence;
no publication bodies, SQL schema, participant descriptor, LOG adapter, SSA
navigation or secondary-index contracts are changed. The next substantive
obligations are integrity-read visibility and the pending lock-manager semantics
listed above; the manager separately owns facade extraction and aggregate gates.

## IMS-1405.application-checkpoint-restart (declared bounded leaf)

Parent IMS-1405 remains in progress. Exact clean entry candidate:
`66e8bce0a4fa97daa057da61664a4da1816faecf`. Preserve its LOG adapter,
per-PCB positions, checkpoint commit/reset and witnessed local UOW fences.
This leaf connects basic CHKP, symbolic CHKP and XRST to the existing typed
selected host route, RecoverySession and atomic provider-row bridge. No new
engine, coordinator, store, participant admission or TM backout is owned.
Catalog scope: `ibm-ims-15.6-dli-2026-08-31:dli-call-families:0002/:0016/:0025`,
with symbolic CHKP `:0023` as required context. Supported execution projection:
DB-batch CALL on installed selected signed metadata, all its real DB PCBs.
Other source-applicable contexts/command/raw language adapters remain pending;
their rejection is not execution credit. SSA/GSAM future DTOs, secondary
metadata/indexes and ordinary-write Q fencing belong to other owners.

Obligation classes: exact logical operands/selectors and context/order errors
without mutation; mandatory SAF over every affected database; real undo commit
and all-PCB position/hold/Q release; bounded seven-area save/restore; actual
qualified GU and GN continuation on retained Session rows; canonical identity,
atomic CAS/failure/replay, process-exit recovery and explicit ambiguous/lost-ack
observation. Independent tests exercise Memory and SQLite public host and
signed selected-package/coordinator routes. They are supplemental, with no
official row credit until the shared accepted Conformance IR owner binds them.
Licensed differential remains 0/25, excluded by the human for this continuation.

Necessary small shared edits: additive host recovery enums/exports/validation
and canonical encoders; defaulted retained Session recovery-order metadata;
crate-private visibility for existing PCB/session helper calls; a private
existing-bridge hook to publish session/undo/recovery rows in one batch; bounded
RecoverySession adapter methods using its existing state/replay authority.
Tests, this appended status, a unique fragment and routine docs manifest are
owned. No other owner's private authority or generated inventory is edited.

Source review uses verified offline `ims-1405-topic-cache` search/read:
`ibm-ims-15.6-recovery-utilities-2026-09-11`, basic CHKP
`1029208c47f44b8a0144472c8127767b18140c57be70850fdfa00da83ef1743a`,
symbolic CHKP `87ede5820b177ea850473dda852c424a7b4a8f5a93dd06c68a0b95c11a4093b5`,
XRST `aff46320869b8910ab9916011c8d722a5e044f9e33970d2996e0501204046cb6`,
checkpoint introduction `c3aaf84e538be688d44af8fe9072e6cb00c47e4f9e46277f2ba22aa053be013d`,
restart command context `ac6ec41de956052fe05cb68efd58b59793af4c0903458cf447852b93b0211670`.
Topic paths are the corresponding APR/APG entries of the committed manifest.
CALL XRST is once and before CHKP, but need not precede database calls;
command XRST is first. A program uses only one checkpoint kind. Timestamp
selection requires the authentic DFS0540I `IIIIDDDHHMMSST` identity; existing
logical clocks lack region/day/time-of-day authority. That concrete context
boundary remains unsupported/pending, never synthesized from ticks or supplied
by the caller. LAST is BMP-only and remains outside this DB-batch route.

Acceptance: fail-first/pass focused host/provider/server and child-process
checks, warnings-denied scoped Clippy with --no-deps, fmt, affected shared
effect/provider-row/durable/schema/catalog/participant/security/retention guards,
spec integrity, mandatory dependency/docs/changelog gates, exact leaf seal/check
and one local completion commit. Receipts remain outside Git/target; no source
refresh, licensed run, orchestration, push or PR is authorized.

The implemented DB-batch typed projection now uses RecoverySession's existing
checkpoint and replay maps, not a parallel engine. Its private bridge atomically
publishes real Session positions, witnessed undo removal, Q release, recovery
receipt and selected-metadata/database CAS fences. Canonical replay observes the
prior receipt before inspecting later work, so it cannot commit that later UOW.
Named XRST issues qualified GU against current database images and retains the
actual per-PCB cursor for GN; missing keyed segments report GE and continue
after the deleted key. Normal start is a distinct non-restoring selector.
The provider derives PCB numbers, database identities and bounded key recipes;
callers cannot submit resource mutations or snapshot namespace identities.
Read-only observation supports explicit fenced reconciliation of lost acknowledgments
and leaves absent/ambiguous receipts unresolved without redispatch.

Additional offline position/metadata basis:
`ibm-ims-15.6-programming-contracts-2026-09-11`, APR
`ims_gughucall.htm` and `ims_gnghncall.htm`; and
`ibm-ims-15.6-metadata-contracts-2026-09-11`, SUR
`ims_fieldstmt.htm`, `ims_psbgendlipcbstmt.htm`, `ims_psbgensensegstmt.htm`.
Each manifest SHA-256 was verified before repository-parser review. The latter
bodies were available in the content-addressed raw archive despite absent
retained topic-path files; no source mismatch or refresh occurred. Exact full
paths/hashes and selected execution results are in the external leaf handoff.

Focused public host/provider, signed-package/coordinator and separate-process
regressions pass. Independent canonical framing preserves the old LOG and IMS
goldens. Strict host/IMS Clippy, formatting, dependency policy, affected shared
architecture/participant/security/retention guards, schema/catalog/spec integrity
and changelog checks pass. The required IMS assurance check exposed stale
pre-existing generic test paths; only their binding locators were repaired,
without changing expectations, applicability or zero credit. Server diagnostic
review finds no warning/error in changed files. Whole-server strict Clippy and
the aggregate module-budget guard remain blocked by unchanged owner code;
no lint allowances, inventory ceiling increase or unrelated repair is claimed.
Changed production modules stay below the ordinary limit, with the existing
service facade held at its entry size. Docs generation and exact leaf seal/check
are packaging steps recorded by the external handoff, not official IMS credit.

No SQL or row-schema migration is introduced. Prior Session rows default missing
recovery-order fields; prior RecoverySession rows retain their digest and replay
encoding. Existing per-PCB readers and version-two witnessed undo remain intact.
New application result bytes use a private bounded prefix in the existing replay
map. Older binaries cannot dispatch the new canonical variants or reliably
enforce the new order markers, and their former replay-data bound may reject large
new area receipts. Before downgrade, stop admission, drain/reconcile new effects
and UOWs and use a coherent pre-upgrade backup containing database images,
sessions, recovery/checkpoints, selected metadata, journals and audits.
There is no automatic pruning; configured row/recovery bounds still reject
saturation. Coherent restore and retention expiry are separate pending obligations.

Remaining source-applicable work is explicit: authentic timestamp/context
authority; BMP/LAST and other execution contexts; command/raw language/JCL CKPTID
precedence; GSAM RSA/file restoration; the currently unsupported deleted-key
continuation for non-key-ordered roots; and raw PCB feedback. Nonunique/keyless
paths receive no successful reposition claim. SETS/SETU/ROLS/ROLL/ROLB real
database/TM backout, atomic concurrent recovery-lease fencing, participant
admission, official accepted-IR bindings and licensed differential remain pending.
The inherited accepted shared-contract identities and prior licensed-pending
dispositions are unchanged. This seals only the verified bounded leaf; IMS-1405,
the full recovery family and the human's v0.14 goal remain in progress.

### Application checkpoint manager integration

Fold this route into the current Q/SSA/secondary candidate without changing
their UOW or provider authorities. Extract the existing state/definition
validators into `service/validation.rs`, retaining their parent exports and
lowering the exact facade budget. New defaulted execution markers and actual
session/undo/recovery atomic changes remain in the existing row codec.

The pinned XRST contract (`ims_xrstcall.htm`, `aff46320...`, lines 166–200)
requires the saved PCB's actual access sequence, not a primary-order substitute.
The initial public regression demonstrated symbolic CHKP accepting an indexed
cursor into a physical key path. Until secondary checkpoint capture/resolution
is implemented, an established selected-secondary position returns Unsupported
before checkpoint publication; primary positions and basic commit behavior
remain supported. The retained-position reader likewise fails closed rather
than resolving that PCB through the primary engine. This is an explicit pending
obligation, not secondary restart credit. `checkpoint-secondary-red.log` records
the intended runtime failure separately from the earlier module-path compile
failure. Combined checks are in `checkpoint-integration.log`; original worker
receipts and manager re-seal identities remain distinct.
The integrated provider run passes 23 application-recovery cases, including the
new secondary boundary, alongside 23 PCB/secondary, 18 reservation, 18 recovery
runtime and four host-contract cases. Strict three-package Clippy, the global
module ratchet and assurance/docs checks pass. The first manager server filter
matched zero tests and earns no credit; the corrected
`ims_package_tests::application_recovery` run is retained in the seal receipt.
