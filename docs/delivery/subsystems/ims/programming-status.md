# IMS — DB / TM programming surface progress

Subsystem: **ims**
Phase: **programming**
Target release: **0.14.0**

Status: **Proposed**

## Root SHISAM fixed-layout integration — 2026-10-03

Root composes worker `5a7118d94d889de5d670fca953080d712a0a057c` onto
`7cb46bc8f8e25f999bdae5d9d555facbf54c307a` with the same exact nine-path
SHISAM allowlist below. Manager reviewed the complete source-backed handoff,
both existing production predicates, every added public/signed/engine test and
all fifteen actual retained-reader executions. Exact committed inputs, original
artifacts, four retained binaries, command logs and stopped/removed task clusters
independently verify. All six Rust inputs remain byte-exact to the worker's
passing strict lint; the intervening TM commit changed none of those inputs.
Worker runtime/PostgreSQL receipts keep their original candidate identities.

The expected status insertion and generated manifest conflicts preserve both
complete TM and SHISAM sections. Only normal documentation generation may
recompute the manifest; no generated hash is authored. Changed root docs/check,
changelog/check, coverage/check, formatting and the exact nine-path generated
seal/committed check are required, followed by root Cargo cleanup. No unchanged
runtime, PostgreSQL or known CICS source-prerequisite exploration is repeated.
This finite fixed-layout fence does not complete the remaining organization,
U/V, shared TM/raw CALL, official/HUMAN or parent v0.14 obligations. Licensed IBM
certification remains excluded; OSS dependency/license policy remains required.

## IMS-1404.tm-output-identity-local-order (finite runtime declaration, 2026-10-03)

Target **0.14.0**, composed base `86bf917e5a20d581a3b5d56b89de86db94ccadf4`.
The historical fail-first packet remains candidate `87f951c4`, preserved in stash
`c015e2b8ea26730bccd5806494f484e971cb5297` and its external receipts. Manager
authorizes only future private IMS outbound identity and same-started-input/work
completion order, using retained input/work incarnation, PCB, observed session
CAS version and bounded completion slot. Sequence is checked version + slot + 1;
PURG uses slot zero, commit enumerates remaining buffers deterministically and
terminates the session. Existing pending rows retain their bytes/sequence; express
available rows stay outside pending IDs. One publication/store proposal remains
the owner; identity does not authorize a lease or shared TM recovery.

Maximum exact allowlist declared before runtime edits:

- `crates/providers/mainframe-env-ims/src/tm/service.rs`
- `crates/providers/mainframe-env-ims/src/tm/support.rs`
- `crates/providers/mainframe-env-ims/tests/tm_runtime.rs`
- `crates/apps/mainframe-env-server/src/product/tests/ims_tm_backout_gap_tests.rs`
- `docs/delivery/subsystems/ims/programming-status.md`
- `docs/decisions/0042-ims-tm-output-identity-local-order.md`
- `changes/unreleased/ims-tm-output-identity-local-order-20261003.toml`
- `docs/documentation-registry.json`
- `docs/README.md` (normal generator)
- `docs/generated/documentation-manifest.json` (normal generator)

Actual offline search/full reads reverify the three unchanged `ims-tm-contracts`
pins, baseline `ibm-ims-15.6-tm-contracts-2026-09-11`: ISRT-TM 63–90/151–159,
PURG 66–79, conversation recovery 30–38, with 167/132/49 complete selected lines.
Catalog context stays ISRT `dli-call-families:0008`, GU `:0005`; supplemental PURG
adds no comparison row. Source identities/commands remain external under
`tm-output-identity-runtime/`. Reference source credit is zero.

Required proof maps to public TmService and installed/published signed-selected
ProductServer on Memory/file SQLite: original four literal two-group tests and
four controls; mixed buffers/PCBs, replay/conflicts, ordinary commit/cancel/
rollback immutability, real reuse/reclaim histories, capacity/CAS/failure/retry.
Base-produced legacy fixture and retained base reader precede production changes;
actual independent SQLite writer/read/replay processes carry phase proof. Scoped
TM controls, strict affected Clippy and mandatory policy/docs/contract gates are
required before a generated complete seal. No tests have yet passed for this
runtime candidate. Coverage inventory and mandatory denominator stay unchanged.

No row/schema/canonical migration or receipt pruning; old writers must stop and
drain, unknown outcomes reconciled with retained rows/receipts. Schema readability
does not permit rolling writers. Fresh calls require distinct existing call keys;
terminal PURG replay and old-key/new-input replay lifetime remain separate seams.
No dense/global/cross-incarnation order or mixed-restore guarantee. Coherent
restore needs catalog/input/session/outbound/replay/work epochs/packages/artifacts/
clock together. Proposed ADR0042 covers private ownership only. ADR0031/0033,
raw CALL, participant/shared lease/settlement and actual TM application recovery
remain unanswered; PostgreSQL/full backup/official/HUMAN and full v0.14 remain
unfinished (official/HUMAN 0/25; licensed certification excluded).

Local runtime results: 18 public tests pass (the three fixture entries ignored in
that invocation are not proof); six signed gap/express tests pass, including the
two unchanged negative actual-TM backout witnesses; one arithmetic boundary test
passes. A further exact signed TM dispatch/selection/rollback control passes.
These are 26 distinct ordinary/local test entries, with the two mixed histories
also checked separately against literal sequences 4/6/8/10/15/16/17 and IO 18.
Independent child phases actually execute 11 exact one-test processes: base
writer/read/replay (3), new writer/read/replay plus both cross-version readers and
legacy replay (6), and publication-lost-ack writer/cold retry (2). The five explicit
fixture entry names use --ignored --exact with required external paths; their
selected process logs show one pass, zero ignored, and real phase markers. No
unselected/ignored helper, reopen-only or PostgreSQL skip earns process credit.

Old reader binary SHA-256
`a1d0fe7fcc1fb72b004ad6c19bea726ca292e953c9a269cc3bc1b868661fb1b5`
was compiled while both production owners matched base 86bf917e, before edits;
genuine available express/pending ordinary fixtures and exact row/receipt bytes
remain external. The new reader preserves the base rows/replay; the actual old
read-only reader preserves new rows. Original test prefixes, root HISAM/source
sections and six other TM production owners are independently byte-verified.

All-target/all-feature/no-deps IMS/server strict Clippy passes. Mandatory offline
deny, license notices/supply-chain, schemas/spec (134 Python cases), IMS catalog/
assurance, coverage inventory, changelog and execution/effect/provider-row/storage/
SAF/retention/participant/typed/module/public-API guards pass without ratchet or
denominator edits. Normal docs generation/check, final formatting and the exact
generated ten-path seal/check complete packaging; external command receipts bind
each check to its actual inputs. No unchanged global architecture/CICS source
prerequisite is rerun or declared solved.

Retained diagnostics: first public compilation exits 101 for a test-only invalid
WorkState enum name (repaired to real Queued); its dependent process step executes
zero tests. A later fmt check exits 1 for wrapping the added literal assertion.
Both sequences retain nonzero aggregate exits and cargo-clean logs; no semantic
expectation or production rule is weakened. Subsequent changed-input checks,
normal docs packaging and cleanup are recorded separately. Original four failing
second-PURG logs remain historical 87f951c4 evidence, never relabeled green.

The first normal docs generation rejects the newly registered ADR because its
navigation entry was omitted. Repair only that authorized registry entry; preserve
the exit-1 packaging receipt/cleanup and rerun changed docs generation/check.
No runtime, lint or policy result is relabeled or repeated for this docs repair.

## IMS-1403.shisam-fixed-layout-admission — runtime leaf declaration, 2026-10-03

Entry `86bf917e5a20d581a3b5d56b89de86db94ccadf4`, branch
`codex/v014-shisam-fixed-layout-admission-20261003`; target **0.14.0**.
Manager reviewed the completed test-only phase and approves only two production
changes: SHISAM min/max equality after shared metadata numeric bounds, returning
existing IncompatibleReference; and extending the existing engine fixed-root
SHSAM guard to SHSAM or SHISAM, returning InvalidDefinition. This typed equal-
bounds constraint changes no DTO, bytes/schema/identity, root count, logical/
index/key/PROCOPT/context or storage/navigation authority. No new ADR is needed.

Exact nine-path allowlist declared before repair:

- `crates/contracts/mainframe-env-host-api/src/ims_metadata.rs`
- `crates/providers/mainframe-env-ims/src/database/definition.rs`
- `crates/providers/mainframe-env-ims/src/database/tests.rs`
- `crates/providers/mainframe-env-ims/tests/shisam_fixed_admission.rs`
- `crates/apps/mainframe-env-server/src/product.rs` (test declaration only)
- `crates/apps/mainframe-env-server/src/product/tests/shisam_fixed_admission_tests.rs`
- `docs/delivery/subsystems/ims/programming-status.md`
- `changes/unreleased/ims-shisam-fixed-layout-admission-20261003.toml`
- `docs/generated/documentation-manifest.json` (normal generator only)

Rows remain 0008 ISRT, 0005 retrieval, 0006 holds and 0019 scheduling controls
under `ibm-ims-15.6-dli-2026-08-31:dli-call-families`; all25 mandatory, accepted
official/HUMAN0/25. Existing typed Batch invocation public registry/provider and
signed selected package routes remain the proof consumers; no all-context claim.
Reverify selected metadata-contracts DBD205–234/SEGM300–354 offline before the
predicate edits. Preserve original v2 red/v3 controls and all reviewed bytes/
artifacts externally. Capture actual good base-created engine/public/signed
state and existing reader binaries outside target before repair, then prove
new readers against exact old good and invalid bytes, distinct provider/image/
package/selected-generation fail-closed mappings without retained mutation, and
actual old read-only compatibility with new good state. Local backend matrix is
Memory, file SQLite and mandatory scoped PostgreSQL18.6 public/signed parity.

Gates: all six original rejections green with no partial rows/generation changes;
six positive controls; affected host metadata/engine organization/image/install/
signed and historical-reader tests; strict host-api/IMS/server all-target/all-
feature no-deps Clippy -D warnings; fmt, offline deny/license/supply-chain; normal
docs/changelog/catalog/assurance/schema/spec/coverage and relevant execution/
effect/rows/storage/SAF/retention/participant/typed/module/API guards. Each build/
test/lint/generator sequence cleans only this checkout target. Fresh receipts
are external under `shisam-fixed-layout-runtime/`. Task PG is isolated loopback
only, stopped on pass/failure with logs retained before its resolved data cleanup.
Seal the exact leaf only after all declared gates pass; no push/PR.

Legacy invalid layouts stay parseable but new runtime validation fails closed;
deployment availability may require separately reviewed operator recovery. No
coercion, automatic migration or re-signing. HISAM/HIDAM ranges remain intact;
shared single-root and HSAM/SHSAM equality gaps stay separately mandatory. This
does not certify literal DBDGEN minbytes presence/compression/physical VSAM,
process restart/backup, participant or parent v0.14 completion. Raw CALL/TM
ADR0031/0033 remain unanswered; licensed certification is excluded.

Local runtime leaf results: **Complete** for this finite typed equality fence.
Fresh `focused-v1` executes host metadata9, engine/database12, public Memory/
SQLite4 and signed Memory/SQLite4 passing tests, zero failures. All six original
literal rejections are green with no partial rows/generation/publication; six
distinct fixed-layout controls retain keyed ISRT/GU/GHU/GN, replay and reopen.
Seven ordinary external-reader/PG ignores get no credit; required resource-bound
selectors execute explicitly with `--ignored --exact`, never empty filters.
Strict current host-api/IMS/server all-target/all-feature no-deps Clippy with
`-D warnings`, fmt and 17 additional/current install/organization/package/signed
regression executions pass. Original v2 red/v3 controls remain their own inputs.

Base-created good state and explicitly resolved old test readers were captured
before repair. Three read-only sequences execute five tests each: old readers
on old good/invalid bytes, new readers on exact old good/invalid bytes, and old
readers on new good bytes. Four good fixed organization images roundtrip byte-
exact old/new. Invalid engine restore returns InvalidDefinition; public service
open and selected-generation decode return InfrastructureFailure; retained
signed package-registry restore returns Malformed. Full SQLite file hashes and
all provider payload/version digests are unchanged before/after each corrected
reader. The first whole-server old reader advanced retention_lock and was
correctly uncredited for read-only byte preservation; logs/binary remain intact.
The corrected test uses existing registry/selection readers, not new authority.

Scoped PostgreSQL18.6 actually passes public variable rejection/fixed keyed
programming-replay-reopen and signed rejection/fixed selected programming-replay-
reopen, one exact executed test per history in four separate task databases.
Public v2 identities remain unchanged; signed v3 fixes only the test setup's
existing required secret-reference prefix. Initial socket-path startup and signed
setup failures remain uncredited. Loopback-only task clusters are stopped and
their exact resolved data directories removed after external log preservation.
No process-restart, backup, physical VSAM or all-context claim follows reopen.

All 22 declared policy/generated/contract gate commands pass: offline deny,
both-target license notices, offline supply-chain, normal docs/check, changelog,
coverage inventory, schemas/spec, IMS catalog/assurance and execution/effect/rows/
storage/SAF/retention/participant/generator/typed/module/API guards. No ratchet,
schema, source pin, dependency or authority changed; all build/test/lint/generator
sequences clean this checkout target, including failures. Fresh command/input/
source/binary/artifact identities, counts/skips and the exact generated local
seal/check are retained externally under `shisam-fixed-layout-runtime/`.
Final status prose requires only normal docs regeneration/check before seal;
passing runtime/PG explorations are not rerun or relabeled for prose/commit.
Old invalid layouts remain readable to old validators, not equivalent safe
downgrade. Affected deployment availability/operator recovery limits and separate
shared SHISAM single-root/HSAM/SHSAM layout obligations remain as declared above.
Parent IMS/full v0.14, all25 and official/HUMAN0/25 remain unchanged/incomplete.

## IMS-1403.shisam-fixed-layout-failfirst — test-only phase, 2026-10-03

Entry `86bf917e5a20d581a3b5d56b89de86db94ccadf4` on
`codex/v014-shisam-fixed-layout-failfirst-20261003`; target **0.14.0**.
This phase adds independent rejection expectations for a unique named root-only
SHISAM descriptor changed only from lengths 3/3 to 3/4. Existing shared metadata,
engine definition, actual public installation and signed installation/publication
are the admission owners. No production repair is authorized in this phase.

Exact six-path test-only allowlist, declared before test edits:

- `crates/contracts/mainframe-env-host-api/src/ims_metadata.rs` (cfg(test) only)
- `crates/providers/mainframe-env-ims/src/database/tests.rs` (tests only)
- `crates/providers/mainframe-env-ims/tests/shisam_fixed_admission.rs` (new)
- `crates/apps/mainframe-env-server/src/product.rs` (existing cfg(test) module declaration only)
- `crates/apps/mainframe-env-server/src/product/tests/shisam_fixed_admission_tests.rs` (new)
- `docs/delivery/subsystems/ims/programming-status.md`

Proof requires six actual assertion failures (host, engine, public Memory/SQLite,
signed Memory/SQLite), logging accepted results before normative rejection.
Fixed SHISAM keyed programming/replay and retained-image controls execute on
Memory and file SQLite. Selected pinned DBD/SEGM and call/status sources are
reference data with zero execution credit. Focused tests, formatting and strict
affected test lint retain exact commands/input identities externally under
`shisam-fixed-layout-failfirst/`; each Cargo sequence ends with this checkout's
default target cleanup. A minimal later predicate repair and legacy-invalid
readability consequences require manager review before production edits.

U/V production owners and HISAM test paths are untouched. All 25 catalog rows
remain mandatory; accepted official/HUMAN counts remain 0/25. SHSAM/HSAM variable
layout obligations, physical/operational parity, raw CALL/TM ADR0031/0033 and
parent IMS/full v0.14 remain incomplete; no commit or feature seal is authorized.

Test-only results: `failfirst-v1` is compiler/setup failure with zero behavior
credit. `failfirst-v2` executes all six normative rejection tests: shared host
and engine each accept 3/4 SHISAM; public Memory/SQLite each install metadata
with two new IMS rows; signed Memory/SQLite each stage and publish generation 1.
Every normative assertion fails against literal rejection, preserving actual
results before assertion. Public calls also retain 3-byte `A1X` and 4-byte
`B2YZ`; current readers reopen that invalid layout without changing existing
rows. Those artifacts remain external and are not accepted compatibility rules.

The same v2 execution has five additional uncredited control-fixture failures:
reused GU/GHU idempotency identities on four store/route controls and a wrong
SHSAM sequential-order expectation. `controls-v3` repairs only those histories
and executes five passing controls: engine good-image roundtrips for fixed
SHISAM/HISAM/SHSAM/HIDAM, signed Memory/SQLite and public Memory/SQLite keyed
ISRT/GU/GHU/GN, exact replay and reopen. The unchanged host fixed/other-organization
control passed at its original v2 identity. No rejected expectation is weakened
or relabeled as current-candidate execution. Total distinct tests: six normative
red proofs and six passing controls, with no ignored tests or empty selectors.

Current affected all-target/all-feature no-deps Clippy with `-D warnings`, fmt,
offline deny, offline supply-chain and both-target license-notice checks pass.
Each of four Cargo sequences cleaned only this checkout; target is absent.
No production repair, generator output, commit or seal was made. Seven selected
source reads verify matching archive bytes after retained-path absence; general
programming searches return partial results because the previously unavailable,
unselected STAT cache entry is absent. The finite source requirement is available.
The external handoff proposes only SHISAM equality checks in existing shared
metadata and engine definition owners, and identifies fail-closed consequences
for retained invalid images/generations. That runtime phase awaits manager review.

## Root HISAM integration declaration — 2026-10-03

Root composes worker `55cf6d5baead04bcfe97caecdbf5a48b4ef9e145` onto source-only
head `87f951c49f48b117e41f1bc7ec1d46b4ea9606b1`, with the same declared ten-path
HISAM allowlist. The manager fully reviewed its source-backed handoff, production
diff, literal public/signed histories, actual CHKP/XRST and independent SQLite
child assertions. All seven Rust inputs must remain byte-exact to both the
passing strict-lint and affected-regression receipts. Root's intervening source
commit changed no Rust input; unchanged worker runtime results retain their own
candidate identities and are not rerun or relabeled for this composition.

The expected status/header insertion and generated status hash merge conflicts
are resolved by preserving both complete leaf declarations, correcting only the
ISRT catalog label to its literal committed label and normal documentation
regeneration. No generated hash is authored manually. Changed root docs/check,
changelog/check, coverage/check, formatting and exact ten-path generated seal/
committed check are required; receipts remain external and the intended root
Cargo target is cleaned after the sequence. Source-only pins grant no runtime
credit, and this finite Batch insertion leaf is not explicit all-context IMS,
HISAM/SHISAM, PostgreSQL/backup, official/HUMAN/participant or v0.14 completion.
After this root seal, execution and mutation-consumer ownership can transfer to
the same uncommitted U/V CLI lane. Shared raw CALL/TM approvals remain pending.

## IMS-1404.express-purg-output-identity-failfirst (test-only findings, 2026-10-03)

Entry `87f951c49f48b117e41f1bc7ec1d46b4ea9606b1` on
`codex/v014-tm-output-identity-failfirst-20261003`. This bounded investigation
changes only `crates/providers/mainframe-env-ims/tests/tm_runtime.rs`,
`crates/apps/mainframe-env-server/src/product/tests/ims_tm_backout_gap_tests.rs`
and this status. Production, schemas, pins, accepted rules, public APIs and
Proposed ADR0031 remain unchanged. Existing negative TM backout expectations
and other lanes are preserved. Intentionally failing tests remain uncommitted;
no feature seal or subsequent implementation is authorized by these findings.

Independently verified retained-topic-path-first/archive fallback and actual
offline reader search/full reads: `ims-tm-contracts`, baseline
`ibm-ims-15.6-tm-contracts-2026-09-11`, product `SSEPH2_15.6.0`.
ISRT-TM `ims_isrtcalltm.htm`:63–90 groups ordinary message segments and transmits
at express PURG; `ims_purgcall.htm`:66–79 completes a PCB group and permits a next
message; `ims_conversationrecovery.htm`:30–38 completes express output before
commit, independently of later termination. Full selected reads are 167/132/49
lines (348 total), not just Spool API paragraphs. Exact hashes, bytes, cache and
catalog locators remain in the external handoff. Catalog context is the existing
ISRT `dli-call-families:0008` and GU `:0005`; PURG is supplemental, not a new row.
IMS denominator remains 25; official/HUMAN/runtime/licensed credit is zero.

Four separately named public TmService and installed/published signed-selected
ProductServer failure entries actually execute on Memory and file SQLite.
Each uses real enqueue, a live claimed lease, start and GU, then fixed express
PCB EXP on TERM2: literal `first-one`/`first-two`, PURG, exact replay, then
`second-one`/`second-two` and a distinct PURG before any commit/next input.
Both fresh PURGs are independently expected to succeed with distinct IDs and
two immutable ordered groups. The actual second result and stored outbound rows
are captured before assertion: **Err(IdempotencyConflict)** on all four cases;
only the original first group remains, byte/version unchanged. Public first ID
is `out-e98df96a427a4d3fa2d3e0036c7be56a`; signed first ID is
`out-48d11749e1323c2ba8e438a22e29507b`. All first/replay/setup controls pass,
including four separate passing control entries. No private/provider row or
work/lease fabrication, cloned data oracle or weakened error expectation.

Actual focused results: 8 exact named tests execute, 4 controls pass and 4 genuine
semantic failures remain (Cargo exits 101 for each, aggregate wrapper also 101),
0 ignored. No compile/setup failure or fixture repair. Formatting and strict
affected all-feature no-deps test Clippy pass. The sequence ends in this checkout's
`cargo clean` (13,488 files / 6.6 GiB); no unchanged runtime suite repeats for prose.
Dependency/license policy remains required; no broad packaging/certification
campaign is implied. Actual candidate identities and all logs are external under
`worker-receipts/v014-completion-20261002/tm-output-identity-failfirst/`.

Confirmed code cause: TM `support.rs::output_row` hashes run unit/PCB/ordinal;
`service.rs::purge` uses only pending nonexpress IDs as ordinal, so completed
express groups reuse zero. Suggested next design stays with these two IMS-owned
production files: bind new output keys to retained input/work identity and an
existing CAS-versioned completion occurrence, preserving replay/readers and
pending-output ownership. No new schema counter or shared authority. The external
handoff identifies ordering, old receipt/rolling-writer and lifetime obligations
before implementation. Manager review and an exact implementation declaration
are required first. Shared stale-lease publication/settlement and actual TM
application backout remain gated by unanswered Proposed ADR0031; this collision
proof grants none of their acceptance. Parent IMS/full v0.14 remains incomplete.

## IMS-1401.sequential-isrt-position-sources (source-only leaf declared, 2026-10-03)

Entry `3887bb44d334c6408a7a01ddd00ae1fc5361ea51`; target **0.14.0**. Register only
three retained IMS 15.6 HTML pins in scope `ims-sequential-isrt-position`, baseline
`ibm-ims-15.6-sequential-isrt-position-2026-09-11`. The source registry and existing
offline reader remain the owners. Preserve the twelve existing manifest bytes
and registry rows, all prior Proposed ADRs and root's F integration fixture repair.

Exact five-path allowlist, declared before source registration:

- `conformance/0.14/manifests/ims-sequential-isrt-position-topics.json`
- `conformance/0.14/manifests/index.json`
- `docs/delivery/subsystems/ims/programming-status.md`
- `changes/unreleased/ims-sequential-isrt-position-sources-20261003.toml`
- `docs/generated/documentation-manifest.json` (normal generator only)

Reverify the selected topic metadata/body hashes, retained topic paths first,
archive fallback, run identity and TOC/product binding. Import only those three
topics and their TOC into the external task cache with the existing reader;
search the registered scope and read all 215/34/36 plain-text lines, using two
bounded ISRT reads. The archive run is in progress; snapshot/run creation is
2026-09-11, while II was fetched 2026-09-12T00:17:12Z. No HTTP Last-Modified was
retained. This supplement claims neither whole-corpus nor browser reproduction.

Affected gates: existing source-reader regression suite; registered manifests,
schema/spec, IMS catalog/assurance and coverage inventory consumers; normal docs
generation/check, changelog, fmt and offline deny/license-notice/supply-chain
policy. Every sequence cleans this checkout's Cargo target; actual candidate
inputs and receipts stay external under `sequential-isrt-source-pins/`.

The exact three-topic set independently reproduces SHA-256
`f50d0ca77232e353343bfc37bd9b02fef87d20c6bb5926becc5c8e1ef09e55a0`, 33,864
HTML bytes. All three retained paths are absent; selected archive bodies and
metadata verify exactly. The existing reader imports four verified cache entries
(three topics plus TOC), returns three ISRT search matches and reads all 285
lines. Applicable locators in `SSEPH2_15.6.0` are
`com.ibm.ims156.doc.apr/ims_isrtcall.htm`:129–166 (unique/nonunique/unkeyed insert
rules), 202–213 (GE and missing-parent PCB level feedback);
`com.ibm.ims156.doc.apg/ims_posafterisrt.htm`:3–16 (successful continuation after
the inserted occurrence), 29–31 (II before the duplicate); and
`com.ibm.ims156.doc.mc/msgs/ii.htm`:4–30 (II positioning and broader causes,
including sentinel FF). These are reference facts, not new accepted/runtime rules.
The source-reader regression executes 23 tests, all passing with no skips;
manifest/schema/spec, IMS catalog/assurance and coverage/inventory consumers pass.
Existing twelve manifest bytes and registry rows independently remain unchanged.
Final changed documentation/packaging and offline policy gates precede the exact
generated leaf seal/check; no runtime or backend suite is authorized here.

Catalog context stays `ibm-ims-15.6-dli-2026-08-31:dli-call-families:0008` (ISRT
call; exact committed locator `html-table:comparison;row:8;command:ISRT command`),
with the unchanged 25 mandatory rows. This source-only leaf prepares later
II-before-duplicate and failed ISRT positioning review; it is not prerequisite
evidence for the parallel positive HISAM default-LAST runtime leaf. It changes
no runtime, accepted IR rule or HUMAN authority and earns zero execution,
IR/HUMAN/official/licensed coverage credit. FIRST/HERE/F/L-on-ISRT, sentinel FF
and general failed searches remain unfinished runtime classes. Raw CALL/TM
ADR0031/0033 approvals remain unanswered; licensed certification is excluded.
Parent IMS/full v0.14 remains active and incomplete.

## IMS-1403.hisam-nonunique-dependent-last — bounded leaf declaration, 2026-10-03

Entry `3887bb44d334c6408a7a01ddd00ae1fc5361ea51` on
`codex/v014-hisam-nonunique-dependent-last-20261003`, target 0.14.0. This leaf
admits typed public-provider and signed-selected Batch invocation ordinary primary physical
HISAM with exactly two fixed levels, one terminal child type, unique named root
sequence key and named nonunique child sequence key. Actual root GU establishes
live parentage before an ordinary unqualified terminal child ISRT. Validated
selected metadata supplies the private insertion policy; existing RecordId/version
ordering supplies default LAST. Successful insertion retains ancestor root
parentage, sets inserted current and clears hold. Corresponding generic utility
materialization uses the same metadata predicate. Historical standalone engine
insertion defaults, descriptors, wire formats and retained bytes remain unchanged.

Exact ten-path allowlist (manager-reviewed contextual handoff amendment):

- `crates/providers/mainframe-env-ims/src/database/store.rs`
- `crates/providers/mainframe-env-ims/src/service/generic.rs`
- `crates/providers/mainframe-env-ims/src/service/execution.rs` (only trusted Invocation handoff)
- `crates/providers/mainframe-env-ims/src/database/tests.rs`
- `crates/providers/mainframe-env-ims/src/service/generic/tests/closure_tests.rs`
- `crates/providers/mainframe-env-ims/tests/application_recovery/checkpoint_tests.rs`
- `crates/apps/mainframe-env-server/src/product/tests/ims_package_tests.rs`
- `docs/delivery/subsystems/ims/programming-status.md`
- `changes/unreleased/ims-hisam-nonunique-dependent-last-20261003.toml`
- `docs/generated/documentation-manifest.json` (normal generator only)

