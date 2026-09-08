# Documentation portal

This portal separates current operating guidance from normative architecture,
versioned delivery records, and historical research. If two normative documents
conflict, use the precedence order below and record the conflict rather than
silently choosing one.

## Start here

| Goal | Document |
|---|---|
| Understand the product boundary | [Project charter](CHARTER.md) |
| Understand the system | [Architecture overview](architecture/OVERVIEW.md) |
| Build or test the workspace | [Verification strategy](delivery/VERIFICATION-STRATEGY.md) |
| Operate the development server | [Core-server operations](runbooks/OPERATIONS.md) |
| Understand the current release | [0.8 release notes](releases/0.8.md) |
| Prepare work on 0.9.0 | [0.9.0 readiness status](delivery/coverage-versions/status/0.9.0.md) |
| Review pre-0.9 risks | [Pre-0.9 deep review](reviews/PRE-0.9.0-DEEP-REVIEW.md) |
| Contribute or report security issues | [Contribution guide](../CONTRIBUTING.md) and [security policy](../SECURITY.md) |

## Normative precedence

1. [Project charter](CHARTER.md)
2. Accepted or frozen architecture documents in [`architecture/`](architecture/)
3. Versioned contracts in [`contracts/`](contracts/)
4. Accepted architecture decisions in [`decisions/`](decisions/)
5. Delivery, verification, and release contracts in [`delivery/`](delivery/)
6. Version-specific implementation prompts in [`prompts/`](prompts/)
7. Runbooks, release notes, research, and historical status records

Machine-readable schemas, catalogs, and evidence under `conformance/` remain
authoritative for the exact identities and counts they own. Prose must not
silently override them. A later ADR overrides an earlier authority only when it
explicitly names that authority as superseded.

## Architecture and contracts

- [Architecture overview](architecture/OVERVIEW.md)
- [Package map](architecture/PACKAGE-MAP.md)
- [Compiler and IR](architecture/COMPILER-AND-IR.md)
- [COBOL runtime semantics](architecture/COBOL-RUNTIME-SEMANTICS.md)
- [Execution and durability](architecture/EXECUTION-AND-DURABILITY.md)
- [Security and capabilities](architecture/PLUGIN-AND-SECURITY.md)
- [Conformance IR](architecture/CONFORMANCE-IR.md)
- [Coverage authority](architecture/COVERAGE-AUTHORITY.md)
- [Dataset, VSAM, and AMS](architecture/DATASET-VSAM-AMS.md)
- [JES execution](architecture/JES-EXECUTION.md)
- [Application packages](architecture/APPLICATION-PACKAGES.md)
- [Program and route registries](architecture/PROGRAM-AND-ROUTE-REGISTRIES.md)
- [Canonical effect encoding](contracts/EFFECT-CANONICAL-V1.md)

## Architecture decisions

- [ADR-0001: Technology stack](decisions/0001-technology-stack.md)
- [ADR-0002: Deterministic core](decisions/0002-deterministic-core.md)
- [ADR-0003: Contract serialization](decisions/0003-contract-serialization.md)
- [ADR-0004: Versioning and release policy](decisions/0004-versioning-release-policy.md)
- [ADR-0005: Package consolidation](decisions/0005-package-consolidation.md)
- [ADR-0006: CardDemo 0.1.1 profile](decisions/0006-carddemo-0.1.1-profile.md)
- [ADR-0007: CardDemo 0.1.1 release](decisions/0007-carddemo-0.1.1-release.md)
- [Decision index and template](decisions/README.md)

ADR-0005 records a historical 0.1 decision. The current workspace has later
package additions; the pre-0.9 review tracks the missing composed/current
architecture decision.

## Delivery and development

- [Implementation roadmap](delivery/IMPLEMENTATION-ROADMAP.md)
- [Implementation status](delivery/IMPLEMENTATION-STATUS.md)
- [Verification strategy](delivery/VERIFICATION-STRATEGY.md)
- [Compatibility and cutover](delivery/COMPATIBILITY-AND-CUTOVER.md)
- [Versioning and releases](delivery/VERSIONING-AND-RELEASES.md)
- [IBM coverage release plans](delivery/coverage-versions/README.md)
- [Parallel implementation plan](delivery/coverage-versions/PARALLEL-IMPLEMENTATION.md)
- [Implementation prompt index](prompts/coverage-versions/README.md)
- [z/OSMF compatibility API](delivery/ZOSMF-API.md)

Version-specific status files and review reports describe candidates at a point
in time. They are not automatically current product documentation.

## Operations

- [Core-server operations](runbooks/OPERATIONS.md)
- [Backup and restore](runbooks/BACKUP-RESTORE.md)
- [Capacity and recovery](runbooks/CAPACITY-AND-RECOVERY.md)
- [Local Jenkins](runbooks/JENKINS-LOCAL.md)
- [CardDemo operator guide](runbooks/CARDDEMO-OPERATOR.md)
- [CICS licensed pilot](runbooks/cics-licensed-pilot.md)
- [Conformance family rollout](runbooks/conformance-family-rollout.md)

## Releases, reviews, and research

- Release notes: [`releases/`](releases/)
- Deep reviews: [`reviews/`](reviews/)
- IBM official coverage roadmap: [research record](research/IBM-OFFICIAL-COVERAGE-ROADMAP.md)
- Publication source probe: [current research record](research/publication-source-probe.md)
- CICS behavioral pilot: [accepted bounded pilot](research/cics-behavioral-conformance-pilot.md)
- Historical hardening records: [`delivery/hardening/`](delivery/hardening/)

## Document status vocabulary

| Status | Meaning |
|---|---|
| Proposed | Design input; not an implementation contract |
| Accepted | Normative decision for work in its stated scope |
| Implemented | Present in code with focused verification |
| Proven | Required cross-layer, failure, compatibility, and operational evidence passes |
| Historical | Immutable record of a prior candidate or decision |
| Superseded | Replaced by a named newer authority |

Every new normative document should state its status, owner or approving
authority, scope, and the version from which it applies. Historical records
should name their candidate and must not be rewritten as current truth.
