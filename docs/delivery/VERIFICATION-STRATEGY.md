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

### Fuzzing

Persistent fuzz targets cover:

- COBOL and JCL lexers/parsers/preprocessors;
- IR text/binary decoders;
- z/OSMF JSON/path/query/multipart inputs;
- dataset names, records, catalogs, and encoded data;
- RACF/security request parsing and profile matching; and
- checkpoint/configuration readers.

Every discovered crash becomes a minimized regression fixture.

### Model and concurrency checking

- Kani checks bounded pure validators, arithmetic, and selected state-machine
  transitions where tractable.
- Loom checks custom concurrency primitives and publication/permit behavior,
  with its limitations documented.
- TLA+/TLC models durable work claim, lease, attempt, effect intent/result,
  cancellation, and recovery before multi-process durable promotion.

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

### Hosted CI budget policy

Hosted CI implements the risk tiers without repeating release-grade work on
every event:

- feature-branch pushes do not start a run; the pull-request merge ref is the
  pre-merge authority;
- superseded runs for the same pull request are cancelled;
- documentation-only changes do not start the Rust workflow;
- pull requests run formatting, specification and COBOL exit checks, workspace
  tests, Clippy, and the contract MSRV gate;
- the integrated `main` commit runs the complete workspace and documentation
  gates; the MSRV result is not repeated for a standard GitHub merge commit;
- manual dispatch adds complete conformance, evidence, and runtime-architecture
  checks for a minor exit or an explicit integrated audit; and
- release generation, artifact upload, and reproduction run only on a
  `mainframe-env-v*` tag, after the complete manual-tier checks, and use a clean
  build rather than the normal debug cache.

The non-release job caches `target/debug` by runner, Rust version, lockfile, and
workspace manifests. Pull requests are restore-only: they may consume the
trusted default-branch cache but never publish one. A successful `main` or
manual full run saves a new cache only on a miss. Later pull requests rebuild
only changed crate outputs. The cache is an optimization only; every command
remains able to rebuild from an empty cache.

### Milestone gate

- affected package suites;
- profile build and test;
- differential and property suites;
- public API/schema compatibility check;
- fuzz smoke corpus.

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
