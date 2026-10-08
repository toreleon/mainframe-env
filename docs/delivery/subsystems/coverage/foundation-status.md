# Coverage and conformance — Coverage authority progress

Subsystem: **coverage**
Phase: **foundation**

Status: **Implementation candidate; acceptance pending**

## Implemented boundaries

The shared Conformance IR separates official catalog rows, obligations,
executable bindings, verdicts, and licensed-credit policy. Nine pinned IBM
baselines define the 1,506 mandatory catalog identities. Publication bodies
remain in an external cache; the repository retains bounded locators, topic
manifests, schemas, and fixture specifications.

| Work package | Owned implementation |
|---|---|
| CV-201 | Pinned topic manifests and normalized official catalogs |
| CV-202 | Independent six-gate coverage rows and append-only verdict storage |
| CV-203 | Generated semantic identities and explicit handler registry |
| CV-204 | Signed package generations, reference validation, atomic selection, and rollback |
| CV-205 | Catalog-driven Db2 and authorization routes |
| CV-206 | Typed batch controllers decoded from selected signed packages |
| CV-207 | Provider-owned compatibility source libraries for CICS, Db2, and MQ |
| CV-208 | Generated utility/system-service and official/custom route registration |
| CV-209 | Cross-subsystem validation and non-destructive migration boundaries |

Regression requirements include non-destructive Db2 install, upgrade, and
rollback; primary-key integrity; HMAC-verified package publication; exact
controller artifact binding; durable restart; raw Db2 bytes and defaults;
compiled JSON Schema validation; signed program identities; allocation-safe
package preflight; and bounded package/controller admission. Specifications,
fixtures, and executable tests remain in the owning subsystem folders.

## Validate the current candidate

```bash
cargo xtask spec --check
cargo xtask coverage --check
cargo xtask semantic-identities --check
cargo xtask application-packages --check
cargo xtask abi-libraries --check
cargo xtask dehardcoding --check
```

Run applicable backend, CardDemo, and licensed checks with their required inputs.
Historical release identities, execution receipts, and acceptance tables are
removed. Keep fresh output outside Git and describe its candidate and scope in
the change report. A local fixture, catalog, or source review cannot establish
licensed IBM equivalence.

## Source and compatibility boundaries

All source authority comes from the pinned IBM topic manifests under
`conformance/subsystems/coverage/manifests/`. Raw topic bytes and retrieval
observations remain outside the repository. The topic reader distinguishes
republished or unexplained changes from an older cached revision; an ordinary
review never silently re-pins a topic. See the
[publication source investigation](../../../research/publication-source-probe.md)
and [cache runbook](../../../runbooks/IBM-DOCS-CACHE.md).

The original z/OSMF heading-level denominator remains frozen. Endpoint
normalization is owned by the z/OSMF subsystem and creates its own source-bound
projection; it does not rewrite the shared catalog's identities.

## CV-209.host-contract-lint

Status: **Complete (bounded mechanical slice only)**. This non-semantic slice owns mechanical lint repairs in
the existing host-contract package and its tests. It changes no official catalog
row, public ABI, behavior, source pin or durable schema. The consumed base is
`e3830278efc8de147f14b0e98da3d759250be782`. The isolated CLI worker must preserve
negative cases and strict lint policy, reporting any repair that needs a contract
change instead of suppressing the diagnostic. Acceptance is the strict affected
package Clippy run, package regressions, formatting, module/documentation/
changelog checks, dependency policy and diff review. It grants no conformance or
licensed credit and does not complete the whole-workspace lint backlog.

## Current public completion scope

Status: **In progress** on the consumed main candidate
`f42b96642a8662a96d89e32599615aa95f00f222`. Continuation follows subsystem
dependency order. Licensed and private-only implementation and the three unready
CICS rows are excluded from task scope, retaining their identities and pending
credit. Source cache availability does not establish product acceptance.

Six isolated English CLI audits inspected existing owners. The initial checks
found stale semantic-identity and ABI bindings; their bounded repairs are sealed
below. All six checks now pass on the integrated candidate. These checks do not
replace executable product regressions. Remaining audit findings must be
reproduced by focused negatives before repair.

| Slice | Parent, boundary and owners | Dependencies and acceptance |
|---|---|---|
| `CV-203.generated-binding` | CV-203; regenerate existing semantic identity artifacts through their current owner, without catalog/handler admission changes | Current normative inputs; owner check, deterministic regeneration, required metadata gates and reviewed diff |
| `CV-207.generated-binding` | CV-207; regenerate existing ABI inventory through its current owner, preserving source bytes and library identity | Current provider-owned sources; owner check, deterministic regeneration, required metadata gates and reviewed diff |
| `CV-202.snapshot-continuity` | CV-202; existing coverage contract/store and focused tests; preserve catalog row membership, unit/applicability and retained evidence across generations | Existing six-gate and immutable snapshot contracts; focused omission/drift/conflict negatives, unchanged state on refusal, positive monotonic/retry tests, coverage/spec and required gates |
| `CV-205.catalog-generation` | CV-205; existing Db2 signed-catalog install/rollback and tests; no SQL language change | Current package/store contracts; reproduce identical-seed-at-capacity and retained-generation replacement defects; insert/conflict/refusal atomicity, reopen and rollback regressions, focused provider tests and required gates |

The manager owns shared contract decisions, status, generated output, sealing,
worktree integration and PRs. A passing bounded slice does not complete Foundation
or establish a licensed differential. CardDemo, affected backend and integrated
exit requirements remain pending until executed on their consumed candidate.

