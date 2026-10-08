# Subsystem implementation prompts

Use the prompt for a named subsystem phase from the repository root. Plans,
status, specifications, fixtures, and tests define the work.

## Prompt index

<!-- BEGIN GENERATED SUBSYSTEM INDEX -->
| Subsystem | Phase | Prompt |
|---|---|---|
| Coverage and conformance | Coverage authority | [Implement](coverage/IMPLEMENT_FOUNDATION.md) |
| COBOL | Grammar and types | [Implement](cobol/IMPLEMENT_STRUCTURE.md) |
| COBOL | Execution semantics | [Implement](cobol/IMPLEMENT_EXECUTION.md) |
| RACF / SAF | Commands and authorization | [Implement](racf/IMPLEMENT_SECURITY.md) |
| Datasets / VSAM / AMS | Dataset services | [Implement](dataset/IMPLEMENT_DATA.md) |
| JCL | Converter and planner | [Implement](jcl/IMPLEMENT_PLANNING.md) |
| JES2 and utilities | Jobs, spool and utilities | [Implement](jes/IMPLEMENT_EXECUTION.md) |
| CICS | Application API | [Implement](cics/IMPLEMENT_APPLICATION_API.md) |
| CICS | SPI and FEPI | [Implement](cics/IMPLEMENT_SYSTEM_API.md) |
| z/OSMF | REST portfolio | [Implement](zosmf/IMPLEMENT_REST.md) |
| Db2 | Engine and common SQL | [Implement](db2/IMPLEMENT_CORE.md) |
| Db2 | Complete programming surface | [Implement](db2/IMPLEMENT_PROGRAMMING.md) |
| IMS | DB / TM programming surface | [Implement](ims/IMPLEMENT_PROGRAMMING.md) |
| IBM MQ | MQI programming surface | [Implement](mq/IMPLEMENT_PROGRAMMING.md) |
| Cross-resource integration | Transactions and recovery | [Implement](integration/IMPLEMENT_TRANSACTIONS.md) |
| Licensed certification | Differential certification | [Implement](certification/IMPLEMENT_LICENSED.md) |
<!-- END GENERATED SUBSYSTEM INDEX -->

The subsystem plans are indexed in
[`docs/delivery/subsystems/README.md`](../../delivery/subsystems/README.md).
The dependency and concurrency rules are in
[`DEPENDENCIES.md`](../../delivery/subsystems/DEPENDENCIES.md).

## Common execution contract

Every prompt in this directory incorporates this contract by reference. Read it
completely before editing.

### Authority and entry

1. Work from the `mainframe-env` repository root.
2. Read `AGENTS.md` and any more-local instructions when present.
3. Read the target subsystem phase plan and progress record, this contract, the research roadmap, the
   machine roadmap, accepted ADRs, architecture contracts, subsystem tracking
   policy, and relevant package READMEs before editing.
4. Inspect the current source and tests; do not assume the roadmap's snapshot
   still describes the tree exactly.
5. Verify each completion dependency using current checks. A subsystem may
   prepare private catalogs, parsers, fixtures, and oracle harnesses early only
   where its prompt permits. It may not merge or advertise public behavior
   whose dependency gate has not passed.

