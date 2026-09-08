# ADR-0009: Govern the current 26-package topology

Status: **Accepted by repository owner**
Owner: **repository owner**
Scope: **current workspace package boundaries and topology changes**
Applies from: **mainframe-env 0.8.3 development**
Supersedes: **ADR-0005 for current package counts and topology**

## Context

ADR-0005 reduced the proposed 0.1 workspace to twenty boundary packages. The
accepted 0.1 inventory ultimately contained twenty-four packages, including
the server and CLI applications plus conformance and xtask tooling. Later
accepted work added `mainframe-env-coverage` in 0.2 and
`mainframe-env-spool` in 0.8. The workspace therefore contains twenty-six
packages, while ADR-0005 remains an important but no longer current topology
record.

Leaving the current topology implicit makes package counts, ownership, and
split authority easy to misstate. It also permits a new package to appear in
Cargo without a current architecture decision that explains the boundary.

## Decision

The current workspace consists of these twenty-six packages:

| Layer | Packages | Count |
|---|---|---:|
| Foundation | `mainframe-env-source`, `mainframe-env-diagnostics`, `mainframe-env-encoding`, `mainframe-env-ir` | 4 |
| Contracts | `mainframe-env-compiler-api`, `mainframe-env-execution-api`, `mainframe-env-host-api`, `mainframe-env-store-api`, `mainframe-env-coverage` | 5 |
| Kernel | `mainframe-env-compiler`, `mainframe-env-interpreter`, `mainframe-env-application` | 3 |
| Providers | `mainframe-env-dataset`, `mainframe-env-racf`, `mainframe-env-cics`, `mainframe-env-db2`, `mainframe-env-ims`, `mainframe-env-mq`, `mainframe-env-spool` | 7 |
| Stores | `mainframe-env-store` | 1 |
| Applications | `mainframe-env-batch`, `mainframe-env-server`, `mainframe-env-cli` | 3 |
| Gateways | `mainframe-env-zosmf` | 1 |
| Tooling | `mainframe-env-conformance`, `xtask` | 2 |
| **Total** |  | **26** |

The isolated, non-publishing `fuzz/` Cargo workspace is a cargo-fuzz harness,
not a product or tooling package in the main workspace. It has its own lockfile
so nightly/libFuzzer dependencies cannot enter the shipping dependency graph,
and no production package may depend on it. Its presence does not change the
twenty-six-package machine inventory.

The machine authority is the twenty-four-package base in
[`conformance/0.1/inventory/packages.json`](../../conformance/0.1/inventory/packages.json),
plus the versioned additions in
[`conformance/0.2/inventory/package-additions.json`](../../conformance/0.2/inventory/package-additions.json)
and
[`conformance/0.8/inventory/package-additions.json`](../../conformance/0.8/inventory/package-additions.json).
Cargo workspace membership and the machine authority must remain equal.

The existing consolidation rule remains in force: a crate is justified only
when it enforces dependency direction, owns a versioned public contract,
provides an independently selected provider, or forms an application/tooling
boundary. Readability and command-family separation use modules, not new
packages.

Any package addition, removal, layer move, or new normal dependency that changes
these boundaries requires all of the following in one reviewable change:

1. a superseding ADR or an explicit amendment authorized by this ADR;
2. an updated versioned machine inventory;
3. updated package-map and generated documentation navigation;
4. architecture/profile closure checks; and
5. a compatibility and migration statement when a public or durable boundary
   changes.

## Consequences

- ADR-0005 remains the immutable record of the original consolidation and its
  split criteria, but it is not a current package-count authority.
- The [package map](../architecture/PACKAGE-MAP.md) describes this topology and
  links back to this decision.
- `cargo xtask architecture --check` remains the machine package/dependency
  gate; `cargo xtask docs --check` prevents the public topology decision,
  navigation, and version truth from drifting.
- Adding the planned CICS surface does not itself justify new crates. Stable
  command-family modules must be exhausted before proposing another package.