## Additional declared Foundation repairs

| Slice | Parent, exact owners and boundary | Acceptance and compatibility |
|---|---|---|
| `CV-207.materialization-bounds` | CV-207; existing source ABI materializer and tests; resource preflight before copies | Aggregate/count/per-file boundary failures with no partial materialization; exact-boundary success, identity/license/duplicate regressions; unchanged ABI bytes and source contract, focused source tests and required gates |
| `CV-204.generation-selection` | CV-204; existing application installer generation commit, recovery and tests | Old-ready retry cannot implicitly roll back; stale staged commit and malformed retained topology fail closed; explicit rollback and valid staged/null/selected-ready round trips; focused installer and required gates. No signature/identity domain changes |
| `CV-209.test-floor` | CV-209; existing CI assurance recorder/tests and workspace-test Jenkins selection | Optional minimum executed-test threshold; actual passes count, ignored/malformed/insufficient output refuses admission; selected workspace receipt requires 260; existing candidate binding and focused receipts remain intact |
| `CV-209.host-effect-layout` | CV-209; shared HostRequest/HostResult MQI payload representation and mechanical consumers | Box only outer MQI payloads, preserve exclusive ownership and nested DTOs; independent canonical byte/digest and retained replay regressions, affected consumers compile, strict host Clippy, size evidence, ADR and required gates |

The host layout prerequisite intentionally changes Rust enum construction and
nested patterns before public release. It does not change the explicit canonical
wire/domain or durable encoding. Consumers use `Box::new` and owned unboxing;
allocation costs remain explicit and no total-heap-bound claim is made. This
parent-owned shared representation decision is separate from the mechanical lint
slice, which depends on it. No new MQI operation, public admission or private
participant implementation is authorized by this prerequisite.

The remaining audit findings include package identity framing, bounded package
codec/preflight, signed section prevalidation and publication fencing, serialized
schema bindings, complete production scanning, common-program control ownership,
controller root validation and executable journey/client/exit-gate closure.
They remain pending bounded assignments and compatibility decisions; static
findings are not reproduced defects or completion receipts. Retained signatures
and identity domains must not be silently rehashed.

### CV-203.generated-binding acceptance

Status: **Complete (generated binding only)**. The failing owner check reproduced
a stale catalog binding. Normal regeneration updates only the catalog input
digest; the generated identity-set digest and all 1,506 descriptors are unchanged.
The owner check, deterministic second generation, current spec/coverage checks,
formatting, dependency policy, documentation/changelog/subsystem and diff checks
pass. No handler or runtime gate receives credit from this repair.

### CV-207.generated-binding acceptance

Status: **Complete (generated inventory only)**. The failing inventory check
reproduced stale DFHAID byte length/hash and the resulting CICS library digest.
Normal owner regeneration updates those three fields to the unchanged provider
source bytes. The owner check and deterministic second generation, formatting,
dependency policy, module/documentation/changelog/subsystem and diff checks pass.
This repair grants no ABI-equivalence, runtime or licensed credit.

### CV-202.snapshot-continuity integration

Status: **Complete (snapshot continuity only)**. New snapshot
generations preserve row/unit/applicability descriptors and append-only retained
evidence histories. Missing failure observations and stale sequence appends
refuse before state changes; historical immutable retries remain idempotent.
The tests-only candidate reproduced six failures with two passing controls. The
reviewed repair passes all 28 coverage-package tests and strict package Clippy.
Public methods, error vocabulary and persisted representation are unchanged.
First-snapshot official membership still comes from the catalog/compiler owner.
Current integrated spec/coverage, strict lint, formatting and dependency checks
pass. The module extraction below resolves the facade gate without raising a
ceiling. Required documentation/changelog/subsystem checks and the completion seal pass;
this closes no Foundation exit or licensed gate.

### CV-209.store-contract-lint

Parent: CV-209. Status: **Complete (bounded mechanical slice only)**. This mechanical prerequisite owns only
four nested-condition diagnostics in the existing Memory/SQLite root-terminal
guards and provider-mutation dependency check, plus nine private test snapshot
tuple aliases revealed after those dependency errors were removed. Evaluation order, awaits, errors,
CAS/fencing and durable bytes remain unchanged. No lint suppression or new
storage authority is permitted. Acceptance is strict affected-store Clippy,
existing focused root-terminal regressions, module/format/required repository
gates and diff review after the host lint/layout prerequisites are integrated.

### Reviewed public repair integration

The manager integrated only reviewed owned files and isolated fragments from the
worker checkouts. Source-file hashes and producing-base identities remain in
external handoffs; those results are not relabeled as integrated receipts.

| Slice | Executed focused result | Accepted scope and remaining parent requirement |
|---|---|---|
| `CV-205.catalog-generation` | Original failures reproduced on Memory/file-backed SQLite; 12 new and six existing regressions pass | Integrated dependency-inclusive Clippy and 12 catalog regressions pass; metadata gates and seal pass. Adapter reopen is not process-crash proof |
| `CV-204.generation-selection` | Seven original failures/two positive controls; all 24 package tests and owned-package Clippy pass | Integrated dependency-inclusive Clippy and all 24 installer tests pass; metadata gates and seal pass; signatures/identity domains unchanged |
| `CV-207.materialization-bounds` | Both original aggregate-bound failures reproduced; 10 ABI and all 16 source-package tests, strict Clippy pass | Integrated ABI owner and strict Clippy pass; metadata gates and seal pass; source bytes/order/digests unchanged |
| `CV-209.test-floor` | Original 259-pass admission failure reproduced; all 35 focused recorder tests pass | All 35 integrated recorder tests pass; metadata gates and seal pass. Workspace recorder selects a 260 actual-pass minimum; certification wiring remains pending |

