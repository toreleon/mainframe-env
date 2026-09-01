# Shared Conformance IR v1

This directory contains the single product-neutral Conformance IR authority
introduced by CI-300. `v1/spec.json` is the readable source compiled by
`cargo xtask spec --check`; later subsystem minors extend that one document and
the typed registries rather than creating subsystem-local harness contracts.

The CI-300 foundation deliberately claimed no official behavior. CB-301 through
CB-303 add reviewed executable claims for COBOL compiler-directing statements,
directive groups, file/data description clauses, and all 44 procedure-statement
families. Catalog rows enter the numerator only after a work package supplies a
reviewed row specification, mandatory obligations, executable bindings, and
verdict events. Verdict and ledger schemas define projections; committed
per-row or per-obligation verdict files are prohibited.

Cross-subsystem cases use the separate bounded `ScenarioSpec` section. A
scenario contains only typed participant drivers, ordered step references,
failure-point references, and exact credits to already registered
`(row, obligation, gate)` bindings; it is not a workflow or expression DSL.
