# z/OSMF — REST portfolio progress

Subsystem: **zosmf**
Phase: **rest**

Status: **ZMF-1101 operation-normalization foundation complete; no new routes advertised**

Candidate branch: `codex/parallel-zosmf.rest-zmf1101`

Rebased integration parent: `782c65830845ae2c0ddc7289977521fcaf3fa3fa`
(`origin/main` on 2026-09-24).

Original preparation parent: `5ab706b1dd069e26db7cb9a2b66e921c9001fc39`
(`origin/main` when this isolated lane started on 2026-09-22).

## Scope and dependency boundary

This lane owns only ZMF-1101: the source normalization and generated contract
foundation for the pinned z/OSMF 3.2 portfolio. It does not implement ZMF-1102
through ZMF-1106, publish a new route, or claim the zosmf.rest exit gate.

The checked-in racf.security, dataset.data, and jes.execution status records identify accepted local
RACF/SAF, dataset/AMS, and JES/spool implementations, with their licensed
differentials still explicitly pending. No `cics/system-api-status.md` or accepted
cics.system-api SPI/FEPI candidate exists at this parent. CICS-backed z/OSMF operations
therefore cannot be assigned an accepted cics.system-api backend here. The same rule is
applied to every other family: an architectural package that might eventually
host an operation is not recorded as an accepted capability unless the current
source and evidence expose that typed backend.

## Declared bounded slices

| Slice | Parent | Semantic scope | Dependencies | Acceptance gates | State |
|---|---|---|---|---|---|
| `ZMF-1101.source-normalization` | `ZMF-1101` | Map all 27 family rows and all 189 direct-guide heading rows to hash-pinned source identities, explicit dispositions, aliases, and source-proven operation/method/path variants | immutable coverage.foundation z/OSMF catalog and manifest | offline source verifier, catalog schema, positive/negative/mutation tests, `git diff --check` | pass; rebased commit `a4219a9f` |
| `ZMF-1101.generated-contracts` | `ZMF-1101` | Generate route, operation, schema, error, backend-ownership, collision, and closure artifacts from the one normative catalog without changing public registration | source-normalization slice | deterministic regeneration, Draft 2020-12 validation, route collision/closure and official/custom separation tests, gateway regressions, architecture/docs/format/policy checks | pass; sealed by the bounded completion commit containing this status |

The parent ZMF-1101 milestone is complete because both declared slices pass and
the generated closure agrees with the normative catalog. This is not the
zosmf.rest exit gate: ZMF-1102 through ZMF-1106 remain separate later work packages.

## Source authority

- Baseline: `ibm-zosmf-3.2-2026-07-27`.
- Product/publication: z/OSMF 3.2 Programming Guide, `SSLTBW_3.2.0`, pinned by
  `conformance/subsystems/coverage/manifests/zosmf-topics.json`.
- Manifest identity:
  `sha256:146cb3faaebd146c6396cc57b04ad6e48b49533b37dfa34f1eca2411518cea9b`
  over 395 topics and 9,761,388 bytes.
- TOC identity:
  `sha256:477a3de1989e3cb185f2dacdc7cea9bab8304ef148e7c9e5e8be55fe3f77dc2b`.
- Retained bodies: the matching content-addressed HTML under
  `/Users/tore/Library/Caches/mainframe-env/ibm-docs-archive/raw/html/sha256`.
  This host path is review context only and is not embedded in generated
  product artifacts.

The offline reader verified all 395 topics and the TOC. Review used
`conformance/tools/ibm_docs.py search` and `read`, including the concrete
`GET /zosmf/info` page and the Application Linking Manager overview that lists
six method/path pairs. No browser, network fetch, PDF, or substituted body was
used.

## Normalization decisions

- The 27 family rows and 189 heading rows remain distinct denominator
  identities. A family overview is not silently counted as a route.
- Heading dispositions distinguish context, family/group overview, concrete
  operation, body-discriminated operation bundle, schema reference, and error
  reference.
- The Runtime Diagnostics family overview is also the source of its concrete
  analyze-anomaly operation; this is recorded against the family row instead
  of inventing a missing heading row.
- Cloud provisioning group pages enumerate operations. Six resource-pool
  operations also have individually pinned child heading rows; those locators
  become aliases of the same normalized operations, not duplicate operations.
- Data-set/member and UNIX utility headings contain multiple request-body
  actions on shared method/path pairs. Their action identities remain distinct
  while collision reporting records the required typed discriminator.
- Source URI spellings and normalized route templates are both retained.
  Query parameters do not create separate router paths, and path aliases do not
  erase source provenance.
- Every normalized operation receives explicit request, response, error,
  authorization, state/failure, backend-capability, publication, and mandatory
  obligation identities. A pending detailed schema or absent backend remains a
  blocking disposition; a generated identity is not execution credit.

## Existing public boundary

The frozen 23 official route bindings under `/zosmf/*` and seven custom CICS
session bindings under `/mainframe-env/*` remain unchanged. Custom routes have
zero official coverage credit. ZMF-1101 may associate a normalized operation
with an existing official route, but it neither upgrades that route's semantic
claim nor makes an unowned family executable.

## Path-to-gate map