Production owners are existing database insertion and generic metadata adaptation;
tests stay inline in the listed owners. The existing execution caller passes its
trusted Invocation into the crate-private generic adapter, which derives the same
run_unit_id and fences new admission without requiring an authorizer. The legacy
ImsRequest has no explicit execution context; Batch invocation proof does not
establish all DbBatch/TmBatch/DBCTL/TM distinctions. No navigation/SSA/position-witness,
feedback/recovery producer, other execution change, module declaration, F/L/null/logical test,
ratchet, dependency, source pin, accepted rule or assurance-floor edit is declared.
No new public API, RULES field, ABI, schema, witness, retention edge, authority or
ADR0042 is authorized. Full-catalog logical participation at either endpoint,
retained logical links, secondary processing, deeper/multiple/variable/unkeyed
layouts, other organizations/contexts and SSA/raw update commands remain outside
this finite class and mandatory unfinished work.

Source baseline `ibm-ims-15.6-database-contracts-2026-09-11` HISAM, metadata
baseline FIELD/SEGM (SEQ M and default LAST), programming baseline GU/GN/GNP
(including GNP123–127 parentage), position/status and CHKP/XRST pins are consulted
offline before edits, retained-path first then exact archive fallback. Catalog
ISRT call `0008` is the new behavior; actual consumers `0005/0006/0015/0002/0023/0025`
receive bounded proof. All25 remains mandatory. DLET `0004`, LOAD `0009`, II
before-duplicate, general failed-parent ISRT progress, later-twin DLET continuation,
sentinel FF, FIRST/HERE/L-update/raw/variable/deeper/context and SHISAM fixed
admission remain separately unfinished. No source review is execution evidence.

Required local proof: independent fail-first and repaired literal public/signed
Memory/file-SQLite insertion/order/parentage, PCB isolation, hold/REPL/DA/DJ,
canonical replay/conflicts, store failure/CAS/capacity/lost acknowledgement,
SAF/sensitivity/cancellation, real backout/CHKP/XRST, and nonzero independent
SQLite seed/hold, reopen/replay and next-occurrence/REPL processes. Required gates
are focused changed-input regressions, strict affected IMS/server all-target/
all-feature no-deps Clippy, fmt, offline deny/license/supply-chain, normal docs/
changelog/coverage/schema/spec/catalog/assurance and affected boundary guards.
Receipts stay external under `worker-receipts/v014-completion-20261002/hisam-nonunique-runtime/`;
every build/test/lint/generator sequence ends in this checkout's Cargo clean.
PostgreSQL parity is explicitly unrun/pending, not a required local backend gate.
Old readers can read distinct IDs but reject further equal-key insertion with II;
readable historical bytes do not establish equivalent downgrade semantics.
Parent IMS/official/HUMAN/participant/full-v0.14 remains incomplete; shared raw
CALL/TM ADR0031/0033 approvals remain unanswered, licensed exclusion earns zero
credit and mixed-resource closure remains a 0.16 obligation.

Runtime verification for this finite leaf passes: repaired independent public and
signed Memory/file-SQLite fail-first cases originally observe II against literal
blank, then return blank/affected1 with distinct twin IDs. The original setup
compile errors and all intermediate fixture failures remain external and
uncredited. Twelve IMS filter entries pass, of which the idle process worker
earns no standalone credit; three substantive SQLite child invocations each
report one executed test and their exact seed-hold/reopen-replay/next-replace
success marker. Three signed entries pass, including four independent GN/GNP
Memory/file-SQLite coordinator histories. Existing replay ownership requires the
original execution/run/sequence; the signed fixture now preserves that owner and
grants the existing read capability for its UNLOAD control.

Literal proof covers inserted current/root parentage, real root-hold cancellation,
C1A/C1B/C2Z traversal and bounded GE, independent A2Y/other PCB, held second
ID/version and repeated nonkey REPL, DA/DJ, unchanged canonical replay after
navigation/REPL, utility twins, all four non-Batch classes without an authorizer,
metadata/access/condition fences, capacity/CAS/lost acknowledgement and competing
insert, backout order and cancelled positions. Real CHKP/XRST runs on Memory and
file SQLite clear the child hold and decline nonunique positioning without
inventing a twin identity; the unique-root control repositions normally.
Affected regressions pass 220 generic, 9 engine, 76 application-recovery and
50 selected server tests (one existing ignored entry receives zero credit).
Strict IMS/server all-target/all-feature no-deps Clippy passes with warnings
denied. Source review is 13 selected verified archive fallbacks, zero retained
matches or body mismatches, 13 successful actual reads and three searches; the
partial programming-cache search reports unselected STAT unavailable. No source
review result is relabeled as execution or as a fresh whole-corpus audit.

All 23 declared policy/generated/boundary commands pass: fmt, offline deny,
pinned supply-chain host policy, license notices, normal docs generation/check,
changelog, coverage inventory, schemas/spec, IMS catalog/assurance, execution,
effect encoding, provider rows, storage, SAF, retention, participant bindings and
generator, typed/module and public API guards. The existing ignored private
retention process worker runs substantively only under its separate bounded
parent; its idle selection earns no credit here. Module budgets and API ratchets
remain unchanged. This finite runtime leaf is complete; exact command outcomes
and candidate hashes remain external, with the generated feature seal binding
only the declared ten paths. Unchanged runtime inputs retain their original
receipt identities across this documentation update; no runtime suite repeats
for prose or sealing. The known global CICS source prerequisite remains unchanged
and unrerun, not passed by these selected guards.
No production owner beyond the declared ten paths changed. PostgreSQL, backup
composition, official/HUMAN/participant acceptance and broader mandatory classes
remain pending; a provider reopen is not process-restart or backup certification.

## Root F integration declaration — 2026-10-03

Root consumes worker `31fd19d85d95cbf4c6f591dc11aaf72f00db522f` from the same
entry `dd097038e761730f394098b58f01ffca3906eda7`, using the identical declared
seventeen-path leaf allowlist. All three production files must remain byte-exact
to that reviewed worker. Its scoped runtime, source and mandatory gate receipts
retain their own candidate identities; they are not relabeled as a new root run.
The manager fully reviewed the external handoff and committed seal/check, the
production diff, finite fences and actual signed/replay/child proof.

One test-only integration repair makes the independently authored ordinary
pre-F retained root-parent fixture literal C1Q, rather than C2R. Real later REPL
changes that same C1 to C1Z, while retained replay must still return old C1Q
without moving today's position or changing the original row bytes. The fixture
remains compatibility proof, not a captured historical/official run. It changes
no live handler, old stored receipt, canonical encoding or source rule.

Changed-input root proof is the exact compatibility test on substantive Memory
and file SQLite, strict affected IMS all-target/all-feature no-deps lint and
formatting. The unchanged worker F/L/ordinary/signed/null/logical/source/policy
results are reused only with their actual input identities. Changed normal docs,
changelog, coverage inventory and exact root seal/check are required; no suite
repeat merely for prose/commit/PR or unchanged global CICS-source gate. Every
sequence cleans this intended root Cargo target, keeping receipts external under
worker-receipts/v014-completion-20261002/ssa-first-root-*. The exact compatibility
test passes one entry with real Memory and file-SQLite scenarios; strict affected
all-target/all-feature lint and fmt pass. All ten unchanged worker Rust inputs,
including the three production files, independently match their original hashes.
This sequence cleans 1.7 GiB of root Cargo output. The first docs attempt rejects
the integration section preceding required subsystem header metadata; moving
that unchanged metadata back to the header repairs it, and the failed receipt
and 6.1 GiB cleanup remain recorded. Only changed documentation/packaging needs
retry; no runtime suite repeats. The finite F leaf, ADR0041 Proposed status and
all broader SSA/raw/TM/official/HUMAN/participant/v0.14 limitations remain unchanged.

## IMS-1403.ssa-first-direct-child (bounded runtime leaf declared, 2026-10-03)

Entry: `dd097038e761730f394098b58f01ffca3906eda7` on
`codex/v014-ssa-first-direct-child-20261003`, target 0.14.0. Parent IMS-1403,
IMS-1401 and full v0.14 remain incomplete. The manager authorizes only typed
CALL/public-provider and signed-selected DbBatch GNP/GHNP, primary HIDAM with
exactly two physical levels and one child type, uniquely named fixed-length
sequence keys, live root parentage and current root or that root's direct child,
one unqualified child SSA with one active F. Existing null slots may surround F;
their raw bytes and slot bounds remain authoritative. No deeper child dependents
exist in this finite class, so same-occurrence satisfaction has no dependent
position to reset. Other required F classes remain unfinished, not inapplicable.

Sources: independently verified retained-topic-path-first, exact archive fallback,
repository plain-text parse and actual offline search/read for the committed
`ims-ssa-position-commands` baseline
`ibm-ims-15.6-ssa-position-commands-2026-09-11`, set
`f99728026ed7f14fcc8e104678bc55939581af2defee68385bc3bf35b170e8b9`.
`ims_fcmdcode.htm` SHA-256
`425e453bd01c99b47f2a513fcf74678a9b5ff84cd16c719d83696053e9e47cd0`,
7545 bytes, lines 8–33 supplies first-under-parent restart and same-occurrence
satisfaction. Existing L/success/failure pins and programming-contracts
GNP/GHNP/hold topics supply parentage/failure/hold composition. Catalog context:
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0005/:0006`, composed
REPL/backout/CHKP/XRST `:0015/:0004/:0002/:0023/:0025`. Local tests grant no
official row, accepted IR, HUMAN, licensed, participant or source execution credit.

Owners: the existing navigation matcher/position/hold authority, narrowly
generalized L metadata and live-path fence, SSA parser, PCB sensitivity/PROCOPT,
SAF, unpublished proposal/CAS/row/replay and selected package/coordinator/recovery.
F restarts that same traversal at the established root's first child, selecting
literal C1Q/A1 from root/C1/C2/C3 and repeatedly C1. L retains remaining-forward
Last, ordinary First/Unique/Next/NextInParent stays unchanged. Fresh empty-root F
returns GE/no segment, keeps root anchor/parentage and cancels only its old hold.
Pre-validation exclusions, AC/AM, SAF and publication failures retain their
existing distinct condition/no-mutation boundaries. No new public request,
cursor/schema/history witness, traversal/search pass, store/coordinator or shared
raw CALL/TM authority. U/V design and all broader F forms remain separate.

Proof map: independent public and signed Memory/file-SQLite fail-first against
Unsupported; root/intermediate/last/repeated F starts, GNP/GHNP versioned hold,
ordinary C2 continuations, empty A0/B2 graph, per-PCB isolation, real REPL/backout/
CHKP/XRST, deletion fencing, catalog-wide logical and secondary/shape exclusions,
AC/AM/key-only/PROCOPT/SAF, raw-null conflicts and exact replay after mutation,
capacity/real session CAS/lost ack, historical cursor/receipt compatibility,
reopen and substantive independent writer/reader processes with phase proof.
Affected L/ordinary SSA, signed logical/null regression controls, strict affected
all-target/all-feature no-deps Clippy, fmt, deny, docs/changelog/catalog/assurance/
schema/spec/coverage, execution/effect/rows/storage/SAF/retention/participant/typed/
module/API/supply-chain gates are required. PostgreSQL remains unrun unless
actually configured/executed; no early-return helper or local reopen earns full
backend/backup/official acceptance. No unchanged global architecture/CICS retry.

Exact declared seventeen-path allowlist (including normal ADR packaging):

```text
crates/providers/mainframe-env-ims/src/database/navigation.rs
crates/providers/mainframe-env-ims/src/database/ssa.rs
crates/providers/mainframe-env-ims/src/service/generic/ssa.rs
crates/providers/mainframe-env-ims/src/service/generic/tests/ssa_tests.rs
crates/providers/mainframe-env-ims/src/service/generic/tests/ssa_tests/first_tests.rs
crates/providers/mainframe-env-ims/src/service/generic/tests/ssa_tests/first_tests/fences.rs
crates/providers/mainframe-env-ims/src/service/generic/tests/ssa_tests/first_tests/recovery.rs
crates/providers/mainframe-env-ims/src/service/generic/tests/ssa_tests/first_tests/publication.rs
crates/apps/mainframe-env-server/src/product/tests/ims_package_tests.rs
crates/apps/mainframe-env-server/src/product/tests/ims_package_tests/ssa_first_tests.rs
crates/apps/mainframe-env-server/src/product/tests/ims_package_tests/ssa_first_tests/recovery_tests.rs
docs/delivery/subsystems/ims/programming-status.md
docs/decisions/0041-first-direct-child-ssa-selection.md
changes/unreleased/ims-first-direct-child-20261003.toml
docs/documentation-registry.json
docs/README.md
docs/generated/documentation-manifest.json
```

ADR-0041 stays Proposed. Preserve ADR-0039 and all prior navigation entries;
ADR-0040 is reserved for separate U/V review. No pin/catalog denominator,
shared contract/metadata/retention/null/logical lane, ratchet or policy changes.
Receipts stay outside Git under worker-receipts/v014-completion-20261002/
ssa-first-runtime, with original candidate inputs and failed/repair logs retained.
Every build/test/lint/generator sequence ends in this checkout's cargo clean.
Seal only this finite leaf after declared gates pass; no push or PR operation.

Local verification is complete for this finite class. The original fail-first
fixture accidentally duplicated C1's unique key; its Malformed setup failures
are retained separately. After repair, all four independent public/signed
Memory/SQLite witnesses fail with Unsupported against literal C1Q/A1 before
runtime edits. Candidate-01 passed twelve provider and all six signed entries;
two negative fixtures required valid mixed-C syntax and metadata-installation
rejection for invalid HIDAM roots. Candidate-02 passed fourteen provider entries
and strict lint; its remaining null-F fixture still contained L. That operand
was corrected, with no runtime relaxation. All original receipts remain external.

Final runtime proof passes fifteen new provider and six new signed entries.
The full affected SSA filter also passes fourteen L and forty substantive
ordinary/null/mixed/secondary controls; signed controls add six L, one ordinary,
one null and two logical cases. The runner reports 87 passes, zero failures or
ignored; two unconfigured inherited process-helper entries earn zero credit,
leaving 85 substantive entries. New F writer/reader parents each require two
actual independent SQLite child processes to execute one exact test and pass.
Literal full-result fields, canonical result digest, actual physical paths and
versioned holds agree. Empty-root GE also preserves the other PCB. An independent
base-shaped ordinary receipt and legacy cursor-field fixture retain exact bytes
through mutation/reopen/replay; this is not historical licensed execution.

Strict affected all-target/all-feature no-deps Clippy, fmt, offline deny and
license notices, normal docs/changelog, catalog/assurance/schema/spec/coverage,
execution/effect/rows/storage/SAF/retention/participant/typed/module/API,
assurance inventory and offline supply-chain gates pass. Structural coverage
checks earn zero new official credit. Final prose requires only normal docs
generation/check and exact allowlist seal/check, not another runtime campaign.
PostgreSQL, full backup/backend acceptance, broader F, U/V witnesses, raw CALL/TM
approvals, accepted IR/HUMAN/official/licensed/participant and parent IMS-1401/
IMS-1403/v0.14 remain pending. The unchanged global CICS cache prerequisite is
not retried or claimed green. ADR-0041 remains Proposed; this is one local leaf.

## IMS-1401.null-ssa-command-slots (bounded leaf declared, 2026-10-02)

Parent IMS-1401 and v0.14 remain open. Entry candidate is
`a8deb3d8a97be2660cfa0d38327ff246a194a910`; licensed certification is excluded
with zero credit. This leaf recognizes the literal `-` null slot specified by
IMS 15.6 `apr/ims_cmdcodref.htm`, SHA-256
`cb1d0cc0cf765f13836b888efaea5538bcbcbbd0453028388b07f69736946093`,
programming-contract baseline `ibm-ims-15.6-programming-contracts-2026-09-11`.
The manager used the offline repository search/read and the verified existing
source cache; no refresh, repin or publication body is required.

Rows :0005/:0006 of `ibm-ims-15.6-dli-2026-08-31:dli-call-families` supply
retrieval/Get Hold context, not official case or gate credit. The normative SSA
grammar, its shared generator and bounded host parser own recognition. A null
slot has no active command behavior; all slots count against the existing
command bound. Existing request bytes, per-PCB positioning, sensitivity,
authorization, replay and atomic publication remain authoritative. New active
F/L/U/V/W semantics, update SSAs and raw CALL are outside this leaf.

Declared proof: fail-first literal/null-plus-active parsing and public provider
navigation; negative digits/terminators/slot saturation; independently expected
ordinary/qualified/path/Get Hold behavior, exact request-conflict/no-mutation,
Memory and file-SQLite reopen, and signed selected-package admission. Affected
host/provider/server tests, generated catalog/schema checks, scoped strict lint,
formatting, dependency policy, normal docs/changelog and unchanged module/API
ratchets are required. [ADR-0036](../../../decisions/0036-null-ssa-command-slots.md)
records compatibility. The declaration preceded edits and the final local
results below; it does not establish parent or official acceptance.

Resolved leaf allowlist: `conformance/0.14/ims/ssa-rules.json`,
`conformance/0.14/schemas/ims-ssa-rules.schema.json`, `xtask/src/ims_catalog.rs`,
host `src/ims.rs`, generated `src/generated/ims_ssa_rules.rs`,
`tests/ims_null_ssa_slots.rs` and README; provider generic `tests/ssa_tests.rs`
and its new `null_slot_tests.rs`; server `product/tests/ims_package_tests.rs`
and its new `null_ssa_tests.rs`; this status, ADR-0036, the unique
`ims-null-ssa-command-slots-20261002.toml` fragment, normal generated manifest,
`docs/documentation-registry.json` and generated `docs/README.md`. The last two
register the Proposed decision through the existing documentation authority.

Local fail-first parser tests reproduce InvalidCommandCode; the public-provider
test reproduces Malformed. The first generator attempt referenced its new
constant before generation and failed compilation. Root restored the old parser
for normal bootstrap generation, captured the public failure, then connected
the generated constant; no generated source was edited by hand. Those failed
receipts remain separate. Final focused checks pass two new parser cases, three
new provider cases including file-SQLite reopen, 26 selected host contract cases,
five existing parser cases, fourteen public-provider SSA cases and both the new
signed null case and existing signed SSA regression. Repeated selected cases are
not additional scenario credit. Strict affected all-feature/all-target Clippy
and existing execution/effect/row/storage/SAF/retention/participant/typed/module
guards pass. No process-restart, PostgreSQL, full matrix or official row claim is
made by SQLite reopen. Normal docs generation/check, offline dependency and
license-notice policy, catalog/assurance/schema/spec/coverage/changelog gates,
formatting and exact existing public API ratchets pass. The initial docs failure
for missing ADR applicability metadata was repaired and its receipt retained;
no runtime suite was repeated for that metadata repair. The exact-path feature
seal/check passed in commit `92977fcdf06c62bcd3ce5b9e2e5136072953ba5f`;
that local packaging result is not parent acceptance.

Receipts are external under worker-receipts/v014-completion-20261002/null-ssa-*;
each sequence cleans this checkout's Cargo target. The new grammar byte is
generated privately; the active seventeen-letter inventory, twenty-five-row
denominator, accepted IR rules, public DTOs and historical canonical fixtures
are unchanged. Raw/TM owner approvals and full-minor acceptance remain open.

## IMS-1403.ssa-last-direct-child (verified bounded local runtime leaf)

Parent IMS-1403 and full v0.14 remain incomplete. Entry is the preserved source
seal `6bca4715d3ebac54aec1d89bc70a8095c6e187b9`, target 0.14.0. Manager review
authorizes only DbBatch primary HIDAM, exactly two physical levels and one
direct-child type, uniquely fully keyed fixed-length root/child, valid existing
primary root parentage, current root/direct child, exactly one unqualified child SSA
with exactly one active L command. Root prefixes, other contexts/operations,
logical/secondary/other organizations, deeper/multiple/unkeyed/nonunique levels,
qualified child, other commands and unresolved retained positions stay excluded.
Null command slots belong to the manager lane and are not imported here.

The last occurrence is selected in the remaining forward parent interval by
the existing navigation owner using a private selection policy. Success retains
root parentage and ordinary versioned GHNP hold; empty/exhausted fresh calls
return GE, retain current anchor/root parentage, cancel only this PCB's old hold,
and never cross the next root. Pre-validation rejection and exact replay do not
mutate position/hold or rows. Root-level qualification ambiguity stays pending.
No public request/position/schema, traversal, namespace, coordinator, permission,
raw CALL/TM boundary or accepted IR extension is authorized.

The manager's source-backed parentage disposition uses the existing GNP topic
lines 94–111: GU/GN or a preceding P call may establish root parentage. A
GU/GHU-only history marker is neither required nor admitted. Proof includes real
GU/GHU, GN/GHN and prior P setups, while this L request contains no P. Existing
AC/AM condition receipts and observer/canonical publication are preserved with
no cursor, hold or database mutation; HostProblem shape/SAF rejection and failed
atomic publication preserve the stronger no-mutation boundary.

Logical exclusion inspects participation in declarations anywhere in the full
existing catalog plus actual retained engine links, not only the selected
database's local declaration list. The already-declared `last_tests/fences.rs`
owns remote-only declaration, local declaration and retained-link negative
controls with real root positioning and pre-validation no-mutation checks.

Dependencies: the preserved `ims-ssa-last-position` zero-credit source baseline,
existing GNP/GHNP and GU/GHU/GHN pins, selected metadata/SSA/current/hold owner,
canonical replay, atomic row/CAS publication and actual CHKP/XRST owner.
Acceptance: independent literal fail-first provider and signed selected-route
C3 witnesses; finite starts/GE/continuation/holds/update/backout/checkpoint,
per-PCB isolation, sensitivity/processing/SAF/no-mutation and unsupported shape
fences, exact replay/conflicts/capacity/CAS; Memory and file SQLite reopen and
real separate-process restart; focused regressions, strict affected all-target
all-feature no-deps Clippy, fmt/deny/docs/changelog/catalog/assurance/schema/spec/
coverage and execution/effect/rows/storage/security/retention/participant/typed/
module/public-API/supply-chain policy gates. No source refresh, full campaigns,
licensed/HUMAN/official/parent acceptance, push or PR. Receipts stay outside
targets under worker-receipts/v014-completion-20261002/ssa-last-runtime;
Cargo cleanup ends every build/test/lint/generator sequence, including failures.

Exact initial/final-intended allowlist (any addition requires declaration):

```text
crates/providers/mainframe-env-ims/src/database/navigation.rs
crates/providers/mainframe-env-ims/src/database/ssa.rs
crates/providers/mainframe-env-ims/src/service/generic/ssa.rs
crates/providers/mainframe-env-ims/src/service/generic/tests/ssa_tests.rs
crates/providers/mainframe-env-ims/src/service/generic/tests/ssa_tests/last_tests.rs
crates/providers/mainframe-env-ims/src/service/generic/tests/ssa_tests/last_tests/fences.rs
crates/providers/mainframe-env-ims/src/service/generic/tests/ssa_tests/last_tests/recovery.rs
crates/providers/mainframe-env-ims/src/service/generic/tests/ssa_tests/last_tests/publication.rs
crates/apps/mainframe-env-server/src/product/tests/ims_package_tests.rs
crates/apps/mainframe-env-server/src/product/tests/ims_package_tests/ssa_last_tests.rs
crates/apps/mainframe-env-server/src/product/tests/ims_package_tests/ssa_last_tests/recovery_tests.rs
docs/delivery/subsystems/ims/programming-status.md
docs/decisions/0039-last-direct-child-ssa-selection.md
changes/unreleased/ims-last-direct-child-20261002.toml
docs/documentation-registry.json
docs/README.md
docs/generated/documentation-manifest.json
```

The bounded implementation is locally verified. Independent fail-first Memory
and SQLite public-provider and signed-coordinator C3 witnesses failed with
Unsupported before runtime admission; setup errors are retained separately.
Final provider proof executes 14 tests, including 12 root/intermediate/last
GNP/GHNP cases, four separate empty-root cases and six GU-independent GN/GHN/P
parentage cases across the two backends. Signed proof executes six test identities:
five pass in `candidate-04`, and the corrected real CHKP/XRST test passes in
`candidate-05`; the earlier aggregate failure is not relabeled. Signed finite
starts include 16 forward cases, four empty-root cases and four AC/AM conditions.
Both routes prove separate-process SQLite retained versioned holds and exact
replay, while file reopen and actual CHKP/XRST prove child parentage/no hold and
explicit root GU reestablishment. Canonical execution-context changes conflict;
exact replay uses the original identity and never recreates a hold.

Remote-only/local logical declarations each have genuine public negative controls
on both backends with empty-child physical graphs and real GHU root parentage;
four real retained-link cases also exercise the engine exclusion. Proposal replay
capacity, real session CAS conflict and lost acknowledgement retain existing
publication/UnknownOutcome owners. AC/AM preserve cursor/hold/database contents
while existing observer/receipt publication may advance row versions. Real
deletion on another PCB invalidates removed authority through the existing owner.

Affected regressions pass: 25 provider SSA tests, 46 actual checkpoint/recovery
tests and nine signed secondary SSA tests. Strict affected all-target/all-feature
no-deps Clippy, fmt/deny/docs/changelog/IMS catalog/assurance matrix/schemas/spec/
coverage and all declared boundary, API and supply-chain policy gates pass.
Receipts are external under `ssa-last-runtime`, with original candidate hashes,
commands, failures/repairs and Cargo cleanup. Documentation-only final packaging
rechecks the changed docs and source-credit gate; unchanged runtime receipts keep
their original identities. No tests return early for a missing PostgreSQL URL.
PostgreSQL parity, raw CALL/TM, root qualification ambiguity, broader L forms,
manager integration, accepted IR/HUMAN/official/licensed and full IMS-1403/IMS-1401/
v0.14 obligations remain pending. ADR-0039 stays Proposed. Source pins, source-only
seal and zero source credit are preserved; no null-slot lane is imported.

## IMS-1401.ssa-last-position-sources (declared source-only leaf)

Parent IMS-1401 and the full v0.14 manager goal remain incomplete. This leaf
starts from `a8deb3d8a97be2660cfa0d38327ff246a194a910`, target 0.14.0. Its
scope is registration of exactly three existing IMS 15.6 archived topics:
L command code, position after a successful call, and position after an
unsuccessful call. The separate `ims-ssa-last-position` scope uses the existing
later-manifest mechanism with semantic_authority=false and coverage_credit=0.
All previous publication pins, accepted IR and catalog denominators are preserved.

Dependencies are the manager-authorized immutable archive identities, matching
shared IMS TOC, existing registry/source reader and cache runbook. Acceptance is
retained-topic-path-first SHA/byte and archive metadata binding verification,
actual offline search/read of all three registered topics, an external finite
two-level GNP-L exhaustion-position interpretation for manager review, normal
docs generation/check, dependency policy, changelog, IMS catalog, schemas/spec,
affected source registry checks, and exact-path generated leaf seal/check with
one local commit. No runtime tests are needed for this source-only delta.

Exact product/docs allowlist: `conformance/0.14/manifests/ims-ssa-last-position-topics.json`,
`conformance/0.14/manifests/index.json`, this status file,
`changes/unreleased/ims-ssa-last-position-sources-20261002.toml`, and the normal
`docs/generated/documentation-manifest.json`. External reports and command
receipts stay under worker-receipts/v014-completion-20261002/ssa-last-source.
There is no fetch, refresh, source-body Git copy, product semantic change,
shared contract/schema extension, ADR0039, HUMAN/official/licensed acceptance,
push or PR operation. F/L/U/V/W runtime admission remains Unsupported; the
future bounded L leaf and its failure-position expectations need separate
manager review. Cargo cleanup ends each verification/generator/seal sequence.

The bounded source registration is verified: baseline
`ibm-ims-15.6-ssa-last-position-2026-09-11` binds L command code
`ims_lcmdcode.htm`, successful position `ims_currentpossuccess.htm`, and failed
position `ims_currentpostafterfail.htm`, topic-set SHA-256
`804ec48438f467df2e8afc4bac243264ec0a0f9ebee4454e749d6c8e67b79cb6`.
The retained topic-path files were absent; exact archived bodies/metadata and
the unchanged IMS TOC matched. The repository reader searched and read all
three complete topics through the new registered scope using an external cache.
The external disposition proposes finite empty/exhausted GNP-L transitions
from those sources and existing GNP/hold rules, while preserving the unresolved
same-PCB hold and parent-level qualification distinctions for manager review.
This source leaf provides zero behavioral/conformance credit and no runtime
admission; the manager's full v0.14 and future L semantic leaf remain open.

Root integration preserves worker seal
`6bca4715d3ebac54aec1d89bc70a8095c6e187b9` and its original five-file evidence.
The manager independently searched/read the three registered sources and the
actual pinned hold-call discussion, then approved a separate bounded runtime
leaf for only one unqualified direct-child L SSA in GNP/GHNP. Its attempted
same-PCB exhausted search cancels the old hold through the existing owner;
the forward remaining interval stays parent-bounded. This is a source-derived
composition, not an IBM dedicated failure example or an accepted HUMAN rule.
The optional root-qualification prefix, including a matching prefix, is excluded
until its position/hold disposition is settled. Runtime work remains isolated
and unintegrated; no L execution credit is granted by this source seal.

## IMS-1401.cobol-dli-call-boundary (declared contract-gap leaf)

Parent IMS-1401 remains open. Clean entry `9e97502604d61440047e4c1643190bb178fef5df`
is preserved on `codex/v014-gsam-record-formats-20261002`. This branch,
`codex/v014-cobol-dli-call-20261002`, starts from exact manager
`56871bf83f38ed86a615b0a29fda298a467289b4`; no format-worker delta is consumed.
Target is 0.14.0. The accepted dependency identities and pending participant
dispositions below remain in force.

Before edits, inspection identifies a genuine shared contract gap. The generic
CALL producer erases passing modes and guest address/alias identity into copied
`mainframe-env.cobol.call@1` values. Signed database PCB metadata lacks KEYLEN,
raw-mask capacity and linkage/PSB-order binding; existing typed results lack
complete validity-tagged raw feedback. A raw adapter cannot infer these from
names, copied bytes or current position. No unintegrated feedback DTO is consumed.

Bounded scope: actual compiled COBOL CALL CBLTDLI GU/GHU/GN/REPL witnesses,
signed selected CardDemo package and real coordinator/product routes, independent
literal function/SSA/PCB/I/O bytes, rejection/no guest or provider mutation,
Memory and SQLite reopen, source registration/contract-gap ADR, unique fragment
and normal docs. Catalog context is
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0005/:0006/:0015`; COBOL CALL is
`ibm-enterprise-cobol-6.5-2026-05-31:procedure-statements:0005`, both verified
against the committed catalogs. Required raw execution, status/level/key
writeback, selected-PCB holds/undo, alias/capacity/encoding/replay acceptance
remain pending unless their owning contracts prove them. Correct rejection
does not earn required-execution credit.

