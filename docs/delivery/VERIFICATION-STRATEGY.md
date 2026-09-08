# 0.1 Verification Strategy

Status: **Accepted by repository owner**
Owner: **verification maintainers**
Scope: **verification layers, assurance tiers, and required evidence**
Applies from: **mainframe-env 0.1.0**

## Objective

Prove the 0.1 COBOL/CICS/JCL/JES/dataset/RACF/z/OSMF surface through independent
layers of evidence. Test count or line coverage alone does not establish
semantic correctness or operational robustness.

## Verification layers

### Unit and invariant tests

Cover identifiers, bounds, validation, encoding, decimal operations, storage
layouts, conditions, state transitions, effect sequencing, and schema upgrades.

### Golden fixtures

Freeze representative:

- COBOL source/copybook/preprocessor inputs and diagnostics;
- data layouts, aliases, decimal and encoding results;
- CICS request/outcome/EIB/condition/terminal/file traces;
- JCL procedure, DD, condition, step, job and spool behavior;
- dataset catalog and record operations;
- RACF authentication/SAF decisions and audit redaction; and
- z/OSMF request/response/status/error compatibility.

Golden output includes an explicit schema and fixture identity. Reviewers can
distinguish an intentional update from regenerated noise.

### Differential verification

The current OpenMainframe workspace runs out of process or through a dedicated
test-only oracle adapter. mainframe-env production packages do not import it.

Compare semantic observations rather than backend instruction counts:

```text
diagnostics and source provenance
layout and final storage
alias-visible values
control-flow identity
ordered host requests/effects
conditions and EIB/status
terminal/spool/stdout/stderr
dataset state
return code, transfer, suspension, ABEND
resource counters and failure category
```

Known legacy defects are frozen as explicit gaps; mainframe-env does not reproduce a
defect silently merely to obtain byte equality.

### Property tests

Use Proptest for:

- codec and text roundtrips;
- layout non-overlap and alias invariants;
- decimal encode/decode and rounding properties;
- parser never-panic and bounded recovery properties;
- IR verifier soundness properties over generated small modules;
- machine transition invariants; and
- idempotency/effect sequence behavior.

### Fuzzing

The repository has persistent cargo-fuzz/libFuzzer targets for the bounded
COBOL frontend and IR binary/text decoders. `fuzz-smoke` executes both targets
against copied seed corpora with a small generated-input budget; the larger
`fuzz-periodic` run is part of every full tier, including the weekly scheduled
run. Corpora and crash artifacts are kept separate so CI cannot rewrite a
committed seed while earning a clean-candidate receipt. Every discovered crash
must become a minimized regression fixture.

The remaining target state is to extend persistent fuzzing to:

- JCL lexers/parsers/preprocessors;
- z/OSMF JSON/path/query/multipart inputs;
- dataset names, records, catalogs, and encoded data;
- RACF/security request parsing and profile matching; and
- checkpoint/configuration readers.

The tracked assurance registry requires both current targets, nonempty corpora,
positive input/run bounds, the pinned nightly, and the exact cargo-fuzz version.
Removing a target or corpus therefore fails before fuzz execution rather than
producing an empty green gate.

### Model and concurrency checking

The `model-check` gate uses Loom to enumerate schedules for version-fenced
execution transitions and effect intent finalization. A deliberately unfenced
lost-update mutant is required to fail under Loom, proving that the gate is
exploring schedules rather than merely running an ordinary happy-path test.
These bounded models use the production `ExecutionState::can_transition_to`
and effect-state types; they do not claim to model PostgreSQL or the complete
multi-process work queue.

Kani remains a candidate for pure validators after an approved pinned verifier
distribution can be installed on the capped Jenkins node. TLA+/TLC work-lease
modeling remains blocked on R-18: the reviewed production lease transition is
known to admit expired work, so formalizing it now would preserve the defect as
the specification. TLC also needs a pinned, checksum-verified JVM artifact
before it can enter this repository's offline assurance boundary. Neither tool
is current evidence, and the implemented Loom scope must not be described as a
substitute for those future proofs.

