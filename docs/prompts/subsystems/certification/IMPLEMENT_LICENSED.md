# Execution Prompt — Licensed certification — Differential certification

Subsystem: **certification**
Phase: **licensed**

Completion dependencies: zosmf.rest, integration.transactions

Use this prompt from the repository root. The
[common execution contract](../README.md#common-execution-contract) is normative.
Apply its [hardened slice acceptance](../README.md#hardened-slice-acceptance),
[early participant contract](../README.md#early-transaction-participant-contract),
and [licensed-harness preparation](../README.md#licensed-harness-preparation)
requirements alongside the version-specific boundaries below.

---

You are implementing **mainframe-env certification.licensed: licensed IBM differential
certification and 1.0 rehearsal**. This minor closes evidence and defects on one
unchanged candidate; it does not publish 1.0.

## Read and verify first

Read `docs/prompts/subsystems/README.md`,
`docs/delivery/subsystems/certification/licensed-plan.md`, every pinned official baseline and
coverage ledger, release/security/durability/provenance contracts, and accepted
zosmf.rest plus integration.transactions evidence. Verify the full route/profile/provider closure and
all migration heads before creating the certification candidate.

Licensed IBM systems, credentials, product media and raw oracle data stay
outside production/release closure. If the required environment is unavailable,
prepare reproducible harnesses and mark the final differential gate blocked; do
not fabricate a pass.

The accepted dataset.data implementation enters this gate with its dataset/VSAM/AMS
licensed differential explicitly pending at 0/36. Its independent reference
simulation is development assurance only and grants no CER-1702 or differential
credit. CER-1702 must run the real pinned 36-row dataset campaign and produce a
reviewed candidate-bound licensed receipt before release certification can pass.

The accepted cobol.execution implementation enters this gate with its Enterprise COBOL 6.5
licensed differential explicitly pending at 0/153. Its approved 16-case
GnuCOBOL reference campaign is local development assurance only and grants no
CER-1702 or IBM differential credit. CER-1702 must run the real pinned 153-row
COBOL campaign and produce a reviewed candidate-bound licensed receipt before
release certification can pass.

The accepted jes.execution implementation enters this gate with its z/OS 3.2/JES2
licensed differential explicitly pending at 0/16. Hercules, MVS 3.8J, local
models, CardDemo, and current-product observations are development assurance
only and grant no CER-1702 or differential credit. CER-1702 must run the real
pinned 16-scenario JES2 campaign and produce a reviewed candidate-bound
licensed receipt before release certification can pass.

## Implement in this order

1. Implement **CER-1701** pinned environment manifests for product levels,
   APAR/PTF/service state, configuration, locale/CCSID, topology, authority,
   fixtures, tools, redaction and reproducible oracle commands.
2. Run **CER-1702** independent per-subsystem differential campaigns against the
   same source identity. Normalize only documented nondeterministic fields. The
   campaign must include the 34 RACF command-family and 14 RACROUTE rows deferred
   from racf.security with their licensed numerator still starting at 0/48, plus the
   deferred dataset.data dataset/VSAM/AMS 36-row campaign starting at 0/36 and the
   deferred cobol.execution Enterprise COBOL 6.5 153-row campaign starting at 0/153, plus
   the deferred jes.execution z/OS 3.2/JES2 16-scenario campaign starting at 0/16.
3. Triage each mismatch to an implementation defect, baseline correction or
   explicit out-of-scope row. Add a focused regression before every code fix and
   invalidate/rerun all affected receipts after a merge.
4. Run **CER-1703/CER-1704** cross-subsystem replay, failure/scale/soak, migration,
   backup/restore, upgrade, rollback and compatibility certification.
5. Produce **CER-1705/CER-1706** canonical evidence/provenance/SBOM/security
   closure and an independent, unchanged-source 1.0 release rehearsal.

## Early harness inputs and exact-candidate handoff

CER-1701 preparation starts alongside provider lanes: pin environment capability,
independent fixture/capture/normalization inputs and representative harness
checks through the existing shared interfaces. Revalidate those inputs for the
final candidate; earlier observations or local plumbing checks are not final
licensed campaign credit. The four recorded historical campaigns remain pending
at COBOL 0/153, RACF/SAF 0/48, dataset/VSAM/AMS 0/36 and JES2 0/16 until real
licensed receipts are accepted.

Prepare and freeze REL-1001 version/changelog/scope/migration/release metadata
before the terminal CER-1706 rehearsal. This is release preparation, not authority
to tag, publish or promote. The exact proposed 1.0 source/artifact identities must
be the ones rehearsed and independently accepted. A later source, metadata,
dependency, generated catalog, fixture, oracle or environment change invalidates
affected certification under existing policy; record the impact and rerun required
gates, never silently relabel receipts. Reuse of demonstrably unaffected evidence
requires the existing explicit identity/compatibility proof, not a docs-only waiver.

CER-1702 mismatch dispositions cannot drop a mandatory row by prose. An actual
baseline correction requires pinned provenance and review; every row remaining
mandatory must close. Final certification and separately authorized 1.0 promotion
remain distinct even when preparation overlaps.

## Reuse and architecture guardrails

- Drive licensed z/OS jobs, datasets, console, workflows, CICS, Db2, IMS, MQ,
  and z/OSMF interactions through pinned Zowe CLI/SDK or supported IBM client
  adapters where available. Do not hand-build duplicate transport clients,
  credential stores, polling loops, or native wire protocols for certification.
- Use established, pinned tools for fuzzing, concurrency/model checking,
  dependency/advisory/license/source policy, SBOM generation, artifact signing,
  provenance, and container/service fault injection. Prefer cargo-fuzz,
  Cargo/RustSec policy tooling, CycloneDX/SPDX, and cosign/Sigstore-compatible
  release verification over repository-private format implementations.
- External tools and clients are harness components only. Their successful exit,
  generated catalog, SBOM, signature, or connection does not increment IBM
  semantic coverage without the normalized candidate-bound result evidence.
- Pin tool binary/container identity, version, license, configuration, trust
  roots, credentials-by-reference, environment, normalizer, timeout, and raw
  outputs. Validate the final evidence and release artifacts independently from
  the tool that generated them.
- Reuse one cross-subsystem fixture manifest, transport adapter interface,
  normalizer/comparator, mismatch taxonomy, artifact store, evidence schema, and
  release orchestrator. Per-subsystem campaigns may add adapters and fixtures,
  not fork the certification framework.

## Version-specific invariants

- Every mandatory pinned row is complete at every applicable gate; there is no
  accepted unsupported row and partial rows never round up.
- Reference simulations (including GnuCOBOL and Hercules/MVS), modeled outputs,
  official documentation, historical receipts, and current-product
  observations cannot substitute for the deferred licensed COBOL, RACF/SAF,
  dataset/VSAM/AMS, or JES2 campaigns.
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