Owners: shared compiler/IR/interpreter guest storage and CALL frame, host
contracts/dispatcher, IMS metadata/context/provider and selected signed product
route. No new interpreter, memory engine, PCB registry, coordinator, persistence
or private API is admitted. Existing service/execution, rows/providers, product/ims,
request and canonical owners remain unchanged. Mixed SSA, selected-secondary
checkpoint, feedback algorithms and API-doc ratchets are outside this leaf.
BMP/JCL/CMPAT/other language/distributed classes remain unproved and fail closed.

Acceptance for this gap deliverable: substantive fail-first compiled signed-route
witnesses, passing explicit rejection regressions, focused affected tests and
strict all-target no-deps Clippy, fmt/deny, execution/effect/provider-row/storage/
participant/security/retention/typed/module/supply-chain/catalog/assurance/schema/
spec/docs/changelog checks, exact-path leaf seal/check and one local commit.
Receipts remain external under worker-receipts/v014-completion-20261002/
ims-cobol-dli-call; Cargo clean ends each sequence. No refresh, delegation,
licensed/official/human/participant/parent/release acceptance, push or PR.

The bounded deliverable is a contract-gap packet, not an implemented raw adapter.
[ADR-0033](../../../decisions/0033-cobol-dli-call-boundary.md) proposes the shared
owned reference/mode/atomic-copyout frame, signed PCB entry/KEYLEN binding and
complete validity-tagged feedback decisions. A separate five-topic invocation
supplement, `ibm-ims-15.6-cobol-dli-boundary-2026-09-11`, is registered with zero
credit; historical baselines are unchanged. Selected IMS 15.6 and COBOL 6.5 pins
matched exact archived SHA-256/byte counts after retained topic-path checks;
repository plain-text parsing and offline search/read were used. No selected
source was unavailable and no refresh or publication body was committed.

Current-candidate evidence has three focused tests, including twenty actual
compiled signed-route cases across Memory and file SQLite with reopen. GU/GHU/
GN/REPL, optional count, two unbound PCB canaries, short/malformed/aliased areas
and absent Program capability are exercised through the real coordinator and
product Program provider. Independent literal output and initialized guest
storage remain unchanged; IMS row payloads and versions remain unchanged.
The granted route returns `PROGRAM-NOTFOUND:CBLTDLI`; the denied route returns
Unauthorized. The fail-first receipt separately observes erased REFERENCE/
CONTENT identity and missing CBLTDLI ABI registration on both signed backends.
These are substantive raw producer/route gap witnesses. Typed Schedule/Load/
Checkpoint/GHU only establish a selected database and held control occurrence;
they earn no raw CALL credit. Malformed cases prove absent-adapter rejection,
not raw validation or IMS status/PCB writeback.

Focused tests, strict affected Clippy and all declared mandatory guards/gates
pass. The new ADR was added to the normal documentation registry after its
initial missing-registry failure. Receipts preserve earlier unsuccessful test
setup attempts separately; only the final candidate tests are passing evidence.
No runtime/schema/migration or frozen facade change is introduced. Compatibility,
drain/reconcile and coherent backup obligations remain those of the manager base;
no backup/restore or subprocess restart acceptance is claimed. Shared CALL-frame,
IMS entry/metadata and feedback owners must admit a coherent class before raw
status/cursor/hold/update/undo/replay/copyout acceptance or parent closure.
## IMS-1405.secondary-index-checkpoint-restart (bounded integrated leaf)

Historical worker clean entry `531f39c137ab5dad00d8db72f28b083aea2b19d3` and its branch are
preserved. This lane starts directly from manager seal
`06ed341181151c5e9b526989f7c2ee5f2fb6b9db`, without the old TM gap branch.
Parent IMS-1405 and target 0.14.0 remain incomplete. Catalog identities are
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0002/:0023/:0025`, with
`:0005/:0008` and hold/update consumers. Execution is signed selected DB-batch
CALL metadata and existing full-function root-target secondary navigation.
Physical source/current paths must have unique sequence keys. Nonroot inverted
hierarchies, aliases, DEDB, optional fields, NULLVAL, physical RSA and timestamps,
HDAM missing-key resume, and licensed tie-order equivalence remain unsupported.

Owners: additive SavedPcbPosition identity/validation, one selected-secondary
checkpoint helper and minimum engine/checkpoint hooks, focused provider and
signed-package tests, unique fragment and boundary ADR. Preserve existing GSAM
identity/future fields, primary historical bytes/digests, selected_index_field,
ordinary PCB/Q/holds, generic integrity/UOW/publication and recovery replay.
The original worker changes no GSAM formats, mixed SSA evaluator, PCB feedback,
TM or backout algorithms. Integration below composes the root's generic Batch
undo and epoch/incarnation; its already-removed GSAM settlement helper stays absent.

Mandatory obligations: fail-first public CHKP/XRST; same-PCB GU/GN/GNP/hold;
independent PCBs and mixed primary/GSAM/secondary PSB; binary/composite search
and distinct source/target; deleted/moved/replaced identities GE without revival;
SAF/malformed/context/limits without mutation; replay after later work; actual
row CAS, missing-image, lost-ack/UnknownOutcome and read-only reconciliation;
Memory/file SQLite reopen and real process phases; signed canonical coordinator.
Run focused tests, strict scoped all-targets/no-deps Clippy, and mandatory
execution/effect/provider-row/storage/participant/security/retention/typed/module/
supply-chain/deny/catalog/assurance/schema/spec/fmt/docs/changelog gates.
Exact allowlist seal, feature commit and committed --check follow verification.
Receipts stay outside Git/targets under worker-receipts/v014-completion-20261002/
ims-secondary-index-restart. No official, human, licensed, parent or release
credit, network refresh, delegation, push or PR is authorized.

Before semantics, eleven selected topics matched committed hashes and byte
counts after checking the retained topic-path root, then the SHA archive and
explicit ims-1405/ims-1403 caches. ibm_docs.py search/read and its plain_text
parser were used offline. Baselines: ims-recovery-utilities-contracts
`7418508fe1db54bbc8374fc1b8ea4cfd45a3e9aceff75f9734b75cb0afd4bde8`,
ims-database-contracts
`3b819e69f7ce608d66bf1b8f195556047024333374e6f77413af29822960e38b`,
ims-programming-contracts
`119dd5e589399cb023f70c7a28fa9a1a937be1fa2c2393679ab65402ac735182`.
Relevant topics: apr/ims_xrstcall.htm, ims_symbolicchkpcall.htm;
apg/ims_restartingprogramandcheckforpos.htm, ims_ssassecondaryindex.htm,
ims_secondaryindexlogicalrelationships.htm, ims_currentpos.htm,
ims_imsdbdbpcbmask.htm; dag/ims_howseindexmaint.htm,
ims_howhierrstruc_fullfunction.htm; apr/ims_gughucall.htm, ims_gnghncall.htm.
Full SSEPH2_15.6.0 paths, exact SHA/bytes and resolutions are in external
sources.json. No selected source is unavailable or mismatched; unselected cache
entries were not audited. No publication bodies enter Git.

### Implemented selected secondary restart projection

SavedPcbPosition's optional `secondary` variant is exclusive with GSAM and the
historical hierarchy key. It binds selected PSB/PCB/database metadata, index,
binary/composite search key, distinct source/target/current occurrence witnesses
and unique physical paths. Witnesses are issued on existing engine records in
the existing CHKP proposal. The engine's secondary GU has one exact-source
filter; existing pointer ordering, SSA parser/shared selected_index_field and
parentage/hold authority remain unchanged. XRST clears holds and establishes GU
parentage. Data-only REPL preserves witnesses; changed index keys invalidate
them even after changing back. Deletion/reinsertion and reload cannot revive an
old witness. GE continuation uses actual selected GN in keyed-root order.

The historical worker candidate passes 207 IMS unit tests and the 44-test public
application recovery dispatch suite, two historical GSAM and two new secondary
retained contract tests, plus existing participant/TM consumer tests. Standalone
process-worker entries and empty doc-test/filter binaries earn no scenario
credit. Substantive parents execute three real SQLite checkpoint/restart/
continuation processes and assert outcomes. A later focused additional test
proves two saved source pointers returning the same target resume independently
on Memory and SQLite. Final focused secondary dispatch passes 12 entries, including
11 substantive cases plus the environment-only process worker; real three-phase
child execution is asserted by its parent. Actual missing database images reject
XRST without mutation, and nonunique physical source paths reject CHKP without
mutation. These receipts are separate from the prior 44-test suite.
The signed public package/canonical coordinator suite passes all 22 tests,
including two new Memory/SQLite secondary CHKP/XRST/reopen cases. Strict IMS
and server Clippy passes with all-targets/no-deps and warnings denied; the later
test-only addition passes strict IMS Clippy separately. No inherited warning is
waived in the selected crates. Dependency warnings outside no-deps scope remain
distinct. Real backend database CAS conflicts, capacity, CHKP/XRST lost-ack,
read-only observation, missing receipts, replay after later work, auth/context/
malformed/limits, corrupt images, binary composite values, child field collision,
independent PCBs, holds/GNP/changed-key parentage and mixed FF/GSAM/secondary
proposals are exercised. Initial expected and fixture failures remain retained.

Historical worker receipt groups are `final-code.json`, `final-secondary.json`, `policy.json`,
`sources.json` and `pinned-source-reader.json` under the external leaf receipt
directory above. All eleven selected topic bodies, exact SHA/bytes and scoped
reads pass under Python 3.12.13. The programming-cache search reports one
unselected missing entry; this partial scope is not a whole-cache completeness
claim or a missing selected source. Shared TOC verification succeeds. The
documentation registry in that worker registered a colliding ADR-0029. The
integrated leaf registers [ADR-0032](../../../decisions/0032-selected-secondary-checkpoint-position.md)
and preserves existing GSAM ADR-0028 and PCB-feedback ADR-0029.

The original worker adds no SQL migration, namespace, request/result canonical domain, retention lifetime,
coordinator, address registry, lock service or backout algorithm changes.
Absent fields preserve old serialized positions/images and checkpoint digests;
GSAM formats/identities remain intact. Rust saved-position literals require
`secondary: None`. Older writers/readers must be drained before new retained
secondary rows are admitted. Stop admission, settle UOWs/holds/Q and unknown
effects, and retain a coherent database/session/recovery/selected-metadata/
journal/audit backup before upgrade or downgrade. Coherent backup restoration
and retention expiry are not certified by reopen tests.

Original worker handoff obligation: retain the exact-source GU hook, additive record witness
and changed-index-key invalidation when integrating generic Batch retained undo
and epoch/incarnation. Restoring an old raw undo image can otherwise revive an
invalidated witness; integrate the manager's authority rather than adding a
second generation registry here. The GSAM batch settlement helper is unchanged
except its saved-position literal and must be removed by the manager as planned.
Primary/GSAM fresh behavior, ordinary Q/release/commit and foreign integrity-read
fences remain owned by their existing modules. Unsupported physical/nonunique
path applicability, nonroot inversion/aliases/DEDB, HDAM missing-boundary resume,
accepted participant/lease composition, backup/retention exercise, official rule
acceptance and licensed differentials remain parent obligations. This bounded
leaf never marks IMS-1405 or v0.14 complete.

The historical worker's required execution/effect/provider-row/storage/participant/generated-participant/
security/retention/typed/module, offline supply-chain/dependency/license, IMS
catalog/assurance, shared assurance inventory, schema/spec, fmt/changelog and
docs generation/check gates pass. New production owners are 220 engine, 79
retained contract and 154 adapter lines; the existing checkpoint owner is 798,
database module 638 and frozen owners do not grow beyond their ratchets.
Command exits and nonzero test outcomes are retained in the external receipt
directory. All Cargo build targets are cleaned after each verification sequence.

The historical worker's extra global public API-doc ratchet was blocked by contract
owners: execution-api 305 missing-doc items against 176, host-api 2,100 against
1,128. Neither package nor its ratchet changed in this leaf. The isolated
`acceptance-final-10.log` records this failure; it is not reported as a passing
required docs check. The initial API-doc build also raced cleanup when incorrectly
run alongside Cargo; that infrastructure failure is retained separately, and the
serialized retry diagnoses the actual inherited documentation debt. No warning
allowance or other-lane documentation rewrite was introduced. Manager owns that
repair and integrated candidate resealing; licensed/official credit stays zero.

### Secondary checkpoint integration with application backout (2026-10-02)

The integration branch starts at manager commit
`0634ffc221aa977c66a063ba792ab22b379a248d` and consumes only the exact
26-path delta of sealed `89bf004e332bde150541e106976328feaacb0673` from
`06ed341181151c5e9b526989f7c2ee5f2fb6b9db`. Its prior sealed worker branch
and the unconsumed TM gap `531f39c137ab5dad00d8db72f28b083aea2b19d3`
remain unchanged. Other newer unsealed worker branches are excluded.

Witness issuance binds the root's existing UOW incarnation/epoch in addition
to canonical request, actual database CAS version and engine image. The existing
generic backout publication reconciles restored occurrence witnesses against
the current actual image after validating the original owned-image digest.
Data-only replacement retains identity; changed index keys and deletion cannot
revive an older identity through named ROLS, ROLB, terminal ROLL or generic
Rollback. The reconciled image remains owned by the same retained UOW, including
a subsequent full rollback after named ROLS. CHKP advances the existing epoch;
rescheduling changes incarnation. Old named tokens are rejected across both.
Retained checkpoint references can still reposition live occurrences after a
legitimate new schedule. No additional generation store or dispatcher is created.

Root pristine integrity read-source, dependency CAS/Q guards, rich SSA field
resolver, feedback projection, GSAM output restart and common execution pipeline
are preserved. The older worker's removed GSAM Batch settlement helper is not
reintroduced. Only its saved-position literal gains `secondary: None`.
Normal documentation generation incorporates ADR-0032 registry metadata and
links; the feature's unique existing change fragment is retained.

Offline source review verifies sixteen selected IMS 15.6 pinned HTML identities
and reads. The eleven historical selected topics above plus SETS/SETU, ROLS,
ROLB, ROLL and intermediate-backout topics inform the integration. Catalog
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0017/:0018/:0020/:0021`
supplements the unchanged checkpoint/navigation rows. The programming search's
one unselected missing cache topic is reported without a cache-completeness
claim. All selected retained-path checks resolve matching archive bodies; no
selected source is missing or mismatched. External actual command logs and
candidate identities live under worker-receipts/v014-completion-20261002/
ims-secondary-manager. Old worker receipts retain their historical identities.

Focused public recovery, signed selected-secondary package/coordinator tests,
both backends and independent SQLite checkpoint/restart/continuation child
phases are the integration acceptance scope. The composition regression adds
named/full/terminal/generic backout, data-only versus changed-key/deleted-source
witnesses, lost selection, epoch invalidation and incarnation isolation.
Mandatory policy and unchanged exact API-documentation checking apply to the
manager base; historical worker API-doc failures are not this candidate's result.
Nonunique physical paths, optional fields/aliases/nonroot inversion/DEDB remain
Unsupported. No acceptance rules, pins, thresholds or denominator change;
official/human/licensed credit stays zero. Parent IMS-1405, participant/lease,
coherent backup/retention, root composition and v0.14 acceptance remain open.
## IMS-1401.mixed-ssa-evaluation manager integration (2026-10-02)

The integration branch consumes only manager base
`0634ffc221aa977c66a063ba792ab22b379a248d`, the separately sealed guard
`f1efa47f167ae426b9ce3c3356e4c14f87093697` from bdbca36, and the evaluation
delta from `dad91c4a5e4fbb39b7b42bcd65917381abd050eb`. Probe repair 5ed2e0c1
remains separately owned by the root manager. Historical leaf receipts retain
their original candidates; integration receipts are external in
worker-receipts/v014-completion-20261002/ims-mixed-manager.

Before semantic changes, all four exact pins in
`ibm-ims-15.6-mixed-ssa-supplement-2026-09-11` were searched and read offline
with ibm_docs.py after retained-path checks and SHA/size verification. Catalog
context is `ibm-ims-15.6-dli-2026-08-31:dli-call-families:0005/:0006`, with
inherited :0004/:0008/:0015 maintenance interactions. The supplement registers
only four reference bodies; existing manager manifests and registry rows remain
byte-exact. Source presence grants zero semantic or execution credit.

The same matcher now evaluates OR-separated AND sets. Pure selected-XDFLD
Independent AND admits distinct EQ groups correlated through the existing
index_target authority to the same target, in qualification order, once per
target per group. Physical-field qualifications use dependent behavior.
Independent ranges, repeated keys, mixed independent groups, primary Independent
AND, unrepresentable continuation contexts and primary HDAM/DEDB root multiple
qualifications remain Unsupported. Canonical/DTO/AST/row/SQL schemas are unchanged.

The manager's target-only selected_index_field resolver body and existing
physical-child/root-virtual collision regression are retained. Its return
lifetime is made explicit for the new correlated-index helper; the duplicate
leaf collision test is omitted. There is one matcher and secondary pointer
traversal. Manager service/execution.rs and shared generic/tests/session_cas.rs
retain authority, with the existing nested CAS re-export used by the new tests.
Feedback, backout, GSAM, authorization and read-integrity owners are unchanged.

Focused integration verification covers mixed logical sets, binary boundaries,
same-target correlation, disjoint-group order/duplicates, signed selection and
the real durable coordinator, malformed/denied requests, replay/conflict,
Memory/file SQLite reopen, actual CAS refusal/lost acknowledgement and three
independent SQLite child phases. Mandatory policies and the unchanged public
API documentation ratchet are required. The older base's three AMS probe
citations are not repaired or bypassed here; an unchanged failure is recorded
once for root composition. Content seals grant no official, human, licensed,
participant, parent IMS-1401 or v0.14 acceptance.

Integration runtime receipts record 66 passing parent tests: eight new provider
cases, three signed/coordinator cases and 55 affected regressions, including the
retained manager collision case. The process parents require real configured
SQLite children; unconfigured helpers receive no parent credit. The initial
two CAS/lost-ack failures were fixture assumptions about the existing UOW CAS
envelope. The repaired fixtures retain full-row stability on refusal and replay,
assert one publication version advance and unchanged database image bytes, and
use the unchanged shared CAS helper. No production owner was altered to fit them.

Strict scoped Clippy, fmt, deny, supply-chain/license notices, catalog/assurance,
schemas/spec, execution/effect/row/storage/SAF/retention/participant/module guards,
participant generation, changelog and the unchanged API documentation ratchet
passed. Mandatory coverage policy failed once on the unchanged three AMS probe
citations (6202/6080/3050 versus actual 6194/6072/3052); later aggregate coverage
subchecks were not reached. No retry or out-of-allowlist repair occurred. Root
must compose its separately sealed probe repair and verify the affected gate on
that actual candidate. Final docs generation and exact feature sealing are
packaging checks, with receipts bound to the actual tested inputs and candidates.

## IMS-1403.gsam-record-addressability (bounded slice implemented)

