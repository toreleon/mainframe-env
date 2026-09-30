# CardDemo READACCT run bundle

Set `CARDDEMO_CORPUS_DIR` to the pinned local CardDemo checkout, then run
`cargo xtask carddemo-readacct --check`. The gate reruns the exact READACCT JCL,
compares the logical replay digest and observations with
`conformance/0.8/evidence/carddemo-readacct-bundle@1.json`, verifies five
digest-pinned modernize-ai schema fixtures, and validates the local projection
offline. The checked-in wall-clock observation belongs to the recording run;
the check deliberately allows a different wall clock.

To record a new self-recorded candidate after an intentional behavior change,
run `cargo xtask carddemo-readacct` and review the resulting evidence diff,
source identities and output bytes. The local projection has
`development-only` authority and no independent conformance credit; see
[ADR-0026](../decisions/0026-run-bundle.md).