Bounded slice acceptance records below identify completed repairs. Whole-phase
acceptance remains pending. Installer recovery
preserves an explicit null selection with retained Ready generations, rejects
malformed topology before verifier calls, and separates ready commit retries
from explicit rollback. ABI materialization bounds all member/library byte and
file counts before source-buffer copies; bounded path metadata can allocate.

Host layout validation reproduced an invalidated owned-source fixture binding.
The manager extends `CV-209.host-effect-layout` to refresh only the two existing
MQ local fixture digests in the shared spec from the changed transcript adapter.
The existing independent binder validates that input source; expected fixture
bytes, official rows/obligations, IBM publication pins and gates are unchanged.
This source-identity update cannot reuse old candidate verdicts or oracle credit.

The integrated module gate found that the new continuity checks exceed the
coverage facade's frozen non-growing ceiling. The manager extends this slice
to move the existing CoverageStore implementation into its private `store`
module without changing its public methods, fields, evaluation order or tests.
The reviewed facade inventory ceiling is lowered from 533 to 440 production
lines; all other exemptions remain unchanged. Focused tests and strict lint
must be rerun.

The current integrated candidate passes all six Foundation owner checks,
formatting, frozen dependency policy, strict Clippy for the five affected
coverage/source/application/Db2/store packages with dependencies, 355 host
tests, 24 installer tests, 12 catalog-generation tests and three local MQ
source-binding tests. Store regressions pass 124 root-terminal tests and
97 unit tests; three PostgreSQL unit tests remain ignored and receive no
acceptance credit. No workspace test-floor, PostgreSQL parity, process-crash,
CardDemo or live Zowe exit result follows from these scoped runs.

### Foundation exit remains pending

The broad `architecture-fast --check` stops at the existing CICS sources-a
freshness guard because retained topic
`SSNAQ8_11.1.0/reference-api/r_dump.html` is unavailable in the configured cache.
All preceding execution-route, participant, canonical-effect, persistence,
retention and CICS descriptor/source-map guards passed. This is an unavailable
source gate, not a pass or a reason to refresh or re-pin sources. Scoped module
and shared-contract repairs can be reviewed independently; full Foundation
acceptance remains pending with this gate and the other exit requirements.

### CV-209.host-effect-layout acceptance

Status: **Complete (bounded slice only)**. Scoped compatibility acceptance passed: outer MQI boxing and migrated consumers preserve canonical vectors and retained replay codecs. Rust constructor/pattern migration is explicit in ADR-0050. The two changed local source bindings pass their independent binder; no IBM pin or expected bytes changed.
Formatting, frozen dependency policy, module, documentation/changelog/subsystem
and diff checks pass for the reviewed integrated scope. The unavailable CICS
source freshness gate and full Foundation exit remain pending.

### CV-209.host-contract-lint acceptance

Status: **Complete (bounded slice only)**. Mechanical implementation is in the preceding focused repair commit. With the separately accepted host-layout prerequisite, integrated strict host Clippy and all 355 host tests pass. This closes only the host-package lint slice.
Formatting, frozen dependency policy, module, documentation/changelog/subsystem
and diff checks pass for the reviewed integrated scope. The unavailable CICS
source freshness gate and full Foundation exit remain pending.

### CV-209.store-contract-lint acceptance

Status: **Complete (bounded slice only)**. Integrated strict store/dependency Clippy, 124 root-terminal tests and 97 store unit tests pass. Three PostgreSQL unit tests were ignored and are not credited; the mechanical aliases and conditions do not alter durable contracts.
Formatting, frozen dependency policy, module, documentation/changelog/subsystem
and diff checks pass for the reviewed integrated scope. The unavailable CICS
source freshness gate and full Foundation exit remain pending.

### CV-202.snapshot-continuity acceptance

Status: **Complete (bounded slice only)**. All 28 coverage tests, integrated strict Clippy, spec/coverage and module gates pass. The private store-module extraction preserves the public API and lowers the facade ceiling from 533 to 440; all other ceilings remain unchanged.
Formatting, frozen dependency policy, module, documentation/changelog/subsystem
and diff checks pass for the reviewed integrated scope. The unavailable CICS
source freshness gate and full Foundation exit remain pending.

### CV-204.generation-selection acceptance

Status: **Complete (bounded slice only)**. All 24 integrated application-package tests and dependency-inclusive strict Clippy pass. Ready retries, stale staged refusal and retained topology/null selection are covered; identity framing, bounds and publication fencing remain separate pending work.
Formatting, frozen dependency policy, module, documentation/changelog/subsystem
and diff checks pass for the reviewed integrated scope. The unavailable CICS
source freshness gate and full Foundation exit remain pending.

### CV-205.catalog-generation acceptance

Status: **Complete (bounded slice only)**. The 12 new Memory/file-backed SQLite regressions and six existing catalog regressions pass, with integrated strict Clippy. Identical seeds at capacity and conflicting retained identities are covered; same-process adapter reopen is not process-crash proof.
Formatting, frozen dependency policy, module, documentation/changelog/subsystem
and diff checks pass for the reviewed integrated scope. The unavailable CICS
source freshness gate and full Foundation exit remain pending.

### CV-207.materialization-bounds acceptance

