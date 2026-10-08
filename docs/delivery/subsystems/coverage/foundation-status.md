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
