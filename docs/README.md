# Documentation portal

This portal separates current operating guidance from normative architecture,
subsystem delivery records, and historical research. If two normative documents
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
| Track implementation progress | [Subsystem progress](delivery/IMPLEMENTATION-STATUS.md) |
| Prepare CICS application API work | [CICS application API progress](delivery/subsystems/cics/application-api-status.md) |
| Review pre-0.9 risks | [Pre-0.9 deep review](reviews/PRE-0.9.0-DEEP-REVIEW.md) |
| Contribute or report security issues | [Contribution guide](../CONTRIBUTING.md) and [security policy](../SECURITY.md) |

## Normative precedence

1. [Project charter](CHARTER.md)
2. Accepted or frozen architecture documents in [`architecture/`](architecture)
3. Versioned contracts in [`contracts/`](contracts)
4. Accepted architecture decisions in [`decisions/`](decisions)
5. Delivery, verification, and release contracts in [`delivery/`](delivery)
6. Subsystem implementation prompts in [`prompts/`](prompts)
7. Runbooks, release notes, research, and historical status records

Machine-readable schemas, catalogs, and evidence under `conformance/` remain
authoritative for the exact identities and counts they own. Prose must not
silently override them. A later ADR overrides an earlier authority only when it
explicitly names that authority as superseded.

<!-- BEGIN GENERATED DOCUMENTATION NAVIGATION -->
## Architecture and contracts

- [Project charter](CHARTER.md)
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
- [IBM MQ programming surface](architecture/MQ-PROGRAMMING-SURFACE.md)
- [Application packages](architecture/APPLICATION-PACKAGES.md)
- [Batch controller registry](architecture/BATCH-CONTROLLER-REGISTRY.md)
- [CICS command routing](architecture/CICS-COMMAND-ROUTING.md)
- [CICS terminal control](architecture/CICS-TERMINAL-CONTROL.md)
- [Db2 application catalog](architecture/DB2-APPLICATION-CATALOG.md)
- [Host ABI source libraries](architecture/HOST-ABI-SOURCE-LIBRARIES.md)
- [Program and route registries](architecture/PROGRAM-AND-ROUTE-REGISTRIES.md)
- [Durable storage profile](contracts/DURABLE-STORAGE-PROFILE.md)
- [Canonical effect encoding](contracts/EFFECT-CANONICAL-V1.md)
- [Provider object-row persistence](contracts/PROVIDER-ROW-PERSISTENCE-V1.md)
- [Durable retention lifecycle](contracts/RETENTION-LIFECYCLE-V1.md)
- [Target release build type](contracts/RELEASE-BUILD-V1.md)
- [Transaction participant contract](contracts/TRANSACTION-PARTICIPANT-V1.md)
- [Release builder security model](architecture/RELEASE-BUILDER.md)

## Architecture decisions

- [ADR-0001: Technology stack](decisions/0001-technology-stack.md)
- [ADR-0002: Deterministic core](decisions/0002-deterministic-core.md)
- [ADR-0003: Contract serialization](decisions/0003-contract-serialization.md)
- [ADR-0004: Versioning and release policy](decisions/0004-versioning-release-policy.md)
- [ADR-0005: Historical package consolidation](decisions/0005-package-consolidation.md)
- [ADR-0006: CardDemo 0.1.1 profile](decisions/0006-carddemo-0.1.1-profile.md)
- [ADR-0007: CardDemo 0.1.1 release](decisions/0007-carddemo-0.1.1-release.md)
- [ADR-0008: ICU license compliance](decisions/0008-icu-license-compliance.md)
- [ADR-0009: Current package topology](decisions/0009-current-package-topology.md)
- [ADR-0010: Rust module review budgets](decisions/0010-rust-module-review-budgets.md)
- [ADR-0011: Typed language HIR and semantic IR](decisions/0011-typed-language-hir-and-semantic-ir.md)
- [ADR-0012: Checked AMODE64 storage boundary](decisions/0012-checked-amode64-storage-boundary.md)
- [ADR-0013: CICS web service control boundary](decisions/0013-cics-web-service-control-boundary.md)
- [ADR-0014: Named-counter pool authority](decisions/0014-named-counter-authority.md)
- [ADR-0015: CICS operator reply boundary](decisions/0015-cics-operator-reply-boundary.md)
- [ADR-0016: CICS TCP/IP ingress context](decisions/0016-cics-tcpip-ingress-context.md)
- [ADR-0017: CICS immediate START target authority](decisions/0017-cics-immediate-start-target-authority.md)
- [ADR-0018: CICS START ATTACH lifetime](decisions/0018-cics-start-attach-lifetime.md)
- [ADR-0019: BTS lifecycle authority](decisions/0019-bts-lifecycle-authority.md)
- [ADR-0020: Conversation peer exchange ledger](decisions/0020-conversation-peer-exchange-ledger.md)
- [ADR-0021: Conversation ISSUE control staging](decisions/0021-conversation-issue-control-staging.md)
- [ADR-0022: ISSUE PASS target handoff](decisions/0022-issue-pass-target-handoff.md)
- [ADR-0023: BTS lifecycle authority](decisions/0023-bts-lifecycle-authority.md)
- [ADR-0024: Per-assertion oracle provenance](decisions/0024-per-assertion-oracle-provenance.md)
- [ADR-0025: Licence and provenance policy for IBM oracle evidence](decisions/0025-licence-and-provenance-policy.md)
- [ADR-0026: CardDemo run bundle](decisions/0026-run-bundle.md)
- [ADR-0027: CICS task ownership across logical program frames](decisions/0027-cics-logical-program-frames.md)
- [ADR-0028: Bounded GSAM logical record addresses](decisions/0028-gsam-logical-address.md)
- [ADR-0031: IMS TM recovery publication and work settlement](decisions/0031-ims-tm-recovery-publication.md)
- [ADR-0029: Versioned selected database PCB feedback](decisions/0029-selected-pcb-feedback.md)
- [Decision index and template](decisions/README.md)