Parent: IMS-1403. Consumed clean base: `65d2904d47155a9c0cffc6c21948eb5c76feb68c`,
preserving the sealed public SSA feature and manager per-PCB/UOW authority.
Branch: `codex/v014-gsam-record-addressability-20261002`. Scope: additive owned
GSAM GU/GN/ISRT request, logical record address output and lookup, bounded
canonical encoding, public provider and selected-package adapters, engine
address helpers and focused tests. Catalog context:
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0005/:0008`; checkpoint/restart
`:0002/:0023/:0025` are consumed interfaces and followup, not owned recovery semantics.
Review execution-context/GSAM-PCB/PROCOPT/RSA/no-SSA applicability against exact
pinned local sources before semantic edits. Obligations: independent PCB position,
generated-address roundtrip, wrong-database/stale-address rejection, EOF/status/
position, insertion, SAF-before-observation/mutation, canonical replay/conflicts,
atomic CAS/failure/unknown outcome, Memory and SQLite fresh reopen, historical
canonical bytes and retained row readers. No physical RSA, full-function key or
guessed record ordinal is admitted as a host address.

Host contracts own new bounded operands/results/canonical modules; IMS owns
the adapter and engine helpers. Shared storage, row CAS, replay, SAF, participant,
per-PCB maps and local UOW remain authoritative. No dependency, store, coordinator,
recovery engine, secondary-index/Q/typed-CHKP-XRST module is owned here. Minimal
enum/export/provider/selected-package dispatch integration will be reported.
Acceptance: fail-first public-route regression, focused passing host/provider/
selected-package tests, strict scoped Clippy, fmt/deny/docs/changelog, affected
catalog/assurance/schema/shared guards and exact allowlist feature seal/check.
Receipts stay outside Git/targets under the named worker receipt directory.
No official row credit, licensed campaign, network refresh, parent completion,
push or PR is authorized or claimed. Manager owns existing aggregate lint/module
guard repairs and final integration re-sealing.

This declaration preceded semantic changes. The implemented additive
`HostRequest::ImsGsam` / `HostResult::ImsGsam`, `ImsService::execute_gsam` and
`ProductServer::ims_gsam_selected` connect GU/GN/ISRT through the existing
provider factory and signed metadata/publication fences. Only standalone
`DbBatch`, GSAM organization, fixed length metadata, G/GS retrieval and L/LS
insertion are admitted. The generic metadata Database PCB denotes GSAM when
its referenced organization is GSAM; applicability is validated as Gsam PCB.
No hierarchical SSA, hold, Q, replace/delete or full-function key is accepted.
`save_address` models the optional fourth GN/ISRT output; GU accepts a prior
opaque token or Beginning. AH and AJ leave position/data unchanged; GB clears
position and the following new GN starts at the beginning. Status/replay
bookkeeping is published normally. Beginning clears position and returns no
record; its raw IBM I/O-area behavior is unproved. Successful GU positions the
selected input PCB at the addressed record. Invalid-address position retention
is an explicit host contract, not an established IBM equivalence assertion.

The address is the normalized database name plus 32 opaque bytes. Issuance
binds a separate domain, canonical request, shared database row CAS version
and engine image/occurrence identity. It is not a record ordinal or IBM physical
RSA. Existing record identities survive append and reopen; rollback/load
replacement invalidates removed identities even when bytes/occurrence recur.
First saved-address GN on retained records materializes identity using the
existing local UOW witness and atomic publication, which can conservatively
acquire that database's fence. No-save GN does not materialize an identity.
Insertion uses the same image/UOW authority. This host mutation/position
contract does not establish physical I/O, record-lock, or operational parity.

Compatibility: optional engine-record `gsam_address` and optional replay-output
`gsam` fields retain absent historical readers/serialization; the existing
object envelope/namespaces and all old canonical bytes stay fixed. Retention
now validates/hashes the additive result variant for GSAM receipts only.
Live identities are bounded by existing record limits and survive independent
replay pruning; removed identities in retained replies cannot locate records.
No SQL migration, new retention target or automatic history rewrite. Downgrade
requires stopping new GSAM calls, draining active UOWs and preserving compatible
image/replay/checkpoint backups: old writers can lose identity fields and old
strict replay readers reject new GSAM receipts. See
[ADR-0034](../../../decisions/0034-gsam-logical-address.md).

Integration exceptions: exhaustive request/result enum and canonical arms,
exports/module declarations, existing provider operand dispatch and replay
output, retention's result-family hash choice, selected-package helper's generic
return type, and two test module declarations. Per-PCB/UOW/Q/secondary-index/
typed-recovery owners' modules remain unchanged. The sealed SSA implementation
is preserved; its selected helper now accepts either result type behind the
same selection fences.

### Exact offline sources

All fourteen selected registered topics and the relevant IMS TOC matched the
committed pins. The retained root was checked first; exact bytes resolved from
the sharded SHA archive and were parsed locally by `ibm_docs.py search/read`.
No selected exact source was missing or mismatched, no topic was redownloaded,
and no IBM body entered Git. The partial reader's unrelated missing topics do
not represent a whole-cache audit or unavailable selected evidence.

Database baseline `ibm-ims-15.6-database-contracts-2026-09-11`, topic-set digest
`3b819e69f7ce608d66bf1b8f195556047024333374e6f77413af29822960e38b`:

| Topic under `SSEPH2_15.6.0/com.ibm.ims156.doc.apg/` | SHA-256 |
|---|---|
| `ims_retrieveinsertgsamdb.htm` | `8dcb020dc55ed37c1cdf304e3cc7f6c0a724edb2868c910f4055e81ce2bd2ae1` |
| `ims_processinggsamdb.htm` | `709a5195deb5a7b505f5d70d456d9a004bcf19078994583390f7edd0640f80c5` |
| `ims_gsamstatuscodes.htm` | `a8531909e5276c69924ffb11b89a1e09d148ac0ff1ab8c8fec6843e107422592` |
| `ims_gsamrecordformats.htm` | `c6eb5c8d24469ed275cd153ab191ff89f9ccee98b6e3cb921bbb0d7943aa50d4` |

Programming baseline `ibm-ims-15.6-programming-contracts-2026-09-11`, topic-set
digest `a0ab40de8ec8c01a5cd43d4cab98f04c932f4b0b161e544da1d7d83aaac05594`:

| Topic under `SSEPH2_15.6.0/com.ibm.ims156.doc.apg/` | SHA-256 |
|---|---|
| `ims_pcbmaskgsamdb.htm` | `033ee82b96d160db01409f168a44f9076429bd090e70a49d83585f774755dcd3` |
| `ims_processingoptions.htm` | `bb549e17230c1990ac0b5b5f0386512493bcd5552b3b4762bd7fc4436a923dd4` |
| `ims_currentpos.htm` | `07aafcddb9b30591eef1da5ad50bfe8bc3c1e9b9e9e51b56b27d39bf6adf5673` |

The same scope's `apr/ims_gughucall.htm`
(`0a9b433d9e38e58c125232a94147acd309e5bbbd7baf3c8cb900a3e8fdf6c8b9`)
and `apr/ims_gnghncall.htm`
(`063ff108614ee13694ea7df2f7b39647447059590162614da56de2aa2eb49cb4`)
were also read. The GSAM-specific retrieve/insert topic supplies the exact ISRT
and saved-RSA rules; no generic full-function insertion assumption is substituted.

Recovery baseline `ibm-ims-15.6-recovery-utilities-2026-09-11`, topic-set digest
`7418508fe1db54bbc8374fc1b8ea4cfd45a3e9aceff75f9734b75cb0afd4bde8`:

| Topic under `SSEPH2_15.6.0/com.ibm.ims156.doc.apr/` | SHA-256 |
|---|---|
| `ims_basicchkpcall.htm` | `1029208c47f44b8a0144472c8127767b18140c57be70850fdfa00da83ef1743a` |
| `ims_symbolicchkpcall.htm` | `87ede5820b177ea850473dda852c424a7b4a8f5a93dd06c68a0b95c11a4093b5` |
| `ims_xrstcall.htm` | `aff46320869b8910ab9916011c8d722a5e044f9e33970d2996e0501204046cb6` |

Also read from that scope's `apg/`: `ims_chckpntcallsintro.htm`
(`c3aaf84e538be688d44af8fe9072e6cb00c47e4f9e46277f2ba22aa053be013d`)
and `ims_restartingprogramandcheckforpos.htm`
(`ac6ec41de956052fe05cb68efd58b59793af4c0903458cf447852b93b0211670`).
The shared TOC hash is
`aaa12586b41e9994921bfddce588b186dc5bdda8ab253db054ae1e5014d6f618`.

### Unsupported classes and recovery handoff

Unsupported: BMP/JBP region binding, other contexts, RECFM V/U and LL/RDW,
undefined-record PCB length, IBM 8/12-byte RBA/TTR/volume/displacement layouts,
INIT RSA12, tape/DASD selection, data-set concatenation, OPEN/CLOSE/PURGE and
physical I/O error conditions, raw PCB key-feedback bytes, symbolic GSAM
checkpoint/restart, embedded DL/I/AIB adapters and licensed equivalence.
The source requires symbolic CHKP/XRST for GSAM and rejects basic checkpoint
support. Existing basic checkpoint code is consumed unchanged and grants no
GSAM recovery credit. The typed CHKP/XRST owner must extend the existing
`SavedPcbPosition` interface with a discriminated address/beginning/EOF form,
capture it before symbolic CHKP, retain engine identities, and resolve GSAM
positions with the existing `RecoverySession::xrst` resolver and selected PCB
helpers in one atomic transition. Do not serialize a GSAM token as the current
full-function `segment_key`. Prove statuses and process restart there. No second
recovery engine or checkpoint DTO is created here.

### Scoped verification and integration limits

Fail-first public provider dispatch returned Malformed before the GSAM arm was
connected. Final focused `cargo test -p mainframe-env-ims public_gsam` passed
11 tests. The host/IMS package run passed host 108 unit + 9 integration and
IMS 128 unit + 17 integration tests before the final limits/format/auth-failure
regression was added; that added regression passed in the final focused run.
`cargo test -p mainframe-env-server product::tests::ims_package_tests` passed
all 10 signed-package tests, including preserved SSA selection. Earlier GSAM
package fixture failures (missing alternate PCB and foreign execution replay)
were corrected without changing runtime authority; foreign replay is now
explicitly asserted to conflict.

Passed: strict `cargo clippy -p mainframe-env-host-api -p mainframe-env-ims
--all-targets --no-deps -- -D warnings`; `cargo fmt --all -- --check`;
`cargo deny --offline check`; `cargo xtask ims-catalog`,
`ims-assurance-matrix`, `schemas`, `spec`, `docs`, and `changelog`, each with
`--check`. Documentation was regenerated normally after registering ADR-0028
and supplying its required metadata. `python3 -B tools/supply_chain.py check`
passed after correcting the initial CLI invocation. Shared Python execution
route, effect encoding, provider rows, storage profile, enterprise authorization,
retention lifecycle, typed semantic boundary and transaction participant guards
passed; `generate_transaction_participant.py --check` also passed.

Receipts and exact argv are outside Git/targets at
`/Users/tore/Library/Caches/mainframe-env/worker-receipts/v014-completion-20261002/IMS-1403.gsam-record-addressability`.
Every Cargo/generator verification sequence ended with this checkout's
`cargo clean`. No skips receive conformance credit. An exact reviewed path
allowlist supplies the scoped feature seal and committed `--check`; any manager
integration changes require re-sealing the changed blobs.

The inherited server strict Clippy 11 diagnostics, aggregate architecture/cache
and module guard blockers were not repeated or repaired here. The new production
modules are at most 172 lines. Minimal exhaustive integration changes increase
already over-budget blobs: product 6240→6241, canonical arms 3076→3086, host
request 2341→2350, IMS service 2157→2213. The manager owns their extraction and
guard repair. This is no aggregate guard pass, full-parent acceptance, official
row credit or release/promotion claim; IMS-1403 and symbolic GSAM recovery remain
open at their respective boundaries.

Manager integration consumes the sealed STAT/integrity/checkpoint/index/Q/SSA
candidate at `b0b2ee94`, rather than overwriting its newer authorities with the
worker base. GSAM's selected facade is folded into `product/ims.rs`; canonical
arms remain in `canonical/dispatch.rs`, host request methods remain in
`request/host_request.rs`, and the existing provider factory adds the GSAM arm.
One common `service/execution.rs` owns database/SSA/GSAM request validation,
authorization, digest/replay, fresh image/integrity fences and atomic publication.
GSAM preparation runs after exact replay and fresh read validation. Image
publication passes the current limits to the existing Q/UOW witness helper.
There is no second facade, dispatcher, recovery engine or lock authority.

The unchanged host result-validation implementation moves to
`request/host_result.rs`; the shared execution and replay-migration methods move
out of the IMS facade. The IMS facade is now below the 1,200-production-line
limit and loses its oversized exemption. The host request exact ceiling lowers
from 1,938 to 1,611. Source-based canonical replay, SAF ordering and retention
guards read the named execution owner with their original required assertions;
six focused Python tests include removal mutants for those controls. Initial
missing-limits compilation and facade-only locator failures are retained as
failures, not relabeled passing evidence. Worker-base evidence above retains its
original identity; final manager receipts are recorded separately.

Final manager verification: host 111 unit + 15 integration, IMS 191 unit + 41
integration, and 14 selected signed-package tests pass on this integrated
candidate. The `gsam` filter runs 12 tests (including the existing engine case);
unmatched integration filters earn no credit. Strict scoped IMS/host/server
Clippy, formatting, module boundaries, IMS catalog/assurance and schemas/spec
checks pass. Shared row, execution-route, canonical effect, storage, enterprise
SAF, retention, typed-boundary, participant and supply-chain guards pass after
bounded locator repairs; the row guard keeps its forbidden whole-state checks
and a removal/addition mutant regression on both IMS and MQ child owners.
Manager receipts are `gsam-integration.log`, `gsam-integration-scoped.log` and
`gsam-integration-policy.log` outside Git under the continuation receipt root.
The single initial compile failure is `gsam-integration-initial.log`; the three
locator failures are `gsam-policy-locator-initial-failure.md`. No older receipt
is relabeled as this candidate. Licensed certification remains excluded by the
user; official acceptance and parent completion remain unclaimed.

## IMS-1401.secondary-ssa-navigation (implemented bounded leaf, 2026-10-02)

Parent IMS-1401 remains open. Clean entry `174aa627330315024359a340cc8185ddfbfe2515`
is preserved on `codex/v014-integrity-read-visibility-20261002`; this leaf starts
from manager base `10ef210c67117d88a199e377ca01b66c00ae69da` on
`codex/v014-secondary-ssa-navigation-20261002`. Integrity-read integration stays
manager-owned; both operand forms retain the common execute_operands_at seam.
Consumed dependency identities and licensed-pending dispositions below apply.

Exact ownership: bounded database SSA/secondary selection helpers, generic/ssa.rs,
minimal selected field-catalog projection, focused engine/public-provider/signed
package tests, this appended status, unique fragment and routine docs manifest.
No host ABI, checkpoint/backout/GSAM/STAT/integrity algorithm, index maintenance
algorithm, cursor, store, coordinator, schema, dependency or official IR ownership.
Shared seam: supply the existing parser with selected XDFLD length metadata and
the existing secondary traversal with the existing rich SSA evaluator.
The mandatory provider-row guard also needs a locator-only integration repair:
the base extracted MQ/IMS persistence helpers to service/rows.rs, while the guard
still searches facades alone. Read each explicit child plus its facade, require
their existing module import, and preserve every required/forbidden pattern.

Catalog context is `ibm-ims-15.6-dli-2026-08-31:dli-call-families:0005/:0006`,
with :0015/:0004/:0008 mutation-maintenance interactions. Obligation classes:
named/offset binary relations and uniform Boolean qualification, XDFLD composite
bytes, source-to-target pointer identity and indexed order, applicable commands,
exact current/parentage/hold and GE/GB/GP/AK conditions, independent selected PCBs,
sensitivity/key-only/SAF before observation, malformed/context no mutation,
canonical replay/conflict, real REPL/DLET/rollback and primary maintenance,
Memory/file SQLite reopen and process exit, real publication CAS and lost-ack
UnknownOutcome. Supported contexts remain the existing full-function admitted
physical-root-target hierarchy. Nonroot inversion/aliases, NULLVAL/exits/SUBSEQ,
unsupported commands/mixed Boolean/raw/context classes remain explicit.
Selected-secondary symbolic checkpoint capture/resolution stays blocked; no
primary-key substitute is allowed. Recovery, participant admission, accepted
official IR, licensed differential and parent/release obligations remain pending.

Acceptance: fail-first public selected-provider and signed-package cases, focused
passing engine/provider/server/backend regressions, strict scoped all-target
Clippy --no-deps -D warnings, fmt, mandatory affected execution/effect/provider-row/
storage/schema/catalog/assurance/security/retention/dependency/docs/changelog gates,
exact-path seal/check for this leaf and one local completion commit. Receipts stay
outside Git/target under worker-receipts/v014-completion-20261002/
ims-secondary-ssa-navigation. No delegation, refresh, licensed run, push or PR.
Local checks/source review/sealing grant zero official or licensed credit.

### Implementation and source-derived behavior

The public canonical ImsNavigation variant now parses once with the existing
parser and a selected XDFLD field-length projection. Both rich and legacy
requests use the existing secondary pointer traversal and caller-owned PCB
cursor. Rich named/physical-offset predicates use the original binary relation
and uniform Boolean evaluator; index predicates read the actual pointer key,
including ordered composite bytes from a distinct source occurrence. No primary
order fallback, copied maintenance/expression algorithm or new cursor is added.
GNP checks parent-level qualification against that same selected pointer before
selection, preserving GE/GP and retained hold/current/parentage distinctions.
The existing sensitivity, key-only, SAF, replay, UOW, observer, update maintenance
and atomic CAS publication owners remain authoritative. Physical C keys apply
only to the admitted root-target hierarchy with keyed levels; D, one P and O
retain their existing applicability and output/bounds rules.

Source review verified IMS 15.6 explicit scopes ims-programming-contracts and
ims-database-contracts using offline ibm_docs.py search/read, committed hashes
and catalog rows. The retained topic-path root lacked all sixteen reviewed
bodies; each exact SHA-archive body matched its committed byte count/hash and
was read with the repository plain_text parser. No required body was missing or
mismatched. Full identities and zero-credit source review are external in
ims-secondary-ssa-navigation/sources.json and source-review.log. Primary pins:

| Baseline / topic | SHA-256 |
|---|---|
| ibm-ims-15.6-database-contracts-2026-09-11 / APG ims_ssassecondaryindex.htm | 4b3a1ee3cc0eabbbb4984d23a0eb60fe93be50887e1853643e0f382710139900 |
| Same database baseline / DAG ims_howhierrstruc_fullfunction.htm | 8535859c6dfc8e9683b34d307535bc524321cdc445bb6b94c1143d8d0206d1ca |
| Same database baseline / DAG ims_howseindexmaint.htm | 910d3494d8be6d834eb24844d86153ce675f67566fd49a455056d8da230cb086 |
| Same database baseline / DAG ims_replacecall.htm | 55778b03e47f21e92fb967995ec54b7ff1903c7886b98bdb1394d6d85e8fd23a |
| Same database baseline / DAG ims_issuedeletecall.htm | f40ecf698e4817f47ca8a4abd5214baa880476393c4f765a5c215051acdc6a23 |
| ibm-ims-15.6-programming-contracts-2026-09-11 / APG ims_ssacodingrules.htm | cfb772b7ae68ea657006d135792441a4da20185c2c8833389171c76432c0fba5 |
| Same programming baseline / APR ims_ccmdcode.htm | 038fcdaa210493f4c423ae2b0bc7fbbbf8bd381146a8ad25cf7615d28174d65a |
| Same programming baseline / APR ims_gnpghnpcall.htm | 6daaf5929bf3640a6d4ab17ea97d81328e60b26eb38c32b2c991c77d41592762 |

Additional programming topics read at their committed pins are ims_ssas.htm,
ims_ssacodingformats.htm, ims_ssas_cmdcodes.htm, ims_cmdcodref.htm,
ims_gughucall.htm, ims_gnghncall.htm, ims_currentpos.htm and
ims_processingoptions.htm. Full paths use SSEPH2_15.6.0/com.ibm.ims156.doc.apg,
.apr or .dag as declared in the manifests; their exact hashes remain in the
external source identity list. The root-target restructuring source preserves
the physical hierarchy, while nonroot inversion and duplicate-reference aliases
need metadata this base cannot represent. Those shapes remain rejected.

### Compatibility and parent obligations

No host ABI/canonical variant, SQL migration, namespace, retained row codec,
descriptor or position shape changes. Existing rows and replay digests are not
rewritten. Stop new admission and drain/reconcile indexed SSA effects, sessions,
holds/reservations and witnessed UOWs before binary downgrade; the previous
binary rejects this selected rich route before replay. Preserve a coherent
pre-upgrade backup containing selected/retained metadata and package artifacts,
database images, sessions, undo, checkpoints/recovery, journals, replay and
audits. Do not strip pointer or selector fields to simulate rollback. Inherited
extended-index/per-PCB/v2-undo writer compatibility limits still apply; this
leaf is not coherent backup/restore certification or a retention-expiry pass.

The selected-secondary symbolic CHKP/XRST guard is unchanged and remains a
required unsupported recovery leaf after GSAM discriminant integration. No
primary-key substitute is published. Manager-owned integrity-read visibility
must be integrated around both operand forms in execute_operands_at before
selection and atomic publication, retaining replay ordering and read fences.
This leaf changes neither that common seam nor integrity algorithms. Application
backout, GSAM restart, participant admission/fencing and official IR candidates
remain with their owners. Unsupported A/F/G/L/M/N/Q/R/S/U/V/W/Z/subsets, mixed
Boolean precedence, multiple P, nonroot aliases, NULLVAL/exits/SUBSEQ, virtual or
unkeyed concatenated keys, raw language/EBCDIC framing and unsupported contexts
remain explicit pending source-applicable classes. Deterministic duplicate-source
tie order is a local rule, not licensed IBM tie-order equivalence. No parent,
official row, maintainer approval, differential or release completion is claimed.

### Local verification and packaging

Fail-first provider and signed-package receipts reproduce Unsupported on the
public route. The former root boundary test now proves B-before-A indexed order
and exact selected pointer state while preserving the primary PCB. Local runs
pass 36 affected PCB/secondary tests, 11 inherited primary SSA tests, 15 engine
tests and the one selected-secondary symbolic-checkpoint guard. The final
composite helper runs twice on Memory/file SQLite, including fresh-reader
continuation with a distinct source occurrence. Four signed secondary cases
cover opposite order, composite binary XDFLD, distinct source/target and SQLite
reopen; one inherited signed primary SSA case preserves selection fences.
Three separately executed SQLite child processes prove pointer/replay retention;
the helper's ordinary no-environment invocation earns no process-restart credit.
Filtered binaries with zero selected tests earn no evidence. Final helper/test
additions were checked separately without relabeling earlier runtime receipts.

Strict IMS/server all-target Clippy --no-deps -D warnings, format, offline deny,
license notices, supply-chain policy, schemas, IMS catalog/assurance, shared spec
integrity and changelog pass. Execution, canonical effect, provider-row (after
the documented locator repair), storage, enterprise SAF, retention and pending
participant guards pass. The global module ratchet passes without a ceiling
increase; no frozen facade grows. Initial compiler/fixture/locator failures and
their repaired results remain separate external receipts. No source refresh,
PostgreSQL campaign, CardDemo-full, licensed differential, whole-cache audit or
release certification ran. Such required parent obligations are not waived.

Routine docs generation/check and the exact 17-path leaf seal/check are the final
packaging sequence recorded in external handoff.md. All Cargo sequences end
with cargo clean for this checkout; receipts remain outside disposable targets
and Git. The local completion commit seals only IMS-1401.secondary-ssa-navigation
at target 0.14.0, with zero official row/verdict or licensed credit. The next
manager action is to integrate this leaf through the preserved common pipeline,
including integrity-read fences, while retaining the secondary recovery guard.

Manager integration consumes GSAM/STAT/integrity at sealed `a0165979` and keeps
the common `service/execution.rs` fresh-read and exact-replay ordering. Both
primary and selected secondary operands pass through that one publication
pipeline; the selected-secondary checkpoint guard is not weakened. The older
nested Session CAS shim remains a re-export of the shared test-only owner,
which now gains the worker's lost-acknowledgement injection rather than a second
race algorithm. The provider-row locator repair already present in the manager
is unchanged and retains its removal/whole-state mutant checks.

Integration found a valid child physical field sharing the XDFLD identity.
The worker evaluator incorrectly used the pointer bytes for that child as well
as the target; a public GU returned GE instead of literal `C2AZ`. The retained
`secondary-ssa-integration-red.log` reproduces that failure. Parsing and matching
now share the same selected-target field resolver, leaving a child's physical
field scoped to its own data. The added Memory/SQLite public regression retains
exact replay and verifies ordinary GU parentage, explicit root P parentage and
negative GE. An initial private-ID/SQLite-constructor fixture compile failure
and a later incorrect default-parentage assertion are separately retained in
`secondary-ssa-integration-compile-failure.log` and
`secondary-ssa-integration.log`; neither is credited passing. Final integrated
checks are recorded separately in `secondary-ssa-integration-fixed.log` under
the manager continuation receipt root outside Git and targets.

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
## IMS-1401.stat-observable-contract (bounded slice, 2026-10-02)

Parent: IMS-1401. Entry HEAD: `f2cb3009b2d3929e642b9bab83d59ffab5329b55`;
branch: `codex/v014-stat-observable-contract-20261002`. Preserve the sealed
secondary feature and the accepted host/effect/provider-row/storage/SAF/retention
and pending participant boundaries above. This is provider-owned observable
behavior, with no new dependency, store, coordinator, execution credit or
maintainer source acceptance. Catalog: IMS 15.6 baseline
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0022`.

Inspection found name-ordered pools, ignored formats/extensions, synthetic zero
counters, a database/family cursor rather than selected-PCB iteration, no GA
totals or intervening-PCB reset, and scheduled-PCB-only system validation.
The bounded supported class is a versioned **typed host projection** of basic
DBAS/VBAS F/S/U statistics using explicitly published existing read/write
counters. It does not provide IBM print records, raw fullwords, AIB operands or
all IBM counters. Enhanced DBES/VBES, E1, and unproven DBASO must reject explicitly;
their missing buffer-handler/hiperspace/CF counters cannot be manufactured.
Historical request/result canonical bytes and retained rows stay readable;
an additive call/result distinguishes new output and I/O-capacity operands.

Offline `ibm_docs.py search/read` was run before edits: STAT is unregistered,
while call-functions is readable in the existing verified 1401 cache. The
recorded direct `com.ibm.ims156.doc.apr/ims_statcall.htm` pin is exactly
`cc777a81e45ccaedc704c8319a9f9b6a6eb7b30521b00b110cd371491eb5fb51`,
16,635 bytes, verified against the retained SHA archive and its topic metadata.
The repository `plain_text` parser was used, with no refresh. A minimal locator
repair may add only that existing exact pin to the programming-contract reader
manifest/registry; it grants zero credit. Related retained archive identities:
`ims_statcalldbstatistics.htm` / `04a50c5a16d20ff421a787d3acf995f7dac6c3e8fe930a421966aa77fcc31109`
(5,599 bytes), `ims_osambufferpoolstats.htm` /
`d52ef6a0d812e17234b4299b89eafa78585aa4a6f1870d16a0cd4e411fc4b2d5`
(6,521), `ims_vsambuffersubpoolstats.htm` /
`b8222f22ccaaf9f4ed630bdcd6bf2bf8ed03ba9bb3441f2bfb66c916114a5706`
(9,029), and `ims_osambuffersubpoolformat.htm` /
`9a663f2a9d4d714314c764786c44001d206e6b07d032df38704af756e600a2fd`
(19,814). These supplemental bodies are hash/length verified reference review,
not registered execution evidence. The specific summary pages require 180 bytes
for basic S where the call page says 120; enhanced U pages exceed the call
page's generic 72. Use the stricter proven basic capacity; leave enhanced/raw
layout discrepancies pending rather than assert parity.

Mandatory local obligations: DBAS aggregate repeatability; VBAS LSR definition
order, data-before-index and ascending buffer size, GA aggregate and GE absent
pools; independent PCB cursor, reset on intervening PCB use, exact replay without
advancement; F/S/U capacity boundaries; every invalid combination, unsupported
form/context/PCB and unavailable runtime/counter rejection without mutation;
SAF before observation, canonical conflicts, atomic row/CAS/failure and explicit
post-publication unknown outcomes. Backends: Memory and fresh file-backed SQLite
reopen. LSR grouping/type metadata is additive and required for VSAM ordering;
historical runtime definitions without it remain readable but cannot prove that
order. No unverified DSR ordering rule is inferred from the LSR-only direct page.

Owned files: bounded `service/system/stat.rs` and STAT test modules; minimal
`ims_system.rs` DTO/canonical changes, runtime publication/validation and STAT
call sites in `service/system.rs`; focused existing STAT fixture corrections;
the two source locator files, this status, one fragment and generated docs
manifest. Do not change Q/integrity algorithms, GSAM, CHKP/XRST or manager facade
and row-helper infrastructure; use original helpers. Shared integration is
limited to STAT-selected PCB admission and reset hooks.

Exact checks: fail-first `cargo test --locked -p mainframe-env-ims stat_`;
passing focused STAT public-provider tests and existing `generic::tests::`;
host `ims_stat`/canonical compatibility tests; strict affected package
`cargo clippy --locked -p mainframe-env-ims --all-targets --no-deps -- -D warnings`;
`cargo fmt --all -- --check`, `cargo deny check`, `cargo xtask ims-catalog --check`,
`cargo xtask ims-assurance-matrix --check`, `cargo xtask schemas --check`,
`cargo xtask spec --check`, focused execution/effect/provider-row/durable/SAF/
retention/participant/module guards, `cargo xtask docs --check` and
`cargo xtask changelog --check`. Diagnose unchanged aggregate blockers once;
no whole-cache/global certification run. Clean this checkout's Cargo target
after each sequence; external receipts live under
`v014-completion-20261002/IMS-1401.stat-observable-contract`. Seal only this ID
with exact changed-path allowlist and target `0.14.0`, then `--check`.
Official and licensed credit remains **0/25**, with no parent/release claim.

Bounded implementation outcome: `StatisticsV2` supplies a distinct canonical
request with capacity and a format-tagged partial host observation. DBAS returns
repeatable OSAM totals. VBAS returns explicitly described LSR subpools in pool
definition order, then data before index, ascending buffer size; the subsequent
GA returns totals. A selected full-function DB PCB owns its cursor independently
of the scheduled PCB. Intervening database use resets that PCB; exact retained
replay does not advance it. GA remains the aggregate until an intervening use;
automatic post-GA restart is not claimed. Empty configured pools return GE;
missing runtime, ordering proof or published counters fail explicitly. Defined
zero counters become observable only after authoritative publication. Totals
overflow, authorization denial, conflict and failed atomic publication preserve
the prior durable cursor and receipt. Post-publication clock failure returns
UnknownOutcome and retries replay the committed receipt.

The direct pin is now registered without replacing its identity. Related
registered comparison, call-function, PCB and status topics were read through
the pinned reader. Additional exact retained reference pins read with its
`plain_text` parser are `ims_vsambuffersubpoolformat.htm` /
`ef46730a81ac5e063d703a9293a7b4b474d6da66aaed710145be7ad5d841db74`
(12,292 bytes) and `ims_statcmd.htm` /
`2dd3f64795b6b9ad6f78d1b3113d6ab87d3a2d38bde53420eb870ca40a9646cd`
(20,833 bytes). These supplemental pins remain unregistered, zero-credit
reference review. The command page permits USING PCB / INTO / LENGTH and the
basic VSAM/NONVSAM formatted/unformatted/summary selection; it does not justify
rejecting all typed Command syntax. Both typed syntax classes retain existing
applicability/context authority. The programming manifest has 27 topics,
591,162 bytes, set digest
`119dd5e589399cb023f70c7a28fa9a1a937be1fa2c2393679ab65402ac735182`.
Only generated SSA/PCB manifest digest constants change with this locator
repair; no applicability, status or catalog row is granted new credit.

Compatibility: independently frozen old Statistics call/result canonical
vectors and historical runtime JSON remain exact. Old replay receipts retain
their original values, including previously overbroad results; reading them is
not recertification of those semantics. Fresh legacy calls support only the
representable basic Full single-OSAM result (or GE for empty configured pools);
formats, aggregates and enhanced classes requiring a new result reject
Unsupported. New requests/results use additive V2 variants. Old retained runtime
and sessions deserialize with absent V2 metadata/proof/cursor defaults and are
not rewritten on open; historical counters without publication proof remain
unavailable to fresh calls. Rust runtime struct literals require the new default
field. Older binaries cannot interpret V2 replies and may discard additive
state fields on write. Drain older writers before admission and preserve a
coherent pre-feature provider/replay/session/checkpoint/UOW/metadata backup;
rollback restores that backup rather than stripping fields or replay variants.

Local verification receipts are external under the declared directory.
Fail-first public-route cases exposed three actual entry-HEAD gaps: repeated
DBAS incorrectly exhausted, unmeasured pools returned invented zero counters,
and E1 returned an unsupported basic result. Passing checks: focused public
Memory and fresh SQLite reopen STAT cases; the existing `generic::tests::`
suite (56 tests); host `ims_stat` capacity/extension/JSON and canonical vectors;
final STAT tests after the reset-hook extraction; strict IMS `--all-targets
--no-deps` Clippy with `-D warnings`; fmt; offline locked deny; IMS catalog
generation/check; IMS assurance matrix; schemas; spec; changelog; license
notices; supply-chain check; assurance inventory; source-reader unit tests;
execution/effect/provider-row/storage/SAF/retention/participant guards. Filtered
zero-test binaries receive no credit. Cargo targets were cleaned after each
sequence. Docs are regenerated and checked for this final status.

Unchanged manager-owned blockers diagnosed once: strict host-api Clippy fails
at `mq_validation.rs:438/443` (`collapsible_if`, `manual_contains`); the global
module guard fails at server `product.rs` (6,291 versus its 6,008 ceiling).
No lint suppression, algorithm rewrite or budget refresh is included. Owned
production modules remain bounded: STAT helper 295 lines, host ims_system 412,
provider system 987. Minimal shared edits are V2 DTO/exports/canonical dispatch,
runtime proof/validation, selected-PCB STAT admission and reset hooks; Q,
integrity, GSAM, CHKP/XRST and manager row-helper algorithms are unchanged.

Remaining STAT work: enhanced DBES/VBES and E1 require authoritative missing
buffer-handler/error/fix/hiperspace/CF counters and complete format sources;
DBASO and the basic S 120-versus-180-byte discrepancy remain unproven classes.
No raw IBM binary/EBCDIC layout, AIB/raw operand/status parity, complete printed
output, DSR ordering, licensed comparison or official IR case credit is claimed.
Fresh SQLite connection reopen is not process restart or PostgreSQL acceptance;
coherent rollback, shared participant acceptance and full IMS/release gates remain
separate. Seal only `IMS-1401.stat-observable-contract` for `0.14.0` with the exact
changed-path allowlist and verify with `--check`; parent IMS-1401 remains open.

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
## IMS-1406.integrity-read-visibility (implemented bounded leaf, 2026-10-02)

