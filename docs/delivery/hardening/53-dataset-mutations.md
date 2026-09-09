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
ordinary tests pass, then changes one production source at a time. The
independent dataset section reverses KSDS order, omits AIX publication,
publishes a failed transaction, and omits GDG scratch. Separate product-runtime
sections mutate CICS file/UOW transitions, numeric-edited `MOVE`, and the typed
decimal ADD/COMPUTE path. The typed-decimal mutations corrupt the shared add
primitive, ignore receiver rounding, and violate staged atomic assignment. The
runner reruns the **unchanged normal tests** for every mutant. No observation is
fabricated and no test assertion is patched. The source copy is restored
between cases and removed at exit; the caller's worktree is not modified.
Shared Cargo build output may be used, but no cache is published by this tool.

A valid runtime assertion failure identifies a killing test. Compile errors,
timeouts, empty selections and harness failures are not kills. Surviving mutants
are reported honestly and cause the campaign gate to fail; they are not labeled
"equivalent" automatically. Equivalent-behavior judgments require explicit
human review and supporting evidence.

The receipt records the commit/tree, toolchain, source and test digests, exact
mutation anchors, changed-source digests, identical command, exit codes,
durations, killing tests, and log hashes. Preserve the receipt and **all logs**
as candidate-bound CI artifacts. They are not folded into comparator counts.
The independent dataset section grants zero product-runtime credit; each named
product section records its own product-runtime credit. Every section grants
zero licensed IBM differential credit. This remains a bounded representative
campaign, not a complete mutation score or compatibility certification.
