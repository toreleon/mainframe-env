# CI and agent feedback cost review

Review date: 2026-09-10. Scope: the local Docker/Jenkins environment and repository
agent instructions. Measurements below are observations of retained builds, not
a representative performance benchmark or a promise of future latency.

## Observed baseline

The live `mainframe-env-local` job runs the baked `docker/Jenkinsfile` and
`docker/ci.sh`. The separate root `Jenkinsfile` already has changed-path selection,
full/release modes, and a weekly full trigger in its definition. That root job's
schedule does not establish a running full campaign on the Docker controller.

| Retained local build | Result | Wall time | Relevant observation |
|---|---|---:|---|
| 5 | Success | 404.982 s | Release compilation alone reported 2m 25s, about 36% of total wall time |
| 6 | Failure | 152.425 s | Workspace test failure; later build/deploy stages were skipped |
| 4 | Failure | 262.750 s | PostgreSQL shared-artifact/quota contract failed |

Build 7 was running when sampled; its incomplete duration is excluded. These
builds use different candidates and cache states, so their times cannot be
treated as a before/after optimization comparison. The installed plugin set did
not expose the requested per-stage `wfapi` endpoint. No plugin was added merely
to collect metrics; the existing command receipt format provides durations.

## Causes found

1. The Docker script always ran workspace tests, MSRV, Clippy, PostgreSQL, release
   compilation, and deployment even for prose changes. Its `--event manual` plan
   selected the full tier for backend receipt plumbing, although the script did
   not execute the root pipeline's full assurance gate set.
2. Agent guidance listed a broad quality floor without a clear stopping rule.
   That encouraged repeating checks around edit/commit/PR transitions instead of
   deciding whether tested inputs or acceptance requirements changed.
3. Source lookup, whole-book source auditing, deterministic pilots, external
   compatibility tests, licensed differential execution, and aggregate
   certification are distinct operations. Treating every source consultation as
   a reason to run all of them adds work without establishing new facts.
4. The root pipeline uses the previous commit as one comparison fallback. A
   previous *failed* candidate is an unsafe baseline for narrowing subsequent
   checks. The local optimization explicitly uses the last successful candidate.
5. Existing CardDemo probes expose compilation and WAITSTEP completion failures.
   Repeated full probes with identical inputs will rediscover these blockers;
   they do not improve assurance. This optimization does not repair those bugs.

The local Docker script did **not** automatically call a licensed IBM oracle.
Full certification commands can validate recorded evidence rather than launch
new remote execution; their names alone must not be treated as proof of an
external run. Report exactly what a selected gate exercised.

## Implemented first step

The proposed branch adds a tested local selector using an ancestor-validated
last-successful base and the normative document registry. Prose-only changes
retain five policy/docs/tooling command
groups and skip five runtime command groups, release compilation, and deployment.
Runtime, infrastructure, normative, unknown, or missing-base changes retain the
existing complete local runtime checks. An explicit runtime mode remains available.

Per-command timings are archived; stale receipts are cleared and cannot make
an unrun gate pass. Check/build/deploy timeouts are bounded at 30/15/3 minutes
within a 60-minute job limit. These limits surface stuck work; they do not grant
success. Agent guidance now specifies focused tests, input-aware reuse, escalation
conditions, and a stopping rule. Read the
[operational workflow](../runbooks/VERIFICATION-WORKFLOW.md) for exact behavior.

## Controlled validation

The implementation commit `29f1eb0a216a908a5a0ab0c6303e6225b7388546` ran
the complete local-docs command group in Docker with a warm cache. The comparison
base was deliberately set to that same clean commit to exercise a no-change
selection. This is a controlled runner test, not a claim that Jenkins had already
validated that commit or a before/after benchmark of the complete pipeline.

| Command group | Measured seconds |
|---|---:|
| Source-input policy | 0.198 |
| Dependency policy | 2.152 |
| License notices | 0.653 |
| Documentation | 2.796 |
| Python/shell tooling | 12.128 |
| Total selected commands | 17.928 |

Tooling reported 403 executed tests and one existing skip, including 16 local
selector tests. The installed Jenkins Declarative linter accepted the proposed
pipeline. Histories cover missing/nonancestor bases, failed code followed by
prose, registry changes, normative paths, deleted code, dirty checkouts, and
stale/wrong-candidate receipts. No full workspace, CardDemo, or licensed campaign
was rerun merely to validate this selector.

The Docker controller loads its pipeline/runner from the toolchain image.
Merging source alone does not replace the running controller definition: rebuild
from the merged checkout and recreate Jenkins when its executor is idle to
activate the new pipeline. Preserve work and image changes belonging to another
active development task. Until rollout, measured savings are limited to the
controlled command-group test above.

## Next optimizations, in order

| Priority | Change | Evidence needed before enabling |
|---|---|---|
| 1 | Deploy this selector after review; collect docs/runtime timings | Correct selected/skipped gates on the installed controller, current-candidate receipts, no deployment for prose-only changes |
| 2 | Fix the root pipeline's failed-base fallback | Tests covering failed code followed by prose, PR target ancestry, missing history, and root full/release behavior |
| 3 | Select affected Rust packages plus consumers and PostgreSQL families | A reviewed dependency/ownership map; conservative selection for shared contracts, migrations, fixtures, generators, unknown paths, and deletions |
| 4 | Reduce repeated conformance/oracle work | Measure nested gate call graphs first; reuse only artifacts with validated input/candidate/environment identity; preserve promotion requirements |
| 5 | Tune compiler cache retention or incremental compilation | Compare warm/cold elapsed time and peak disk use inside the existing 50 GB limit |
| 6 | Supersede queued validation jobs safely | Separate interruptible validation from deployment/recovery before considering cancellation of an older build |

Do not add CI parallelism to the current 4-vCPU/8-GB VM without measurement.
Separate Cargo processes can contend for cache locks, memory, and disk. Keep
normal feedback bounded and run the applicable integration/full campaign at its
defined acceptance boundary. Do not create another scheduler or duplicate full
job as part of this first step.

## Source basis

- [Jenkins Pipeline best practices](https://www.jenkins.io/doc/book/pipeline/pipeline-best-practices/)
  supports keeping orchestration small and executing work in shell/tooling. This
  change keeps selection in tested Python and uses Groovy for stage conditions.
- [Jenkins Pipeline syntax](https://www.jenkins.io/doc/book/pipeline/syntax/)
  documents stage conditions, timeouts, and concurrency options. A Git diff from
  the last successful ancestor is used instead of relying on a single latest
  changeset, so failed intermediate changes remain selected.
- [Cargo build cache](https://doc.rust-lang.org/cargo/reference/build-cache.html)
  documents target/profile separation and reusable build output. Existing named
  volumes already supply that mechanism; adding a new cache service is deferred.
- [OpenAI model guidance](https://developers.openai.com/api/docs/guides/latest-model)
  recommends calibrating verification to the change and expanding/repeating only
  when new changes, failures, or unresolved concerns justify it.
- [Codex AGENTS.md guidance](https://learn.chatgpt.com/docs/agent-configuration/agents-md)
  describes repository/device instructions. The implementation puts concise
  behavior rules in AGENTS.md and detailed examples in the linked workflow.