Parent: IMS-1406. Clean entry and preserved Q seal:
`ab085b2c720d3b4964ea31a0e6bddb5e27d30581`; branch
`codex/v014-integrity-read-visibility-20261002`. Consumes the accepted
0.4/0.5/0.6 identities above and the shared host/SAF/effect/provider-row/CAS,
storage, UOW, retention and pending participant boundaries. IBM-observable
visibility remains provider-owned; no private locks, store or coordinator.

Catalog context: `ibm-ims-15.6-dli-2026-08-31:dli-call-families:0005/:0006`
(GU/GN/GNP and hold forms); settlement compatibility uses existing
`:0002/:0017` context without altering recovery algorithms. No official row,
participant, shared IR, licensed or parent credit is claimed.

Source baseline `ibm-ims-15.6-programming-contracts-2026-09-11`:
`ims_processingoptions.htm` (`bb549e17230c1990ac0b5b5f0386512493bcd5552b3b4762bd7fc4436a923dd4`),
`ims_gughucall.htm` (`0a9b433d9e38e58c125232a94147acd309e5bbbd7baf3c8cb900a3e8fdf6c8b9`),
`ims_gnghncall.htm` (`063ff108614ee13694ea7df2f7b39647447059590162614da56de2aa2eb49cb4`),
`ims_gnpghnpcall.htm` (`6daaf5929bf3640a6d4ab17ea97d81328e60b26eb38c32b2c991c77d41592762`),
and `ims_comparingcmdcodesandopts.htm`
(`eec570be49de991fe17b672c5e81dc4a83733d66267d3e0b561502e8820854ec`).
All five retained path files are absent; their raw SHA archive bodies and
committed TOC hash were verified, then used by the offline shared search/read
parser. No selected registered topic is missing from both roots. The scoped
reader's other missing-topic count is not a whole-cache audit finding.

Existing archive supplements, separately verified against archive metadata,
remain unregistered: `ims_reservingsegments.htm`
(`4d3967707752564dcc2193e8003341bfdd63a2a2d5358ba8d5f9db210830fbbe`),
`ims_readwithoutntegrity.htm`
(`2c47c2c4d31978fe88fba7011b47bfadd043052b0413691264256471e587ff5b`),
`ims_psbgendlipcbstmt.htm`
(`0dad54edd1a9940ca9a6988e06412836ff35a706cc2e1f7fbd36eb14fa02cfba`),
`ims_psbgenfastpathproc.htm`
(`cf00db795cf0d591ac6217ee2116d62a1b07e5f4eb8a14e10891553989b9324c`).
Exact shared-reader requests for these unregistered topics are unavailable;
they are supplements, not silently registered baseline replacements. No IBM
bodies enter Git or execution evidence; no network refresh is authorized.

Applicability: normal G/R/D/A reads must not expose foreign uncommitted
images; I implies G for DEDB only (existing capability validation remains
separately owned). PCB O is read without integrity, restricted to the trusted
GO/GOP/GON/GONP/GOT/GOTP forms. It cannot authorize segment updates or be
inferred from a SENSEG O or caller flag. Full-function and DEDB have source
support for O; MSDB and GSAM do not receive that exemption. N/T pointer-error,
physical CI buffering/retry and exclusive E scheduling are not implemented by
this image fence. The current legacy request carries selected PCB but no
validated IMS execution-context/syncpoint owner: Interactive/Batch are local
test contexts, not proof of the source DB/DC, DBCTL, DB batch matrix. Context
validation and new SSA/GSAM DTOs stay with the manager/other workers.

Owned edits: new bounded `service/generic/integrity.rs` and
`service/generic/tests/integrity_tests.rs`, minimal module declarations and
common `service.rs` execute/persistence integration, a unique changelog
fragment, this status section and routine generated documentation manifest.
Affected compatibility fixtures in `tests/closure_tests.rs`,
`tests/isolation_tests.rs` and the utility-bridge test module also need
source-consistent expectations/ordering: obtain holds from committed data,
reject foreign pending insert/replace/delete observations and stage utilities
after the fresh read CAS. Their product algorithms remain unchanged.
Fence exact replay before fresh navigation; use pending undo plus unchanged
database/dependency CAS rows in the existing atomic publication. Owner reads
remain valid; foreign normal reads fail with host contention and no status,
position, hold, Q or replay publication. Preserve Q algorithms and all
STAT/index/recovery/participant ownership. Manager resolves facade movement.

Required proof: deterministic public typed-provider fail-first on Memory and
file SQLite; all six read/hold operations, selected PCBs and trusted O forms,
forbidden O metadata/update cases, owner reads, commit/backout release,
fresh/stale services, dependency scope, replay before later UOWs, no-op/new
writer CAS races, atomic failure/lost acknowledgement, corruption/legacy undo,
SQLite processes/reopen and inherited lease/retention compatibility. Run
focused regressions, strict scoped `--no-deps` Clippy with `-D warnings`,
fmt/deny/catalog/assurance/spec/shared guards/docs/changelog and exact leaf
allowlist seal/check; retain receipts outside targets and clean each sequence.

Remaining limits: database/dependency contention is conservative and does not
prove IBM record/block lock granularity, shared-Q scheduling, ordinary Get
Hold read locks, waits/deadlocks or complete isolation. Shared lease/live
controls/audit/retention/coordinator and mixed-resource acceptance stay pending.
No push/PR, licensed execution, network or unrelated campaigns.

Implementation: `generic/integrity.rs` derives the read fence from the existing
`ImsRequest.operation` and selected PCB. Common `execute_at` refreshes session
authority for authorization, resolves exact canonical read replay, then refreshes
database/undo and performs the integrity check before fresh navigation. There is
one algorithm for legacy requests and manager-composed SSA/GSAM routes; no absent
DTOs or private SSA/GSAM implementation. The existing row-diff/CAS publication
adds unchanged observed database/dependency rows alongside session, status/Q and
replay changes. New writers and no-op ownership transitions cannot publish past
the inspected version. Ordinary G/R/D/A reads reject foreign pending images with
host `IdempotencyConflict`; owner reads remain valid with verified undo. Host
contention does not manufacture an IBM PCB status or change position/hold/Q.

Trusted O metadata permits foreign pending data only for the declared database
classes. O on SENSEG alone grants no exemption; O PCBs with update SENSEGs and
O on MSDB/GSAM fail closed with `Unsupported` before observation. Ordinary
mutation permission checks prohibit updating through any O PCB even with a
SENSEG override. Legacy definition option strings lack the validated generic
PROCOPT/organization authority and receive a conservative integrity fence.
Logical-parent RULES are absent from this bounded metadata contract, so O on a
child cannot exempt a different related database's foreign pending undo.
Malformed undo fails closed; retained plain-map/unmatched post-image witnesses
stay `UnknownOutcome` where ownership safety cannot be proved. O does not grant
permission to consume corrupt state. Original replay returns recorded bytes
before consulting a later unrelated UOW, including after a lost publication
acknowledgement, without redispatch or restoring an obsolete position/hold.

The deterministic fail-first receipt contains both public Memory and file-SQLite
G reads returning foreign pending `C1Z`. Final focused verification executes
15 integrity tests through `ims_providers`, including all six read/hold forms,
selected G/update/O PCBs, G/R/D/A and full-function/DEDB GO/N/T classes,
forbidden O cases, owner reads, commit/rollback release, stale/fresh services,
unrelated/logical dependency scope, canonical replay/conflict, no-op/new-writer
CAS races, atomic failure, lost acknowledgement, authorization, retained undo,
corruption, legacy route and three separate SQLite seed/observe/verify processes.
Existing Q/PCB/checkpoint/recovery/retention/participant regressions passed in
the affected package run: 139 unit and 17 integration tests, before the final
test-only PROCOPT matrix addition; final focused 15 and strict Clippy include
that addition. These are local executable checks, not official verdict events.

Exact commands: `cargo test -p mainframe-env-ims integrity_fail_first --
--nocapture` (expected failure), `cargo test -p mainframe-env-ims`,
`cargo test -p mainframe-env-ims integrity_`,
`cargo clippy -p mainframe-env-ims --all-targets --no-deps -- -D warnings`,
`cargo fmt --all -- --check`, `cargo deny --offline check`,
`cargo xtask ims-catalog --check`, `cargo xtask ims-assurance-matrix --check`,
`cargo xtask spec --check` (134 policy tests), `cargo xtask changelog --check`.
Shared Python guards pass: `tools/check_execution_route.py`,
`check_effect_encoding.py`, `check_provider_rows.py`, `check_storage_profile.py`,
`check_enterprise_authorization.py`, `check_retention_lifecycle.py`,
`check_transaction_participant.py`, plus
`python3 -B tools/generate_transaction_participant.py --check`. Every Python
command uses `python3 -B`; Cargo uses `PATH=/Users/tore/.local/bin:$PATH` and
`CARGO_NET_OFFLINE=true`. Tools resolve to Python 3.12.13 / Cargo/Rust 1.98.0.
No unchanged global architecture/cache/module failure is rerun or relabeled.
The scoped helper has 218 production lines, generic 943, facade 2,123; the
inherited facade ceiling and shared module/lint repairs remain manager-owned.

Compatibility: no durable/public schema, namespace, request/result canonical
domain, undo witness or replay-age/retention lifetime changes. Fresh reads now
advance the observed image CAS even for unchanged bytes; utility stages bound
to an older version require restaging. No second committed image is retained.
Historical successful replay, including a read previously admitted incorrectly,
is not rewritten. Stop admission and drain/reconcile sessions, Q reservations
and UOWs before upgrade or binary rollback; retain original checkpoint/replay
references or restore a consistent compatible backup. Mixed old/new writers
remain unsupported, and v2-undo downgrade limits still apply. This leaf adds no
coordinator recovery-lease epoch, current-time/cancellation check, audit
composition, new retention authority or accepted participant capabilities.

Receipts: external
`/Users/tore/Library/Caches/mainframe-env/worker-receipts/v014-completion-20261002/IMS-1406.integrity-read-visibility`.
Routine final documentation generation/check uses `cargo xtask docs` and
`cargo xtask docs --check`. The leaf seal uses the exact staged changed-path
allowlist with `cargo xtask work-package-seal --id
IMS-1406.integrity-read-visibility --target-version 0.14.0`, then the same
allowlist with `--check` after local commit. Cargo targets are cleaned after
each sequence; no publication bodies enter Git. Manager must retain the common
pipeline fence and existing row-helper names during mechanical module movement,
integrate other worker changes and re-seal changed blobs. The next substantive
work is source-backed record/read-lock/shared-Q scheduling and pending shared
participant/context/IR acceptance, not repeating this leaf's successful checks.
Official/IR credit remains zero and licensed differentials remain 0/25 pending;
parent IMS-1406 and the 0.14 exit gate remain incomplete.

### Integrity-read manager integration

All fresh legacy and rich SSA reads share the common fence after exact replay
resolution. SSA preparation follows the refreshed authoritative image rather
than examining an old image before replay. Preserve current Q, selected-PCB,
secondary, recovery markers and the extracted row/validation helpers. Remove
only the redundant canonical digest wrapper and lower the exact facade budget.
An additional public regression covers rich SSA replay during later foreign
pending work, fresh normal-read rejection, and trusted GO visibility on both
Memory and file SQLite.

XRST's internally qualified GU is also a read. A fail-first selected recovery
case reproduced fresh XRST accepting another run's uncommitted image. Apply
the same trusted-PCB visibility preparation before repositioning; the existing
recovery bridge atomically CAS-fences every selected database with the recovery
and session publication. Do not invent a second recovery/read-lock authority.
`integrity-restart-red.log` retains the failure; combined verification and the
exact re-seal remain separate from the worker's historical receipts.
The first combined unit run passed 165 cases and found one new fixture supplying
both raw SSAs and legacy segment operands; validation correctly rejected it.
That failure stays in `integrity-integration-initial-failure.log`. After fixing
only that fixture, the focused rich SSA case, all 24 application recovery cases
and all 13 signed IMS package cases pass in `integrity-integration.log`, with
strict IMS/server Clippy and the module/assurance guards. Do not relabel the
earlier whole-unit run as a later whole-suite receipt. Licensed and official
row counts are unchanged.

### STAT manager integration

Keep the current Q observer, actual per-PCB refresh, integrity-read publication
and checkpoint markers when folding the basic typed STAT projection. The direct
STAT call pin `cc777a81...` now resolves through the shared offline reader; the
additional exact topic locator expands the zero-credit programming source set
without repinning any existing topic body or granting official approval. Format
detail pages remain verified archive supplements. Their minimum-summary-area
discrepancy with the call page is retained; raw layout parity remains pending.

The first combined STAT run passed 13 cases and found the inherited expectation
that a stale adapter must fail instead of refreshing. Preserve the correct fresh
subpool result, then use the shared test-only session shim to force a real CAS
race after refresh and before atomic publication. Assert the exact unchanged
cursor/system/replay payloads, with only the shim's independently advanced
session version permitted. Capacity rejection/retry remains asserted. Reuse
that same shim for the existing selected-secondary backend regressions rather
than duplicate a race algorithm. The old test-only module path re-exports it.
Initial and passing combined receipts are `stat-integration-initial-failure.log`
and `stat-integration.log`; original worker receipts are not relabeled.
The repaired candidate passes 14 STAT, two existing system-family, 23 selected
PCB/secondary and 24 application-recovery cases, two host STAT contracts and
one canonical vector case. All 23 offline source-reader tests, strict IMS/host
Clippy, global module guard, IMS catalog/assurance, schemas and docs checks pass.
The exact manager re-seal is separate. Enhanced/extended counters, complete raw
layouts, official maintainer-accepted IR and licensed equivalence remain open.

The isolated GSAM restart branch starts from sealed GSAM
`90b87d38b99c55362e8f2b0d70489ab2143c8476` and integrates exactly sealed
`a1a84c7926851e61d0249ba576ba688794b073f9` as a separate prerequisite commit.
Its parent LOG adapter registration is absent from the GSAM base: restore only
the required host enum/validation/canonical/module exports, provider export,
server factory composition and test declaration. Keep GSAM requests, addresses,
canonical encoding, replay, PCB and witnessed UOW owners unchanged. Both lane
status sections are retained; regenerate the documentation manifest normally.
Conflicts comprise host/provider READMEs and exports, IMS service imports/module,
status append, generated manifest, and six modify/delete adapter/contract/test
files. The latter retain prerequisite bytes. This integration does not reseal
the prerequisite or claim its old receipts for the combined tree.

## IMS-1405.gsam-checkpoint-restart (declared bounded leaf)

Parent IMS-1405 remains in progress. Clean GSAM entry is
`90b87d38b99c55362e8f2b0d70489ab2143c8476`; prerequisite integration is
`cb7aa908ff48527dfa35ff634c7831cd6fde341b`, consuming exactly sealed
`a1a84c7926851e61d0249ba576ba688794b073f9`. Catalog scope is
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0002/:0023/:0016/:0025`,
with :0005/:0008 as GSAM GN/GU/ISRT consumers. Execution projection is installed
signed selected metadata, DB-batch CALL, fixed-length logical GSAM files and
G/GS input or L/LS output PCBs. Physical BSAM/VSAM formats/loading restrictions,
other contexts, raw RSA layouts and authentic DFS0540I timestamp authority remain
explicit unsupported boundaries. Logical ticks cannot supply region/day/time.

Obligation classes: real GN/ISRT address provenance; retained record/start/EOF
and output append boundary; CHKP commit/position behavior and basic rejection;
XRST real per-PCB resolve, input continuation, output suffix removal and empty
output validation; multiple PCBs/databases; row CAS and stale issuance guard;
bounded capacity; malformed/missing/unknown images; authorization before data;
replay after later work without reapplying; lost acknowledgments/unknown outcomes;
actual Memory/file SQLite atomic races and process exits; historical reader,
drain and backup boundaries. Expected bytes/statuses derive independently from
the pinned topics, not the product resolver. Supplemental tests grant no official
accepted-IR/maintainer/licensed credit. Licensed differential remains 0/25.

Owners: new service/application_recovery/gsam_checkpoint helper, bounded saved
GSAM DTO and engine resolver helpers, focused provider/host/signed-package tests,
minimal checkpoint hooks and the GSAM batch-UOW composition helper. SETS/SETU/
ROLS/ROLL/ROLB, ordinary read visibility and STAT algorithms remain untouched.
No new store, engine, coordinator, address registry or dependency. New GSAM
positions never occupy full-function segment_key. Manager's facade extractions
are absent here; feature edits use bounded modules and report shared seams for
manager integration. The standalone source supplement requires two metadata
files because the committed scopes omit the exact GSAM checkpoint topic.

Offline source basis: existing recovery/programming/database baselines
`ibm-ims-15.6-recovery-utilities-2026-09-11`,
`ibm-ims-15.6-programming-contracts-2026-09-11` and
`ibm-ims-15.6-database-contracts-2026-09-11`,
plus separate zero-credit `ibm-ims-15.6-gsam-recovery-2026-09-11`, topic
`SSEPH2_15.6.0/com.ibm.ims156.doc.apg/ims_gsamsymbolicchkpandxrst.htm`,
SHA-256 `ebc695770a487c4fd3b9b06fa0d6948353a1f6543c75e29ec45e94048066616c`.
The bounded retained/archive scope was hash-verified, searched and read with
ibm_docs.py before semantics. All twelve required topics and shared TOC verify;
retained topic-path copies are absent, matching raw archive bodies are used.
Unselected cache entries remain unavailable without whole-cache audit/refresh.
No publication body is committed and no existing baseline is repinned.

Required acceptance is focused fail-first/pass provider, host and signed-package
route with child processes; strict scoped Clippy --all-targets --no-deps with
warnings denied, fmt, execution/effect/provider-row/storage/participant/security/
retention/dependency/IMS catalog/schema/assurance/spec/docs/changelog gates;
exact allowlist leaf seal/check and one feature commit. Receipts stay outside
Git and targets. No delegation, certification, push/PR or source refresh.

### Implemented local GSAM recovery projection

SavedPcbPosition now retains an optional discriminated GSAM beginning, EOF,
live issued address or integrity-checked output prefix; the hierarchy key stays
empty for GSAM. Absent fields preserve historical serialized checkpoint bytes
and digests. The existing RecoverySession verifies the selected image and calls
the actual selected-PCB resolver. Symbolic CHKP materializes required live
identities before capture, commits actual writes, and releases position/UOW in
the existing atomic transition. XRST restores input position and removes only
a witnessed later output suffix after verifying the live prefix. Removed
addresses are never reconstructed. Existing issuance uses database identity and
current row CAS version; restart resolves a live retained address regardless of
settlement alone. A later committed suffix without an ownership witness returns
UnknownOutcome with no erasure. This is reconciliation, not a successful restart.

Focused current-candidate results: provider dispatch 34 passed (including 12
GSAM harness tests; the process-worker entry without environment earns no
scenario credit and the parent executes three real SQLite child phases), saved
position contract 2, recovery regression 18, prior GSAM regression 12, ordinary
basic checkpoint regression 4, host recovery contract 4, host canonical 18,
signed selected IMS GSAM package 2 Memory/SQLite roundtrips after the final
settlement-helper move. The earlier 16-test signed selected-package regression
passed before that move; its unchanged consumer evidence is retained separately,
not relabeled as a final-tree whole-package run.
The real canonical coordinator and public selected route prove ISRT/GN addresses,
checkpoint commit, later work, process reopen, saved areas and GU/GN continuation.
Negative checks include initial empty/output, EOF/restart-at-start, no-save GN,
multiple files/PCBs, stale replacement, malformed/order/selectors, SAF, quota,
actual threaded Memory/file SQLite CAS races, capacity and lost-ack observation,
missing-receipt ambiguity, and replay after later work. Initial failures are
retained separately and are not labeled passing receipts.

Strict host and IMS Clippy with all-targets/no-deps and warnings denied pass.
Strict server all-targets remains blocked by inherited COBOL/CICS/product and
test-helper diagnostics, including unchanged ims_package_tests.rs:250; no new
GSAM module or feature-modified line has a diagnostic. No lint allowance or
other-lane algorithm change was made. Exact diagnostics and command exits are
in the external worker receipt directory. Manager must retain this distinction
when folding the delta onto its extracted facades and resealing its candidate.

Shared feature seams are a settlement-helper call in service.rs, ordinary basic
CHKP GSAM rejection, saved-position capture/resolve and quota hooks in the
existing checkpoint bridge, and broader visibility for the existing image
publication helper. Isolation, backout and STAT algorithms are unchanged.
Manager integration maps these hooks into its service rows/providers, selected
product IMS facade and host request/canonical modules; those extractions do not
exist in this base. The separate prerequisite commit must not be folded as part
of the feature delta if already present in the manager candidate.

No SQL migration or new persistence/recovery authority. New saved-position rows
require a compatible reader. Before upgrade/downgrade, stop admission, drain
UOWs, reconcile unknown effects and preserve a coherent database/session/recovery/
selected-metadata/journal/audit backup. Existing Rust saved-position literals
must set gsam to None; historical JSON remains readable. Older writers cannot be admitted against
new GSAM checkpoint rows. Reopen tests do not certify backup restore or retention
expiry. Physical BSAM/VSAM loading/repositioning, temporary/SYSOUT datasets,
variable/undefined formats, raw RSA layouts, BMP/LAST, authentic root timestamp
day/region authority, raw framing, participant/lease admission, official IR and
licensed differential remain parent obligations. Logical ticks are not timestamp
authority; no source-required physical applicability is labeled fake success.
Only this bounded logical leaf can complete; IMS-1405 and v0.14 remain open.

Required execution/effect/provider-row/storage/participant/generated-participant/
security/retention/typed-boundary/supply-chain and offline dependency-policy gates
pass, as do IMS catalog, assurance, schemas, shared spec, changelog and docs
generate/check. The new zero-credit topic manifest passes shared schema/spec
integrity. Scoped production budgets pass; service.rs stays at its prerequisite
2,216 production lines and isolation.rs does not grow. Formatting/diff checks
pass. The aggregate inherited server Clippy debt is not waived or reported as
passing. Cargo targets are cleaned after each verification sequence; command,
source and cleanup receipts are outside Git at
`/Users/tore/Library/Caches/mainframe-env/worker-receipts/v014-completion-20261002/ims-gsam-checkpoint-restart`.
Manager acceptance/reseal, participant/lease closure, physical source-required
applicability, coherent backup/retention exercise and licensed evidence remain
open; no official row or parent completion is asserted by the leaf seal.
The local bounded GSAM projection is complete under
`IMS-1405.gsam-checkpoint-restart`; its exact allowlist seal/check and commit
identity are retained in the external handoff and completion receipt.

### Manager GSAM restart integration — 2026-10-02

The manager consumed exactly `2b7a92a1e57848fa3fbe158b71c38c6de71009af`
on its sealed GSAM and secondary-SSA candidate, without duplicating the worker's
prerequisite registration commit. Settlement is called from the existing
`service/execution.rs` owner. The image publisher retains the current limits
contract, ordinary read-visibility check and secondary-index restart rejection.
No second facade, engine, address registry or recovery authority was introduced.

The first integrated run exposed a real composition failure: after one GSAM
output PCB planned its witnessed suffix removal, a later PCB's integrity read
mistook that unpublished change for a foreign write. XRST now checks every read
against the pristine witnessed proposal, while the existing atomic bridge still
CAS-fences all selected database rows when publishing the whole transition.
The multiple-database/independent-PCB regression now passes. This does not waive
the foreign full-function dirty-read rejection or secondary-index guard.

Current integration checks passed 36 application-recovery dispatch tests and
two retained GSAM contract tests, then eight recovery-runtime unit tests,
12 GSAM unit tests and 20 signed IMS-package tests. Empty filtered binaries and
standalone process harnesses earn no scenario credit; substantive parent tests
assert real SQLite child execution. Strict IMS/server all-targets Clippy,
IMS catalog/assurance/schema, formatting and module-boundary checks pass on
this integrated candidate. The worker's inherited lint/module blockers above
are historical base results, not current manager failures.

External manager receipts are `gsam-restart-integration-fixed.log` and
`gsam-restart-remaining-gates.log` under the existing completion receipt root.
The former records a later incorrect test-target selection after its passing
36+2 checks; the remaining sequence used the correct unit filter. Initial
semantic/selection failures remain distinct from passing evidence. Existing
source baseline/topic and catalog identities above are unchanged, with zero
official or licensed credit. Exact resealing, dependency/docs/changelog gates
and cleanup accompany the manager feature commit. Participant/lease admission,
physical applicability, coherent backup/retention and human rule acceptance
remain open. Licensed certification is excluded by the user, not certified.
## IMS-1405.application-backout (declared bounded leaf, 2026-10-02)

Parent IMS-1405 remains in progress. Clean entry HEAD:
`a1a84c7926851e61d0249ba576ba688794b073f9`; branch
`codex/v014-application-backout-20261002`. Scope is additive selected signed
application DB-batch CALL SETS/SETU, intermediate and prior-commit ROLS,
ROLL and ROLB over actual generic database images and witnessed local undo.
Catalog: `ibm-ims-15.6-dli-2026-08-31:dli-call-families:0017/:0018/:0020/:0021`;
checkpoint ordering/commit context is :0002/:0023 and XRST :0016/:0025.
Logical I/O PCB operands are typed; raw LL/ZZ/AIB/language adapters remain
outside this leaf. Unsupported DEDB/MSDB/GSAM savepoint PSBs require exact SC
disposition, not a successful partial-backout claim. Other allowed source
execution contexts remain pending, not retrospectively inapplicable.

Obligation classes: exact operands, statuses, checkpoint/savepoint ordering;
real post-checkpoint database updates and witnessed undo; nested, replaced,
cancelled and bounded points; shared per-PCB positions and SystemState Q
authority; no mutation on malformed/denied/stale/conflicting requests;
canonical replay before later UOW observation; atomic publication faults,
lost acknowledgments, explicit unknown outcomes, CAS races and authoritative
observation; Memory and file SQLite reopen including separate child processes;
retained readers, capacity, retention and downgrade consequences. Independent
public-provider and signed selected-package/coordinator tests supplement the
shared accepted IR; they grant no official row credit or maintainer acceptance.
Licensed differential stays 0/25. No parent completion, push or PR is owned.

Owners: host ims_recovery and its canonical module; application_recovery and
new bounded application_backout owners; existing RecoverySession and atomic
bridge with only necessary runtime seams; focused host/provider/server tests,
one unique change fragment and this appended status/docs manifest. Frozen
facades must not grow; tiny integration seams are reported for the manager's
already extracted owners. No SSA/GSAM/secondary/STAT/Q algorithm edits.

The TM queue's real session/output rows share ProviderStateStore, but work
release/completion uses separate WorkStore operations and durable replay-work
repair. DB-only recovery must not claim TM suspension, express-message scope
or work-lease settlement. A live TM-backed run must reject this DB-batch leaf.
The TM/coordinator owner must stage actual queue/session/output/conversation
mutations under its existing lease and join the DB publication, then reconcile
the separate work disposition honestly. No cross-store atomicity is invented.

Verification: fail-first/pass public host/provider and selected signed routes;
strict scoped all-target Clippy --no-deps, fmt, affected effect/provider-row/
storage/participant/security/retention/schema guards, dependency policy,
IMS catalog/assurance and shared spec integrity, docs/changelog, exact-path
seal/check with this leaf ID and target 0.14.0, then one feature commit.
Receipts remain outside Git and disposable Cargo targets. Required failures
remain explicit; no lint waiver, lowered acceptance or licensed run is allowed.

### Implementation and local delivery evidence (not maintainer acceptance)

The bounded selected DB-batch route now uses actual database engine images,
the existing local UOW's baseline/postimage witnesses and RecoverySession's
point/replay authority. Generic Batch updates retain undo until an explicit
commit/checkpoint/backout boundary; the legacy definition route keeps its
prior behavior. A point reserves supported images using the existing UOW owner,
without a separate reservation map. Scheduling incarnation plus commit epoch
prevents an expired point from restoring a later UOW, including a reused run
identifier. Intermediate ROLS preserves the local UOW and Q reservations but
loses all real per-PCB positions/holds and Q current-position flags. ROLB restores
only current-interval undo and releases Q through the existing rollback observer.
ROLL and tokenless ROLS additionally persist U0778/U3303 terminal disposition;
the typed application machine consumes it as a coordinator Abend. New calls on
that Session reject, while retained recovery/database/LOG results replay first.

Four-byte tokens retain arbitrary bytes. Named points nest; replacing a token
captures the current image and cancels later points; tokenless SETS/SETU cancels
points without settling the UOW. Nine points are admitted; the tenth returns SB.
Absent, cancelled or prior-interval tokens return RA. SETS rejects unsupported
DEDB/MSDB/GSAM PSBs with SC; SETU's SC warning retains a point over supported
images only and never undoes the unsupported image. RC distinguishes the missing
point in that unsupported scope. Output-area size must exactly match saved data.
Malformed operands, unauthorized databases, unrelated pending work, stale image
witnesses, wrong selection, conflicting replay, cancellation, deadline and leased
or unknown core effects fail before publication. Documented conditions persist
only bounded receipts/CAS fences, not database or undo payload changes.

The existing atomic bridge publishes actual images, Session, undo, Q state and
recovery receipt with selected metadata/database CAS fences. Memory/file SQLite
tests cover real simultaneous publications (one backend CAS winner), failures,
lost acknowledgment and observation. Six separate SQLite child phases prove
durable point capture, image restoration, later-work-safe replay, prior-commit
backout, lost acknowledgment and unknown-outcome observation without redispatch.
Public provider and signed package/coordinator tests exercise positive and
negative paths, SAF, all-PCB holds, Q, mixed SETU, terminal outcomes, capacity,
replaced/cancelled points, commit epochs, foreign UOWs and unrelated work. Old
canonical LOG/CHKP/XRST and ordinary IMS vectors remain fixed; new backout vectors
are independently framed. Historical RecoverySession points remain readable
with their exact old digest, but lack application-epoch authority and cannot
be used to restore a real selected application image.

Pinned semantic baseline is `ibm-ims-15.6-recovery-utilities-2026-09-11`, selected
through `ims-1405-topic-cache`, scope `ims-recovery-utilities-contracts`:

| Topic (under `SSEPH2_15.6.0/`) | SHA-256 |
|---|---|
| `com.ibm.ims156.doc.apr/ims_setssetucall.htm` | `53b9a76d65aed978e2eca2d9c10a295bea6abf16e88cb5f6949e3effa5709336` |
| `com.ibm.ims156.doc.apr/ims_rolscall.htm` | `b7e15d0c110d3296eac11d895326b3ef48ac913fd682b94b312aa6c59ad14af5` |
| `com.ibm.ims156.doc.apr/ims_rollcall.htm` | `01a33e88387636985ef575bdd7bdb99e0d3ce6a794dcf227d2f1e75c843ec61f` |
| `com.ibm.ims156.doc.apr/ims_rolbcall.htm` | `166bc5f6ac4b4a75be331419fd9a185d76867a3be2aa322316a8e385bca2158b` |
| `com.ibm.ims156.doc.apg/ims_backingoutintermediate.htm` | `73e987b85ca10963e4bfc68e83c4612433689052b7baba385eaff735ae42f4f2` |

Related pinned scopes are `ims-programming-contracts` (I/O PCB, processing
options, C command code and system-service status table) and `ims-tm-contracts`
(ISRT/PURG/message-I/O boundaries), using their 2026-09-11 baselines and
ims-1403/1404 topic caches. CHKP/XRST ordering uses the pinned recovery topics
already identified in the preceding leaf. Catalog rows are precisely
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0017` (ROLL/ROLB), `:0018` (ROLS),
`:0020` (SETS), `:0021` (SETU); related `:0002/:0023/:0016/:0025` stay unchanged.
All selected registered bodies matched SHA-256/byte counts. Retained topic-path
HTML was absent; matching content-addressed raw HTML supplied local parser
review. Nine additional hash-verified archive bodies linked by those pinned
topics supplement prior-commit/Q/status review without repinning the catalog.
Full exact identities and offline receipts are in the external handoff directory;
no publication body is committed and availability earns no execution credit.

