# Execution Prompt — Implement mainframe-env 0.9.0

Target version: **0.9.0**
Completion dependencies: 0.4.0, 0.5.0, 0.6.0
Implementation prerequisite: accepted post-review hardening candidate (R-01–R-28)

Use this prompt from the repository root. The
[common execution contract](README.md#common-execution-contract) is normative.

---

You are implementing **mainframe-env 0.9.0: complete CICS application API** for
all 263 pinned CICS TS 6.x application commands.

## Read and verify first

Read `docs/prompts/coverage-versions/README.md`,
`docs/delivery/coverage-versions/0.9.0.md`, the generated CICS API catalog,
CICS resource/EIB/condition contracts, and accepted 0.4.0 COBOL host ABI, 0.5.0
SAF, and 0.6.0 data-authority evidence. Verify all three dependency gates before
public integration.

Also read and verify the current implementation baseline before editing:

- `docs/delivery/coverage-versions/status/0.9.0.md`;
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

Retest the accepted file/UOW pilot against the hardened execution and storage
contracts. Its twelve obligation credits do not imply twelve complete commands
or completion of any of the six broad work packages.

During entry planning, record the licensed runner availability, exact product
and environment identities, supported families, and missing campaign coverage.
The existing licensed adapter covers the bounded pilot only. Extend its owned
capture/comparison boundary for each new family. If the licensed environment is
unavailable, continue independent implementation after the hardening entry gate,
keep `differential=pending`, and record the final-gate blocker. Never substitute
local/model evidence or remove the licensed completion requirement.

## Implement in this order

1. Freeze **CIC-901** generated grammar, command/option legality, resource keys,
   EIB/RESP/RESP2, conditions, effects, handler registration, and limits. Bind
   authorization, audit, mutation identity, deadlines, recovery and retention
   requirements to those descriptors and the existing owned contracts.
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

Use the common contract's slice commit/sealing rules. Commit each passing slice
before beginning the next dependent slice, and keep the parent in progress
until its declared scope and integrated gates pass. Slice completion grants
only its explicit obligation/gate coverage. Keep unfinished new capabilities
unreachable from the public profile while preserving the accepted pilot and
prior released behavior.

## Reuse and architecture guardrails

- Build the 263-command application API on one shared CICS command runtime that
  owns generated identities, option legality, resource keys, conditions,
  EIB/RESP mapping, bounds, effect metadata, and exhaustive handler closure.
  Later SPI/FEPI work must extend this runtime rather than fork it.
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

- Generate and exhaustively register all 263 commands; every accepted option
  affects semantics and every missing handler fails explicitly.
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
