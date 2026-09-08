# mainframe-env

`mainframe-env` is a greenfield Rust implementation of a bounded mainframe
application environment. It combines a deterministic COBOL compiler and
execution kernel with owned CICS, JCL/JES, dataset, RACF/SAF, z/OSMF, Db2, IMS,
MQ, spool, and persistence contracts.

The project uses IBM documentation and licensed systems as conformance
authorities. It is not an IBM product, and local or modeled results are never
presented as licensed IBM equivalence.

## Project status

| Item | Status |
|---|---|
| Latest published release | [0.8.2](https://github.com/toreleon/mainframe-env/releases/tag/mainframe-env-v0.8.2), source bundle only |
| Current workspace version | `0.8.3` (development) |
| Development baseline | `0.8.3` contains unreleased pre-0.9 hardening after the published 0.8.2 tag |
| Next planned minor | [0.9.0 — complete CICS application API](docs/delivery/coverage-versions/0.9.0.md) |
| Production readiness | Not claimed |
| Licensed differential status | Required campaigns remain pending where the release notes say so |

The current pre-0.9 assessment is a **no-go for broad implementation** until
the P1 hardening items in the
[pre-0.9 deep review](docs/reviews/PRE-0.9.0-DEEP-REVIEW.md) are closed. A clean
test run is necessary but does not override those findings.

## Quick start

The repository pins Rust 1.98.0. Install the toolchain declared in
[`rust-toolchain.toml`](rust-toolchain.toml), then run:

```bash
cargo build --workspace --all-features --locked
cargo test --workspace --all-features --locked --no-fail-fast
```

Compile and run the included fixed-format COBOL fixture:

```bash
cargo run --quiet -p mainframe-env-cli --bin mainframe-env -- \
  compile conformance/0.1/fixtures/cobol/HELLO.cbl --format fixed

cargo run --quiet -p mainframe-env-cli --bin mainframe-env -- \
  run conformance/0.1/fixtures/cobol/HELLO.cbl --format fixed
```

The standalone server is still a development composition, not a turnkey
deployment. In particular, a fresh store has no supported operator bootstrap
command. See the [operations runbook](docs/runbooks/OPERATIONS.md) before
starting it.

## Architecture at a glance

```text
HTTP / CLI / z/OSMF
         |
         v
application composition and admission
         |
         v
compiler / interpreter / batch kernels
         |
         v
owned execution, host, and store contracts
         |
         v
CICS / dataset / RACF / Db2 / IMS / MQ / spool / durable stores
```

The core rule is that semantic work remains deterministic while I/O,
scheduling, persistence, and transport stay behind owned contracts. Start with
the [architecture overview](docs/architecture/OVERVIEW.md) and the
[package map](docs/architecture/PACKAGE-MAP.md).

## Verification

Common local checks are:

```bash
cargo fmt --all -- --check
"$(tools/jenkins/select-python.sh)" -B tools/supply_chain.py check
cargo deny check
cargo xtask license-notices --check
cargo xtask docs --check
cargo +1.95.0 check --workspace --all-targets --all-features --locked
cargo xtask spec --check
cargo xtask architecture-fast --check
cargo test --workspace --all-features --locked --no-fail-fast
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked
```

The full conformance and certification commands are documented in the
[verification strategy](docs/delivery/VERIFICATION-STRATEGY.md). Some gates
need a disposable PostgreSQL 18 instance, a pinned CardDemo checkout, a live
Zowe client, or a licensed IBM environment; skipped environment-dependent tests
receive no evidence credit.

## Repository map

| Path | Purpose |
|---|---|
| `crates/foundation/` | Source, diagnostics, encoding, and IR primitives |
| `crates/contracts/` | Compiler, execution, host, store, and conformance contracts |
| `crates/kernel/` | Compiler, interpreter, and application-package authorities |
| `crates/providers/` | Dataset, CICS, RACF, Db2, IMS, MQ, and spool implementations |
| `crates/apps/` | CLI, batch, and server composition |
| `crates/gateways/` | z/OSMF protocol translation |
| `crates/stores/` | Memory, SQLite, PostgreSQL, and local artifact adapters |
| `crates/tooling/`, `xtask/` | Conformance, generation, validation, and release tooling |
| `conformance/` | Versioned catalogs, schemas, fixtures, and evidence |
| `docs/` | Architecture, decisions, delivery plans, runbooks, research, and releases |

## Documentation

- [Documentation portal](docs/README.md)
- [Project charter](docs/CHARTER.md)
- [Architecture overview](docs/architecture/OVERVIEW.md)
- [Verification strategy](docs/delivery/VERIFICATION-STRATEGY.md)
- [Versioning and release policy](docs/delivery/VERSIONING-AND-RELEASES.md)
- [0.8 release notes](docs/releases/0.8.md)
- [0.9.0 readiness status](docs/delivery/coverage-versions/status/0.9.0.md)
- [Pre-0.9 deep review](docs/reviews/PRE-0.9.0-DEEP-REVIEW.md)
- [Contribution guide](CONTRIBUTING.md)
- [Security policy](SECURITY.md)

## Compatibility boundary

The superseded OpenMainframe workspace is an executable compatibility oracle,
not a source dependency:

```text
OpenMainframe reference workspace
        | differential observations
        v
mainframe-env owned contracts and implementation
```

No production deployment, remote publication, or licensed-equivalence claim is
implied by repository publication or by passing local tests.

## License

mainframe-env is licensed under the [Apache License 2.0](LICENSE). Required
project and third-party attributions are in [NOTICE](NOTICE); the exact ICU text
approved for the locked decNumber dependency is retained in
[LICENSES/ICU.txt](LICENSES/ICU.txt). Release tooling generates full,
target-specific `LICENSES.md` notices for every production dependency before it
writes release receipts.
