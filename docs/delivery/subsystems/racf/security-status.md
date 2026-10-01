# RACF / SAF — Commands and authorization progress

Subsystem: **racf**
Phase: **security**
Target release: **0.5.0**

Status: **Implementation candidate; pass with licensed differential pending; not released**

The isolated implementation branch `impl/0.5.0` starts from the controller-
accepted CI-300 repair candidate
`7d50310381a44878c23c51c38aed841e3a70347f`. The 0.2 dependency evidence is
checked in under `conformance/0.2/`; its frozen RACF/SAF baseline is
`ibm-zos-3.2-racf-saf-2026`, with 34 command-family rows and 14 RACROUTE rows.
The 0.5 implementation must reuse shared Conformance IR v1 without an
incompatible contract change.

## Current work package

SEC-501 is complete. The v2 database freezes bounded owned schemas for
principals, group connections, templates/segments, classes, profiles/access,
ACEE/tokens, certificate/key references, SAF decisions/reasons, audit,
transactions, recovery, and migrations. A focused repair added the originally
missing user/group template-to-segment binding before command integration.
The existing RACF host surface uses that single CAS-updated provider-state
authority; CardDemo compatibility, default deny, redaction, SQLite restart,
schema compilation, and affected consumer tests pass.

SEC-502 is complete. One generated catalog recognizes and validates all 34
frozen command families. The 21 SEC-502 user/group/connection/dataset/general-
resource/list/search families execute through common atomic transactions with
authorization, deny audit, idempotent replay, multi-target rollback, concurrent
CAS retry, bounded malformed diagnostics, and no secret-bearing public AST.
The shared Conformance IR now contains 34 RACF command rows, 110 obligations,
and 165 bindings; the current focused run reports recognition 34/34,
validation 34/34, execution 21/34, and conditioned behavior 34/34.

SEC-503 is complete. Supplied class defaults are generated from a reviewed
schema/configuration catalog and installation-defined classes use the same
validated descriptor. SETROPTS owns class activation, generic policy, RACLIST
snapshots/refresh, PROGRAM control and global options; RACPRIV, RACPRMCK, SET,
RVARY, STOP, and RESTART use the same transaction/audit authority. Tests prove
stale RACLIST behavior until refresh, fail-closed absent cache/inactive class/
stopped subsystem/database, PROGRAM enablement, custom CDT-style class data,
and operational transitions. The shared IR now has 124 obligations and 186
bindings, reporting execution 28/34 while retaining recognition/validation/
conditioned 34/34. SEC-504 is active; recovery and licensed differential remain
pending by construction.

SEC-504 is complete. A generated registry is exact against all 14 frozen
RACROUTE rows, and every request has typed recognition/validation, authorized,
deny, malformed/status, atomic state-machine, and audit bindings. One access
evaluator now owns host AUTH, RACROUTE AUTH/FASTAUTH, group hierarchy,
conditional access, security level/label/category, PROGRAM, and RACLIST
semantics. ACEE creation/deletion/nesting, delegation, safe token build/map/
extract, cache-backed LIST, and return/reason matrices pass. The shared IR now
contains all 48 RACF/SAF rows, 180 obligations, and 270 bindings; current counts
are recognition 48/48, validation 48/48, execution 42/48 (all 14 RACROUTE plus
28 command families), and conditioned 48/48. SEC-505 is active.

SEC-505 is complete. PASSWORD/PHRASE policy and history, safe-reference
certificate/key/keyring lifecycle, MFA enrollment and VERIFYX proof checking,
distributed identity mappings, RRSF nodes/user associations, durable sign-on
sessions, and SIGNOFF invalidation all use the same bounded database and
transaction/audit authority. Focused tests prove referenced secrets do not
enter public results or audits. The shared IR now contains 48 rows, 192
obligations, and 288 bindings; recognition, validation, execution, and
conditioned behavior each report 48/48 (34 commands plus 14 RACROUTE). SEC-506
then owns audit hardening, recovery, migration, the independent simulation, and
the fail-closed licensed adapter.

SEC-506 is complete under the user-approved 2026-09-01 scoped completion
policy. Audit fields now pass one
content- and name-aware redactor before durable storage and bounded SMF type-80
projection. Retained v1 user/group/profile/audit records migrate atomically to
v2 with source/result digests; corrupt input publishes no partial migration,
and rollback keeps the v1 records available to the old reader. Startup
reconciles intent and unknown-outcome transactions exactly once, a lost commit
acknowledgement replays without duplicate mutation, and the full SEC-505 state
survives SQLite restart. Explicit per-row audit/redaction and bounded-limit
obligations prevent broad tests from implying those results, while per-row
atomic-retry obligations execute each selected route beside a concurrent CAS
mutation. The shared IR has 48 rows, 384 obligations, and 480 bindings;
recognition, validation, execution, conditioned behavior, and recovery each
report 48/48. A table-driven pure-state reference simulation independently
covers all 34 command-family and 14 RACROUTE rows without importing the
production provider. It checks precedence and transition matrices, return and
reason codes, audit/redaction, restart/cache, token/ACEE, and credential/MFA
invariants; installation exits, cryptographic material, undocumented database
internals, and RRSF transport timing remain explicit unknowns. Property,
metamorphic, and mutant tests prevent the simulation from collapsing into a
generic-success or shared-implementation oracle. The workspace regression
passes.

