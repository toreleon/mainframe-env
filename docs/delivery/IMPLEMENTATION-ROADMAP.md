# Subsystem implementation roadmap

Status: **Accepted delivery organization**
Owner: **maintainers**
Scope: **subsystem progress, dependency sequencing, and acceptance boundaries**
Applies from: **mainframe-env subsystem management v1**

Manage work by subsystem and phase using the
[plans](subsystems/README.md), [progress overview](IMPLEMENTATION-STATUS.md),
and [dependency map](subsystems/DEPENDENCIES.md). The documentation registry owns
this mapping and generates navigation, prompt indexes, and progress views.

## Work sequencing

1. Select a named phase and read its plan, status, implementation prompt, and
   source/catalog authorities.
2. Verify start gates and consumed dependencies on the current candidate.
3. Define one bounded slice with an owner, fixtures, regressions, and blockers.
4. Implement through the existing contract owners and run the relevant gates.
5. Update the phase record and regenerate documentation.

## Acceptance boundaries

The [common execution contract](../prompts/subsystems/README.md#common-execution-contract)
and phase-specific checks define acceptance. Parser recognition, local execution,
whole-row conformance, and licensed differential completion have separate scopes.
Unavailable licensed environments remain pending. Keep generated execution
outputs outside Git; preserve specifications, source locators, fixtures, and tests.
