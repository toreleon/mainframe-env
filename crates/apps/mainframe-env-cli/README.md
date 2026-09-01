# mainframe-env-cli

Ownership: local compile, run, and inspect commands over the same 0.1 services.
Non-goals: an alternate compiler or execution path. It may use filesystem and
Clap application types but converts to owned source/compiler/execution
contracts before invoking product behavior.

Verify with `cargo test -p mainframe-env-cli` and `mainframe-env --help`.

`compile`, `inspect`, and `run` accept repeatable `--library DIRECTORY` options.
Directories are enumerated deterministically into ordered logical libraries;
physical paths and timestamps do not enter source identity. Supplying libraries
also appends the explicitly ordered CICS, Db2, and MQ provider-owned ABI source
libraries. The compiler itself supplies no compatibility fallback.