Compatibility: no SQL migration or canonical-reader replacement. New Session
markers and point epoch fields extend private JSON codecs; new readers preserve
old rows, while older binaries reject new fields or cannot enforce these UOW
boundaries. Downgrade requires drained/reconciled writers, effects and UOWs plus
a coherent verified pre-feature backup (images, sessions, recovery/checkpoints,
selected metadata, journals and audits), or a retained compatible reader. Never
strip fields or rewrite digests to manufacture a downgrade. Recovery-session
replay remains bounded and protected, outside the ordinary IMS replay retention
target; private-row expiry, graph attribution and coherent restore remain pending.
Unvalidated generic retention rejects; quota pressure cannot authorize deletion.

Explicit integration blockers remain: the whole-server warnings-denied Clippy
gate hits unchanged COBOL/CICS/product/test diagnostics; the aggregate module
guard hits unchanged product.rs (6300 versus 6008), and the inherited IMS facade
already exceeded its recorded inventory ceiling. This leaf shrinks service.rs
from 2111 to 2105 production lines and introduces bounded 557/179-line owners;
no exemptions are raised. Manager must fold the tiny service prepare/settle hooks,
module/import visibility seams and test registration into its already extracted
request/canonical/product/service owners, and repair inherited lint/budget debt.
The shared participant contract and licensed-pending ancestry are preserved;
IMS participant admission remains pending, with licensed credit 0/25.

Remaining source-applicable obligations are TM queue/session/output/conversation
backout joined with the DB proposal and existing work-lease repair (already
PURGed express output survives; unpurged express buffers do not), authentic TM
admission fencing, atomic core recovery-lease/publication fencing, other execution
contexts, raw CALL/LLZZ/PCB/CMPAT/JCL/region-log/BKO authorities, complete unsupported
database-family recovery, official IR acceptance and licensed differential.
The real TM guard proves DB-only rejection preserves queued output/buffers and
the WorkStore lease. It does not fence concurrent TM admission or claim full TM
recovery. Other GSAM, STAT, secondary and integrity-read lanes remain manager-owned.
The human's v0.14 parent and IMS-1405 remain in progress.

### Manager application-backout integration — 2026-10-02

The manager consumed exactly `bf0dc41c798ea344a195e5c2b3d47bfed7908277`
after its sealed GSAM restart integration. The prepare/settle hooks live in the
existing `service/execution.rs`; the old facade body was not restored. The
common application-backout settlement owner now covers generic Batch, including
GSAM, so the temporary XRST-specific GSAM settlement helper is removed.
Both image publishers retain current limits and Q reservation checks. Checkpoint
epoch/incarnation updates compose with the existing GSAM resolver, pristine
integrity read-source snapshot and secondary-index restart rejection.

Current integration verification passed 55 application-recovery dispatch tests,
207 IMS unit tests and 22 signed IMS-package tests, including real Memory/file
SQLite backout, GSAM restart, SSA/index, integrity, Q and process scenarios.
Standalone empty child harnesses and filtered zero-test binaries earn no
scenario credit. Strict IMS/server all-targets Clippy, formatting and exact
module-boundary checks pass; the older worker's inherited lint/budget failures
are not current manager failures. These checks are retained externally in
`backout-integration-initial.log` under the completion receipt root.

The participant preparation document now describes generic Batch explicit
settlement and the retained legacy definition-route policy. Pending/null
participant metadata and all blocked admission obligations remain unchanged.
This is a material client migration: explicitly commit/checkpoint/back out
generic Batch work instead of assuming each write commits. Current canonical,
host/recovery and participant compatibility checks, mandatory policy/generator/
dependency/docs/changelog gates and exact feature reseal accompany the commit.
Earlier source baseline/topic/catalog identities above remain unchanged and
were independently searched/read offline before manager semantic integration.

Actual TM recovery still requires an owned shared work/effect-lease publication
and settlement contract; a read-only TM-session absence check does not fence
concurrent admission. Raw contexts/framing, protected retention/coherent restore,
participant admission and human accepted-rule execution remain open. No
official, human-maintainer, parent or release credit is promoted. Licensed
certification remains excluded by the user's request, not treated as passing.

## IMS-1406.official-ir-candidate (preparation leaf, declared 2026-10-02)

Parent: IMS-1406; parent and release remain open. Base:
`21e51c8be1546ace252a50a3b10bc3021ac29042`. The clean sealed STAT HEAD
`14db41c20ddb053da85f8e8c19477cb621d2d01c` remains on its original branch.
This branch owns only shared candidate-reference preparation, a thin tooling
IMS driver/helper, independent fixtures and proposed rules in `conformance/spec`,
focused compiler/driver/mutant checks, bounded xtask registration, this append,
a unique fragment and regenerated documentation manifest. It does not own IMS
product semantics, Q/integrity, GSAM, STAT, checkpoint/backout algorithms,
other in-flight feature integration, dependency promotion or release counters.

Exact initial catalog scope: IMS 15.6 `ibm-ims-15.6-dli-2026-08-31`,
`dli-call-families:0004` (DLET), `:0005` (GU/GN/GNP), `:0006`
(GHU/GHN/GHNP), `:0015` (REPL). Applicability: call-level full-function
two-level physical HIDAM, DB PCB with AP/G processing options, DB-batch
navigation operands and interactive local UOW settlement. The public host
provider, metadata parser, SAF, row CAS/replay and Memory/SQLite authorities
remain product-owned. Tooling inspects bounded actual output and retained
position/hold/database rows; expected data is independently authored from pins.
Positive, boundary, condition, forbidden mutation, malformed/SAF, retry,
fresh-reader restart and local rollback preparation are mandatory. Fresh-reader
reopen is not process-death, XRST or full checkpoint/recovery credit.

Minimal cross-owner integration: shared coverage gets a candidate wrapper
around existing IR/compiler/runner references; it cannot expose official
verdicts/ledger or convert a candidate into accepted rules. Xtask checks the
candidate during spec validation and explicitly reports maintainer-pending IMS
preparation at its existing focused selector. Frozen facades must shrink or
remain within their existing ceilings; no new evidence/ledger/runner authority,
general DSL or product dispatch by row/obligation identity is permitted.

All publication rules remain **pending HUMAN conformance maintainer acceptance**.
No approval identity is supplied by this agent and no accepted-rule registry is
edited. Official IMS rows and all six gate numerators remain **0/25**; the local
25-row assurance map remains zero-credit. Licensed execution, network/browser
refresh, delegation, push and PR are user-excluded. Exact proposed bindings,
source anchors, expected observations, missing classes and reviewer action will
be retained in the bounded shared candidate artifact. Acceptance here completes
preparation only, never official-row, parent or release acceptance.

The preparation artifacts are `conformance/spec/candidates/ims-db.json` (22
proposed rules, 42 ordinary IR bindings, four catalog rows) and
`conformance/spec/fixtures/ims-db.json` (40 independent Memory/SQLite fixtures).
The GN boundary uses an explicitly declared roots-only seed with unqualified
calls: A1, B2, GB, A1. This excludes unresolved GA/GK cross-type/level proposals.
The other recipes retain the two-level physical hierarchy. Shared coverage
gate identities and immutable catalog loading were extracted unchanged into
small modules; the coverage facade ceiling was ratcheted from 571 to 533
production lines. No ceiling was raised and no product dispatch uses row IDs.

Offline review verified 13 selected retained HTML identities against committed
manifests and read them with `ibm_docs.py` in the explicit programming,
database and recovery scopes. Rule anchors retain the exact baseline, topic,
manifest and SHA-256. GU/GHU `0a9b433d…`, GN/GHN `063ff108…`, GNP/GHNP
`6daaf592…`, REPL `55778b03…`, DLET `f40ecf69…`, SSA coding `cfb772b7…`
and the database status table `2b41e141…` are central anchors. The original
catalog comparison topic is `ce4a179e…`. Full hashes and all proposed obligations
are in the candidate artifact and the external handoff. No publication body,
network refresh or licensed execution evidence was added to Git.

Focused checks passed 19 shared coverage tests, five IMS candidate compiler/
runner tests and three tooling driver/fixture tests. Preparation executes 42
bindings with zero mismatches and rejects seven observation perturbations;
the driver test rejects eight representative mutants through the shared
runner. Exact replay executes one nonzero binding. Ordinary official IMS
selection fails with the HUMAN acceptance blocker. Strict coverage/tooling
Clippy, formatting, spec, IMS catalog, IMS assurance, schema, dependency/license,
execution/effect/security/storage/retention/transaction guards and the module
ratchet are checked independently of source review. These are local preparation
results and grant no official or licensed credit.

Two inherited manager-base checks remain blocked without any waiver or
out-of-scope repair: the provider-row guard reads only MQ `service.rs` although
its required row functions moved to `service/rows.rs`; strict xtask Clippy
reports `collapsible_if` at unchanged `xtask/src/docs.rs:183`. The external
receipts preserve both failures and byte-for-byte base comparisons. They
remain integration blockers; the allowlist digest seal is not a claim that
these gates passed.

Reviewer action: a HUMAN conformance maintainer must accept or reject every
proposed anchor and expected observation, review the separately pinned DA/DJ
explanatory rules, and settle the recorded applicability and missing classes
before creating an accepted reviewed-rule artifact. In particular, injected
SAF denial is not full RACF-profile evidence, fresh SQLite connection reopen
is not process death/XRST, and local rollback is not full ROLL/CHKP recovery.
Other contexts/organizations, path/command/logical/secondary variants, warning
positions, multi-PCB/Q/integrity, concurrency/failure/migration/retention and
licensed differential remain pending. Manager owns later Q/checkpoint/GSAM/
STAT integration and the remaining parent/release acceptance. This leaf
delivers candidate preparation only; human approval cannot be simulated.

### Manager candidate-IR integration and source-gap repair — 2026-10-02

The manager consumed exactly `6f1bd076a2d0fee70ed34c7946fc6917416ada79`
after sealed GSAM restart and application backout. The shared IR compiler,
runtime registry and runner remain authoritative; the candidate wrapper exports
diagnostics only and discards draft events/ledger. The accepted v1 spec remains
byte-identical, with no official IMS registration, reviewed-rule approval or
licensed differential. The packet still proposes 22 rules, 42 bindings and four
rows against 40 independent Memory/SQLite fixtures.

The previously unavailable DA/DJ explanations were found in the retained archive
metadata and verified offline. They are now registered in a separate zero-credit
`ims-status-explanations` scope, baseline
`ibm-ims-15.6-status-explanations-2026-09-11`, without repinning older baselines:

| Topic | SHA-256 | Bytes |
|---|---|---|
| `SSEPH2_15.6.0/com.ibm.ims156.doc.mc/msgs/da.htm` | `2cdd75d8c8b15e6ca28deecbc28b458fe57cb7bedaf72ac48b24cf5ba825201d` | 1804 |
| `SSEPH2_15.6.0/com.ibm.ims156.doc.mc/msgs/dj.htm` | `61eb932d71cf9d009e0e6652d24840945ba51ed6a3bc98e676dac54164e18c82` | 3085 |

Pinned `ibm_docs.py` search/read and the repository parser read both full topics;
the retained topic-path files were absent but the raw archive hashes/bytes match.
The existing pinned IMS TOC is unchanged. No download, browser refresh, whole-
cache audit or publication body in Git occurred. Proposed no-hold, changed-key
and intervening-Get cases now carry these exact anchors. Topic/hash/baseline
mutation tests reject mismatches. Availability resolves a source-location gap,
not human acceptance or the unproved explanatory/context classes.

Current checks pass 19 coverage tests, six IMS xtask tests, eight docs tests and
three tooling-driver tests. The shared preparation run executes 42 checks with
zero mismatches and rejects seven observation perturbations; a selected replay
executes one check. Ordinary IMS conformance deliberately refuses pending HUMAN
maintainer acceptance. These are diagnostics, not 42 official passes. Strict
coverage/conformance/xtask all-targets Clippy passes after the behavior-preserving
docs navigation let-chain repair. The prior MQ provider-row locator blocker is
already repaired by the manager. Formatting/module checks pass; new public draft
contracts are documented without new API authority or raised policy thresholds.
Runtime receipts precede the Rustdoc-only additions; their original tested-input
identities are retained, with an independent unchanged-code-token comparison.

Receipts: `ir-candidate-integration.log`, `ir-candidate-official-refusal.log` and
the bounded `ims-status-explanations-cache` under the completion receipt root.
Mandatory source-reader/policy/schema/spec/dependency/docs/changelog checks and
exact resealing accompany the manager feature commit. Human source/expectation/
applicability acceptance, complete applicable classes, participant/lease closure,
coherent restore/retention and full v0.14 acceptance remain pending. Licensed
certification is user-excluded, not counted passing. No parent/release credit.
## IMS-1405.tm-application-backout (bounded contract gap delivery, 2026-10-02)

Parent: IMS-1405, still in progress. Clean entry:
`bf0dc41c798ea344a195e5c2b3d47bfed7908277`; branch:
`codex/v014-tm-application-backout-20261002`. The user authorizes a precise
source/contract gap packet where genuine shared-owner authority is required.
This slice declares that delivery before implementation: proposed
[ADR-0031](../../../decisions/0031-ims-tm-recovery-publication.md), focused
signed scheduled-TM rejection and shared-store interleaving witnesses, this
status, a unique fragment, and normal generated documentation. No runtime,
public ABI, durable schema, participant capability, or algorithm is changed.

Catalog context is `ibm-ims-15.6-dli-2026-08-31:dli-call-families`
`:0017/:0018/:0020/:0021`, with `:0002/:0023` checkpoint and
`:0005/:0008/:0024` message-I/O dependencies. Required obligations are one
RecoverySession/DB UOW/TM publication authority, actual signed transaction
selection and claimed input, work lease/incarnation and core recovery fencing,
ordinary/unpurged/express-PURGed output distinction, point/commit interval
ownership, conversation/input resumption, suspend versus reschedule/terminal
disposition, canonical replay, live controls/SAF/bounds, and retained read-only
observation plus fenced settlement resolution. Memory and file SQLite are
the bounded investigation matrix; PostgreSQL and all implementation acceptance
phases remain pending until an owned contract extension exists.

The existing `ProviderStateStore` can publish TM and DB rows together but
cannot predicate that publication on a typed WorkStore lease or effect
recovery fence. The Memory work map is separate from provider rows; SQLite's
private `durable-work` encoding is not a portable provider authority. A
read-only lease check or `has_session` absence check cannot fence publication
against concurrent lease replacement/admission. TM release/completion remains
a second operation with retained replay-work data, without an authoritative
settlement outcome tied to the exact scheduling incarnation. The proposal
names the shared store/coordinator, host and IMS owner decisions required;
no private queue/work table/lock service or raw core-row mutation substitutes
for them. No dormant staged helper is shipped before those decisions.

Actual TM application backout remains Unsupported, including attempts to
label an existing TM session DB-batch. The preceding DB-only SETS/SETU/ROLS/
ROLL/ROLB and generic Batch undo/epoch/incarnation behavior is preserved.
The older base lacks the manager's extracted `product/ims.rs` owner; the only
product seam here is test registration in the existing recovery test owner.
Official, licensed, maintainer, parent and release credit remain zero; mixed
resource closure remains v0.16. ADR-0031 supplies source identities, the exact
missing guarantees, proposed compatibility/failure rules, and future acceptance
matrix. Current-candidate receipts and the sealed commit identity belong in
the external handoff, never a new committed execution ledger.

### Manager TM contract-gap integration — 2026-10-02

The manager consumes exactly `531f39c137ab5dad00d8db72f28b083aea2b19d3`
after sealed application backout and candidate-IR preparation. Its proposed
decision is renumbered ADR-0031 to avoid the independently allocated GSAM
ADR-0028; source content and Proposed status are preserved. Both documentation
navigation entries survive integration. No accepted ADR, store/execution API,
provider behavior, work codec, migration or participant capability changes.

The negative witnesses run real signed scheduled input and provider mutations,
plus Memory/file SQLite stale-lease and provider-CAS interleavings. They show
an unavailable guarantee: a provider-row transaction can succeed before a
separate work completion rejects its replaced lease. This is not successful
TM backout or a source-excluded fake execution. The required owner decision is
an additive shared conditional publication and authoritative work settlement,
with default fail-closed behavior for old implementations and genuine backend
failure/replay/crash proof. No provider-private work table can substitute.

Current manager verification receipts and exact gap-packet seal remain external.
Two signed scheduled-TM rejection cases and four shared-store test entries pass;
the no-environment process helper earns no scenario credit, while its substantive
parent asserts three real SQLite child phases. Strict changed-store-test and
server all-targets Clippy, module/architecture, dependency, IMS catalog/assurance/
schema/spec, formatting, docs and changelog checks pass in the manager checkout.
The older worker's inherited server-lint/module blockers are not waived; the
manager verifies its extracted owners directly. Human/official/participant/TM
execution/parent/release acceptance stays open. Licensed certification remains
excluded, with no execution credit. Actual TM recovery needs the proposed
shared-owner decision; read-only absence or lease observations are insufficient.
## IMS-1401.selected-pcb-feedback (bounded ownership declared)

Parent: IMS-1401. Base: `a01659799de8bf2291511714ac224279619a8571`.
Branch: `codex/v014-selected-pcb-feedback-20261002`. The sealed official-IR
candidate `6f1bd076a2d0fee70ed34c7946fc6917416ada79` stays on its branch.
Owned scope: additive explicitly versioned selected full-function database PCB
feedback DTO/canonical leaf, provider projection from the existing navigation or
mutation proposal, focused tests, minimal existing host/execution/provider/replay
and signed selected-package seams, ADR, unique fragment and normal documentation.
Historical canonical bytes and receipt authority remain fixed; replay must return
the retained feedback, never observe later mutable state to reconstruct it.

Acceptance classes: fail-first missing public owned response; source/metadata
derived literal status, segment level, key validity/bytes and data length on
successful/unsuccessful GU/GN/GNP and Get Hold; unequal keys/data, key-only/path,
independent PCBs and admitted secondary target/source; REPL invalidation;
authorization before observation, capacity/no mutation; exact replay after later
work; conflict, actual CAS and unknown lost acknowledgement; Memory, file SQLite
fresh reopen and relevant process boundary; signed package through the real
coordinator. Catalog context is `ibm-ims-15.6-dli-2026-08-31`, database get/update
families; exact source pins/rows and admitted classes follow offline review.
Focused regressions plus mandatory catalog/assurance/schema/spec/guards,
fmt/deny/docs/changelog and strict scoped Clippy are required. Exact path sealing
and committed `--check` follow passing scoped acceptance; receipts stay outside
Git and disposable targets. No official row, parent, maintainer, release or
licensed acceptance is claimed.

PCB state, key-only suppression, indexed virtual fields, parentage/lost hold,
SSA/index/GSAM/recovery/STAT/TM/backout/participant algorithms retain their owners.
No new cursor/parser/metadata/store/replay/lock/recovery authority, raised module
ceiling, delegation, refresh, other-checkout edit, push or PR is authorized.
Raw EBCDIC COBOL/C masks, physical GSAM RSA, unavailable feedback invalidation
semantics and fields without metadata remain Unsupported/unproved. If a source
or ABI prerequisite prevents honest implementation, retain Unsupported and
deliver an exact bounded missing-class packet rather than assign false credit.

The bounded additive host projection is implemented. `ImsPcbFeedbackV1`
request/result variants, `ImsService::execute_pcb_feedback_v1` and
`ProductServer::ims_pcb_feedback_selected_v1` share the existing provider and
signed publication fences. Primary successful Gets and ISRT project exact
metadata sequence bytes from the existing proposal path, with segment name,
level and valid byte length. Status/data and transferred length use that same
result. Capacity rejection discards the unpublished proposal. The optional
feedback output is retained in the existing receipt and included in its
canonical result/retention binding; exact replay returns it after later work,
fresh file SQLite connections and separate process restart.

Source review used the IMS 15.6 programming, database and metadata baselines
dated 2026-09-11. All selected retained topic-path files were absent; the
hash-addressed archive files matched and were read with the repository parser
and pinned search/read commands. Exact topics, hashes, catalog rows, independently
derived literals, shared integration seams and unproved classes are recorded in
[the bounded class review](selected-pcb-feedback.md), with the additive boundary
and downgrade procedure in [ADR-0035](../../../decisions/0035-selected-pcb-feedback.md).
No failed-call last-satisfied witness, selected secondary key-layout recipe,
primary REPL/DLET validity, missing sequence/logical recipe or physical ABI is
invented. Successful secondary REPL alone has source-defined invalidity.

Fail-first proved the public owned DTO/response absent. Focused and affected
host, IMS and signed selected-package tests pass, including the actual
coordinator, independent PCB/hold/path/key-only behavior, all six Get forms,
mutation conditions, deny/failure before observation, no-publication capacity,
conflict/actual CAS/lost acknowledgement and retained response equality.
Strict scoped three-package no-deps Clippy, fmt, dependency policy, IMS
catalog/schema/assurance, spec and affected architecture/module/participant
guards pass. The additional public-API documentation ratchet diagnostic is
blocked by the untouched execution API and existing host surface exceeding
its policy; new DTO items are documented and no policy ceiling is raised.
This is not a full-minor documentation/maintainer acceptance claim.

Receipts remain outside Git/targets under the slice's existing worker receipt
directory. The existing request module's HostResult declaration was moved
mechanically behind the stable export and its inventory ceiling lowered from
1611 to 1592; no frozen or new module ceiling increased. No navigation, index,
GSAM, recovery, STAT, TM, backout or participant algorithm changed. Parent
IMS-1401, complete-v0.14, official/maintainer/parent/release and licensed
acceptance stay open. Next work is the exact owner prerequisites in the class
review, then manager integration with the separate newer secondary SSA,
GSAM-checkpoint and backout leaves; none is silently consumed here.

### Manager composition of selected PCB feedback (2026-10-02)

The manager consumes sealed `86a8774f6e3eae07d2f222d7fd174a000da1b266`
after `284e769b` in its nonlicensed integration checkout. The common execution
proposal retains the newer application-backout prepare/settle hooks, generic
Batch undo retention, Q/image publication guards and GSAM checkpoint integrity
read authority. The older worker's Batch autocommit block is not reinstated.
HostResult moves mechanically beside its existing validation and stable
re-export; the frozen facade count lowers from1611 to1592 without a raised
ceiling. No feedback projection is used as a second cursor or lease authority.

The pinned database PCB mask source remains
`ibm-ims-15.6-programming-contracts-2026-09-11`,
`apg/ims_imsdbdbpcbmask.htm`, SHA-256
`699a551e0c2804db26725d0997be3b1f9fdc91379a69f490c8e509d76fcc61b3`;
catalog rows `dli-call-families:0005/:0006/:0008/:0015/:0004` retain their
existing denominator. Sensitive-type count follows explicit selected metadata;
KEYLEN is not guessed caller-area capacity. No raw mask or failed-call witness
is invented. The retained result-family corruption guard rejects a receipt
containing both feedback and GSAM, even if the selected feedback digest matches.
The added manager regression verifies this on Memory and file SQLite.

Thirteen provider harness tests pass, with zero scenario credit for the idle
process helper; the substantive parent verifies real independent SQLite child
phases. Twenty-six signed IMS package parent tests and two independent binary
feedback contract tests pass. Strict IMS/host/server all-target Clippy passes.
These receipts describe this code candidate, not later prose/commit CI evidence.
Mandatory policy/docs acceptance is recorded separately outside Git; manager
command-selection failures are retained rather than counted as gate passes.
The separate API-documentation repair is not folded into this feature seal.
Unsupported secondary/failure/raw ABI classes, official/human/participant/parent
and release acceptance remain open; licensed certification is excluded.
## IMS-1406.api-documentation-ratchet-repair (implemented documentation leaf)

Parent IMS-1406; target 0.14.0. Clean base:
`86a8774f6e3eae07d2f222d7fd174a000da1b266`; branch
`codex/v014-api-doc-ratchet-20261002`. Preserve the prior branch and the
`6f1bd076` official-IR candidate. This nonsemantic infrastructure leaf owns
Rustdoc additions only in execution `participant.rs` and host `ims.rs`,
`ims_applicability.rs`, `ims_metadata.rs`, `ims_navigation.rs`, `ims_pcb.rs`,
`ims_status.rs`, `ims_system.rs`, `ims_tm.rs`, `request/ims.rs`, `dataset.rs`,
`mq_message_contract.rs`, `mq_validation.rs`, `canonical.rs` and
`runtime_service.rs`. It also owns
this status section, a unique change fragment and normal generated docs manifest.
These are complete missing-item owner groups, including the bounded shared
message/validation contracts needed to return to the unchanged host baseline.
Inline enum fields retain their original tokens; variant docs explain their
units and validity collectively. The immutable runtime-service registry is a
complete additional shared host owner, with exact resolution and bounded inputs.
No generated Rust, provider/server code, algorithms or policy thresholds change.

