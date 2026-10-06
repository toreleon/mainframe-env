# mainframe-env-cli

Ownership: local compile, run, and inspect commands over the same 0.1 services.
Non-goals: an alternate compiler or execution path. It may use filesystem and
Clap application types but converts to owned source/compiler/execution
contracts before invoking product behavior.

Verify with `cargo test -p mainframe-env-cli` and `mainframe-env --help`.

## Local commands

Run from the repository root:

```bash
cargo run --locked -p mainframe-env-cli --bin mainframe-env -- \
  inspect conformance/subsystems/platform/fixtures/cobol/HELLO.cbl --format fixed
cargo run --locked -p mainframe-env-cli --bin mainframe-env -- \
  compile conformance/subsystems/platform/fixtures/cobol/HELLO.cbl --format fixed
cargo run --locked -p mainframe-env-cli --bin mainframe-env -- \
  run conformance/subsystems/platform/fixtures/cobol/HELLO.cbl --format fixed
```

`inspect` reports analysis and diagnostics; it does not certify executability.
`compile` prints artifact identities and sizes without writing a file. `run`
compiles and executes through the local coordinator and reference machine.
Source is read as UTF-8; the default format is free. This local composition does
not register the complete server host-service portfolio.

## Source libraries

`compile`, `inspect`, and `run` accept repeatable `--library DIRECTORY` options.
Directories are enumerated deterministically into ordered logical libraries;
physical paths and timestamps do not enter source identity. Supplying libraries
also appends the explicitly ordered CICS, Db2, and MQ provider-owned ABI source
libraries. The compiler itself supplies no compatibility fallback.

## Documentation

[Documentation portal](../../../docs/README.md) · [Current package map](../../../docs/architecture/PACKAGE-MAP.md) · [Embedding guide](../../../docs/guides/EMBEDDING.md) · [Contribution and verification](../../../CONTRIBUTING.md)
