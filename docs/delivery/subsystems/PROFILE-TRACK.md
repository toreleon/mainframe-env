# Workload-profile track

Status: **Approved 2026-09-27; intake in progress**
Epic: [#300](https://github.com/toreleon/mainframe-env/issues/300)
Profile contract: [ADR-0006](../../decisions/0006-carddemo-profile.md)

The [subsystem delivery program](README.md) grows the pinned IBM
programming surface one subsystem at a time. This track grows something
different: the number of **complete workload profiles** mainframe-env runs.
It runs beside subsystem implementation and does not change any subsystem's scope or exit
criteria.

## Why a second track

ADR-0006 defines six cumulative CardDemo profiles, and rule 7 of the shared
acceptance contract requires a current passing run for each declared profile. CardDemo is also the only application family, so every
subsystem phase is regression-checked against a single application's shape. A
second family with a different shape (GenApp: CICS service layers over Db2 and
VSAM, TSQ, web-service copybooks) finds gaps that CardDemo cannot.

## What a profile is

A profile names an application family, the resources it installs, the
journeys that drive it through product entry points, and the gate that
proves those journeys pass. The rules are those of ADR-0006:

- lower profiles stay independently testable;
- an unavailable dependency never becomes generic success;
- a profile counts only when its gate passes on one exact candidate, and a
  partial profile never counts.

## Intake rules

Every issue on this track states:

1. **Goal clause:** correctness, honesty, growth by profile, or the
   artifact boundary.
2. **Profile:** the profile it unlocks or protects, such as `genapp-base` or
   `carddemo-base`.
3. **Gate:** the command or journey that proves it.
4. **Subsystem owner:** the subsystem phase whose rows it needs. A construct
   the profile needs is filed against that owner and built there,
   not special-cased on this track. Production crates still carry no
   application-specific dispatch (rule 3).

## External corpora

Application sources stay outside the repository. Each is pinned by
repository URL and commit, and verified clean before use, as CardDemo is. No
application bytes are vendored, whatever the licence; IBM GenApp is EPL-2.0.
Tests use small handwritten fixtures that reproduce a construct, never copied
application source.

## Lifecycle of a profile

1. **Intake:** `profile intake` runs every member of the pinned corpus
   through the product frontends. It reports each member's status, its typed
   diagnostics, and the constructs it uses, each mapped to a coverage-registry
   row or reported as unregistered.
2. **Triage:** gaps are filed as issues grouped by construct, each naming the
   profile and its ladder owner.
3. **Declaration:** the profile is declared with its resources and journeys
   once its ladder rows exist.
4. **Gate:** the profile's journeys pass through product entry points. From
   then on it is a protected profile under rule 7.

Evidence keeps its oracle class. A profile passing locally is not licensed IBM
equivalence, and nothing on this track is presented as such.

## Work packages

| Id | Work | Profile | Gate | Ladder owner |
|---|---|---|---|---|
| PT-1 | This document and the intake rules | all | Documentation checks | — |
| PT-2 | `profile intake` command ([#301](https://github.com/toreleon/mainframe-env/issues/301)) | `genapp-base` | Fixture-corpus report tests | — |
| PT-3 | GenApp gap report and issues | `genapp-base` | Reproducible report from the pin | Per gap |
| PT-4 | `genapp-base` declaration and journeys | `genapp-base` | GenApp journeys | cics.application-api web and transforms; Db2 in db2.core–db2.programming (expected) |
| PT-5 | Per-profile performance and scale budgets | `carddemo-base` first | Batch-window budget; first fails on [#186](https://github.com/toreleon/mainframe-env/issues/186) | — |
| PT-6 | Crash-and-resume journey per profile | `carddemo-base` first | Restart equals a clean run | integration.transactions for cross-resource cases |
| PT-7 | Customer-shape regression tests | `carddemo-base` | Unit and conformance tests | Per shape |

Candidate later profiles, each needing a sponsor workload before it is
started: a CardDemo batch cycle under a scheduler
([#234](https://github.com/toreleon/mainframe-env/issues/234)), SORT-heavy
reporting ([#235](https://github.com/toreleon/mainframe-env/issues/235)), and
an IMS DB/TM profile after ims.programming.
