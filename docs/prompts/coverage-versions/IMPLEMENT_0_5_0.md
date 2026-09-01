# Execution Prompt — Implement mainframe-env 0.5.0

Target version: **0.5.0**
Completion dependencies: 0.2.0

Use this prompt from the repository root. The
[common execution contract](README.md#common-execution-contract) is normative.

---

You are implementing **mainframe-env 0.5.0: complete RACF command language and
SAF**. Deliver one generic security authority for the pinned RACF/SAF surface.

## Read and verify first

Read `docs/prompts/coverage-versions/README.md`,
`docs/delivery/coverage-versions/0.5.0.md`, the official RACF/SAF catalogs,
security model, principal/capability contracts, audit/redaction policy, store
schemas, and relevant ADRs. Verify the 0.2 generated identity, package, coverage,
and evidence contracts before integrating public commands or SAF calls.

## Implement in this order

1. Implement **SEC-501** and freeze principal, ACEE, token, profile template,
   segment, class, access, decision, reason, certificate/key reference, audit,
   transaction, recovery, and migration schemas.
2. Implement **SEC-502/SEC-503**: the generated RACF command parser/validator,
   all user/group/resource command handlers, SETROPTS, class activation,
   RACLIST/cache and policy with atomic mutation and exact diagnostics.
3. Implement **SEC-504**: all 14 pinned RACROUTE request state machines, generic
   class/profile resolution, ACEE/tokens, caching/invalidation and delegation.
4. Implement **SEC-505** certificate/keyring, password/phrase, MFA, identity
   mapping and RRSF behavior through safe secret references.
5. Implement **SEC-506** audit/redaction, concurrency, restart, migration,
   recovery, independent bounded reference simulation, and the fail-closed
   licensed differential adapter.

## Reuse and architecture guardrails

- RACF/SAF profile resolution, access decisions, return/reason codes, ACEE,
  RACLIST, SETROPTS, cache, delegation, and audit semantics remain one owned
  security authority. OPA, Cedar, Casbin, or another generic policy engine may
  be an optional custom-policy adapter, never RACF compatibility authority.
- Reuse reviewed password hashing, TLS, X.509, signature, secret-wrapper, and
  zeroization libraries. Do not implement cryptographic algorithms, ASN.1/X.509
  parsing, or secret-memory redaction formats in the provider.
- Keep plaintext credentials, keys, certificates, and tokens inside bounded
  provider scopes. Third-party secret/crypto types cannot enter checkpoints,
  evidence, public DTOs, or logs; convert them to owned redacted outcomes.
- Extend the shared schema/catalog compiler, store/migration runtime, principal
  contract, audit envelope, and failure harness. Do not create separate security
  schema, migration, transaction, or evidence engines.

## Version-specific invariants

- Default deny at every protected entry point; unavailable policy or store state
  cannot become allow.
- Exactly one authority owns each profile, token, credential, decision, cache,
  and audit mutation. Reads cannot bypass it through provider-local shortcuts.
- Application packages may declare resources and grants but cannot embed trusted
  principals, plaintext secrets, or unconditional authorization decisions.
- Sensitive values never enter logs/evidence; tests prove redaction and denial.
- Command syntax coverage is separate from SAF execution and recovery coverage.

## Completion gate

Do not finish until 34/34 RACF command families and 14/14 RACROUTE requests pass
recognition, validation, execution, condition, authorization, atomicity,
concurrency, restart/recovery, malformed, limit, and audit matrices. The
independent bounded reference simulation must pass its catalog, precedence,
state, status-code, redaction, invariant, metamorphic, and mutant suites without
reusing production implementation. ACEE/token/policy/certificate behavior and
prior CardDemo security journeys must be exact without application-specific
branches.

Scoped completion policy approved by the user on 2026-09-01: 0.5 may exit as
`pass-with-licensed-differential-pending` because no licensed z/OS 3.2 RACF/SAF
receipt is available in this development cycle. Keep the licensed differential
numerator exactly 0/48, never fabricate or infer a pass, and retain the
fail-closed campaign adapter. The real licensed 48-row campaign is a hard gate
of 0.17 `release-certify` and remains mandatory before 1.0/release
certification.

At handoff, include per-family gate counts, deny-path evidence, audit/redaction
checks, migration/rollback and recovery results, reference-simulation results,
the explicit licensed-differential pending reason, and the exact candidate
identity. Do not expose secrets or tag/publish the release.