### Source coverage visibility

The full tier uses pinned `cargo-llvm-cov` and the pinned toolchain's
`llvm-tools-preview` component to instrument the IR, COBOL compiler, store
contract, and store implementation packages. It archives the machine-readable
summary and records exact line/function totals. The validator requires nonzero
covered code and the presence of every declared package, so an empty or
mis-scoped report blocks the gate. Conservative absolute covered-line and
function floors derived from the first measured baseline prevent the gate from
becoming vacuous while leaving room for refactoring. No arbitrary percentage is
treated as semantic evidence; the baseline is visibility and regression input,
not release coverage credit.

### Failure and chaos testing

Inject:

- provider panic/failure/timeout/cancellation;
- database slow, disconnect, serialization conflict, and saturation;
- process restart during queued/running/suspended/completing states;
- crash before, during, and after mutating effect publication;
- stale checkpoint/artifact/provider versions;
- output, queue, memory, record, and session limit exhaustion; and
- malformed/unauthorized z/OSMF requests.

## Required gates

### Per-change gate

- formatting and diff hygiene;
- narrow affected tests;
- architecture/dependency check;
- async-context store and composed execution-route regression checks;
- schema/fixture check when relevant.

### Local Jenkins budget policy

The repository Jenkinsfile implements the risk tiers without repeating
release-grade work on every event:

- jobs run only for branches and changes configured in local Jenkins; the
  checked-out SHA is the execution authority;
- concurrent runs of the same job are serialized to bound local disk use;
- documentation-only changes skip the ordinary Rust suite but still run the
  immutable supply-chain, locked dependency-policy, and full-notice checks;
- pull requests run the supply-chain gate, `cargo deny check`, target-production license-notice
  validation, formatting, specification and COBOL exit checks, workspace tests,
  Clippy, and a Rust 1.95.0 check over the full workspace, all targets, and all
  features;
- the integrated `main` commit runs the complete workspace and documentation
  gates; the MSRV result is not repeated for a standard merge commit;
- a manual `full` run adds complete conformance, certification, evidence, and
  runtime-architecture checks for a minor exit or an explicit integrated
  audit; and
- release generation, artifact upload, and reproduction run only on a
  `mainframe-env-v*` tag, after the complete manual-tier checks, and use a clean
  build rather than the normal debug cache.

Cargo registry downloads are shared in the capped Cargo home. Build outputs are
kept in the per-build workspace and removed after archiving receipts, so every
run can rebuild from an empty target directory. The Jenkins home, workspace,
Cargo home/target, temporary files, logs, and artifacts live on a filesystem
whose total capacity is at most 10 GiB. Selected backend parity uses a
disposable PostgreSQL 18 cluster whose data, socket and log also stay in that
workspace.

The Jenkins WAR, complete plugin dependency closure, Rust compiler/Cargo
commits, and separately installed tool versions are locked and checked before
credit. Tracked actions require commit SHAs, tracked images require digests,
and tracked package installation is forbidden. The reviewed inventory and
update procedure are in `docs/runbooks/CI-SUPPLY-CHAIN.md`.

### Milestone gate

- affected package suites;
- profile build and test;
- differential and property suites;
- public API/schema compatibility check;
- bounded parser/decoder fuzz smoke and nonempty corpus validation;
- Loom schedule exploration for the registered durable-state models; and
- instrumented coverage visibility for the registered critical packages.

### Release gate

- complete core-server and conformance profiles;
- long-running mixed workload and leak tests;
- overload/backpressure and cancellation evidence;
- database backup/restore and restart recovery;
- release-binary SQLite startup, readiness, and shutdown smoke;
- immutable CI/controller inputs and full-workspace MSRV;
- blocking `cargo deny check` plus deterministic full target-production license
  notices;
- compatibility and cutover rehearsal; and
- reproducible artifacts and documentation.

Broad validation is run once per unchanged release candidate. Failures are
repaired and proven with the narrowest relevant suite before rerunning the full
gate.
