# Incremental conformance-family rollout

This workflow was exercised by the COBOL elementary numeric MOVE pilot. It is
not a proposal to migrate every subsystem at once, and it does not create
placeholder registries for families that have no implementation owner.

## 1. Freeze one implemented behavior

Choose one official catalog row and a smaller behavioral subset whose product
route already exists. Record exact sender/receiver shapes, compiler options,
environment, positive/negative/boundary values and exclusions. For the exercised
proof this is the existing MOVE statement row, a signed `S9(9) COMP-5` sender,
a `PIC ----9` numeric-edited receiver, default compiler options, and values 100,
-911, 0 and the five-digit truncation probe 99999. MOVE CORRESPONDING, group moves, decimal fractions,
overflow, national/UTF-8, other editing pictures and licensed differential work
remain outside this subset.

## 2. Pin the finite source closure

Reuse the documentation content endpoint and topic-manifest digest contract.
Follow only links required to interpret the selected behavior, within explicit
topic/byte/depth limits. The MOVE proof uses six already-pinned Enterprise
COBOL 6.5 topics: MOVE, elementary moves, elementary move rules, valid/invalid
elementary moves, alignment rules, and floating insertion editing. The manifest contains 69,858 bytes by
digest; no publication body is retained in the repository.

Generate the review projection from external snapshots:

```text
python3 conformance/0.9/tools/extract_cobol_move_rules.py \
  --topics /outside/repository/cobol-move-pilot \
  --manifest conformance/0.9/manifests/cobol-numeric-move-topics.json \
  --compile-rules conformance/0.9/cobol/move-compile-rules.json \
  --output conformance/0.9/generated/cobol-move-semantic-candidates.json
```

The exercised projection inventories 166 fragments: eight candidates and 158
explicit outside-scope dispositions, with zero unsupported fragments and zero
conflicts. It preserves the structural table row that authorizes numeric
integer to numeric-edited MOVE and the bounded floating-insertion list that
requires an extra insertion position to avoid truncation.

## 3. Require independent review

Review every candidate and the aggregate disposition of every source fragment.
Correct or reject interpretations in the review artifact; never promote an
extractor count, schema-valid JSON or model self-review. A source, locator,
fragment digest, release or review mismatch must fail `spec --check`.

The exercised MOVE artifact is still `pending-maintainer`, so it grants no new
claim. This is expected fail-closed behavior, not a skipped gate.

## 4. Bind obligations without forking the framework

Reuse the existing official row, `RowSpec`, mandatory obligations, cases,
typed registries, shared runner, environment identity, verdict events
and derived ledger. Add only family-specific compile rules, source shapes and
observation types required by the reviewed rule.

MOVE is a single-language compiler/interpreter path, so it uses one ordinary
`ConformanceCase` driver and does not create a `ScenarioSpec`, synthetic
readback participant, failure point, or CICS/UOW primitive.

For MOVE, the proposed new obligation is one exact numeric-move byte result.
The existing `runtime-normal` binding covers an alphanumeric literal moved to
`PIC X(8)`; the new binding covers signed COMP-5 to floating-minus
numeric-edited conversion. They do not overlap, so both remain as the sole
claim authority for their respective scope. Historical verdicts and IDs remain
bound to their original spec.

## 5. Execute the real product route

The MOVE driver compiles the fixture source with the product compiler, executes
the published artifact in the product interpreter, and captures raw output
bytes. Its comparator reads the expected hex string from the reviewed fixture;
it does not invoke the product's decimal or numeric-editing helpers.

```text
cargo test -p mainframe-env-conformance \
  cobol_move_pilot::tests:: -- --test-threads=1
```

The command was exercised locally. The positive, negative-sign, zero/padding
and truncation-boundary values produce exact independent bytes. `99999` moved
to `PIC ----9` yields space plus `9999`, matching the pinned IBM capacity rule
and an external GnuCOBOL control. A comparator perturbation that removes the
negative sign fails.

## 6. Prove product adequacy

Extend the existing disposable source-copy mutation campaign; do not create a
new receipt family. Run the unchanged normal case against a product-source
mutant. The MOVE campaign separately removes the reserved floating insertion
position and suppresses the floating-minus output; the unchanged exact-byte
case must kill both. Compile errors, empty test selection, timeouts or
harness failures receive no kill credit.

## 7. Select appropriate CI tiers

Manifest, review, fixture, comparator, compiler/interpreter and shared-contract
changes must select non-empty fast checks and the bounded mutation tier.
Ordinary PR assurance does not claim full/release or licensed execution.
Restart, broad product mutation and licensed campaigns remain cost-appropriate
tiers with explicit `not-run` states when skipped.

## 8. Cut over after acceptance

After maintainer review, add the exact case/credit binding, run the old and
new paths on the same candidate, investigate any disagreement against the
reviewed source. Retain the old binding because its alphanumeric scope is
distinct; add the numeric-edited obligation without creating duplicate credit
for either behavior. Recompute current claims under the new spec digest. Keep
useful unit tests and immutable historical evidence. Report the scoped result
and all exclusions; never describe this MOVE subset as whole-language coverage.

## Reuse report from the exercised proof

- Reused unchanged: topic retrieval/digests, structural fragment engine,
  review/disposition boundary, `RowSpec`/case registries, ordinary driver and
  observation binding, verdict/cache/ledger identity, Jenkins selector and source-copy
  mutation envelope.
- Family-specific: six-topic source closure, eight compile rules, one
  numeric-edited byte observation, one fixture, and two product mutants.
- New generic primitives: none; the reviewed-rule identity already required by
  the CICS pilot is reused.
- Unsupported/conflicting fragments: zero/zero in the selected MOVE corpus.
- Reviewer corrections: the one-byte truncation expectation, missing
  floating-insertion source, and non-overlapping binding decision are applied;
  promotion remains blocked until the maintainer accepts the corrected set.