Dependency acceptance follows the
[recorded licensed-pending dispositions](../../delivery/subsystems/README.md#recorded-licensed-pending-dispositions).
An accepted implementation baseline is not a licensed differential pass or
release authorization. Verify the actual consumed candidate and receipt; a
historical branch base, merged PR, or version label alone is insufficient.

### Persistent program control

Before broad implementation, maintain the concise human-readable
`docs/delivery/subsystems/<subsystem>/<phase>-status.md`. Record the
candidate/dependency identity, current and completed work packages, blockers,
decisions, and next executable step. Do not turn controller state into product
conformance evidence.

From cobol.structure onward, do not create a version-specific status schema, command-result
ledger, dirty-tree receipt, or `review-repair-round-N` evidence/schema family.
The documentation manifest provides the shared subsystem mapping outside the
coverage ledger. Automation must not create another controller ledger. Read the status at every continuation and
never restart completed discovery after compaction.

### Conformance-driven COBOL structure workflow

Read and follow [`CONFORMANCE-IR.md`](../../architecture/CONFORMANCE-IR.md).
The coverage.foundation catalogs and row identities define the denominator; the shared typed
Conformance IR defines behavior and binds claimed gates to executable tests.

- Each claimed official row has a typed specification with `row_id`, operation,
  input, preconditions, transition, postconditions, conditions, recovery,
  oracle, applicable gates, and mandatory obligation IDs. Split a coarse row
  into the smallest independently testable valid, boundary, condition, or
  forbidden-mutation obligations; partial obligations never pass the row.
- Every behavioral conformance test binds at least one
  `(row_id, obligation_id, gate)` and emits a bounded canonical verdict event.
  Internal unit tests without an official binding remain useful but contribute
  no official coverage.
- Keep the IR a thin typed binding layer. Drivers invoke product-owned routes
  and expectations use bounded registry references; do not duplicate product
  algorithms in a general predicate, expression, transition, or workflow DSL.
- Keep source/catalog, product behavior, test expectation, and licensed/golden
  oracle authorities distinct. Never generate expected results from the same
  product handler being tested, and never let product behavior dispatch on row
  or obligation IDs.
- Generate the coverage ledger from the official catalog and verdict events.
  Never edit pass counts or infer row completion from a broad workload result.
- CardDemo and other applications are integration profiles. They may exercise
  registered row/obligation bindings but are not the primary IBM conformance
  model.
- Provide fast `spec --check`, focused subsystem/gate conformance, and
  public-distribution certification entry points. Keep the agent inner loop on the
  first two tiers.
- Design cases with equivalence classes, mandatory boundaries, properties,
  pairwise combinations, and bounded fuzzing instead of a Cartesian-product
  test explosion. Deterministically shard and exactly cache by candidate,
  spec/runner, fixture/oracle, environment, subsystem, family, gate, and
  obligation identity; fail if any expected shard or obligation is absent.
- Add representative harness mutation checks for omitted transitions, generic
  success, bypassed validation/authorization, forbidden mutation, and byte or
  encoding errors. Mutation scores are diagnostic, not coverage evidence.
- Use a separate bounded `ScenarioSpec` for cross-subsystem workflows. It may
  credit only explicitly mapped `(row_id, obligation_id, gate)` bindings.
- Every failure reports source locator, row/obligation/gate, driver/test, seed or
  fixture, bounded expected/actual observations, and a deterministic replay
  command.
- Use model checking only for bounded concurrency, transaction, restart,
  retry, rollback, or unknown-outcome transitions.
- Retain only candidate SHA, spec/catalog identity, verdict/ledger artifact,
  CI verdict/reference, and shipped artifact digest, plus recovery/oracle
  receipts when those gates apply.

Freeze the minimal IR v1 before dependent lanes merge claims. Prefer additive
typed-registry extensions, explicitly version incompatible semantic changes,
and keep accepted historical ledgers on the spec version that produced them.
Do not add per-row/obligation committed verdict files, subsystem-local IRs,
shell-in-spec, general formal languages, or generated tests for internal details.

### Hardened slice acceptance

For cics.application-api–certification.licensed, bind each slice to the current accepted baseline and existing
[execution/durability](../../architecture/EXECUTION-AND-DURABILITY.md),
[security](../../architecture/PLUGIN-AND-SECURITY.md),
[canonical effect](../../contracts/EFFECT-CANONICAL-V1.md),
[object-row persistence](../../contracts/PROVIDER-ROW-PERSISTENCE-V1.md), and
[durable storage](../../contracts/DURABLE-STORAGE-PROFILE.md) contracts.
Preserve the accepted HIR/MIR, coordinator, host ABI and package topology; these
phases extend their owners rather than introduce another universal IR,
provider-private coordinator, store, security evaluator, or evidence framework.

Before implementation, declare the slice's exact catalog rows and mandatory
obligations, execution-context applicability, contract/module owners, affected
public routes and backend matrix in the existing target status document. Review
applicability against pinned sources before claiming a gate: a genuinely
inapplicable gate needs a source-backed disposition, while a required but
unsupported operation remains pending. Neither blanket six-gate requirements
for non-behavioral rows nor retrospective exclusions may distort the denominator.
Correct rejection of a forbidden context is a condition obligation, not proof
of required execution in an allowed context.

Each mutating slice must demonstrate its applicable guarantees before integration:

- Typed SAF resource/intent decisions precede mutation; deny and failure paths
  are audited. Audit/effect/lifecycle results commit atomically wherever they
  share store authority; audit saturation and journal failure are explicit.
- Canonical operation/content identities, bounded replay and idempotency,
  finite deadlines, live cancellation, durable clocks, lease epochs and fencing
  preserve the existing contract. Post-dispatch uncertainty remains an explicit
  unknown outcome for service-specific fenced reconciliation, never automatic
  mutation redispatch.
- Versioned object rows, schema readers, migrations, rollback, restart and
  backup/restore preserve retained checkpoint/artifact/audit/replay references.
  Retention watermarks and idempotency lifetimes are declared and tested.
- Independent expectations exercise the selected public product route and all
  affected backend contracts. Direct handler tests supplement that proof;
  memory-only tests earn no durable process-restart credit. A skipped required
  environment test remains pending, not a pass.

Declare bounded family slices and preserve the existing parent work-package IDs
and sealing rules. The final family/integration milestone combines and stresses
these guarantees; it is not their first implementation or test. Use the existing
CI selectors, Conformance IR and generated coverage pipeline, not another ledger.
Keep this acceptance rule here rather than copying competing versions into each
provider. Architecture evolution proposals do not require a runtime rewrite
before the next bounded delivery slice.

### Early transaction participant contract

Before dependent CICS, Db2, IMS or MQ participants integrate, the shared contract
owner must freeze the minimum participant boundary and its executable tests:
transaction/syncpoint owner; supported local/distributed modes and capabilities;
prepare applicability; commit/rollback behavior; compensation limits; heuristic,
in-doubt and unknown outcomes; idempotency scope/lifetime; lock/effect ordering;
fencing; deadline/cancellation; and durable recovery/schema ownership.

Review what the accepted coordinator already supplies and extend it additively.
Declare the early contract slice under INT-1601 in the existing status mechanism;
it is a prerequisite to dependent adapter integration, not a claim that all of
integration.transactions has passed. Each participant supplies minimum mutation, failure, replay and
restart evidence when its slice lands. Full mixed-resource combinations,
operator resolution and coherent backup/restore close in integration.transactions. Do not require
universal prepare, rollback, two-phase commit or exactly-once behavior where the
pinned execution context does not support it.

### Licensed-harness preparation

Prepare CER-1701 environment/adapter inputs alongside each provider lane rather
than first discovering oracle prerequisites at certification.licensed. Record pinned product and
service levels, authorized access, capabilities, independent fixtures, capture,
reviewed normalization, bounds and representative harness checks in existing
status/spec/fixture authorities. Keep proprietary media, credentials and raw
licensed outputs outside production and public release closure.

Local fixtures validate harness plumbing only. Earlier licensed observations
retain their original candidate/environment identities; they do not certify a
later candidate. All phase-specific licensed completion requirements and the final
certification.licensed campaigns remain binding. Missing environments stay explicit blockers to
those gates, not fabricated passes or new blanket implementation exceptions.
Source, spec, fixture, oracle or environment changes invalidate affected receipts
under the existing candidate policy; any permitted reuse needs explicit identity
and compatibility proof, never silent relabeling of an old receipt.

### Implementation discipline

- Work one named work package at a time. Start with a focused failing test or
  inventory/gate failure, implement real behavior, run narrow validation, then
  update derived evidence.
- After a work package passes its focused and affected gates, create one local
  commit before starting the next package. Use a subject beginning
  `Complete <WORK-PACKAGE-ID>` and include these trailers:

  ```text
  Work-Package: <WORK-PACKAGE-ID>=pass
  Target-Subsystem: <target-subsystem>
  Evidence-Digest: sha256:<canonical-work-package-digest>
  ```

  Digest materialization is generated, not typed by hand. The repository-owned
  sealer derives content hashes, projections, and commit trailers and provides a
  deterministic, non-mutating `--check` mode for CI. Keep it a build-hygiene
  tool: local Jenkins status remains the authority for executed tests, and committed
  evidence must not duplicate free-form command results, test counts, or a
  tamper-resistant remote attestation system.

  Use exact artifact allowlists, avoid self-referential commit identities, and
  have callbacks emit Git-derived JSON for the controller to store outside the
  repository. Do not add network authentication frameworks, capability
  filesystems, or adversarial multi-user defenses unless the product's actual
  deployment threat model later requires them. Literal digests are permitted
  only for immutable imported-source identities and reviewed golden/oracle
  fixtures with a named generator or verifier.

  Do not combine two named work packages in one completion commit. Focused
  repair commits are allowed but do not replace the completion commit.
- A dossier may divide a large work package into bounded slices while retaining
  its parent milestone. Declare each slice ID, parent, semantic scope,
  dependencies and acceptance gates in the existing human-readable status
  document before implementation. IDs such as `CIC-903.file-update` are accepted
  by the shared sealer. Seal a passing slice with its own ID using the same
  subject/trailers and exact changed-path allowlist above; never use the parent
  ID to certify a partial slice. Keep the parent in progress until every slice
  and its integrated gates pass, then create the parent's completion commit
  with generated aggregate evidence. Slice bookkeeping does not change the
  official coverage denominator or create a new evidence schema/ledger family.
- Preserve user-owned changes and keep unrelated files out of the task diff.
- Production behavior must be generic and selected through typed contracts.
  Do not add application-name, program-name, table-name, transaction-name,
  dataset-name, queue-name, map-name, or principal-name dispatch.
- Generated catalogs prove identity and exhaustiveness, not semantic completion.
  A row is complete only when every applicable `recognized`, `validated`,
  `executed`, `conditioned`, `recovered`, and `differential` gate passes.
- Do not ship placeholders, `todo!()`, generic-success handlers, silent operand
  drops, test-only authorities, fixture-output shortcuts, or fallback to oracle
  binaries/native IBM code.
- Keep deterministic semantic kernels separate from asynchronous infrastructure
  and keep one authority for each public route, mutable state, security decision,
  transaction protocol, generated catalog, and durable schema.
- Bound input, output, collections, queues, recursion, concurrency, retries,
  retained state, and evidence. Fail closed on malformed, unauthorized,
  unsupported, unavailable, saturated, cancelled, or indeterminate operations.
- Never fabricate official coverage or licensed IBM differential results. Mark
  `differential=pending` until a pinned licensed oracle actually passes.
- Add migrations, backup/restore, restart, rollback, compatibility, and
  corruption/failure evidence whenever durable state or public schemas change.

### Reuse and build-versus-buy discipline

IBM-observable language, subsystem, return-code, condition, recovery, and
failure semantics are product behavior and remain owned by mainframe-env.
Commodity infrastructure and standards implementations are not product
semantics. Before implementing either category, classify the boundary and
record the decision in the target status ledger.

- Reuse the accepted coverage.foundation authorities rather than creating subsystem-private
  variants: the contract/catalog compiler, application-package trust and
  generation runtime, store/migration/artifact adapters, and the cobol.structure shared
  Conformance IR/compiler/runner/ledger pipeline. Extend their typed contracts
  through their owners.
- One readable normative catalog or schema must generate all applicable Rust
  identities/descriptors, registry tables, schema/OpenAPI artifacts, coverage
  rows, documentation, and handler-closure inputs. Do not hand-maintain parallel
  inventories or make generated Rust the only statement of the contract.
- Compile every normative JSON Schema with a reviewed Draft 2020-12 validator
  and validate every mapped artifact. A schema-to-Rust generator may reduce DTO
  boilerplate, but generated types never replace semantic constructors, bounds,
  cross-reference checks, or negative validation.
- Use reviewed standards libraries at adapter boundaries for COSE/signatures,
  decimal primitives, object storage, HTTP/OpenAPI, secret handling,
  observability, model checking, fuzzing, SBOMs, and supply-chain verification
  when they satisfy the pinned contract. Do not implement cryptographic
  primitives, cloud object-store clients, OpenAPI models, or SBOM formats in
  product code.
- Third-party AST, query-plan, workflow, policy, telemetry, serializer, and
  framework types must be converted immediately to owned bounded DTOs. They may
  not enter stable contracts, semantic identities, durable state, checkpoints,
  evidence schemas, or public compatibility claims.
- An external SQL, broker, policy, workflow, storage, or scheduler engine may be
  a replaceable substrate or oracle adapter only after a representative
  semantic-gap matrix passes. It never receives implicit authority for Db2,
  MQI, RACF/SAF, JES, CICS, IMS, COBOL, or cross-resource outcomes.
- Do not add a second authority for routing, scheduling, transactions,
  persistence, migrations, security, package selection, or evidence. If a
  framework would own one of those concerns, integrate it behind the existing
  port or record an ADR explaining why the authority boundary must change.
- Before adding a production dependency or external runtime, record its exact
  version, license, MSRV/platform support, maintenance status, enabled features,
  transitive/build dependencies, failure model, deterministic-test strategy,
  semantic gaps, and removal/fallback plan. Run the dependency, advisory,
  license, and source gates. Do not add speculative dependencies.
- Prefer a focused spike with frozen fixtures over a framework-wide adoption.
  Reject the framework when the compatibility adapter is larger, less bounded,
  less deterministic, or harder to validate than the owned implementation.
- External tools may automate licensed differential execution, release
  evidence, provenance, or transport. They do not contribute coverage by their
  presence and may not become a native-code fallback for product execution.

### Risk-tiered validation and evidence

Conformance protects IBM-observable product semantics; it is not a gate on every
developer edit. Classify the change before selecting validation. A change to
language/runtime behavior, a public contract, durable state, transactions,
security, restart/recovery, or a selected product route is semantic. A change
limited to documentation, generated evidence, CI plumbing, or repository
tooling is non-semantic unless it changes what the product ships or claims.

Use these validation tiers:

1. **Inner loop:** run the smallest focused package tests and affected schema,
   inventory, architecture, or conformance shard. Do not run full workspace,
   CardDemo, PostgreSQL, Zowe, dual-subsystem phase, or licensed-oracle gates.
2. **Work-package/PR:** run formatting, compile/check, focused tests, every
   affected public-route conformance shard, required negative/condition tests,
   and `git diff --check`. Add restart/rollback/security checks only when the
   change crosses those boundaries. A tooling-only PR runs tooling/schema tests
   and does not rerun application environments.
3. **Phase integration/exit:** after all work packages are integrated, run the
   workspace and the target phase's complete affected-subsystem conformance once
   on one unchanged candidate. Regress prior profiles that consume the changed
   contracts or routes; do not rerun unrelated subsystem matrices.
4. **Nightly/release certification:** run global CardDemo 20/20, PostgreSQL,
   live Zowe, bounded load, backup/restore, dual-subsystem phase reproduction,
   cross-subsystem replay, and licensed IBM differential gates. Run this tier
   for integration.transactions/certification.licensed, release candidates, scheduled integration, or earlier only
   when the changed scope actually affects the corresponding environment.

Unless a target is integration.transactions/certification.licensed or its dossier explicitly marks an environment as
affected, references to “full validation” in an individual phase prompt mean
tier-3 complete affected-scope validation, not tier-4 global certification.
These tiers guide local validation; they do not waive unconditional policy
checks or affected-environment gates selected by the repository CI plan.

Maintain an explicit path/contract-to-gate map and fail closed when affected
scope is ambiguous. Never skip the focused malformed, condition/status,
authorization, failure, and forbidden-state-mutation tests for changed product
semantics. Direct unit tests supplement rather than replace public selected-route
evidence for behavior that changed.

Generate evidence once from the final accepted run for the applicable tier.
Evidence records identity and results but does not make a gate pass; CI and the
actual test process remain authoritative. Do not repeat an expensive successful
gate merely to refresh hashes, prose, receipts, or commit metadata.

### Change and release boundary

Keep changes scoped to the owning subsystem and consumed shared contracts.
Preserve tests, specifications, fixture identities, and compatibility schemas.
Add an isolated changelog fragment and regenerate documentation. Keep execution
receipts outside Git and report current checks and pending environments in the
handoff. Release tags and published assets do not manage subsystem progress.

### Stop-the-line and handoff

Stop and record a blocker only when a dependency receipt is invalid, an official
baseline cannot be pinned safely, a required licensed environment is unavailable
for the final gate, a security/durability contract cannot be preserved, a
destructive decision needs authority, or user-owned changes cannot be isolated.
Large scope, failing tests, or incomplete implementation are not blockers.

At handoff, report completed and remaining work packages, exact validation
commands/results, evidence paths, coverage numerators/denominators by gate,
source identity, migrations/rollback status, known limitations, and the next
smallest action. Never report the phase complete unless every exit condition in
its dossier and prompt is satisfied. When complete, push the assigned branch and
open the pull request; include its URL in the final response.
