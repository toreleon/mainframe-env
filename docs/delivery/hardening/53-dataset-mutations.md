# Dataset evidence terminology and source-mutation campaign (#53)

The eight original cases alter **observations** and test the comparator. Keep
these useful tests, but report `observation_perturbations_rejected`, not
`mutants_killed`. The live report transcript and local certification fixture
now use the correct name. The fixture schema is explicitly bumped to
`mainframe-env.dataset-certification@2`; @1's ambiguous metric must not be
interpreted as behavioral mutation evidence. Immutable published artifacts and
historical Git revisions are not rewritten. Other historical pass counts in the
fixture have not been re-certified by this terminology change.

## Behavioral mutants are a separate executable campaign

From a clean, committed candidate, run:

```sh
python3 -B -m unittest discover -s tools/tests -p 'test_dataset_mutations.py'
python3 -B tools/dataset_mutations.py --output target/mutations/run-1
```

The runner archives that exact commit into a disposable directory, proves the
ordinary tests pass, then changes one production source at a time. Schema
`mainframe-env.source-mutations@4` records 20 named mutants in five sections:
four independent dataset-reference mutants, three CICS runtime mutants, three
numeric-edited `MOVE` runtime mutants, seven typed-decimal runtime mutants, and
three `ADD CORRESPONDING` compiler mutants.

The typed-decimal section covers the shared add primitive and rounding plus the
receiver rules reviewed for #140. Its mutants suppress successful receiver
stores when a sibling conversion fails, overwrite a failing receiver, drop the
resulting `SIZE ERROR` condition, incorrectly preserve an overflowing receiver
when no `ON SIZE ERROR` phrase exists, or evaluate and write sequentially before
all operands have been captured. These are checked by the unchanged
`machine::typed_decimal::tests::*` suite, including first/middle/last failures,
all-success and all-failure batches, condition selection, operand capture, and
shared-expression failure.

The `ADD CORRESPONDING` section covers the compiler rules reviewed for #141.
Its mutants reduce matching to an unqualified leaf name, omit the `OCCURS`
eligibility exclusion, or omit bilateral uniqueness after relative
qualification. The unchanged `framework::tests::add_corresponding_*`
conformance suite runs compiled programs against independent exact output bytes
for differing qualifier paths, documented subordinate-item exclusions,
duplicate source candidates, duplicate target candidates, and the selected
occurrence compatibility route.

The runner reruns the **unchanged normal tests** for every mutant. No
observation is fabricated and no test assertion is patched. The source copy is
restored between cases and removed at exit; the caller's worktree is not
modified. Shared Cargo build output may be used, but no cache is published by
this tool.

A valid runtime assertion failure identifies a killing test. Compile errors,
timeouts, empty selections and harness failures are not kills. Surviving mutants
are reported honestly and cause the campaign gate to fail; they are not labeled
"equivalent" automatically. Equivalent-behavior judgments require explicit
human review and supporting evidence.

The receipt records the commit/tree, toolchain, source and unchanged-test
digests, exact mutation anchors, changed-source digests, command, exit codes,
durations, killing tests, and log hashes. It reports killed, survived, invalid,
and timed-out counts separately for every section; overall success requires all
20 mutants to be killed by assertion failures from the recorded normal test
set. Preserve the receipt and **all logs** as candidate-bound CI artifacts.
They are not folded into comparator counts. The independent dataset section
grants zero product-runtime credit; each named product compiler/runtime section
records its own product-mutation credit. Every section grants zero licensed IBM
differential credit. This remains a bounded representative campaign, not a
complete mutation score or compatibility certification.
