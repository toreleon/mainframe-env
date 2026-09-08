# CICS file/UOW behavioral conformance pilot

Status: implementation audit and scope freeze for CF-01. The audit started from
`d7a47e8` on `main`; implementation work continues on
`codex/resolve-issues-82-91`. A result below is called reproduced only when the
named command was run against that candidate or this branch.

## What the existing evidence proves

| Sample | Concrete path | Boundary actually exercised | Evidence-chain classification |
|---|---|---|---|
| COBOL statement recognition and invalid forms | `crates/tooling/mainframe-env-conformance/src/cobol_statements.rs` | Compiler parser/semantic validation with reviewed fixtures | keep; correct parser boundary |
| COBOL file machine fixture | `crates/tooling/mainframe-env-conformance/src/cobol_files.rs` | Compiler and interpreter with a test-supplied `DatasetResult::Mutated` | keep and narrow claim; mocked host effect, not provider/store integration |
| Dataset organization and AMS drivers | `crates/tooling/mainframe-env-conformance/src/dataset.rs` | Product dataset/batch providers over a real in-process store, with direct owned readback | keep; provider scope, not CICS product integration |
| CICS RESP/EIB machine tests | `crates/tooling/mainframe-env-conformance/src/lib.rs` (`cics_response_updates_into_resp_and_eib_storage`) | Compiler ABI and interpreter with a test-supplied `CicsResponse` | keep; machine/mock-host boundary |
| CICS file rollback test at audit start | `crates/providers/mainframe-env-cics/src/service.rs` (`syncpoint_rollback_compensates_reached_dataset_rewrite`) | CICS provider with a traced fake dataset authority | replace the invalid sequence, add provenance and product integration |
| Shared runner and ledger | `crates/contracts/mainframe-env-coverage/src/conformance.rs` | Typed case bindings, observations, verdicts, shards, cache identity and derived ledger | reuse; add one bounded scenario executor and explicit environment/provenance identity |
| Licensed oracle | no CICS adapter or candidate-bound licensed capture exists | none | absence recorded; differential stays pending |

The audit reproduced a product defect rather than only an evidence gap. The
CICS service populated rewrite context after every `READ`, so a plain `READ`
followed by `REWRITE` succeeded. The pinned REWRITE command topic requires a
preceding `READ UPDATE` and assigns `INVREQ`, RESP 16, RESP2 30 when the
no-token context is absent. The provider now distinguishes read-only and update
reads; the old rollback test uses `OPTION.UPDATE`; and a regression proves that
a plain read cannot reach the dataset rewrite authority.

The audit also found three response-boundary gaps needed by the pilot: missing
keyed reads lost RESP2 80, file-security denial lost RESP2 101, and a closed
file returned the repository's synthetic `DISABLED` result instead of the
reviewed NOTOPEN 19/60 result. These are product-owned mappings and are fixed in
the CICS service, not in the conformance comparator.

## Reproduced commands

The following were run locally. Historical issue or PR counts are not used as
substitutes.

```text
cargo test -p mainframe-env-cics
cargo test -p mainframe-env-coverage
cargo test -p mainframe-env-conformance cics_pilot_runs_compiler_interpreter_providers_and_real_stores -- --nocapture
cargo test -p mainframe-env-conformance cics_pilot_sqlite_process_restart_recovers_at_bounded_file_faults -- --nocapture
python3 -m unittest discover -s conformance/0.9/tools/tests -p 'test_*.py'
cargo xtask spec --check
```

The first pre-change CICS run reproduced the incorrect plain-READ rollback
fixture. The post-change runs establish only the local scopes named here. No
licensed IBM campaign was run.

## Frozen pilot scope

The pilot is one local, keyed, fixed-length, CP037 file named `ACCTDAT`, backed
by `PILOT.ACCTDAT`, with a two-byte key and four-byte record. The resource is
recoverable. The two executed profiles are the in-memory product store and the
durable SQLite product store. `IBMUSER` is allowed and `DENIED` is rejected by
the file-resource security authority. The exact configuration is
`conformance/0.9/cics/pilot-environment.json`; changing any behavior-relevant
field changes its digest and invalidates observation/cache identity.

The selected forms and behaviors are:

