# ADR-0025: Licence and provenance policy for IBM oracle evidence

Status: **Proposed**
Owner: **repository owner**
Scope: **licensed IBM oracles, customer captures, and IBM-derived material**
Applies from: **mainframe-env 0.17 licensed-evidence planning, subject to acceptance**

## Context

Issue [#260](https://github.com/toreleon/mainframe-env/issues/260) identifies
licensed differential evidence as a release dependency, including CER-1701 and
the 0.10–0.17 campaigns. A reproducible harness does not itself establish that
an environment may be used for this project or that its output may be published.
[ADR-0024 (PR #255)](https://github.com/toreleon/mainframe-env/pull/255)
defines per-assertion oracle source classes and credit rules; this decision
governs the acquisition and handling of `licensed-ibm` and
`customer-captured` material and the provenance of IBM-derived inputs.

The existing [documentation licensing rule](../research/IBM-OFFICIAL-COVERAGE-ROADMAP.md#documentation-licensing)
allows source identifiers, URLs, edition dates, hashes, and reviewed derived
facts, while excluding IBM manuals and proprietary binaries. The repository's
IBM-source and security rules also keep raw HTML and customer data outside Git.

## Decision

### Environment authority and observation boundary

- A licensed z/OS installation on owned or rented IBM Z, a licensed zPDT/ZD&T
  installation, or a hosted IBM offering is a candidate only when the named
  licensee and the specific agreement permit this project's operator, location,
  workload, capture, retention, and intended use. No product name, access to a
  machine, or customer invitation substitutes for that review. Record the
  licensee, agreement reference and version, permitted-use statement,
  environment/product/service levels, operator, reviewer, and review date in a
  protected environment record before a licensed campaign. Keep the agreement
  and credentials outside Git.
- A customer environment is operated under the customer's authorization and
  applicable terms. The customer remains the licensee unless the relevant
  agreement identifies another party. Record who runs the tests, what may be
  captured, who may receive the result, and the agreed deletion date before
  capture. Customer access does not grant this repository an IBM licence.
- Execute pinned, independently prepared inputs and observe externally visible
  results only: return codes, messages, datasets, spool, and documented
  diagnostics. Do not disassemble or patch IBM binaries, inspect IBM code or
  internals through dumps or traces, or use them as a design source. Operators
  record provenance and observations; reviewers approve any derived assertion
  separately. Role separation alone is not a permission grant.
- The issue draft records **2026-12-31** as the zPDT end-of-support date. Treat
  that as a sourcing risk to verify for the exact zPDT/ZD&T version and licence;
  [IBM's lifecycle entry for ZD&T 13.x](https://www.ibm.com/support/pages/node/6357827)
  shows that support dates are version specific. A support date does not decide
  whether an existing licence permits continued use. Record a dated sourcing
  choice for each campaign, including a customer-capture fallback when lawful
  licensed access is unavailable; keep its licensed exit criterion pending.

### Repository boundary and provenance

- For IBM publications and licensed captures, Git may hold source identifiers,
  URLs, edition and product/version identifiers, hashes, bounded locators, and
  **zero-credit verification receipts**. Raw HTML, manuals, proprietary IBM
  binaries, unredacted oracle output, licence documents, secrets, and raw
  customer captures stay outside Git. A locator must identify a bounded source
  location without reproducing publication text. Generated inventories and
  promoted examples may include only independently worded, reviewed derived
  facts; copied passages, tables, and customer payloads are excluded. Record
  the reviewer and source identity before publication.
- Every fixture, catalog row, observation, and expected-value assertion records
  its source class, source ID, digest, bounded locator, author/operator,
  reviewer, and applicable licence or permission reference. The permission
  reference may be an opaque ID pointing to a protected record. Preserve links
  from derived rows to their source records without embedding the source bytes.
- Apply ADR-0024's classes at each assertion: `self-recorded` for values copied
  from this project's own run; `independent-reference` for independently
  authored or permitted public reference values; `customer-captured` for an
  authorized customer observation; and `licensed-ibm` for an observation from a
  reviewed IBM environment. An IBM documentation-derived fact is source context,
  not automatically an oracle value; classify it as `independent-reference`
  only when it independently supports that exact expected assertion. Public
  open-source material also requires its own licence/provenance review. Missing
  attribution earns no conformance credit. A `licensed-ibm` label alone earns
  none without ADR-0024's protected attestation and subsystem validation.

### Customer capture, review, and release claims

- Keep raw customer evidence in access-controlled storage designated by the
  customer agreement, with a named custodian, restricted recipients, retention
  period, deletion date, and deletion record. Redact credentials, tokens,
  personal/customer identifiers, paths, configuration secrets, and unrelated
  business data before review or transfer. Verify redaction before producing
  bounded digests and a zero-credit repository receipt. If required fields
  cannot be safely redacted, retain the evidence only within the agreed
  protected boundary and do not publish or claim credit from it.
- An owner-appointed reviewer records the environment authority, provenance,
  redaction, and attestation decisions as actual approvals. Pending, missing,
  or synthesized approvals do not authorize use or licensed differential
  credit. The proposed release gate keeps the affected licensed differential
  exit criterion pending and prevents a 1.0 licensed-evidence claim until the
  required campaign and approvals exist; implementation of that gate is
  follow-on work.

## Acceptance and open questions

This ADR remains **Proposed** until the owner's qualified counsel reviews the
environment, capture, derived-material, retention, and release requirements and
the owner records acceptance. This document states proposed operating rules,
not a legal conclusion. Counsel must resolve:

1. Which exact z/OS, zPDT/ZD&T, hosted ISV, or other offering agreements permit
   this project's use, including commercial use, differential testing, capture,
   transfer, and retention? Who is the licensee for each candidate environment?
2. What customer authorization, data-processing terms, and deletion periods
   permit customer-run capture and receipt sharing?
3. Which IBM publication-derived fields or examples may be published as
   reviewed derived facts, and what review evidence is required?
4. What approvals and records are required before licensed evidence may count
   toward a release or a 1.0 tag?

## Consequences and scope

This proposal adds no oracle credit or approval. Schema changes, provenance
backfill, source procurement, a dated campaign sourcing decision, CER-1701 and
release-gate implementation, and updates to 0.10–0.17 exit criteria require
separate work. It does not resolve #157's federated-runtime licence matrix,
change language or subsystem semantics, or decide freedom to operate.
