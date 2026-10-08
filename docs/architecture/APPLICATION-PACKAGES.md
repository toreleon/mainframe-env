# Application package generation contract

Status: **Frozen for mainframe-env coverage.foundation**
Owner: **application-package maintainers**
Scope: **application package generation and installation contract**
Applies from: **mainframe-env current subsystem contracts**

Current writers emit `mainframe-env.application-package@3`, using the framed
identity defined in [ADR 0051](../decisions/0051-package-identity-framing.md).
The typed envelope is additive to the accepted profile.carddemo reader. It binds
the version 1 content-addressed manifest, a positive monotonic
generation, the typed subsystem section contract, and a signature verified by
an injected trust authority. Signature bytes and key IDs are carried by the
package; trust keys and secrets are not. The Rust DTO remains
`ApplicationPackageV2`; its section tag selects the finite @2 or @3 identity
domain. The schema describes the actual `base`, `generation`, `sections`, and
`signature` fields, including omitted or explicit-null optional IMS sections.

Fresh admission requires @3. Trusted retained @2 packages keep their original
identities and signatures; recovery verifies them under current trust. A legacy
retry must equal the complete retained package. Neither recovery nor retry
relabels a signature. Standalone @1 remains available. Back up retained state
before upgrading: an older binary cannot read current @3 generations.

Identity writers enforce default `PackageLimits` before materializing hash
inputs. Embedders with a larger approved budget use
`package_generation_identity_with_limits` and configure the installer with the
same limits. Native schema validation checks structure; it does not authenticate
signatures, establish cross-reference closure, or measure process heap usage.

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
keeps its frozen @2 identity when retained; current @3 frames optional-field
presence explicitly. It publishes an explicit no-metadata
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

Fresh production packages use the bounded canonical `cose-mac0-hmac256@1`
profile ([ADR 0055](../decisions/0055-standard-package-mac-envelope.md)):
tagged COSE_Mac0, protected HMAC-256 algorithm and matching UTF-8 key ID,
empty unprotected headers, the attached computed package identity, a 32-byte
MAC and fixed application-authentication external AAD. Key IDs are 1–64 bytes
and contain no control characters. Encoded signatures are capped at 256 bytes
and decoded envelopes at 192 bytes before parsing. Noncanonical, duplicate,
unknown or critical headers, trailing data and mismatched identities refuse
before secret resolution. These bounds limit parser work, not total process heap.

The existing `hmac-sha256@1` profile remains an explicit retained verification
branch, with a 43-byte encoded cap, exactly 32 decoded MAC bytes and retained
key IDs up to 128 bytes. Configured references retain that 128-byte limit; both
profiles require resolved secrets of 32–4096 bytes. Production fresh admission
accepts only the COSE profile. Retained raw @2 or @3 packages recover under
current trust without rewriting their signature. If either side of an
existing-generation retry uses the legacy policy, the complete package must
equal the retained package; retry cannot upgrade or downgrade its envelope.
Custom injected verifiers retain their explicit policy and are not evidence
that production trust accepted a package. Symmetric MAC authentication does
not provide nonrepudiation or authenticate an arbitrary storage snapshot.

The migration from `mainframe-env.application-package@1` to version 2 is
non-destructive. The old reader remains, and rollback remains possible by
selecting the prior ready generation or by continuing to read an accepted v1
package. Version 2 now persists retained installer and explicit publication
recovery state; migration remains additive and does not select partial provider
state.
