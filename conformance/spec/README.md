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

CB-304 strengthens the existing 27 clause bindings without changing the
official denominator: each accepted clause case now carries independently
reviewed layout expectations for data class, USAGE, size/alignment, table and
alias metadata, initialization, allocation, and provenance where applicable.

CB-305 adds all 82 intrinsic-function rows with separate valid-signature
recognition and invalid-signature validation obligations. Their independent
fixture catalog checks exact inferred result type, fixed length, generated
identity, and provenance; the generated special-register registry is closed by
28 reviewed metadata expectations without creating non-official coverage rows.

Cross-subsystem cases use the separate bounded `ScenarioSpec` section. A
scenario contains only typed participant drivers, ordered step references,
failure-point references, and exact credits to already registered
`(row, obligation, gate)` bindings; it is not a workflow or expression DSL.

Publication-derived drafts live in `candidates/`, outside the accepted
`v1/spec.json` registry. The shared typed candidate container embeds ordinary
IR v1 and binds proposed rules to pinned source identities and anchors. Its
compiler rejects approval fields, nonzero credit, accepted reviewed-rule
references and licensed bindings. Preparation delegates to the existing
compiler, runtime registry and runner, then exposes diagnostics only; draft
events and ledger projections are not official evidence.

`candidates/ims-db.json` proposes 22 rules and 42 bindings for catalog rows
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0004`, `:0005`, `:0006` and
`:0015`. The independent `fixtures/ims-db.json` contains 40 Memory/SQLite
fixtures. The tooling driver invokes the existing public host provider and
observes bounded real outputs, position, parentage, hold and database rows.
Its finite recipes cover GU/GN/GNP, Get-Hold, REPL and DLET. A roots-only seed for GN
avoids unresolved cross-type GA/GK status proposals. Injected SAF denial,
canonical retry, fresh-connection reopen and local rollback are host-contract
preparation, not full security-profile, process-death, XRST or ROLL evidence.

Run `cargo xtask conformance --subsystem ims --prepare-candidates` for
zero-credit preparation; ordinary IMS official conformance reports the human
acceptance blocker. A HUMAN conformance maintainer must review every proposed
source anchor, independently authored expected observation, applicability and
missing class, including the separately pinned DA/DJ explanatory rules, and accept or
reject the proposals through the reviewed-rule authority. No approval ID is
provided here. Promotion requires separately accepted reviewed-rule artifacts,
complete applicable obligations and genuine shared-runner official evidence.
Successful preparation grants no partial row or gate credit. Official IMS and
licensed differential remain 0/25; parent and release acceptance remain open.

The zero-credit `ims-status-explanations` scope binds the exact archived DA/DJ
bodies without changing any old baseline. Their expected hashes and byte counts
were verified and read offline. Availability resolves the source-location gap,
not human rule acceptance; the candidate's source validator rejects mismatched
topic/hash/baseline identities. No publication body is retained in Git.