Status: **Complete (bounded slice only)**. All 16 source-package tests, including 10 ABI tests, strict Clippy and the current ABI inventory check pass. Aggregate/file/count preflight precedes source copies and hashing; bounded path metadata can still allocate. Source bytes and identities remain unchanged.
Formatting, frozen dependency policy, module, documentation/changelog/subsystem
and diff checks pass for the reviewed integrated scope. The unavailable CICS
source freshness gate and full Foundation exit remain pending.

### CV-209.test-floor acceptance

Status: **Complete (bounded slice only)**. All 35 integrated recorder tests pass. The workspace selector now requires 260 actual passed tests from complete successful summaries; this is wiring acceptance, not an executed workspace-floor or licensed certification pass.
Formatting, frozen dependency policy, module, documentation/changelog/subsystem
and diff checks pass for the reviewed integrated scope. The unavailable CICS
source freshness gate and full Foundation exit remain pending.

## Next executable work

Continue the recorded public Foundation repairs in dependency order: reproduce
and decide package identity framing compatibility, then close bounded package
codecs and signed publication preflight/fencing. The existing audits also identify
schema mappings, production scanning, controller root validation and journey/client
closure. All remain pending; no new discovery round, licensed campaign, private
implementation or source refresh is authorized by these scoped seals.

## Next declared public repair slices

Consumed candidate: `d0cce85e`; the preceding bounded repairs are sealed, while
Foundation remains in progress. No licensed/private-only implementation or
unready CICS command is included.

| Slice | Parent and exact owners | Focused acceptance and compatibility |
|---|---|---|
| `CV-204.identity-framing` | CV-204; existing package identity/trust owner. First assignment is an independent reproducer and compatibility proposal only | Reproduce distinct valid IMS graphs sharing the existing identity; inventory every variable collection. Manager must approve an explicit identity-domain/writer/finite-reader migration before production edits; preserve retained v1/legacy selection and signatures without relabeling |
| `CV-204.package-bounds` | CV-204; existing application package preflight and installer state codec, private bounded codec module and tests | Reject counts/text/checked aggregate excess before allocating validators/hash/owned decoding; bounded export stops before full clone/encoding. Preserve admitted wire bytes, finite retained forms, topology and verifier behavior; focused boundary/refusal/no-mutation tests |
| `CV-202.serialized-schema-bindings` | CV-202; existing coverage row/evidence/ledger schemas, typed projection and xtask schema mapping/tests | Reviewed Draft 2020-12 validation rejects invalid oracle-on-pass, gate keys, incomplete evidence and malformed ledger items; valid historical forms and differential failure stay accepted. Typed owner validates cross-reference/denominator relationships; no new ledger |
| `CV-208.production-scanner` | CV-208; existing xtask dehardcoding guards and shared Rust item scanner/tests | Test-only items are excluded without dropping later production; markers in comments/strings cannot truncate a scan. Independently constructed negative fixture must fail; current zero-H1 result is reevaluated, with no product semantic change |
| `CV-206.controller-root-validation` | CV-206; installed IMS controller admission and loader/tests | Duplicate roots, including identical duplicates, refuse before any participant effect. Root-key width beyond root-record width fails admission and preserves selection; successful/rejected-load/commit regressions remain. No new IMS operation or semantics |
| `CV-209.journey-closure` | CV-209; existing CardDemo manifest and conformance runner/receipt/tests | Bind existing CD.J01–CD.J20 to actual route observations and derive counts from exact closure; omitted/duplicate/unknown journey or missing mandatory observations fail. Focused harness tests first; full workload remains a separate current-candidate gate, without new evidence family |

Workers use isolated checkouts at this candidate and English prompts. The first
Cargo grants are package-bounds and controller-root-validation, at most two
build sequences at once. Other workers prepare independent regressions and
report the required command; they do not execute Cargo until the manager grants
it. Each completed sequence saves output externally and cleans its own target.
The manager reviews compatibility, integrates exact owned diffs, runs affected
gates, seals each slice separately and opens a scoped PR. Parent acceptance and
all unavailable source/backend/client gates remain pending.

### Supplemental cache disposition

The user reports no additional retained cache and explicitly directs skipping
checks that need the missing supplemental bodies. Their result remains
`skipped/unavailable`, with zero source or behavioral credit; no refresh, re-pin
or fabricated pass is permitted. Continue independent public implementation
and applicable available-input gates. Do not retry the unchanged missing-cache
checks or request the same cache again.

The manager extends `CV-209.journey-closure` to read and bind the existing
`carddemo-gap-matrix.json` issue denominator (CD-001 through CD-026). No new
issue ledger or changed expectation is allowed. Unobserved required behaviors,
including process restart rather than same-process reopen, must remain pending
and cause full closure to refuse until real observations are implemented.

### Serialized coverage compatibility decision

The manager inspected only the structural layout of the former repository-owned
`coverage-ledger@1` producer output in Git object
`a53b9dbfa8c683f43d17e168b226338dbd499518`; no historical result is restored or
credited. Its finite baseline summary is `id`, `catalog_sha256`, `mandatory_rows`,
`complete_rows`, and `gates`, with all six gate names mapping to `numerator` and
`denominator`. Evidence records are embedded records, not reference strings;
row evidence references retain their existing separate form. The schema repair
must preserve this valid shape and empty evidence arrays/explicit pending empty
histories, while refusing impossible/unknown fields. Current catalog identity,
row/evidence closure and derived counts are verified by the existing typed
owners; schema shape alone cannot certify those relationships. The legacy
wire identities remain unchanged and no ledger/evidence family is added.

