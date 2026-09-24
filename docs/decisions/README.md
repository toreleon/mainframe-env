# Architecture Decision Records

ADRs capture decisions that constrain implementation and public contracts.

| ADR | Decision | Status |
|---|---|---|
| [0001](0001-technology-stack.md) | 0.1 technology stack and deferred frameworks | Accepted |
| [0002](0002-deterministic-core.md) | deterministic core with asynchronous shell | Accepted |
| [0003](0003-contract-serialization.md) | owned contracts, codecs, identity, and schema evolution | Accepted |
| [0004](0004-versioning-release-policy.md) | v0.1 product, contract, phase-commit, and release policy | Accepted |
| [0005](0005-package-consolidation.md) | historical twenty-package physical map and split triggers | Superseded by 0009 |
| [0006](0006-carddemo-0.1.1-profile.md) | additive 0.1.1 CardDemo-full profile and new split triggers | Accepted |
| [0007](0007-carddemo-0.1.1-release.md) | release CardDemo-full 0.1.1 and canonicalize CARDDEMO naming | Accepted |
| [0008](0008-icu-license-compliance.md) | retain the locked decNumber dependency under ICU and ship complete notices | Accepted |
| [0009](0009-current-package-topology.md) | current 26-package topology and change governance | Accepted |
| [0010](0010-rust-module-review-budgets.md) | hard Rust module budgets, facade ratchet, and CICS family layout | Accepted |
| [0011](0011-typed-language-hir-and-semantic-ir.md) | language-specific HIR, semantic IR dialects, and typed host effects | Accepted |
| [0012](0012-checked-amode64-storage-boundary.md) | checked AMODE(64) virtual storage and checkpoint boundary | Proposed |
| [0013](0013-cics-web-service-control-boundary.md) | bounded CICS web service control and durable channel replay | Proposed |
| [0014](0014-named-counter-authority.md) | versioned named-counter pool authority and bounded typed routing | Proposed |
| [0015](0015-cics-operator-reply-boundary.md) | durable CICS operator reply and console ingress boundary | Proposed |
| [0016](0016-cics-tcpip-ingress-context.md) | trusted TCP/IP and client-certificate task context | Proposed |
| [0017](0017-cics-immediate-start-target-authority.md) | durable local target admission for immediate CICS START | Proposed |
| [0018](0018-cics-start-attach-lifetime.md) | noncancelable START ATTACH state and live-address boundary | Proposed |
| [0019](0019-bts-lifecycle-authority.md) | shared versioned BTS process/activity state and UOW acquisition | Proposed |
| [0020](0020-conversation-peer-exchange-ledger.md) | shared APPC/MRO ledger and explicit durable peer frames | Proposed |
| [0021](0021-conversation-issue-control-staging.md) | staged ISSUE controls and confirmed partner transitions in the shared ledger | Proposed |
| [0022](0022-issue-pass-target-handoff.md) | replayable ISSUE PASS target claim and CICS logon data handoff | Proposed |

An accepted ADR is immutable. A material change creates a superseding ADR and
links to the previous decision.
