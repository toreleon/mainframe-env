# Versioning, Phase Commits, and Release Gates

Status: **Accepted by repository owner**
Owner: **release maintainers**
Scope: **version authorities, release gates, artifacts, and publication**
Applies from: **mainframe-env 0.1.0**
Initial release line: **mainframe-env 0.1**

## 1. Version inventory

The version check generates a machine-readable inventory containing:

```text
product version and channel
workspace crate versions
Rust toolchain and MSRV
supported profile manifest versions
public contract/interface/dialect versions
configuration version
artifact/checkpoint/session versions
SQL schema and migration head
z/OSMF compatibility versions
fixture/evidence schema versions
```

Every released artifact embeds or exposes enough of this inventory to diagnose
compatibility without relying on a source checkout.

## 2. Phase commit protocol

Every phase `ME.V0` through `ME.V7` must end in one dedicated completion commit.

### Before the commit

1. Complete the phase deliverables and update phase-specific documentation.
2. Run the narrow and phase gate commands required by the implementation
   prompt.
3. Generate/check phase inventory, schemas, evidence, and content digest.
4. Update `docs/delivery/IMPLEMENTATION-STATUS.md` and
   `conformance/0.1/evidence/program-status.json` before staging.
5. Review the complete diff and ensure unrelated user changes are not staged.
6. Confirm the phase result is derived `pass`; do not create a completion
   commit for a failing phase.

### Commit subjects

| Phase | Required subject |
|---|---|
| ME.V0 | `Complete ME.V0 workspace foundation` |
| ME.V1 | `Complete ME.V1 foundation contracts` |
| ME.V2 | `Complete ME.V2 COBOL execution path` |
| ME.V3 | `Complete ME.V3 dataset and security authorities` |
| ME.V4 | `Complete ME.V4 typed CICS runtime` |
| ME.V5 | `Complete ME.V5 JCL and JES batch path` |
| ME.V6 | `Complete ME.V6 durable z/OSMF product` |
| ME.V7 | `Complete ME.V7 certification and cutover` |

The body includes:

```text
Phase-Gate: ME.Vn=pass
Evidence-Digest: sha256:<canonical phase digest>
Product-Version: <VERSION>
```

### After the commit

- Verify the commit contains only the intended phase scope.
- Verify the phase commit and trailers are readable from Git history.
- Do not edit the just-committed evidence merely to insert its own commit hash.
- The next phase status update may reference the prior phase commit normally.
- Do not amend a commit after it is used by a release tag or shared externally.

Intermediate repair or focused commits are allowed, but they do not replace the
mandatory phase-completion commit.

## 3. Release channels

### Development snapshot

`0.1.0-alpha.0` is the initial development identity. It is not published and
has no compatibility promise beyond checked-in contracts.

### Alpha gate

A `0.1.0-alpha.N` release may be prepared only when:

- ME.V0–ME.V4 pass;
- the foundation, contracts, COBOL reference execution, dataset/security, and
  typed CICS vertical path work through public services;
- no old implementation is linked;
- selected fixtures and hostile controls pass;
- the full workspace, all targets, and all features check on both the pinned
  toolchain and declared MSRV; and
- known incomplete JCL/JES/z/OSMF/durability work is explicitly documented.

Alpha is for architecture and integration validation, not production use.

### Beta gate

A `0.1.0-beta.N` release may be prepared only when:

- ME.V0–ME.V6 pass;
- the complete frozen 0.1 functional surface is implemented;
- `core-server` dependency closure is clean;
- SQLite and PostgreSQL migrations and restart recovery pass;
- z/OSMF compatibility suites pass or carry accepted corrections;
- normal, condition, failure, cancellation, authorization, overload, and store
  saturation gates pass; and
- public configuration/API/known-limit documentation is complete.

Beta freezes supported 0.1 public semantics. Later incompatible changes require
an explicit compatibility decision and usually a new minor line.

### Release-candidate gate

A `0.1.0-rc.N` release may be prepared only when:

- ME.V0–ME.V7 functional gates pass on the candidate tree;
- no known release-blocking correctness, security, durability, compatibility,
  or unbounded-resource issue remains;
- full release validation succeeds on advertised targets;
- long-run, mixed-load, 2x overload, leak, backup/restore, crash-point, and
  rollback/cutover rehearsals pass;
