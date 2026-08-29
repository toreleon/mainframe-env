# xtask

Ownership: mainframe-env maintainers.

This internal binary owns deterministic version, architecture, profile,
inventory, schema, evidence, and content-digest checks. It is not shipped in
the `core-server` product profile and never imports production implementation
code.

Non-goals include generating semantic pass results, running hidden fallback
routes, and replacing selector-specific conformance tests.

Verify with `cargo test -p xtask` and `cargo xtask conformance`.