| Changed boundary | Required checks |
|---|---|
| normative z/OSMF normalization catalog | source/manifest closure, Draft 2020-12 schema validation, positive/negative/mutation normalization tests |
| generated z/OSMF artifacts | deterministic generation/check, operation and schema reference closure, classified route collisions, backend/publication closure |
| gateway generated metadata | 23-route preservation, handler closure, official/custom namespace separation, focused gateway tests |
| status/architecture documentation | documentation and architecture-fast checks |

## Current blockers carried forward

- There is no accepted cics.system-api SPI/FEPI dependency candidate in this checkout.
- Most pinned z/OSMF families have no accepted typed backend capability in the
  current product. Planned ownership is recorded separately from accepted
  capability evidence.
- Detailed request/response/error field normalization remains explicit where a
  heading proves a route but this bounded slice has not yet frozen every payload
  property. Such operations remain withheld from publication.
- Licensed z/OSMF 3.2 differential execution belongs to ZMF-1106/certification.licensed and is
  not available or claimed by this source-review foundation.

## Source-normalization result

The source-normalization slice records 27 families, 189 headings, 216 distinct
source rows, 278 normalized operations, 352 method/URI variants, and 1,727
mandatory obligation identities. Heading dispositions are 174 concrete
operations, six operation-group overviews, two body-discriminated operation
bundles, two schema references, four error references, and one context
reference. The retained-source verifier matched all 216 used HTML bodies and
358 route-evidence locations to their manifest pins; the extra six evidence
locations are the individual aliases of resource-pool operations also listed
on their overview page.

Backend ownership is intentionally non-promotional: one family capability is
accepted, four are partial, six have a planned owner but a missing typed
capability, and sixteen remain unresolved. At operation level, 22 map to an
existing backend/route, 38 are missing, 27 are partial, and 191 are unresolved.
All 256 non-accepted operations are withheld. None of these counts is a
recognized, validated, executed, conditioned, recovered, or differential pass.

## Generated-contract result

The deterministic generator emits:

- `zosmf-contracts.json`: 278 operation descriptors, 352 route variants, 583
  request/response/error schema identities, 27 error families, 27 backend
  ownership rows, 1,727 mandatory obligations, and the unchanged 23 legacy
  official route bindings;
- `zosmf-collision-report.json`: 310 route keys, 14 shared-dispatch groups,
  five same-operation source-alias groups, one frozen body-discriminator group,
  and 13 blocked groups that still require typed dispatch contracts; and
- `zosmf-closure-report.json`: complete source/normalization closure, zero new
  official routes, seven custom routes with zero official credit, 256 withheld
  operations, and an explicit `zosmf_0_11_exit_complete=false` result.

The generated gateway module is metadata only. It contains 27 family backend
rows and the mapping of the frozen 23 routes to normalized operation identities;
`official_routes::register` remains generated solely from the platform.runtime-integration/coverage.foundation frozen
route authority.

Artifact SHA-256 identities for the accepted generated bytes are:

- contracts: `e36a0c4e4f219d7bf990dd7b3b2386c53cd739a94e3d0099c4e0a4c78544cf8a`;
- collision report: `5f2d554aea1fdbf4193c4f897d971dcfa4c49e19824fa1caeda205a68174b5ec`;
- closure report: `e81312673e767ccf202f19b0cb22b76e7e7bc6500058a6fe3248931a7b5de79e`;
  and
- gateway metadata: `a260751dc6ad399b6d127c4a72aba59ed44669827079a50788224758210a52e3`.

## Focused validation

Passing checks on this candidate:

- offline `ibm_docs.py status`, search, and bounded reads for the z/OSMF
  baseline;
- `verify_zosmf_sources.py --archive-root ...`: 216/216 bodies, 358/358 route
  evidence locations, and the one pinned TOC verified;
- four Python positive/negative/mutation source-normalization tests;
- `cargo xtask zosmf-contracts --check`;
- `cargo xtask route-registries --check`;
- two focused xtask generator tests;
- all five existing z/OSMF gateway unit tests and the new generated-contract
  namespace/denominator test;
- `python3 -B tools/check_module_boundaries.py`;
- `cargo clippy -p mainframe-env-zosmf --all-targets --locked -- -D warnings`;
- `cargo xtask docs --check`;
- `cargo fmt --all -- --check`; and
- `git diff --check`.

The repository-wide `cargo xtask schemas --check` reached and accepted the new
zosmf.rest schema/catalog validation, then stopped on the unchanged jes.execution
`carddemo-base-batch.json` note exceeding its unrelated 256-character schema
bound. That evidence file and its schema are unchanged by ZMF-1101. The rebased
mainline resolves the other inherited findings recorded during initial
development: module boundaries now pass, `WRITE_LENGTH_SOURCE` is used, and
`Cargo.lock` carries the advisory-fixed `rustls 0.23.45`. Unrelated broad suites
were not repeated merely because the branch base changed.

## Next executable step

Resolve one backend-bounded family slice under ZMF-1102 or ZMF-1103, including
its 13 still-blocked shared-dispatch keys where applicable, before proposing any
new route publication. The cics.system-api SPI/FEPI dependency and licensed z/OSMF
differential environment remain external prerequisites to their later gates.
