# mainframe-env-racf

Ownership: the RACF/SAF identity, authentication, schema-tagged profile and
segment database, authorization, ACEE/token records, group hierarchy,
certificate/key references, audit, transaction/recovery records, and
persistence authority. Non-goals: exposing credentials, database handles,
crypto-library values, or ambient global policy. Allowed production
dependencies are owned host/execution/store contracts, Argon2id, zeroization,
Ring HMAC comparison, and the workspace serialization stack.

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

SEC-504 adds the generated 14-request RACROUTE registry and typed AUDIT, AUTH,
DEFINE, DIRAUTH, EXTRACT, FASTAUTH, LIST, SIGNON, STAT, TOKENBLD, TOKENMAP,
TOKENXTR, VERIFY, and VERIFYX state machines. The host authorization adapter and
SAF routes share one evaluator for profile specificity, group hierarchy,
conditional access, labels/levels/categories, RACLIST generations, PROGRAM
control, and return/reason mapping. ACEEs, nesting/delegation, and token
metadata remain bounded owned records; raw credentials and token material do
not enter results or audit.

SEC-505 completes PASSWORD/PHRASE, RACDCERT, RACLINK, RACMAP, SIGNOFF, and
TARGET through that same transaction authority. Certificates, keys, MFA
factors, and tokens persist only safe references and digests; public results
and audits expose no referenced material. Distributed identity mappings, RRSF
nodes/associations, durable sign-on sessions, MFA proof comparison, password
history, keyrings, and certificate lifecycle operations are bounded by the v2
database schema. The shared Conformance IR now executes all 34 command families
and all 14 RACROUTE request types.

SEC-506 local gates add one centralized fail-closed audit redactor and a bounded
SMF type-80 projection, automatic migration of retained v1 provider records,
rollback that leaves the v1 source intact for an older reader, startup
reconciliation of intent/unknown-outcome transactions, and restart/replay
coverage for every command and RACROUTE row. Corrupt migration input and
corrupt v2 snapshots never publish partial authority state. Licensed IBM
differential remains unclaimed until a pinned z/OS 3.2 oracle actually runs and
supplies receipts through the shared Conformance IR oracle registry.
Each official row also has explicit bounded-limit, audit/redaction, and
concurrent CAS-retry obligations; these results are not inferred from the broad
workspace regression.

The 0.5 conformance tooling contains a separate pure-state reference simulation
for development assurance. It does not import this production crate, does not
define provider contracts or authority state, and cannot supply licensed
differential credit. Under the user-approved 2026-09-01 scoped policy, 0.5
retains `differential=0/48 pending`; the real licensed campaign is deferred to
the 0.17 `release-certify` hard gate.
