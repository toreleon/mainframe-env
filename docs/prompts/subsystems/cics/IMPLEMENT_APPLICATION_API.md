# Execution Prompt — CICS — Application API

Subsystem: **cics**
Phase: **application-api**

Target version: **0.9.0**
Completion dependencies: cobol.execution, racf.security, dataset.data
Implementation prerequisite: accepted post-review hardening candidate (R-01–R-28)

Use this prompt from the repository root. The
[common execution contract](../README.md#common-execution-contract) is normative.
Apply its [hardened slice acceptance](../README.md#hardened-slice-acceptance),
[early participant contract](../README.md#early-transaction-participant-contract),
and [licensed-harness preparation](../README.md#licensed-harness-preparation)
requirements alongside the version-specific boundaries below.

---

You are implementing **mainframe-env 0.9.0: complete CICS application API** for
all 263 pinned CICS TS 6.x application commands.

## Current CIC-901 boundary

CIC-901 is an incremental, non-release boundary already frozen from three
independently verified IBM HTML source batches: `sources-a` rows `0001`–`0088`,
`sources-b` rows `0089`–`0176`, and `sources-c` rows `0177`–`0263`. Treat the
committed maps, corpora, topic manifests, extraction plans, generated
candidates, automatic review receipts, 263-row command contract, and compact IR
registry as the current authority. Fetch missing or changed IBM material as
fresh HTML through the repository browser-fetch bridge and the user's Chrome
session, update the content-addressed cache, then reproject and independently
verify it. Do not use PDF or introduce a human-only approval gate.

The frozen registry shape does not mean 263 executable commands. Its readiness
split is exactly 3 `typed-runtime`, 20 `legacy-compatibility`, and 240 `unready`
API rows. Only the 23 existing API routes are advertised. Automatic
registration remains disabled, there is no default handler or generic-success
fallback, and unready rows fail explicitly. SPI and FEPI identities cannot
enter this application registry or be treated as application routes.

Within that unchanged split, the existing time handler backs the official
`ASKTIME ABSTIME` form, which returns the packed-decimal destination. Bare
`ASKTIME` is unready until its distinct EIBDATE/EIBTIME updates are implemented.
One generated compiler-only compatibility descriptor preserves exactly
`INQUIRE PROGRAM` on the pre-existing raw `Inquire` route. It remains bound to
SPI row `0155` outside the 263-row application registry and digest; it must not
admit `SET FILE`, another `INQUIRE` form, unknown options, or an application
candidate selected by an application discriminator.

CIC-901 carries zero execution, coverage, semantic, conformance, and licensed
differential credit. It establishes the source-backed contract needed for
CIC-902–CIC-905 vertical family slices; it does not make 0.9.0 release-ready.
Do not add release automation, nested environment orchestration, or a broad licensed campaign
at this boundary. Licensed evidence remains mandatory at the full 0.9
completion gate after the applicable executable families exist.

## Read and verify first

Read `docs/prompts/subsystems/README.md`,
`docs/delivery/subsystems/cics/application-api-plan.md`, the generated CICS API catalog,
CICS resource/EIB/condition contracts, and accepted 0.4.0 COBOL host ABI, 0.5.0
SAF, and 0.6.0 data-authority evidence. Verify all three dependency gates before
public integration.

For CIC-901 specifically, inspect all three source projections and their
independent reviews, plus
`conformance/0.9/generated/cics-application-command-contracts.json` and
`crates/foundation/mainframe-env-ir/src/generated/cics_application_registry.rs`.
Preserve every bounded ambiguity as bounded; a closed row is not necessarily a
fully resolved or executable row.

Also read and verify the current implementation baseline before editing:

- `docs/delivery/subsystems/cics/application-api-status.md`;
- `docs/reviews/PRE-0.9.0-DEEP-REVIEW.md`;
- `docs/research/cics-behavioral-conformance-pilot.md`;
- `conformance/0.9/cics/pilot-rule-review.json`;
- `conformance/0.9/cics/pilot-fixtures.json`;
- `conformance/0.9/cics/pilot-environment.json`;
- `docs/runbooks/cics-licensed-pilot.md`;
- `docs/contracts/EFFECT-CANONICAL-V1.md`;
- `docs/contracts/PROVIDER-ROW-PERSISTENCE-V1.md`;
- `docs/contracts/DURABLE-STORAGE-PROFILE.md`;
- `docs/architecture/EXECUTION-AND-DURABILITY.md`;
- `docs/architecture/PLUGIN-AND-SECURITY.md`;
- `docs/decisions/0009-current-package-topology.md`;
- `docs/runbooks/OPERATIONS.md`;
- `docs/runbooks/CAPACITY-AND-RECOVERY.md`; and
- `docs/delivery/VERIFICATION-STRATEGY.md`.

The deep review records the historical no-go and remains unchanged. Current
entry authority is the status document: broad CIC-901 work requires **Ready
for CIC-901**, backed by all 28 findings resolved and integrated, one fix commit
per finding, focused regression references, and passing review Gates A–D on one
unchanged candidate. Record that candidate's SHA/tree and dependency/CI
references in the status document. Issue closure or separate green branches
alone do not establish the entry gate. The existing 0.4/0.5/0.6 dependency
receipts remain required; the hardening candidate is an additional prerequisite.

PR [#131](https://github.com/toreleon/mainframe-env/pull/131) merged as
`8b7459ab9d9c23e3b872314323d3e30020e13f31`. First reconcile its existing fix and
regression mappings with the live status and integrated CI receipts; do not
restart all 28 remediations merely because the status is stale. Verification
is tracked in [#137](https://github.com/toreleon/mainframe-env/issues/137).
Retain valid exact-candidate receipts under existing policy and run missing or
invalidated entry checks. The merged SHA is not automatically the accepted SHA.

Verify the accepted file/UOW pilot evidence against the hardened execution and
storage contracts; rerun checks only when their evidence is missing or invalidated. Its twelve obligation credits do not imply twelve complete commands
or completion of any of the six broad work packages.

Do not block CIC-901 contract freeze on licensed-runner availability or start a
broad campaign before executable family slices exist. When CIC-902 and later
slices make a family executable, record runner availability, exact product and
environment identities, supported families, and missing campaign coverage. The
existing licensed adapter covers the bounded pilot only; extend its owned
capture/comparison boundary for each implemented family. If the licensed
environment is unavailable, continue independent implementation after the
hardening entry gate, keep `differential=pending`, and record the final-gate
blocker. Never substitute local/model evidence or remove the licensed full-minor
completion requirement.

## Implement in this order

1. Preserve the frozen **CIC-901** generated grammar, command/option legality,
   resource keys, EIB/RESP/RESP2, conditions, effects, fail-closed registration
   shape, and limits. Reproject and regenerate it only when its pinned IBM HTML
   or owned contracts change; do not convert unready identities into handlers
   outside a reviewed vertical slice.
2. Implement **CIC-902/CIC-903** program/task/interval/storage/recovery,
   terminal/BMS, TSQ/TDQ, file, journal, and spool command families.
3. Implement **CIC-904** channels/containers, BTS, documents, web/HTTP,
   transforms, business transactions, and supported event APIs.
4. Implement **CIC-905** APPC/MRO conversation state and distributed program
   link through transport-neutral contracts.
5. Complete **CIC-906** integrated cross-family SAF, concurrency, syncpoint,
   cancellation, recovery, scale, compatibility, and licensed differential
   campaigns, including the exact final candidate.

Every CIC-902–CIC-905 slice must implement and test its applicable authorization,
audit, cancellation/deadline, condition, bounds, mutation/replay, and recovery
behavior before integration. CIC-906 combines and stresses those guarantees;
it is not their first implementation or first test.

## Reviewable slices

Keep CIC-901–CIC-906 as parent milestones. Before starting a parent, declare its
bounded slices in the status document with a stable ID, command/option and
row/obligation scope, owning modules, dependencies, and acceptance gates. For
example, separate file update, file browse, TSQ, and TDQ work under CIC-903;
split further when a slice exceeds the repository's module review budget.

The sealed CIC-901 contract/registry boundary may remain complete while the
0.9.0 minor stays in progress. It authorizes incremental CIC-902–CIC-905 family
work, not release publication, 263-command execution claims, or early credit
against CIC-906 and the full-minor completion gate.

Use the common contract's slice commit/sealing rules. Commit each passing slice
before beginning the next dependent slice, and keep the parent in progress
until its declared scope and integrated gates pass. Slice completion grants
only its explicit obligation/gate coverage. Keep unfinished new capabilities
unreachable from the public profile while preserving the accepted pilot and
prior released behavior.

## Execution-context and selected-route proof

CIC-901 must freeze command/option applicability by execution context, including
local tasks and DPL server programs. CIC-905 binds the pinned restricted DPL
API, syncpoint ownership, `SYNCONRETURN`, return and failure behavior to explicit
obligations; transport success cannot prove transaction semantics.

Each command-family slice requires at least one independent end-to-end proof
through compiled COBOL, the accepted host ABI, durable coordinator and selected
CICS provider route, plus its focused command/condition obligations. A direct
handler call, mocked host result or the pilot's twelve scoped credits cannot
substitute for this route proof or complete a whole family.

## Reuse and architecture guardrails

- Build the 263-command application API incrementally on one shared CICS command
  runtime. CIC-901 owns generated identities, option legality, resource keys,
  conditions, EIB/RESP mapping, bounds, effect metadata, and the complete
  fail-closed registry shape. Executable handler closure arrives only through
  sealed CIC-902–CIC-905 vertical slices; no default handler, automatic
  registration, or generic-success route may stand in for it. Later SPI/FEPI
  work must extend this runtime rather than fork it.
- Keep descriptors and dispatch shared, with semantic handlers in stable
  command-family modules. Follow the R-26 module boundaries and the roughly
  800–1,200-line review trigger; do not expand one service module to hold all
  263 commands. Document new public contract items under the R-27 ratchet.
- Reuse Tower/HTTP and reviewed transport libraries for web, sockets-facing,
  timeout, limit, and tracing adapters. Convert immediately to typed CICS
  requests; transport libraries never define CICS conditions or transaction
  semantics.
- Reuse the accepted dataset, SAF, program, session, checkpoint, UOW, migration,
  package, and evidence authorities. TSQ/TDQ, file, journal, spool, and program
  commands must not hide provider-private stores or retry policies.
- Online execution and resume use the durable coordinator and validated
  checkpoints. Keep `UnknownOutcome` explicit through CICS and gateway mapping;
  post-dispatch uncertainty goes to fenced, service-specific reconciliation,
  never automatic mutation redispatch.
- Derive capabilities from verified typed operations and authorize each
  sensitive resource and intent through SAF before dispatch. Persist typed
  audit decisions with effect results and lifecycle events atomically wherever
  they share store authority, including deny and failure paths.
- Reuse bounded worker lanes, the durable clock and lease epochs, live request
  cancellation, finite deadlines, expiring authentication sessions, actual
  secret-reference resolution, and truthful readiness. New background CICS
  work must participate in worker health, admission and graceful shutdown.
- Persist versioned object rows with scoped atomic mutations; keep immutable
  executable content identity distinct from semantic identity. Use the shared
  artifact adapter for PostgreSQL. Declare schema/read-version compatibility,
  migration and rollback for changed state, plus retention watermarks and
  idempotency lifetimes that protect live checkpoints, audits and recovery.
- APPC/MRO and distributed-link behavior remains a transport-neutral owned
  protocol state machine. Do not substitute a message broker's delivery or
  acknowledgement semantics for the pinned CICS contract.

## Version-specific invariants

- At CIC-901, generate all 263 registry shapes and make every unready handler
  fail explicitly. At full 0.9 completion, all 263 commands must have sealed
  executable registrations and every accepted option must affect semantics.
- Preserve exact EIB, RESP/RESP2, HANDLE/IGNORE/NOHANDLE and condition behavior
  across normal, failure, cancellation, syncpoint, and restart paths.
- File and security behavior use the 0.6/0.5 authorities; host calls use the 0.4
  ABI. No duplicate resource state or provider-local authorization is allowed.
- APPC/MRO and DPL state is bounded, recoverable where required, and independent
  of application transaction/program names.
- SPI and FEPI completion remain out of scope until 0.10.

## Validation by changed boundary

Use the dossier's backend and failure matrix and the common validation tiers.
Run focused family tests during development; run affected public-route and
backend contracts before integrating a slice. These must cover the boundaries
actually changed, including success followed by journal failure, audit
saturation, deny-before-mutation, stale-owner rejection, deadline/cancellation,
restart/resume and retention that preserves replay/checkpoint recovery.

The PostgreSQL durable profile is explicitly affected by 0.9. Its shared
artifact, concurrent-owner and restart tests are required at minor integration;
a skipped environment test cannot satisfy them. Memory tests establish
determinism and invariants but carry no durable process-restart credit.

Extend the existing R-24 fuzz, concurrency/model and coverage infrastructure for
new CICS parser/decoder and transaction/recovery boundaries. Record its actual
CICS scope; existing compiler/store results do not prove CICS coverage. Preserve
the discovered tooling/PostgreSQL suites, schema parity, architecture/docs/API
ratchets, full-workspace MSRV, and supply-chain/license gates through their
existing CI selectors. Coverage percentages remain diagnostic, not IBM credit.

## Completion gate

The CIC-901 non-release boundary does not weaken or satisfy this gate. Do not
use its 263 closed contract rows, 23 advertised compatibility routes, or zero-
credit source reviews as the completion numerator.

Do not finish until 263/263 application commands pass all applicable coverage
gates; option, EIB/response, condition, terminal, resource, conversation,
syncpoint, cancellation, malformed, bound, authorization and recovery matrices
pass on the declared backend/environment matrix; licensed CICS TS 6.x
application-interface differentials cover all applicable command obligations
and pass on the exact candidate; and
CardDemo transactions/maps/resources remain exact without production hardcode.

At handoff, provide per-family/per-gate counts, generated catalog and registry
digests, failure/recovery and oracle evidence, and full validation on the exact
candidate. Do not include SPI/FEPI rows in the completion numerator.
