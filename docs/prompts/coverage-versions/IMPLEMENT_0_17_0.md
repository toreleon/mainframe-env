# Execution Prompt — Implement mainframe-env 0.17.0

Target version: **0.17.0**  
Completion dependencies: 0.11.0, 0.16.0

Use this prompt from the repository root. The
[common execution contract](README.md#common-execution-contract) is normative.

---

You are implementing **mainframe-env 0.17.0: licensed IBM differential
certification and 1.0 rehearsal**. This minor closes evidence and defects on one
unchanged candidate; it does not publish 1.0.

## Read and verify first

Read `docs/prompts/coverage-versions/README.md`,
`docs/delivery/coverage-versions/0.17.0.md`, every pinned official baseline and
coverage ledger, release/security/durability/provenance contracts, and accepted
0.11.0 plus 0.16.0 evidence. Verify the full route/profile/provider closure and
all migration heads before creating the certification candidate.

Licensed IBM systems, credentials, product media and raw oracle data stay
outside production/release closure. If the required environment is unavailable,
prepare reproducible harnesses and mark the final differential gate blocked; do
not fabricate a pass.

## Implement in this order

1. Implement **CER-1701** pinned environment manifests for product levels,
   APAR/PTF/service state, configuration, locale/CCSID, topology, authority,
   fixtures, tools, redaction and reproducible oracle commands.
2. Run **CER-1702** independent per-subsystem differential campaigns against the
   same source identity. Normalize only documented nondeterministic fields.
3. Triage each mismatch to an implementation defect, baseline correction or
   explicit out-of-scope row. Add a focused regression before every code fix and
   invalidate/rerun all affected receipts after a merge.
4. Run **CER-1703/CER-1704** cross-subsystem replay, failure/scale/soak, migration,
   backup/restore, upgrade, rollback and compatibility certification.
5. Produce **CER-1705/CER-1706** canonical evidence/provenance/SBOM/security
   closure and an independent, unchanged-source 1.0 release rehearsal.

## Version-specific invariants

- Every mandatory pinned row is complete at every applicable gate; there is no
  accepted unsupported row and partial rows never round up.
- Inputs, outputs, traces, diagnostics, statuses, environment identities and
  normalization rules are retained with bounded/redacted evidence and digests.
- A mismatch cannot be waived by changing expected output to match the product.
  Baseline corrections require provenance and review.
- Per-subsystem campaigns may run in parallel, but fixes merge one at a time;
  final cross-system replay and evidence digest run on one unchanged candidate.
- Do not tag, publish, deploy or claim whole IBM ecosystem parity.

## Completion gate

Do not finish until every mandatory programming-surface row passes all applicable
gates; all licensed campaigns have reproducible reviewed receipts; architecture,
de-hardcoding, dependency/license/advisory, fuzz, load, soak, failure, recovery,
migration, backup/restore and rollback gates pass; an independent reviewer can
reproduce the pack; and the exact proposed 1.0 source completes rehearsal without
code changes.

At handoff, provide the canonical coverage/evidence digest, environment and
source/artifact identities, all mismatch dispositions, independent review,
rehearsal results and any genuine blocker. Leave 1.0 promotion to its separately
authorized release gate.
