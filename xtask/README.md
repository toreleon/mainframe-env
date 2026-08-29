# xtask

Ownership: mainframe-env maintainers.

This internal binary owns deterministic version, architecture, profile,
inventory, schema, evidence, and content-digest checks. It is not shipped in
the `core-server` product profile and never imports production implementation
code directly. The CardDemo-only corpus command delegates the external fixture
contract to the conformance package and is the only xtask gate that requires
`CARDEMO_CORPUS_DIR`.

Non-goals include generating semantic pass results, running hidden fallback
routes, and replacing selector-specific conformance tests.

Verify with `cargo test -p xtask`, `cargo xtask conformance`, and (when the
pinned checkout is available) `cargo xtask carddemo-corpus --check`.
`cargo xtask carddemo-source --check` additionally replays all pinned COBOL
source closures through the CD-002 preprocessing surface.
