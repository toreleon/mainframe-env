# Public source distribution

The public framework is built from the repository. Read
[capabilities](CAPABILITIES.md) and [subsystem status](../delivery/IMPLEMENTATION-STATUS.md)
before choosing a workload. There is no maintained release archive or historical
receipt collection. Cargo package versions and contract schema identities remain
build and compatibility metadata.

## Review the public source

- Execute the [quick start](GETTING-STARTED.md) from the current checkout.
- Check capabilities and pending licensed work against each owning subsystem.
- Verify documentation links, generated indexes, and Mermaid rendering.
- Review LICENSE, NOTICE, dependency notices, CONTRIBUTING, and SECURITY.
- Keep credentials, customer data, licensed publication bodies, and oracle
  observations outside the distributed tree.

## Validate the checkout

```bash
cargo xtask docs
cargo xtask docs --check
cargo xtask subsystems --check
cargo xtask changelog --check
cargo xtask license-notices --check
cargo deny check
git diff --check
```

These checks validate documentation, tracking, and dependency policy. Run the
additional subsystem and runtime checks required by the diff, following the
[verification strategy](../delivery/VERIFICATION-STRATEGY.md).

```mermaid
flowchart LR
    checkout["Source checkout"] --> scope["Subsystem scope and contracts"]
    scope --> checks["Current executable checks"]
    checks --> review["Documentation, notices and limitations"]
    review --> source["Reviewable public source"]
```

Record unavailable environments and pending licensed checks in the owning status
record. Local success does not establish production readiness or IBM equivalence.

## Migration from historical layouts

Conformance inputs now live under `conformance/subsystems/` and workload profiles
under `conformance/profiles/`. Use `cargo xtask subsystems --check` to validate the
named phases and dependencies. Historical release commands and receipt archives
have been removed; run the applicable verifier against the current checkout.

Moved paths and renamed metadata change input and projection hashes. Canonical
identities that embed those hashes also change, even when the underlying IBM
source facts and runtime framing stay the same. Keep schema-version checks and
reject stale replay bindings; do not relabel historical execution receipts as
results from the new layout. Synthetic fixtures remain zero-credit test inputs.
