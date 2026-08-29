# mainframe-env-diagnostics

Ownership: stable diagnostic and problem contracts. Non-goals: CLI rendering,
HTTP mapping, logging exporters, or localization policy. It depends only on
`mainframe-env-source`. Public surface: validated codes, bounded diagnostics,
source spans, completeness, redaction, and execution problem categories.

Invariants: messages and related spans are bounded; redacted fields never
render their secret value. Verify with `cargo test -p mainframe-env-diagnostics`.
