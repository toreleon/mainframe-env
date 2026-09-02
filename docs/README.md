# Documentation Index

This documentation is normative for the mainframe-env greenfield
rewrite. When prose conflicts, the order below defines precedence.

1. [mainframe-env charter](CHARTER.md)
2. [Architecture overview](architecture/OVERVIEW.md)
3. [Compiler and IR architecture](architecture/COMPILER-AND-IR.md)
4. [Execution and durability architecture](architecture/EXECUTION-AND-DURABILITY.md)
5. [Security and capability architecture](architecture/PLUGIN-AND-SECURITY.md)
6. [Package map](architecture/PACKAGE-MAP.md)
7. Accepted architecture decisions in [`decisions/`](decisions/)
8. Delivery and verification contracts in [`delivery/`](delivery/)
9. [Long-horizontal 0.1 implementation prompt](prompts/IMPLEMENT_MAINFRAME_ENV_0_1.md)

## Architecture decisions

- [ADR-0001: Technology stack](decisions/0001-technology-stack.md)
- [ADR-0002: Deterministic core and asynchronous shell](decisions/0002-deterministic-core.md)
- [ADR-0003: Contract and serialization policy](decisions/0003-contract-serialization.md)
- [ADR-0004: v0.1 versioning and release policy](decisions/0004-versioning-release-policy.md)

## Delivery contracts

- [Verification strategy](delivery/VERIFICATION-STRATEGY.md)
- [Compatibility and cutover](delivery/COMPATIBILITY-AND-CUTOVER.md)
- [Implementation roadmap](delivery/IMPLEMENTATION-ROADMAP.md)
- [Versioning and release gates](delivery/VERSIONING-AND-RELEASES.md)
- [GitHub Project synchronization](delivery/coverage-versions/GITHUB-PROJECT.md)
- [CardDemo reached compatibility copybooks](compatibility/CARDDEMO-COPYBOOKS.md)

## Document status vocabulary

| Status | Meaning |
|---|---|
| Proposed | Design input, not yet an implementation contract |
| Accepted | Normative for new mainframe-env work |
| Implemented | Present in mainframe-env code with focused verification |
| Proven | Required cross-layer, failure, compatibility, and operational evidence passes |
| Superseded | Replaced by a newer versioned decision |

Only an accepted decision may define a stable public boundary. Implementation
alone does not make a contract stable.
