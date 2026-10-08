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
| CV-209 | Bounded host/store/batch/execution/CICS/server/conformance lint repairs, host/MQ layout, test-floor wiring and canonical conformance output implemented | Full MQ lint, integrated executed-test floor, application closure, global backend parity and live client gates remain pending |

## Current acceptance boundaries

| Slice / gate | Status | Dependency / next step |
|---|---|---|
| `CV-204.standard-envelope` | Bounded production authentication implemented | Fresh production requires canonical `cose-mac0-hmac256@1`; retained recovery/retry preserves original identities and signatures. MAC authentication does not provide nonrepudiation or arbitrary snapshot authentication |
| `CV-204.publication-fencing` | One-publisher process admission and file-SQLite crash/recovery controls implemented | Actual retained selection joins the complete publication tuple; preserve the process-local limitation and separate cross-process/backend obligations |
| `CV-209.mq-mechanical-layout` | Bounded mechanical layout complete; full MQ contract-lint pending | After the size-consistency repair, strict lint retains 25 original production diagnostics and no new diagnostics. Keep legacy import, decoder/initializer, pending provenance/lifecycle and fence/upgrade responsibilities pending under their owners |
| `CV-209.mq-public-provenance-size` | Original-request byte-count consistency complete; all 31 selected controls pass | Size-only substitutions refuse before retained intent lookup; memory/SQLite state and all old controls remain intact. Full MQ strict lint remains pending |
| `CV-209.journey-closure` | Closure authority and observation transport implemented | Full CardDemo refuses incomplete observations; do not infer workload acceptance from harness tests |
| `CV-209.carddemo-transactions` | Selected date/duplicate comparisons and binding implemented | J06 AIX browse/navigation and all 105 issue acceptance requirements remain unbound; 20/20 journeys and 26/26 transaction closure remain pending |
| `CV-209.postgres-parity-selection` / `CV-209.postgres-loopback-owner` | All twelve existing live controls passed on the qualified source-equivalent producer | TCP-only startup removes an unused Unix socket path limit. Host loader/timezone differences, retained cleanup failures and one defunct PID-1 child remain qualified; this is no global backend or full CardDemo closure |
| `CV-209.public-client-compatibility` | Seven frozen fixture controls pass; second real run reaches files then exceeds the ten-second action allowance | Four commands make five real HTTP observations: submit 201 and list/query/files 200. Further validation-cost repair, content and authentication/ownership acceptance remain pending |
| `CV-209.public-client-action-layout` | Finite layout/log repair complete; 146 qualified private-PID controls pass | Real client workload acceptance remains separate; preserve exact readonly mounts and byte/type/mode/link bounds |
| `CV-209.public-client-validation-cost` | Command-local optimization complete; 106 combined controls pass | Every POST byte remains hashed; one byte-only timing diagnostic does not prove the full action meets its allowance |
| `CV-209.public-client-tree-directory-authority` | Command-local descriptor traversal complete; 112 affected controls pass | Fresh bytes and all closing fences remain; repeated member ancestor resolution is removed. Changed-tree timing and real-client acceptance remain pending |
| `CV-209.cics-bms-input-storage` | Supported symbolic input storage complete; eight final controls and seventeen regression methods pass | Preserve raw/non-BMS behavior and fixed-group checks; provider/official/full-layout acceptance remains separate |
| `CV-209.carddemo-transaction-navigation` | Latest changed-runtime run: eleven passes, five failures, nineteen completed shutdowns | Preserve all four actual failed producers. Resolve CICS first reverse-read positioning and separately reviewed private oracle errors; navigation token and full closure remain pending |
| `CV-209.test-floor` and integrated exit | Minimum 260-pass recorder wiring implemented; full exit pending | Count actually executed passing tests on the selected unchanged candidate; preserve every affected architecture/profile/schema/package/source/backend gate |
| `CV-209.public-status-docs` | Current subsystem status and documentation reconciled | Preserve active preparation scope, exact pending gates and implemented sealer; retain raw outputs externally |
| `CV-209.command-supervision` | Optional Linux deadline/output ownership complete | Current-invocation logs and retained-leader fencing prevent stale output/PGID reuse; exclusive wait and conservative procfs limitations remain explicit |
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

