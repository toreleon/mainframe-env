# 0.1 Compatibility and Cutover Contract

Status: **Accepted by repository owner**
Owner: **repository owner**
Scope: **compatibility authority, migration, cutover, and rollback**
Applies from: **mainframe-env 0.1.0**

## Compatibility boundary

Only accepted 0.1 COBOL, CICS, JCL/JES, dataset, RACF/security, and z/OSMF
selectors create compatibility obligations. All other current packages and
selectors are excluded from 0.1 without per-selector migration work.

## Current workspace role

The current OpenMainframe workspace is:

- a source of fixtures and behavioral characterization;
- an executable black-box oracle;
- a source of known gaps and failure cases; and
- historical provenance for accepted behavior.

It is not:

- a production dependency;
- a stable internal API;
- permission to copy its architecture;
- automatically correct when behavior conflicts with specification; or
- a fallback embedded in mainframe-env production binaries.

## Freeze manifest

The 0.1 freeze records:

```text
current source revision
in-scope packages and selectors
accepted product routes
fixtures and external corpus identities
configuration and protocol behavior
known gaps and disputed behaviors
oracle commands and environment
redaction policy
```

Out-of-scope workspace members are recorded in one exclusion manifest.

## Compatibility categories

| Category | Required comparison |
|---|---|
| COBOL compiler | accepted/rejected source, diagnostics, layouts, storage, output, return/control outcomes |
| CICS | typed operations, options, effects, terminal/file state, conditions, EIB, transfers and ABEND |
| JCL/JES | symbols/procedures, DD, step conditions, job status, spool and cancellation |
| Dataset | names/catalog, record bytes, keys, status/conditions, mutation and concurrency behavior |
| RACF/security | authentication, SAF decisions, profile matching, audit and redaction |
| z/OSMF | routes, auth, payload/status/error formats, pagination and lifecycle semantics |

## Intentional corrections

When current behavior is clearly defective or unsafe:

1. freeze the current observation;
2. document the governing specification or invariant;
3. add a negative/control fixture;
4. record mainframe-env corrected behavior;
5. classify compatibility impact; and
6. require explicit owner acceptance before cutover.

The differential report may pass with an accepted correction only when the
correction identity is explicit. Normalization cannot hide semantic mismatch.

## Cutover stages

1. **Offline characterization** — mainframe-env and current oracle run independently.
2. **Fixture parity** — all accepted deterministic fixtures pass.
3. **Shadow observation** — isolated copies of selected workloads run through
   both systems with no duplicate external mutation.
4. **Explicit canary** — named selectors and principals use mainframe-env by explicit
   routing.
5. **0.1 default** — the complete accepted 0.1 profile routes to mainframe-env.
6. **Rollback rehearsal** — actual queued/running/suspended paths return to the
   declared previous authority where compatibility permits.
7. **Final cutover** — mainframe-env becomes the sole default product workspace and the
   old implementation is archived/removed from production closure.

## Cutover blockers

- an in-scope selector lacks a fixture or disposition;
- two automatic default authorities remain;
- shadow mode can duplicate mutation;
- rollback requires unproven state conversion;
- checkpoint/artifact readers silently accept incompatible versions;
- z/OSMF compatibility relies on current implementation internals;
- production profile imports conformance or out-of-scope packages;
- security denial/failure can become success; or
- overload, restart, cancellation, or store saturation is unbounded.

## Final state

The final 0.1 production workspace contains no compiled dependency on the old
workspace and no hidden legacy fallback. Historical oracle source/evidence may
remain archived outside the production dependency closure.
