# ADR-0004: v0.1 Versioning and Release Policy

Status: **Accepted by repository owner**
Decision scope: **mainframe-env product, crates, contracts, schemas, artifacts,
stores, releases, and phase commits**

## Context

mainframe-env 0.1 is the first product surface, but its initial release line is
`0.1`. The implementation program spans multiple phases and needs reproducible
phase boundaries, compatibility rules, release candidates, and final artifacts
without making commit hashes self-referential inside checked-in evidence.

## Decision

### Product version

The first public release line is **mainframe-env 0.1**. Its first final semantic
version is:

```text
0.1.0
```

Development and promotion use SemVer prereleases:

```text
0.1.0-alpha.N
0.1.0-beta.N
0.1.0-rc.N
0.1.0
```

The initial workspace starts at `0.1.0-alpha.0`. A release version changes only
through an explicit release-preparation commit.

### Version sources

The repository contains:

- `VERSION` — exact current product version;
- root `[workspace.package].version` — the same version for shipped Rust crates;
- `release.toml` — release line, channel, supported profiles, minimum Rust,
  contract compatibility ranges, and artifact policy;
- `CHANGELOG.md` — Keep-a-Changelog-style user-visible changes;
- `docs/releases/0.1.md` — scope, compatibility, known limitations, upgrade and
  rollback notes; and
- machine-readable release contracts/evidence under `conformance/0.1/`.

`cargo xtask versions --check` verifies equality and compatibility among these
sources. Generated files do not silently become a second version authority.

### Rust crate versions

All crates shipped together in `core-server` use the product version in
lockstep for the 0.1 line. Internal crate API stability is not inferred from the
crate version alone; support classification is explicit.

Test-only `xtask` and conformance packages may use the workspace version but are
not published product artifacts.

### Contract versions

Product, Rust crate, and contract versions are independent. At minimum version:

- source bundle;
- diagnostic/problem schema;
- compiler API;
- IR object, text, binary, and envelope;
- each executable dialect/interface;
- artifact envelope;
- execution API/outcomes/events;
- host-service interfaces;
- CICS operation/provider contract;
- checkpoint/session state;
- SQL schema/migration set;
- product configuration;
- z/OSMF route compatibility family; and
- conformance evidence.

Contract identifiers use stable explicit identities such as
`mainframe-env.execution@1` or a documented equivalent. Product `0.1.1` does
not imply execution contract `1.1`, and vice versa.

### Compatibility within 0.1.x

Once `0.1.0` is final, patch releases `0.1.x` are backward compatible for every
surface classified supported in 0.1:

- no removal or semantic redefinition of supported selectors;
- no reuse of removed field numbers, enum values, operation IDs, diagnostic
  codes, or SQL migration identities;
- readers continue to accept the documented 0.1 artifact/checkpoint/config
  range;
- durable schema changes use expand/migrate/contract sequencing;
- new optional fields or operations must be safely ignored or explicitly
  negotiated by older supported readers as their contract specifies; and
- security fixes may deny behavior previously accepted only when the security
  impact and compatibility exception are documented.

A breaking supported-surface change requires `0.2.0` or a separately versioned
major contract with an explicit compatibility/migration plan. The fact that
SemVer permits rapid change before 1.0 is not permission to break the accepted
0.1 product line casually.

### Git and phase identity

Every completed implementation phase `ME.V0` through `ME.V7` ends with a
dedicated phase-completion commit after its gate passes. The commit carries:

```text
Phase-Gate: ME.Vn=pass
Evidence-Digest: sha256:<digest>
Product-Version: <current VERSION>
```

Evidence uses a canonical content-tree/input digest that excludes Git metadata
and self-referential receipt fields. The phase commit itself supplies the Git
identity. No checked-in file attempts to contain the hash of the commit that
contains that file.

If the repository is not yet under Git, the implementation program may
initialize a local repository to satisfy the explicitly requested phase-commit
workflow. It must not invent or push a remote.

### Tags

Release tags are annotated and use:

```text
mainframe-env-v0.1.0-alpha.N
mainframe-env-v0.1.0-beta.N
mainframe-env-v0.1.0-rc.N
mainframe-env-v0.1.0
```

A tag points to a clean release-preparation or final-release commit. Tags are
created only after the corresponding release gate passes. Pushing tags or
publishing artifacts remains an explicit external release action.

## Consequences

- Phase history is reviewable and resumable.
- Release artifacts and source can be mapped to an immutable commit and content
  digest without recursive evidence churn.
- Product and contract evolution can proceed independently.
- The 0.1 line has a real compatibility promise despite being pre-1.0.
- Breaking experimentation occurs before a surface is promoted or in the next
  minor line, not in an unannounced patch.