The selected twelve live PostgreSQL controls passed on their retained producer;
global backend acceptance and the finite authenticated public client workload
remain pending. Optional Zowe and Node inputs stay outside Git
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
The next dependencies are bounded navigation and
the finite public client fixture under their explicit manager grants below.

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

### CV-209.carddemo-transaction-navigation preparation declaration

Manager authorizes source-only preparation of the next J06 transaction AIX
browse and screen-navigation comparisons, after the accepted date/duplicate
slice. Reuse the preserved corpus, source closure and actual comparisons;
inspect the real COTRN00C/COTRN01C/COTRN02C CSD/maps/copybooks and current
private fixture owners. Freeze literal independent browse order/identity/bytes,
real menu/detail/back navigation and unchanged state/teardown requirements.
Do not infer tokens from a declared manifest or issue a full closure receipt.

No repository edit, Cargo, compilation, server/job/native route workload, source
refresh/re-pin or campaign is granted. Perform only needed offline pinned-source
lookup; unavailable bodies remain skipped/zero without repeat requests. Exclude
all private/licensed work and the three unready CICS rows. Hand off the smallest
exact owner allowlist and independently failing control plan before a later
implementation grant. Preserve prior eighteen-control producing receipts.

### CV-209.carddemo-transaction-navigation implementation declaration

Manager accepts the source-only eight-path plan and sixteen independent controls.
Grant selected six-primary/five-map compilation and bounded terminal navigation,
plus separately uncredited physical AIX probes through the existing dataset API.
Keep the sealed date fixture's original two-row bound and eighteen controls.
Register private navigation owners under the exact proposed allowlist; manager
owns the facade ratchet, generated files, status and seal. New production modules
remain below 1,200 lines, without a harness DSL or provider ownership extension.

Use the independently frozen twelve raw rows, complete fields/identities/versions
and source menu/CSD/map semantics. No retargeted TRANSACT alias, application edit,
manifest weakening or AIX application token: the real CT00/CT01 browse the primary
KSDS. The physical AIX probes cannot close that application requirement. Preserve
the pinned ENDFILE RESP2 qualification and keep official credit pending.

Run only the sixteen-control selector on a checkout-local task target, jobs=2,
with retained initial prerequisites, actual binary/input identities, captures and
bounded shutdown. Stop on any genuine compilation, route, PF7 or provider defect;
report the source-bound red before requesting a different owner scope. A missing
screen-navigation observation is a behavioral red only after all comparisons
pass. Only then bind that one token through the existing observation owner and
rerun affected controls. No full CardDemo, issue credit, private/licensed work,
cache refresh, provider implementation or automatic prerequisite retry is granted.

### CV-209.postgres-loopback-owner acceptance

Status: **Complete (selected twelve-control live parity)**. The existing lifecycle
owner now disables its unused Unix socket and retains TCP loopback, allowing the
actual long workspace to start without a socket-path workaround. All twelve
unchanged controls passed with one actual passing test each and zero ignored
tests on their clean producer. Producing runtime/test/tool/source bytes match the
integrated slice; raw candidate identities remain with the original receipts.
The CardDemo restart control is synthetic and does not close the public corpus.

Both earlier setup failures remain not-run, not accepted. PostgreSQL 18.6 used
qualified retained native inputs: host loader and absolute timezone differences
remain explicit. Database/port/task target were removed after artifact retention;
Cargo clean failed before validated exact-target removal, and one terminated
PostgreSQL child remains defunct under PID 1. No live owned executable/listener
remains, but all-PIDs-absent is false. This bounded pass does not establish full
Foundation, global backend, application, official or licensed acceptance.

