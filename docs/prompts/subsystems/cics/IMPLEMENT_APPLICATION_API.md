# Execution Prompt — CICS — Application API

Subsystem: **cics**
Phase: **application-api**

Completion dependencies: cobol.execution, racf.security, dataset.data
Implementation prerequisite: accepted post-review hardening candidate (R-01–R-28)

Use this prompt from the repository root. The
[common execution contract](../README.md#common-execution-contract) is normative.
Apply its [hardened slice acceptance](../README.md#hardened-slice-acceptance),
[early participant contract](../README.md#early-transaction-participant-contract),
and [licensed-harness preparation](../README.md#licensed-harness-preparation)
requirements alongside the version-specific boundaries below.

---

You are implementing **mainframe-env cics.application-api: complete CICS application API** for
all 263 pinned CICS TS 6.x application commands.

## Current entry and frozen CIC-901 boundary

Read the current `docs/delivery/subsystems/cics/application-api-status.md` and
inspect the generated command contract and typed registrations before selecting
work. The integrated registry has 260 `typed-runtime`, zero
`legacy-compatibility` and three `unready` application rows: CICSMESSAGE,
GETNEXT TIMER and ISSUE COPY. These are routing identities, not 260 complete
commands. Do not reopen implemented families from historical checkpoint counts
or admit an unready command without its reviewed slice and applicable gates.

The frozen CIC-901 source boundary covers three independently verified IBM HTML
batches: `sources-a` rows `0001`–`0088`, `sources-b` rows `0089`–`0176` and
`sources-c` rows `0177`–`0263`. Preserve the committed maps, corpora, topic
manifests, extraction plans, generated candidates, independent reviews, 263-row
command contract and compact IR registry. Its initial 3 typed / 20 legacy /
240 unready split and 23 compatibility routes are historical freeze facts;
later family registrations changed runtime readiness without changing the
application denominator.

Use offline pinned `ibm_docs.py search` and `read` before changing semantics.
Verify matching retained HTML before considering missing bytes unavailable.
Follow `AGENTS.md` and `docs/runbooks/IBM-DOCS-CACHE.md`: ordinary work does not
refresh sources. A missing body or TOC remains unavailable; do not re-pin it,
replace it with current web text, or invoke the browser bridge without an
explicit refresh request. A metadata or synthetic transport repository does not
supply missing publication bodies. Reprojection consumes matching local bytes
and must pass its independent verifier.

The generated application contract has no automatic execution authority or
conformance credit. Typed routes require explicit reviewed registrations;
unready rows fail closed, with no default handler or generic-success fallback.
SPI/FEPI identities and existing exact SPI compatibility forms remain outside
the 263-row application registry and digest. Follow the existing routing
contract rather than treating a compatibility descriptor as a new SPI route.

CIC-901 source preparation does not complete cics.application-api. Continue the
current status document's declared CIC-902–CIC-906 slices and retain pending
licensed differentials; an implementation-only waiver grants no licensed credit
or release authorization.

## Read and verify first

Read `docs/prompts/subsystems/README.md`,
`docs/delivery/subsystems/cics/application-api-plan.md`, the generated CICS API catalog,
CICS resource/EIB/condition contracts, and accepted cobol.execution COBOL host ABI, racf.security
SAF, and dataset.data data-authority evidence. Verify all three dependency gates before
public integration.

For CIC-901 specifically, inspect all three source projections and their
independent reviews, plus
`conformance/subsystems/cics/application/generated/cics-application-command-contracts.json` and
`crates/foundation/mainframe-env-ir/src/generated/cics_application_registry.rs`.
Preserve every bounded ambiguity as bounded; a closed row is not necessarily a
fully resolved or executable row.

Also read and verify the current implementation baseline before editing:

- `docs/delivery/subsystems/cics/application-api-status.md`;
- `docs/reviews/SUBSYSTEM-REVIEW.md`;
- `docs/research/cics-behavioral-conformance-pilot.md`;
- `conformance/subsystems/cics/application/cics/pilot-rule-review.json`;
- `conformance/subsystems/cics/application/cics/pilot-fixtures.json`;
- `conformance/subsystems/cics/application/cics/pilot-environment.json`;
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

The deep review records the historical no-go and remains unchanged. The
integrated hardening and CIC-901 freeze are recorded in the current progress
document; do not require its overall status to revert to the historical
**Ready for CIC-901** entry label. Reconcile the 28 finding/regression mappings
and review Gates A–D with the consumed candidate and existing CI references.
Run missing or invalidated checks, preserving valid scoped results under the
existing verification policy. Issue closure, a merged SHA or separate green
branches alone does not establish acceptance. The cobol.execution,
racf.security and dataset.data dependencies and the hardened execution/storage
contracts still apply to each integrating slice.

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
blocker. Never substitute local/model evidence or remove the licensed full-phase
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
cics.application-api minor stays in progress. It authorizes incremental CIC-902–CIC-905 family
work, not release publication, 263-command execution claims, or early credit
against CIC-906 and the full-phase completion gate.

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
  fail explicitly. At full cics.application-api completion, all 263 commands must have sealed
  executable registrations and every accepted option must affect semantics.
- Preserve exact EIB, RESP/RESP2, HANDLE/IGNORE/NOHANDLE and condition behavior
  across normal, failure, cancellation, syncpoint, and restart paths.
- File and security behavior use the dataset.data/racf.security authorities; host calls use the cobol.execution
  ABI. No duplicate resource state or provider-local authorization is allowed.
- APPC/MRO and DPL state is bounded, recoverable where required, and independent
  of application transaction/program names.
- SPI and FEPI completion remain out of scope until cics.system-api.

## Validation by changed boundary

Use the dossier's backend and failure matrix and the common validation tiers.
Run focused family tests during development; run affected public-route and
backend contracts before integrating a slice. These must cover the boundaries
actually changed, including success followed by journal failure, audit
saturation, deny-before-mutation, stale-owner rejection, deadline/cancellation,
restart/resume and retention that preserves replay/checkpoint recovery.

The PostgreSQL durable profile is explicitly affected by cics.application-api. Its shared
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
use its 263 closed contract rows, historical compatibility-route count, current
typed-readiness count or zero-credit source reviews as the completion numerator.

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
