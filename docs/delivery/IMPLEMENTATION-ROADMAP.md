# Subsystem implementation roadmap

Status: **Accepted delivery organization**
Owner: **maintainers**
Scope: **subsystem progress, dependency sequencing, and acceptance boundaries**
Applies from: **mainframe-env 0.8.3 development**

Manage implementation by subsystem and phase using the
[subsystem plans](subsystems/README.md), [progress overview](IMPLEMENTATION-STATUS.md),
and [dependency map](subsystems/DEPENDENCIES.md). The documentation registry
owns the mapping and generates navigation, prompt indexes, and progress views.
COBOL structure/execution, CICS application/system APIs, and Db2 core/programming
remain separate phases under their respective subsystem owners.

## Work sequencing

1. Select the owning subsystem phase and read its plan, progress record,
   [execution contract](../prompts/subsystems/README.md#common-execution-contract),
   source/catalog authorities, and consumed dependency receipts.
2. Verify the phase's start gate. Private catalog, parser, fixture, and harness
   preparation may proceed only within the existing plan's declared boundaries.
3. Declare one bounded work package or slice in that phase's progress record,
   including ownership, dependencies, exact scope, applicable gates, and candidate.
4. Implement and run focused validation through the existing contract owners.
   Integrate only after the consumed subsystem dependencies pass on the named
   candidate; a version label or merged PR alone supplies no acceptance evidence.
5. Update the phase's progress record and regenerate documentation. Preserve
   work-package IDs, sealing trailers, target releases, licensed-pending
   dispositions, and historical evidence identities.

## Acceptance and release boundaries

The [common release contract](subsystems/README.md#common-release-contract)
and phase-specific exit gates remain binding. Partial work does not complete
an official row; unavailable licensed environments remain pending. Cross-resource
integration and final certification consume accepted provider behavior and
do not replace each provider's mutation, failure, and recovery evidence.

Release versions remain compatibility and publication identities. Promotion,
tagging, publication, deployment, and compatibility claims retain their existing
authorization and candidate requirements. Organizing work by subsystem does
not expose later-release behavior in an earlier public profile.

The original [0.1 implementation roadmap](history/INITIAL-IMPLEMENTATION-ROADMAP.md)
is retained as history; its ME.V0–ME.V7 phases do not describe current progress.