### CV-209.subsystem-prompt-consistency acceptance

Status: **Complete (documentation only)**. Common execution instructions, phase
prompts and plan links now use subsystem/phase delivery instead of obsolete
minor-release and 1.0 planning labels. Existing selectors, work-package IDs,
wire/spec versions, source identities, six-gate obligations and licensed pending
requirements are unchanged. The documentation/changelog/subsystem validators
and frozen dependency policy passed; no product behavior or execution credit
is established by this wording repair.

### CV-209.public-client-inputs acceptance

Status: **Complete (optional input grammar and byte admission only)**. The existing
lock has strict retained @1 and finite @2 readers; @2 adds one explicitly selected
Linux development profile without changing the eight required tools or ordinary
CI/runtime scopes. Twelve fixed file roles, archive/SRI/tree bytes, package
identity, bounded membership and nonsymlink stable metadata are checked without
execution, extraction, download or package resolution. Byte authority stays in
the existing lock. All thirty-three focused controls pass after an independent
three-subcase float-mode red and exact-integer repair; original controls and pins
are unchanged. The earlier twelve-role byte admission remains bound to its
original producer and unchanged inputs, not relabeled as a new runtime result.

Node LICENSE follows the accepted design prose; the retained Zowe tree digest
uses the original producer's path-component ordering, independently controlled.
Node trust signatures remain unverified; Zowe SRI is not source-build attestation;
bwrap/library bytes remain host-qualified with bootstrap outside its namespace.
This does not provide launcher containment, live jobs, public-route, whole
workspace/backend, official or licensed credit. The finite command owner and
actual ProductServer/client fixture remain the next separate dependencies.

### CV-209.command-supervision acceptance

Status: **Complete (qualified optional Linux supervision)**. Silent/non-newline
commands, combined output saturation, cancellation and remaining same-group
descendants refuse under the finite owner. Pre-launch refusal binds current error
bytes; leader identity remains reserved through every group operation before the
sole wait. Default callers, receipt schemas, candidate checks and test floors
remain unchanged. All eighty-two unique controls have observed passes across
the preserved suite and one focused external-observer repair; the original
suite exit 1 is not relabeled as a single green run. All thirty-nine prior
controls passed, and twenty-six observed native launchers completed their waits.

Independent initial reds and final source review address stale-log binding and
early reaping. Lifetime ordering is source/mock proof; the native escaped-session
control is not a PGID-reuse experiment. Bounded mode requires matching Linux
procfs/non-reaping wait and exclusive child-wait ownership; conservative census,
callback/kernel/escape and prior PID-1 zombie limitations remain qualified.
No live client, whole campaign, portable sandbox or official/licensed credit.

### Navigation prerequisite disposition

STRING and figurative-comparison repairs clear the menu-label and LOW-VALUES
prerequisites. The latest sixteen-control continuation executes six passes and
ten failures, including four uncredited physical-index/source-gap controls.
The independently supported missing keyed READ 13/80 comparison now passes.
The remaining boundaries are three-digit amount field names and RECEIVE MAP
transport frames entering symbolic storage. Fresh empty validation skips the
thirteen-target clear; the missing-key clear already succeeds. PF7 and the
aggregate receipt seam remain unreached. No navigation/AIX application
observation is bound.

All three failed producers retain their executable, source, raw observations
and successful shutdowns. The keyed READ correction retains the original 13/0
failure separately. The bounded generic RECEIVE MAP declaration below precedes
the separately source-supported amount-name and fresh-empty oracle corrections.
No unchanged retry or provider/source change is granted.

### CV-209.cobol-string-references acceptance

Status: **Complete (existing checked STRING reference boundary)**. A private
STRING owner consumes complete sender and identifier-delimiter references through
the existing reference/read owner, preserving selected raw PIC bytes and leading
zeros. Literal/figurative, target, pointer, overflow and UNSTRING behavior is
unchanged. Seven frozen native controls pass after five genuine runtime failures;
the shared layout baseline and unchanged pointer/UNSTRING preservation control
also pass. The legacy facade decreases to 11,683 production lines and the new
private owner has 122. Strict affected-owner lint and selected policy/module
checks pass on the integrated source; original native receipts retain their
producing candidate and byte identities.