- schema/API/SemVer checks pass;
- SBOM, checksums, provenance, package contents, configuration schema,
  migrations, docs, and release notes are reproducible; and
- the source tree is clean at the candidate commit.

Only release-blocker repairs enter an RC. Each repair triggers the smallest
affected validation and then one new full candidate gate.

### Final 0.1.0 gate

`mainframe-env 0.1.0` may be released only when:

- the latest RC is reproduced from a clean checkout or equivalent immutable
  source snapshot;
- artifact digests match the release manifest;
- supported package/profile contents contain no excluded component or test
  harness;
- all required migrations and rollback instructions are present;
- security/advisory/license/source policy passes on the locked dependency graph;
- final compatibility, operations, security, quality, and owner decisions are
  recorded truthfully;
- the final cutover/default-authority state is correct; and
- the generated final gate reports `MAINFRAME-ENV 0.1 = PASS`.

The final release commit changes the version to `0.1.0`, finalizes changelog and
release notes, regenerates the release manifest, and receives the annotated tag
`mainframe-env-v0.1.0` after the gate passes.

## 4. Release artifacts

A release candidate/final release produces, as applicable:

- server and CLI binaries for advertised targets;
- `core-server` package manifest;
- default/example product configuration and schema;
- SQL migration bundle and migration digest;
- operation, capability, route, diagnostic, and compatibility catalogs;
- public API and crate documentation;
- license notices and SBOM;
- checksums and build/source provenance;
- compatibility/evidence summary and known limitations;
- backup, restore, upgrade, rollback, and capacity runbooks; and
- release manifest mapping every artifact to source commit, content digest,
  toolchain, target, features/profile, and dependency lock identity.

Offline Cargo bundles additionally retain `SUPPLY-CHAIN/BUILD-INPUTS.json`,
which binds the exact vendored tree, source revision, locked CI/controller
inputs, and build-tool executable identities. CI input changes follow
`docs/runbooks/CI-SUPPLY-CHAIN.md`.

Raw credentials, local absolute paths, uncontrolled raw evidence, temporary
files, oracle binaries, test datasets, and current OpenMainframe implementation
code must not appear in release packages.

## 5. Release manifest

The release manifest records:

```text
product/version/channel/tag
source commit and canonical content digest
dirty=false
Rust/Cargo/tool versions
Cargo.lock digest
profile and package closure digest
contract/version inventory digest
configuration/migration heads
artifact names, media types, sizes, targets, and SHA-256 digests
SBOM/provenance/checksum identities
conformance/security/resource/recovery gate digests
known limitations and compatibility range
```

The checked-in candidate data uses the canonical content digest. The annotated
tag and externally generated release receipt bind it to the final Git commit,
avoiding self-referential commit hashes.

## 6. Patch releases

For `0.1.x`, use:

1. issue/security/compatibility classification;
2. focused regression fixture;
3. implementation and narrow validation;
4. contract/schema compatibility check;
5. affected profile and release-gate replay;
6. changelog/release-note/version update;
7. release commit and annotated tag; and
8. publish only after artifact/provenance verification.

The supported offline Cargo archive is produced only by
`tools/package_offline_cargo_bundle.sh`. Its archive helper uses the locked
Linux/amd64 GNU-tar image, perturbs and normalizes two clean copies, and accepts
the output only when both SHA-256 digests match. Publication downloads any
same-named GitHub asset and skips it only when the bytes match. A different
remote digest or a concurrent name collision stops publication; release assets
are never uploaded with overwrite semantics.

Patch releases may add backward-compatible optional behavior or fix defects.
They may not remove or silently redefine supported 0.1 behavior.

## 7. Release stop-the-line

Do not create or publish a release tag when:

- the source tree is dirty or contains unrelated changes;
- the version sources disagree;
- a required phase commit/gate is absent;
- a public or durable schema change is incompatible with the release channel;
- an artifact cannot be reproduced or matched to source/toolchain/profile;
- a supported route has two default authorities or a hidden fallback;
- a migration lacks backup, restore, or rollback evidence;
- an excluded component appears in a shipped closure;
- security/advisory/license/provenance policy fails;
- evidence is stale, fabricated, or tied to a different source digest; or
- owner authorization for final publish/cutover is absent.

Local phase commits are required by the implementation program. Remote pushes,
release tags, package publication, and production deployment remain external
actions and require their corresponding authorization.
