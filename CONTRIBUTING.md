# Contributing to mainframe-env

Status: **Development contribution guide**

Contributions should preserve the project's deterministic-core, owned-contract,
bounded-resource, and fail-closed conformance rules. Read the
[project charter](docs/CHARTER.md), [architecture overview](docs/architecture/OVERVIEW.md),
and [verification strategy](docs/delivery/VERIFICATION-STRATEGY.md) before
changing a public or durable boundary.

## Development setup

The repository pins Rust in `rust-toolchain.toml` and commits `Cargo.lock`.

```bash
cargo build --workspace --all-features --locked
cargo test --workspace --all-features --locked --no-fail-fast
```

Some conformance suites additionally require PostgreSQL 18, a pinned CardDemo
checkout, a live Zowe client, cached IBM publication inputs, or a licensed IBM
environment. A skipped external test receives no evidence credit.

## Change workflow

1. Start from a clean branch and preserve unrelated user changes.
2. Identify the owning contract, provider, schema, and recovery boundary before
   editing.
3. Add a focused negative regression before or with a defect fix.
4. Regenerate derived files through `cargo xtask`; do not hand-edit generated
   Rust, catalogs, ledgers, or receipts.
5. Run the narrow affected tests, then the applicable gates below.
6. Update user-visible documentation and `CHANGELOG.md` in the same change.
7. Keep implementation, release promotion, publication, and deployment as
   separate decisions.

## Local quality floor

```bash
cargo fmt --all -- --check
cargo xtask spec --check
cargo xtask architecture-fast --check
cargo test --workspace --all-features --locked --no-fail-fast
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked
git diff --check
```

Run `cargo deny check` for dependency changes. It is currently a known
pre-0.9 blocker until the ICU license decision and complete notice packaging
are resolved; do not waive the failure silently.

## Public and durable changes

A public API, schema, effect encoding, artifact, checkpoint, session, store,
route, or migration change also requires:

- backward-compatibility and version review;
- malformed, bound, authorization, cancellation, and failure tests;
- restart, replay, rollback, and unknown-outcome tests when state can persist;
- parity across memory, SQLite, and PostgreSQL where the contract applies; and
- an ADR when ownership, dependency direction, or a stable boundary changes.

## Conformance evidence

Catalog presence is not semantic coverage. Every claimed result must retain its
row, obligation, gate, driver, candidate, fixture/environment, and oracle
identity through the shared Conformance IR. Local, modeled, GnuCOBOL, Hercules,
and historical results cannot grant licensed IBM differential credit.

Never commit proprietary IBM publication bodies, credentials, customer data,
raw secrets, local absolute paths, or a licensed oracle capture that the
repository policy requires to remain external.

## Reviewability

Keep modules and pull requests centered on one reason to change. The accepted
architecture uses roughly 800–1,200 lines as a review trigger. Large release
programs should use bounded work-package reviews while incomplete behavior
remains unreachable from the public profile; the final integrated candidate
must still pass the complete exit gate.

Security-sensitive findings should follow [SECURITY.md](SECURITY.md), not a
public issue.
