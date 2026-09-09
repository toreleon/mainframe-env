# CICS command descriptor and semantic-family routing

Status: **Implemented**
Owner: **CICS provider maintainers**
Scope: **typed CICS operation descriptors, generated routing families, and provider semantic modules**
Applies from: **mainframe-env 0.9.0 development**

## Authorities

The readable command authority is
[`conformance/0.9/cics/command-descriptors.json`](../../conformance/0.9/cics/command-descriptors.json),
validated by
[`cics-command-descriptors.schema.json`](../../conformance/0.9/schemas/cics-command-descriptors.schema.json).
Every descriptor binds one typed `CicsOperation` to one row in the pinned
official CICS catalog, records whether the existing host contract treats it as
mutating, and assigns exactly one stable semantic family.

`python3 -B tools/generate_cics_descriptors.py` deterministically writes the
Rust descriptor module. Use `--check` to compare bytes without writing. The
freshness check and the module-boundary guard both run in
`cargo xtask architecture-fast --check`; hand-editing generated Rust or
changing the catalog without regeneration fails the gate.

## Frozen families

| Family | Owns |
|---|---|
| `task-control` | task context, HANDLE state, ASSIGN, RETRIEVE, ABEND, and pseudo-conversation RETURN |
| `time` | ASKTIME clock acquisition and FORMATTIME conversion |
| `program-control` | program inquiry, LINK, and XCTL |
| `terminal-control` | BMS and text send/receive behavior |
| `file-control` | file status, keyed I/O, and browse behavior |
| `queue-control` | transient-data queue writes |
| `recovery` | SYNCPOINT coordination, rollback, and subsystem unit-of-work completion |

`CicsService::invoke_run` selects the generated descriptor first and routes on
its family. Each family has a real reviewed implementation module under
`crates/providers/mainframe-env-cics/src/handlers/`; no command is dispatched by
an ad hoc keyword match in the service monolith. The service retains the shared
session/run state, authorization boundary, durable compare-and-swap primitives,
common condition/response machinery, and bounded codecs used across families.
The accepted `retention.rs` sibling owns provider-lifecycle codecs and
dependency descriptions; it is deliberately outside the command-family layer
and cannot become an alternate dispatch path.

The compiler/runtime boundary is governed by
[ADR-0011](../decisions/0011-typed-language-hir-and-semantic-ir.md). A migrated
COBOL CICS family resolves static command identity, options, resource bindings,
and output destinations before execution, then emits the existing owned typed
request through the execution coordinator. The provider never parses COBOL HIR
or source syntax, and the migration cannot introduce a parallel CICS provider,
store, unit-of-work protocol, or condition authority.

## Change contract

Adding or changing a typed CICS command requires one reviewable change that:

1. updates the readable descriptor catalog and its official row binding;
2. regenerates the Rust descriptor module;
3. adds semantics to the named family module, keeping it below the hard
   1,200-production-line limit; and
4. adds focused condition, authorization, durability, and recovery tests
   appropriate to the operation.

A new semantic family changes the accepted module inventory and requires an ADR
amendment. Generated descriptors never contain behavior, and handler modules
are never generator-owned. [ADR-0010](../decisions/0010-rust-module-review-budgets.md)
records the hard limits and exact legacy ceiling policy.

## Verification

```bash
python3 -B tools/generate_cics_descriptors.py --check
python3 -B -m unittest tools.tests.test_cics_descriptors tools.tests.test_module_boundaries
cargo test -p mainframe-env-cics --all-features --locked
cargo xtask architecture-fast --check
```
