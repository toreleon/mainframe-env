# ADR-0024: Per-assertion oracle provenance for CardDemo base batch

Status: **Proposed for the first increment of issue #245**
Owner: **conformance and xtask maintainers**
Scope: **CardDemo 0.8 base-batch expected values and conformance credit**
Applies from: **mainframe-env 0.8 evidence contract v2**

## Context

The 0.8 CardDemo base-batch receipt pins statuses, counts, dataset and spool
digests, and a journey digest. It does not identify an independent source for
each expected value. Replaying those pins is useful regression evidence, but a
self-recorded result cannot establish conformance to an external system.

## Decision

The v2 evidence contract gives every correctness leaf an
`expected_value_provenance` entry with its JSON Pointer `assertion_path`,
`source`, `source_id`, `source_digest`, and `source_locator`. The four source
classes are `self-recorded`, `independent-reference`, `customer-captured`, and
`licensed-ibm`. The validator checks exact coverage, duplicate and unknown
paths, source classes, and the self-recorded binding to the unchanged v1
receipt. A missing attribution has zero conformance credit. The validator
derives credit from validated provenance; evidence cannot edit a `credit`
field. The current v2 artifact cites v1 for all 90 assertions, so it remains
a passing regression with zero conformance credit.

A `licensed-ibm` label alone grants no licensed equivalence. That still
requires CER-1701 protected attestation and the relevant subsystem validators
under `conformance/0.17/schemas/oracle-harness-*.schema.json`. Until those
checks establish authority for an assertion, the CardDemo reader reports it
as licensed pending and gives it zero credit.

For documentation only, the source classes map to modernize-ai's
`OracleAuthority` as follows: `self-recorded` approximately corresponds to
`DevelopmentOnly`; `customer-captured` approximately corresponds to
`CustomerCaptured`; and `licensed-ibm` approximately corresponds to
`AuthoritativeZos` **only** for signed direct z/OS runs. Keep
`independent-reference` distinct. This mapping creates no code dependency.

## Consequences

**Compatibility impact:** this is an additive versioned evidence contract.
The v1 0.8 artifact and historical 0.1.1 CD-023 receipt remain readable and
unchanged; readers assign their unattributed assertions zero credit. The new
v2 artifact supersedes v1 for the ordinary CardDemo base-batch check. Under
ADR-0004's expand/migrate/contract sequence, this increment expands the
reader to accept v1 and v2, migrates the current pins into a v2 artifact with
honest self-recorded attribution, and defers any contraction of v1 support to
a separately governed compatibility change. No durable state migration or
execution semantics change.
