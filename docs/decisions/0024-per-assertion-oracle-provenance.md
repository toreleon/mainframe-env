# ADR-0024: Per-assertion oracle provenance for CardDemo base batch

Status: **Proposed for the first increment of issue #245**
Owner: **conformance and xtask maintainers**
Scope: **CardDemo jes.execution base-batch expected values and conformance credit**
Applies from: **mainframe-env current subsystem contracts**

## Context

The jes.execution CardDemo base-batch receipt pins statuses, counts, dataset and spool
digests, and a journey digest. It does not identify an independent source for
each expected value. Replaying those pins is useful regression evidence, but a
self-recorded result cannot establish conformance to an external system.

## Decision

The v2 evidence contract gives every correctness leaf an
`expected_value_provenance` entry with its JSON Pointer `assertion_path`,
`source`, `source_id`, `source_digest`, and `source_locator`. The four source
classes are `self-recorded`, `independent-reference`, `customer-captured`, and
`licensed-ibm`. The validator checks exact coverage, duplicate and unknown
paths, source classes, and self-recorded bindings to the historical v1 or
refreshed v2 receipt. A missing attribution has zero conformance credit. The
validator derives credit from validated provenance; evidence cannot edit a
`credit` field. Self-recorded leaves remain regression evidence with zero
conformance credit. Refreshes leave the historical v1 receipt unchanged.

A `licensed-ibm` label alone grants no licensed equivalence. That still
requires CER-1701 protected attestation and the relevant subsystem validators
under `conformance/subsystems/certification/schemas/oracle-harness-*.schema.json`. Until those
checks establish authority for an assertion, the CardDemo reader reports it
as licensed pending and gives it zero credit.

For documentation only, the source classes map to modernize-ai's
`OracleAuthority` as follows: `self-recorded` approximately corresponds to
`DevelopmentOnly`; `customer-captured` approximately corresponds to
`CustomerCaptured`; and `licensed-ibm` approximately corresponds to
`AuthoritativeZos` **only** for signed direct z/OS runs. Keep
`independent-reference` distinct. This mapping creates no code dependency.

## Consequences

### First independent reference: TRANREPT

The CardDemo jes.execution TRANREPT reference binds a product-captured, fixed-record
`TRANSACT.BKUP.G0002V00` input to a separately compiled GnuCOBOL translation
of the JCL SORT/INCLUDE card and the unmodified corpus `CBTRN03C` program.
It tests the selected records and report stage for that exact input. The input
comes from this implementation's upstream jobs, so this reference does not
validate their posting or backup behavior. GnuCOBOL is an independent
implementation, not IBM authority, and this reference earns no licensed
credit. Only receipt leaves whose independently derived values match are
eligible for `independent-reference` credit; divergent leaves retain their
prior self-recorded attribution. A dataset digest also frames the dataset's
attributes, so a digest is eligible only when those attributes are derived
independently too. The first increment credits the selected and report record
counts (2 of 90). `TRANSACT.DALY` records match, but its framed attributes are
the product's own (see #267). The report digest differs because of the insertion
comma defect #229. The captured input also carries the upstream defect #266
(interest transaction IDs without the PARM date); this is in scope only as a
pinned input.

**Compatibility impact:** this is an additive versioned evidence contract.
The v1 jes.execution artifact and historical profile.carddemo CD-023 receipt remain readable and
unchanged; readers assign their unattributed assertions zero credit. The new
v2 artifact supersedes v1 for the ordinary CardDemo base-batch check. Under
ADR-0004's expand/migrate/contract sequence, this increment expands the
reader to accept v1 and v2, migrates the current pins into a v2 artifact with
honest self-recorded attribution, and defers any contraction of v1 support to
a separately governed compatibility change. No durable state migration or
execution semantics change.