Qualification follows the existing shared resolver boundary; interleaved
qualification after a subscript is not established. Missing COBOL bodies remain
skipped/zero. This generic public/native repair grants no IBM differential or
licensed credit and does not yet establish CardDemo navigation. The original
navigation prerequisite failures remain immutable; one changed-runtime focused
continuation is the next dependency, with independent expectations unchanged.

### CV-209.sandbox-build-parallelism acceptance

Status: **Complete (build-owner resource bound only)**. Docker RUN now passes
two jobs explicitly to Rust and Git builds; it no longer relies on a host
environment variable to reach the image builder. Repository input policy,
frozen dependency policy and docs/fragment/subsystem validation passed. Pins,
targets, runtime, legal inputs and containment are unchanged. No Docker build,
current image size, speed, sandbox workload or application batch pass is implied;
those remain final-candidate gates.

### CV-209.public-client-command acceptance

Status: **Complete (finite command transport only)**. Ten fixed action shapes
reuse optional locked input validation and the existing Linux supervisor. Scalar
applicability/duplicates, exact root admission, owned settings/JCL, fixed readonly
mounts, empty child environment, bounded separate raw captures, actual successful
wait exit and before/after identity/state validation are checked. Default callers
and receipt/floor contracts remain unchanged. A reap callback cannot restore
group authority or overwrite actual exit with a fallback status. Independent
mocked controls and two tiny native keyword controls establish these boundaries;
review-found unbounded diagnostics and hidden duplicate/abbreviated root admission
have genuine retained REDs and focused passes.

The final host 136-test suite failed one unchanged conservative census control
when required procfs metadata disappeared; that failed producer remains intact.
One separately granted changed-condition run in a private PID/proc namespace
passes the same 136 tests with zero skips, actual waited bubblewrap exit zero,
unchanged source/input hashes and exact owned temp cleanup. This is qualified
tooling validation, not public-client containment or host group-absence proof.
The original 123/134 passing and 136 failing receipts keep their own producers.

No Node/client version, server, HTTP route, job, spool or authentication workload
is established by this slice. Rust still owns the actual application fixture,
semantic refusal/state assertions, listener lifecycle and teardown. Host
bootstrap/trust, inherited network, synchronous validation and inspection quotas
stay qualified. No official/licensed or full Foundation acceptance credit.

### CV-209.cobol-figurative-comparisons acceptance

Status: **Complete (ordinary byte-relation figurative width)**. Explicit unquoted
figurative constants use the evaluated opposing byte operand's width. Ordinary
identifier/literal space padding, numeric ZERO, mixed numeric handling, collating
sequence, parser/reference/layout and level-88 behavior remain unchanged. A
private condition owner reduces the legacy facade from 11,683 to 11,498
production lines; the new owner has 216. Eight frozen native controls pass after
six genuine runtime failures, covering 71 literal branch comparisons and 28
unchanged storage checks. The existing forty-four-fixture method passes as one
Rust test, and three existing interpreter condition-selected controls pass.
Strict affected-owner lint and integration policy/module checks pass separately;
native receipts retain their actual producing candidate and bytes.

Missing COBOL bodies remain user-skipped/zero. This generic public/native repair
grants no IBM differential, licensed, full grammar or CardDemo navigation credit.
One changed-runtime navigation continuation may now examine the unchanged
expectations; the separate NOTFND secondary-code oracle remains unresolved.
Original runtime failures and qualified source/reference boundaries are retained.

### Navigation changed-runtime and keyed READ oracle declaration

