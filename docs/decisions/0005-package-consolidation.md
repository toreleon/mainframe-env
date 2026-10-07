# ADR 0005: Initial package boundary consolidation

Status: **Superseded by ADR-0009 for current topology**
Owner: **repository owner**
Scope: **historical platform.runtime-integration package consolidation and split criteria**
Applies from: **mainframe-env current subsystem contracts**

Historical scope note: this ADR records the original consolidation target. The
accepted platform.runtime-integration machine inventory later contained 24 workspace packages, and
versioned additions in coverage.foundation and jes.execution bring the current workspace to 26. See the
[package map](../architecture/PACKAGE-MAP.md) and machine inventories for
current composition. [ADR-0009](0009-current-package-topology.md) supersedes
this decision for the current package count and topology.

## Context

The initial package map proposed 28 crates so that every possible compiler,
runtime, provider, and adapter responsibility was visible. The package-boundary
rule is stricter: a crate is justified only when it enforces dependency
direction, a versioned public contract, independent provider selection, or an
application boundary. A crate created only for source-file organization adds
publication and dependency overhead without strengthening the architecture.

## Decision

The platform.runtime-integration workspace has 20 packages. The following proposed crates are physical
modules in an owning package:

- `mainframe-env-ir-codec` is `mainframe-env-ir::codec`;
- `mainframe-env-cics-api` is the typed CICS family in
  `mainframe-env-host-api`;
- `mainframe-env-execution` coordination is
  `mainframe-env-interpreter::coordinator` over execution, host, and store
  contracts;
- `mainframe-env-cobol-syntax` and `mainframe-env-cobol` are private syntax,
  semantic, HIR, and lowering modules in `mainframe-env-compiler`;
- `mainframe-env-jcl` and `mainframe-env-jes` share
  `mainframe-env-batch`, because the durable job/step/spool state transition is
  one authority; and
- memory, SQL, and artifact adapters share `mainframe-env-store` while
  implementing separate owned store interfaces.

The machine inventory in `conformance/subsystems/platform/inventory/packages.json` is the exact
package authority. Logical modules remain separate and may not bypass owned
contracts merely because they share a crate.

## Boundary tests

- Compiler stage constructors remain private to `mainframe-env-compiler-api`.
- Syntax, semantic, HIR, and lowering modules are not independent public
  provider surfaces.
- The reference machine and coordinator depend on store interfaces, never a
  concrete store.
- JCL/JES dispatches through the program and host contracts.
- SQL, memory, and artifact implementations remain replaceable through owned
  store interfaces.
- `cargo xtask architecture --check` compares the actual Cargo graph with the
  declared dependency graph and this package inventory.

## Split triggers

A consolidated unit becomes a separate crate only if it gains an independent
consumer, provider-selection boundary, contract version, or prohibited edge
that a module boundary cannot enforce. That change requires a superseding ADR.

## Consequences

The workspace has fewer public surfaces while retaining the original logical
architecture. In particular, COBOL remains separated by module responsibility,
but platform.runtime-integration does not claim independently versioned syntax or semantic packages.
