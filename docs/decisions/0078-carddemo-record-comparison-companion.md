# ADR-0078: CardDemo record-comparison companion

Status: **Proposed on Foundation PR #393**
Owner: **conformance and xtask maintainers**
Scope: **issues #245 and #262, local base-batch provenance**
Applies from: **mainframe-env unreleased companion; runtime owner verification pending**

The existing v1 receipt hashes dataset attributes, versions, identities and records
with u64 framing. The independent financial source model instead hashes record
bytes with u32 lengths. Those hashes are different assertions and cannot be
substituted for one another.

Add `mainframe-env.carddemo-base-batch-comparison-evidence@1` alongside the unchanged
v1 receipt and historical v1/v2 artifacts. Its payload contains the original
receipt, three actual record comparisons, and successful checked drain/cleanup
facts. Only the complete original owner can construct its private comparison
proof. Every correctness leaf receives exact JSON Pointer attribution. Missing,
duplicate, unknown, editable-credit and mismatched source bindings are rejected.

The six count/hash leaves for POSTTRAN TCATBALF and TRANREPT selected/report
records bind to the bounded, reproducible `carddemo-financial-reference@1.json`
manifest. Its portable Python producer consumes only the 36 original source and
seed pins for the existing manual 50-account/300-transaction route. Raw oracle
fixtures stay external. The declared local Cp037/numeric editing/stable tie-order
qualifications remain; this is independent local assurance and grants no IBM or
licensed equivalence. Every legacy digest and actual attribute/version remains
self-recorded. Summary credit is derived by the production reader.

The old bare v1 API, default command output and receipt are retained. Legacy unattributed v1 artifacts
remain readable with zero credit. The separate `carddemo-base-batch-evidence --check` command emits
its companion and derived local summary only after the original jobs, comparison
set, restart, both drains and exact cleanup succeed. This is an additive evidence
contract under ADR-0004; no execution semantics, workload, deadlines or floors
change. Failure in the actual owner emits no successful companion.