Manager accepts the independent checked-in `cics.read.notfound-80` rule in
`pilot-compile-rules.json` and the exact missing-record 13/80 expectation in
`pilot-fixtures.json`. These precede this navigation helper and do not derive
from the provider implementation or its observed output. Their source locators
and generated candidate pins provide reference context, not fresh body or
official/licensed execution credit. The application's primary RESP check alone
does not establish RESP2. Retain the original frozen 13/0 failure separately.

In the already granted navigation owner only, make `require_read` accept explicit
expected RESP2: both NORMAL call sites retain 0/0 and 350 payload bytes; only the
missing keyed READ requires 13/80. Preserve every other raw row, field/message,
page, state/version, one-READ, empty-input and successful payload expectation.
Do not add a new failure-payload golden from the observed zero bytes. No provider,
source, CSD, token identity or denominator change is granted.

After the byte-preserved navigation patch advances onto the sealed STRING and
figurative-comparison runtime, one sixteen-control continuation is granted with
this independently supported oracle correction. New prerequisites, including a
PF7 failure, stop for source/trace diagnosis. Only if all actual comparisons and
shutdowns pass and the intended missing-observation control fails at its receipt
seam may the already granted transport owner bind navigation, followed by one
same-selector validation. Keep all previous failed producers, source/ELF/raw
observations and teardown results; no unchanged retry or broader campaign.

### CV-209.public-client-compatibility declaration

Manager grants only conformance `lib.rs` test-only registration, a new private
`public_client_compatibility.rs` fixture and one isolated fragment. Reuse the
accepted input lock/validator and finite Python command without modifying them.
Freeze independent literal JSON, request-vector, content, mutation and cleanup
assertions before helpers. This is a new conformance harness; do not invent a
product RED using missing symbols or a deliberately failing implementation stub.

Use real ProductServer/MemoryStore/Local artifacts, a held loopback listener's
actual nonzero port, administrator bootstrap/full readiness and a separate valid
read-only identity. Observe and forward real HTTP, never fake routes. Execute
the accepted nine fixed calls plus at most twelve status polls; require independent
ACTIVE/null, OUTPUT/CC 0000, selected STEP1:SYSPRINT seven-byte content, actual
401/403 and filtered-empty lookup refusals. Before each negative, require both
job completion and queue completion, then compare bounded exact job/provider
rows and every owned local artifact byte. No production principal-name branches.

Keep five-second readiness, 120-second phase, per-command maximum ten seconds,
joined serve/shutdown/listener release and exact owned directory cleanup on every
body result. An explicitly selected ignored live test must fail missing inputs,
not skip. Retain raw command exits/errors/HTTP/source/actual compiled executable.
Run affected independent assertion controls first; one actual finite live fixture
then stops on any product/infrastructure prerequisite, without broadening owners
or modifying its oracle. Strict lint, clean-candidate final recorder, integration
and sealing remain manager-owned. Initial compilation awaits the explicit
external Cargo-slot grant after navigation releases its target. No new version
workload, source refresh, full workspace/backend/CardDemo, private or licensed
implementation and no official acceptance credit.

The first native client continuation passes all seven frozen assertion controls.
Its one real submit attempt fails before any HTTP request: the accepted client
cannot resolve `@zowe/cli`, and state inspection rejects `imperative_debug.log`.
The fixture also reports its ten-second invocation allowance exceeded; no
child-only timing breakdown was measured. Preserve these distinct errors and the
actual failed producer. Server shutdown, listener release and owned scratch
cleanup succeed, and the exact Cargo target is cleaned after artifact retention.
No job, spool, authentication refusal or live-route acceptance is established.
A separate finite command-owner prerequisite must pass before another
changed-input run.

### CV-209.public-client-action-layout acceptance

Status: **Complete (finite command layout and log inspection only)**. The single
readonly accepted package mounts as local `node_modules/@zowe/cli`, allowing
ordinary nested module resolution without aliases, global lookup, NODE_PATH or
extra host mounts. Only the source-proven `imperative_debug.log` name is added;
existing per-file/aggregate caps, ownership, type, mode, link and stable-byte
checks remain unchanged. Version feasibility retains its original limited scope.

