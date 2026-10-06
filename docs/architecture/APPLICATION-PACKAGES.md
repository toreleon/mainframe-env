# Application package generation contract

Status: **Frozen for mainframe-env coverage.foundation**
Owner: **application-package maintainers**
Scope: **application package generation and installation contract**
Applies from: **mainframe-env current subsystem contracts**

Version 2 of the application-package envelope is additive to the accepted profile.carddemo
reader. It binds the version 1 content-addressed manifest, a positive monotonic
generation, the typed subsystem section contract, and a signature verified by
an injected trust authority. Signature bytes and key IDs are carried by the
package; trust keys and secrets are not.

The typed sections are host ABI libraries, SQL tables and seed rows, IMS
definitions and seed rows, optional versioned IMS DBD/PSB metadata, MQ
resources, batch controllers, and security resources. ABI members reference
package blobs that must also be fully
validated manifest entries; no subsystem reference can reach bytes outside the
signed content closure. SQL rows reference declared
tables and columns. IMS rows reference declared definitions and segments. Batch
controllers reference program entries. MQ targets and controllers reference
resources in the same generation. Every reference and bound is validated
before staging. Allocation-safe preflight text checks run before case
normalization, hashing, parsing, cloning, or owned-key construction. Every
top-level section count is checked in that first allocation-free pass. Bounds
cover aggregate manifest/blob
bytes, total nested entries, per-library ABI members, SQL columns/keys/rows,
IMS segments/rows, controller property maps, conservative structural memory,
and per-application and global retained byte and item totals; the installer
rejects hostile input before hashing or cloning it.

The optional `ims_tm` section binds each transaction's PSB, program selector,
artifact digest, execution context, timeout, conversation size, and alternate
PCB routes to the signed generation. The PSB and alternate PCBs must match the
validated IMS metadata; the selector and artifact must match one signed program
entry. An absent section preserves older package wire forms and identity. TM
publication uses the existing application publication state. The TM provider
retains immutable generation definitions through `ProviderStateStore`; message,
session, and conversation rows keep their generation binding across package
rollback. The public server TM API admits new messages only from the complete
selected generation and resolves continued work through retained ready ones.
This bounded TM runtime selects one active TM application at a time; a second
active TM application conflicts at publication instead of replacing its
catalog. Packages without TM definitions do not displace another application.

Installation uses bounded, durable retained per-application generations.
Staging records a fully verified identity but does not change selection. The
retained package graph and selection are persisted under
`mainframe-env.application-installer@1`; open re-runs bounds, signature, digest,
size, and reference-closure validation before any retained handle is usable.
Commit marks that exact identity ready and selects it in one critical section.
A retry of the same identity is idempotent. Corrupt, conflicting, partial,
stale, or unverified generations cannot become selected. Rollback selects a
retained ready generation and does not reconstruct it from mutable source
paths.

`ProductServer` publishes controller, Db2, and IMS metadata sections through one serialized
`mainframe-env.application-publication@1` state machine. The prepared record is
bound to the expected package generation and identity. Each applicable section
is durably `pending`, `applying`, `applied`, or `failed`; restart retries
idempotent provider operations from explicit partial state. Selection becomes
complete only after all applicable sections are durable. This is recovery over
separate provider transactions, not a cross-provider exactly-once claim.

IMS metadata publication uses the shared `mainframe-env.ims-metadata@1` DTO and
validator. The provider atomically retains the package-bound generation and
advances its selected-generation row through `ProviderStateStore`; rollback
selects the exact retained generation. A package without the optional field
keeps the historical package identity and publishes an explicit no-metadata
selection, so a later generation cannot accidentally inherit stale IMS state.

`ProductServer` owns the v2 installer. Its production constructor receives a
verification-only keyed HMAC-SHA256 authority assembled from
`MAINFRAME_ENV_PACKAGE_HMAC_KEY_REFS`, a JSON map of key IDs to `SecretRef`
values. Verification resolves each reference within the secret-provider
boundary into a non-cloneable, non-debuggable, zeroizing transient wrapper.
Missing, revoked, or malformed material denies the package; plaintext keys and
signing operations are absent from the production verifier. Subsystem
publishers obtain an opaque selected-generation handle from the installer,
never a caller-provided digest.

The migration from `mainframe-env.application-package@1` to version 2 is
non-destructive. The old reader remains, and rollback remains possible by
selecting the prior ready generation or by continuing to read an accepted v1
package. Version 2 now persists retained installer and explicit publication
recovery state; migration remains additive and does not select partial provider
state.
