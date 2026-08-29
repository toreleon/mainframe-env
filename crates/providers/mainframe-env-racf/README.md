# mainframe-env-racf

Ownership: the 0.1 RACF/SAF identity, authentication, profile authorization,
group, audit, and persistence authority. Non-goals: exposing credentials,
database handles, or ambient global policy. Allowed dependencies are owned
host/execution/store contracts and Argon2id.

Invariants: missing users/profiles/grants and provider errors deny; plaintext
secrets are resolved only inside an authentication scope and cleared after
use; audits are bounded and redacted. Verify with
`cargo test -p mainframe-env-racf`.