Independent layout/log controls have genuine initial failures. The full affected
146-test suite passes with zero skips in the qualified private PID/proc test
namespace, actual waited exit zero and owned temp cleanup. A final new-test
identifier-only correction has exact body/AST equivalence; the suite's original
producer is retained. The unsupported owner-execute refusal expectation and its
failed receipt remain disqualified; production mode policy was not changed.
No actual client/server workload or live route/containment/official credit is
established. Validation-cost and finite fixture gates remain separate pending
dependencies, with original failed live observations retained.

### CV-209.public-client-validation-cost acceptance

Status: **Complete (command-local input validation optimization only)**. Full PRE
admission privately retains immutable archive semantic conclusions for exactly
one POST. POST still reads and hashes every current archive/role/tree byte,
requires SRI and exact lock/profile/path identities, and keeps both archive
descriptor/path fences through all remaining checks. Only redundant POST
decompression is omitted. Proof mint follows PRE identity rechecks; reuse expires
with the command. Default validation, input pins, SHA policy and ten/120-second
allowances are unchanged. There is no persistent or metadata-only admission cache.

The duplicate-parser observer has a genuine retained failure. Ninety-six affected
controls pass; the integrated layout/cost candidate passes all 106 combined
controls. The intermediate PRE-lock mint failure is genuine, while its following
binding subcase was contaminated and is disqualified as an independent RED.
One actual byte-only diagnostic measures PRE 5.873 seconds and POST 2.677 seconds,
with parser opens 2/0 and identical accepted tree/role identities. This is neither
a workload benchmark nor proof a full client action meets its allowance. Original
failed live receipts remain unchanged; finite application acceptance is pending.

### CV-209.cics-bms-input-storage acceptance

Status: **Complete (supported symbolic RECEIVE MAP input storage)**. The existing
response owner checks decoded I/L fields within the selected fixed group before
mapped writes. Transport frames never enter that symbolic storage. Omitted bits,
flags and adjacent bytes remain unchanged, while present empty data follows the
existing byte-write owner. Qualified targets prevent duplicate simple names from
redirecting writes. Invalid widths/lengths and ambiguous/unknown members refuse.
Non-symbolic/non-BMS raw behavior remains; Legacy representation preservation
has static guard proof, with no separately executed variant claim. No provider,
ABI, encoder, source/copybook, shared reference or MOVE owner changes.

The initial eight controls have three passes and five genuine failures. The final
checked source passes those eight and seventeen existing regression methods,
including the forty-four-fixture baseline counted as one method. Strict affected
owner lint and formatting pass. The facade decreases from 11,498 to 11,453
production lines; the response owner has 417. Original failures, intermediate and
final actual executables retain separate producing inputs. All frozen literals,
compiled fixture bytes and twenty-five old methods are preserved. Exact target
cleanup succeeds after retention. Native source equivalence supports integration
without relabeling these dirty development receipts as clean-candidate CI.

This is bounded internal first-party ownership proof, not complete BMS layout,
provider/application, IBM wire, official or licensed acceptance. Missing bodies
retain user-skipped zero credit. CardDemo must still execute its changed-runtime
continuation with only the previously source-supported three-digit amount-name
and fresh-empty zero-byte oracle corrections. Missing-key/PF4-cleared spaces,
successful records, raw rows/state/READ counts and all previous producers remain.
PF7/token/full journey and Foundation acceptance stay pending.

### CardDemo navigation after symbolic input repair

The generic input-storage dependency is sealed. The preserved seven-owner
navigation candidate advances onto that accepted runtime with the prior NOTFND
13/80 correction intact. Manager grants exactly two additional source-supported
oracle corrections in its private comparator and independent test owner: only
amount names use TAMT001..010, and only fresh empty-detail result fields retain
literal zero bytes at their thirteen pinned widths. Missing-key/PF4 spaces and
every successful value, raw row, trace/count, state/version and source pin remain.
Add the corresponding independent fresh-empty raw-field assertions; never infer
them from an observed handler response. ENDFILE primary 20 retains its frozen
scope; secondary 90 remains qualified without adding an assertion or credit.

