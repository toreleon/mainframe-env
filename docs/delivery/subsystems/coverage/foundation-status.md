# Coverage and conformance — Coverage authority progress

Subsystem: **coverage**
Phase: **foundation**

Status: **Implementation candidate; acceptance pending**

## Current scope

Foundation owns the shared official catalogs, independent six-gate coverage
contracts, semantic identities, package trust/install runtime, ABI libraries,
program/route registration, and cross-subsystem validation. The bounded public
repairs below are implemented; integrated Foundation acceptance remains pending.
Execution outputs, candidate identities and producing-input comparisons belong
in the external handoff, not a repository controller history.

Public preparation excludes licensed and private-only implementation and the
three unready CICS rows: **CICSMESSAGE**, **GETNEXT TIMER**, and **ISSUE COPY**.
Their identities and pending gates remain intact. Specifications, fixtures,
compatibility readers, and executable tests remain with their existing owners.

## Dependency flow

```mermaid
flowchart LR
    Inputs["Pinned inputs and catalogs"] --> Shared["Shared contracts"]
    Shared --> Owners["Subsystem implementations"]
    Owners --> Scoped["Scoped acceptance"]
    Scoped --> Integrated["Integrated public gates: pending"]
    Integrated --> Review["PR review"]
```

## Work packages and dependencies

| Package | Current bounded implementation | Remaining dependency / next step |
|---|---|---|
| CV-201 | Nine pinned baseline manifests and normalized catalogs; catalog-locator membership repaired | Preserve source pins and denominators; missing supplemental bodies remain unavailable with zero credit |
| CV-202 | Six independent gates, immutable evidence and snapshots; snapshot continuity, serialized schema bindings and package-state projections repaired | Structural schemas supplement typed identity/topology/derived-count validation; run affected integrated gates |
| CV-203 | Generated semantic identities and handler registry; generated binding repaired | Registration grants no runtime credit; retain exact selected-route requirements |
| CV-204 | Atomic generation selection/rollback; package bounds, framed @3 identities, canonical COSE_Mac0 authentication and publication fencing repaired | Retained @2/raw forms verify without rewriting; process-local fencing and file-SQLite recovery do not establish distributed exclusion or backend parity |
| CV-205 | Generic Db2 catalog install/rollback; retained catalog generation and seed-at-capacity controls repaired | Current backend and application regressions; same-process reopen does not prove a process crash |
| CV-206 | Installed typed batch controllers; duplicate-root and root-width admission repaired | Actual signed package/publication selection precedes participant effects; retain affected integration gates |
| CV-207 | Provider-owned CICS/Db2/MQ source libraries; generated binding and materialization bounds repaired | Preserve ABI bytes, licensing and source identities; local source tests grant no licensed equivalence |
| CV-208 | Generated official/custom routes and program registration; production scanner and typed common-program policy repaired | Preserve namespace and control ownership; generated registration does not establish execution coverage |
| CV-209 | Bounded host/store/batch/execution/CICS/server/conformance lint repairs, host/MQ layout, test-floor wiring and canonical conformance output implemented | Full MQ lint, integrated executed-test floor, application closure, backend parity and live client gates remain pending |

## Current acceptance boundaries

| Slice / gate | Status | Dependency / next step |
|---|---|---|
| `CV-204.standard-envelope` | Bounded production authentication implemented | Fresh production requires canonical `cose-mac0-hmac256@1`; retained recovery/retry preserves original identities and signatures. MAC authentication does not provide nonrepudiation or arbitrary snapshot authentication |
| `CV-204.publication-fencing` | One-publisher process admission and file-SQLite crash/recovery controls implemented | Actual retained selection joins the complete publication tuple; preserve the process-local limitation and separate cross-process/backend obligations |
| `CV-209.mq-mechanical-layout` | Bounded mechanical layout complete; full MQ contract-lint pending | Final affected tests pass; strict lint fails with exactly 26 retained production diagnostics. Keep legacy import, decoder/initializer, provenance/lifecycle and fence/upgrade responsibilities pending under their owners |
| `CV-209.journey-closure` | Closure authority and observation transport implemented | Full CardDemo refuses incomplete observations; do not infer workload acceptance from harness tests |
| `CV-209.carddemo-transactions` | Selected date/duplicate comparisons and binding implemented | J06 AIX browse/navigation and all 105 issue acceptance requirements remain unbound; 20/20 journeys and 26/26 transaction closure remain pending |
| `CV-209.postgres-parity-selection` | Twelve selectors and nonempty-pass checks wired | No live backend result; prepare and execute the existing twelve controls on the intended clean candidate through the existing lifecycle owner |
| `CV-209.public-client-compatibility` | External input/startup feasibility only; API fixture preparation active | `--version` feasibility is not a job workload pass. Submit/query/files/content, authentication/ownership refusals and actual method/URI/status capture remain pending |
| `CV-209.test-floor` and integrated exit | Minimum 260-pass recorder wiring implemented; full exit pending | Count actually executed passing tests on the selected unchanged candidate; preserve every affected architecture/profile/schema/package/source/backend gate |
| `CV-209.public-status-docs` | Current subsystem status and documentation reconciled | Preserve active preparation scope, exact pending gates and implemented sealer; retain raw outputs externally |
| `CV-209.candidate-cleanliness` | Recorded command boundary validation complete | Existing recorder rejects tracked/untracked source changes before launching and at completion; ignored builds/external logs remain valid. This does not provide continuous source attestation |

