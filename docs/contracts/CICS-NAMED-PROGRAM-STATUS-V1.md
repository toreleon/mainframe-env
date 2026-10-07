# Private named PROGRAM STATUS consumption boundary

Status: **Proposed private prerequisite**
Owner: **CICS command, ProgramControl, security and storage maintainers**
Scope: **named PROGRAM STATUS for a trusted public, local, non-Java cohort**
Applies from: **mainframe-env development**

Runtime admission and integration: **Pending**

This boundary prepares SPI row
`ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0155` over the existing CICS
ProgramControl, State, command, condition, SAF, effect and storage authorities.
It grants no executable catalog entry, public profile, command coverage or
application dependency acceptance. The application catalog keeps all 263 rows,
including the three user-deferred unready rows 0027, 0093 and 0114.

## Consumed scope and owners

The first candidate is a named inquiry of STATUS for a trusted public issuer and
a local, public, non-Java artifact-backed PROGRAM definition. These are bounded
product admission limits; other IBM-supported programs and forms stay Pending.
PROGRAM and STATUS select this candidate. RESP, RESP2 and NOHANDLE use the
existing condition policy. STATUS remains optional in the full IBM grammar.
Browse, application selectors and other outputs are outside this candidate.

The manager owns typed compiler/plan/host admission, append-only identity tags,
receiver validation, policy configuration and integration. ProgramControl owns
definition observation. Existing coordinator, SAF and stores keep their current
responsibilities. A distinct SPI identity must select the future typed route;
the legacy generic `CicsOperation::Inquire` existence handler cannot supply it.
No fixture name, application name or arbitrary caller binding selects a handler.

## Admission and observation

1. The existing trusted run/program owner must attest the issuing program's
   context and the visible catalog cohort. Missing platform bindings alone do
   not establish a public issuer. Application-entry metadata does not implement
   private-first/public-fallback namespace resolution.
2. Validate the closed operands and resolved receiver before provider dispatch.
   PROGRAM has the reviewed eight-byte source ceiling and existing project name
   normalization. STATUS needs a writable, unscaled, four-byte binary area.
   RESP2 retains the existing RESP dependency. Unsupported operands, unresolved
   storage and missing context authority fail closed.
3. Preserve transaction admission, then perform distinct configured command and
   named-resource checks through the existing typed SAF owner before disclosure.
   The source command resource identifier is PROGRAM, with configured prefixing
   where applicable. The actual class/profile/access configuration and active
   CMDSEC policy must be supplied and validated by that owner. The existing
   FACILITY Execute existence check is insufficient. The pinned VCICSCMD
   examples do not by themselves freeze the deployment's complete policy.
4. Reuse `observe_named_status` under the existing State mutex. An immutable
   definition's `enabled` field supplies ENABLED or DISABLED for this bounded
   cohort. Observe generation and status together; a disabled definition remains
   a valid inquiry result. NameOnly and NotCatalogued do not supply a status.
   No module load, autoinstall, execution, refresh or use-count change occurs.

The current helper distinguishes absence in its own catalog only. Maps,
partition sets and private application resources have other unresolved owners.
PGMIDERR 27/1 requires an attested complete visible namespace or a closed test
cohort; a bare NotCatalogued result cannot establish that condition generally.
Corrupt definition metadata retains InfrastructureFailure. Missing authority,
malformed product ABI and unsupported shape retain typed product failures rather
than invented IBM response codes.

## Receiver and condition boundary

The future STATUS payload must use the existing typed CICS output transport and
resolved storage writer. Its source-reviewed values are ENABLED 23 and DISABLED
24. Four-byte extent, schema, numeric membership, output name, operation and
disposition all require validation before storage writes. Reuse the existing
project binary/CVDA conversion conventions; IBM's fullword description does not
establish a native host C ABI, alignment or byte order. No wire tag or public
operation tag is assigned by this preparation document.

A verified command denial maps to NOTAUTH 70/100; verified resource denial maps
to NOTAUTH 70/101. Browse END/ILLOGIC conditions are outside this candidate.
The source does not order simultaneous authorization and existence failures.
Malformed or unexpected STATUS results must not overwrite the receiver. Keeping
an error receiver's sentinel is a fail-closed product expectation requiring a
test; it is not a general IBM SPI output-preservation claim.

## Effects, concurrency and restart

The inquiry is read-only for installed resource, load and UOW state. Existing
task lifecycle, condition fields and mandatory security/effect/audit publication
still apply. No resource codec, undo row, recovery coordinator or read-result
ledger is added. MECPGD1 definitions and their immutable references stay intact.

The first scope requires one live catalog owner. In-instance registration and
observation must expose one whole generation. Cross-instance cache freshness
remains Pending. Stable-definition close/reopen can reproduce a fresh inquiry;
it does not prove IBM warm restart. Existing coordinator resume may reobserve a
read and reject a changed result digest as UnknownOutcome. Never convert that
uncertainty to success or add automatic redispatch.

## Required evidence before a runtime binding

- Current scoped application contract-consumption evidence and explicit manager
  disposition, using the existing Conformance IR/verdict/ledger machinery.
- Independently reviewed issuer/namespace and command/resource SAF configuration,
  including missing, forged, deny and infrastructure-failure controls.
- Selected typed product route with positive enabled/disabled results; receiver
  width, representation, ownership, name bounds and unsupported-form negatives.
- Complete resource/UOW/load snapshots around queries and failures; audit/effect
  outcomes, deadline/cancellation and coherent concurrent registration controls.
- Applicable SQLite/PostgreSQL reopen and unknown-outcome/replay proofs on the
  selected unchanged candidate; no credit from ignored or historical runs.

Until those criteria pass, this document and the existing private observation
helper remain preparation. All six command gates, licensed differential and
parent completion remain Pending, with zero new credit.

## Pinned source authority

- Row 0155, `ibm-cics-ts-6x-spi-command-bodies-2026-09-12`,
  `SSJL4D_6.x/reference-system-programming/commands-spi/dfha8_inquireprogram.html`,
  SHA-256 `e3d8ed4c069bd26822c6373b35278ec126591e2efaf020b8fbcf8b811845f20f`:
  parser 35–56 no-load/module scope; 65–103 namespace; 566–572 STATUS;
  605–619 authorization and missing-resource conditions.
- `ibm-cics-ts-6x-application-api-sources-b-2026-09-10`,
  `SSJL4D_6.x/reference-security/command-security-resource-reference.html`,
  SHA-256 `57539dc40fa06aa78da3b435a22c1955fa750d30a47f865341d9bc6e78417c8d`:
  parser 3–16 identifier/prefix roles; 306–310 PROGRAM; 503–523 examples and
  remaining access cross-reference.
- The same sources-b baseline, `SSJL4D_6.x/system-programming/intro/dfha80x.html`,
  SHA-256 `81f101e030365400b431ecf68250dfcabc5673e1acbf05010c9285bf590e3b25`,
  parser 7–15 receiver/fullword rules.
- `ibm-cics-ts-6x-misc-tail-cvda-2026-09-23`,
  `SSJL4D_6.x/reference-applications/commands-api/dfha80c.html`, SHA-256
  `5b95b620971d42a9f57511b362f9a12dc04e9ad4b9c26f42cc8e7be943221381`:
  parser 442–443 DISABLED 24 and 528–529 ENABLED 23. Existing private PROGRAM
  numeric facets retain this reference pin; no new encoding authority is added.

Source consultation is offline reference review and earns no execution or
licensed credit. Publication bodies remain outside Git.