Run one changed-runtime sixteen-control continuation under the existing finite
execution owner, frozen/offline jobs two and a new exact checkout target. Stop on
any real runtime/source/application prerequisite, including PF7, retaining actual
errors, executables, inputs and teardown. Only if all comparisons and shutdowns
succeed and the missing-observation control reaches its intended receipt-only
failure may the already granted observation transport bind screen navigation;
then validate the same selector once. No issue token, native-AIX application
claim, source/CSD/alias/provider/kernel repair or full journey closure is inferred.
Manager owns shared ratchets, strict owner lint, generated status, integration
and seal; private/licensed work and the three unready CICS rows remain excluded.

The changed-runtime continuation is complete with eleven passes, five failures
and nineteen completed fixture shutdowns. Its actual executable, compiler
libraries, source/input identities and raw results are retained before successful
exact target cleanup. No navigation observation was added. PF7 leaves an
incomplete page numbered two; keep the full page-one and raw-row/state assertions.
The existing Dataset gap-cursor contract remains authoritative. A CICS
first-read/reposition adapter needs its own bounded repair and controls.

Independent source review also distinguishes initial-display zero bytes from
spaces initialized for the next task after RETURN, source-driven PF5 discard
from ordinary first-page entry, and lexical first-mismatch diagnostics. These
private oracle corrections require a separate exact grant; the failed producer
and its earlier zero-byte rationale remain unchanged, with no acceptance credit.

### Finite public client after launcher repairs

The preserved three-owner fixture advances to the sealed layout, validation-cost
and input-storage candidate with exact source bytes/modes unchanged. Reuse the
original seven pure-control passes only as source-equivalent prior results.
Compile one new current executable and execute the same finite real fixture once,
under its existing 180-second execution owner and unchanged ten-second action,
120-second phase, literal HTTP/JSON/content/state/refusal and cleanup checks.
Require the exclusive target and idle unrelated workers before native launch.
Stop at any real prerequisite without product repair, retry or oracle adjustment;
retain every partial observation and actual wait/cleanup before exact Cargo clean.
This remains development evidence; integration, strict owner lint, seal and final
clean-candidate acceptance are separate manager responsibilities.

The second actual fixture stops after four waited commands and five HTTP
observations. Submit, owner-list and query-status comparisons pass; files routes
return 200 but the whole files invocation exceeds its ten-second allowance.
Content and negative cases are unreached. Keep the limit and both failed
producers; diagnose further current-byte validation cost before another run.

### Navigation oracle corrections before the CICS positioning repair

Manager grants source-only correction in the preserved private comparator/test
owners: spaces for the reentered empty-detail result fields, while independently
asserting the initial display's thirteen zero fields; PF5-only full rows 42..51;
and the lexical first-mismatch TDESC01 diagnostic with refusal/no-observation
unchanged. Other first-page, source, successful record, NOTFND and state oracles
remain intact. Preserve the superseded zero rationale and all actual producers.

For the first PF7, pinned source independently requires discard of 51, ten rows
50..41, then one mandatory lookbehind reaching ENDFILE. Freeze the complete page
41..50, page one and reached-top message, with twelve READPREV results: eleven
NORMAL and one primary 20. The next PF7 retains already-at-top and zero browse
reads. This strengthens positioning checks; it cannot accept the current partial
page two. Secondary response qualification and all raw-row/state bounds remain.
No Cargo or native repeat, transport/token binding or fragment is granted during
this preparation. The CICS positioning repair and actual validation remain
separate pending dependencies owned by the manager.

### Command-local tree directory authority acceptance