### Batch lint prerequisite

Declare `CV-209.batch-contract-lint` under CV-209. Exact production owners are
`mainframe-env-batch` service, `service/run_retirement.rs` and
`service/running_step.rs`, plus private tests only if strict lint exposes them.
The controller-root worker found 21 existing package-local diagnostics on these
unchanged files. This mechanical prerequisite preserves borrow/evaluation order,
awaits, errors, ownership, retirement/step fencing and durable bytes; no lint
suppression, private behavior or expanded admission is allowed. Focused existing
regressions, strict package Clippy and required gates precede its separate seal.
The root-validation slice remains pending until this prerequisite passes.

The manager selects the explicit framing/admission compatibility design in
[ADR-0051](../../../decisions/0051-package-identity-framing.md). The independent
legacy ambiguity run reproduces four failures with one valid control. Worker
implementation must preserve the declared finite writer/reader, trusted recovery
and exact legacy-retry boundary. The unchanged installer-state shape is retained;
no silent legacy hash/signature migration or new snapshot-authentication guarantee
is claimed. Standards-envelope adoption remains a separate pending slice.

### Publication preparation scope

Declare `CV-204.publication-fencing` under CV-204. Existing owners are the
ProductServer application publication/install/rollback state machine and its
batch-controller, Db2 and IMS application adapters; selected controller admission
in BatchService; existing provider-state/artifact ports; and focused server/batch
tests. Preparation consumes the separately sealed controller and package-bounds
repairs. No new coordinator, provider-state namespace or signature format is
authorized. Prevalidate every publishable section before provider mutation and
fence public admission with the existing complete publication identity. Preserve
finite retained readers and explicit install/rollback recovery states.

First prepare independently expected actual JES/controller route refusals for
missing or substituted executable artifacts, invalid/missing SQL catalog and
partial install/rollback windows. Focused Memory tests supplement fresh-process
file-SQLite recovery; backend parity remains pending until executed. No Cargo or
production repair is granted during preparation. The manager must review the
exact regression/adapter scope before granting its red/implementation sequence.

### Identity producer and schema binding scope

Extend `CV-204.identity-framing` to the current signed package producers in
`mainframe-env-conformance/src/carddemo.rs`, `carddemo/ims_packages.rs` and
`carddemo/ims_process_tests.rs`; the ProductServer test producer/resigner helpers
in `product.rs`; the existing `application-package-v2.schema.json`; and only
affected application-package guard bindings in `xtask/src/main.rs`.
Align that existing schema with the actual `base`, `generation`, `sections` and
`signature` DTO, with exact finite @2/@3 discrimination in `sections`; it must
not retain an invented top-level projection or add a second discriminator.
Preserve legacy-only migration fixtures and independent frozen identity vectors.
Current producers use @3 and finite identity dispatch. Production HMAC/key policy
is unchanged. Publication behavior, installer/publication schema repairs and
standards-envelope adoption remain separate slices. Shared contract inventories,
architecture docs and generated metadata remain manager-owned.

### Catalog locator membership scope

Declare `CV-201.catalog-locator-membership` under CV-201. Owners are the existing
coverage/catalog/compiler check in `xtask/src/main.rs`, `conformance_catalog.rs`
and `topic_manifests.rs`, with focused private tests. Join each official catalog
locator to its indexed baseline manifest or existing supporting-source receipt.
An unpinned or wrong-product locator must refuse even if its modified catalog
digest is internally consistent. Preserve embedded/link locators and the five
explicit zero-credit roadmap-normalization dispositions; no catalog row, pin,
denominator or publication body is rewritten or fetched. Source-body verification
remains separate. Prepare independent mutation tests first; Cargo grant is
manager-controlled and completion requires affected offline metadata checks.

### Execution lint prerequisite

Declare `CV-209.execution-contract-lint` under CV-209. The scanner's strict
dependency-inclusive build exposed 20 existing interpreter and four existing
compiler diagnostics. Exact owners are the reported compiler CICS resolution
helpers and interpreter coordinator, machine/EIB/decimal commit, typed CICS/MQ
and storage64 modules, with affected private tests and one isolated fragment.
Mechanical repairs preserve evaluation/error order, effect identities and
continuation/checkpoint bytes; no lint suppression or new private behavior is
allowed. The unused private arithmetic wrapper may be removed after confirming
no callers. Boxing the private `PendingKind::MqMqi` target DTO may reduce its
inline size; it must preserve canonical recovery bytes and does not promise less
total heap use. Focused continuation/recovery/arithmetic controls and strict
affected-package Clippy precede its separate seal. Tooling slices do not claim
dependency lint closure until this prerequisite and any further reported owners
pass. Licensed/private-only implementation and unready CICS rows remain excluded.

### CV-204.package-bounds acceptance

Status: **Complete (bounded package admission and codec only)**. The independently reproduced allocation/codec refusals are repaired. All 33 focused package tests and dependency-inclusive strict Clippy pass in the producing checkout; integrated application-package/spec/coverage, module, formatting and frozen dependency gates pass. Streaming preflight checks counts, text and checked footprint before typed decoding; capped export borrows retained packages and stops before full encoding. Duplicate JSON members refuse. Serde scratch and bounded metadata sets still allocate; this is not a process-wide heap quota. Identity framing and publication fencing remain pending.
Documentation/changelog/subsystem and diff checks pass for this reviewed scope.
Missing supplemental source checks are `skipped/unavailable` by user direction,
with zero credit. Foundation acceptance and other public exit gates remain pending.

