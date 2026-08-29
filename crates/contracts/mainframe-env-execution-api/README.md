# mainframe-env-execution-api

Ownership: validated identities, principals/grants, invocation context,
resource limits, machine drives, resume inputs, outcomes, and lifecycle events.
Non-goals: async scheduling, providers, stores, or gateway DTOs. It depends only
on stable diagnostics.

Invariants: normal control is data; every identity/payload/output/quantum is
bounded; cancellation and failure categories remain distinct. Verify with
`cargo test -p mainframe-env-execution-api`.