ADR-0009 supersedes ADR-0005 for current topology. Historical ADRs remain immutable records of the decisions they originally authorized.

## Delivery and development

- [Implementation roadmap](delivery/IMPLEMENTATION-ROADMAP.md)
- [Implementation status](delivery/IMPLEMENTATION-STATUS.md)
- [Verification strategy](delivery/VERIFICATION-STRATEGY.md)
- [Compatibility and cutover](delivery/COMPATIBILITY-AND-CUTOVER.md)
- [Versioning and releases](delivery/VERSIONING-AND-RELEASES.md)
- [z/OSMF compatibility API](delivery/ZOSMF-API.md)

Implementation progress is organized by subsystem and phase. Release versions identify compatibility and historical evidence.

## Subsystem progress

- [Subsystem plans and acceptance contract](delivery/subsystems/README.md)
- [Subsystem dependencies and concurrency](delivery/subsystems/DEPENDENCIES.md)
- [Workload-profile track](delivery/subsystems/PROFILE-TRACK.md)
- [Subsystem implementation prompts](prompts/subsystems/README.md)
- [Project coordination and release history](delivery/subsystems/GITHUB-PROJECT.md)
- [Coverage and conformance — Coverage authority](delivery/subsystems/coverage/foundation-status.md)
- [COBOL — Grammar and types](delivery/subsystems/cobol/structure-status.md)
- [COBOL — Execution semantics](delivery/subsystems/cobol/execution-status.md)
- [RACF / SAF — Commands and authorization](delivery/subsystems/racf/security-status.md)
- [Datasets / VSAM / AMS — Dataset services](delivery/subsystems/dataset/data-status.md)
- [JCL — Converter and planner](delivery/subsystems/jcl/planning-status.md)
- [JES2 and utilities — Jobs, spool and utilities](delivery/subsystems/jes/execution-status.md)
- [CICS — Application API](delivery/subsystems/cics/application-api-status.md)
- [CICS — SPI and FEPI](delivery/subsystems/cics/system-api-status.md)
- [z/OSMF — REST portfolio](delivery/subsystems/zosmf/rest-status.md)
- [Db2 — Engine and common SQL](delivery/subsystems/db2/core-status.md)
- [Db2 — Complete programming surface](delivery/subsystems/db2/programming-plan.md)
- [IMS — DB / TM programming surface](delivery/subsystems/ims/programming-status.md)
- [IBM MQ — MQI programming surface](delivery/subsystems/mq/programming-status.md)
- [Cross-resource integration — Transactions and recovery](delivery/subsystems/integration/transactions-status.md)
- [Licensed certification — Differential certification](delivery/subsystems/certification/licensed-status.md)
- [Licensed certification — Stable release promotion](delivery/subsystems/certification/stable-release-plan.md)

## Operations

- [Core-server operations](runbooks/OPERATIONS.md)
- [Backup and restore](runbooks/BACKUP-RESTORE.md)
- [Capacity and recovery](runbooks/CAPACITY-AND-RECOVERY.md)
- [Local Jenkins](runbooks/JENKINS-LOCAL.md)
- [CardDemo operator guide](runbooks/CARDDEMO-OPERATOR.md)
- [CardDemo READACCT run bundle](runbooks/CARDDEMO-READACCT-BUNDLE.md)
- [CICS licensed pilot](runbooks/cics-licensed-pilot.md)
- [Conformance family rollout](runbooks/conformance-family-rollout.md)

## Releases, reviews, and research

- [0.8 release notes](releases/0.8.md)
- [Pre-0.9 deep review](reviews/PRE-0.9.0-DEEP-REVIEW.md)
- [Review index](reviews/README.md)
- [IBM official coverage roadmap](research/IBM-OFFICIAL-COVERAGE-ROADMAP.md)
- [Publication source probe](research/publication-source-probe.md)
- [CICS behavioral pilot](research/cics-behavioral-conformance-pilot.md)
<!-- END GENERATED DOCUMENTATION NAVIGATION -->

## Documentation governance

[`documentation-registry.json`](documentation-registry.json) identifies every
normative document, supplies the generated navigation above, and declares the
checked manifest location. It also owns subsystem phases, progress records,
prompts, target releases, and completion dependencies. Subsystem navigation and
indexes are generated from that mapping. Each normative document must state its status,
owner, scope, and first applicable mainframe-env version near its title.

Run `cargo xtask docs` after an intentional documentation change, then run
`cargo xtask docs --check`. The check verifies the generated
[`documentation-manifest.json`](generated/documentation-manifest.json), relative
links and anchors, documented xtask subcommands and options, normative metadata,
navigation, subsystem ownership and dependency consistency, and the
released/development version authorities.

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