The current base diagnostic is execution 305 versus 176 and host 2100 versus
1128; compiler 70, coverage 386 and store 95 equal their baselines. The older
selected-feedback diagnostic's host 2126 is historical, not this base's count.
Required obligations: accurate owned inputs/results, byte/count/tick units,
absence/default meaning, identity/fencing/uncertainty and fail-closed boundaries;
unchanged Rust code tokens, enum order, serde and canonical/type golden inputs;
unchanged ratchet equality, focused ratchet tooling tests, fmt, strict execution/
host all-target no-deps Clippy, affected no-deps Rustdoc, module/typed/supply-chain/
deny/participant-generator/schema/spec/docs/changelog guards and exact leaf
seal with committed check. No examples or runtime changes are planned.

The module checker actually counts physical production comments (ADR-0010);
no assumption that Rustdoc is excluded will waive its exact frozen inventory.
The selected modules fit ordinary budgets; frozen modules stay untouched.
Execution-context and backend applicability are unchanged; no official catalog
row or mandatory behavioral obligation is added or passed. Rustdoc describes
existing local contracts and explicit unsupported/pending boundaries, without
new IBM semantic assertions, source refresh or fabricated physical equivalence.
No unrelated IBM lookup, architecture-fast cache gate, runtime campaign,
licensed certification, participant admission, parent or release acceptance.
Receipts remain outside Git/targets in the external worker directory
`v014-completion-20261002/ims-api-doc-ratchet`. Manager composition of feedback,
GSAM formats, mixed SSA and selected-secondary restart remains separate.

### Documentation repair verification and handoff

The actual sealed-base failure is retained as `base-ratchet.log`; the historical
selected-feedback log is separately retained without candidate credit. The
unchanged all-package checker now passes exactly: compiler 70, coverage 386,
execution 176, host 1128 and store 95. This removes 129 execution and 972 host
missing-doc diagnostics through complete owner groups. Inline enum fields retain
eleven missing-doc diagnostics to preserve their original code tokens; their
units, bounds and identity meaning are described in the variant documentation.
Remaining documentation debt is the unchanged baseline, with no exemption,
suppression, threshold increase or detection change.

`contracts-final-receipt.json` records passing focused public-doc tooling tests
(3), formatting, strict execution/host all-target no-deps Clippy `-Dwarnings`
and no-deps Rustdoc for both affected packages. No examples were added, so no
new example requires doctest execution. `comment-only-identity.json` independently
verifies identical Rust tokens in all 15 owners and byte identity of 346 retained
golden/schema/generated/test/policy inputs, including historical canonical/type
inputs. It also verifies the base seal and preserved prior branch/candidate.
The largest changed owner is dataset at 1,076 physical production lines; every
changed owner remains under 1,200 and the frozen inventory is byte-identical.

Final module and typed-boundary checks pass in `policy-final-scoped-receipt.json`.
`policy-receipt.json` retains passing supply-chain, deny, participant contract and
participant-generator, schema and spec checks on their unchanged inputs; it is
not relabeled as a later candidate run. Documentation generation/check and
changelog checking use `packaging-receipt.json`; the final manifest regeneration,
docs check and seal use `completion-receipt.json`. Final staging must match the
exact 15-source/status/fragment/generated-manifest allowlist; the feature seal
and committed `--check` receipts are retained externally. Every Cargo sequence
cleans this checkout's default target, preserving receipts outside disposable
output. No runtime suite, cache-source gate or licensed campaign was repeated
for comments. These checks establish this documentation leaf only; all parent,
official, release, participant and licensed acceptance obligations remain open.

### Manager API-documentation composition (2026-10-02)

The manager consumes `0ced07859b1b314e5794a90a9ac48d3fbdcc6977` after
its selected-feedback seal `8379fd3f9eacd1301327ccafe92ece378fb7eea8`.
Independent comparison of all15 changed Rust owners against that actual manager
base proves exact byte identity after excluding Rustdoc lines; no runtime,
canonical, serde, schema, fixture, accepted-rule or ratchet-policy input changes.
The root gate passes at compiler70, coverage386, execution176, host1128 and
store95, including the newer backout and IR-candidate API surface. Module/typed
guards, strict affected all-target Clippy, formatting and normal docs/changelog
checks pass. Receipts remain external with actual code-candidate identity.
No runtime suite is repeated for these comments and no old worker receipt is
relabeled as this manager candidate. Unchanged dependency/architecture policy
results retain their original evidence identities. Source/ABI/participant,
human/official, full-v0.14 and release acceptance remain pending; licensed
certification stays excluded.

### Manager compiled-CBLTDLI gap composition (2026-10-02)

The manager consumes `24c9255909f93d02176758433edc5657f9e8aeb8` after
`eb5890acd6546214ebe18831d7a864a74553764c`. Its Proposed ABI decision is
renumbered [ADR-0033](../../../decisions/0033-cobol-dli-call-boundary.md)
to keep independent IMS decisions distinct. The now-integrated versioned PCB
feedback route does not supply the missing raw guest binding, capacity or
unsuccessful-call witness. No runtime adapter or schema is added by this leaf.

Before handoff interpretation, the manager searches/reads all361 lines of pinned
Enterprise COBOL6.5 CALL `lr/ref/rlpscall.html`, SHA-256
`fdf73c18a03049cd540efcfa652bfc7f842a16a1000c68a09a18938e2689cc4a`,
baseline `ibm-enterprise-cobol-6.5-2026-05-31:procedure-statements:0005`.
It also reads complete IMS invocation and batch-entry topics in
`ibm-ims-15.6-cobol-dli-boundary-2026-09-11`, hashes `49543439b7d78448832f226646b1393de857cc5290567b3d7a5f3984a0be8994`
and `b128e9b82ea93b39b92e75feafaaa620cdff6552713fe6cebe7a55ecfca35f6c`.
The previously reviewed programming PCB-mask pin remains unchanged. Default
reference shares storage, argument correspondence is positional, content/value
do not modify the caller, and IMS supplies PSB-ordered entry PCBs. A source
lookup's wrong initial scope spelling is corrected without source refresh.
The partial COBOL cache's unselected missing entries are not a whole-cache
finding; every selected pin is available and hash verified.

Three focused compiled tests pass through the existing signed Program provider
and coordinator on Memory and file SQLite, including20 real rejection cases,
literal copied-frame collision, output/canaries and unchanged IMS rows. Those
are gap/rejection witnesses, not raw operand validation or successful DL/I calls.
Strict affected Clippy, module guard, coverage policy, formatting and normal
docs/changelog checks pass. Empty filtered test binaries earn no credit.
Receipts retain the actual code candidate, separate from this final prose seal.
Compiler/guest CALL references, signed PCB-capacity/linkage binding and complete
validity-tagged raw feedback still require the shared owners' boundary decision.
No parent/raw/official/human/participant/release success or licensed credit is
claimed. Human implementation authorization has been requested separately.

### Current-main merge and Proposed decision identity repair (2026-10-02)

Current main advances to `f0727cf8b138439c1bed9da572fa37a43c167580`
through independently merged DB2 work. The manager preserves its code and two
existing dependency links unchanged; the shared conformance facade combines
the independent declared modules. Only documentation navigation/registry and
generated-manifest conflicts require manual resolution. No DB2 semantic change
is made by this merge. Because main already owns decision IDs0028/0029, the
manager's still-Proposed GSAM address and PCB-feedback decisions move to0034
and0035, respectively. Their actual links and titles follow those identities;
accepted/main DB2 decisions are not renumbered. Old feature seals/receipts retain
their original identities and are not rewritten. Reserved integration IDs0030
for GSAM formats,0031 for TM publication,0032 for secondary restart and0033 for
raw CALL keep independent ownership. Runtime/source rules and official/license
credit are unaffected by this navigation repair. Candidate merge checks and
cleanup have their own external receipts; later worker deltas are not credited
as part of this merge's unchanged IMS code.
## IMS-1403.gsam-record-formats (declared bounded leaf)

Parent IMS-1403 remains open. Exact clean entry is
`2b7a92a1e57848fa3fbe158b71c38c6de71009af`, branch
`codex/v014-gsam-record-formats-20261002`. Preserve prerequisite integration
`cb7aa908ff48527dfa35ff634c7831cd6fde341b` and both sealed GSAM features.
Catalog context: `ibm-ims-15.6-dli-2026-08-31:dli-call-families:0005/:0008`,
with :0002/:0023/:0016/:0025 as checkpoint/restart consumers. Scope is the owned
DbBatch CALL GSAM route on selected signed generic metadata, G/GS retrieval and
L/LS append, explicit logical V/U format and owned application record lengths.
Physical DASD/BSAM/VSAM/tape layout, raw RSA/PCB/AIB framing, JCL/label precedence,
OPEN/CLOSE, physical dataset BASIC/LARGE, participant/lease and licensed parity
remain unsupported. Synthetic segment bounds never infer RECFM.

Mandatory local obligations: fail-first public Unsupported; independent literal
V length/data and U separate length boundaries; malformed zero/small/large/
truncated/mismatched controls without mutation; GN/GU/ISRT identity/order/status/
EOF/independent PCB positions; SAF before read/write; exact canonical replay and
conflicts; quotas, atomic CAS and unknown acknowledgments; Memory/file SQLite
fresh reopen and child processes; capture/restart format identity and rejection
of wrong/stale format without mutation; historical absent metadata/image/replay/
checkpoint/canonical bytes. Tests supplement shared IR and grant no official
row or licensed credit. Missing safely representable ABI classes stay Unsupported
with explicit followup acceptance rather than invented equivalence.

Owners: bounded format adapter/helpers and explicit additive host/metadata/
canonical seam where needed, existing engine image and checkpoint resolver hooks,
focused public provider/signed-package tests, boundary ADR, unique fragment and
normally generated docs. Keep one dispatcher, address, integrity/UOW/recovery and
store authority; no secondary/SSA/STAT/backout/TM/lease/participant algorithm edits,
new dependency, frozen facade growth or budget ceiling increase. Manager mapping
targets service/providers.rs, service/execution.rs, canonical/dispatch.rs,
request/host_request.rs, request/host_result.rs and product/ims.rs; this base may
retain pre-extraction files. No other checkout or worker is touched.

Offline source review first checked retained topic-path files, then verified
exact repository hashes/bytes in the SHA archive with ibm_docs.py plain_text and
bounded search/read. Database/programming/metadata/recovery baselines and exact
GSAM recovery supplement remain unchanged. DATASET/I/O-area/origin details absent
from those manifests require a separately pinned zero-credit retained supplement;
no refresh or publication bodies enter Git. Acceptance is focused nonempty route
regressions and strict scoped Clippy, catalog/assurance/schema/spec/shared guards,
fmt/deny/docs/changelog, exact allowlist seal and committed --check. Receipts
remain outside Git/targets; cargo clean follows each sequence. Known unchanged
infrastructure is diagnosed once and reported. No delegation, certification,
push/PR, official/maintainer/parent/release completion is authorized.

### Owned record-format implementation and remaining ABI obligations

The additive GSAM route now admits explicit version-1 `ImsGsamFormat` metadata
for F/V/U, BSAM/VSAM applicability, block bound and None/ASA/machine control.
V retains the literal complete application area, checks big-endian LL against
its exact byte length and adds no physical ZZ/RDW bytes. U is BSAM-only and
requires a separate owned u32 length on ISRT; successful GN/GU return that
length from the retained record. U areas are greater than 11 and bounded by
BLKSIZE. The BSAM V projection accounts for the source's physical +2 ZZ bytes
in admission bounds without emulating their physical storage. See
[ADR-0030](../../../decisions/0030-gsam-application-record-formats.md) for exact
bounds, source derivation and upgrade/rollback obligations.

Absent format metadata remains fixed-only on the additive route. Historical
`ImsRequest`/navigation envelopes lack U's separate length ABI and keep U data
calls Unsupported. The owned bulk image contract validates the same record
areas through the existing engine. No raw PCB support is claimed. Optional
format, U replay length and saved format identity serialize by omission when
absent; prior fixed canonical goldens and ordinary checkpoint preimages remain
frozen. The signed generic metadata and engine definition own characteristics;
min/max never infer RECFM. There is no format, length or address side registry.

Capture validates current format/image and binds explicit format plus area
bounds to each saved GSAM PCB. Restart checks that identity before the existing
resolver changes position or removes witnessed output suffixes. Wrong/absent
saved identities, corrupt LL/U areas and stale image characteristics fail with
no row publication. Identity, integrity, UOW fences, checkpoint resolution and
atomic CAS remain with their prior owners. The common execution method is
extracted once to `service/execution.rs`; its authorization, dispatch, replay
and publication algorithm is retained, with only the optional owned output and
unrepresentable historical-U envelope check added. Shared guards follow that
owner. Frozen product/request/canonical outer dispatch facades do not grow.

Exact new source pins (product `SSEPH2_15.6.0`, IMS 15.6), baseline
`ibm-ims-15.6-gsam-formats-2026-09-11`, zero-credit topic-set digest
`3dd6d4c3d4a361dd23a3c5b65a2d873f085363c09715a9aa9539454d202957d8`:

| Topic below product root | SHA-256 |
|---|---|
| `com.ibm.ims156.doc.apg/ims_gsamioareas.htm` | `eb104bbf0d57784d7f70d39dfa6ece1436d8cff70d947f48cd16990b1abacbe2` |
| `com.ibm.ims156.doc.apg/ims_origingsamdataset.htm` | `90c324c979936c308b959f0e5ccd49a24eb17e0647aceda4b7568b649ad3459b` |
| `com.ibm.ims156.doc.sur/ims_datastmt.htm` | `847d7dd38e43c170861b9b5fba2d615a4813cb491e94330b2bca2265181d72b6` |

The retained topic-path cache was absent for the selected topics. Each required
topic was resolved from the bounded archive by expected repository SHA/bytes
and read with `ibm_docs.py`'s plain-text parser. No selected topic mismatch or
unavailability was found. Initial subsystem searches intentionally reported
unselected missing entries; those are not whole-cache verification. The new
three-topic scope search verifies only its explicit selected set and shared
TOC. Existing formats/PCB/retrieval/DBD and CHKP/XRST pins remain unchanged.

Followup/acceptance packet: raw PCB/AIB input/output length and memory ownership,
physical RDW/BDW/FB/VB, physical RSA, DASD/tape/VSAM device adapters, JCL/label
precedence, concatenations and BASIC/LARGE need explicit ABI classes and their
own public adapter. Preserve Unsupported until source-derived literal framing,
bad/truncated controls, memory bounds, device errors, independent PCBs,
checkpoint/restart and durable replay tests pass through that adapter. This
leaf supplies an owned application-area projection, not physical equivalence.
The source-derived followup is detailed in ADR-0030; licensed execution and
official/maintainer/parent IMS-1403/v0.14/release completion remain pending.

Candidate receipts, fail-first Unsupported and exact command/source identities
are outside Git and disposable targets at
`/Users/tore/Library/Caches/mainframe-env/worker-receipts/v014-completion-20261002/IMS-1403.gsam-record-formats`.
Memory/file SQLite tests use independent literal records, public provider and
signed selected-package routes, real CAS/failure/unknown acknowledgments and
actual process exit/reopen. No empty filtered suite or unconfigured process
worker grants scenario credit. New production modules stay below 1,200 lines.
The inherited server strict-Clippy diagnostics and unchanged server facade
budget failure are reported once and left to their owners; ceilings are not
raised. They are not relabeled passes by the bounded feature seal.

### Manager GSAM format integration — 2026-10-02

The bounded integration consumes only record-format delta
`9e97502604d61440047e4c1643190bb178fef5df` on manager base
`0634ffc221aa977c66a063ba792ab22b379a248d`, preserving the sealed raw COBOL and
original format branches. [ADR-0030](../../../decisions/0030-gsam-application-record-formats.md)
owns the new format decision; legacy GSAM ADR-0028 and selected-PCB ADR-0029
remain unchanged for the root manager's separate numbering composition.

The manager's complete execution owner retains authorization before read/replay,
integrity refresh and read fences, reservations, image/Q publication fencing,
application-backout prepare/settle and feedback projection before atomic publish.
The additive U length is carried in `feedback::ExecutionOutput`, live GSAM
results, canonical recorded host results and exact replay. Historical omitted
length bytes stay unchanged. The historical envelope guard follows retained
replay and authority. No old extraction or generic Batch autocommit helper is
restored. Saved format identity composes with the existing epoch/incarnation,
pristine XRST read source and output suffix witness.

Focused GSAM verification passes 53 parent harness cases: six host, 23 IMS unit,
17 recovery, two checkpoint-contract and five signed-package cases. The V/U
process parent also captures six configured seed/restart/replay child executions.
Two unconfigured recovery workers and empty filtered binaries earn no scenario
credit. Added Memory/SQLite composition regressions protect generic Batch undo
and epoch after V/U plus PCB-feedback mutations, rollback and later exact replay,
legacy replay before U-envelope rejection and mutually exclusive result outputs.
The CAS fixture now explicitly commits its winning foreign UOW before reading it;
this preserves the manager's unsettled-image fence. Separate focused feedback
(15 harness cases) and backout (19 harness cases) suites pass, with unconfigured
child entry points again excluded from scenario credit. Affected strict host/IMS
Clippy, formatting, module and typed boundaries, all eight architecture guards
and seven guard tooling regressions pass. Historical worker receipts remain bound
to their original candidates; current logs, hashes and remaining gate results
are external under the `ims-gsam-format-manager` receipt directory.

Source review verifies all 12 selected IMS 15.6 pins with the repository reader,
including full GSAM record-format, I/O-area, origin/DATASET, PCB and CHKP/XRST
topics. Topic-path cache entries were absent; exact retained archive hashes and
shared TOC were read locally without a refresh. Catalog context remains
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0005/:0008`, with
`:0002/:0023/:0016/:0025` checkpoint consumers. Source text stays external and
grants zero execution credit. Parent IMS-1403/1405, selected-secondary integration,
root current-main composition, participant/official/human acceptance, licensed
evidence, full-v0.14 and release promotion remain open.

### Root secondary/GSAM/current-main composition — 2026-10-02

Root consumes `d90b35ea39280e8d1667aa5033bab7a94ea18b5b` on GSAM-format
seal `88f88f23` after the current-main merge. The retained position validator
combines both leaves: hierarchy keys, GSAM positions and selected-secondary
identities are mutually exclusive; a format identity belongs only to GSAM.
Historical omitted fields still round-trip byte-for-byte. The new secondary
adapter explicitly supplies no GSAM format, and the older mixed-position fixture
explicitly supplies no U length. Root keeps the pristine XRST integrity read,
output-suffix witness, epoch/incarnation and one backout publication owner.
Proposed ADR-0032 links the renumbered IMS decisions 0034/0035, preserving main's
Db2 decisions 0028/0029 and their code/dependency identities.

The integrated recovery harness initially passed 72 existing entries and failed
the new joint fixture: it incorrectly declared a U record minimum of two rather
than twelve bytes. Two earlier compile errors were fixture composition issues,
not passing evidence. After fixing the fixture, the joint parent passes four
Memory/file-SQLite format/backend combinations. One checkpoint contains a primary
position, two distinct selected-secondary positions and GSAM input/output positions;
reopen restores all five, preserves V/U areas and U length, continues each PCB,
and rejects an address from the truncated output suffix. The four binary retained
contract tests pass, including the new secondary-with-GSAM-format rejection.
Strict IMS all-target Clippy, formatting, module, docs and changelog checks pass.
Idle subprocess helper entries are not independent scenario credit. Receipts
remain external under `v014-completion-20261002/secondary-root-*`; prior worker
receipts retain their actual base. Final signed-route/mixed-SSA and aggregate
policy composition is a separate candidate sequence, not retroactive credit.
This completes only the bounded secondary restart leaf, not IMS-1405 or v0.14.

### Root mixed-SSA composition and continuation repair — 2026-10-02

Root retains the guard as separate seal `0cbb08a0` and consumes evaluation
`5242de84feb32751815ef6b4bad8ddb668a73654` after secondary/GSAM/current-main.
Exact-occurrence GU and disjoint independent-group scans are thin adapters over
one traversal loop; root retains both filters and the selected-field collision
resolver. Both raw-CALL and mixed-SSA supplements remain registered without
repinning any existing publication. The separately sealed AMS locator repair
is present. Initial composed runtime evidence passes 244 IMS unit entries,
74 recovery entries, four retained recovery contracts, 26 selected host unit
entries, two binary SSA recognition tests and 37 selected server entries, plus
strict affected all-feature Clippy. Unconfigured process helpers earn zero credit.
All mandatory offline policy/schema/spec/catalog/assurance/coverage and exact
public-API-ratchet checks pass on their actual recorded inputs.

A read-only Codex CLI review at gpt-6.1-sol/high/default service tier, fast mode
disabled, found one concrete P2 continuation defect. The review retains its
original HEAD/file hashes and is static, not test evidence. Root's first fixture
hit an outside-group Unsupported guard because it included an extra unmatched
third root; the corrected two-target fixture reproduces the actual silent skip:
GE is returned before the surviving correlated target in reversed groups.

The repair retains the original bounded checkpoint witness in the existing PCB
position when exact GU cannot find it. The same traversal compares this boundary
in qualification order, not the ordinary byte-order predecessor. Actual selection
consumes the provenance; CHKP cannot promote an unresolved predecessor into a new
witness. Default omission preserves historical bytes, strict validation protects
retained shape and old readers must reject the new field. The regression restores
GE, reopens, then returns the remaining target once in each independent group
before final GE. Repaired IMS unit/recovery and strict IMS lint checks pass;
the final signed-route/corruption/coverage checks are recorded separately after
this changed input. No original failed or pre-repair receipt is relabeled.
Source bindings are the pinned XRST deletion/unique-path rules and independent
AND group rules, rows :0005/:0023/:0025 with :0004 maintenance. Raw/TM shared
owner decisions, human rule approval, full applicability and release acceptance
remain open; licensed certification remains excluded with zero credit.

The follow-up static review confirms the valid-state repair but finds a retained
shape defect: merely existing segment names admitted a reversed historical path.
Root's corruption regression reproduces that acceptance before the repair.
Definition-bound validation now checks ordered parents, exact sequence-key widths,
the index source endpoint and a shared root target, without requiring deleted
occurrences to be live. The focused regression includes zero digest, reversed
path, wrong endpoint, wrong key width and mismatched root-target corruptions.
Review reports and failed attempts remain separately bound to their original
bytes. No shared CALL/store or host request/result contract, pin, threshold or
acceptance rule is changed; the retained position field requires compatible readers.

Final definition-bound repair checks pass 244 IMS unit and 75 recovery harness
entries, four retained recovery contract tests, the negative/reopen regression
and both signed secondary checkpoint routes. Six unconfigured recovery helpers
and two unconfigured IMS unit helpers receive zero scenario credit. A mistaken
`ims_secondary` server filter selected zero tests and is explicitly uncredited;
the corrected `secondary_ssa` selector is recorded separately. Earlier 37 selected
server entries include 35 IMS-package entries and two incidental reclaim tests;
the latter are not IMS execution evidence. Strict affected all-feature Clippy,
module/typed guards, docs/changelog and mandatory coverage policy pass after the
repair. Original dependency/schema/spec/catalog/assurance/API-ratchet and other
policy results retain their input identities; their relevant unchanged inputs
are not relabeled as committed-head CI. Root verification receipts and Rust
identities are external under `boundary-path-*` and `mixed-evaluation-root-*`.
The initial mixed worker's stale AMS failure is resolved by composing the root's
probe repair; no acceptance gate was waived. The PR stays draft and v0.14 open.
## Historical prerequisite packet imported from bdbca36 (2026-10-02)

The following packet retains its original source and verification disposition.
It is not current integration evidence. The four supplement bodies have now
been reviewed offline before integration; the evaluation delta is a separate
feature and seal. Manager selected-index field resolution and the existing
child-name collision regression are preserved. Integration receipts are kept
in the external ims-mixed-manager receipt directory.

## IMS-1401.mixed-boolean-ssa (scope declaration, 2026-10-02)

Parent IMS-1401 remains open. Clean sealed base is
`b7068757a3af472579c2e493f2ff9eb1a4909a66`, isolated branch
`codex/v014-mixed-boolean-ssa-20261002`. This leaf owns bounded SSA helper
changes in the existing public parser/AST and database matcher, minimal rich
SSA integration, focused new tests, a unique fragment and routine generated
docs. No separate expression engine or cursor, frozen facade growth, recovery,
GSAM, checkpoint, STAT, lock, integrity, row-guard, coordinator or participant
algorithm change is authorized. Manager execution/host-result extractions and
shared SessionCasStore integration remain external seams; do not duplicate them.

Catalog scope: `ibm-ims-15.6-dli-2026-08-31:dli-call-families:0005/:0006`,
with inherited :0004/:0008/:0015 maintenance interactions. Expected classes:
source-defined mixed dependent AND / OR / independent AND grouping and encoding,
exact binary relations and metadata byte lengths, multiple clauses/groups,
primary hierarchy and admitted full-function selected-root secondary order,
distinct source/target composite XDFLD, C/D/P/O preservation, per-PCB position,
parentage, hold and sensitivity, malformed/unsupported no mutation, SAF,
canonical replay/conflict, actual atomic CAS/lost acknowledgement and Memory /
file SQLite reopen/child process where applicable. No unsupported context is
loosened. Missing precedence/grouping source claims require an independently
authored acceptance gap packet and zero implementation credit for that class.

Before semantics, inspect exact IMS 15.6 programming/database pins using bounded
offline search/read, retained topic-path root first, then SHA archive verification
and the repository plain_text parser. No refresh or publication bodies in Git.
Acceptance uses fail-first public provider and signed selected-package cases,
focused affected regressions and mandatory policy/catalog/assurance/schema/docs/
fmt/deny/changelog gates, then exact allowlist feature seal, commit and committed
check. Receipts remain outside Git and disposable targets. Official, maintainer,
licensed and parent/release completion credit stays zero. Consumed dependencies
and compatibility boundaries in earlier handoffs remain applicable.

### Source-gap disposition and bounded runtime guard

Mixed evaluation remains pending. The independently authored
[acceptance gap packet](mixed-boolean-ssa-gap.md) records exact missing claims,
discriminating fixtures and pending outcomes, without an expression evaluator
or official bindings. Verified programming pins establish binary comparison,
exact field/value lengths and all five connector encodings; they do not establish
mixed precedence or dependent versus independent AND evaluation. The SSA overview
links `SSEPH2_15.6.0/com.ibm.ims156.doc.apg/ims_multiqualificationstmts.htm`.
That body and the TOC-located `ims_examplmultiqualificationstmts.htm`,
`ims_multqualificationstmtshdam_phdam_dedb.htm` and
`ims_multqualificationstatementssecondaryindex.htm` have no registered body pin.
Their expected SHA/byte counts are unavailable; retained topic-path files are
absent. No unpinned archive body is promoted to authority and no refresh occurs.

Sixteen bounded existing topics were checked first at the retained root, then
read from matching SHA-archive bytes through ibm_docs.py search/read/plain_text.
TOC pin `aaa12586b41e9994921bfddce588b186dc5bdda8ab253db054ae1e5014d6f618`
verified. Programming baseline
`ibm-ims-15.6-programming-contracts-2026-09-11`: APG ims_ssas.htm
`7a0fb0faa50c4308924576ebbe6bfcca0dd78e217f33c59e78480b9c6dd8ae20`,
ims_ssacodingrules.htm
`cfb772b7ae68ea657006d135792441a4da20185c2c8833389171c76432c0fba5`,
ims_ssacodingformats.htm
`dd8afe24c819750ec361d8359529ad328c4ef280235553da7bdd918be28504d9`.
Database baseline `ibm-ims-15.6-database-contracts-2026-09-11`: APG
ims_ssassecondaryindex.htm
`4b3a1ee3cc0eabbbb4984d23a0eb60fe93be50887e1853643e0f382710139900`.
Full source identities, explicit bounded-cache omissions and unavailable claims
remain in external sources.json, source-review.log and source-gaps.json. There
was no mismatch or contradiction in the selected verified body set.

The sole production change tightens database/ssa.rs admission to uniform
connector identities. Previously `&` plus `#` silently executed as uniform AND.
It now returns local Unsupported before matcher/publication, just as existing
OR/AND mixtures do. Encoding aliases `*`/`&` and `+`/`|` still map to the same
identity and remain admitted. This is a conservative local guard, not an
IBM-documented rejection or implemented mixed-expression claim. Parser/AST,
field resolver, C/D/P/O, cursor/position, canonical/retained schemas, index
maintenance and atomic publication algorithms remain unchanged.

