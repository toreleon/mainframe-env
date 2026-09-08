# Conformance IR and executable coverage

Status: **Normative from 0.3.0**

## Purpose

mainframe-env is a compatibility product. Official IBM catalog rows define the
coverage denominator, but a catalog row is not yet a behavioral specification
and its presence never proves product behavior. From 0.3 onward, one typed
Conformance IR connects each claimed row and gate to executable observations.

```text
pinned IBM source
  -> normalized official row
  -> typed row specification
  -> mandatory executable obligations
  -> generated/registered test bindings
  -> obligation-level verdict events
  -> derived coverage ledger
```

This pipeline is the primary conformance authority. CardDemo and other
applications consume it as integration profiles; they are not substitutes for
IBM row-level specifications.

## Thin typed model

The first version remains deliberately small and has two layers. `RowSpec`
records the IBM-observable contract while `ConformanceCase` records one
executable obligation for one gate:

```rust
pub struct RowSpec {
    pub row_id: OfficialRowId,
    pub operation: OperationRef,
    pub input: InputShapeRef,
    pub preconditions: Vec<PredicateRef>,
    pub transition: TransitionRef,
    pub postconditions: Vec<ObservationRef>,
    pub conditions: Vec<ConditionRef>,
    pub recovery: Option<RecoveryRef>,
    pub oracle: Option<OracleRef>,
    pub applicable_gates: GateSet,
    pub obligations: Vec<ObligationId>,
    pub reviewed_rules: Vec<ReviewedRuleRef>,
}

pub struct ConformanceCase {
    pub row_id: OfficialRowId,
    pub obligation_id: ObligationId,
    pub gate: CoverageGate,
    pub driver: DriverRef,
    pub input: FixtureRef,
    pub preconditions: Vec<PredicateRef>,
    pub expected: Vec<ObservationRef>,
    pub recovery: Option<RecoveryRef>,
    pub oracle: Option<OracleRef>,
    pub reviewed_rule: Option<ReviewedRuleRef>,
    pub scenario: Option<ScenarioId>,
}
```

The serialized row contract therefore retains the required semantic fields:

```text
row_id
operation
input
preconditions
transition
postconditions
conditions
recovery
oracle
applicable_gates
obligations
reviewed_rules
```

References resolve through bounded typed registries owned by the relevant
compiler or subsystem. The IR does not embed Rust source, shell commands,
application names, arbitrary expressions, or duplicated product algorithms.
Add predicate or observation forms only when a real official row requires them.

Publication-derived behavioral rules remain candidates until a maintainer
accepts them. Accepted rules enter the `reviewed_rules` artifact registry by
identity and digest; rows and cases reference that registry. Schema-valid
output, automated extraction, or a model-generated self-review cannot promote
a rule. Changing the accepted review artifact therefore changes the spec digest
and invalidates prior execution evidence.

An obligation is the smallest independently testable requirement needed to
credit one row/gate pair: for example a valid form, a boundary, a documented
condition, or a forbidden mutation. Large IBM rows split into stable mandatory
obligations instead of hiding partial coverage inside one coarse pass. A row's
gate passes only when every mandatory obligation for that gate passes. The
ledger may show obligation progress, but partial obligations never count as a
row pass.

The model is intentionally a typed binding layer, not a second semantic engine.
`DriverRef` invokes the product's selected public or owned subsystem route;
predicate and observation references inspect bounded inputs and results. The IR
must not contain a general predicate language, an arbitrary state-machine DSL,
shell snippets, or a copy of the product transition algorithm.

## Authority separation

Keep four authorities distinct so a generated test cannot prove itself:

- pinned IBM sources and the normalized catalog own the denominator and source
  locator;
- product code owns routing, validation, transitions, conditions, and recovery;
- independently reviewed row specifications, obligations, fixtures, and
  expected observations own what is tested; and
- licensed IBM adapters, reviewed golden data, or explicitly justified
  metamorphic/property relations own oracle results.

Code generation may compile these authorities into registrations and typed
identities, but it may not derive an expected result from the same handler or
transition implementation being tested. Product crates must not inspect
`row_id` or `obligation_id` to select behavior.

## Gate semantics

The six existing gates remain independent:

- `recognized`: the public parser/protocol recognizes the official form;
- `validated`: operands, context, limits, and authority are checked;
- `executed`: the owned semantic transition produces the required effect;
- `conditioned`: documented status, condition, diagnostic, or failure is exact;
- `recovered`: retry, restart, rollback, or reconciliation preserves invariants;
- `differential`: a pinned IBM oracle produces the accepted equivalent result.

`applicable_gates` says which gates the row can eventually satisfy. A gate owned
by a later version remains `pending`; it is not silently marked non-applicable.
For example, 0.3 may pass COBOL recognition and validation while execution,
recovery, and IBM differential remain pending for 0.4 or 0.17.

## Executable binding

Every behavioral conformance test must register at least one
`(row_id, obligation_id, gate)` binding. One reusable driver may execute many
cases and one scenario may observe multiple rows, but every emitted verdict is
explicit. Tests that are purely internal implementation checks need no official
binding and do not contribute coverage.

The runner emits bounded canonical events:

```rust
pub struct VerdictEvent {
    pub spec_version: SpecVersion,
    pub row_id: OfficialRowId,
    pub obligation_id: ObligationId,
    pub gate: CoverageGate,
    pub test_id: TestId,
    pub verdict: Verdict,
    pub observation_digest: ArtifactDigest,
    pub replay: ReplayRef,
    pub oracle: Option<OracleReceiptRef>,
}
```

`pass` is valid only when the registered obligation executed successfully on
the candidate. Missing, duplicate, stale-spec, unknown-row, unknown-obligation,
unexecuted, skipped, or conflicting events fail ledger generation. An oracle
reference is mandatory for `differential=pass`.

