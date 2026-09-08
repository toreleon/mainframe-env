# 0.1 Verification Strategy

Status: **Accepted by repository owner**

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

### Fuzzing — planned, not yet implemented

The target state is for persistent fuzz targets to cover:

- COBOL and JCL lexers/parsers/preprocessors;
- IR text/binary decoders;
- z/OSMF JSON/path/query/multipart inputs;
- dataset names, records, catalogs, and encoded data;
- RACF/security request parsing and profile matching; and
- checkpoint/configuration readers.

Every discovered crash must become a minimized regression fixture. As of the
pre-0.9 review, the repository has property tests but no persistent fuzz target
or CI fuzz-smoke gate; this section is a requirement, not current evidence.

### Model and concurrency checking — planned, not yet implemented

- Kani checks bounded pure validators, arithmetic, and selected state-machine
  transitions where tractable.
- Loom checks custom concurrency primitives and publication/permit behavior,
  with its limitations documented.
- TLA+/TLC models durable work claim, lease, attempt, effect intent/result,
  cancellation, and recovery before multi-process durable promotion.

No Kani harness, Loom dependency, or TLA+/TLC model is currently present. Do
not cite this target-state section as a completed assurance layer.

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
- documentation-only changes do not start the Rust workflow;
- pull requests run formatting, specification and COBOL exit checks, workspace
  tests, Clippy, and the contract MSRV gate;
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

### Milestone gate

- affected package suites;
- profile build and test;
- differential and property suites;
- public API/schema compatibility check;
- fuzz smoke corpus once the planned harness exists; until then the missing
  gate remains an explicit release-readiness gap.

### Release gate

- complete core-server and conformance profiles;
- long-running mixed workload and leak tests;
- overload/backpressure and cancellation evidence;
- database backup/restore and restart recovery;
- release-binary SQLite startup, readiness, and shutdown smoke;
- security/advisory/license checks;
- compatibility and cutover rehearsal; and
- reproducible artifacts and documentation.

Broad validation is run once per unchanged release candidate. Failures are
repaired and proven with the narrowest relevant suite before rerunning the full
gate.