This lane has no configured licensed z/OS 3.2 RACF/SAF oracle or receipt, so
licensed differential remains exactly 0/48 pending rather than inferred. The
scoped policy permits that pending state for the 0.5 exit only and moves the
real 48-row campaign to the 0.17 `release-certify` hard gate.
The fail-closed campaign adapter is ready: it accepts only one bounded,
secret-scanned 48-row receipt at
`conformance/0.5/racf/licensed-oracle.json`, checks its frozen source/product/
license and fixture identities, requires external licensed execution plus a
fresh release-certify campaign from approved adapter/environment namespaces,
executes the same current-product routes, and requires canonical observation
equality before adding shared-IR differential bindings or an oracle digest.
Simulated, modeled, documentation-derived, historical, and current-product
origins fail before registry projection. With no receipt file, the generated
spec remains unchanged and contains no differential claim.

## Controller review 1 repair

The first controller/CLI review of PR #4 is repaired on the existing branch.
ALTUSER now separates bounded self-service fields from SPECIAL-only authority,
and ADDUSER, ALTUSER, and PASSWORD/PHRASE use one length/current-password/
history policy path. DFLTGRP requires an active group connection. PERMIT WHEN
conditions are typed, evaluated, and independently replaceable/deletable;
unsupported conditions fail before protected mutation. Direction operands that
come only from the command-direction grammar fail before local execution, while
the explicitly cataloged SIGNOFF AT filter remains local and typed.

RACROUTE now binds a digest of the complete request and caller context to one
durable transaction and bounded terminal-result projection. Exact replay does
not duplicate ACEEs, tokens, sessions, profiles, cache refreshes, audits, or
returned identities; key reuse with a different request fails closed, and an
unknown outcome reconciles once across restart. Caller and request-shape checks
precede secret resolution, missing and wrong credential handles have the same
external denial shape, ACEE extraction requires owner/auditor/SPECIAL, and
closed ACEEs/sessions are safely removed when their user is deleted.

Legacy migration probes every namespace at configured maximum plus one with
checked arithmetic. Overflow fails before a migration marker or partial v2
state, and a backing store that cannot expose the probe is treated as
fail-closed rather than silently truncated. Audit prefix redaction is
case-insensitive through durable storage and type-80 projection. Work-package
sealing disables rename detection so both old/delete and new/add paths enter the
allowlist and digest.

The independent reference model now emits a bounded normalized observation per
binding and compares it to a separately projected product observation. Status
and reason codes, semantic-domain deltas, returned identity kind, access
decision, audit/redaction, restart/atomic behavior, and exact RACROUTE replay
must match. Reference restart serializes and reloads pure state; representative
generic-success, no-op, wrong-transition, wrong-status, missing-audit,
authorization-bypass, unsafe-projection, shared-reuse, and replay-duplication
mutants are rejected. This adds no coverage gate or subsystem-local IR and does
not change licensed differential credit.

## Work package ledger

| Work package | State | Deliverable |
|---|---|---|
| SEC-501 | pass | Generic profile template/segment database |
| SEC-502 | pass | User/group/resource command processor families |
| SEC-503 | pass | SETROPTS, class activation, RACLIST, cache, and policy |
| SEC-504 | pass | ACEE/token and complete RACROUTE state machines |
| SEC-505 | pass | Certificates, keyrings, MFA, identity mapping, and RRSF |
| SEC-506 | pass-with-licensed-differential-pending | Audit, migration, recovery, independent simulation, and fail-closed licensed adapter |

## Decisions

- RACF/SAF observable semantics remain owned by `mainframe-env-racf`; the
  provider-state store remains the durable transaction substrate.
- The shared 0.2 catalog and CI-300 Conformance IR, registries, runner, shard
  identity, ScenarioSpec, verdict events, and derived ledger remain the only
  conformance authorities.
- Secret and cryptographic material crosses stable boundaries only as owned
  references or redacted outcomes. Existing Argon2 and zeroization libraries
  remain the password/secret primitives; Ring supplies constant-time HMAC proof
  comparison without exposing a third-party type across a stable boundary.
- The independent reference simulation is development assurance only. It does
  not create an oracle receipt, add differential bindings, or change the
  licensed numerator from 0/48.
- No public 0.5 behavior will be claimed until the 0.2 dependency checks and
  the relevant executable obligation gates pass.

## Blockers

There is no remaining 0.5 implementation blocker. Licensed differential is
still unavailable and remains 0/48 pending, but the user-approved scoped policy
makes it a 0.17 release-certification blocker rather than a 0.5 completion
blocker. No result is fabricated, inferred from simulation, or repurposed from
historical evidence.

## Deferred certification step

At 0.17 `release-certify`, install a real approved 48-row campaign at
`conformance/0.5/racf/licensed-oracle.json`, regenerate the shared spec, and run
the licensed differential gate on the unchanged release candidate. Until then,
the numerator remains exactly 0/48 pending.