Failures identify the row, obligation, gate, IBM source locator, driver/test,
seed or fixture, bounded expected and actual observations, and one deterministic
replay command. Large payloads remain artifacts referenced by digest. Generated
test code must not make failures harder to reproduce than handwritten tests.

## Derived ledger

Coverage ledgers are generated from the immutable official catalog plus verdict
events. They are never edited to mark a row complete. The generator reports:

- denominator per subsystem and gate;
- pass, fail, pending, and non-applicable counts;
- mandatory-obligation progress plus exact test IDs and observation digests
  behind each claimed gate; and
- conflicts, stale bindings, and rows without executable mappings.

A product or application workload pass may emit row verdicts only through its
registered Conformance IR bindings. A broad workload result alone grants no
official coverage.

## Case design and adequacy

Do not enumerate the Cartesian product of every operand and environment. Split
official behavior into equivalence classes and mandatory obligations, then use
reviewed golden examples, boundary cases, properties, pairwise combinations,
and bounded fuzzing as appropriate. Store the seed and minimized failing input
for every generated/property failure.

The conformance harness itself needs mutation adequacy checks. Representative
mutants must prove that cases fail when the product transition is omitted,
generic success replaces a documented condition, an authorization or operand
check is bypassed, a forbidden mutation occurs, or byte/encoding behavior is
wrong. Run the small harness mutation suite when a registry, predicate,
observation, verdict, or ledger contract changes; run broader product mutation
sampling in scheduled integration. Mutation scores are diagnostics, not a new
coverage denominator or release evidence family.

Cross-subsystem workflows use a separate bounded `ScenarioSpec`. A scenario
declares its participating drivers, ordering/failure points, and exact
`(row_id, obligation_id, gate)` credits. Scenario success cannot infer coverage
for rows it did not map, and it does not replace subsystem cases.

A registered Rust `ConformanceScenarioDriver` is the bounded execution
mechanism. It runs the owned product route and returns an observation map keyed
by every exact credit declared by its `ScenarioSpec`. The shared runner requires
set equality before evaluating the ordinary typed observation registry:
missing, duplicate, extra, and unknown per-credit observations fail closed. A
scenario-bound case cannot fall back to the one-case driver path. Scenario
metadata and a scenario-wide success flag therefore cannot emit row credit.

## Commands and feedback tiers

The 0.3 foundation provides these stable entry points:

```text
cargo xtask spec --check
cargo xtask conformance --subsystem <name> [--gate <gate>]
cargo xtask release-certify
```

- `spec --check` compiles catalogs, IR, registries, test bindings, and schemas
  without running product environments and should complete in seconds.
- focused `conformance` runs only the selected subsystem/gates and emits verdict
  events plus a derived ledger.
- `release-certify` runs global integration, recovery, release, and licensed
  oracle gates according to the risk-tiered validation policy.

Cases are deterministically sharded by subsystem, operation family, gate, and
obligation. Exact results may be cached by candidate SHA, catalog/spec digest,
runner version, selected shard, fixture/oracle identity, environment class, and
the digest of a behavior-relevant environment manifest. The manifest covers
store/provider adapters, encoding, resource definition, recoverability,
principal/security configuration, and oracle adapter identity.
The runner must reject incomplete shard sets and never reuse a cached verdict
after any key changes. A ledger declares the complete expected obligation set so
parallel execution cannot silently omit a shard.

Freeze IR v1 before later subsystem lanes merge claims. Prefer additive registry
extensions; an incompatible field or gate semantic requires an explicit spec
version bump. Previously accepted ledgers remain bound to their original spec
and are not mass-migrated merely to match the newest schema.

## Evidence boundary

From 0.3 onward, retained conformance evidence is minimal:

- candidate commit SHA;
- catalog and Conformance IR version/digest;
- canonical obligation-level verdict stream or its artifact digest;
- derived coverage ledger;
- CI verdict/reference; and
- shipped artifact digest.

Each verdict states the candidate, catalog, spec, runner, and environment
manifest identities directly as well as their combined cache identity. Import
recomputes them and rejects any stale or conflicting component.

Migration, recovery, or licensed-oracle receipts are added only when those gates
apply. Do not add per-review schemas, free-form command transcripts, test-count
claims, GitHub job archives, or status-ledger digests. Review findings close by
changing product/spec/tests and producing a new verdict, not by creating a new
`review-repair-round-N` evidence family.

## Model checking boundary

Use TLA+, state-machine exploration, Loom, or another model checker only for a
bounded transition with concurrency, transaction, restart, retry, rollback, or
unknown-outcome behavior. Grammar recognition, catalog completeness, DTO
validation, and straight-line pure semantics use ordinary examples, properties,
and fuzzing instead.

## Migration from 0.2

The 0.2 catalog authority, row identities, six-gate contract, and historical
evidence remain frozen. Do not retrofit 0.2 receipts. Version 0.3 introduces the
shared Conformance IR and begins emitting executable bindings for COBOL rows.
Rows outside the active minor remain pending. Later subsystem minors reuse this
same IR, runner, verdict event, and ledger generator rather than creating local
evidence frameworks.

## Explicit non-goals

Do not build any of the following as part of Conformance IR:

- a general predicate or expression language;
- an arbitrary state-machine/workflow DSL;
- shell execution embedded in specifications;
- a generated test for every unit or internal implementation detail;
- one schema, receipt, or committed verdict file per subsystem, work package,
  review round, row, or obligation;
- TLA+/model checking for catalogs, parsers, or straight-line validation;
- cryptographic notarization of the developer process; or
- a second product router, transaction engine, recovery engine, or semantic
  implementation hidden inside the conformance harness.