### CV-209.batch-contract-lint acceptance

Status: **Complete (mechanical batch lint only)**. The 21 existing package-local diagnostics are repaired without suppression or changed assertions. Integrated dependency-inclusive strict batch Clippy passes, with 10 running-step and 56 contained-run tests passing. Borrow/evaluation order, awaits, errors and fencing remain unchanged; module ceilings are unchanged.
Documentation/changelog/subsystem and diff checks pass for this reviewed scope.
Missing supplemental source checks are `skipped/unavailable` by user direction,
with zero credit. Foundation acceptance and other public exit gates remain pending.

### CV-206.controller-root-validation acceptance

Status: **Complete (controller admission and root validation only)**. Integrated controller tests (11) and IMS completion tests (8) pass, with strict batch Clippy after the separate lint prerequisite. Duplicate root keys, including identical duplicates, refuse before participant effects. Impossible root-key widths refuse admission and leave selection intact. Valid retained forms remain unchanged; historical impossible-width retained generations also refuse reconstruction through the existing install path. Same-process reopen supplies no process-crash or backend-parity credit.
Documentation/changelog/subsystem and diff checks pass for this reviewed scope.
Missing supplemental source checks are `skipped/unavailable` by user direction,
with zero credit. Foundation acceptance and other public exit gates remain pending.

### CV-209.execution-contract-lint acceptance

Status: **Complete (mechanical compiler/interpreter slice only)**. Dependency-inclusive
strict Clippy for both owning packages and 157 focused test results pass. The three
newly exposed test-only diagnostics were repaired without changing assertions.
The unused private arithmetic wrapper has no callers and was removed. Private
MQI target boxing preserves DTO ownership; pending effects/bound MQI frames still
refuse checkpoint serialization, and codecs remain unchanged. Integrated compile,
module, formatting, frozen dependency, documentation/changelog/subsystem and diff
checks pass. The broader xtask dependency lint now exposes unchanged MQ and CICS
diagnostics; those owners remain separate pending work, with no suppression or
full-build acceptance claim. Foundation and public exit acceptance remain pending.

### Provider lint prerequisites

Declare two separate CV-209 mechanical slices: `CV-209.mq-contract-lint` owns
the reported existing diagnostics in `mainframe-env-mq` and its direct private
consumers/tests; `CV-209.cics-contract-lint` owns the reported existing diagnostics
in `mainframe-env-cics` and its direct private consumers/tests. The integrated
strict xtask run reports 46 MQ and 41 CICS diagnostics after consuming the sealed
execution lint repair. Public/provider APIs, SAF order, controls/fencing, effect
identities, canonical durable/replay bytes and finite readers must remain intact.
No lint suppression, changed expectation, new private behavior or unready command
implementation is allowed. Private allocation/layout or helper-argument grouping
must remain within existing owners; any public constructor change requires a
separate manager decision. Test-only helpers may be scoped to tests only after
proving all callers are test-only; never hide a production reader or migration.
Focused affected behavior/serialization regressions and strict package lint
precede separate seals. Other dependency owners remain pending if later exposed.

### Identity writer resource policy

The framing slice may add a bounded `package_generation_identity_with_limits`
entry point using the existing `PackageLimits` for hosts with explicitly selected
budgets above the default writer limits. The default entry points stay bounded;
the frozen @2 byte grammar and installer recovery's caller-selected limits stay
unchanged. Test both finite domains with a valid package above the defaults and
an explicit fitting budget, including refusal under an insufficient budget.
This is an additive owned API, not an unbounded signer or semantic admission.
The six producer/schema bindings declared above are approved for implementation.

### Provider lint review disposition

The CICS candidate passes 148 focused tests but retains four public signature
diagnostics and one unreachable production receipt-pruning operation. The MQ
candidate retains 30 production responsibility diagnostics and is not compiled.
Both slices remain pending; no test-only scoping of production migrations,
readers, provenance or retention responsibility is authorized. The manager will
review public parameter grouping and existing operational reachability before
implementation. These lint failures do not become passed tooling gates.

### Retained package-state schema preparation

Declare `CV-202.package-state-schema` under CV-202, consuming the separately
accepted identity framing implementation. The existing installer-state and
publication-state schemas must describe the actual serialized DTOs, including
finite retained packages and historical absent IMS publication state. Prepare
native offline Draft 2020-12 validation and typed DTO projection regressions
through existing owners, with positive retained forms and malformed/mismatched
negative cases. Package schema/framing remains owned by its existing slice.
Preparation adds no runtime admission, alternate snapshot guarantee or public
DTO; tests must distinguish structural schema checks from current trust,
reference, topology and publication validation. Dependencies and private-owner
test bindings require manager review before compilation or production edits.

### CICS operation context decision

[ADR-0052](../../../decisions/0052-cics-operation-contexts.md) approves borrowed
BTS replay and partition-input grouping in `CV-209.cics-contract-lint`. Extend
that slice to the inventoried direct server test callers and the exact facade
exports for the existing receipt-pruning operation. Preserve every expression,
authority check, durable codec and retention algorithm; no automatic cleanup or
private command activation is authorized. Strict CICS lint, affected owner tests
and server caller compilation must pass before acceptance. The accepted design
does not complete this slice or change any official coverage gate.

