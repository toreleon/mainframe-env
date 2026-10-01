# xtask

Ownership: mainframe-env maintainers.

This internal binary owns deterministic version, architecture, profile,
inventory, schema, evidence, and content-digest checks. It is not shipped in
the `core-server` product profile and never imports production implementation
code directly. The CardDemo-only corpus command delegates the external fixture
contract to the conformance package and is the only xtask gate that requires
`CARDDEMO_CORPUS_DIR`.

Non-goals include generating semantic pass results, running hidden fallback
routes, and replacing selector-specific conformance tests.

`cargo xtask docs` regenerates the documentation portal navigation and manifest;
`cargo xtask docs --check` validates them without writing files, together with
relative links and anchors, command examples, normative metadata, package
topology, and public version truth.

`cargo xtask changelog --check` validates isolated TOML fragments under
`changes/unreleased/`. `cargo xtask changelog` is reserved for release or batch
integration: it consumes the fragments into the Unreleased changelog and then
regenerates the documentation manifest.

`cargo xtask profile-intake --manifest conformance/profiles/genapp-base/corpus.json
--corpus /path/to/pinned/cics-genapp --json /path/to/report.json
--markdown /path/to/report.md` verifies a clean external checkout and emits a
deterministic gap report. The command exits successfully when a valid report is
produced, even when members have recognition gaps. It rejects a changed pin or
invalid manifest. A matched catalog row records recognition only, with product
support and coverage-ledger status reported separately.

Verify with `cargo test -p xtask`, `cargo xtask conformance`, and (when the
pinned checkout is available) `cargo xtask carddemo-corpus --check`.
`cargo xtask carddemo-source --check` additionally replays all pinned COBOL
source closures through the CD-002 preprocessing surface.
`cargo xtask carddemo-closure --check` verifies the CD-003 ordered-library and
owned compatibility closure across every pinned program.
`cargo xtask carddemo-layout --check` derives the CD-004 qualified layout and
exact storage digest across the same closures.
