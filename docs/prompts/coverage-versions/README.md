# Implementation prompts for IBM coverage minor releases

Status: **Prepared prompts; implementation and release remain separate actions**

Use one prompt from the repository root for the corresponding minor release.
The prompts cover `0.2.0` through `0.17.0`. `1.0.0` is intentionally excluded:
it is a promotion/release gate, not a minor implementation program.

## Prompt index

| Minor | Prompt | Dependency gate |
|---|---|---|
| 0.2.0 | [Implement 0.2.0](IMPLEMENT_0_2_0.md) | none beyond the accepted 0.1.1 baseline |
| 0.3.0 | [Implement 0.3.0](IMPLEMENT_0_3_0.md) | 0.2.0 |
| 0.4.0 | [Implement 0.4.0](IMPLEMENT_0_4_0.md) | 0.3.0 |
| 0.5.0 | [Implement 0.5.0](IMPLEMENT_0_5_0.md) | 0.2.0 |
| 0.6.0 | [Implement 0.6.0](IMPLEMENT_0_6_0.md) | 0.2.0 |
| 0.7.0 | [Implement 0.7.0](IMPLEMENT_0_7_0.md) | 0.2.0 |
| 0.8.0 | [Implement 0.8.0](IMPLEMENT_0_8_0.md) | 0.5.0, 0.6.0, 0.7.0 |
| 0.9.0 | [Implement 0.9.0](IMPLEMENT_0_9_0.md) | 0.4.0, 0.5.0, 0.6.0 |
| 0.10.0 | [Implement 0.10.0](IMPLEMENT_0_10_0.md) | 0.9.0 |
| 0.11.0 | [Implement 0.11.0](IMPLEMENT_0_11_0.md) | 0.5.0, 0.6.0, 0.8.0, 0.10.0 |
| 0.12.0 | [Implement 0.12.0](IMPLEMENT_0_12_0.md) | 0.2.0, 0.4.0, 0.5.0 |
| 0.13.0 | [Implement 0.13.0](IMPLEMENT_0_13_0.md) | 0.12.0 |
| 0.14.0 | [Implement 0.14.0](IMPLEMENT_0_14_0.md) | 0.4.0, 0.5.0, 0.6.0 |
| 0.15.0 | [Implement 0.15.0](IMPLEMENT_0_15_0.md) | 0.4.0, 0.5.0 |
| 0.16.0 | [Implement 0.16.0](IMPLEMENT_0_16_0.md) | 0.8.0, 0.10.0, 0.13.0, 0.14.0, 0.15.0 |
| 0.17.0 | [Implement 0.17.0](IMPLEMENT_0_17_0.md) | 0.11.0, 0.16.0 |

The release dossiers are indexed in
[`docs/delivery/coverage-versions/README.md`](../../delivery/coverage-versions/README.md).
The dependency and concurrency rules are in
[`PARALLEL-IMPLEMENTATION.md`](../../delivery/coverage-versions/PARALLEL-IMPLEMENTATION.md).

## Common execution contract

Every prompt in this directory incorporates this contract by reference. Read it
completely before editing.

### Authority and entry

1. Work from the `mainframe-env` repository root.
2. Read `AGENTS.md` and any more-local instructions when present.
3. Read the target release dossier, this contract, the research roadmap, the
   machine roadmap, accepted ADRs, architecture contracts, version/release
   policy, and relevant package READMEs before editing.
4. Inspect the current source and tests; do not assume the roadmap's snapshot
   still describes the tree exactly.
5. Verify each completion dependency from checked-in evidence. A version may
   prepare private catalogs, parsers, fixtures, and oracle harnesses early only
   where its prompt permits. It may not merge or advertise public behavior
   whose dependency gate has not passed.

### Persistent program control

Before broad implementation, create and maintain:

- `docs/delivery/coverage-versions/status/<target-version>.md`; and
- `conformance/<minor-line>/evidence/program-status.json`, with its schema and
  deterministic validation command.