The receipt export is a manual raw-store maintenance primitive. Its trusted
embedding must establish explicit authority, conservative post-persist age,
archive policy, complete protected effect/checkpoint references and coordination
with concurrent replay/recovery under the existing
[retention contract](../../../contracts/RETENTION-LIFECYCLE-V1.md). A receipt's
deadline is not a resolution tick or proof of safe reclamation. Preserve the
existing conditional per-row deletions and partial-error limitation, documenting
these obligations on the public function. This export supplies no complete
retention-framework or licensed gate credit.

### Tooling candidate integration

The reviewed serialized coverage-schema, production-scanner and catalog-locator
candidates pass 30, four and eight focused Rust tests respectively on their
joint manager input. Native offline schemas, coverage, spec, module, formatting,
frozen dependency and diff checks pass. Strict lint of the tooling owner alone
passes with `--no-deps`; dependency-inclusive lint remains failed on the separately
recorded MQ/CICS prerequisites. The bounded-owner seals below record their
acceptance and do not claim full dependency lint or Foundation acceptance. Locator
membership preserves all 1,506 rows and existing zero-credit dispositions.

### MQ responsibility disposition

The MQ review distinguishes redundant delegation and fixture convenience from
retained production responsibilities. The mechanical slice may remove the
`inspect` and `plan_delivery` delegates while retaining identical underlying
operations; replace test-only `DeliveryRows::load` callers with the exact
validation/scan/restore fixture sequence; and scope the four split service
mint/bind conveniences to fixtures after caller proof. Production row/snapshot
decoders, import initialization and planning, Binding-mode ownership, CICS/IMS
lifecycle hooks, persisted fence and profile upgrade remain intact and pending
their existing later-owned composition. No automatic migration, public minting
or private feature activation is authorized. Existing occurrence capture may
consume the exact validated result-preflight borrows instead of duplicate
original/reply arguments, preserving identity, bytes and publication order.
Request-byte and pending-reason provenance remain retained; no discarded reads
or new refusal predicate is authorized as a lint repair. Strict MQ lint remains
pending where genuine production roots have no authorized operational caller.

### Focused conformance output scope

Declare `CV-209.conformance-output` under CV-209. Owners are the existing focused
xtask runners, shared canonical event/ledger output and a bounded private output
adapter with its CLI/tests. Reuse `ConformanceRunReport` and its canonical owners;
no alternate report schema, evidence ledger, verdict inference or success token
is authorized. Prepare actual CLI/output regressions for dropped dataset/RACF
reports, mixed JSON/diagnostic output and JCL's hardcoded destination. Review the
explicit destination/default-stream policy and serialization/I/O failure behavior
before implementation. No new IBM semantics or licensed campaign is required.

### Common program policy preparation

Declare `CV-208.common-program-policy` under CV-208. Owners are the existing
batch common-program catalog/schema/generator and program control declarations.
Prepare a typed generated representation of the current grammar policy, removing
name-string policy dispatch without changing control acceptance or utility
behavior. Manager must decide source-catalog compatibility before any schema
change; no second registry, new parser framework, product hardcode exception or
IBM command semantics is authorized. Preserve all current grammar/refusal tests.

### Framing native schema binding

Extend `CV-204.identity-framing` to the existing local schema compiler in
`xtask/src/coverage_projection.rs`, a private native schema regression module
and synthetic serialized DTO fixtures under xtask tests. Resolve the existing
package schema's IMS reference from its current local owner, refusing external
retrieval. Validate all six actual retained/current omitted/null/present controls
and nine structural negatives before sealing. Fixtures describe DTO shape, not
cryptographic or licensed truth. Keep the actual package discriminator in
`sections`; no second version field or alternative application schema is added.
Installer/publication schema changes remain in their separate declared slice.

### CV-202.serialized-schema-bindings acceptance

Status: **Complete (serialized coverage authority only)**. The frozen @1 row/evidence/ledger bindings now validate actual serialized shapes through native offline Draft 2020-12 and existing typed evidence/history/catalog owners. All 30 focused tests pass on the joint candidate, preserving valid failure histories, null oracle and empty pending controls. Unknown refs/fields, stale standalone projections and forged derived claims refuse. This does not provide a generic pre-allocation JSON reader.
Joint schemas/coverage/spec, module, formatting, frozen dependency, metadata and
diff checks pass. Strict tooling-owner lint passes with `--no-deps`; the unchanged
dependency-inclusive MQ/CICS lint failures remain separately pending and are not
waived or described as passed. Foundation/public exit acceptance remains pending.
Missing supplemental cache checks remain `skipped/unavailable`, zero credit.

### CV-208.production-scanner acceptance

Status: **Complete (production scanner boundary only)**. Actual Rust test items are excluded by the shared lexical scanner without hiding later production behind comment/string/test markers. Four real Rust-owner tests and the 26 Python scanner/boundary tests pass. Existing application/dehardcoding guards use one batched source transport and fail closed on malformed input or transport failure. Lexical cfg handling is conservative, not full compiler/macro evaluation.
Joint schemas/coverage/spec, module, formatting, frozen dependency, metadata and
diff checks pass. Strict tooling-owner lint passes with `--no-deps`; the unchanged
dependency-inclusive MQ/CICS lint failures remain separately pending and are not
waived or described as passed. Foundation/public exit acceptance remains pending.
Missing supplemental cache checks remain `skipped/unavailable`, zero credit.

### CV-201.catalog-locator-membership acceptance

