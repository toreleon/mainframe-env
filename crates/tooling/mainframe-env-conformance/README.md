# mainframe-env-conformance

Ownership: independent 0.1 fixtures, selected-route observations, and
out-of-process oracle comparison. Non-goals: production authority or reusable
product state. It may depend on public product crates; production crates may
not depend on it.

Invariants: fixtures are immutable and bounded, compatibility mismatches stay
exact, and oracle execution is out of process. Verify with
`cargo test -p mainframe-env-conformance`.
