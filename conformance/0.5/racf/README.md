# RACF/SAF 0.5 conformance inputs

The command, RACROUTE, and supplied-class catalogs in this directory project
into the shared Conformance IR. They do not define a second RACF authority.

The 0.5 development gate also runs a bounded, table-driven, pure-state
reference simulation over the same 34 command and 14 RACROUTE row identities.
It is intentionally independent of the production provider and has explicit
unknowns for installation exits, cryptographic material, undocumented database
internals, and RRSF transport timing. Its property, metamorphic, and mutant
tests provide development assurance only: they do not create a campaign file,
an oracle receipt, a differential binding, or licensed credit.

Licensed differential is enabled only when an approved adapter installs
`licensed-oracle.json` at this path. The file must validate against
`../schemas/racf-oracle-campaign.schema.json`, cover exactly the frozen 34
command and 14 RACROUTE rows, identify IBM z/OS 3.2 RACF/SAF, assert that the
environment is licensed, carry the approved adapter and licensed-environment
identity prefixes, declare external licensed execution and a fresh
release-certify campaign, and contain no credential, key, token, certificate,
or MFA material. Simulated, modeled, documentation-derived, historical, and
current-product origins fail before registry projection. Absence of the file
means `differential=pending`.

Each case binds the exact generated differential fixture digest and carries a
normalized JSON observation produced by the licensed adapter:

- command: `surface`, `keyword`, `status`, and redacted `records`;
- RACROUTE: `surface`, `keyword`, `status`, `states`, and redacted `result`.

The product driver executes the same selected route on the current candidate
and requires JSON equality with the adapter observation. Only then does it emit
the campaign file digest through the shared runner's oracle-receipt field. The
shared cache identity already binds candidate, catalog, spec, fixture, oracle,
environment, shard, row, gate, and obligation identities.

The campaign is an imported receipt, not a hand-authored expectation. After an
approved adapter supplies it, run:

```text
cargo xtask racf-catalog
cargo xtask spec --check
cargo xtask conformance --subsystem racf-saf --gate differential
```

Do not use official documentation, local product output, or a synthetic fixture
as a substitute for the licensed IBM execution receipt. Under the user-approved
2026-09-01 scoped completion policy, its absence keeps 0.5 at
`differential=0/48 pending`; the real campaign remains mandatory at the 0.17
`release-certify` hard gate.
