# xtask

Ownership: mainframe-env maintainers.

This internal binary owns architecture, subsystem, workload-profile, inventory,
schema, source-input, and content-digest checks. Production applications do not
ship xtask. CardDemo commands delegate execution and corpus validation to the
conformance package and require `CARDDEMO_CORPUS_DIR`; the complete CardDemo gate
also requires a disposable PostgreSQL database.

## Documentation and subsystem management

`cargo xtask docs` regenerates portal navigation, subsystem plan/prompt/dependency
indexes, progress, and the manifest. `cargo xtask docs --check` validates links,
anchors, command examples, normative metadata, package topology, and generated
content. `cargo xtask subsystems --check` verifies the same registered ownership
and dependency model without writing files.

`cargo xtask changelog --check` validates isolated TOML fragments under
`changes/unreleased/`. Batch integration runs `cargo xtask changelog` to consume
those fragments into the Unreleased changelog and regenerate documentation.
Historical release/version commands and receipt replay/sealing commands are
retired. Keep current run output outside Git.

## Workload checks

```bash
cargo xtask carddemo-corpus --check
cargo xtask carddemo-source --check
cargo xtask carddemo-closure --check
cargo xtask carddemo-readacct --check
cargo xtask carddemo-host-integration --check
cargo xtask carddemo-full --check
```

The corpus gate validates the clean pinned real upstream repository. Source,
closure, layout, and execution gates have separate scopes; only a complete
passing run establishes the declared workload profile. See the
[CardDemo operator runbook](../docs/runbooks/CARDDEMO-OPERATOR.md).

`cargo xtask profile-intake --manifest conformance/profiles/genapp-base/corpus.json
--corpus /path/to/pinned/cics-genapp --json /path/to/report.json
--markdown /path/to/report.md` verifies a clean external checkout and emits a
bounded gap report. A valid report can contain recognition gaps. Recognition,
product support, and coverage remain separate results.

Verify tooling with `cargo test -p xtask` and the focused checks required by your
change. Use `cargo run --locked -p xtask -- --help` for the current command inventory.

[Documentation portal](../docs/README.md) · [Package map](../docs/architecture/PACKAGE-MAP.md) · [Contribution and verification](../CONTRIBUTING.md)