Status: **Complete (bounded command-local namespace admission only)**. Private
PRE/POST use a fresh depth-bounded descriptor-relative tree walk. Held ancestors
authorize each leaf; root/package remain held across the fresh manifest and late
directory checks. Closing root resolution precedes its final descriptor/path
identity comparison. Explicit raw leaf descriptor ownership covers stream-open
failure. Full current byte/hash/SRI, type/uid/mode/capability, exact member/count/
depth/byte and component-order digest checks remain. Existing hardlink policy and
ordinary standalone validation remain unchanged; both POST archives surround all
remaining checks. No persistent or cross-command admission cache is introduced.

Independent duplicate-work controls genuinely fail on the old owner. The final
tiny fixture observes root/manifest/closing-root checks in every phase without
per-member absolute resolution. Two independently exposed closing-order and
stream-open leak defects have retained failures followed by minimal repairs.
All 112 selected controls pass, including thirteen new controls; integrated root
validation also passes the same selector. Old test bodies and all pins remain
byte-preserved. Independent review accepts the bounded authority/cleanup model.
The earlier incomplete safety model, observer-setup failures and successful
intermediate producer retain their own external qualifications.

This establishes focused mocked/tiny-fixture correctness, not accepted-tree
latency, a continuously atomic filesystem snapshot, native containment or a live
ten-second action pass. Both real-client failures remain unchanged. A separately
bounded changed-source timing diagnostic and real fixture continuation remain
pending under manager sequencing. Full client/Foundation acceptance is pending.

### CICS first reverse position prerequisite

Manager accepts the bounded additive Dataset positioned-read design for the
ordinary full-key first READPREV after successful STARTBR/RESETBR. Retain the
existing gap-cursor contract, reverse assertions and canonical request bytes.
The new read observes the retained logical-key/base-identity tuple and live body
under one existing Dataset state lock, without advancing or creating a cursor.
The CICS adapter consumes its bounded pending seed only on a validated completed
response; one real browse delegate replaces the existing one. End-sentinel,
generic, update/token and unsupported reposition cases retain their old paths.

The reviewed twenty-one-owner design is the exact proposed implementation scope,
including ADR-0056; no unrelated owner or cleanup behavior is implied. First
prepare and freeze its eight existing-CICS-API composed controls and exact old
baseline sources in the isolated first-reverse checkout. Only the new server
test owner and its test registration may change during this preparation. No
Cargo/native execution or production semantic repair occurs until the manager
checks the frozen packet and grants one bounded initial execution.

Task-drop browse retirement is a concrete separate acceptance gap. Neither the
positioned read nor explicit ENDBR tests prove cancellation-independent task
cleanup. Preserve that pending gate and its real ownership requirements; do not
discard refused retirement, fabricate cleanup authority or claim full CICS,
CardDemo or Foundation acceptance from the narrower repair.

### MQ original-request size consistency acceptance

Status: **Complete (bounded original-request provenance consistency only)**.
Core-intent binding recomputes the privately borrowed original request's canonical
byte count under the existing hard bound and compares the admission summary
before retained intent lookup. Valid request behavior, provider/envelope limits,
original digest/owner/context checks and physical publication remain unchanged.

Four independent memory/SQLite below/above controls genuinely fail on the old
binding, then pass with unchanged effect, execution, queue, audit and retention.
All four separate absent-intent ordering probes also pass on the repaired source;
they were unreached during the original failures. The three affected selectors
pass nineteen, three and nine actual methods. Existing test bodies/literals
remain byte-preserved; new-control formatting preserves every nonwhitespace
token. Both actual executables and their producing inputs remain external.

Strict affected-owner lint still fails with twenty-five original dormant
responsibilities, zero new or changed-owner diagnostics; only the repaired size
summary diagnostic is absent. No dormant import, decoder, lifecycle, fence or
profile operation is activated or hidden. Exact target cleanup passes after
retention. Integration uses matching native inputs, not a new clean CI result;
full MQ lint and Foundation acceptance remain pending.
