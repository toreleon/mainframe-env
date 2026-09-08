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
-911, 0 and 99999. MOVE CORRESPONDING, group moves, decimal fractions,
overflow, national/UTF-8, other editing pictures and licensed differential work
remain outside this subset.

## 2. Pin the finite source closure

Reuse the documentation content endpoint and topic-manifest digest contract.
Follow only links required to interpret the selected behavior, within explicit
topic/byte/depth limits. The MOVE proof uses five already-pinned Enterprise
COBOL 6.5 topics: MOVE, elementary moves, elementary move rules, valid/invalid
elementary moves, and alignment rules. The manifest contains 57,772 bytes by
digest; no publication body is retained in the repository.

Generate the review projection from external snapshots:

```text
python3 conformance/0.9/tools/extract_cobol_move_rules.py \
  --topics /outside/repository/cobol-move-pilot \
  --manifest conformance/0.9/manifests/cobol-numeric-move-topics.json \
  --compile-rules conformance/0.9/cobol/move-compile-rules.json \
  --output conformance/0.9/generated/cobol-move-semantic-candidates.json
```

The exercised projection inventories 154 fragments: seven candidates and 147
explicit outside-scope dispositions, with zero unsupported fragments and zero
conflicts. It preserves the structural table row that authorizes numeric
integer to numeric-edited MOVE, rather than treating table text as unstructured
prose.

## 3. Require independent review

Review every candidate and the aggregate disposition of every source fragment.
Correct or reject interpretations in the review artifact; never promote an
extractor count, schema-valid JSON or model self-review. A source, locator,
fragment digest, release or review mismatch must fail `spec --check`.

The exercised MOVE artifact is still `pending-maintainer`, so it grants no new
claim. This is expected fail-closed behavior, not a skipped gate.

## 4. Bind obligations without forking the framework

Reuse the existing official row, `RowSpec`, mandatory obligations, cases,
typed registries, shared scenario runner, environment identity, verdict events
and derived ledger. Add only family-specific compile rules, source shapes and
observation types required by the reviewed rule.

For MOVE, the proposed new obligation is one exact numeric-move byte result.
The old broad `runtime-normal` binding and new scoped binding overlap; the
review artifact therefore defines a cutover rather than allowing two current
claim authorities. Historical verdicts remain bound to their original spec.

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
and upper-bound values produce exact independent bytes. A comparator
perturbation that removes the negative sign fails.

## 6. Prove product adequacy

Extend the existing disposable source-copy mutation campaign; do not create a
new receipt family. Run the unchanged normal scenario against a product-source
mutant. The MOVE campaign changes the interpreter's floating-minus output and
requires the unchanged exact-byte scenario to fail. Compile errors, empty test
selection, timeouts or harness failures receive no kill credit.

## 7. Select appropriate CI tiers

Manifest, review, fixture, comparator, compiler/interpreter and shared-contract
changes must select non-empty fast checks and the bounded mutation tier.
Ordinary PR assurance does not claim full/release or licensed execution.
Restart, broad product mutation and licensed campaigns remain cost-appropriate
tiers with explicit `not-run` states when skipped.

## 8. Cut over after acceptance

After maintainer review, add the exact scenario/credit binding, run the old and
new paths on the same candidate, investigate any disagreement against the
reviewed source, remove only the overlapping current coverage authority, and
recompute current claims under the new spec digest. Keep useful unit tests and
immutable historical evidence. Report the scoped result and all exclusions;
never describe this MOVE subset as whole-language coverage.

## Reuse report from the exercised proof

- Reused unchanged: topic retrieval/digests, structural fragment engine,
  review/disposition boundary, `RowSpec`/case registries, scenario observation
  set equality, verdict/cache/ledger identity, Jenkins selector and source-copy
  mutation envelope.
- Family-specific: five-topic source closure, seven compile rules, one
  numeric-edited byte observation, one fixture, and one product mutant.
- New generic primitives: none beyond the scenario driver and reviewed-rule
  identity already required by the CICS pilot.
- Unsupported/conflicting fragments: zero/zero in the selected MOVE corpus.
- Reviewer corrections: pending; promotion and old/new cutover are blocked until
  a maintainer records acceptance or changes.
