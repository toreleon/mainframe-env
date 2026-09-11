# Verification workflow

Use enough evidence to establish the changed behavior and its affected boundaries.
Avoid turning each edit into a new full certification campaign. Explicit task,
release, promotion, and conformance acceptance requirements remain mandatory.
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

The root `Jenkinsfile` owns candidate-bound CI, full assurance, and release gates.
It refuses dirty or untracked source, records each selected command, and grants no
licensed credit without the corresponding protected execution evidence. A later
prose commit cannot hide an intervening failed code change.

## Timing and caches

`ci-checks/plan.json`, per-command JSON/logs, and `summary.json` are archived by
Jenkins. The summary records actual selected-command time. Failed runs leave later
gates unrun, and each attempt starts with fresh receipts. Release compilation has
its own `build-server` timing. The current five-build/two-artifact retention stays
bounded. Download/compiler caches are reusable computation; test receipts are
evidence and cannot substitute for required candidate checks.

Avoid routine cache deletion, toolchain changes, alternate build flags, or
parallel full Cargo invocations just to seek reassurance. Cleanup is for measured
pressure; cache and parallelism changes need elapsed-time and peak-storage
measurements first.

See the [IBM cache guide](IBM-DOCS-CACHE.md).
