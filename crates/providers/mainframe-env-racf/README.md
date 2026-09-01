# mainframe-env-racf

Ownership: the RACF/SAF identity, authentication, schema-tagged profile and
segment database, authorization, ACEE/token records, group hierarchy,
certificate/key references, audit, transaction/recovery records, and
persistence authority. Non-goals: exposing credentials, database handles,
crypto-library values, or ambient global policy. Allowed production
dependencies are owned host/execution/store contracts, Argon2id, zeroization,
and the workspace serialization stack.

Invariants: missing users/profiles/grants and provider errors deny; plaintext
secrets are resolved only inside an authentication scope and cleared after
use; audits are bounded and redacted. The v2 database is one bounded CAS-updated
provider-state snapshot, so mixed security mutations can commit atomically
without adding a second store or transaction authority. Its normative Draft
2020-12 schemas are under `conformance/0.5/schemas`. Verify with
`cargo test -p mainframe-env-racf`, `cargo xtask schemas --check`, and the
focused `racf-saf` Conformance IR gate once command/SAF bindings are installed.

The generated command registry is sourced from
`conformance/0.5/racf/command-language.json` and checked with
`cargo xtask racf-catalog --check`. The bounded parser recognizes all 34 frozen
families without retaining command secrets in public DTOs. SEC-502 command
execution covers the 21 user, group, connection, dataset/general-resource,
list, display, and search families through authorized, deny, rollback,
idempotency, concurrent-CAS, and audit routes. Policy/operations and advanced
identity families remain assigned to their later named work packages rather
than receiving generic-success handlers.

SEC-503 adds generated supplied-class metadata plus validated installation-
defined classes, SETROPTS/class activation and generic controls, owned RACLIST
snapshots with explicit refresh semantics, PROGRAM control, RACPRIV, SET,
RVARY, STOP/RESTART, and bounded RACPRMCK member validation. A RACLISTed class
never falls through to live profiles when its cache is absent, and stopped or
inactive authority state fails closed.