Record the target/source identity, dependency receipts, current work package,
completed work, commands and exit codes, evidence paths, dirty-tree identity,
open decisions, blockers, and next smallest executable step. Read these ledgers
at every continuation; never restart completed discovery after compaction.

### Implementation discipline

- Work one named work package at a time. Start with a focused failing test or
  inventory/gate failure, implement real behavior, run narrow validation, then
  update derived evidence.
- After a work package passes its focused and affected gates, create one local
  commit before starting the next package. Use a subject beginning
  `Complete <WORK-PACKAGE-ID>` and include these trailers:

  ```text
  Work-Package: <WORK-PACKAGE-ID>=pass
  Target-Version: <target-version>
  Evidence-Digest: sha256:<canonical-work-package-digest>
  ```

  Digest materialization is generated, not typed by hand. The repository-owned
  sealer derives content hashes, projections, and commit trailers and provides a
  deterministic, non-mutating `--check` mode for CI. Keep it a build-hygiene
  tool: GitHub CI status remains the authority for executed tests, and committed
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

- Reuse the accepted 0.2 authorities rather than creating subsystem-private
  variants: the contract/catalog compiler, application-package trust and
  generation runtime, store/migration/artifact adapters, and conformance/oracle
  evidence harness. Extend their typed contracts through their owners.
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
   CardDemo, PostgreSQL, Zowe, dual-target release, or licensed-oracle gates.
2. **Work-package/PR:** run formatting, compile/check, focused tests, every
   affected public-route conformance shard, required negative/condition tests,
   and `git diff --check`. Add restart/rollback/security checks only when the
   change crosses those boundaries. A tooling-only PR runs tooling/schema tests
   and does not rerun application environments.
3. **Minor integration/exit:** after all work packages are integrated, run the
   workspace and the target minor's complete affected-subsystem conformance once
   on one unchanged candidate. Regress prior profiles that consume the changed
   contracts or routes; do not rerun unrelated subsystem matrices.
4. **Nightly/release certification:** run global CardDemo 20/20, PostgreSQL,
   live Zowe, bounded load, backup/restore, dual-target release reproduction,
   cross-subsystem replay, and licensed IBM differential gates. Run this tier
   for 0.16/0.17, release candidates, scheduled integration, or earlier only
   when the changed scope actually affects the corresponding environment.

Unless a target is 0.16/0.17 or its dossier explicitly marks an environment as
affected, references to “full validation” in an individual minor prompt mean
tier-3 complete affected-scope validation, not tier-4 global certification.

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

Implement the target minor only. A future-version capability may not leak into
an earlier public profile. Do not weaken earlier released behavior, rewrite
historical evidence, or silently change a durable/public contract.

This managed implementation program authorizes implementation edits, local
validation, the work-package commits above, pushing only the assigned isolated
implementation branch, and opening one pull request against `main` after the
entire minor exit gate passes. The pull request must list every work-package
commit and evidence digest and must not claim the minor is released.

It does not authorize destructive migration, force-push, merge, tag,
publication, deployment, production cutover, or a compatibility claim. Version
promotion occurs only after the target exit gate passes on the exact candidate;
the controller reviews and merges the pull request separately.

### Stop-the-line and handoff

Stop and record a blocker only when a dependency receipt is invalid, an official
baseline cannot be pinned safely, a required licensed environment is unavailable
for the final gate, a security/durability contract cannot be preserved, a
destructive decision needs authority, or user-owned changes cannot be isolated.
Large scope, failing tests, or incomplete implementation are not blockers.

At handoff, report completed and remaining work packages, exact validation
commands/results, evidence paths, coverage numerators/denominators by gate,
source identity, migrations/rollback status, known limitations, and the next
smallest action. Never report the minor complete unless every exit condition in
its dossier and prompt is satisfied. When complete, push the assigned branch and
open the pull request; include its URL in the final response.