Fail-first public-provider and signed-route receipts reproduce unintended
mixed-AND success. Initial secondary fixture expectation and server nested-module
locator failures are separately retained, not credited as semantic proof.
New focused checks pass two public-parser tests, four provider cases and two
signed selected-package cases. Memory/file SQLite fresh reopen preserves rows,
position/hold, replay/conflict and denied-observation behavior. Fixtures preserve
unequal primary/index order and distinct child source/root target composite
bytes; no mixed evaluation outcome is guessed. Existing mixed-OR rejection is
also exercised, along with malformed grouping and length boundaries.

Upgrade admission rejects old completed mixed-AND requests before replay. Retain
their canonical receipts and reconcile/drain them; rejection does not prove
nonpublication and must not trigger redispatch under a new key. No SQL, AST,
host/canonical or row migration occurs. Older binary rollback restores the
flattening defect. Inherited extended-index/undo/recovery downgrade and coherent
backup restrictions remain unchanged. Manager-owned integrity integration,
service/execution.rs, request/host_result.rs and shared SessionCasStore retain
their authority; this patch does not edit or resurrect those owners. The older
nested session_cas helper is untouched, so manager re-export integration has
no competing implementation in this leaf.

### Local verification and seal boundary

Current runtime inputs are frozen in external runtime-inputs.json. Focused new
tests above pass, plus 11 existing primary SSA cases, 13 existing rich-secondary
cases and four inherited signed secondary cases. These cover exact binary
relations, C/D/P/O, failed positions/status, sensitivity, real update/index
maintenance, rollback, Memory/file SQLite reopen, actual atomic session CAS,
lost acknowledgement/UnknownOutcome and three independent SQLite child processes.
The ordinary unconfigured worker helper was excluded; only the parent that
actually launches its processes earns local process evidence. Zero-test filtered
binaries earn no evidence. Signed primary selection and old canonical goldens
are the final compatibility checks recorded separately in external handoff.md.

Strict host/IMS/server all-target Clippy --no-deps -D warnings, fmt, offline
deny, supply-chain and license policy, schemas, IMS catalog/assurance, shared
spec integrity, changelog and diff checks pass. Execution/effect/provider-row/
storage/enterprise SAF/retention/participant guards and the participant generator
check pass. The module ratchet passes without any ceiling change; the sole
production helper is smaller than its base. No unrelated aggregate cache audit,
CardDemo-full, PostgreSQL, fuzz/coverage campaign, licensed certification, push
or PR occurs. Required official/parent gates remain pending, not waived.

Receipts remain outside Git/targets in the external
worker-receipts/v014-completion-20261002/ims-mixed-boolean-ssa directory. Each
Cargo sequence ends with cargo clean for this exact checkout. The merge driver
is installed. Final routine docs generation/check, exact ten-path leaf seal,
feature commit and committed seal check are packaging commands retained there.
The seal's pass disposition applies only to this bounded guard and acceptance
gap packet, not the pending mixed evaluation, any official gate, maintainer
approval, licensed differential, parent IMS-1401 or v0.14 completion. Next step:
authorize and review exact pins for the four missing topics, then implement and
prove independently derived mixed outcomes through the same existing authority.
## IMS-1405.private-recovery-retention-fence — bounded implementation declaration

Base: `a8deb3d8a97be2660cfa0d38327ff246a194a910`. Disposition: **Complete**
for this user-authorized bounded fence and its declared local proof; parent
IMS-1405 and v0.14 remain incomplete. ADR-0037 stays Proposed; it is the
authorized leaf decision, not an external approval claim.
This leaf preserves canonical recovery references by OR-ing conservative presence
of reserved `ims-recovery-v1-` rows into the existing unowned core fence under
the existing provider epoch. It adds no expiry, attribution, graph decoder,
target, coordinator, store, lease, participant, raw CALL or TM authority.

Applicability: existing Memory and file SQLite maintenance; configured isolated
PostgreSQL gate when available. Required local proof starts with an actual
failing planner/signed selected LOG regression, then empty/present/malformed/
unknown prefix cases, real forecast and maintenance, store failure, real stale
epoch insertion, ordinary replay pruning with canonical private replay intact,
and separate SQLite process plus coherent signed-package backup/restore.
Reopen alone is not process evidence. PostgreSQL 18 bounded gate is unconfigured
on this host and remains pending; local completion grants no PostgreSQL parity.

Owners: IMS retention helper/export, existing server dependency inventory and
existing test owners below. Shared retention/store APIs remain unchanged.
Catalog `ibm-ims-15.6-dli-2026-08-31/dli-call-families` :0010 LOG,
:0023 symbolic CHKP and :0016/:0025 XRST motivate reference preservation;
no official row, denominator, licensed, participant or acceptance credit changes.
Required gates: focused new proofs and affected replay/checkpoint/epoch contracts,
strict IMS/server all-target Clippy, fmt, offline deny, docs, changelog, coverage
policy, IMS catalog/assurance, schemas/spec, affected execution/effect/provider/
storage/SAF/retention/participant/typed-boundary/module/public-API guards,
participant generator, supply-chain and license policy. Receipts remain external.

Exact revised allowlist (13 paths):

```
crates/providers/mainframe-env-ims/src/retention.rs
crates/providers/mainframe-env-ims/src/lib.rs
crates/apps/mainframe-env-server/src/retention_maintenance/provider.rs
crates/apps/mainframe-env-server/src/retention_maintenance/provider/dependencies.rs
crates/apps/mainframe-env-server/src/retention_maintenance/provider/ims_recovery_tests.rs
crates/apps/mainframe-env-server/src/product/tests/ims_application_recovery_tests.rs
docs/delivery/subsystems/ims/programming-status.md
changes/unreleased/ims-private-recovery-retention-fence.toml
docs/generated/documentation-manifest.json
docs/decisions/0037-private-ims-recovery-retention-fence.md
docs/contracts/RETENTION-LIFECYCLE-V1.md
docs/documentation-registry.json
docs/README.md
```

The last two paths revise the authorized eleven-path declaration solely because
xtask/docs.rs requires every numbered ADR in normative_documents and navigation;
normal docs generation updates the portal. No baseline, policy, dependency,
catalog, ledger/evidence schema or adjacent ADR-0036/0038 owner changes are admitted.
All four dependency-sensitive core families may remain globally protected
indefinitely while any private row exists. Audit/outbox and excluded legacy
session/checkpoint/undo families gain no new protection from this leaf. Private
expiry and complete mixed backup/restore remain external owner/parent obligations.

Local outcome: the fail-first signed selected LOG tests reported two eligible
canonical effects on both Memory and file SQLite after actual ordinary replay
pruning while a private LOG row remained. The implemented IMS-owned max-one
prefix predicate now makes the existing core inventory unowned. It does not
alter ordinary replay planning, source CAS, store APIs, payloads, target order,
watermarks or idempotency lifetimes. Provider insertion after empty inventory
rejects stale archive epoch; insertion before the closing inventory epoch check
rejects the scan. Failed prefix/store reads cannot certify absence.

Two IMS helper tests and eight server proofs pass. The server parent executes
three substantive independent SQLite processes (seed, backup, restore); its
ignored worker alone earns no credit. The restore uses existing integrity and
VACUUM INTO APIs at fresh paths, exact IMS/journal/epoch/clock snapshot equality,
canonical LOG replay, authoritative symbolic-checkpoint observation and all six
signed-package blobs copied with file metadata and verified through
LocalArtifactStore. The combined checkpoint fixture contains inherited direct
receipts whose ordinary retention planner scan fails closed; this composition
keeps them intact. Ordinary replay pruning and subsequent canonical private
replay preservation are separately proved by the two signed LOG tests. The
bounded composition does not certify arbitrary signed recovery graphs, full
GSAM/secondary/backout backup matrices, v0.16 mixed restore or PostgreSQL.

Affected regressions pass 69 provider application-recovery cases (six idle
process helpers excluded), eight store retention/watermark/archive saturation/
concurrency cases, and seven existing server planner cases. Three PostgreSQL
cases across these selections remain ignored/unconfigured, not parity evidence.
Strict IMS/server all-target Clippy --no-deps -D warnings, fmt, offline deny,
public API documentation, source guards, participant generation, supply-chain,
license notices, IMS catalog/assurance, schemas, spec, changelog, coverage policy
and docs generation/check pass. Spec also runs 134 Python cases. Coverage policy
and assurance inventory are not a new instrumented coverage campaign. No
baseline module/API ratchet, dependency, catalog denominator or accepted IR rule
changes. Source review and earlier audit paragraphs remain reference facts;
only current executed logs provide proof.

Receipts and input hashes are external under
worker-receipts/v014-completion-20261002/private-retention-implementation.
Every Cargo/build/test/lint/generator sequence ends with this checkout's cargo
clean, including failed attempts. Final docs refresh, exact thirteen-path
generated Complete seal, one commit and committed --check are packaging gates;
the external handoff records their actual outcome. No push, PR action, browser,
download, cache campaign, full CardDemo or licensed/certification work occurs.
Canonical/audit/journal saturation and pending participant contracts keep their
existing owners; this read-only inventory predicate introduces no atomic audit
or admission extension. Drain older maintenance clients before retaining private
graphs; rollback must disable maintenance or retain a compatible fence. Private
expiry needs accepted attribution/age/horizon and archive-before-CAS authority.

### Root composition declaration for private retention

Root integrates worker `5a4588bf0c28f1822322cfa5cfe21af9edb9a84f` onto
`339b653bf3204a4a0eea9b28e5a62e1eec401803`, preserving the original worker
receipts and thirteen-path allowlist above. Review confirms the production
predicate is in the core inventory, not the ordinary nested replay planner;
only the existing four-family unowned fence changes. The normal documentation
registry merge keeps ADR-0036 and ADR-0037, and the manifest is regenerated
through its owner rather than resolving generated hashes manually.

Before verification, the manager adds exact child-count and phase-completion
assertions to the existing three-process parent. A successful zero-test selector
must not become durable evidence. This affects only the already-declared server
test file. Root focused helper/planner/signed LOG/backup composition checks,
strict affected lint, relevant boundary/policy gates and normal docs/seal checks
will bind to this composed candidate; their results remain pending until actually
executed. No old runtime receipt is relabeled as root candidate or CI evidence.
Full PostgreSQL acceptance, private expiry, official/HUMAN rows and the full
minor remain open.

Root additionally found the already installed exact PostgreSQL 18.6 tools.
The repository's read-only parity prerequisite check passes, including pinned
runtime policy and support files. Before any backend claim, the manager will
start one disposable loopback-only cluster in a fresh task-specific temporary
directory, using no existing server, database or data directory. Only the
declared private-retention planner/epoch case and existing PostgreSQL retention
contract are selected, with a fresh owned database between them. No install,
full CardDemo, whole-backend campaign or licensed work is authorized by this
local attempt. Its execution outcome remains pending; worker PostgreSQL skips
retain their original identities and cannot be relabeled as passes.

Root outcome: two IMS helper cases and eight server cases pass on the composed
candidate, plus the parent actually executes three independent SQLite children
(seed 38100, backup 38110, restore 38111). Each now proves one passing exact
child test and its phase-completion marker. Strict IMS/server all-target and
all-feature Clippy passes; this sequence cleans 6.5 GiB of Cargo output.

The separate focused PostgreSQL attempt passes against verified installed
18.6 (server_version_num 180006): one private-retention planner/presence/epoch
case and one existing core retention contract, each explicitly selected with
--ignored --exact on separate fresh owned databases. No zero-test selection
is credited. The disposable loopback cluster is stopped, its log retained
externally, and only that resolved generated task directory is removed. Cargo
clean ends the PostgreSQL sequence. These two root receipts do not alter the
worker's original skips or establish signed LOG/backup parity on PostgreSQL.
The historical attempt declaration above remains as provenance. Composed
execution/effect/row/storage/SAF/retention/participant/typed/module guards,
participant generation, offline runtime/dependency/license policy, normal docs,
changelog, catalog/assurance/schema/spec/coverage and exact existing API ratchets
pass. Final packaging regenerates documentation and the exact thirteen-path
feature seal/check; it does not rerun unchanged runtime suites for this prose.

## IMS-1406.integration-dependency-inventory — declared metadata repair

Root entry is the clean private-retention seal
`d800c089d98168775cf43850682807d465bb5ec1`, target 0.14.0. The logical-feedback
worker's actual architecture-fast failure and unchanged-base comparison expose
the existing `mainframe-env-db2 -> mainframe-env-encoding` normal dependency
absent from the declared graph. Cargo.toml already owns that dependency; this
leaf changes no dependency, library, runtime behavior, semantic rule or boundary.
IBM lookup is not required for this infrastructure inventory correction.

Use one current-release additions artifact under conformance/0.14/inventory and
the existing shared graph comparator's ordered additions list. Do not rewrite
the historical 0.1 graph or earlier release additions, suppress graph comparison,
change Cargo.lock/manifests, or weaken layer/module/API policy. Scope is exactly
the new dependency-additions.json, one lookup entry in xtask/src/main.rs,
this status, the unique v014-db2-encoding-dependency-inventory-20261002.toml
fragment and normal generated documentation-manifest.json (five paths).

Required proof: the original missing-edge failure remains external; actual
architecture-fast must progress past the exact graph comparator after repair.
Any later unrelated prerequisite failure remains a global-gate blocker, never
called architecture pass. Negative missing/stale declarations must still fail
comparison. Selected metadata/schema/spec, strict xtask lint, formatting,
dependency policy, normal docs/changelog and exact-path seal/check are required;
no runtime suite or full source/cache/backend campaign for an inventory-only
delta. Cargo clean ends the sequence. Receipts are external, old worker/CI
identities are not relabeled, and the full minor stays incomplete.

The composed actual architecture-fast run now progresses past dependency graph
comparison and the execution/participant/effect/row/storage/SAF/retention checks.
It stops at the previously diagnosed CICS sources-a review prerequisite:
missing cached SSJL4D_6.x/applications/designing/dfhp37p.html. That global gate
remains failed; no source refresh, acceptance waiver or whole-cache audit occurs.
Actual comparator probes with the new declaration temporarily empty and with
one extra DB2-to-coverage edge independently reject the exact missing and stale
edges. The final additions artifact contains only the real encoding edge.
These probes preserve exact equality rather than relaxing it. Final scoped
policy, documentation and packaging outcomes remain pending until execution.

Strict xtask all-target lint, schemas, spec (including 134 Python cases), module
and typed-boundary checks pass. The first dependency-policy invocation placed
--offline after check and was rejected without policy credit; its sequence
cleans Cargo output. The corrected cargo deny --offline check and formatting
pass and clean. Normal docs/changelog and exact five-path content sealing are
the remaining packaging checks, with actual outcomes recorded externally.

## IMS-1403.logical-child-physical-key-feedback — bounded implementation declaration

State: local bounded implementation and affected gates passed.
The generated leaf seal is the completion boundary.
Base `a8deb3d8a97be2660cfa0d38327ff246a194a910`.
This leaf alone is authorized; IMS-1403 and the parent v0.14 remain incomplete.
No official, HUMAN, licensed or participant acceptance credit is claimed.

Applicability: DbBatch typed CALL, primary HIDAM database PCB with G/AP, successful six
Get/Get Hold forms, fixed-length records with unique sequence fields along the
actual physical source path. A selected real logical child must have one
deduplicated forward declaration and one validated occurrence link, with an
actual physical parent on the source path, including
a non-root logical parent. Unaffected ordinary physical paths in such a
database use the same proven source-key recipe. Other logical classes retain
Unsupported: virtual/reverse/SOURCE aliases, multiple links, unkeyed/nonunique
paths, variable lengths, secondary sequencing, logical ISRT feedback and other
contexts. Failed-call witnesses remain unproved.

Rows: `ibm-ims-15.6-dli-2026-08-31:dli-call-families`, selected retrieval
0005/0006; affected inherited maintenance 0004/0008/0015 and checkpoint
0002/0023/0025, exact `html-table:comparison;row:<n>;command` locators. No
catalog denominator, bindings, schema, policy or ratchet edits. Source identities
and verified hash/topic locators are retained in the external logical audit;
ADR-0038 records this physical-direction recipe and its exclusions.

Owners: only service/feedback.rs projects from the same unpublished proposal.
Physical C/qualified SSA selection, engine cursor/position, key-only sensitivity,
source/destination SAF, integrity/foreign undo/Q fences, correlated key
maintenance, checkpoint capture and proposal/replay/CAS remain their existing
owners. No host, shared CALL/TM/UOW, store or retained representation changes.
ADR-0036 null SSA and ADR-0037 retention are separately owned and untouched.

Exact handwritten allowlist: feedback.rs; generic/tests/feedback_tests.rs
(module declaration); its new logical_navigation_tests.rs; server
ims_package_tests.rs (module declaration); its new logical_feedback_tests.rs;
this status; selected-pcb-feedback.md; the isolated
ims-logical-child-physical-key-feedback-20261002.toml fragment; and Proposed
docs/decisions/0038-logical-child-physical-key-feedback.md. The manager approved
the necessary ADR registration in docs/documentation-registry.json and normal
generated docs/README.md plus docs/generated/documentation-manifest.json output.
These precise packaging paths extend the final allowlist to twelve files; they
add no semantic authority or runtime scope. No other production edit.

Backend/test map: fail-first public provider and signed selected package with
literal A1C1 source key and two LPARENT(L2) occurrences under P9/Q8. Memory,
file SQLite/fresh connections and a substantive subprocess parent cover all
six Gets, PCB isolation, C/qualified path, binary keys, K sensitivity, failure
witnesses, exclusions, SAF denies, integrity/foreign undo/Q, actual session CAS,
capacity/lost acknowledgement, exact replay after mutation, old Unsupported
receipts and mixed physical/secondary/GSAM checkpoint composition. Required
focused inherited suites, scoped strict Clippy/fmt, deny/docs/changelog/coverage,
architecture/module/API and catalog/schema/spec/assurance gates precede the
exact generated leaf seal and committed --check. Receipts stay outside Git in
worker-receipts/v014-completion-20261002/logical-feedback-implementation.

The public provider and signed selected-package routes now project the real
source physical key from the unpublished proposal. Literal destination twin
fixtures discriminate the exact retained link, and direct engine-path checks
confirm each selected/restored source position before later navigation. The
existing checkpoint owner composes physical, secondary and GSAM PCBs; later
source/destination changes do not reproject an exact retained reply. An
independently authored historical Unsupported receipt retains its availability
and bytes. No canonical, SQL, receipt, checkpoint or host representation changes.

Required scoped Clippy/fmt, dependency/license/supply-chain policy,
schema/spec/catalog/assurance/coverage, docs/changelog and affected execution/
effect/provider-row/storage/SAF/retention/participant/module/typed-boundary guards
pass. External receipts retain actual candidate inputs, fail-first outcomes and
fixture/lint repairs. Cargo clean ends each Cargo sequence for this checkout.

An additional global architecture-fast check fails before its broader checks:
the exact base already has a DB2 -> encoding dependency missing from the declared
graph. base-architecture-gap.json verifies unchanged manifest, graph/additions,
root manifest and checker bytes. The affected guards pass independently; no
graph or ratchet is edited. This unrelated parent integration gap is retained,
not represented as a passing global architecture gate. No full campaign,
licensed oracle, official/HUMAN/participant/parent completion, push or PR occurs.

Remaining source gaps: virtual/reverse/SOURCE alias paths, destination
KEY/DATA/RULES, failed-call witnesses and secondary feedback recipes. Other
logical contexts, organizations, PCB options, variable/unkeyed/nonunique/multiple
link paths, logical links attached to physical roots and logical ISRT feedback
remain Unsupported as declared above. Root-link metadata supplies no proof of
the approved physical-child ancestry. The selected pinned topics supply no
root-link key recipe for this metadata shape; no such rule is inferred.

### Root composition declaration for logical physical-key feedback

Root integrates worker 0bea49ccfa28b407a01ee226b1654dd96d085216 onto
6e3e24d014cfb4a58c6b79efff88e9b999bfc9cc. The exact twelve-path allowlist is
unchanged. Merge conflicts keep both logical and null-SSA server test modules,
both Proposed ADR-0037/0038 entries, and all previous status declarations.
Generated README/manifest are refreshed normally. No worker runtime receipt is
relabeled as root or CI proof. The only production feedback recipe is byte-for-
byte the reviewed worker seal; root owns composition verification and packaging.

Before any root result claim, select the eleven logical provider entries and
two signed backend entries, the prior signed null-SSA route and one retained
private recovery planner case. Strict affected all-target/all-feature lint,
boundary guards, normal policy/docs/changelog and exact-path seal/check remain
required. No global architecture retry for unchanged CICS sources: the separate
metadata repair already corrects the worker's original graph failure, and its
actual broader gate remains failed at the missing source prerequisite. Root
does not refresh sources or waive that gate. Leaf/local proof does not complete
IMS-1403, HUMAN/official/participant acceptance or full v0.14; licensed work stays
excluded. Cargo clean ends each intended-checkout verification sequence.

Root focused outcome: eleven logical provider entries and two signed backend
entries pass, including three actual seed/mutate/reopen SQLite child processes.
The separately selected prior signed null-SSA route and SQLite private-retention
planner each execute one passing case. Strict IMS/server all-target/all-feature
Clippy and formatting pass; Cargo clean removes this sequence's 6.5 GiB output.
No production feedback bytes differ from the worker seal. Unchanged schema/spec,
source/catalog/assurance/API-ratchet inputs retain their earlier passing root
and worker policy receipts; no unchanged exploratory suite is repeated. Final
boundary/dependency/docs/changelog and exact-path packaging outcome is recorded
externally; the broader CICS prerequisite remains failed, not waived.

### Root composition declaration for last-direct-child selection

Root integrates worker ad6358eba0ed51e16e5fd90f45c7ced9c817df08 onto
091ab017e436b1be2b6ed0290cae9a5acd8b60f0 with the same seventeen-path
allowlist. Preserve both provider last/null test modules, server logical/null/L
modules, Proposed ADR-0036/0037/0038/0039 and earlier source declarations.
The three reviewed production files are identical to the worker seal. Source
pins, active-command inventory, private cursor fields and public contracts stay
unchanged. Normal documentation generation resolves generated packaging.

Before proof, root updates only the existing declared fences test and ADR-0039
null-slot integration note. The worker's literal L-minus malformed assertion
belongs to its pre-null base and must not be relabeled as composed behavior.
Instead the composed literal CHILD *-L- request must select C3S/A1 with real
root parentage, cancel old hold, and conflict with different raw bytes under
the same canonical request identity. Null slots cannot admit a second active
command or weaken the finite L shape. All existing negative/public/signed
recovery controls remain required. This is a test composition correction, not
a new production recipe or a rewrite of worker evidence.

Root selects all fourteen L provider and six signed test identities, prior
logical feedback and the signed null control, then strict affected all-target/
all-feature lint and relevant mandatory policy/docs/changelog/seal checks.
Worker's passing unaffected recovery/secondary and unchanged source/API policy
inputs retain their original receipt identities. No PostgreSQL L parity, global
CICS-source retry, licensed work, HUMAN/official/participant or parent completion
is inferred. Cargo clean ends each intended-checkout sequence.

Root outcome: all fourteen L provider and six signed test identities pass on
the composed candidate, including actual SQLite cold children and CHKP/XRST.
The revised null-slot control returns literal C3S/A1 with current 5/root parentage
2/no hold; different raw SSA bytes under the same request key conflict without
mutation. A second active command still rejects before publication. Both prior
signed logical backend cases and the exact signed null case execute and pass.
Strict affected all-target/all-feature Clippy and fmt pass; Cargo clean removes
6.5 GiB. The three production file hashes match the reviewed worker seal, while
the revised fence input is separately bound in root runtime-inputs.json. Final
relevant policy, normal docs and seventeen-path seal/check are packaging gates;
external root receipts record their actual disposition, not CI or full-minor
acceptance. Original worker/base-only null failures keep their identities.

## IMS-1401.ssa-position-command-sources — declared source-only leaf

Clean entry `6e3e24d014cfb4a58c6b79efff88e9b999bfc9cc`, target 0.14.0.
The manager authorizes registration of exactly four previously audited IMS 15.6
F/U/V/W archive topics, not runtime implementation or acceptance. Register scope
`ims-ssa-position-commands`, baseline
`ibm-ims-15.6-ssa-position-commands-2026-09-11`, snapshot 2026-09-11,
product SSEPH2_15.6.0 and TOC
`aaa12586b41e9994921bfddce588b186dc5bdda8ab253db054ae1e5014d6f618`.
The existing L/success/failure scope stays separate. Preserve every existing pin
and registry entry; semantic authority remains false and coverage credit zero.

Exact five-path allowlist, declared before source edits:

- `conformance/0.14/manifests/ims-ssa-position-command-topics.json` (new)
- `conformance/0.14/manifests/index.json`
- `docs/delivery/subsystems/ims/programming-status.md`
- `changes/unreleased/ims-ssa-position-command-sources-20261003.toml` (new)
- `docs/generated/documentation-manifest.json` (normal generator only)

Proof: first check retained topic_path against expected SHA/bytes, then verify
matching archive fallback, immutable metadata/run/TOC binding and the existing
topic-set digest definition. Import only these four bodies and their TOC into
an external scoped cache through the existing reader; actual search/read must
fully read all four bounded topics. Publication text stays outside Git.
Required local checks are existing registry/source-reader consumers, selected
schemas/spec/catalog/assurance, normal docs generation/check, changelog,
formatting, offline dependency policy and coverage inventory. Exact-path feature
seal, full generated completion message, commit and committed --check follow
only after these gates pass. Clean this checkout's target after each actual
build/test/lint/generator sequence, failures included; receipts remain external
under worker-receipts/v014-completion-20261002/ssa-position-command-sources/.

No runtime/source-reader/checker rewrite, schema, dependency, semantic rule,
denominator, HUMAN/licensed/certification credit, raw CALL/TM/participant
extension or full-minor completion is authorized. ADR-0039 belongs to L;
any later private U/V design requires separately reviewed ADR-0040.

Source outcome: the four retained topic paths are absent; matching immutable
archive fallback verifies all four metadata hashes, the run/product/TOC binding,
exact TOC membership and body SHA/byte counts. No selected source is missing or
mismatched. F/U/V/W are respectively 7,545 / 5,269 / 3,557 / 2,614 bytes,
18,985 total. Topic-set digest is
`f99728026ed7f14fcc8e104678bc55939581af2defee68385bc3bf35b170e8b9`;
manifest-byte digest is
`6890befdceb5c0209670b26bd665ba10d5f070cb0a8374818431ceea2081f544`.
Archive metadata has no recorded HTTP Last-Modified value; the manifest states
that absence instead of treating the body's Last Updated label as that header.
All eleven preexisting manifest pins and registry rows remain identical.

Actual scoped import publishes four topics and one TOC; status verifies 4/4 and
1/1. Four command searches and four full reader calls pass, reading all 187
plain-text lines. The existing source-reader suite passes 23 cases. Selected
schemas/spec (134 Python cases), IMS catalog/assurance, coverage inventory,
formatting, offline deny advisories/bans/licenses/sources, normal docs generation
and docs/changelog checks pass. Cargo clean ends the sequence. Final status
packaging regenerates the documentation manifest and verifies the exact
five-path feature seal and committed --check; its receipts remain external.
No runtime suite, global architecture retry, campaign, licensed or HUMAN
acceptance was run or inferred. The unchanged audit remains historical source
review, not current-candidate runtime evidence. Only this source registration
leaf is ready for its content seal; IMS-1401 and the full minor remain incomplete.

### Root composition of positioning command source pins

Root integrates source worker 1c4b71fd53d0f12c5b8e878c0023e0ff6737c418 onto
680c571d4e7194b4b5fe03fec4be11e1266315e3 with the same five-path allowlist.
The manager independently verifies retained absence/archive SHA and byte counts,
the existing topic-set digest, and actual registered search/full read of all
four topics (187 lines) from the clean source worker checkout. Manifest and
registry bytes are unchanged during composition; all prior pins stay identical.
These are source-review identities, never execution or HUMAN acceptance.
Root keeps both logical/L status additions, regenerates the manifest normally,
and selects the source reader, coverage registry and docs/changelog/policy/seal
checks for this metadata-only composition. No Rust runtime repeat or CICS cache
retry is justified by these unchanged semantic inputs. F/U/V/W behavior and the
proposed private positioning authority remain unfinished; full v0.14 stays open.
