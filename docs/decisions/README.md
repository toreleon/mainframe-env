# Architecture Decision Records

ADRs capture decisions that constrain implementation and public contracts.

[ADR-0047](0047-carddemo-participant-completion.md) records the implemented
CardDemo participant ownership and batch completion boundary.

| ADR | Decision | Status |
|---|---|---|
| [0001](0001-technology-stack.md) | initial technology stack and deferred frameworks | Accepted |
| [0002](0002-deterministic-core.md) | deterministic core with asynchronous shell | Accepted |
| [0003](0003-contract-serialization.md) | owned contracts, codecs, identity, and schema evolution | Accepted |
| [0005](0005-package-consolidation.md) | historical twenty-package physical map and split triggers | Superseded by 0009 |
| [0006](0006-carddemo-profile.md) | additive profile.carddemo CardDemo-full profile and new split triggers | Accepted |
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
| [0023](0023-bts-lifecycle-authority.md) | shared versioned BTS process/activity state and UOW acquisition | Proposed |
| [0024](0024-per-assertion-oracle-provenance.md) | per-assertion CardDemo base-batch oracle provenance and derived credit | Proposed |
| [0025](0025-licence-and-provenance-policy.md) | licence and provenance policy for IBM oracle evidence | Proposed |
| [0026](0026-run-bundle.md) | self-recorded CardDemo READACCT run bundle and logical replay digest | Proposed |
| [0027](0027-cics-logical-program-frames.md) | shared CICS task ownership and bounded logical program frames | Proposed |
| [0028](0028-explicit-bts-set-loan-lifetime.md) | bounded explicit BTS SET lifetime in existing checked storage | Proposed |
| [0029](0029-cobol-storage-entry-identity.md) | canonical language storage scopes within one CICS task | Proposed |
| [0030](0030-cics-source-condition-authority.md) | common CICS response authority for private source validation | Proposed |
| [0028 (Db2)](0028-db2-typed-catalog-evolution.md) | typed Db2 catalog evolution through signed packages, existing generations and versioned persistence | Proposed |
| [0028 (MQ)](0028-mq-shared-handle-kernel.md) | one volatile MQ handle authority across properties and pub/sub | Proposed |
| [0029 (publication)](0029-audited-provider-publication.md) | audited provider publication under one retained core intent | Proposed |
| [0029 (Db2)](0029-db2-core-participant-evolution.md) | versioned local Db2 core participant binding before mutating integration, preserving frozen CICS v1 | Proposed |
| [0030](0030-mq-host-lifecycle-directory.md) | private volatile MQ lifecycle directory with explicit persisted restart fencing prerequisite | Proposed |
| [0031](0031-mq-selected-service-authority.md) | one selected legacy/rich MQ service authority and strict same-store opener | Proposed |
| [0032](0032-mq-program-machine-frame.md) | explicit installed-batch MQI frame and original machine connection effects | Proposed |
| [0033](0033-mq-full-md-value.md) | complete lossless MQMD1/2 value and bounded separate canonical value codec | Proposed |
| [0033](0033-mq-historical-handle-observation.md) | historical MQ canonical handle identity without live registry authority | Proposed |
| [0035](0035-mq-root-scoped-abi-aliases.md) | explicit volatile SAME TASK connection aliases and atomic machine writeback | Proposed |
| [0036 (conformance)](0036-conformance-pending-obligations.md) | explicit mandatory pending obligations and bounded selected MQ evidence | Proposed |
| [0037](0037-running-step-program-transport.md) | opaque revocable Running-step observation and neutral Program context transport; activation pending | Accepted (bounded transport) |
| [0038](0038-coordinator-original-dispatch-extraction.md) | one private original HostCall implementation borrowed by the real coordinator drive; controller activation pending | Accepted (bounded extraction) |
| [0040](0040-batch-run-stop-containment.md) | genuine opt-in Batch run owner, opaque exit and irrevocable next-action stop; host activation pending | Accepted (bounded prerequisite) |
| [0043](0043-batch-contained-all-effect-loan.md) | explicit contained all-effect occurrence transport with external audit ownership; enclosing controller activation pending | Accepted (bounded transport) |
| [0045](0045-batch-prepared-selection-plan.md) | private bounded exact selection observation with complete Job/configuration revalidation; atomic joined admission pending | Accepted (bounded prerequisite) |
| [0046](0046-batch-bounded-provider-prefetch.md) | bounded provider namespace byte/shape preflight before payload materialization; joined admission pending | Accepted (bounded prerequisite) |
| [0034](0034-gsam-logical-address.md) | bounded public GSAM logical addresses using existing engine, PCB and row authorities | Proposed |
| [0031](0031-ims-tm-recovery-publication.md) | shared work/effect-fenced IMS TM recovery publication and settlement | Proposed |
| [0035](0035-selected-pcb-feedback.md) | versioned owned selected PCB feedback retained in the existing proposal and receipt | Proposed |
| [0033](0033-cobol-dli-call-boundary.md) | owned raw COBOL DL/I CALL frame, PCB entry binding and feedback contract proposal | Proposed |
| [0030](0030-gsam-application-record-formats.md) | explicit GSAM application formats, owned U length and checkpoint format identity | Proposed |
| [0032](0032-selected-secondary-checkpoint-position.md) | selected secondary checkpoint occurrence witnesses composed with existing local backout | Proposed |
| [0050](0050-host-effect-envelope-layout.md) | exclusive boxed outer MQI host payloads with unchanged canonical encoding | Accepted; scoped compatibility passed |

| [0051](0051-package-identity-framing.md) | framed current package domain and finite trusted legacy recovery | Accepted; scoped compatibility passed |
| [0052](0052-cics-operation-contexts.md) | borrowed CICS operation inputs and existing explicit receipt retention export | Accepted; scoped compatibility passed |
| [0053](0053-selected-controller-publication-fence.md) | local shared admission fence and publication prevalidation | Accepted design; implementation pending |
| [0054](0054-focused-conformance-output.md) | canonical focused JSONL output and bounded sink publication | Accepted; scoped compatibility passed |
| [0055](0055-standard-package-mac-envelope.md) | bounded COSE_Mac0 profile and exact retained verification | Bounded production MAC implementation accepted; full Foundation pending |
| [0056](0056-cics-first-reverse-position.md) | additive retained-position Dataset observation for the implicit CICS first reverse read | Accepted bounded design; verification pending |

Parallel subsystem work allocated the same 0028 and 0029 numeric prefixes.
Their distinct full filenames and subsystem-qualified labels identify each
proposal; no historical decision body or reference has been replaced.

An accepted ADR is immutable. A material change creates a superseding ADR and
links to the previous decision.