Status: **Complete (offline catalog locator membership only)**. The shared catalog compiler and coverage check join locators to their own indexed baseline manifest or supporting receipt. Eight independent real-boundary tests pass, including five coherent rehashed locator refusals and three controls. All 1,506 rows, immutable denominators and five zero-credit normalization dispositions remain unchanged. This validates membership, not source-body execution or licensed behavior.
Joint schemas/coverage/spec, module, formatting, frozen dependency, metadata and
diff checks pass. Strict tooling-owner lint passes with `--no-deps`; the unchanged
dependency-inclusive MQ/CICS lint failures remain separately pending and are not
waived or described as passed. Foundation/public exit acceptance remains pending.
Missing supplemental cache checks remain `skipped/unavailable`, zero credit.

### Common program policy implementation decision

Manager selects option A for `CV-208.common-program-policy`: derive the finite
generated control declaration from existing typed builtin/execution/action
bindings, preserving the frozen source catalog @1 and all existing control
tokens, admission order and delegated grammars. No new catalog fields, policy
registry, parser DSL, source credit or IBM behavior is authorized. The existing
generator may decode finite role strings; runtime policy must use generated
typed declarations rather than program-name cases. The IEFBR14 bypass is an
explicit typed IgnoreInput declaration. Manager authorizes the reviewed additive
route/generator tests, owner generator/runtime changes and deterministic existing
program-registry regeneration in the isolated checkout, subject to unchanged
source input. One focused Cargo sequence is authorized; retain actual initial
assertion failures and final hashes, compile all affected targets, run owner lint,
focused tests, formatting, dependency policy and exact-target cleanup. Strict
dependency-inclusive failures remain pending. Manager owns integration/sealing
and shared metadata; no source-schema compatibility change is authorized.

### Focused conformance output implementation decision

Manager accepts ADR-0054 and the prepared writer policy for
`CV-209.conformance-output`: the existing canonical report owners remain the
sole event/ledger authority; default focused stdout becomes JSONL, human
diagnostics move to stderr, and explicit --output selects one external file.
The conservative 512 MiB record projection and exact 64 MiB staging bound are
CLI limits, not heap quotas. The reviewed six owner paths and additive tests
are authorized in the isolated checkout. Retain six actual old-CLI output
failures/four refusal controls. Production/tests may be prepared now without
Cargo; a later manager grant is required for its one focused build sequence.
No schema/dependency/new ledger/source semantics or private activation is
authorized. Manager owns current CLI documentation and sealing.

### Selected controller publication decision

Manager accepts ADR-0053 for `CV-204.publication-fencing`, including the shared
try-read/try-write busy policy, borrowed owner-created write context, unchanged
publication DTO relocation, one direct Batch application dependency, narrow
read-only Db2 prospective-install validation, existing pure IMS validation and
actual router registered-name refusal for ProgramCall controllers. Supported
composition is one publishing server per store. Existing pre-framing initial
red remains historical; implementation must consume the identity-framing seal
before green validation. Exact production/test owner scope follows the reviewed
publication inventory plus server/cobol.rs's narrow private registered-name
query and affected additive producer/interleaving controls. No distributed
transaction, new signature/state format or private subsystem is authorized.
The worker is awaiting the prerequisite seal and a separate execution grant.

### Journey closure module ratchet

The existing module gate requires exact recorded counts even after extraction.
Manager reviewed the same-function extraction and lowers only carddemo.rs's
legacy production ratchet from 12,796 to 12,077 as part of
`CV-209.journey-closure`. No ceiling is raised, no exemption is added, and all
new production modules remain below 1,200 lines. The initial joint module
failure is preserved; the corrected lower ratchet must pass before sealing.

### CV-204.identity-framing acceptance

Status: **Complete (bounded identity and native DTO schema only)**. Current writers emit framed @3 identities with finite neutral dispatch; trusted retained @2 identities/signatures remain exact, fresh @2 admission refuses, and retry requires full retained equality and current trust. The producing owner candidate passed 55 application tests, strict application lint, affected caller compilation and two selected server trust tests. Native integration reproduced all six valid DTO schema refusals, then passed two test families covering six valid controls and nine malformed refusals, plus 30 unchanged coverage schema controls. Explicit-budget identity and recovery remain bounded by configured PackageLimits. This does not authenticate arbitrary external snapshots, adopt a standard signature envelope, repair publication or complete Foundation.
Joint native schemas, application-package metadata, spec, coverage, program registry,
module, formatting, frozen dependency policy and diff checks pass. Tooling-owner
strict lint passes; retained MQ/dependency-inclusive lint failures are separately
pending. Missing supplemental cache remains skipped/unavailable with zero credit.
Broader public and Foundation exit checks remain pending.

### CV-209.cics-contract-lint acceptance

Status: **Complete (mechanical CICS and approved public API migration only)**. ADR-0052 borrowed contexts migrate all 79 inventoried caller expressions in original order; four public Rust signatures change before release. Existing receipt prune is exposed as manual trusted raw-store maintenance with authority/age/archive/protected-reference/coordination obligations and individual conditional deletions. Strict dependency-inclusive CICS Clippy passed; 114 focused tests passed, with unchanged earlier mechanical controls retained separately. Joint server all-targets compilation passes. Durable encodings and excluded commands are unchanged; full retention policy, private composition and official CICS completeness remain pending.
Joint native schemas, application-package metadata, spec, coverage, program registry,
module, formatting, frozen dependency policy and diff checks pass. Tooling-owner
strict lint passes; retained MQ/dependency-inclusive lint failures are separately
pending. Missing supplemental cache remains skipped/unavailable with zero credit.
Broader public and Foundation exit checks remain pending.