## Source and coverage boundaries

The nine catalog baselines retain **1,506 mandatory identities**. Each gate's
numerator and denominator come from the existing catalog/applicability/verdict
owners; this progress summary assigns no coverage counts. Generated catalogs,
source lookup and local fixture checks grant no semantic or licensed credit.
A row is complete only after every applicable `recognized`, `validated`,
`executed`, `conditioned`, `recovered`, and `differential` gate passes.

The five VSAM roadmap-normalization rows keep their zero-publication-credit
disposition. The original z/OSMF heading-level denominator remains frozen;
endpoint normalization has its own source-bound subsystem projection.

Raw IBM publication bodies stay in the external cache. Ordinary review does not
refresh or re-pin topics. Missing supplemental bodies remain **skipped/unavailable**
with zero credit under the existing user disposition; do not retry unchanged
missing-cache checks or request the same cache again. Continue available-input
public work without turning a source-cache skip into product acceptance.

## Remaining integrated acceptance

Full CardDemo requires actual observation closure for all 20 journeys, all 26
transaction requirements and all 105 issue acceptance requirements, followed by
its applicable isolation, backup/restore and restart controls. The selected
transaction repair supplies only its observed date/duplicate scope. Same-process
SQLite reopen is not a fresh-process crash/recovery control.

All twelve live PostgreSQL parity controls and the finite authenticated public
client workload remain pending. Optional Zowe and Node inputs stay outside Git
and the production sandbox image. Query status is not named-status-route
coverage; lookup refusal is not direct spool 403 coverage; selected SYSPRINT
content is not complete DD inventory or official z/OSMF profile parity.

Licensed differentials remain pending and private composition stays outside this
public task. Bounded implementation acceptance does not complete Foundation,
claim whole IBM subsystem compatibility, or authorize publication/deployment.

## Validation and next step

Use the existing owners for focused checks and retain actual outputs outside Git:

```bash
cargo xtask spec --check
cargo xtask coverage --check
cargo xtask semantic-identities --check
cargo xtask application-packages --check
cargo xtask abi-libraries --check
cargo xtask dehardcoding --check
```

Select remaining checks from the changed contracts and the Foundation exit gate;
a listed command is not a recorded pass. Preserve initial failures and their
producing inputs. The prior full MQ package result predates fixture-only changes
and must not be represented as a current-input full-package rerun.

The manager owns integration, status edits and bounded-slice acceptance. Use the
implemented `cargo xtask work-package-seal` owner for exact changed-path
allowlists and generated commit trailers/digests; its `--check` validates the
seal. A seal does not create product execution credit. Raw results stay outside
Git; do not hand-hash a replacement seal or create another controller ledger.
The next step is the existing external-only client and PostgreSQL preparation below;
their execution and implementation require the manager's subsequent grant.

### CV-209.public-client-compatibility preparation declaration

Manager authorizes source-only preparation of a finite first-party API compatibility
fixture against the actual ProductServer using optional external @zowe/cli 8.39.0
and the accepted Node 24.19.0/bubblewrap inputs. Existing CI assurance and input
policy owners remain authoritative. No new ledger/schema, production API change,
full DD inventory golden, missing-source refresh or official conformance credit.

Freeze exact existing owner paths and lifecycle before implementation: exclusive
loopback listener/artifact/client directories; valid second principal; actual
method/URI/status capture; independently expected selected STEP1:SYSPRINT record
IEFBR14; successful submit/query/files/content and actual authentication/ownership
refusals with unchanged job/spool state. Query status and lookup refusal have only
their actual scope. Retain the existing ignored manual-server test unchanged.
Bounded readiness/client/shutdown and per-command process-group teardown are
required. Keep HOME/CODEX_HOME unset, verified minimal filesystem binds, disabled
plugins/native/global lookup and accepted intact external archive. No npm, scripts,
helper installation, ambient profiles or host filesystem widening.

Preparation is external design/source inspection only: no repository edits, Cargo,
server/job/client execution, new pins/downloads, CI input mutation or acceptance
status. Hand off the smallest exact owner plan, source-independent assertions,
regressions and supervisor lifecycle to the manager for implementation grant.

### CV-209.postgres-parity runtime preparation declaration

Manager authorizes external-only preparation of the exact PostgreSQL 18.6 input
for the existing twelve-gate parity owner. Inspect retained image/native inputs
and existing tools/jenkins/postgres_parity.sh; reuse its lifecycle/reset/cleanup.
No repository edits, database/server/test campaign, new lifecycle manager, Cargo,
package install/download or input-lock changes. The retained image may be inspected
and safely copied from an owned stopped container into external scratch, with
container cleanup; no daemon/host security changes. Verify exact version and
relocated support files/individual dynamic libraries needed by pg_config/initdb/
pg_ctl/postgres. Keep review/provenance failures and scoped source pins intact.
Prepare the exact clean-candidate plan/command and finite supervisor/receipt/target
retention for a later actual twelve-control gate; backend acceptance stays pending.
