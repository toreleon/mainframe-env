# Application package generation contract

Status: **Frozen for mainframe-env 0.2.0**

Version 2 of the application-package envelope is additive to the accepted 0.1.1
reader. It binds the version 1 content-addressed manifest, a positive monotonic
generation, the typed subsystem section contract, and a signature verified by
an injected trust authority. Signature bytes and key IDs are carried by the
package; trust keys and secrets are not.

The typed sections are host ABI libraries, SQL tables and seed rows, IMS
definitions and seed rows, MQ resources, batch controllers, and security
resources. ABI members reference package blobs. SQL rows reference declared
tables and columns. IMS rows reference declared definitions and segments. Batch
controllers reference program entries. MQ targets and controllers reference
resources in the same generation. Every reference and bound is validated
before staging. Preflight bounds cover aggregate manifest/blob bytes, total
nested entries, per-library ABI members, SQL columns/keys/rows, IMS
segments/rows, controller property maps, and cumulative retained package bytes;
the installer rejects them before hashing or cloning hostile input.

Installation uses retained per-application generations. Staging records a
fully verified identity but does not change selection. Commit marks that exact
identity ready and selects it in one critical section. A retry of the same
identity is idempotent. Corrupt, conflicting, partial, stale, or unverified
generations cannot become selected. Rollback selects a retained ready
generation and does not reconstruct it from mutable source paths.

`ProductServer` owns the v2 installer. Its production constructor receives a
keyed HMAC-SHA256 trust authority assembled from
`MAINFRAME_ENV_PACKAGE_HMAC_KEYS` (a JSON map of key IDs to base64 keys); an
empty authority denies every package. Subsystem publishers obtain an opaque
selected-generation handle from that installer, never a caller-provided digest.

The migration from `mainframe-env.application-package@1` to version 2 is
non-destructive. The old reader remains, and rollback remains possible by
selecting the prior ready generation or by continuing to read an accepted v1
package. No package migration changes subsystem provider state by itself.
