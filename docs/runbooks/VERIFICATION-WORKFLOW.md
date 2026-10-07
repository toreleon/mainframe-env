# Verification workflow

Use enough evidence to establish the changed behavior and its affected boundaries.
Avoid turning each edit into a new full certification campaign. Explicit task,
policy, integration, and conformance acceptance requirements remain mandatory.
Run project commands directly from the intended checkout with the pinned tools.

## Agent development loop

1. Inspect the diff and identify the owning module, consumers, and changed contract.
   State the focused check and any necessary expansion before a costly command.
2. For IBM semantics, search/read the relevant pinned cache topics. Reuse topic
   citations while their pinned bytes remain unchanged. A lookup is not a request
   to audit 4,488 topics, refresh the network, or start a licensed oracle campaign.
3. Add a regression for a changed behavior or real defect. Run that test and the
   affected package/tooling checks. Verify the selected test actually executed;
   a filter matching zero tests is not success.
4. Complete mandatory dependency/license policy and affected generated/docs gates.
   Stop when the change's acceptance checks pass. Expand for a changed dependency,
   observed failure, unresolved risk, or explicit broader acceptance requirement.
5. Report commands, results, skips, source identities, and remaining blockers once.
   A commit, PR description edit, or unchanged status inquiry alone does not justify
   repeating the same exploratory suite.

Do not create tests that merely restate a reversible prose/configuration edit.
Reuse exploratory findings only while their relevant inputs/environment match.
Jenkins receipts still bind to the actual candidate and tree: never copy or edit
an old receipt to claim a new commit passed. Changed code, fixtures, dependencies,
toolchains, or relevant environment invalidate affected results.

An unchanged compiler/backend failure should receive one focused diagnostic,
then be fixed within scope or reported. Repeated full CardDemo/certification runs
without a changed input add cost and no evidence. Environment failures receive
one diagnosis/repair attempt before reporting the concrete prerequisite; a later
repair or new evidence justifies retrying. Required gates must not be waived.

## CI ownership

The root `Jenkinsfile` owns candidate-bound CI, full assurance gates.
It refuses dirty or untracked source, records each selected command, and grants no
licensed credit without the corresponding protected execution evidence. A later
prose commit cannot hide an intervening failed code change.

## Timing and caches

`ci-checks/plan.json`, per-command JSON/logs, and `summary.json` are archived by
Jenkins. The summary records actual selected-command time. Failed runs leave later
gates unrun, and each attempt starts with fresh receipts. The current five-build/two-artifact retention stays
bounded. Download/compiler caches are reusable computation; test receipts are
evidence and cannot substitute for required candidate checks.

Avoid routine cache deletion, toolchain changes, alternate build flags, or
parallel full Cargo invocations just to seek reassurance. Cleanup is for measured
pressure; cache and parallelism changes need elapsed-time and peak-storage
measurements first.

See the [IBM cache guide](IBM-DOCS-CACHE.md).

## Effective Conformance IR inspection

`cargo xtask conformance-spec-export` emits a JSON tooling bundle containing the
validated effective document used by the existing runner, including admitted
pilot augmentation, normalized catalog rows and candidate/catalog/spec digests.
The committed static spec alone does not include every admitted pilot. Consumers
can reconstruct `OfficialCatalogRow` values and compile `spec_document` with the
existing `CompiledSpec::compile_json` API, checking the exported spec digest.
Both export and execution use the same builder and augmentation order.

Retain exports outside Git. They contain metadata and bindings, never execution
or licensed acceptance; both credit fields are zero. Actual selected product
route observations and verdict/ledger receipts are still required. An export's
candidate identity is its exact producing checkout, including relevant dirty
state, and cannot be relabeled for another candidate.

## Scoped CICS contract-consumption tests

The SYNCPOINT ledger tests consume a fresh same-builder export. Before running
them or the full workspace tests, export the current checkout into an ignored
file or an external receipt directory and supply its path explicitly:

```sh
mkdir -p target/conformance-inputs &&
cargo run --quiet --locked -p xtask -- conformance-spec-export > target/conformance-inputs/effective-spec.json &&
MAINFRAME_ENV_CONFORMANCE_SPEC_EXPORT="$PWD/target/conformance-inputs/effective-spec.json" \
    cargo test --locked -p mainframe-env-conformance cics_pilot::tests::contract_consumption::
```

Regenerate after changing candidate inputs. Missing, malformed or stale input
fails the tests; it earns no skipped or successful evidence. The existing Jenkins
Foundation stage generates the same input before the unchanged workspace test
command. Retain required receipts outside disposable targets before cleanup.
The export is metadata with zero execution and licensed credit; the tests still
have to obtain and evaluate observations from the compiled product path.
