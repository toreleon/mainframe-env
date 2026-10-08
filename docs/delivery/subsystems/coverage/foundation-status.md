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

Status: **In progress**. This non-semantic slice owns mechanical lint repairs in
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

Six isolated English CLI audits inspect existing owners. Initial current checks
pass spec, coverage, application packages and dehardcoding; semantic identities
and ABI inventory fail as stale. These checks do not replace executable product
regressions. Audit findings must be reproduced by focused negatives before repair.

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
