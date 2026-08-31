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
   recovery and licensed differential suites.

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
concurrency, restart/recovery, malformed, limit, audit, and licensed differential
matrices. ACEE/token/policy/certificate behavior and prior CardDemo security
journeys must be exact without application-specific branches.

At handoff, include per-family gate counts, deny-path evidence, audit/redaction
checks, migration/rollback and recovery results, oracle receipts, and the exact
candidate identity. Do not expose secrets or tag/publish the release.