- plain `READ FILE INTO RIDFLD RESP RESP2`;
- `READ ... UPDATE`, followed by no-token `REWRITE`;
- `SYNCPOINT` commit and `SYNCPOINT ROLLBACK`;
- invalid REWRITE without an update read, missing key, denied file access, and
  a closed file;
- update-context invalidation at a syncpoint; and
- a committed A→B transition followed by B→C and rollback to B.

Every case starts from explicit setup and reads final bytes through an owned
dataset read API. Intermediate command status is emitted by the compiled COBOL
program. The comparator consumes independent values from
`conformance/0.9/cics/pilot-fixtures.json`; it does not call CICS or dataset
semantic helpers to compute expectations.

Out of scope remains pending, not non-applicable: TOKEN and multiple
outstanding updates, RLS, remote files/function shipping, BDAM and data tables,
multi-owner locking/deadlock ordering, PostgreSQL, other READ/REWRITE options,
and licensed IBM differential execution. Concurrency is deliberately excluded
from this single-owner pilot. Three extracted locking fragments are therefore
dispositioned `defer-pending` rather than removed.

## Pinned behavioral source set

`conformance/0.9/manifests/cics-file-uow-topics.json` pins nine CICS TS 6.x
topics: the READ, REWRITE, SYNCPOINT and SYNCPOINT ROLLBACK command pages;
general updating-records, syncpoint, efficient-VSAM, authorization, and file
control/recovery topics. The set is finite: 9 topics, 188,259 bytes outside the
repository, with a manifest digest of
`fb318cc37ebb5a985317bd68a67d0d9614bc4f27edca8a7c4df0464602847007`.
It reuses the IBM content endpoint, byte digest, locator and no-retained-bytes
policy established by #78/#81.

The bounded extractor inventories 365 selected structural fragments. It emits
14 candidate fragments, 336 explicit outside-scope dispositions, 15
informative headings, zero unsupported fragments, and zero conflicts. Candidate
records preserve topic and fragment digests, structural locators, applicability,
negation/conditional cues and required links without retaining publication
text. Extractor output carries zero coverage credit.

## Smallest execution addition

`ScenarioSpec` remains metadata, ordering and exact-credit authority. A
registered Rust `ConformanceScenarioDriver` is the only new execution
mechanism. It runs the product route once for its declared scenario and returns
a bounded map keyed by exact `(row_id, obligation_id, gate)`. The shared runner
rejects a missing, duplicate, extra or unknown per-credit observation before it
can derive a ledger. It then evaluates the same typed observation registry used
by ordinary `ConformanceCase` bindings. A scenario-bound case cannot fall back
to the ordinary one-case driver route.

This is not a workflow interpreter: the CICS driver is ordinary bounded Rust
that owns setup and calls the compiler, ABI/interpreter, CICS, dataset/security
providers and selected store. Scenario metadata contains no expressions,
branches, shell commands or product transition logic.

Verdicts now carry candidate, catalog, spec, runner and environment-manifest
identities explicitly; the same fields participate in cache identity. The
declared selection remains authoritative, and omitted per-credit observations
or shard batches fail closed.

## Dependencies and ownership

- #78's retrieval prerequisite is met for this finite corpus without waiting
  for unrelated subsystem retrieval work.
- #15 prerequisites are only the already-reached compiler CICS ABI, file
  READ/REWRITE, RESP/RESP2, resource authorization, SYNCPOINT, product dataset
  provider, and memory/SQLite stores. The pilot neither waits for nor closes
  full CICS 0.9.
- #53's source-copy mutation campaign is extended to distinguish CICS product
  mutants from its existing independent-model mutants. The unchanged normal
  pilot kills data-transition, rollback and update-guard mutations.
- #54's cost-aware Jenkins selector is retained. Manifest, reviewed-rule,
  fixture, observation, provider and shared-contract paths have explicit
  selection tests; full/release assurance remains separate.

## Review gate

`conformance/0.9/cics/pilot-rule-review.json` is intentionally
`pending-maintainer`. The extractor's interpretations and eleven proposed
accepted fragments are not promoted into current Conformance IR merely because
they are schema-valid or agent-produced. `cargo xtask spec --check` enforces
that boundary: while review is pending, no CICS row may appear in the compiled
spec. A maintainer must accept or correct the finite decisions in repository or
PR review before CF-03 and the scenario credits can be promoted. This is the
only dependency that cannot be satisfied by additional local execution.
