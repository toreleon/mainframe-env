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
  -> typed Conformance IR case
  -> generated/registered executable test
  -> row-level verdict event
  -> derived coverage ledger
```

This pipeline is the primary conformance authority. CardDemo and other
applications consume it as integration profiles; they are not substitutes for
IBM row-level specifications.

## Minimal typed model

The first version remains deliberately small:

```rust
pub struct ConformanceCase {
    pub row_id: OfficialRowId,
    pub operation: OperationRef,
    pub input: InputShapeRef,
    pub preconditions: Vec<Predicate>,
    pub transition: TransitionRef,
    pub postconditions: Vec<Predicate>,
    pub conditions: Vec<ExpectedCondition>,
    pub recovery: Option<RecoveryInvariant>,
    pub oracle: Option<OracleRef>,
    pub applicable_gates: GateSet,
}
```

The serialized contract carries the equivalent fields:

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
```

References resolve through bounded typed registries owned by the relevant
compiler or subsystem. The IR does not embed Rust source, shell commands,
application names, arbitrary expressions, or duplicated product algorithms.
Add predicate or observation forms only when a real official row requires them.

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

Every behavioral conformance test must register at least one `(row_id, gate)`
binding. One test may cover multiple rows or gates, but every emitted verdict is
explicit. Tests that are purely internal implementation checks need no official
row binding and do not contribute coverage.

The runner emits bounded canonical events:

```rust
pub struct VerdictEvent {
    pub spec_version: SpecVersion,
    pub row_id: OfficialRowId,
    pub gate: CoverageGate,
    pub test_id: TestId,
    pub verdict: Verdict,
    pub observation_digest: ArtifactDigest,
    pub oracle: Option<OracleReceiptRef>,
}
```

`pass` is valid only when the registered test executed successfully on the
candidate. Missing, duplicate, stale-spec, unknown-row, unexecuted, skipped, or
conflicting events fail ledger generation. An oracle reference is mandatory for
`differential=pass`.

## Derived ledger

Coverage ledgers are generated from the immutable official catalog plus verdict
events. They are never edited to mark a row complete. The generator reports:

- denominator per subsystem and gate;
- pass, fail, pending, and non-applicable counts;
- exact test IDs and observation digests behind each claimed gate; and
- conflicts, stale bindings, and rows without executable mappings.

A product or application workload pass may emit row verdicts only through its
registered Conformance IR bindings. A broad workload result alone grants no
official coverage.

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

## Evidence boundary

From 0.3 onward, retained conformance evidence is minimal:

- candidate commit SHA;
- catalog and Conformance IR version/digest;
- canonical row-level verdict stream or its artifact digest;
- derived coverage ledger;
- CI verdict/reference; and
- shipped artifact digest.

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
