# #54 — selected PR gates and explicit full campaigns

Current executor: the repository `Jenkinsfile` on the capped local Jenkins
volume. The original implementation and recorded measurements used GitHub
Actions; committed GitHub receipts remain historical evidence only.

## Acceptance boundaries

The inexpensive selector runs for all main-target PRs and main pushes, including
document changes. A prose-only change runs selector tests and records an empty
obligation plan; the foundation, MSRV and backend jobs are explicitly skipped,
not credited as passed. The supply-chain, dependency-license, and complete
notice gates remain mandatory. New/unknown paths and missing comparison history
select all obligations instead of guessing that no checks are required. Deleted
paths and both sides of a rename are considered using Git's no-renames diff mode.

A relevant PR retains the existing formatting, specification, COBOL-exit,
workspace-test and Clippy gates. It additionally runs selected architecture-fast
and evidence-fast gates in that same Rust build job, reusing compiled dependencies
rather than duplicating a whole workspace build in a new assurance job. Its MSRV
gate checks the complete workspace, all targets, and all features on the exact
Rust/Cargo 1.95.0 identity.

Architecture-fast retains dependency-direction/declared-graph, common coordinator
route and dehardcoding checks, but omits the two targeted runtime tests and release
server build. The original architecture and runtime-architecture commands retain
those broader checks. Evidence-fast validates schemas and their mapped evidence
instances plus the historical evidence seal. It is not a newly executed full
conformance campaign or an attestation of licensed equivalence.

## Changed-path obligations

| Change | Selected supplemental obligations |
|---|---|
| Runtime, applications, gateways, configuration | Architecture, evidence, runtime |
| Compiler | Architecture, evidence, compiler |
| Stores | Architecture, evidence, runtime, backend parity |
| Providers | Architecture, evidence, runtime, backend parity, reference mutations |
| Shared contracts/foundation, manifests/toolchain, workflows/tools/xtask | All |
| Normative architecture/decision/compatibility/contract/generated docs, conformance, release evidence | All |
| README/changelog and recognized research/prompt/runbook/delivery Markdown | No Rust build solely for prose |
| Unrecognized paths or unavailable diff | All (fail-closed selection) |

The existing workspace tests still cover the selected runtime/compiler behavior;
the table is not a license to replace them with only a named subset. Shared
contract changes cannot select no conformance obligations. This mapping is tested
against normal, mixed, unknown, renamed and deleted paths.

## Full and bounded additional tiers

Manual `full` runs, the weekly Jenkins schedule and release tags select the full
tier. Workspace target checks/documentation, full conformance, certification,
the evidence seal and runtime architecture run on one exact checked-out commit.
The full tier also exports that commit with `git archive` and requires two
mtime-distinct reproductions in the digest-pinned GNU-tar image to match before
retaining the source archive and command receipt.
Tag release packaging and release checks remain unchanged. Scheduling only takes
effect after the Jenkinsfile reaches the configured job branch; an unexecuted
schedule grants no credit.

Backend parity runs on relevant store/shared/normative changes and all full tiers,
using a disposable PostgreSQL 18 service. It explicitly selects #51's ignored
PostgreSQL Move contract and rejects zero-test success. Memory/SQLite counterparts
remain in the workspace suite. When #57's effect contract exists in the candidate,
its ignored PostgreSQL contract is also explicitly required and recorded. The
writable-readiness rollback contract is selected explicitly as well, so the
provider-state DML probe cannot remain hidden behind workspace-test skipping.

#53's four reference-source mutants run only for selected mutation obligations
and full tiers. Their receipt retains zero product-runtime-mutation and licensed
credit. A compile error, missing test, timeout, or survivor is not a killed mutant.
There is no blanket `cargo test -- --ignored` that silently runs an unbounded
benchmark or credits unavailable external systems.

## Security, cost and evidence

The local Jenkins job uses no PR secrets. The optional GitHub publication
credential is injected only into its explicitly selected stage; PostgreSQL
parity uses a disposable local cluster. Every gate runs against the checked-out
SHA emitted by selection. The controller WAR, complete plugin closure,
toolchains, and host-tool versions are locked and verified by
`tools/supply_chain.py`; the update process is in
`docs/runbooks/CI-SUPPLY-CHAIN.md`. Cargo downloads are
shared on the capped volume, while build targets stay in the build workspace and
are deleted after each run. Prose avoids Rust, PostgreSQL and mutation work;
relevant code keeps one local build rather than scheduling duplicate builds.

`tools/ci_assurance.py record` writes command, candidate/tree, Rust/Cargo versions,
runner details, observed test count, exit status, wall seconds and log SHA-256.
Receipts from another commit, missing commands and empty selected test runs cannot
pass. Unselected full gates are listed separately. The Jenkins build result
requires every selected policy, foundation, MSRV and enabled backend stage to succeed.
A job receipt is not a release acceptance decision for epic #46.

The experiment below measures actual warm command overhead on one runner and
compares architecture-fast with the broader architecture gate. The old PR tier
had no dedicated architecture/evidence command: the new fast checks add measured
assurance cost, not an asserted overall speedup. Command seconds exclude runner
provisioning, the separate PostgreSQL job, parallelism and billing rounding; no
currency savings or universal performance ceiling is claimed. Actual PR job
runtime and the experiment receipt are linked from the implementation PR.

```
python3 -B -m unittest discover -s tools/tests -p test_ci_assurance.py
cargo build --locked -p xtask
python3 -B tools/ci_assurance_regressions.py --xtask target/debug/xtask --output target/ci-gate-experiment
```

Run on a clean committed candidate. The experiment uses a disposable worktree,
measures the unchanged gates, then adds a forbidden tokio dependency to a contract
and corrupts an actual evidence schema. It requires the real checker to reject
both mutations for the intended reason, not merely any process failure. Original
source and historical evidence are restored; raw receipts stay outside tracked
source. These are checker-regression tests, not product behavioral mutants.

### Recorded run

Candidate `9da098b`, local aarch64-apple-darwin, Rust 1.98.0, warm build.
Twelve selector tests passed. Both mutations were rejected for the intended
reason rather than by incidental failure:

| Gate | Purpose | Exit | Detected as expected | Seconds |
|---|---|---|---|---|
| architecture-fast | baseline | 0 | yes | 1.54 |
| architecture | baseline | 0 | yes | 34.65 |
| evidence-fast | baseline | 0 | yes | 6.96 |
| architecture-fast | forbidden `tokio` contract dependency | 1 | `depends on infrastructure tokio` | 0.01 |
| evidence-fast | corrupted evidence schema | 1 | `must describe an object` | 0.00 |

Architecture-fast costs 1.54 s against 34.65 s for the broader architecture gate
on the same warm tree, so selected PR runs pay roughly 8.5 s of added command
time for both fast gates instead of the ~34.7 s the broad gate alone would cost.
These are warm single-machine command seconds on a developer host, not runner
provisioning, billing units, or a claim about total PR wall-clock. The full
tier still runs the broad gates; the fast gates do not replace them.
