# ADR-0068: Owned local RACF profile composition

Status: proposed local first-party contract; implementation and controls unrun
Owner: application composition and RACF storage maintainers
Scope: finite signed profiles in one genuine local provider-state transaction

Applies from: mainframe-env 0.2.0 local application profile composition

## Context

The signed application package already authenticates opaque Profile entry bytes and
the class/profile/owner security section. It does not define a RACF Profile payload
codec. Sequential `define_profile` and `permit` calls independently commit the mutable
MERACF2 authority snapshot, so they cannot establish the local composition transaction
required by ADR-0064.

## Decision

The finite first-party payload `mainframe-env.application-local-racf-profile@1` has
canonical compact JSON fields `contract` and `declaration`. The declaration contains
the exact uppercase class, name and owner, explicit UACC, and a complete ordered map
of unconditional principal access levels. The signed Profile entry is
`security-profile/{class}/{name}`. Its complete bytes and the original signed
class/profile/owner section must agree with the typed declaration. No missing grants,
owner, class, template or UACC are inferred.

This local profile uses the existing ResourceProfile representation: the explicit
unconditional ACL has no condition and no audit policy; the security level is zero,
label absent, categories and segments empty. These are this named first-party
profile's declared boundaries, not IBM defaults. Existing class and template
validation, principal references and provider limits still apply. A raclisted class
or retained raclist cache refuses until its separate owner participates. Arbitrary
segments, conditions, credential creation, class installation and vendor command
interpretation remain outside this finite declaration.

The RACF participant holds the actual SecurityDatabase writer after the existing
CICS and dataset preparation owners. It decodes the real current whole authority,
changes only participating profiles, validates and encodes the complete original
snapshot, and offers one expected-version write to the same genuine
ProviderCompositionCommit. No write occurs during preparation and no authorize or
mutation API is called while holding the writer. The owner finishes only when the
acknowledgement covers its exact Arc, full payload and expected version.

CURRENT selected package projections fence foreign profile names before an own retry.
The selected-package and retained-generation identities must be present and coherent.
Historical unselected generations do not reserve names. Existing foreign profiles
without a prior owned projection refuse even if their content happens to match.
Trusted administration can change or remove a profile, but cannot let a different
CURRENT package borrow the original selected name. This does not exclude another
SecurityDatabase instance or trusted direct provider-store writers.

The local selection stores exact participating ResourceProfiles with their actual
versions and required class/template context. The composition identity excludes only
the automatically assigned profile version, allowing retained rollback to restore
the signed declared content at a fresh monotonic version. Readiness still compares
the full actual version and content under the writer. A rollback re-reads the current
snapshot and preserves every unrelated runtime domain; it never reinstalls a saved
whole authority. Ordinary audit, session, token and other runtime updates therefore
remain valid and survive retry and rollback.

Unknown shared-store outcomes quarantine the live RACF database in addition to the
existing seed/CICS/server owners. Snapshot reads, mutations, preparation and retention
access refuse until reopen decodes the actual durable authority. An exact unchanged
retry emits no RACF write. The unchanged provider 16 MiB envelope limit and original
64 MiB composition budget apply; raw current and prepared authority bytes are charged
within a finite 32 MiB admission. These byte bounds are not aggregate RSS assertions.

## Verification and scope

Seven proposed real Memory/SQLite controls cover complete snapshot/Ready reopen,
actual authorization audits with stable projection retry, signed declaration and
section refusals, foreign selected ownership after ACL emptying, monotonic upgrade
and rollback preserving unrelated runtime fields, genuine capacity/overflow and
before/after unknown outcomes, wrong-store acknowledgement, and SQLite atomic
companion-conflict rollback. All original ten composition controls remain unchanged.
The new controls are unrun in the external draft.

This establishes a finite local participant implementation. It supplies no CD009,
CD016, Foundation, IBM semantic, licensed execution or vendor-equivalence credit.
The original full package/V3/seed/reopen/rollback/raw-oracle requirements remain.
