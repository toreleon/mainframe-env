# mainframe-env-application

Ownership: content-addressed application manifests, bounded typed subsystem
sections, signature-gated generation staging, atomic ready-generation
selection, retained rollback generations, program artifacts, BMS/CSD resources,
and dataset catalogs.

The `mainframe-env.application-package@1` reader remains available for accepted
0.1.1 packages. Version 2 adds host ABI libraries, SQL schemas and rows, IMS
definitions and rows, MQ resources, batch controllers, and security resources.
Every cross-reference is validated before a generation can be staged. A
signature verifier is mandatory; the package kernel does not contain a trust
store or accept a digest as a signature.

Every blob-bearing reference is inside the validated manifest closure. Section
counts and nested bounds are checked before normalization or owned allocation.
The bounded installer state persists retained package bytes and selection, and
revalidates signatures, content digests, sizes, and references on recovery.

A staged generation is never selected. Commit changes its state to ready and
selects it under one application lock. Repeating stage/commit for the same
identity is idempotent, while same-generation conflicts, stale generations,
bad signatures, changed or non-manifest blobs, unresolved references, and
exceeded bounds fail
closed. Rollback selects only a retained ready generation.

Non-goals: provider state, application-specific behavior, native executable
loading, or a plugin marketplace. Verify with
`cargo test -p mainframe-env-application --locked` and
`cargo xtask application-packages --check`.
