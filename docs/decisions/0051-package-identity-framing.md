# ADR-0051: Frame current package identities and retain explicit legacy readers

Status: Accepted design; implementation pending
Owner: Application-package and shared trust maintainers
Scope: Package identity, fresh admission and trusted retained-state compatibility
Applies from: mainframe-env current public package hardening

## Decision

Current writers emit `mainframe-env.application-package@3` in the existing
`sections.schema_version` discriminator. The owned package data shape remains;
no second discriminator, package authority or cryptographic primitive is added.
A version-neutral `package_generation_identity` dispatches exactly on @2 or @3.
The historical @2 identity entry point remains frozen for @2 and rejects other
versions. Unknown versions refuse; verification never tries another domain.

The @3 preimage uses the existing length-prefixed scalar encoding, explicit
field/record labels, checked collection counts and option-presence bytes.
Every ordinary section, nested sequence/map and record has a distinct role.
All ten section slots are present, including empty and absent values. Preserve
existing sorted set/map order and semantic vector order; do not normalize case,
reorder columns/keys, or change manifest entry/dependency order. Frame the base
manifest directly, including every entry and dependency, rather than relying
only on the structurally unframed historical manifest digest. Blob content
closure and subsystem validation remain with their current owners.

Optional IMS metadata/TM leaves carry their role, presence, existing subsystem
contract and length-prefixed documented DTO JSON. Their internal field/enum,
vector order and omission rules remain versioned inputs. Independent vectors
freeze them; a serializer/DTO change is not an implicit rehash. Bounds run before
sorting, allocating validators, serialization or hashing.

## Legacy admission and retention

The @1 manifest reader and @2 identity/signature algorithms retain their exact
bytes. New signed generation admission accepts @3. An exact @2 retry can
acknowledge an already retained package only after full owned-package equality
and original trust verification; equal legacy hashes alone are insufficient.
Fresh @2 imports refuse. An explicit new generation signed under @3 performs
migration; old records are never rehashed, re-signed or relabeled.

Recovery accepts @2 and @3 from the existing trusted retained-state authority.
The installer-state @1 shape, selection/topology and provider references stay
unchanged; nested package domains explicitly select the finite identity reader.
Older binaries refuse @3 nested packages, so binary downgrade requires the
pre-upgrade backup and stopped admission. Runtime rollback can still select a
retained Ready @2 generation; its original identity/signature remain intact.

`from_state_payload` is a trusted recovery boundary, not an untrusted package
importer. ProductServer supplies only its owned provider-state namespace or its
own pre-mutation snapshot. This relies on the injected PlatformStore and host
caller as trusted authorities; it does not claim cryptographic authentication
of arbitrary snapshot bytes. Original package signature checks, bounds, graph
closure, current revocation policy and explicit selected/null topology still
apply. Do not expose raw recovery as a remote/untrusted ingestion route.
Content-address integrity or a caller-provided digest cannot establish trust.
A deployment needing hostile-store or external backup authentication requires
its own authenticated root, outside this existing single-owner threat model.

```mermaid
flowchart LR
    Fresh[Fresh signed package] --> Domain{Exact package domain}
    Domain -->|@3| Framed[Framed identity and trust verification]
    Domain -->|@2| Retry[Exact retained package retry only]
    Framed --> Stage[Existing generation installer]
    Retry --> Stage
    Store[Trusted retained state] --> Readers[Finite @2 and @3 readers]
    Readers --> Selected[Preserve Ready, Staged and selected or null]
    Selected --> Rollback[Explicit retained-generation rollback]
```

## Acceptance and separation

Retain the independently designed legacy ambiguity reproduction. New-domain
valid graphs have different identities; signature substitution, altered base
ownership, unknown domain, downgrade and changed legacy retry refuse without
mutation. Frozen @1/@2 vectors, optional absent/null forms, trusted mixed-domain
reopen/rollback and key-revocation controls remain. Update every current producer
and affected schema/consumer through its owner; never edit IBM publication pins
or derive expected authentication results from the product hash implementation.

Standards-envelope adoption is a separate bounded trust-adapter slice. It must
reuse reviewed cryptography and standards libraries, keep foreign types behind
the adapter and preserve original legacy signatures. Identity framing alone
neither establishes standards-envelope completion nor full Foundation acceptance.
